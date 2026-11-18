//! Two-stage qualification for a fresh, disposable P028 store. Source runtime
//! and pin resolution always use the original, read-only application paths.
use crate::{
    FilesystemIdentity, InitializationCoordinator, InitializationObservation, InitializationStart,
    PrefixError, PrefixId, PrefixInitializationBlocker, PrefixInitializationPlan,
    PrefixRuntimeAssociation, PrefixScope, PrefixStore, RuntimeSelectionSource, backup,
    initialization_coordinator::AuthenticQualificationAuthorization,
    wine_artifact_profile::{ArtifactProfileBlocker, VerifiedProfile},
    wine_host_abi::{self, HostAbiEvidence, HostAbiState},
    wine_qualification::{self, trusted_directory},
};
use endfield_unix_game_launch::linux_executor::{CancellationToken, ExpectedExecutableIdentity};
use endfield_unix_runtime_selection::{self as pins, PinnedRuntimeReference};
use endfield_unix_storage::AppPaths;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{self, DirBuilder, File, OpenOptions},
    io::{self, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;

const PREFIX: &str = "authentic-disposable";
const INVOCATION: &str = "wine10-headless-wineboot-init-v1";
const ACCEPT: &str = "I_ACCEPT_DISPOSABLE_WINE_EXECUTION";
#[cfg(not(test))]
const EVIDENCE_SCOPE: &str = "authentic_disposable_wine_qualification";
#[cfg(test)]
const EVIDENCE_SCOPE: &str = "synthetic_native_fixture";
const DIRECT_TIMEOUT: Duration = Duration::from_secs(60);
#[cfg(not(test))]
const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(3);
const DIRECTORIES: &[&str] = &[
    "home", "config", "data", "cache", "runtime", "tmp", "records", "evidence", "logs",
];
const ASSUMPTIONS: &[&str] = &[
    "Headless environment with no inherited desktop, D-Bus, Wine or loader overrides",
    "Private XDG runtime directory has no desktop or session bus sockets",
    "Wine 10.0 may create /tmp/.wine-UID/server-DEVICE-INODE outside the XDG tree",
    "glibc cache, hwcaps, NSS and runtime loading are subject to the documented host trust assumptions",
    "The XDG environment is not a filesystem, network or process namespace",
    "The bounded side-effect observation covers the disposable tree and the exact Wine server directory",
];

#[derive(Debug, Error)]
pub enum QualificationBlocker {
    #[error("P027 pin, adapter, digest, or registry revision does not match")]
    PinMismatch,
    #[error("P025/P026/P027 provenance or integrity check failed: {0}")]
    Runtime(&'static str),
    #[error(transparent)]
    Profile(#[from] ArtifactProfileBlocker),
    #[error("host ABI preflight blocked: {0:?}")]
    Host(HostAbiEvidence),
    #[error("test root or environment is unsafe or has changed")]
    UnsafeEnvironment,
    #[error("P029 recovery is not known clear")]
    RecoveryRequired,
    #[error("P028 prefix plan is blocked")]
    PrefixIneligible,
    #[error("P028 prefix plan is blocked: {0:?}")]
    PrefixPlanBlocked(Vec<PrefixInitializationBlocker>),
    #[error("separate opt-in and exact reviewed qualification plan ID are required")]
    ApprovalRequired,
    #[error("the approved plan or its evidence has changed")]
    PlanChanged,
    #[error(
        "execution intent already exists; reconcile preserved evidence before further attempts"
    )]
    AttemptExists,
    #[error("qualification I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Prefix(#[from] PrefixError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Clone)]
pub struct QualificationRequest {
    pub source_paths: AppPaths,
    pub pin_id: String,
    pub pin_revision: u64,
    pub artifact_sha256: String,
    pub test_root: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QualificationResult {
    PreflightBlocked,
    AwaitingApproval,
    ExecutionStarted,
    Inconclusive,
    ExecutionFailed,
    VerificationFailed,
    Qualified,
}

/// Read-only status of one explicitly selected journal. Even a complete
/// recorded outcome does not grant production authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordedQualificationStatus {
    NoAttempt,
    Inconclusive,
    OutcomeRecorded,
    Stale,
    Unreadable,
}

#[derive(Serialize)]
struct Review<'a> {
    schema_version: u32,
    evidence_scope: &'static str,
    invocation_id: &'static str,
    native_arguments: [&'static str; 3],
    runtime: &'a PinnedRuntimeReference,
    artifact_profile_id: &'a str,
    baseline_id: &'a str,
    wine_inf_mtime: u32,
    executable_sha256: &'a str,
    executable_identity: [u64; 3],
    host_abi: &'a HostAbiEvidence,
    test_root: &'a Path,
    test_directory_identities: &'a [FilesystemIdentity],
    environment: Vec<(OsString, OsString)>,
    prefix_plan: &'a PrefixInitializationPlan,
    expected_server_directory: &'a Path,
    environment_assumptions: &'static [&'static str],
}
#[derive(Serialize)]
struct PlanDocument<'a> {
    plan_id: &'a str,
    review: &'a Review<'a>,
}

