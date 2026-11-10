//! Headless, read-only launch preflight. No operation in this module spawns a process.
use clap::{Subcommand, ValueEnum};
use endfield_unix_game_import::{
    GameInstallationRecord, GameInstallationRegistry, GameInstallationState, RootIdentity,
};
use endfield_unix_game_launch as launch;
use endfield_unix_prefix_management as prefixes;
use endfield_unix_runtime_selection as pins;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path, PathBuf},
};

const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum AdapterArg {
    ManagedWine,
    ExternalWine,
    UmuProton,
    DirectProton,
}

impl From<AdapterArg> for launch::RuntimeAdapter {
    fn from(value: AdapterArg) -> Self {
        match value {
            AdapterArg::ManagedWine => Self::ManagedWine,
            AdapterArg::ExternalWine => Self::ExternalWine,
            AdapterArg::UmuProton => Self::UmuProton,
            AdapterArg::DirectProton => Self::DirectProton,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Subcommand)]
pub enum Operation {
    /// Inspect one exact game, runtime pin, adapter, prefix, and executable candidate.
    Plan {
        #[arg(long)]
        id: String,
        #[arg(long)]
        prefix_id: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        executable_relative: String,
        #[arg(long, value_enum)]
        adapter: AdapterArg,
        #[arg(long)]
        json: bool,
    },
}

impl Operation {
    pub fn json(&self) -> bool {
        match self {
            Self::Plan { json, .. } => *json,
        }
    }
}

#[derive(Serialize)]
struct ErrorReport<'a> {
    schema_version: u32,
    error: ErrorDetail<'a>,
}

#[derive(Serialize)]
struct ErrorDetail<'a> {
    code: &'a str,
    message: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InvocationPreparationState {
    PreparedNonExecutable,
    PreparationOnly,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvocationFileIdentity {
    pub(crate) device: u64,
    pub(crate) inode: u64,
    pub(crate) size: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeInvocationAssessment {
    pub(crate) adapter: launch::RuntimeAdapter,
    pub(crate) state: InvocationPreparationState,
    pub(crate) contract_verified: bool,
    pub(crate) invocation_plan_id: Option<String>,
    pub(crate) expected_executable_identity: Option<InvocationFileIdentity>,
    pub(crate) native_argument_count: Option<usize>,
    pub(crate) process_creation_supported: bool,
    pub(crate) reason: &'static str,
    pub(crate) expected_effects: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecoveryEvidence {
    Clear,
    Pending,
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrefixInitializationAssessment {
    pub(crate) lifecycle: Option<launch::PrefixLifecycle>,
    pub(crate) evidence: Option<prefixes::PrefixInitializationEvidenceStatus>,
    pub(crate) recovery: RecoveryEvidence,
    pub(crate) plan: Option<prefixes::PrefixInitializationPlan>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreflightReport {
    #[serde(flatten)]
    pub(crate) plan: launch::LaunchPlan,
    pub(crate) runtime_invocation: RuntimeInvocationAssessment,
    pub(crate) prefix_initialization: PrefixInitializationAssessment,
}

pub fn run(operation: Operation, output: &mut impl Write) -> io::Result<u8> {
    let Operation::Plan {
        id,
        prefix_id,
        profile,
        executable_relative,
        adapter,
        json,
    } = operation;
    let result = preflight(
        &id,
        &prefix_id,
        profile.as_deref(),
        &executable_relative,
        adapter,
    );
    let mut bytes = Vec::new();
    let code = match result {
        Ok(report) => {
            if json {
                serde_json::to_writer_pretty(&mut bytes, &report)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                bytes.push(b'\n');
            } else {
                render_human(&mut bytes, &report)?;
            }
            if report.plan.is_blocked() { 78 } else { 0 }
        }
        Err(error) => {
            if json {
                serde_json::to_writer_pretty(
                    &mut bytes,
                    &ErrorReport {
                        schema_version: launch::PLAN_SCHEMA_VERSION,
                        error: ErrorDetail {
                            code: error.code,
                            message: &error.message,
                        },
                    },
                )
                .map_err(|serialization| {
                    io::Error::new(io::ErrorKind::InvalidData, serialization)
                })?;
                bytes.push(b'\n');
            } else {
                writeln!(
                    bytes,
                    "Launch preflight failed [{}]: {}",
                    error.code, error.message
                )?;
            }
            error.exit_code
        }
    };
    if bytes.len() > MAX_OUTPUT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "launch plan output exceeded its bound",
        ));
    }
    output.write_all(&bytes)?;
    Ok(code)
}

struct PreflightError {
    code: &'static str,
    message: String,
    exit_code: u8,
}

impl PreflightError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_request",
            message: message.into(),
            exit_code: 2,
        }
    }
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: "preflight_unavailable",
            message: message.into(),
            exit_code: 4,
        }
    }
}

