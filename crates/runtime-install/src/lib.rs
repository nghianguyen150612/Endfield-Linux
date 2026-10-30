//! Offline Linux managed installation. No bundled publisher anchors or runtime execution.
#![cfg(target_os = "linux")]
mod extraction;
mod fs;
use endfield_unix_runtime::*;
use endfield_unix_runtime_verification as v;
use endfield_unix_storage::AppPaths;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};
pub type Result<T> = std::result::Result<T, Error>;
const MAX_STAGING_TRANSACTIONS: usize = 1024;
const MAX_MANAGED_INSTALLATIONS: usize = 200_000;
#[derive(Debug)]
pub enum Error {
    Verification(v::Error),
    Io(std::io::Error),
    TrustUnavailable,
    StaleEvidence,
    CheckpointRequired,
    Approval,
    UnsafePath,
    UnsafeOwnership,
    UnsupportedFormat,
    UnsupportedEntry,
    Limit,
    Layout,
    Busy,
    Record,
    ReconciliationRequired,
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<v::Error> for Error {
    fn from(e: v::Error) -> Self {
        Self::Verification(e)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrustClassification {
    IndependentlyAuthenticatedPublisher,
    UserPinnedLimitedTrust,
}

struct InstallationIdentityEvidence<'a> {
    family: &'a str,
    provider: &'a str,
    project: &'a str,
    build: &'a str,
    artifact_sha256: &'a str,
    manifest_sha256: &'a str,
    trust_key_sha256: &'a str,
    generation: u64,
}
fn derive_installation_id(evidence: &InstallationIdentityEvidence<'_>) -> String {
    let evidence_digest = format!(
        "{}:{}:{}:{}:{}:{}:{}",
        evidence.provider,
        evidence.project,
        evidence.build,
        evidence.artifact_sha256,
        evidence.manifest_sha256,
        evidence.trust_key_sha256,
        evidence.generation
    );
    format!(
        "{}-{}-{}",
        evidence.family,
        v::sha256(evidence_digest.as_bytes()),
        evidence.generation
    )
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeInstallationPlan {
    schema_version: u32,
    installation_id: String,
    family: String,
    lineage: String,
    build: String,
    provider: String,
    project: String,
    channel: String,
    key_id: String,
    trust_key_sha256: String,
    artifact_id: String,
    artifact_sha256: String,
    archive_size: u64,
    manifest_sha256: String,
    manifest_id: String,
    manifest_version: u64,
    destination: PathBuf,
    generation: u64,
    trust: TrustClassification,
}
impl RuntimeInstallationPlan {
    pub fn new(
        paths: &AppPaths,
        verified: &v::VerifiedManifest,
        artifact: &str,
        generation: u64,
    ) -> Result<Self> {
        if generation == 0 {
            return Err(Error::Approval);
        }
        let trust = trust(verified)?;
        let m = verified.manifest();
        let a = m
            .artifacts
            .iter()
            .find(|a| a.artifact_id == artifact)
            .ok_or(v::Error::Identity)?;
        if !a.filename.ends_with(".tar.gz") {
            return Err(Error::UnsupportedFormat);
        }
        let id = derive_installation_id(&InstallationIdentityEvidence {
            family: &m.runtime_family,
            provider: &m.provider_id,
            project: &m.project_id,
            build: &m.runtime_build,
            artifact_sha256: &a.sha256,
            manifest_sha256: verified.content_sha256(),
            trust_key_sha256: verified.key_sha256(),
            generation,
        });
        let plan = Self {
            schema_version: 1,
            destination: paths
                .data_dir()
                .join("runtimes")
                .join(&m.runtime_family)
                .join(&id),
            installation_id: id,
            family: m.runtime_family.clone(),
            lineage: m.runtime_lineage.clone(),
            build: m.runtime_build.clone(),
            provider: m.provider_id.clone(),
            project: m.project_id.clone(),
            channel: m.channel.clone(),
            key_id: m.key_id.clone(),
            trust_key_sha256: verified.key_sha256().to_owned(),
            artifact_id: artifact.into(),
            artifact_sha256: a.sha256.clone(),
            archive_size: a.size_bytes,
            manifest_sha256: verified.content_sha256().to_owned(),
            manifest_id: m.manifest_id.clone(),
            manifest_version: m.manifest_version,
            generation,
            trust,
        };
        let resource = Identifier::new(plan.identity()).map_err(|_| Error::Approval)?;
        let decision = RuntimeManagementPolicy
            .evaluate(
                ManagementAction::Install,
                &ManagementTarget::NewManagedLocation(resource),
                &UserIntent::NotProvided,
            )
            .map_err(|_| Error::Approval)?;
        if decision != ManagementDecision::RequiresExplicitApproval {
            return Err(Error::Approval);
        }
        Ok(plan)
    }
    pub fn identity(&self) -> String {
        v::sha256(&serde_json::to_vec(self).expect("serializable plan"))
    }
    pub fn destination(&self) -> &Path {
        &self.destination
    }
    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn artifact_sha256(&self) -> &str {
        &self.artifact_sha256
    }
    pub fn trust_key_sha256(&self) -> &str {
        &self.trust_key_sha256
    }
    pub fn key_id(&self) -> &str {
        &self.key_id
    }
    pub fn trust_classification(&self) -> TrustClassification {
        self.trust
    }
    pub fn family(&self) -> &str {
        &self.family
    }
    pub fn build(&self) -> &str {
        &self.build
    }
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }
    /// Deliberate approval of this exact plan, never deserialized from plan JSON.
    pub fn approve(&self, exact_identity: &str) -> Result<Authorization> {
        if self.trust != TrustClassification::IndependentlyAuthenticatedPublisher
            || exact_identity != self.identity()
        {
            return Err(Error::Approval);
        }
        Ok(Authorization {
            identity: self.identity(),
            trust: self.trust,
        })
    }

    /// Explicit manual-risk acceptance for a key pinned by the user. The
    /// artifact fingerprint and exact plan identity must both be acknowledged.
    pub fn approve_user_pinned(
        &self,
        exact_identity: &str,
        acknowledged_artifact_sha256: &str,
    ) -> Result<Authorization> {
        if self.trust != TrustClassification::UserPinnedLimitedTrust
            || exact_identity != self.identity()
            || acknowledged_artifact_sha256 != self.artifact_sha256
        {
            return Err(Error::Approval);
        }
        Ok(Authorization {
            identity: self.identity(),
            trust: self.trust,
        })
    }
}
pub struct Authorization {
    identity: String,
    trust: TrustClassification,
}
fn trust(m: &v::VerifiedManifest) -> Result<TrustClassification> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|t| i64::try_from(t.as_secs()).ok())
        .ok_or(Error::TrustUnavailable)?;
    if !m.is_fresh_at(now, 300) {
        return Err(Error::StaleEvidence);
    }
    match m.provenance() {
        v::Provenance::IndependentlyAuthenticatedPublisher => {
            if m.rollback() != v::RollbackStatus::Checked {
                return Err(Error::CheckpointRequired);
            }
            Ok(TrustClassification::IndependentlyAuthenticatedPublisher)
        }
        v::Provenance::UserPinned => Ok(TrustClassification::UserPinnedLimitedTrust),
        v::Provenance::Unverified => Err(Error::TrustUnavailable),
    }
}
fn validate_artifact_report(
    report: &v::VerificationReport,
    trust: TrustClassification,
) -> Result<()> {
    let (provenance, state) = match trust {
        TrustClassification::IndependentlyAuthenticatedPublisher => (
            v::Provenance::IndependentlyAuthenticatedPublisher,
            SourceVerificationState::ArtifactIntegrityVerified,
        ),
        TrustClassification::UserPinnedLimitedTrust => (
            v::Provenance::UserPinned,
            SourceVerificationState::CandidateIdentified,
        ),
    };
    if report.provenance != provenance || report.source.verification().state() != state {
        return Err(Error::TrustUnavailable);
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransactionState {
    Planned,
    Authorized,
    ArtifactVerified,
    Staging,
    Extracted,
    Validated,
    Published,
    Failed,
    ReconciliationRequired,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationRecord {
    schema_version: u32,
    owner: String,
    plan: RuntimeInstallationPlan,
    plan_sha256: String,
    state: TransactionState,
    trust: TrustClassification,
    installed_at: u64,
    reconciled_transactions: usize,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransactionJournal {
    schema_version: u32,
    owner: String,
    plan: RuntimeInstallationPlan,
    state: TransactionState,
}
impl InstallationRecord {
    pub fn trust_classification(&self) -> TrustClassification {
        self.trust
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallStage {
    LockAcquired,
    BeforeStageCreation,
    StageCreated,
    BeforeArtifactCopy,
    StagedArtifactCopied,
    ArtifactVerified,
    BeforeExtraction,
    Extracted,
    BeforeValidation,
    Validated,
    RecordWritten,
    BeforePublish,
    Published,
}
pub trait InstallObserver {
    fn reached(&mut self, stage: InstallStage) -> Result<()>;
}
struct NoopObserver;
impl InstallObserver for NoopObserver {
    fn reached(&mut self, _: InstallStage) -> Result<()> {
        Ok(())
    }
}
/// Apply consumes fresh opaque P024 evidence. Trust provenance in the serialized
/// plan is only a claim; this function reconstructs it from the evidence argument.
pub fn install(
    paths: &AppPaths,
    plan: &RuntimeInstallationPlan,
    approval: Authorization,
    verified: &v::VerifiedManifest,
    artifact: &Path,
) -> Result<InstallationRecord> {
    install_observed(paths, plan, approval, verified, artifact, &mut NoopObserver)
}

/// Same transaction with deterministic boundary observation for failure injection.
/// Observer errors abort before publication; after the rename they report a
/// reconciliation-required outcome because visibility has already changed.
pub fn install_observed(
    paths: &AppPaths,
    plan: &RuntimeInstallationPlan,
    approval: Authorization,
    verified: &v::VerifiedManifest,
    artifact: &Path,
    observer: &mut impl InstallObserver,
) -> Result<InstallationRecord> {
    let evidence_trust = trust(verified)?;
    let rebuilt =
        RuntimeInstallationPlan::new(paths, verified, &plan.artifact_id, plan.generation)?;
    if rebuilt.identity() != plan.identity()
        || approval.identity != plan.identity()
        || approval.trust != evidence_trust
    {
        return Err(Error::Approval);
    }
    let resource = Identifier::new(plan.identity()).map_err(|_| Error::Approval)?;
    let intent = UserIntent::ExplicitlyApproved {
        action: UserAction::InstallRuntime,
        resource_id: resource.clone(),
    };
    if RuntimeManagementPolicy
        .evaluate(
            ManagementAction::Install,
            &ManagementTarget::NewManagedLocation(resource),
            &intent,
        )
        .map_err(|_| Error::Approval)?
        != ManagementDecision::AllowedByPolicy
    {
        return Err(Error::Approval);
    }
    let verify = |file| {
        v::verify_artifact(v::ArtifactInput {
            artifact_id: &plan.artifact_id,
            filename: &verified
                .manifest()
                .artifacts
                .iter()
                .find(|a| a.artifact_id == plan.artifact_id)
                .ok_or(v::Error::Identity)?
                .filename,
            build: &plan.build,
            file,
            metadata: verified,
        })
        .map_err(Error::from)
    };
    let external_report = verify(v::open_artifact(artifact)?)?;
    validate_artifact_report(&external_report, evidence_trust)?;
    let app = fs::root(paths.data_dir(), true)?;
    let root = fs::dir(&app, "runtimes", true)?;
    fs::private(&root)?;
    let lock = fs::open(&root, ".install-lock", libc::O_RDWR | libc::O_CREAT, 0o600)?;
    fs::private_lock(&lock)?;
    fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| Error::Busy)?;
    observer.reached(InstallStage::LockAcquired)?;
    let staging = fs::dir(&root, ".staging", true)?;
    fs::private(&staging)?;
    let reconciled_transactions = reconcile_owned_staging(&staging, plan)?;
    let family = fs::dir(&root, &plan.family, true)?;
    fs::private(&family)?;
    // Held directory handles pin all managed parents against path replacement.
    observer.reached(InstallStage::BeforeStageCreation)?;
    let stage_path = PathBuf::from(format!("/proc/self/fd/{}", staging.as_raw_fd()));
    let stage = tempfile::Builder::new()
        .prefix("transaction-")
        .tempdir_in(stage_path)?;
    // /proc fd paths intentionally contain a kernel symlink; open relative to pinned parent instead.
    let name = stage
        .path()
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or(Error::UnsafePath)?;
    let handle = fs::dir(&staging, name, false)?;
    let mut journal = fs::open(
        &handle,
        "transaction.json",
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )?;
    write_journal(&mut journal, &handle, plan, TransactionState::Authorized)?;
    staging.sync_all()?;
    observer.reached(InstallStage::StageCreated)?;
    if fs2::available_space(stage.path())?
        < extraction::MAX_EXPANDED_BYTES
            .min(
                plan.archive_size
                    .saturating_mul(1000)
                    .saturating_add(1024 * 1024),
            )
            .saturating_add(plan.archive_size)
    {
        return Err(Error::Limit);
    }
    let mut copy = fs::open(
        &handle,
        "artifact",
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )?;
    observer.reached(InstallStage::BeforeArtifactCopy)?;
    let count = std::io::copy(
        &mut v::open_artifact(artifact)?.take(plan.archive_size.saturating_add(1)),
        &mut copy,
    )?;
    if count != plan.archive_size {
        return Err(Error::Verification(v::Error::SizeMismatch));
    }
    copy.sync_all()?;
    drop(copy);
    write_journal(&mut journal, &handle, plan, TransactionState::Staging)?;
    observer.reached(InstallStage::StagedArtifactCopied)?;
    let staged_report = verify(fs::open(&handle, "artifact", libc::O_RDONLY, 0)?)?;
    validate_artifact_report(&staged_report, evidence_trust)?;
    write_journal(
        &mut journal,
        &handle,
        plan,
        TransactionState::ArtifactVerified,
    )?;
    observer.reached(InstallStage::ArtifactVerified)?;
    observer.reached(InstallStage::BeforeExtraction)?;
    let tree = fs::dir(&handle, "tree", true)?;
    extraction::extract(
        fs::open(&handle, "artifact", libc::O_RDONLY, 0)?,
        &tree,
        plan.archive_size,
    )?;
    write_journal(&mut journal, &handle, plan, TransactionState::Extracted)?;
    observer.reached(InstallStage::Extracted)?;
    observer.reached(InstallStage::BeforeValidation)?;
    extraction::validate(&tree, &plan.family, &plan.build)?;
    write_journal(&mut journal, &handle, plan, TransactionState::Validated)?;
    observer.reached(InstallStage::Validated)?;
    let baseline_root = PathBuf::from(format!("/proc/self/fd/{}", tree.as_raw_fd()));
    let baseline = endfield_unix_runtime_audit::create_baseline(
        &baseline_root, &plan.installation_id, plan.generation, &plan.family, &plan.build,
        &plan.artifact_sha256, &plan.manifest_sha256,
        match evidence_trust {
            TrustClassification::IndependentlyAuthenticatedPublisher => endfield_unix_runtime_audit::TrustClassification::IndependentlyAuthenticatedPublisher,
            TrustClassification::UserPinnedLimitedTrust => endfield_unix_runtime_audit::TrustClassification::UserPinnedLimitedTrust,
        },
    ).map_err(|_| Error::Record)?;
    endfield_unix_runtime_audit::write_baseline(&baseline_root, &baseline)
        .map_err(|_| Error::Record)?;
    let record = InstallationRecord {
        schema_version: 1,
        owner: "endfield-unix-runtime-install-v1".into(),
        plan: plan.clone(),
        plan_sha256: plan.identity(),
        state: TransactionState::Published,
        trust: evidence_trust,
        installed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::Record)?
            .as_secs(),
        reconciled_transactions,
    };
    // Record travels inside tree in the same atomic publication, eliminating a separate record commit gap.
    let mut f = fs::open(
        &tree,
        ".endfield-installation.json",
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )?;
    f.write_all(&serde_json::to_vec_pretty(&record).map_err(|_| Error::Record)?)?;
    f.sync_all()?;
    tree.sync_all()?;
    observer.reached(InstallStage::RecordWritten)?;
    observer.reached(InstallStage::BeforePublish)?;
    fs::publish(&handle, "tree", &family, &plan.installation_id)?;
    observer
        .reached(InstallStage::Published)
        .map_err(|_| Error::ReconciliationRequired)?;
    Ok(record)
}

fn reconcile_owned_staging(
    staging: &std::fs::File,
    plan: &RuntimeInstallationPlan,
) -> Result<usize> {
    let mut reconciled = 0;
    let entries = std::fs::read_dir(format!("/proc/self/fd/{}", staging.as_raw_fd()))?
        .take(MAX_STAGING_TRANSACTIONS + 1)
        .collect::<std::io::Result<Vec<_>>>()?;
    if entries.len() > MAX_STAGING_TRANSACTIONS {
        return Err(Error::Limit);
    }
    for entry in entries {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.starts_with("transaction-") {
            continue;
        }
        let transaction = match fs::dir(staging, &name, false) {
            Ok(f) if fs::private(&f).is_ok() => f,
            _ => continue,
        };
        let mut journal_file = match fs::open(&transaction, "transaction.json", libc::O_RDONLY, 0) {
            Ok(f) if fs::private_lock(&f).is_ok() => f,
            _ => continue,
        };
        if journal_file.metadata()?.len() > 65536 {
            continue;
        }
        let mut bytes = Vec::new();
        (&mut journal_file).take(65537).read_to_end(&mut bytes)?;
        let Ok(journal) = serde_json::from_slice::<TransactionJournal>(&bytes) else {
            continue;
        };
        let expected_destination = plan.destination.clone();
        if journal.schema_version != 1
            || journal.owner != "endfield-unix-runtime-install-v1"
            || journal.plan.identity() != plan.identity()
            || journal.plan.destination != expected_destination
            || journal.plan.installation_id != plan.installation_id
            || journal.plan.family != plan.family
            || !matches!(
                journal.state,
                TransactionState::Authorized
                    | TransactionState::Staging
                    | TransactionState::ArtifactVerified
                    | TransactionState::Extracted
                    | TransactionState::Validated
            )
        {
            continue;
        }
        drop(journal_file);
        drop(transaction);
        fs::remove_tree(staging, &name)?;
        reconciled += 1;
    }
    Ok(reconciled)
}

fn write_journal(
    file: &mut std::fs::File,
    directory: &std::fs::File,
    plan: &RuntimeInstallationPlan,
    state: TransactionState,
) -> Result<()> {
    let journal = TransactionJournal {
        schema_version: 1,
        owner: "endfield-unix-runtime-install-v1".into(),
        plan: plan.clone(),
        state,
    };
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&serde_json::to_vec(&journal).map_err(|_| Error::Record)?)?;
    file.sync_all()?;
    directory.sync_all()?;
    Ok(())
}

/// Read-only recovery inspection. Interrupted transactions are preserved, never
/// recursively removed by inference from their names. No adoption of unknown trees.
#[derive(Debug, Serialize)]
pub struct RecoveryStatus {
    pub incomplete_staging: usize,
    pub validated_staging_requiring_approval: usize,
    pub recorded_installations: usize,
    pub ambiguous_entries: usize,
}
pub fn status(paths: &AppPaths) -> Result<RecoveryStatus> {
    let path = paths.data_dir().join("runtimes");
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RecoveryStatus {
                incomplete_staging: 0,
                validated_staging_requiring_approval: 0,
                recorded_installations: 0,
                ambiguous_entries: 0,
            });
        }
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    let root = fs::root(&path, false)?;
    let mut status = RecoveryStatus {
        incomplete_staging: 0,
        validated_staging_requiring_approval: 0,
        recorded_installations: 0,
        ambiguous_entries: 0,
    };
    for entry in std::fs::read_dir(format!("/proc/self/fd/{}", root.as_raw_fd()))? {
        let entry = entry?;
        let Some(component) = entry.file_name().to_str().map(str::to_owned) else {
            status.ambiguous_entries += 1;
            continue;
        };
        if !matches!(
            component.as_str(),
            ".staging" | ".install-lock" | "wine" | "proton"
        ) {
            status.ambiguous_entries += 1;
        }
    }
    match fs::open(&root, ".install-lock", libc::O_RDONLY, 0) {
        Ok(lock) if fs::private_lock(&lock).is_ok() => {}
        Ok(_) => status.ambiguous_entries += 1,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => status.ambiguous_entries += 1,
    }
    for name in [".staging", "wine", "proton"] {
        let dir = match fs::dir(&root, name, false) {
            Ok(f) => f,
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                status.ambiguous_entries += 1;
                continue;
            }
        };
        if fs::private(&dir).is_err() {
            status.ambiguous_entries += 1;
            continue;
        }
        let entry_limit = if name == ".staging" {
            MAX_STAGING_TRANSACTIONS
        } else {
            MAX_MANAGED_INSTALLATIONS
        };
        for (index, entry) in std::fs::read_dir(format!("/proc/self/fd/{}", dir.as_raw_fd()))?
            .take(entry_limit + 1)
            .enumerate()
        {
            if index >= entry_limit {
                return Err(Error::Limit);
            }
            let entry = entry?;
            let Some(name_component) = entry.file_name().to_str().map(str::to_owned) else {
                status.ambiguous_entries += 1;
                continue;
            };
            let child = match fs::dir(&dir, &name_component, false) {
                Ok(f) => f,
                Err(_) => {
                    status.ambiguous_entries += 1;
                    continue;
                }
            };
            if fs::private(&child).is_err() {
                status.ambiguous_entries += 1;
                continue;
            }
            if name == ".staging" {
                let mut bytes = Vec::new();
                let journal = fs::open(&child, "transaction.json", libc::O_RDONLY, 0);
                if let Ok(file) = journal {
                    file.take(65537).read_to_end(&mut bytes)?;
                    if bytes.len() <= 65536 {
                        if let Ok(j) = serde_json::from_slice::<TransactionJournal>(&bytes) {
                            let expected = paths
                                .data_dir()
                                .join("runtimes")
                                .join(&j.plan.family)
                                .join(&j.plan.installation_id);
                            if j.schema_version == 1
                                && j.owner == "endfield-unix-runtime-install-v1"
                                && name_component.starts_with("transaction-")
                                && j.plan.destination == expected
                                && matches!(
                                    j.state,
                                    TransactionState::Authorized
                                        | TransactionState::Staging
                                        | TransactionState::ArtifactVerified
                                        | TransactionState::Extracted
                                        | TransactionState::Validated
                                )
                            {
                                status.incomplete_staging += 1;
                                if j.state == TransactionState::Validated {
                                    status.validated_staging_requiring_approval += 1;
                                }
                                continue;
                            }
                        }
                    }
                }
                status.ambiguous_entries += 1;
            } else {
                let mut bytes = Vec::new();
                let record = fs::open(&child, ".endfield-installation.json", libc::O_RDONLY, 0);
                if let Ok(file) = record {
                    file.take(65537).read_to_end(&mut bytes)?;
                    let record = serde_json::from_slice::<InstallationRecord>(&bytes);
                    let expected_destination = paths
                        .data_dir()
                        .join("runtimes")
                        .join(name)
                        .join(&name_component);
                    if bytes.len() <= 65536
                        && record.as_ref().is_ok_and(|r| {
                            r.schema_version == 1
                                && r.owner == "endfield-unix-runtime-install-v1"
                                && r.state == TransactionState::Published
                                && r.plan.schema_version == 1
                                && r.plan_sha256 == r.plan.identity()
                                && r.plan.installation_id == name_component
                                && r.plan.family == name
                                && r.plan.installation_id
                                    == derive_installation_id(&InstallationIdentityEvidence {
                                        family: &r.plan.family,
                                        provider: &r.plan.provider,
                                        project: &r.plan.project,
                                        build: &r.plan.build,
                                        artifact_sha256: &r.plan.artifact_sha256,
                                        manifest_sha256: &r.plan.manifest_sha256,
                                        trust_key_sha256: &r.plan.trust_key_sha256,
                                        generation: r.plan.generation,
                                    })
                                && r.plan.destination == expected_destination
                                && valid_sha256(&r.plan.artifact_sha256)
                                && valid_sha256(&r.plan.manifest_sha256)
                                && valid_sha256(&r.plan.trust_key_sha256)
                        })
                    {
                        status.recorded_installations += 1;
                        continue;
                    }
                }
                status.ambiguous_entries += 1;
            }
        }
    }
    Ok(status)
}