impl QualificationRequest {
    pub fn recorded_status(&self) -> RecordedQualificationStatus {
        use RecordedQualificationStatus as Status;
        let Ok(environment) = IsolatedEnvironment::open(self) else {
            return Status::Unreadable;
        };
        let attempt_path = environment.root.join("records/attempt.json");
        if require_absent(&attempt_path).is_ok() {
            return Status::NoAttempt;
        }
        let Some(read_attempt) = read_value(&environment.root.join("records"), "attempt.json")
        else {
            return Status::Inconclusive;
        };
        let Some(outcome) = read_value(&environment.root.join("evidence"), "result.json") else {
            return Status::Inconclusive;
        };
        let Some(plan) = read_value(&environment.root.join("records"), "plan.json") else {
            return Status::Inconclusive;
        };
        let Some(approval) = read_attempt["approval_id"].as_str() else {
            return Status::Inconclusive;
        };
        let review = &plan["review"];
        let Ok(review_id) = review_id(review) else {
            return Status::Inconclusive;
        };
        if review_id != approval
            || outcome["approval_id"] != approval
            || plan["plan_id"] != approval
            || read_attempt["attempt_id"] != format!("{approval}:1")
            || outcome["attempt_id"] != read_attempt["attempt_id"]
            || outcome["plan"] != plan
            || outcome["scope"] != EVIDENCE_SCOPE
            || read_attempt["scope"] != EVIDENCE_SCOPE
            || outcome["schema_version"] != 1
            || read_attempt["schema_version"] != 1
            || !matches!(
                outcome["state"].as_str(),
                Some("INCONCLUSIVE" | "EXECUTION_FAILED" | "VERIFICATION_FAILED" | "QUALIFIED")
            )
        {
            return Status::Inconclusive;
        }
        let (Ok(pin), Ok(identities)) = (self.pin(), serde_json::to_value(&environment.identities))
        else {
            return Status::Stale;
        };
        let Ok(preflight) = self.preflight(&pin.runtime) else {
            return Status::Stale;
        };
        if review["runtime"] != serde_json::to_value(&pin.runtime).unwrap_or_default()
            || review["host_abi"] != serde_json::to_value(&preflight.host).unwrap_or_default()
            || review["test_directory_identities"] != identities
            || review["test_root"].as_str() != self.test_root.to_str()
            || review["artifact_profile_id"] != preflight.profile.id
            || review["baseline_id"] != preflight.profile.baseline_id
            || review["wine_inf_mtime"] != preflight.profile.wine_inf_mtime
            || review["prefix_plan"]["runtime_pin_id"] != self.pin_id
            || review["prefix_plan"]["runtime_pin_revision"] != self.pin_revision
            || review["executable_sha256"] != preflight.verified.sha256()
            || review["executable_identity"]
                != serde_json::json!([
                    preflight.expected.device,
                    preflight.expected.inode,
                    preflight.expected.size
                ])
        {
            return Status::Stale;
        }
        if outcome["state"] == "INCONCLUSIVE" {
            Status::Inconclusive
        } else {
            Status::OutcomeRecorded
        }
    }

    /// Stage one never executes. The caller supplies a new path under a
    /// private parent and keeps all resulting files, even on failure.
    pub fn prepare(&self) -> Result<String, QualificationBlocker> {
        let environment = IsolatedEnvironment::create(self)?;
        let result = self.prepare_inner(&environment);
        let report = match self.pin() {
            Ok(pin) => wine_qualification::qualify_managed_wine(
                &self.source_paths,
                &self.pin_id,
                self.pin_revision,
                &pin.runtime,
            ),
            Err(_) => {
                wine_qualification::WineRuntimeQualification::unavailable("p027_pin_mismatch")
            }
        };
        let (status, id, blocker) = match &result {
            Ok(id) => (QualificationResult::AwaitingApproval, Some(id), None),
            Err(error) => (
                QualificationResult::PreflightBlocked,
                None,
                Some(error.to_string()),
            ),
        };
        write_json(
            &environment.root.join("evidence/preflight.json"),
            &serde_json::json!({
                "schema_version": 1, "scope": EVIDENCE_SCOPE, "timestamp_unix_seconds": crate::now(),
                "state": status, "actual_wine_execution": "NOT_RUN", "plan_id": id,
                "qualification": report, "blocker": blocker,
            }),
        )?;
        result
    }