fn preflight(
    installation_id: &str,
    prefix_id: &str,
    profile: Option<&str>,
    executable_relative: &str,
    adapter: AdapterArg,
) -> Result<PreflightReport, PreflightError> {
    let request = launch::GameLaunchRequest::new(
        installation_id,
        prefix_id,
        profile.map(ToOwned::to_owned),
        executable_relative,
        adapter.into(),
    )
    .map_err(|error| PreflightError::invalid(error.to_string()))?;
    let paths = endfield_unix_storage::AppPaths::discover()
        .map_err(|_| PreflightError::unavailable("XDG paths are unavailable"))?;
    let game_registry = GameInstallationRegistry::for_app(&paths);
    let games = game_registry.load_read_only().map_err(|error| {
        PreflightError::unavailable(format!("P031 registry is unavailable: {}", error.code()))
    })?;
    let game = games
        .installations
        .iter()
        .find(|record| record.id.as_str() == installation_id);
    let (game_registered, game_revision, root_matches, exe) = match game {
        Some(record) => {
            let validation = game_registry.validate(&record.id).map_err(|error| {
                PreflightError::unavailable(format!("P031 validation failed: {}", error.code()))
            })?;
            let root_matches = validation.observed_root_identity.as_ref()
                == Some(&record.root_identity)
                && matches!(
                    validation.state,
                    GameInstallationState::Registered
                        | GameInstallationState::StructureObserved
                        | GameInstallationState::IdentityUnverified
                        | GameInstallationState::IdentityVerified
                );
            let evidence = inspect_executable(record, executable_relative, root_matches);
            (
                true,
                Some(validation.registry_revision),
                root_matches,
                evidence,
            )
        }
        None => (
            false,
            Some(games.revision),
            false,
            missing_executable(executable_relative),
        ),
    };

    let (mut runtime_evidence, runtime_observation) =
        inspect_runtime(&paths, profile, adapter.into());
    let prefix_observation =
        inspect_prefix(&paths, prefix_id, profile, runtime_observation.as_ref());
    let runtime_invocation = assess_runtime_invocation(RuntimeInvocationContext {
        paths: &paths,
        adapter: adapter.into(),
        runtime: runtime_observation.as_ref(),
        prefix: &prefix_observation,
        game,
        executable_relative,
        game_registry_revision: game_revision,
        executable: &exe,
    });
    runtime_evidence.invocation_contract_verified = runtime_invocation.contract_verified;
    runtime_evidence.invocation_plan_id = runtime_invocation.invocation_plan_id.clone();
    runtime_evidence.invocation_executable_identity = runtime_invocation
        .expected_executable_identity
        .map(|identity| launch::RuntimeExecutableIdentity {
            device: identity.device,
            inode: identity.inode,
            size: identity.size,
        });
    let prefix = prefix_observation.evidence.clone();
    let config = launch::LaunchConfiguration {
        game_registered,
        game_registry_revision: game_revision,
        game_root_identity_matches: root_matches,
        executable: exe,
        runtime: runtime_evidence,
        prefix,
        // P032/P033 do not establish anti-cheat or Linux gameplay compatibility.
        compatibility: launch::CompatibilityEvidence::Unknown,
        // P033 is merged with acceptance debt; unknown dependencies block.
        critical_dependencies: launch::EvidenceState::Unknown,
        // There is not yet a production adapter that can safely create this process.
        policy: launch::LaunchPolicy::fail_closed(),
    };
    let plan = launch::LaunchPlan::build(&request, &config)
        .map_err(|error| PreflightError::invalid(error.to_string()))?;
    Ok(PreflightReport {
        plan,
        runtime_invocation,
        prefix_initialization: PrefixInitializationAssessment {
            lifecycle: prefix_observation.lifecycle,
            evidence: prefix_observation.initialization_evidence,
            recovery: prefix_observation.recovery,
            plan: prefix_observation.initialization_plan,
        },
    })
}

/// Shared preflight entry point used by the GTK worker. It returns the same
/// plan that the headless CLI serializes and never starts a process.
pub(crate) fn plan_for_ui(
    installation_id: String,
    prefix_id: String,
    profile: Option<String>,
    executable_relative: String,
    adapter: AdapterArg,
) -> Result<PreflightReport, String> {
    preflight(
        &installation_id,
        &prefix_id,
        profile.as_deref(),
        &executable_relative,
        adapter,
    )
    .map_err(|error| format!("{}: {}", error.code, error.message))
}

fn unknown_runtime(adapter: launch::RuntimeAdapter) -> launch::RuntimeSelectionEvidence {
    launch::RuntimeSelectionEvidence {
        state: launch::EvidenceState::Unknown,
        selection_source: None,
        runtime_id: None,
        family: None,
        adapter,
        pin_id: None,
        pin_revision: None,
        runtime_reference_sha256: None,
        runtime_generation: None,
        runtime_ownership_record_sha256: None,
        runtime_artifact_sha256: None,
        managed_ownership_verified: false,
        baseline_integrity_verified: false,
        external_structure_revalidated: false,
        external_trust_policy_documented: false,
        invocation_contract_verified: false,
        invocation_plan_id: None,
        invocation_executable_identity: None,
    }
}

struct RuntimeObservation {
    evidence: launch::RuntimeSelectionEvidence,
    reference: pins::PinnedRuntimeReference,
    pin_adapter: Option<pins::AdapterPreference>,
    source: pins::SelectionSource,
    pin_id: String,
    pin_revision: u64,
}