/// Read-only audit of one explicitly selected managed installation. Ownership
/// metadata is validated before the payload baseline is consulted.
pub fn audit_installation(
    paths: &AppPaths,
    installation_id: &str,
) -> Result<endfield_unix_runtime_audit::AuditReport> {
    if installation_id.len() > 256
        || installation_id.contains('/')
        || installation_id.contains("..")
    {
        return Err(Error::Record);
    }
    let family_name = if installation_id.starts_with("wine-") {
        "wine"
    } else if installation_id.starts_with("proton-") {
        "proton"
    } else {
        return Err(Error::Record);
    };
    let runtime_root = paths.data_dir().join("runtimes");
    let root = fs::root(&runtime_root, false)?;
    let audit_lock = fs::open(&root, ".install-lock", libc::O_RDONLY, 0)?;
    fs::private_lock(&audit_lock)?;
    fs2::FileExt::try_lock_shared(&audit_lock).map_err(|_| Error::Busy)?;
    let family = fs::dir(&root, family_name, false)?;
    fs::private(&family)?;
    let install = fs::dir(&family, installation_id, false)?;
    fs::private(&install)?;
    let record_file = fs::open(&install, ".endfield-installation.json", libc::O_RDONLY, 0)?;
    fs::private_lock(&record_file)?;
    let bytes = read_stable_control(&record_file, 65536)?;
    let record: InstallationRecord = serde_json::from_slice(&bytes).map_err(|_| Error::Record)?;
    let expected_destination = runtime_root.join(family_name).join(installation_id);
    if record.schema_version != 1
        || record.owner != "endfield-unix-runtime-install-v1"
        || record.state != TransactionState::Published
        || record.plan.schema_version != 1
        || record.plan.installation_id != installation_id
        || record.plan.family != family_name
        || record.plan.destination != expected_destination
        || record.plan.generation == 0
        || record.plan_sha256 != record.plan.identity()
        || record.trust != record.plan.trust
        || !valid_sha256(&record.plan.artifact_sha256)
        || !valid_sha256(&record.plan.manifest_sha256)
        || !valid_sha256(&record.plan.trust_key_sha256)
        || record.plan.installation_id
            != derive_installation_id(&InstallationIdentityEvidence {
                family: &record.plan.family,
                provider: &record.plan.provider,
                project: &record.plan.project,
                build: &record.plan.build,
                artifact_sha256: &record.plan.artifact_sha256,
                manifest_sha256: &record.plan.manifest_sha256,
                trust_key_sha256: &record.plan.trust_key_sha256,
                generation: record.plan.generation,
            })
    {
        return Err(Error::Record);
    }
    let tree_path = PathBuf::from(format!("/proc/self/fd/{}", install.as_raw_fd()));
    let baseline=match endfield_unix_runtime_audit::load_baseline(&tree_path) {
        Ok(b)=>b,
        Err(error) => return Ok(endfield_unix_runtime_audit::AuditReport{completeness:if error.contains("missing"){endfield_unix_runtime_audit::Completeness::Unverifiable}else{endfield_unix_runtime_audit::Completeness::Failed},inspected:0,matching:0,omitted_findings:0,findings:vec![endfield_unix_runtime_audit::Finding{kind:if error.contains("missing"){endfield_unix_runtime_audit::FindingKind::BaselineMissing}else{endfield_unix_runtime_audit::FindingKind::BaselineInvalid},path:None}],trust:Some(match record.trust {TrustClassification::IndependentlyAuthenticatedPublisher=>endfield_unix_runtime_audit::TrustClassification::IndependentlyAuthenticatedPublisher,TrustClassification::UserPinnedLimitedTrust=>endfield_unix_runtime_audit::TrustClassification::UserPinnedLimitedTrust})}),
    };
    let expected_trust = match record.trust {
        TrustClassification::IndependentlyAuthenticatedPublisher => {
            endfield_unix_runtime_audit::TrustClassification::IndependentlyAuthenticatedPublisher
        }
        TrustClassification::UserPinnedLimitedTrust => {
            endfield_unix_runtime_audit::TrustClassification::UserPinnedLimitedTrust
        }
    };
    if baseline.installation_id != installation_id
        || baseline.generation != record.plan.generation
        || baseline.family != family_name
        || baseline.build != record.plan.build
        || baseline.artifact_sha256 != record.plan.artifact_sha256
        || baseline.manifest_sha256 != record.plan.manifest_sha256
        || baseline.trust != expected_trust
    {
        return Ok(endfield_unix_runtime_audit::AuditReport {
            completeness: endfield_unix_runtime_audit::Completeness::Failed,
            inspected: 0,
            matching: 0,
            omitted_findings: 0,
            findings: vec![endfield_unix_runtime_audit::Finding {
                kind: endfield_unix_runtime_audit::FindingKind::BaselineInvalid,
                path: None,
            }],
            trust: Some(expected_trust),
        });
    }
    Ok(endfield_unix_runtime_audit::audit(&tree_path, &baseline))
}