    fn prepare_inner(
        &self,
        environment: &IsolatedEnvironment,
    ) -> Result<String, QualificationBlocker> {
        let pin = self.pin()?;
        let preflight = self.preflight(&pin.runtime)?;
        let store = environment.store();
        let id = PrefixId::new(PREFIX)?;
        let (scope, association) = association(pin, self.pin_revision);
        let (_, revision) = store.create(id, scope, association.clone(), 0)?;
        let context = self.context(
            environment.clone(),
            &store,
            association,
            &preflight,
            revision,
        )?;
        let review = context.review(&preflight);
        let identifier = review_id(&review)?;
        write_json(
            &environment.root.join("records/plan.json"),
            &PlanDocument {
                plan_id: &identifier,
                review: &review,
            },
        )?;
        Ok(identifier)
    }

    /// Stage two never creates a new plan or retries a consumed attempt.
    pub fn execute(
        &self,
        opt_in: &str,
        reviewed_id: &str,
    ) -> Result<QualificationResult, QualificationBlocker> {
        if opt_in != ACCEPT
            || !reviewed_id
                .strip_prefix("sha256:")
                .is_some_and(crate::is_sha256)
        {
            return Err(QualificationBlocker::ApprovalRequired);
        }
        let environment = IsolatedEnvironment::open(self)?;
        let attempt_path = environment.root.join("records/attempt.json");
        require_absent(&attempt_path).map_err(|_| QualificationBlocker::AttemptExists)?;
        let preflight_document = read_value(&environment.root.join("evidence"), "preflight.json")
            .ok_or(QualificationBlocker::PlanChanged)?;
        if preflight_document["plan_id"] != reviewed_id
            || preflight_document["scope"] != EVIDENCE_SCOPE
            || preflight_document["state"] != "AWAITING_APPROVAL"
        {
            return Err(QualificationBlocker::PlanChanged);
        }
        let prepared = read_record(&environment.root.join("records"), "plan.json")?;
        let pin = self.pin()?;
        let preflight = self.preflight(&pin.runtime)?;
        let store = environment.store();
        let (_, association) = association(pin, self.pin_revision);
        let (registry, records) = store.list(|_| Ok(()))?;
        if records.len() != 1 || records[0].record.id.as_str() != PREFIX {
            return Err(QualificationBlocker::PrefixIneligible);
        }
        let context = Arc::new(self.context(
            environment,
            &store,
            association,
            &preflight,
            registry.revision,
        )?);
        let review = context.review(&preflight);
        let current_id = review_id(&review)?;
        let current_document = serde_json::to_vec_pretty(&PlanDocument {
            plan_id: &current_id,
            review: &review,
        })?;
        if reviewed_id != current_id || prepared != current_document {
            return Err(QualificationBlocker::PlanChanged);
        }
        // The write-once intent is durable before P028 Starting or spawn. A
        // crash with a missing or torn result retains the consumed approval.
        let attempt_id = format!("{}:1", current_id);
        write_json(&attempt_path, &serde_json::json!({
            "schema_version": 1, "scope": EVIDENCE_SCOPE, "state": QualificationResult::ExecutionStarted,
            "outcome_without_final_evidence": QualificationResult::Inconclusive,
            "timestamp_unix_seconds": crate::now(), "approval_id": current_id, "attempt_id": attempt_id,
            "p028_plan_id": context.plan.plan_id,
        })).map_err(|error| match error {
            QualificationBlocker::Io(e) if e.kind() == io::ErrorKind::AlreadyExists => QualificationBlocker::AttemptExists,
            other => other,
        })?;
        let authorization =
            AuthenticQualificationAuthorization::bind(context.clone(), preflight.verified)?;
        let coordinator = InitializationCoordinator::new(store);
        let cancellation = CancellationToken::default();
        let mut state = QualificationResult::Inconclusive;
        let mut process = serde_json::Value::Null;
        let mut prefix_verification = serde_json::Value::Null;
        let mut quiescence = "QUIESCENCE_UNVERIFIED";
        let mut blockers = Vec::new();
        match coordinator.start_authentic(&context.plan, authorization, &cancellation) {
            Ok(InitializationStart::Running(mut attempt)) => {
                let child = attempt.attempt_id().expect("P028 Running owns a child");
                process =
                    serde_json::json!({"pid": child.process_id(), "sequence": child.sequence()});
                let result = match attempt.observe(DIRECT_TIMEOUT) {
                    Ok(InitializationObservation::Running { .. }) => {
                        blockers.push("direct child observation deadline expired".into());
                        attempt.cancel()
                    }
                    result => result,
                };
                let (observed, helper_complete) = attempt.qualification_observations();
                if helper_complete {
                    quiescence = "OBSERVED";
                }
                if let Some(output) = observed {
                    write_new(
                        &context.environment.root.join("logs/stdout.log"),
                        &output.stdout.bytes,
                    )?;
                    write_new(
                        &context.environment.root.join("logs/stderr.log"),
                        &output.stderr.bytes,
                    )?;
                    process["exit_code"] = output.status.code().into();
                    process["stdout_truncated"] = output.stdout.truncated.into();
                    process["stderr_truncated"] = output.stderr.truncated.into();
                    process["stdout_complete"] = output.stdout.complete.into();
                    process["stderr_complete"] = output.stderr.complete.into();
                }
                match result {
                    Ok(InitializationObservation::Initialized { record, .. }) => {
                        prefix_verification = serde_json::json!({
                            "p028_structure": record.initialization_evidence,
                            "wine_inf_update_timestamp_matches": true,
                        });
                        if blockers.is_empty() {
                            state = QualificationResult::Qualified;
                        }
                    }
                    Ok(InitializationObservation::Failed { .. }) => {
                        state = QualificationResult::ExecutionFailed
                    }
                    Ok(InitializationObservation::VerificationPending { error, .. }) => {
                        if helper_complete {
                            state = QualificationResult::VerificationFailed;
                        }
                        blockers.push(error.to_string());
                    }
                    Ok(_) => blockers.push("P028 remains incomplete or interrupted".into()),
                    Err(error) => blockers.push(error.to_string()),
                }
            }
            Ok(InitializationStart::RunningPublicationFailed { attempt, error }) => {
                let child = attempt.attempt_id().expect("created child is owned");
                process =
                    serde_json::json!({"pid": child.process_id(), "sequence": child.sequence()});
                blockers.push(format!("Running publication failed: {error}"));
                if let Err(error) = attempt.abandon_unjournaled() {
                    blockers.push(error.to_string());
                }
            }
            Err(error) => blockers.push(error.to_string()),
        }
        write_json(
            &context.environment.root.join("evidence/result.json"),
            &serde_json::json!({
                "schema_version": 1, "scope": EVIDENCE_SCOPE, "state": state, "attempt_id": attempt_id, "approval_id": current_id,
                "timestamp_unix_seconds": crate::now(), "process_outcome": process,
                "prefix_verification": prefix_verification, "helper_completion": quiescence,
                "side_effects": context.side_effects(), "blockers": blockers,
                "plan": serde_json::from_slice::<serde_json::Value>(&current_document)?,
            }),
        )?;
        Ok(state)
    }