fn inspect_runtime(
    paths: &endfield_unix_storage::AppPaths,
    profile: Option<&str>,
    selected_adapter: launch::RuntimeAdapter,
) -> (launch::RuntimeSelectionEvidence, Option<RuntimeObservation>) {
    let Ok(machine) = pins::current_machine_id() else {
        return (unknown_runtime(selected_adapter), None);
    };
    let profile = match profile {
        Some(value) => match pins::ProfileKey::new(value.to_owned()) {
            Ok(profile) => Some(profile),
            Err(_) => return (unknown_runtime(selected_adapter), None),
        },
        None => None,
    };
    let Ok(registry) = pins::PinStore::for_app(paths).load_read_only() else {
        return (unknown_runtime(selected_adapter), None);
    };
    let report = pins::resolve(
        &registry,
        &machine,
        profile.as_ref(),
        None,
        super::runtime_pin::validate_reference,
    );
    let state = match &report.state {
        pins::ResolutionState::Selected => launch::EvidenceState::Verified,
        pins::ResolutionState::NoPin => launch::EvidenceState::Missing,
        pins::ResolutionState::Stale(_) => launch::EvidenceState::Stale,
    };
    let Some(reference) = report.runtime.clone() else {
        let mut evidence = unknown_runtime(selected_adapter);
        evidence.state = state;
        evidence.selection_source = Some("none".into());
        return (evidence, None);
    };
    let Some(pin_id) = report.pin_id.clone() else {
        return (unknown_runtime(selected_adapter), None);
    };
    let Some(pin) = registry.pins.iter().find(|pin| pin.pin_id == pin_id) else {
        return (unknown_runtime(selected_adapter), None);
    };
    let family = reference.family().to_owned();
    let runtime_id = match &reference {
        pins::PinnedRuntimeReference::Managed(runtime) => runtime.installation_id.clone(),
        pins::PinnedRuntimeReference::External(_) => format!("external:{}", pin_id),
    };
    let (managed, integrity, external, trust, generation, owner_record, artifact) = match &reference {
        pins::PinnedRuntimeReference::Managed(runtime) => (
            state == launch::EvidenceState::Verified,
            state == launch::EvidenceState::Verified,
            false,
            false,
            Some(runtime.generation),
            Some(runtime.ownership_record_sha256.clone()),
            Some(runtime.artifact_sha256.clone()),
        ),
        pins::PinnedRuntimeReference::External(_) => (
            false,
            false,
            state == launch::EvidenceState::Verified,
            false,
            None,
            None,
            None,
        ),
    };
    let runtime_reference_sha256 = hash_runtime_reference(&reference).ok();
    let preference = pin.adapter_preference;
    let preference_matches = matches!(
        (preference, selected_adapter),
        (
            Some(pins::AdapterPreference::ManagedWine),
            launch::RuntimeAdapter::ManagedWine
        ) | (
            Some(pins::AdapterPreference::ExternalWine),
            launch::RuntimeAdapter::ExternalWine
        ) | (
            Some(pins::AdapterPreference::UmuProton),
            launch::RuntimeAdapter::UmuProton
        )
    );
    let mut evidence = launch::RuntimeSelectionEvidence {
        state,
        selection_source: Some(
            match &report.source {
                pins::SelectionSource::ProfilePin => "profile_pin",
                pins::SelectionSource::MachinePin => "machine_pin",
                pins::SelectionSource::ExplicitOverride => "explicit_override",
                pins::SelectionSource::None => "none",
            }
            .into(),
        ),
        runtime_id: Some(runtime_id),
        family: Some(family),
        adapter: selected_adapter,
        pin_id: Some(pin_id.clone()),
        pin_revision: Some(report.registry_revision),
        runtime_reference_sha256,
        runtime_generation: generation,
        runtime_ownership_record_sha256: owner_record,
        runtime_artifact_sha256: artifact,
        managed_ownership_verified: managed,
        baseline_integrity_verified: integrity,
        external_structure_revalidated: external,
        external_trust_policy_documented: trust,
        // P023 only prepares non-executable UMU plans; no production invoker exists.
        invocation_contract_verified: false,
        invocation_plan_id: None,
        invocation_executable_identity: None,
    };
    if !preference_matches {
        evidence.state = launch::EvidenceState::Unsupported;
    }
    let observation = RuntimeObservation {
        evidence,
        reference,
        pin_adapter: preference,
        source: report.source.clone(),
        pin_id,
        pin_revision: report.registry_revision,
    };
    (observation.evidence.clone(), Some(observation))
}

fn hash_runtime_reference(
    reference: &pins::PinnedRuntimeReference,
) -> Result<String, serde_json::Error> {
    let bytes = serde_json::to_vec(reference)?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

struct RuntimeInvocationContext<'a> {
    paths: &'a endfield_unix_storage::AppPaths,
    adapter: launch::RuntimeAdapter,
    runtime: Option<&'a RuntimeObservation>,
    prefix: &'a PrefixObservation,
    game: Option<&'a GameInstallationRecord>,
    executable_relative: &'a str,
    game_registry_revision: Option<u64>,
    executable: &'a launch::GameExecutableEvidence,
}