/// Compare an installation with payload contents reconstructed from an archive
/// that P024 re-verifies against the installation's exact manifest and artifact
/// identity. The temporary inventory is discarded and never enrolled locally.
pub fn audit_against_verified_artifact(
    paths: &AppPaths,
    installation_id: &str,
    verified: &v::VerifiedManifest,
    artifact: &Path,
) -> Result<endfield_unix_runtime_audit::AuditReport> {
    let local = audit_installation(paths, installation_id)?;
    // Audit installation already validates the ownership record. Re-open the
    // selected private tree and validate the same exact record for source binding.
    let family_name = if installation_id.starts_with("wine-") {
        "wine"
    } else {
        "proton"
    };
    let runtime_root = paths.data_dir().join("runtimes");
    let root = fs::root(&runtime_root, false)?;
    let audit_lock = fs::open(&root, ".install-lock", libc::O_RDONLY, 0)?;
    fs::private_lock(&audit_lock)?;
    fs2::FileExt::try_lock_shared(&audit_lock).map_err(|_| Error::Busy)?;
    let family = fs::dir(&root, family_name, false)?;
    let install = fs::dir(&family, installation_id, false)?;
    let record_file = fs::open(&install, ".endfield-installation.json", libc::O_RDONLY, 0)?;
    fs::private_lock(&record_file)?;
    let bytes = read_stable_control(&record_file, 65536)?;
    let record: InstallationRecord = serde_json::from_slice(&bytes).map_err(|_| Error::Record)?;
    let rebuilt = RuntimeInstallationPlan::new(
        paths,
        verified,
        &record.plan.artifact_id,
        record.plan.generation,
    )?;
    if rebuilt.identity() != record.plan.identity()
        || record.state != TransactionState::Published
        || record.trust != trust(verified)?
    {
        return Err(Error::Verification(v::Error::Identity));
    }
    let artifact_meta = verified
        .manifest()
        .artifacts
        .iter()
        .find(|a| a.artifact_id == record.plan.artifact_id)
        .ok_or(v::Error::Identity)?;
    let scratch = tempfile::Builder::new()
        .prefix("endfield-audit-")
        .tempdir()?;
    std::fs::set_permissions(scratch.path(), std::fs::Permissions::from_mode(0o700))?;
    let staged_path = scratch.path().join("source.tar.gz");
    let mut staged = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&staged_path)?;
    let copied = std::io::copy(
        &mut v::open_artifact(artifact)?.take(record.plan.archive_size.saturating_add(1)),
        &mut staged,
    )?;
    if copied != record.plan.archive_size {
        return Err(Error::Verification(v::Error::SizeMismatch));
    }
    staged.sync_all()?;
    drop(staged);
    let staged_report = v::verify_artifact(v::ArtifactInput {
        artifact_id: &record.plan.artifact_id,
        filename: &artifact_meta.filename,
        build: &record.plan.build,
        file: v::open_artifact(&staged_path)?,
        metadata: verified,
    })?;
    validate_artifact_report(&staged_report, record.trust)?;
    let scratch_dir = fs::root(scratch.path(), false)?;
    let expected_tree = fs::dir(&scratch_dir, "tree", true)?;
    extraction::extract(
        v::open_artifact(&staged_path)?,
        &expected_tree,
        record.plan.archive_size,
    )?;
    extraction::validate(&expected_tree, &record.plan.family, &record.plan.build)?;
    let expected_path = PathBuf::from(format!("/proc/self/fd/{}", expected_tree.as_raw_fd()));
    let trust_class = match record.trust {
        TrustClassification::IndependentlyAuthenticatedPublisher => {
            endfield_unix_runtime_audit::TrustClassification::IndependentlyAuthenticatedPublisher
        }
        TrustClassification::UserPinnedLimitedTrust => {
            endfield_unix_runtime_audit::TrustClassification::UserPinnedLimitedTrust
        }
    };
    let mut expected = endfield_unix_runtime_audit::create_baseline(
        &expected_path,
        installation_id,
        record.plan.generation,
        &record.plan.family,
        &record.plan.build,
        &record.plan.artifact_sha256,
        &record.plan.manifest_sha256,
        trust_class,
    )
    .map_err(|_| Error::Record)?;
    expected.origin =
        endfield_unix_runtime_audit::BaselineOrigin::ReconstructedFromReverifiedArtifact;
    let installed_path = PathBuf::from(format!("/proc/self/fd/{}", install.as_raw_fd()));
    let mut result = endfield_unix_runtime_audit::audit(&installed_path, &expected);
    result.trust = Some(trust_class);
    let _ = local;
    Ok(result)
}

fn read_stable_control(file: &std::fs::File, maximum: u64) -> Result<Vec<u8>> {
    use std::os::unix::fs::MetadataExt;
    let before = file.metadata()?;
    if before.len() > maximum {
        return Err(Error::Record);
    }
    let mut bytes = Vec::new();
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if bytes.len() as u64 > maximum
        || before.len() != bytes.len() as u64
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(Error::Record);
    }
    Ok(bytes)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