    fn pin(&self) -> Result<pins::RuntimePin, QualificationBlocker> {
        let registry = pins::PinStore::for_app(&self.source_paths)
            .load_read_only()
            .map_err(|_| QualificationBlocker::PinMismatch)?;
        if registry.revision != self.pin_revision {
            return Err(QualificationBlocker::PinMismatch);
        }
        registry
            .pins
            .into_iter()
            .find(|pin| {
                pin.pin_id == self.pin_id
                    && pin.adapter_preference == Some(pins::AdapterPreference::ManagedWine)
                    && matches!(&pin.runtime, PinnedRuntimeReference::Managed(runtime)
                    if runtime.artifact_sha256 == self.artifact_sha256)
            })
            .ok_or(QualificationBlocker::PinMismatch)
    }

    fn preflight(
        &self,
        reference: &PinnedRuntimeReference,
    ) -> Result<Preflight, QualificationBlocker> {
        let PinnedRuntimeReference::Managed(runtime) = reference else {
            return Err(QualificationBlocker::PinMismatch);
        };
        reference
            .validate()
            .map_err(|_| QualificationBlocker::PinMismatch)?;
        let path = self
            .source_paths
            .data_dir()
            .join("runtimes/wine")
            .join(&runtime.installation_id);
        let root = trusted_directory(&path)?;
        let file = root
            .open_regular(Path::new("bin/wine"))
            .map_err(|_| QualificationBlocker::Runtime("verified launcher unavailable"))?;
        let metadata = file.metadata()?;
        let expected = ExpectedExecutableIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.len(),
        };
        let revalidate = || {
            crate::verify_managed_wine_executable(
                &self.source_paths,
                &self.pin_id,
                self.pin_revision,
                reference,
                expected,
            )
            .map_err(QualificationBlocker::Runtime)
        };
        revalidate()?;
        let baseline = endfield_unix_runtime_audit::load_baseline(&path)
            .map_err(|_| ArtifactProfileBlocker::BaselineMismatch)?;
        let profile = VerifiedProfile::load(&self.source_paths, runtime, &root, &baseline)?;
        let host = wine_host_abi::inspect(&root, &baseline, &profile.profile);
        if host.state != HostAbiState::SupportedUnderHostAssumptions {
            return Err(QualificationBlocker::Host(host));
        }
        let verified = revalidate()?;
        if verified.executable.root_identity() != root.identity()
            || !endfield_unix_runtime_audit::load_baseline(&path)
                .is_ok_and(|observed| observed == baseline)
        {
            return Err(ArtifactProfileBlocker::BaselineMismatch.into());
        }
        Ok(Preflight {
            verified,
            expected,
            profile,
            host,
        })
    }

    fn context(
        &self,
        environment: IsolatedEnvironment,
        store: &PrefixStore,
        association: PrefixRuntimeAssociation,
        preflight: &Preflight,
        revision: u64,
    ) -> Result<ExecutionContext, QualificationBlocker> {
        let id = PrefixId::new(PREFIX)?;
        let recovery = recovery_pending(&self.source_paths, &environment, &id, &association)?;
        let plan = store.plan_initialization(
            &id,
            revision,
            &association,
            Some(recovery),
            |reference| {
                if reference == &association.runtime {
                    Ok(())
                } else {
                    Err("runtime association changed".into())
                }
            },
        )?;
        if recovery {
            return Err(QualificationBlocker::RecoveryRequired);
        }
        if !plan.otherwise_ready_for_scoped_initialization() {
            return Err(QualificationBlocker::PrefixPlanBlocked(plan.blockers));
        }
        let prefix = store.target_path(&id);
        let identity = plan
            .filesystem_identity
            .ok_or(QualificationBlocker::PrefixIneligible)?;
        #[cfg(not(test))]
        let server = ServerDirectory::new(identity)?;
        #[cfg(test)]
        let server = ServerDirectory::under(&environment.root.join("tmp"), identity);
        server.check_unused()?;
        environment.validate_before_spawn()?;
        Ok(ExecutionContext {
            request: self.clone(),
            environment,
            prefix,
            association,
            plan,
            server,
            expected: preflight.expected,
            artifact_profile_id: preflight.profile.id.clone(),
            baseline_id: preflight.profile.baseline_id.clone(),
            wine_inf_mtime: preflight.profile.wine_inf_mtime,
            host: preflight.host.clone(),
        })
    }
}