fn assess_runtime_invocation(
    context: RuntimeInvocationContext<'_>,
) -> RuntimeInvocationAssessment {
    let RuntimeInvocationContext {
        paths,
        adapter,
        runtime,
        prefix,
        game,
        executable_relative,
        game_registry_revision,
        executable,
    } = context;
    let unsupported = |reason| RuntimeInvocationAssessment {
        adapter,
        state: InvocationPreparationState::Unsupported,
        contract_verified: false,
        invocation_plan_id: None,
        expected_executable_identity: None,
        native_argument_count: None,
        process_creation_supported: false,
        reason,
        expected_effects: vec![],
    };

    match adapter {
        launch::RuntimeAdapter::ManagedWine => {
            let Some(runtime) = runtime else {
                return unsupported("no exact P027 runtime selection is available");
            };
            let pins::PinnedRuntimeReference::Managed(reference) = &runtime.reference else {
                return unsupported("selected runtime is not an Endfield-managed Wine install");
            };
            if reference.family != "wine"
                || runtime.pin_adapter != Some(pins::AdapterPreference::ManagedWine)
                || runtime.evidence.state != launch::EvidenceState::Verified
                || !prefix.evidence.runtime_association_matches
            {
                return unsupported("P027/P028 evidence does not select the same verified managed Wine adapter");
            }
            let identity = match endfield_unix_runtime_install::pin_identity(
                paths,
                &reference.installation_id,
            ) {
                Ok(identity) => identity,
                Err(_) => return unsupported("P025/P026 managed Wine revalidation failed"),
            };
            use endfield_unix_runtime_audit::{Completeness, FindingKind};
            if identity.installation_id != reference.installation_id
                || identity.generation != reference.generation
                || identity.ownership_record_sha256 != reference.ownership_record_sha256
                || identity.audit.completeness != Completeness::Complete
                || identity
                    .audit
                    .findings
                    .iter()
                    .any(|finding| finding.kind != FindingKind::Match)
            {
                return unsupported("P025 ownership or P026 baseline evidence changed");
            }
            let runtime_audit_report_sha256 = match serde_json::to_vec(&identity.audit) {
                Ok(bytes) => format!("{:x}", Sha256::digest(bytes)),
                Err(_) => return unsupported("P026 audit evidence could not be bound to the plan"),
            };
            let (Some(prefix_record), Some(prefix_revision)) =
                (prefix.record.as_ref(), prefix.registry_revision)
            else {
                return unsupported("managed prefix record is unavailable");
            };
            let Some(prefix_identity) = prefix_record.filesystem_identity else {
                return unsupported("managed prefix filesystem identity is unknown");
            };
            let runtime_reference_sha256 = match hash_runtime_reference(&runtime.reference) {
                Ok(digest) => digest,
                Err(_) => return unsupported("selected runtime reference could not be hashed"),
            };
            let prefix_runtime_reference_sha256 =
                match hash_runtime_reference(&prefix_record.runtime.runtime) {
                    Ok(digest) => digest,
                    Err(_) => return unsupported("prefix runtime association could not be hashed"),
                };
            let Some(game) = game else {
                return unsupported("registered game installation is unavailable");
            };
            let Some(game_registry_revision) = game_registry_revision else {
                return unsupported("P031 game registry revision is unavailable");
            };
            if !executable.exists
                || !executable.regular_file
                || !executable.symlink_free
                || !executable.within_registered_root
                || !executable.root_identity_matches
            {
                return unsupported("P032 executable identity is incomplete or unsafe");
            }
            let (Some(game_executable_device), Some(game_executable_inode)) =
                (executable.file_device, executable.file_inode)
            else {
                return unsupported("P032 executable filesystem identity is unavailable");
            };
            let Ok(prefix_id) = prefixes::PrefixId::new(prefix_record.id.as_str()) else {
                return unsupported("managed prefix identifier is invalid");
            };
            if launch::validate_relative_executable(executable_relative).is_err() {
                return unsupported("game executable path is invalid");
            }
            let runtime_root = paths
                .data_dir()
                .join("runtimes")
                .join("wine")
                .join(&reference.installation_id);
            let game_root = PathBuf::from(&game.selected_path);
            let game_executable = game_root.join(executable_relative);
            let request = endfield_unix_runtime_linux::wine_plan::ManagedWineInvocationRequest {
                runtime_root,
                prefix: prefixes::PrefixStore::for_app(paths).target_path(&prefix_id),
                game_root,
                game_executable_relative: PathBuf::from(executable_relative),
                working_directory: game_executable
                    .parent()
                    .unwrap_or(Path::new("/"))
                    .to_path_buf(),
                arguments: vec![],
                binding: endfield_unix_runtime_linux::wine_plan::InvocationBinding {
                    runtime_installation_id: identity.installation_id,
                    runtime_pin_id: runtime.pin_id.clone(),
                    runtime_pin_revision: runtime.pin_revision,
                    runtime_reference_sha256,
                    runtime_generation: identity.generation,
                    runtime_ownership_record_sha256: identity.ownership_record_sha256,
                    runtime_artifact_sha256: identity.artifact_sha256,
                    runtime_audit_report_sha256,
                    prefix_id: prefix_record.id.as_str().to_owned(),
                    prefix_registry_revision: prefix_revision,
                    prefix_generation: prefix_record.generation,
                    prefix_device: prefix_identity.device,
                    prefix_inode: prefix_identity.inode,
                    prefix_runtime_pin_id: prefix_record.runtime.pin_id.clone(),
                    prefix_runtime_pin_revision: prefix_record.runtime.pin_revision,
                    prefix_runtime_reference_sha256,
                    game_installation_id: game.id.as_str().to_owned(),
                    game_registry_revision,
                    game_root_device: game.root_identity.device,
                    game_root_inode: game.root_identity.inode,
                    game_executable_device,
                    game_executable_inode,
                },
            };
            let invocation = match endfield_unix_runtime_linux::wine_plan::ManagedWineInvocationPlanBuilder::prepare(request) {
                Ok(invocation) => invocation,
                Err(_) => return unsupported("exact managed Wine argv could not be prepared safely"),
            };
            RuntimeInvocationAssessment {
                adapter,
                state: InvocationPreparationState::PreparedNonExecutable,
                contract_verified: true,
                invocation_plan_id: Some(invocation.plan_id.clone()),
                expected_executable_identity: Some(InvocationFileIdentity {
                    device: invocation.runtime_executable_identity.device,
                    inode: invocation.runtime_executable_identity.inode,
                    size: invocation.runtime_executable_identity.size,
                }),
                native_argument_count: Some(invocation.native_arguments.len()),
                process_creation_supported: false,
                reason: "managed Wine native argv is prepared; production process creation remains disabled",
                expected_effects: invocation.expected_effects,
            }
        }
        launch::RuntimeAdapter::UmuProton => RuntimeInvocationAssessment {
            adapter,
            state: InvocationPreparationState::PreparationOnly,
            contract_verified: false,
            invocation_plan_id: None,
            expected_executable_identity: None,
            native_argument_count: None,
            process_creation_supported: false,
            reason: "P023 provides non-executable UMU preparation; the selected CLI path has no verified process adapter",
            expected_effects: vec![
                "UMU/Proton may modify the prefix and runtime caches.",
                "UMU may acquire or update Steam Linux Runtime components over the network; UMU_RUNTIME_UPDATE=0 is not an offline guarantee.",
            ],
        },
        launch::RuntimeAdapter::ExternalWine => unsupported(
            "P022 external identity does not bind a production-approved exact Wine executable path",
        ),
        launch::RuntimeAdapter::DirectProton => unsupported(
            "direct Proton invocation has no approved Endfield adapter or verified game identifier semantics",
        ),
    }
}