struct Preflight {
    verified: crate::VerifiedManagedWineExecutable,
    expected: ExpectedExecutableIdentity,
    profile: VerifiedProfile,
    host: HostAbiEvidence,
}

pub(crate) struct ExecutionContext {
    request: QualificationRequest,
    environment: IsolatedEnvironment,
    pub(crate) prefix: PathBuf,
    pub(crate) association: PrefixRuntimeAssociation,
    pub(crate) plan: PrefixInitializationPlan,
    server: ServerDirectory,
    expected: ExpectedExecutableIdentity,
    artifact_profile_id: String,
    baseline_id: String,
    wine_inf_mtime: u32,
    host: HostAbiEvidence,
}
impl ExecutionContext {
    fn review<'a>(&'a self, preflight: &'a Preflight) -> Review<'a> {
        Review {
            schema_version: 1,
            evidence_scope: EVIDENCE_SCOPE,
            invocation_id: INVOCATION,
            native_arguments: ["wine", "wineboot.exe", "--init"],
            runtime: &self.association.runtime,
            artifact_profile_id: &self.artifact_profile_id,
            baseline_id: &self.baseline_id,
            wine_inf_mtime: self.wine_inf_mtime,
            executable_sha256: preflight.verified.sha256(),
            executable_identity: [
                self.expected.device,
                self.expected.inode,
                self.expected.size,
            ],
            host_abi: &self.host,
            test_root: &self.environment.root,
            test_directory_identities: &self.environment.identities,
            environment: self.child_environment(),
            prefix_plan: &self.plan,
            expected_server_directory: &self.server.path,
            environment_assumptions: ASSUMPTIONS,
        }
    }

    pub(crate) fn child_environment(&self) -> Vec<(OsString, OsString)> {
        self.environment.variables(&self.prefix)
    }

    pub(crate) fn matches_environment(&self, prefix: &Path, vars: &[(OsString, OsString)]) -> bool {
        self.prefix == prefix
            && vars == self.child_environment()
            && self.environment.validate_before_spawn().is_ok()
            && self.server.check_unused().is_ok()
    }

    pub(crate) fn recovery_pending(&self) -> Option<bool> {
        recovery_pending(
            &self.request.source_paths,
            &self.environment,
            &PrefixId::new(PREFIX).ok()?,
            &self.association,
        )
        .ok()
    }

    pub(crate) fn revalidate(
        &self,
        association: &PrefixRuntimeAssociation,
    ) -> Result<(), QualificationBlocker> {
        if association != &self.association {
            return Err(QualificationBlocker::PinMismatch);
        }
        let current = self.request.preflight(&association.runtime)?;
        if current.expected != self.expected
            || current.profile.id != self.artifact_profile_id
            || current.profile.baseline_id != self.baseline_id
            || current.profile.wine_inf_mtime != self.wine_inf_mtime
            || current.host != self.host
            || self.environment.validate_identity().is_err()
        {
            return Err(QualificationBlocker::PlanChanged);
        }
        Ok(())
    }

    pub(crate) fn verify_install_marker(&self) -> bool {
        let Ok(root) = trusted_directory(&self.prefix) else {
            return false;
        };
        let Some(identity) = self.plan.filesystem_identity else {
            return false;
        };
        if root.identity().device != identity.device || root.identity().inode != identity.inode {
            return false;
        }
        let Ok(marker) = root.open_regular(Path::new(".update-timestamp")) else {
            return false;
        };
        marker
            .metadata()
            .is_ok_and(|metadata| metadata.nlink() == 1)
            && wine_qualification::bounded_bytes(marker, &mut 64)
                .is_ok_and(|bytes| bytes == format!("{}\n", self.wine_inf_mtime).into_bytes())
    }

    pub(crate) fn wait_for_quiescence(&self, store: &PrefixStore) -> bool {
        let Ok(id) = PrefixId::new(PREFIX) else {
            return false;
        };
        let deadline = Instant::now() + QUIESCENCE_TIMEOUT;
        let mut previous = None;
        while Instant::now() < deadline {
            match (self.server.idle_lock(), store.fs.prefix_in_use(&id)) {
                (Ok(Some(lock)), Ok(Some(false))) if previous == Some(lock) => return true,
                (Ok(Some(lock)), Ok(Some(false))) => previous = Some(lock),
                (Ok(_), _) => previous = None,
                _ => return false,
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        false
    }

    fn side_effects(&self) -> serde_json::Value {
        let observations: Vec<_> = DIRECTORIES
            .iter()
            .map(|name| {
                let mut observation = observe_directory(&self.environment.root.join(name));
                observation["directory"] = (*name).into();
                observation
            })
            .collect();
        serde_json::json!({
            "disposable_directories": observations,
            "managed_test_prefix": observe_directory(&self.prefix),
            "wine_server_directory": observe_directory(&self.server.path),
            "monitoring_boundary": ASSUMPTIONS[5],
        })
    }
}

fn observe_directory(path: &Path) -> serde_json::Value {
    const MAX_NAMES: usize = 128;
    if trusted_directory(path).is_err() {
        return serde_json::json!({"observed": false, "present": path.symlink_metadata().is_ok()});
    }
    let Ok(entries) = fs::read_dir(path) else {
        return serde_json::json!({"observed": false, "present": true});
    };
    let mut names = Vec::new();
    let mut errors = 0;
    let mut truncated = false;
    for (index, entry) in entries.take(MAX_NAMES + 1).enumerate() {
        if index == MAX_NAMES {
            truncated = true;
            break;
        }
        match entry {
            Ok(entry) => names.push(entry.file_name().to_string_lossy().into_owned()),
            Err(_) => errors += 1,
        }
    }
    names.sort();
    serde_json::json!({"observed": true, "present": true, "immediate_entries": names,
        "truncated": truncated, "unreadable_entries": errors})
}

fn recovery_pending(
    source: &AppPaths,
    env: &IsolatedEnvironment,
    id: &PrefixId,
    association: &PrefixRuntimeAssociation,
) -> Result<bool, QualificationBlocker> {
    use backup::{CompatibilityRecoveryState, PinRecoveryState};
    let local = backup::BackupStore::for_app(&env.app_paths)
        .compatibility_recovery_observation(id)
        .map_err(|_| QualificationBlocker::RecoveryRequired)?;
    let inspection = env.store().show(id, |_| Ok(()))?;
    let source_recovery = backup::BackupStore::for_app(source)
        .pin_recoveries()
        .map_err(|_| QualificationBlocker::RecoveryRequired)?;
    Ok(
        !matches!(local.restore_state, CompatibilityRecoveryState::None)
            || local.pending_deletion
            || local.pending_pin_recovery
            || inspection.record.runtime != *association
            || source_recovery.iter().any(|entry| {
                entry.state == PinRecoveryState::Pending && entry.scope == inspection.record.scope
            }),
    )
}

fn association(pin: pins::RuntimePin, revision: u64) -> (PrefixScope, PrefixRuntimeAssociation) {
    let (scope, selection_source) = match pin.scope {
        pins::RuntimePinScope::Machine { machine } => (
            PrefixScope::machine(machine),
            RuntimeSelectionSource::MachinePin,
        ),
        pins::RuntimePinScope::Profile { machine, profile } => (
            PrefixScope::profile(machine, profile),
            RuntimeSelectionSource::ProfilePin,
        ),
    };
    (
        scope,
        PrefixRuntimeAssociation {
            runtime: pin.runtime,
            pin_id: pin.pin_id,
            pin_revision: revision,
            selection_source,
            adapter_preference: pin.adapter_preference,
        },
    )
}

fn review_id(review: &impl Serialize) -> Result<String, serde_json::Error> {
    let sorted = serde_json::to_value(review)?;
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&sorted)?)
    ))
}