struct PrefixObservation {
    evidence: launch::PrefixEvidence,
    record: Option<prefixes::PrefixRecord>,
    lifecycle: Option<launch::PrefixLifecycle>,
    registry_revision: Option<u64>,
    initialization_evidence: Option<prefixes::PrefixInitializationEvidenceStatus>,
    recovery: RecoveryEvidence,
    initialization_plan: Option<prefixes::PrefixInitializationPlan>,
}

fn inspect_prefix(
    paths: &endfield_unix_storage::AppPaths,
    prefix_id: &str,
    profile: Option<&str>,
    runtime: Option<&RuntimeObservation>,
) -> PrefixObservation {
    let unavailable = |state, recovery| PrefixObservation {
        evidence: launch::PrefixEvidence {
            state,
            prefix_id: Some(prefix_id.into()),
            lifecycle: launch::PrefixLifecycle::Missing,
            managed_ownership_verified: false,
            directory_identity_verified: false,
            runtime_association_matches: false,
            registry_revision: None,
            filesystem_device: None,
            filesystem_inode: None,
            associated_runtime_pin_id: None,
            associated_runtime_pin_revision: None,
            associated_runtime_reference_sha256: None,
            associated_adapter: None,
            initialization_plan_id: None,
            initialization_verified: false,
            recovery_pending: true,
        },
        record: None,
        lifecycle: None,
        registry_revision: None,
        initialization_evidence: None,
        recovery,
        initialization_plan: None,
    };
    let Ok(id) = prefixes::PrefixId::new(prefix_id.to_owned()) else {
        return unavailable(launch::EvidenceState::Missing, RecoveryEvidence::Unknown);
    };
    let recovery = prefixes::backup::BackupStore::for_app(paths)
        .compatibility_recovery_observation(&id)
        .map(|observation| {
            if observation.pending_deletion
                || observation.pending_pin_recovery
                || !matches!(
                    observation.restore_state,
                    prefixes::backup::CompatibilityRecoveryState::None
                        | prefixes::backup::CompatibilityRecoveryState::Completed
                        | prefixes::backup::CompatibilityRecoveryState::RollbackCompleted
                )
            {
                RecoveryEvidence::Pending
            } else {
                RecoveryEvidence::Clear
            }
        })
        .unwrap_or(RecoveryEvidence::Unknown);
    let store = prefixes::PrefixStore::for_app(paths);
    let report = match store.validate(&id, super::runtime_pin::validate_reference) {
        Ok(report) => report,
        Err(prefixes::PrefixError::NotRegistered | prefixes::PrefixError::Missing) => {
            return unavailable(launch::EvidenceState::Missing, recovery);
        }
        Err(_) => return unavailable(launch::EvidenceState::Unknown, recovery),
    };
    let record = &report.record;
    let lifecycle = match report.observed_lifecycle {
        prefixes::PrefixLifecycleState::Planned => launch::PrefixLifecycle::Planned,
        prefixes::PrefixLifecycleState::Created => launch::PrefixLifecycle::Created,
        prefixes::PrefixLifecycleState::Initialized => launch::PrefixLifecycle::Initialized,
        prefixes::PrefixLifecycleState::Incomplete => launch::PrefixLifecycle::Incomplete,
        prefixes::PrefixLifecycleState::RecoveryRequired => {
            launch::PrefixLifecycle::RecoveryRequired
        }
        prefixes::PrefixLifecycleState::Missing => launch::PrefixLifecycle::Missing,
        prefixes::PrefixLifecycleState::Blocked => launch::PrefixLifecycle::Blocked,
    };
    let runtime_valid = matches!(report.runtime, prefixes::RuntimeValidation::Valid);
    let runtime_matches = runtime.is_some_and(|current| {
        record.runtime.runtime == current.reference
            && record.runtime.pin_id == current.pin_id
            && record.runtime.pin_revision == current.pin_revision
            && record.runtime.adapter_preference == current.pin_adapter
            && matches!(
                (&record.runtime.selection_source, &current.source),
                (
                    prefixes::RuntimeSelectionSource::ProfilePin,
                    pins::SelectionSource::ProfilePin,
                ) | (
                    prefixes::RuntimeSelectionSource::MachinePin,
                    pins::SelectionSource::MachinePin,
                )
            )
    });
    let scope_matches = pins::current_machine_id().is_ok_and(|machine| {
        let selected_profile =
            profile.and_then(|value| pins::ProfileKey::new(value.to_owned()).ok());
        match (selected_profile, &record.scope) {
            (
                Some(profile),
                prefixes::PrefixScope::Profile {
                    machine: saved,
                    profile: saved_profile,
                },
            ) => &machine == saved && &profile == saved_profile,
            (None, prefixes::PrefixScope::Machine { machine: saved }) => &machine == saved,
            _ => false,
        }
    });
    // P028 records claims but no runtime-specific completion verifier exists.
    // A stored CompletedAndVerified label is not sufficient evidence for P034.
    let initialization_verified = false;
    let state = if runtime_valid
        && !matches!(lifecycle, launch::PrefixLifecycle::Blocked | launch::PrefixLifecycle::Missing)
    {
        launch::EvidenceState::Verified
    } else {
        launch::EvidenceState::Stale
    };
    let recovery_pending = recovery != RecoveryEvidence::Clear
        || matches!(
            lifecycle,
            launch::PrefixLifecycle::Incomplete | launch::PrefixLifecycle::RecoveryRequired
        );
    let directory_identity_verified = record.filesystem_identity.is_some()
        && !matches!(lifecycle, launch::PrefixLifecycle::Blocked | launch::PrefixLifecycle::Missing);
    let expected_runtime = runtime.and_then(|current| {
        let selection_source = match &current.source {
            pins::SelectionSource::ProfilePin => prefixes::RuntimeSelectionSource::ProfilePin,
            pins::SelectionSource::MachinePin => prefixes::RuntimeSelectionSource::MachinePin,
            pins::SelectionSource::ExplicitOverride | pins::SelectionSource::None => return None,
        };
        Some(prefixes::PrefixRuntimeAssociation {
            runtime: current.reference.clone(),
            selection_source,
            pin_id: current.pin_id.clone(),
            pin_revision: current.pin_revision,
            adapter_preference: current.pin_adapter,
        })
    });
    let recovery_for_plan = match recovery {
        RecoveryEvidence::Clear => Some(false),
        RecoveryEvidence::Pending => Some(true),
        RecoveryEvidence::Unknown => None,
    };
    let initialization_plan = expected_runtime.and_then(|expected| {
        store
            .plan_initialization(
                &id,
                report.registry_revision,
                &expected,
                recovery_for_plan,
                super::runtime_pin::validate_reference,
            )
            .ok()
    });
    let associated_adapter = match record.runtime.adapter_preference {
        Some(pins::AdapterPreference::ManagedWine) => Some(launch::RuntimeAdapter::ManagedWine),
        Some(pins::AdapterPreference::ExternalWine) => Some(launch::RuntimeAdapter::ExternalWine),
        Some(pins::AdapterPreference::UmuProton) => Some(launch::RuntimeAdapter::UmuProton),
        None => None,
    };
    let (filesystem_device, filesystem_inode) = if directory_identity_verified {
        record
            .filesystem_identity
            .map_or((None, None), |identity| (Some(identity.device), Some(identity.inode)))
    } else {
        (None, None)
    };
    PrefixObservation {
        evidence: launch::PrefixEvidence {
            state,
            prefix_id: Some(record.id.as_str().into()),
            lifecycle,
            managed_ownership_verified: record.ownership
                == prefixes::PrefixOwnership::EndfieldUnixManaged,
            directory_identity_verified,
            runtime_association_matches: runtime_matches && scope_matches,
            registry_revision: Some(report.registry_revision),
            filesystem_device,
            filesystem_inode,
            associated_runtime_pin_id: Some(record.runtime.pin_id.clone()),
            associated_runtime_pin_revision: Some(record.runtime.pin_revision),
            associated_runtime_reference_sha256:
                hash_runtime_reference(&record.runtime.runtime).ok(),
            associated_adapter,
            initialization_plan_id: initialization_plan
                .as_ref()
                .map(|plan| plan.plan_id.clone()),
            initialization_verified,
            recovery_pending,
        },
        record: Some(record.clone()),
        lifecycle: Some(lifecycle),
        registry_revision: Some(report.registry_revision),
        initialization_evidence: Some(report.initialization_evidence),
        recovery,
        initialization_plan,
    }
}