#[derive(Clone)]
struct IsolatedEnvironment {
    root: PathBuf,
    identities: Vec<FilesystemIdentity>,
    app_paths: AppPaths,
}

impl IsolatedEnvironment {
    fn create(request: &QualificationRequest) -> Result<Self, QualificationBlocker> {
        Self::validate_path(request)?;
        private_create(&request.test_root)?;
        for name in DIRECTORIES {
            private_create(&request.test_root.join(name))?;
        }
        #[cfg(test)]
        private_create(&request.test_root.join("records/process-table-fixture"))?;
        File::open(&request.test_root)?.sync_all()?;
        File::open(request.test_root.parent().expect("checked parent"))?.sync_all()?;
        Self::open(request)
    }

    fn open(request: &QualificationRequest) -> Result<Self, QualificationBlocker> {
        Self::validate_path(request)?;
        let root = request.test_root.clone();
        let mut identities = vec![private_directory(&root)?];
        for name in DIRECTORIES {
            identities.push(private_directory(&root.join(name))?);
        }
        let app_paths =
            AppPaths::from_base_dirs(root.join("config"), root.join("data"), root.join("cache"))
                .map_err(|_| QualificationBlocker::UnsafeEnvironment)?;
        Ok(Self {
            root,
            identities,
            app_paths,
        })
    }

    fn validate_path(request: &QualificationRequest) -> Result<(), QualificationBlocker> {
        let root = &request.test_root;
        let parent = root
            .parent()
            .ok_or(QualificationBlocker::UnsafeEnvironment)?;
        let text = root
            .to_str()
            .ok_or(QualificationBlocker::UnsafeEnvironment)?;
        if text.len() > 2048
            || !text.starts_with('/')
            || text[1..].split('/').any(|part| {
                part.is_empty() || matches!(part, "." | "..") || part.chars().any(char::is_control)
            })
            || [
                request.source_paths.data_dir(),
                request.source_paths.cache_dir(),
                request
                    .source_paths
                    .config_file()
                    .parent()
                    .expect("config parent"),
            ]
            .iter()
            .any(|path| root.starts_with(path) || path.starts_with(root))
        {
            return Err(QualificationBlocker::UnsafeEnvironment);
        }
        private_directory(parent)?;
        Ok(())
    }

    fn validate_identity(&self) -> Result<(), QualificationBlocker> {
        if private_directory(&self.root)? != self.identities[0] {
            return Err(QualificationBlocker::UnsafeEnvironment);
        }
        for (index, name) in DIRECTORIES.iter().enumerate() {
            if private_directory(&self.root.join(name))? != self.identities[index + 1] {
                return Err(QualificationBlocker::UnsafeEnvironment);
            }
        }
        Ok(())
    }

    fn validate_before_spawn(&self) -> Result<(), QualificationBlocker> {
        self.validate_identity()?;
        for name in ["home", "config", "cache", "runtime", "tmp", "logs"] {
            if fs::read_dir(self.root.join(name))?.next().is_some() {
                return Err(QualificationBlocker::UnsafeEnvironment);
            }
        }
        Ok(())
    }

    fn store(&self) -> PrefixStore {
        #[cfg(test)]
        {
            // Keep P028's real filesystem and registry checks while isolating
            // native fixture tests from unrelated same-UID processes.
            let filesystem = crate::linux_fs::LinuxPrefixFilesystem::new(self.app_paths.data_dir())
                .with_process_table_for_test(self.root.join("records/process-table-fixture"));
            PrefixStore::with_filesystem(Arc::new(filesystem))
        }
        #[cfg(not(test))]
        PrefixStore::for_app(&self.app_paths)
    }