fn missing_executable(relative: &str) -> launch::GameExecutableEvidence {
    launch::GameExecutableEvidence {
        relative_path: relative.into(),
        exists: false,
        regular_file: false,
        // A missing entry provides no evidence of a symlink. If a symlink is
        // observed during inspection, inspect_executable records that fact.
        symlink_free: true,
        within_registered_root: false,
        root_identity_matches: false,
        game_root_device: None,
        game_root_inode: None,
        file_device: None,
        file_inode: None,
        file_size: None,
        format: launch::ExecutableFormat::Unknown,
        publisher_authenticity_verified: false,
    }
}

fn inspect_executable(
    record: &GameInstallationRecord,
    relative: &str,
    root_identity_matches: bool,
) -> launch::GameExecutableEvidence {
    let mut evidence = missing_executable(relative);
    evidence.root_identity_matches = root_identity_matches;
    if root_identity_matches {
        evidence.game_root_device = Some(record.root_identity.device);
        evidence.game_root_inode = Some(record.root_identity.inode);
    }
    let Ok(path) = launch::validate_relative_executable(relative).map(|_| PathBuf::from(relative))
    else {
        return evidence;
    };
    let root = Path::new(&record.selected_path);
    let Ok(root_file) = open_absolute_directory(root) else {
        return evidence;
    };
    if !matches_root_identity(&root_file, &record.root_identity) {
        return evidence;
    }
    let mut current = root_file;
    let components: Vec<_> = path.components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return evidence;
        };
        let Ok(name) = CString::new(name.as_bytes()) else {
            return evidence;
        };
        let final_component = index + 1 == components.len();
        let flags = if final_component {
            // O_PATH observes a directory entry without opening a device, FIFO,
            // or socket. A regular file is opened for reading only after fstat.
            libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC
        } else {
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC
        };
        // Each component is opened relative to the already-opened parent, with
        // O_NOFOLLOW, so planning cannot traverse an executable symlink.
        let fd = unsafe { libc::openat(current.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            let observed = unsafe {
                libc::fstatat(
                    current.as_raw_fd(),
                    name.as_ptr(),
                    stat.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            };
            if observed == 0 {
                let stat = unsafe { stat.assume_init() };
                if stat.st_mode & libc::S_IFMT == libc::S_IFLNK {
                    evidence.exists = true;
                    evidence.symlink_free = false;
                    evidence.within_registered_root = true;
                }
            }
            return evidence;
        }
        let opened = unsafe { File::from_raw_fd(fd) };
        if final_component {
            let Ok(metadata) = opened.metadata() else {
                return evidence;
            };
            evidence.exists = true;
            evidence.regular_file = metadata.is_file();
            evidence.symlink_free = !metadata.file_type().is_symlink();
            evidence.within_registered_root = true;
            evidence.root_identity_matches = root_identity_matches;
            evidence.file_device = Some(metadata.dev());
            evidence.file_inode = Some(metadata.ino());
            evidence.file_size = Some(metadata.len());
            if !evidence.regular_file || !evidence.symlink_free {
                return evidence;
            }
            let readable_path = format!("/proc/self/fd/{}", opened.as_raw_fd());
            let Ok(readable) = File::open(readable_path) else {
                return evidence;
            };
            let Ok(readable_metadata) = readable.metadata() else {
                return evidence;
            };
            if readable_metadata.dev() != metadata.dev()
                || readable_metadata.ino() != metadata.ino()
            {
                return evidence;
            }
            evidence.format = if is_pe(&readable) {
                launch::ExecutableFormat::PeCoff
            } else {
                launch::ExecutableFormat::Other
            };
        } else {
            current = opened;
        }
    }
    evidence
}