    fn variables(&self, prefix: &Path) -> Vec<(OsString, OsString)> {
        let mut vars: Vec<_> = [
            ("HOME", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_RUNTIME_DIR", "runtime"),
            ("TMPDIR", "tmp"),
        ]
        .into_iter()
        .map(|(name, dir)| (name.into(), self.root.join(dir).into_os_string()))
        .collect();
        vars.extend([
            ("WINEPREFIX".into(), prefix.as_os_str().to_owned()),
            ("USER".into(), "wine-qualification".into()),
            ("LC_ALL".into(), "C".into()),
        ]);
        vars
    }
}

fn private_directory(path: &Path) -> Result<FilesystemIdentity, QualificationBlocker> {
    let root = trusted_directory(path).map_err(|_| QualificationBlocker::UnsafeEnvironment)?;
    let metadata = fs::symlink_metadata(path)?;
    let id = root.identity();
    if !metadata.is_dir()
        || metadata.mode() & 0o7777 != 0o700
        || (metadata.dev(), metadata.ino(), metadata.uid()) != (id.device, id.inode, id.owner_uid)
    {
        return Err(QualificationBlocker::UnsafeEnvironment);
    }
    Ok(FilesystemIdentity {
        device: id.device,
        inode: id.inode,
    })
}

fn private_create(path: &Path) -> io::Result<()> {
    DirBuilder::new().mode(0o700).create(path)
}

struct ServerDirectory {
    base: PathBuf,
    path: PathBuf,
}
impl ServerDirectory {
    #[cfg(not(test))]
    fn new(prefix: FilesystemIdentity) -> Result<Self, QualificationBlocker> {
        let tmp = fs::symlink_metadata("/tmp")?;
        if !tmp.is_dir() || tmp.uid() != 0 || tmp.mode() & 0o1000 == 0 {
            return Err(QualificationBlocker::UnsafeEnvironment);
        }
        Ok(Self::under(Path::new("/tmp"), prefix))
    }

    fn under(tmp: &Path, prefix: FilesystemIdentity) -> Self {
        let base = tmp.join(format!(".wine-{}", crate::current_uid()));
        let path = base.join(format!("server-{:x}-{:x}", prefix.device, prefix.inode));
        Self { base, path }
    }

    fn check_unused(&self) -> Result<(), QualificationBlocker> {
        if !matches!(self.base.symlink_metadata(), Err(e) if e.kind() == io::ErrorKind::NotFound) {
            private_directory(&self.base)?;
        }
        require_absent(&self.path).map_err(|_| QualificationBlocker::UnsafeEnvironment)
    }

    /// Wine uses a one-byte POSIX write lock on this file. Observe it without
    /// changing it, starting a waiter, connecting to a server or signaling a PID.
    fn idle_lock(&self) -> Result<Option<FilesystemIdentity>, QualificationBlocker> {
        if matches!(self.path.symlink_metadata(), Err(e) if e.kind() == io::ErrorKind::NotFound) {
            return Ok(None);
        }
        private_directory(&self.base)?;
        private_directory(&self.path)?;
        let dir = trusted_directory(&self.path)?;
        let file = dir
            .open_regular(Path::new("lock"))
            .map_err(|_| QualificationBlocker::UnsafeEnvironment)?;
        let meta = file.metadata()?;
        if meta.nlink() != 1 || meta.mode() & 0o7777 != 0o600 {
            return Err(QualificationBlocker::UnsafeEnvironment);
        }
        // SAFETY: zero is valid for flock fields; the kernel initializes the
        // conflicting-lock fields on a successful read-only F_GETLK query.
        let mut lock: libc::flock = unsafe { std::mem::zeroed() };
        lock.l_type = libc::F_WRLCK as _;
        lock.l_whence = libc::SEEK_SET as _;
        lock.l_len = 1;
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETLK, &mut lock) } < 0 {
            return Err(io::Error::last_os_error().into());
        }
        let socket_absent = require_absent(&self.path.join("socket")).is_ok();
        let stable_file = fs::symlink_metadata(self.path.join("lock"))?;
        if (stable_file.dev(), stable_file.ino()) != (meta.dev(), meta.ino()) {
            return Err(QualificationBlocker::UnsafeEnvironment);
        }
        Ok(
            (lock.l_type == libc::F_UNLCK as libc::c_short && socket_absent).then_some(
                FilesystemIdentity {
                    device: meta.dev(),
                    inode: meta.ino(),
                },
            ),
        )
    }
}

fn require_absent(path: &Path) -> io::Result<()> {
    match path.symlink_metadata() {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(_) => Err(io::ErrorKind::AlreadyExists.into()),
    }
}

fn read_value(directory: &Path, name: &str) -> Option<serde_json::Value> {
    serde_json::from_slice(&read_record(directory, name).ok()?).ok()
}

fn read_record(directory: &Path, name: &str) -> Result<Vec<u8>, QualificationBlocker> {
    let root = trusted_directory(directory)?;
    let file = root
        .open_regular(Path::new(name))
        .map_err(|_| QualificationBlocker::PlanChanged)?;
    wine_qualification::bounded_bytes(file, &mut (1024 * 1024))
        .map_err(|_| QualificationBlocker::PlanChanged)
}

fn write_json<T: Serialize>(path: &Path, object: &T) -> Result<(), QualificationBlocker> {
    write_new(path, &serde_json::to_vec_pretty(object)?)?;
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    File::open(path.parent().expect("record parent"))?.sync_all()
}

#[cfg(test)]
#[path = "authentic_qualification_tests.rs"]
mod tests;