fn open_absolute_directory(path: &Path) -> io::Result<File> {
    if !path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe registered root",
        ));
    }
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open("/")?;
    let mut current = root;
    for component in path.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name = CString::new(name.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid root component"))?;
        let fd = unsafe {
            libc::openat(
                current.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        current = unsafe { File::from_raw_fd(fd) };
    }
    Ok(current)
}

fn matches_root_identity(file: &File, identity: &RootIdentity) -> bool {
    file.metadata()
        .is_ok_and(|metadata| metadata.dev() == identity.device && metadata.ino() == identity.inode)
}

fn is_pe(file: &File) -> bool {
    let Ok(mut reader) = file.try_clone() else {
        return false;
    };
    let mut dos = [0u8; 64];
    if reader.read_exact(&mut dos).is_err() || &dos[..2] != b"MZ" {
        return false;
    }
    let offset = u32::from_le_bytes([dos[60], dos[61], dos[62], dos[63]]) as u64;
    if offset > 1024 * 1024 || reader.seek(SeekFrom::Start(offset)).is_err() {
        return false;
    }
    let mut signature = [0u8; 4];
    reader.read_exact(&mut signature).is_ok() && &signature == b"PE\0\0"
}

fn render_human(output: &mut impl Write, report: &PreflightReport) -> io::Result<()> {
    let plan = &report.plan;
    writeln!(output, "Launch plan: {}", plan.plan_id)?;
    writeln!(output, "Installation: {}", plan.installation_id)?;
    writeln!(
        output,
        "Installation root identity matches: {}",
        plan.executable.root_identity_matches
    )?;
    writeln!(output, "Executable candidate: {}", plan.executable_relative)?;
    writeln!(
        output,
        "Executable observation: exists={}, regular_file={}, symlink_free={}, within_registered_root={}, format={:?}",
        plan.executable.exists,
        plan.executable.regular_file,
        plan.executable.symlink_free,
        plan.executable.within_registered_root,
        plan.executable.format
    )?;
    writeln!(
        output,
        "Runtime pin: {} ({:?}, revision {:?})",
        plan.runtime.pin_id.as_deref().unwrap_or("unavailable"),
        plan.runtime.state,
        plan.runtime.pin_revision
    )?;
    writeln!(output, "Adapter: {:?}", plan.runtime.adapter)?;
    writeln!(
        output,
        "Runtime invocation: {:?}; process creation supported: {}; {}",
        report.runtime_invocation.state,
        report.runtime_invocation.process_creation_supported,
        report.runtime_invocation.reason
    )?;
    for effect in &report.runtime_invocation.expected_effects {
        writeln!(output, "Invocation side effect: {effect}")?;
    }
    writeln!(
        output,
        "Prefix: {} ({:?}, revision {:?})",
        plan.prefix.prefix_id.as_deref().unwrap_or("unavailable"),
        plan.prefix.lifecycle,
        plan.prefix.registry_revision
    )?;
    writeln!(
        output,
        "Prefix initialization: lifecycle {:?}, evidence {:?}, P029 recovery {:?}",
        report.prefix_initialization.lifecycle,
        report.prefix_initialization.evidence,
        report.prefix_initialization.recovery
    )?;
    if let Some(initialization) = &report.prefix_initialization.plan {
        writeln!(
            output,
            "Initialization plan: {} ({:?}); execution supported: {}; blockers: {:?}",
            initialization.plan_id,
            initialization.state,
            initialization.execution_supported,
            initialization.blockers
        )?;
        for effect in &initialization.expected_effects {
            writeln!(output, "Initialization side effect: {effect}")?;
        }
    }
    writeln!(output, "Approval required: {}", plan.approval_required)?;
    writeln!(
        output,
        "Execution policy: {:?}; process creation allowed: {}",
        plan.policy.state, plan.policy.process_creation_allowed
    )?;
    writeln!(
        output,
        "Process execution supported: {}",
        plan.process_execution_supported
    )?;
    writeln!(output, "State: {:?}", plan.state)?;
    for blocker in &plan.blockers {
        writeln!(output, "Blocker: {}", blocker.code())?;
    }
    for warning in &plan.warnings {
        writeln!(output, "Warning: {warning:?}")?;
    }
    for side_effect in &plan.side_effects {
        writeln!(output, "Side effect: {side_effect}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use endfield_unix_game_import::{
        GameInstallationId, GameInstallationRecord, GameInstallationRegistry, GameInstallationState,
    };
    use endfield_unix_runtime_selection as pins;
    use std::{fs, os::unix::fs::PermissionsExt};
    use tempfile::tempdir;

    fn external_proton(path: &Path) -> pins::ExternalRuntimeReference {
        let provider = endfield_unix_runtime_linux::LinuxRuntimeProvider::new(
            endfield_unix_runtime_linux::DiscoveryOptions {
                additional_roots: vec![path.to_path_buf()],
                include_path: false,
                include_conventional: false,
            },
        )
        .unwrap();
        let report = provider.scan();
        let runtime = report
            .runtimes
            .into_iter()
            .find(|runtime| runtime.path() == path)
            .expect("synthetic Proton candidate");
        super::super::runtime_pin::external_reference(runtime.external_snapshot().unwrap()).unwrap()
    }

    fn create_proton(path: &Path, version: &str) {
        fs::create_dir_all(path).unwrap();
        let launcher = path.join("proton");
        fs::write(&launcher, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(path.join("version"), version).unwrap();
    }

    fn pe_fixture() -> Vec<u8> {
        let mut bytes = vec![0; 68];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes
    }

    fn synthetic_game_record(
        paths: &endfield_unix_storage::AppPaths,
        game_root: &Path,
    ) -> GameInstallationRecord {
        let registry = GameInstallationRegistry::for_app(paths);
        let plan = registry
            .plan_import(
                GameInstallationId::new("endfield-main".to_owned()).unwrap(),
                game_root,
                true,
            )
            .unwrap();
        GameInstallationRecord {
            id: plan.proposed_id,
            scope: plan.scope,
            state: GameInstallationState::Registered,
            selected_path: plan.selected_path,
            root_identity: plan.root_identity,
            filesystem: plan.filesystem,
            evidence: plan.evidence,
            registered_at_unix_seconds: 1,
        }
    }

    #[test]
    fn stale_profile_pin_remains_selected_without_machine_fallback() {
        let temporary = tempdir().unwrap();
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let machine_runtime_path = temporary.path().join("GE-Proton-P034-machine");
        let profile_runtime_path = temporary.path().join("GE-Proton-P034-profile");
        create_proton(&machine_runtime_path, "machine-build\n");
        create_proton(&profile_runtime_path, "profile-build\n");

        let paths = endfield_unix_storage::AppPaths::from_base_dirs(
            temporary.path().join("config"),
            temporary.path().join("data"),
            temporary.path().join("cache"),
        )
        .unwrap();
        let store = pins::PinStore::for_app(&paths);
        let machine = pins::current_machine_id().unwrap();
        let machine_scope = pins::RuntimePinScope::Machine {
            machine: machine.clone(),
        };
        let profile = pins::ProfileKey::new("desktop".to_owned()).unwrap();
        let profile_scope = pins::RuntimePinScope::Profile { machine, profile };
        let machine_reference = external_proton(&machine_runtime_path);
        let profile_reference = external_proton(&profile_runtime_path);
        let machine_pin_id = pins::stable_pin_id(&machine_scope, &machine_reference);
        let profile_pin_id = pins::stable_pin_id(&profile_scope, &profile_reference);
        let machine_pin = pins::RuntimePin::new(
            machine_pin_id,
            machine_scope,
            machine_reference,
            Some(pins::AdapterPreference::UmuProton),
            1,
        )
        .unwrap();
        let snapshot = store.set(machine_pin, 0, false).unwrap();
        let profile_pin = pins::RuntimePin::new(
            profile_pin_id.clone(),
            profile_scope,
            profile_reference,
            Some(pins::AdapterPreference::UmuProton),
            2,
        )
        .unwrap();
        store.set(profile_pin, snapshot.revision, false).unwrap();
        let registry_path = paths
            .config_file()
            .parent()
            .unwrap()
            .join("runtime-pins.json");
        let before = fs::read(&registry_path).unwrap();

        // Change only the profile-selected runtime after pin creation. The
        // valid machine runtime must not replace the stale profile choice.
        fs::write(profile_runtime_path.join("version"), "replacement-build\n").unwrap();
        let (evidence, selected) =
            inspect_runtime(&paths, Some("desktop"), launch::RuntimeAdapter::UmuProton);

        assert_eq!(evidence.state, launch::EvidenceState::Stale);
        assert_eq!(evidence.selection_source.as_deref(), Some("profile_pin"));
        assert_eq!(evidence.pin_id.as_deref(), Some(profile_pin_id.as_str()));
        let expected_runtime_id = format!("external:{profile_pin_id}");
        assert_eq!(
            evidence.runtime_id.as_deref(),
            Some(expected_runtime_id.as_str())
        );
        assert!(selected.is_some());
        assert_eq!(fs::read(&registry_path).unwrap(), before);
    }

    #[test]
    fn executable_inspection_rejects_symlink_escape_and_observes_replacement() {
        let temporary = tempdir().unwrap();
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let game_root = temporary.path().join("game");
        let outside = temporary.path().join("outside");
        fs::create_dir(&game_root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(game_root.join("Selected.exe"), pe_fixture()).unwrap();
        fs::write(outside.join("Escaped.exe"), pe_fixture()).unwrap();
        let paths = endfield_unix_storage::AppPaths::from_base_dirs(
            temporary.path().join("config"),
            temporary.path().join("data"),
            temporary.path().join("cache"),
        )
        .unwrap();
        let record = synthetic_game_record(&paths, &game_root);

        let first = inspect_executable(&record, "Selected.exe", true);
        assert!(first.exists && first.regular_file && first.symlink_free);
        assert_eq!(first.format, launch::ExecutableFormat::PeCoff);
        let first_identity = (first.file_device.unwrap(), first.file_inode.unwrap());

        std::os::unix::fs::symlink(outside.join("Escaped.exe"), game_root.join("Escape.exe"))
            .unwrap();
        let escaped = inspect_executable(&record, "Escape.exe", true);
        assert!(escaped.exists);
        assert!(!escaped.regular_file);
        assert!(!escaped.symlink_free);
        assert!(escaped.within_registered_root);

        fs::rename(
            game_root.join("Selected.exe"),
            game_root.join("Selected.old"),
        )
        .unwrap();
        fs::write(game_root.join("Selected.exe"), pe_fixture()).unwrap();
        let replacement = inspect_executable(&record, "Selected.exe", true);
        let replacement_identity = (
            replacement.file_device.unwrap(),
            replacement.file_inode.unwrap(),
        );
        assert_ne!(first_identity, replacement_identity);
        assert!(replacement.regular_file && replacement.symlink_free);
    }
}
