//! Explicit, isolated Wine prefix directory management.
//!
//! Generic prefix lifecycle APIs never run Wine, Proton, UMU, `wineboot`, or a
//! game. P034's coordinator owns a narrow initialization process boundary,
//! guarded by an opaque runtime authorization that production cannot currently
//! construct. Prefix directory creation and Wine initialization remain separate
//! lifecycle steps.

use endfield_unix_runtime_selection::{
    AdapterPreference, MachineId, PinnedRuntimeReference, ProfileKey,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

mod initialization_coordinator;
mod linux_fs;
pub use initialization_coordinator::{
    InitializationAttempt, InitializationAttemptError, InitializationAttemptPhase,
    InitializationCoordinator, InitializationCoordinatorError, InitializationEvidenceSource,
    InitializationExecutionAuthorization, InitializationObservation, InitializationStart,
    InitializationStep, PreSpawnBlocker,
};
pub use linux_fs::LinuxPrefixFilesystem;
pub use linux_fs::backup;

pub const REGISTRY_SCHEMA_VERSION: u32 = 2;
pub const MAX_REGISTRY_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PREFIXES: usize = 1024;

#[derive(Debug, Error)]
pub enum PrefixError {
    #[error("invalid prefix identifier")]
    InvalidId,
    #[error("invalid prefix scope")]
    InvalidScope,
    #[error("runtime identity is invalid")]
    InvalidRuntime,
    #[error("prefix registry is malformed, corrupt, or uses an unsupported schema")]
    RegistryCorrupt,
    #[error("prefix registry uses unsupported schema version {0}")]
    UnsupportedSchemaVersion(u64),
    #[error("prefix registry changed; reload it and retry with the current revision")]
    RevisionConflict,
    #[error("prefix already exists with a different scope or runtime association")]
    IdentityConflict,
    #[error("a filesystem entry already occupies the managed destination")]
    DestinationCollision,
    #[error("prefix operation is incomplete and requires explicit recovery")]
    RecoveryRequired,
    #[error(
        "the incomplete prefix has no safe recovery path; preserve it and inspect the directory"
    )]
    RecoveryBlocked,
    #[error("managed prefix is missing")]
    Missing,
    #[error("prefix metadata is not registered")]
    NotRegistered,
    #[error("the confirmation path does not exactly match the managed target")]
    ConfirmationMismatch,
    #[error("the managed prefix is actively referenced by a visible same-user process")]
    ActiveUse,
    #[error("prefix use could not be checked safely; destructive removal was refused")]
    UsageUnknown,
    #[error("prefix initialization is unsupported by the validated runtime adapters")]
    InitializationUnsupported,
    #[error("prefix initialization approval does not match the exact plan")]
    ApprovalMismatch,
    #[error("prefix initialization intent is stale or does not match the approved plan")]
    InitializationIntentMismatch,
    #[error("prefix initialization plan fields do not match its plan ID")]
    PlanIntegrityMismatch,
    #[error("managed prefix filesystem state is unsafe or unverifiable")]
    Blocked,
    #[error("prefix ownership or lifecycle requirements are not satisfied")]
    InvalidState,
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("XDG storage paths are unavailable: {0}")]
    Storage(String),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrefixId(String);

impl PrefixId {
    pub fn new(value: impl Into<String>) -> Result<Self, PrefixError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 96
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        {
            return Err(PrefixError::InvalidId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrefixScope {
    Machine {
        machine: MachineId,
    },
    Profile {
        machine: MachineId,
        profile: ProfileKey,
    },
}

impl PrefixScope {
    pub fn machine(machine: MachineId) -> Self {
        Self::Machine { machine }
    }

    pub fn profile(machine: MachineId, profile: ProfileKey) -> Self {
        Self::Profile { machine, profile }
    }

    pub fn profile_id(&self) -> Option<&ProfileKey> {
        match self {
            Self::Machine { .. } => None,
            Self::Profile { profile, .. } => Some(profile),
        }
    }

    fn validate(&self) -> Result<(), PrefixError> {
        match self {
            Self::Machine { machine } => MachineId::from_digest(machine.as_str())
                .map(|_| ())
                .map_err(|_| PrefixError::InvalidScope),
            Self::Profile { machine, profile } => {
                MachineId::from_digest(machine.as_str()).map_err(|_| PrefixError::InvalidScope)?;
                ProfileKey::new(profile.as_str()).map_err(|_| PrefixError::InvalidScope)?;
                Ok(())
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefixRuntimeAssociation {
    pub runtime: PinnedRuntimeReference,
    pub selection_source: RuntimeSelectionSource,
    pub pin_id: String,
    pub pin_revision: u64,
    pub adapter_preference: Option<AdapterPreference>,
}

impl PrefixRuntimeAssociation {
    pub fn validate(&self) -> Result<(), PrefixError> {
        self.runtime
            .validate()
            .map_err(|_| PrefixError::InvalidRuntime)?;
        if self.runtime.family() != "wine" && self.runtime.family() != "proton" {
            return Err(PrefixError::InvalidRuntime);
        }
        if self.pin_id.is_empty()
            || self.pin_id.len() > 96
            || !self
                .pin_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
            || self.pin_revision == 0
        {
            return Err(PrefixError::InvalidRuntime);
        }
        let adapter_valid = match (self.adapter_preference, &self.runtime) {
            (None, _) => true,
            (Some(AdapterPreference::UmuProton), runtime) => runtime.family() == "proton",
            (Some(AdapterPreference::ManagedWine), PinnedRuntimeReference::Managed(runtime)) => {
                runtime.family == "wine"
            }
            (Some(AdapterPreference::ExternalWine), PinnedRuntimeReference::External(runtime)) => {
                runtime.family == "wine"
                    && matches!(
                        runtime.ownership.as_str(),
                        "external-user-managed" | "distribution-managed"
                    )
            }
            _ => false,
        };
        if !adapter_valid {
            return Err(PrefixError::InvalidRuntime);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSelectionSource {
    ProfilePin,
    MachinePin,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrefixLifecycleState {
    Planned,
    Created,
    Initialized,
    Incomplete,
    RecoveryRequired,
    Missing,
    Blocked,
}

/// Durable progress for an explicitly approved Wine initialization attempt.
/// `VerificationPending`, `Failed`, and `Interrupted` never mean initialized.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefixInitializationPhase {
    Starting,
    Running,
    VerificationPending,
    Failed,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefixInitializationIntent {
    pub plan_id: String,
    pub scope: PrefixScope,
    pub adapter_id: String,
    pub runtime_pin_id: String,
    pub runtime_pin_revision: u64,
    pub runtime_reference_sha256: String,
    pub prefix_generation: u64,
    pub filesystem_identity: FilesystemIdentity,
    pub phase: PrefixInitializationPhase,
    pub process_exit_code: Option<i32>,
    pub started_at_unix_seconds: u64,
    pub updated_at_unix_seconds: u64,
}

/// Hashes of the exact Wine 10.0 prefix artifacts checked through no-follow
/// descriptors. This is structural completion evidence, not game compatibility.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wine10PrefixCompletionEvidence {
    pub directory_identity: FilesystemIdentity,
    pub system_registry_sha256: String,
    pub user_registry_sha256: String,
    pub evidence_sha256: String,
}

impl PrefixInitializationIntent {
    fn validate(&self, record: &PrefixRecord) -> Result<(), PrefixError> {
        let phase_lifecycle_matches = matches!(
            (self.phase, &record.lifecycle),
            (
                PrefixInitializationPhase::Starting,
                PrefixLifecycleState::Incomplete
            ) | (
                PrefixInitializationPhase::Running,
                PrefixLifecycleState::Incomplete
            ) | (
                PrefixInitializationPhase::VerificationPending,
                PrefixLifecycleState::Incomplete
            ) | (
                PrefixInitializationPhase::Failed,
                PrefixLifecycleState::RecoveryRequired
            ) | (
                PrefixInitializationPhase::Interrupted,
                PrefixLifecycleState::RecoveryRequired
            )
        );
        let phase_exit_matches = match self.phase {
            PrefixInitializationPhase::Starting | PrefixInitializationPhase::Running => {
                self.process_exit_code.is_none()
            }
            PrefixInitializationPhase::VerificationPending => self.process_exit_code == Some(0),
            PrefixInitializationPhase::Failed => {
                self.process_exit_code.is_none_or(|code| code != 0)
            }
            PrefixInitializationPhase::Interrupted => {
                self.process_exit_code.is_none_or(|code| code == 0)
            }
        };
        if !self.plan_id.strip_prefix("sha256:").is_some_and(is_sha256)
            || self.adapter_id != "managed_wine"
            || self.scope != record.scope
            || self.runtime_pin_id != record.runtime.pin_id
            || self.runtime_pin_revision != record.runtime.pin_revision
            || !is_sha256(&self.runtime_reference_sha256)
            || self.runtime_reference_sha256 != runtime_reference_sha256(&record.runtime.runtime)?
            || !matches!(
                &record.runtime.runtime,
                PinnedRuntimeReference::Managed(runtime)
                    if runtime.family == "wine"
                        && runtime.lineage == "upstream-wine"
                        && runtime.build == "wine-10.0"
                        && runtime.trust
                            == endfield_unix_runtime_selection::Provenance::
                                IndependentlyAuthenticatedPublisher
            )
            || self.prefix_generation != record.generation
            || Some(self.filesystem_identity) != record.filesystem_identity
            || self.started_at_unix_seconds < record.creation_time_unix_seconds
            || self.updated_at_unix_seconds < self.started_at_unix_seconds
            || self.updated_at_unix_seconds > record.update_time_unix_seconds
            || !phase_lifecycle_matches
            || !phase_exit_matches
            || record.initialization_evidence.is_some()
        {
            return Err(PrefixError::RegistryCorrupt);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefixOwnership {
    EndfieldUnixManaged,
    ExternalReadOnly,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitializationEvidenceOutcome {
    CompletedAndVerified,
    Partial,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefixInitializationEvidence {
    pub adapter_id: String,
    pub runtime_reference_sha256: String,
    pub verification_evidence_sha256: String,
    pub outcome: InitializationEvidenceOutcome,
    pub observed_at_unix_seconds: u64,
}

impl PrefixInitializationEvidence {
    fn validate(&self, record: &PrefixRecord) -> Result<(), PrefixError> {
        let valid_adapter = !self.adapter_id.is_empty()
            && self.adapter_id.len() <= 96
            && self
                .adapter_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte));
        if !valid_adapter
            || !is_sha256(&self.runtime_reference_sha256)
            || !is_sha256(&self.verification_evidence_sha256)
            || self.runtime_reference_sha256 != runtime_reference_sha256(&record.runtime.runtime)?
            || self.observed_at_unix_seconds < record.creation_time_unix_seconds
            || self.observed_at_unix_seconds > record.update_time_unix_seconds
        {
            return Err(PrefixError::RegistryCorrupt);
        }
        match &record.lifecycle {
            PrefixLifecycleState::Initialized
                if self.outcome != InitializationEvidenceOutcome::CompletedAndVerified =>
            {
                Err(PrefixError::RegistryCorrupt)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesystemIdentity {
    pub device: u64,
    pub inode: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefixRecord {
    pub id: PrefixId,
    pub scope: PrefixScope,
    /// Relative to the canonical managed root; never accepted from the caller.
    pub path_identity: String,
    pub ownership: PrefixOwnership,
    pub owner_uid: u32,
    pub runtime: PrefixRuntimeAssociation,
    pub lifecycle: PrefixLifecycleState,
    pub filesystem_identity: Option<FilesystemIdentity>,
    pub creation_time_unix_seconds: u64,
    pub update_time_unix_seconds: u64,
    pub generation: u64,
    pub initialization_evidence: Option<PrefixInitializationEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initialization_intent: Option<PrefixInitializationIntent>,
}

impl PrefixRecord {
    fn validate(&self) -> Result<(), PrefixError> {
        PrefixId::new(self.id.as_str())?;
        self.scope.validate()?;
        self.runtime.validate()?;
        if self.path_identity != format!("data/{}", self.id.as_str())
            || self.ownership != PrefixOwnership::EndfieldUnixManaged
            || self.generation == 0
            || self.update_time_unix_seconds < self.creation_time_unix_seconds
        {
            return Err(PrefixError::RegistryCorrupt);
        }
        match &self.lifecycle {
            PrefixLifecycleState::Initialized if self.initialization_evidence.is_none() => {
                return Err(PrefixError::RegistryCorrupt);
            }
            PrefixLifecycleState::Created if self.initialization_evidence.is_some() => {
                return Err(PrefixError::RegistryCorrupt);
            }
            PrefixLifecycleState::Planned
            | PrefixLifecycleState::Missing
            | PrefixLifecycleState::Blocked
                if self.initialization_evidence.is_some() =>
            {
                return Err(PrefixError::RegistryCorrupt);
            }
            _ => {}
        }
        if self
            .initialization_evidence
            .as_ref()
            .is_some_and(|evidence| evidence.validate(self).is_err())
        {
            return Err(PrefixError::RegistryCorrupt);
        }
        if self
            .initialization_intent
            .as_ref()
            .is_some_and(|intent| intent.validate(self).is_err())
        {
            return Err(PrefixError::RegistryCorrupt);
        }
        if self.initialization_intent.is_some()
            && !matches!(
                self.lifecycle,
                PrefixLifecycleState::Incomplete | PrefixLifecycleState::RecoveryRequired
            )
        {
            return Err(PrefixError::RegistryCorrupt);
        }
        if matches!(
            self.lifecycle,
            PrefixLifecycleState::Created | PrefixLifecycleState::Initialized
        ) && self.filesystem_identity.is_none()
        {
            return Err(PrefixError::RegistryCorrupt);
        }
        if matches!(
            self.lifecycle,
            PrefixLifecycleState::Planned
                | PrefixLifecycleState::Missing
                | PrefixLifecycleState::Blocked
        ) {
            return Err(PrefixError::RegistryCorrupt);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefixRegistry {
    pub schema_version: u32,
    pub revision: u64,
    pub records: Vec<PrefixRecord>,
    checksum_sha256: String,
}

#[derive(Serialize)]
struct RegistryPayload<'a> {
    schema_version: u32,
    revision: u64,
    records: &'a [PrefixRecord],
}

impl Default for PrefixRegistry {
    fn default() -> Self {
        let mut registry = Self {
            schema_version: REGISTRY_SCHEMA_VERSION,
            revision: 0,
            records: Vec::new(),
            checksum_sha256: String::new(),
        };
        registry.refresh_checksum();
        registry
    }
}

impl PrefixRegistry {
    fn payload(&self) -> RegistryPayload<'_> {
        RegistryPayload {
            schema_version: self.schema_version,
            revision: self.revision,
            records: &self.records,
        }
    }

    fn refresh_checksum(&mut self) {
        let bytes = serde_json::to_vec(&self.payload()).expect("validated registry serializes");
        self.checksum_sha256 = format!("{:x}", Sha256::digest(bytes));
    }

    fn validate_schema(&self, schema_version: u32) -> Result<(), PrefixError> {
        if self.schema_version != schema_version || self.records.len() > MAX_PREFIXES {
            return Err(PrefixError::RegistryCorrupt);
        }
        for (index, record) in self.records.iter().enumerate() {
            record.validate()?;
            if self.records[..index]
                .iter()
                .any(|previous| previous.id == record.id)
            {
                return Err(PrefixError::RegistryCorrupt);
            }
        }
        let bytes =
            serde_json::to_vec(&self.payload()).map_err(|_| PrefixError::RegistryCorrupt)?;
        let expected = format!("{:x}", Sha256::digest(bytes));
        if self.checksum_sha256 != expected {
            return Err(PrefixError::RegistryCorrupt);
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), PrefixError> {
        self.validate_schema(REGISTRY_SCHEMA_VERSION)
    }

    pub fn record(&self, id: &PrefixId) -> Option<&PrefixRecord> {
        self.records.iter().find(|record| &record.id == id)
    }

    fn encode(&mut self) -> Result<Vec<u8>, PrefixError> {
        self.refresh_checksum();
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| PrefixError::RegistryCorrupt)?;
        if bytes.len() > MAX_REGISTRY_BYTES {
            return Err(PrefixError::RegistryCorrupt);
        }
        Ok(bytes)
    }

    fn decode(bytes: &[u8]) -> Result<Self, PrefixError> {
        if bytes.len() > MAX_REGISTRY_BYTES {
            return Err(PrefixError::RegistryCorrupt);
        }
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| PrefixError::RegistryCorrupt)?;
        let schema_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or(PrefixError::RegistryCorrupt)?;
        if schema_version != 1 && schema_version != u64::from(REGISTRY_SCHEMA_VERSION) {
            return Err(PrefixError::UnsupportedSchemaVersion(schema_version));
        }
        let mut registry: Self =
            serde_json::from_value(value).map_err(|_| PrefixError::RegistryCorrupt)?;
        registry.validate_schema(schema_version as u32)?;
        if schema_version == 1 {
            // The optional initialization-intent field is absent in v1 records.
            // Validate the legacy checksum first, then migrate in memory; the next
            // authorized registry mutation publishes schema v2 atomically.
            registry.schema_version = REGISTRY_SCHEMA_VERSION;
            registry.refresh_checksum();
        }
        Ok(registry)
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn runtime_reference_sha256(reference: &PinnedRuntimeReference) -> Result<String, PrefixError> {
    let bytes = serde_json::to_vec(reference).map_err(|_| PrefixError::RegistryCorrupt)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixOperationPlan {
    pub id: PrefixId,
    pub scope: PrefixScope,
    pub destination: PathBuf,
    pub expected_registry_revision: u64,
    pub free_bytes: Option<u64>,
    pub runtime: PrefixRuntimeAssociation,
    pub requires_wine_execution: bool,
    pub already_created: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeValidation {
    Valid,
    Stale(String),
    NotAssessed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixValidationReport {
    pub record: PrefixRecord,
    pub observed_lifecycle: PrefixLifecycleState,
    pub runtime: RuntimeValidation,
    pub registry_revision: u64,
    pub initialization_evidence: PrefixInitializationEvidenceStatus,
}

/// P028 stores the result claim but has no runtime-specific completion
/// verifier yet. A stored `CompletedAndVerified` value is therefore reported
/// as an unverified claim until a verifier can bind it to observed effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefixInitializationEvidenceStatus {
    Absent,
    CompletedClaimUnverified,
    PartialClaim,
    FailedClaim,
    IntentStarting,
    IntentRunning,
    IntentVerificationPending,
    IntentFailed,
    IntentInterrupted,
}

/// Outcome of a read-only initialization assessment. `Unsupported` means the
/// exact prefix/runtime facts were inspected, but no verified initializer is
/// available. `Blocked` means at least one safety prerequisite also failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefixInitializationPlanState {
    Blocked,
    Unsupported,
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefixInitializationBlocker {
    PrefixNotCreated,
    PrefixIdentityUnknown,
    PrefixIdentityChanged,
    PrefixContainsData,
    PrefixActive,
    PrefixUseUnknown,
    RuntimeAssociationMismatch,
    RuntimeStale,
    RecoveryPending,
    RecoveryUnknown,
    AdapterUnsupported,
    InitializerUnverified,
    ExistingInitializationEvidenceUnverified,
    InitializationInProgress,
    InitializationInterrupted,
    ExecutableIdentityNotBound,
    InitializerChildOwnershipUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefixInitializationStrategy {
    WineBootInitialize,
    UmuManagedPrefixSetup,
    Unsupported,
}

/// A redacted, deterministic initialization review. It contains no executable
/// path, raw runtime reference, or command string. P028 does not execute plans.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PrefixInitializationPlan {
    pub schema_version: u32,
    pub plan_id: String,
    pub prefix_id: String,
    pub scope: PrefixScope,
    pub registry_revision: u64,
    pub prefix_generation: u64,
    pub filesystem_identity: Option<FilesystemIdentity>,
    pub runtime_pin_id: String,
    pub runtime_pin_revision: u64,
    pub runtime_reference_sha256: String,
    pub adapter_id: String,
    pub strategy: PrefixInitializationStrategy,
    pub state: PrefixInitializationPlanState,
    pub blockers: Vec<PrefixInitializationBlocker>,
    pub approval_required: bool,
    pub execution_supported: bool,
    pub expected_effects: Vec<&'static str>,
}

#[derive(Serialize)]
struct PrefixInitializationPlanIdentity<'a> {
    schema_version: u32,
    prefix_id: &'a str,
    scope: &'a PrefixScope,
    registry_revision: u64,
    prefix_generation: u64,
    filesystem_identity: Option<FilesystemIdentity>,
    runtime_pin_id: &'a str,
    runtime_pin_revision: u64,
    runtime_reference_sha256: &'a str,
    adapter_id: &'a str,
    strategy: PrefixInitializationStrategy,
    state: PrefixInitializationPlanState,
    blockers: &'a [PrefixInitializationBlocker],
    approval_required: bool,
    execution_supported: bool,
    expected_effects: &'a [&'static str],
}

impl PrefixInitializationPlan {
    fn validate_integrity(&self) -> Result<(), PrefixError> {
        let identity = PrefixInitializationPlanIdentity {
            schema_version: self.schema_version,
            prefix_id: &self.prefix_id,
            scope: &self.scope,
            registry_revision: self.registry_revision,
            prefix_generation: self.prefix_generation,
            filesystem_identity: self.filesystem_identity,
            runtime_pin_id: &self.runtime_pin_id,
            runtime_pin_revision: self.runtime_pin_revision,
            runtime_reference_sha256: &self.runtime_reference_sha256,
            adapter_id: &self.adapter_id,
            strategy: self.strategy,
            state: self.state,
            blockers: &self.blockers,
            approval_required: self.approval_required,
            execution_supported: self.execution_supported,
            expected_effects: &self.expected_effects,
        };
        let bytes =
            serde_json::to_vec(&identity).map_err(|_| PrefixError::PlanIntegrityMismatch)?;
        if self.schema_version != 2 || self.plan_id != format!("sha256:{:x}", Sha256::digest(bytes))
        {
            return Err(PrefixError::PlanIntegrityMismatch);
        }
        Ok(())
    }

    /// Verify a caller's confirmation against the exact plan. Current runtime
    /// contracts remain unsupported, so the production plan builder never
    /// issues an approval today.
    pub fn approve(
        &self,
        exact_plan_id: &str,
    ) -> Result<PrefixInitializationApproval, PrefixError> {
        self.validate_integrity()?;
        if exact_plan_id != self.plan_id {
            return Err(PrefixError::ApprovalMismatch);
        }
        if !self.approval_required || self.state == PrefixInitializationPlanState::Blocked {
            return Err(PrefixError::InvalidState);
        }
        if !self.execution_supported {
            return Err(PrefixError::InitializationUnsupported);
        }
        if self.state != PrefixInitializationPlanState::Ready {
            return Err(PrefixError::InvalidState);
        }
        Ok(PrefixInitializationApproval {
            plan_id: self.plan_id.clone(),
        })
    }

    #[cfg(test)]
    pub(crate) fn synthetic_approval_for_test(
        &self,
        exact_plan_id: &str,
    ) -> Result<SyntheticInitializationApproval, PrefixError> {
        self.validate_integrity()?;
        if self.plan_id != exact_plan_id {
            return Err(PrefixError::ApprovalMismatch);
        }
        if !self.approval_required
            || self.execution_supported
            || self.state != PrefixInitializationPlanState::Unsupported
            || self.blockers.is_empty()
            || self.blockers.iter().any(|blocker| {
                !matches!(
                    blocker,
                    PrefixInitializationBlocker::ExecutableIdentityNotBound
                        | PrefixInitializationBlocker::InitializerChildOwnershipUnavailable
                )
            })
            || !self
                .blockers
                .contains(&PrefixInitializationBlocker::ExecutableIdentityNotBound)
            || !self
                .blockers
                .contains(&PrefixInitializationBlocker::InitializerChildOwnershipUnavailable)
        {
            return Err(PrefixError::InvalidState);
        }
        Ok(SyntheticInitializationApproval {
            plan_id: self.plan_id.clone(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefixInitializationProcessOutcome {
    /// Process creation failed. Conservatively preserve the write-ahead intent.
    SpawnFailed,
    ExitedZero,
    ExitedNonzero(i32),
    /// Timeout, cancellation, signal, or lost observation. Completion is unknown.
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixInitializationApproval {
    plan_id: String,
}

impl PrefixInitializationApproval {
    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SyntheticInitializationApproval {
    plan_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixInspection {
    pub record: PrefixRecord,
    pub observed_lifecycle: PrefixLifecycleState,
    pub path: PathBuf,
    pub runtime: RuntimeValidation,
}

pub trait PrefixGuard: Send {}

/// Filesystem boundary for prefix storage. Tests and embedders can provide a
/// deterministic adapter; the production implementation uses Linux dirfds,
/// no-follow opens, private metadata, and atomic replacement.
pub trait PrefixFilesystem: Send + Sync {
    fn root_path(&self) -> &Path;
    fn prepare(&self, create: bool) -> Result<(), PrefixError>;
    fn preflight_destination(&self, id: &PrefixId) -> Result<(), PrefixError>;
    fn lock_registry(&self) -> Result<Box<dyn PrefixGuard>, PrefixError>;
    fn lock_prefix(&self, id: &PrefixId) -> Result<Box<dyn PrefixGuard>, PrefixError>;
    fn load_registry_bytes(&self) -> Result<Option<Vec<u8>>, PrefixError>;
    fn save_registry_bytes(
        &self,
        expected_current: Option<&[u8]>,
        replacement: &[u8],
    ) -> Result<(), PrefixError>;
    fn inspect_directory(&self, id: &PrefixId) -> Result<Option<FilesystemIdentity>, PrefixError>;
    fn directory_is_empty(&self, id: &PrefixId) -> Result<bool, PrefixError>;
    fn create_directory(&self, id: &PrefixId) -> Result<FilesystemIdentity, PrefixError>;
    fn remove_directory(
        &self,
        id: &PrefixId,
        expected: FilesystemIdentity,
    ) -> Result<(), PrefixError>;
    fn prefix_in_use(&self, id: &PrefixId) -> Result<Option<bool>, PrefixError>;
    fn target_path(&self, id: &PrefixId) -> PathBuf;
    fn free_bytes(&self) -> Option<u64>;
    fn backup_recovery_pending(&self, _id: &PrefixId) -> Result<bool, PrefixError> {
        Ok(false)
    }
    fn verify_wine10_prefix(
        &self,
        _id: &PrefixId,
        _expected: FilesystemIdentity,
    ) -> Result<Wine10PrefixCompletionEvidence, PrefixError> {
        Err(PrefixError::InitializationUnsupported)
    }
}

struct InitializationTransition {
    allowed_phases: &'static [PrefixInitializationPhase],
    next_phase: PrefixInitializationPhase,
    lifecycle: PrefixLifecycleState,
    /// `None` preserves the old observation; `Some(None)` records an unknown
    /// result such as spawn failure, and `Some(Some(code))` records an exit.
    process_exit_code: Option<Option<i32>>,
}

#[derive(Clone)]
pub struct PrefixStore {
    fs: Arc<dyn PrefixFilesystem>,
}

impl PrefixStore {
    pub fn for_app(paths: &endfield_unix_storage::AppPaths) -> Self {
        Self::with_filesystem(Arc::new(LinuxPrefixFilesystem::new(paths.data_dir())))
    }

    pub fn with_filesystem(filesystem: Arc<dyn PrefixFilesystem>) -> Self {
        Self { fs: filesystem }
    }

    pub fn target_path(&self, id: &PrefixId) -> PathBuf {
        self.fs.target_path(id)
    }

    pub fn list(
        &self,
        mut validate_runtime: impl FnMut(&PinnedRuntimeReference) -> Result<(), String>,
    ) -> Result<(PrefixRegistry, Vec<PrefixInspection>), PrefixError> {
        self.fs.prepare(false)?;
        let snapshot = self.load_snapshot()?;
        let mut items = Vec::with_capacity(snapshot.registry.records.len());
        for record in &snapshot.registry.records {
            let (lifecycle, runtime) = self.inspect_record(record, &mut validate_runtime)?;
            items.push(PrefixInspection {
                record: record.clone(),
                observed_lifecycle: lifecycle,
                path: self.fs.target_path(&record.id),
                runtime,
            });
        }
        Ok((snapshot.registry, items))
    }

    pub fn show(
        &self,
        id: &PrefixId,
        mut validate_runtime: impl FnMut(&PinnedRuntimeReference) -> Result<(), String>,
    ) -> Result<PrefixInspection, PrefixError> {
        self.fs.prepare(false)?;
        let snapshot = self.load_snapshot()?;
        let record = snapshot
            .registry
            .record(id)
            .ok_or(PrefixError::NotRegistered)?;
        let (lifecycle, runtime) = self.inspect_record(record, &mut validate_runtime)?;
        Ok(PrefixInspection {
            record: record.clone(),
            observed_lifecycle: lifecycle,
            path: self.fs.target_path(id),
            runtime,
        })
    }

    pub fn validate(
        &self,
        id: &PrefixId,
        mut validate_runtime: impl FnMut(&PinnedRuntimeReference) -> Result<(), String>,
    ) -> Result<PrefixValidationReport, PrefixError> {
        self.fs.prepare(false)?;
        let snapshot = self.load_snapshot()?;
        let record = snapshot
            .registry
            .record(id)
            .ok_or(PrefixError::NotRegistered)?;
        let (observed_lifecycle, runtime) = self.inspect_record(record, &mut validate_runtime)?;
        Ok(PrefixValidationReport {
            record: record.clone(),
            observed_lifecycle,
            runtime,
            registry_revision: snapshot.registry.revision,
            initialization_evidence: match (
                record.initialization_evidence.as_ref().map(|e| e.outcome),
                record
                    .initialization_intent
                    .as_ref()
                    .map(|intent| intent.phase),
            ) {
                (None, Some(PrefixInitializationPhase::Starting)) => {
                    PrefixInitializationEvidenceStatus::IntentStarting
                }
                (None, Some(PrefixInitializationPhase::Running)) => {
                    PrefixInitializationEvidenceStatus::IntentRunning
                }
                (None, Some(PrefixInitializationPhase::VerificationPending)) => {
                    PrefixInitializationEvidenceStatus::IntentVerificationPending
                }
                (None, Some(PrefixInitializationPhase::Failed)) => {
                    PrefixInitializationEvidenceStatus::IntentFailed
                }
                (None, Some(PrefixInitializationPhase::Interrupted)) => {
                    PrefixInitializationEvidenceStatus::IntentInterrupted
                }
                (None, None) => PrefixInitializationEvidenceStatus::Absent,
                (Some(InitializationEvidenceOutcome::CompletedAndVerified), _) => {
                    PrefixInitializationEvidenceStatus::CompletedClaimUnverified
                }
                (Some(InitializationEvidenceOutcome::Partial), _) => {
                    PrefixInitializationEvidenceStatus::PartialClaim
                }
                (Some(InitializationEvidenceOutcome::Failed), _) => {
                    PrefixInitializationEvidenceStatus::FailedClaim
                }
            },
        })
    }

    /// Produce a read-only initialization assessment for one exact Created
    /// prefix and runtime association. The plan records all observations and
    /// remains unsupported until P028 has a verified initializer, bounded
    /// supervision, and completion verifier.
    pub fn plan_initialization(
        &self,
        id: &PrefixId,
        expected_registry_revision: u64,
        expected_runtime: &PrefixRuntimeAssociation,
        additional_recovery_pending: Option<bool>,
        mut validate_runtime: impl FnMut(&PinnedRuntimeReference) -> Result<(), String>,
    ) -> Result<PrefixInitializationPlan, PrefixError> {
        expected_runtime.validate()?;
        self.fs.prepare(false)?;
        let snapshot = self.load_snapshot()?;
        if snapshot.registry.revision != expected_registry_revision {
            return Err(PrefixError::RevisionConflict);
        }
        let record = snapshot
            .registry
            .record(id)
            .ok_or(PrefixError::NotRegistered)?;
        let mut blockers = Vec::new();

        if record.lifecycle != PrefixLifecycleState::Created {
            blockers.push(PrefixInitializationBlocker::PrefixNotCreated);
        }
        if record.ownership != PrefixOwnership::EndfieldUnixManaged
            || record.owner_uid != current_uid()
        {
            blockers.push(PrefixInitializationBlocker::PrefixIdentityUnknown);
        }
        if &record.runtime != expected_runtime {
            blockers.push(PrefixInitializationBlocker::RuntimeAssociationMismatch);
        }
        let runtime_valid = validate_runtime(&record.runtime.runtime).is_ok();
        if !runtime_valid {
            blockers.push(PrefixInitializationBlocker::RuntimeStale);
        }

        let observed_identity = self.fs.inspect_directory(id)?;
        if observed_identity.is_none() {
            blockers.push(PrefixInitializationBlocker::PrefixIdentityUnknown);
        } else if observed_identity != record.filesystem_identity {
            blockers.push(PrefixInitializationBlocker::PrefixIdentityChanged);
        }
        match self.fs.directory_is_empty(id) {
            Ok(true) => {}
            Ok(false) => blockers.push(PrefixInitializationBlocker::PrefixContainsData),
            Err(_) => blockers.push(PrefixInitializationBlocker::PrefixIdentityUnknown),
        }
        match self.fs.prefix_in_use(id) {
            Ok(Some(false)) => {}
            Ok(Some(true)) => blockers.push(PrefixInitializationBlocker::PrefixActive),
            Ok(None) | Err(_) => blockers.push(PrefixInitializationBlocker::PrefixUseUnknown),
        }
        match additional_recovery_pending {
            Some(true) => blockers.push(PrefixInitializationBlocker::RecoveryPending),
            Some(false) => {}
            None => blockers.push(PrefixInitializationBlocker::RecoveryUnknown),
        }
        match self.fs.backup_recovery_pending(id) {
            Ok(true) => blockers.push(PrefixInitializationBlocker::RecoveryPending),
            Ok(false) => {}
            Err(_) => blockers.push(PrefixInitializationBlocker::RecoveryUnknown),
        }
        if matches!(
            record.lifecycle,
            PrefixLifecycleState::Incomplete | PrefixLifecycleState::RecoveryRequired
        ) {
            blockers.push(PrefixInitializationBlocker::RecoveryPending);
        }
        if record.lifecycle == PrefixLifecycleState::Initialized
            && record.initialization_evidence.is_some()
        {
            blockers.push(PrefixInitializationBlocker::ExistingInitializationEvidenceUnverified);
        }
        if let Some(intent) = &record.initialization_intent {
            blockers.push(match intent.phase {
                PrefixInitializationPhase::Starting
                | PrefixInitializationPhase::Running
                | PrefixInitializationPhase::VerificationPending => {
                    PrefixInitializationBlocker::InitializationInProgress
                }
                PrefixInitializationPhase::Failed | PrefixInitializationPhase::Interrupted => {
                    PrefixInitializationBlocker::InitializationInterrupted
                }
            });
        }

        let (adapter_id, strategy) =
            match (record.runtime.adapter_preference, &record.runtime.runtime) {
                (
                    Some(AdapterPreference::ManagedWine),
                    PinnedRuntimeReference::Managed(runtime),
                ) if runtime.family == "wine" => (
                    "managed_wine".to_owned(),
                    PrefixInitializationStrategy::WineBootInitialize,
                ),
                (
                    Some(AdapterPreference::ExternalWine),
                    PinnedRuntimeReference::External(runtime),
                ) if runtime.family == "wine" => (
                    "external_wine".to_owned(),
                    PrefixInitializationStrategy::WineBootInitialize,
                ),
                (Some(AdapterPreference::UmuProton), runtime) if runtime.family() == "proton" => (
                    "umu_proton".to_owned(),
                    PrefixInitializationStrategy::UmuManagedPrefixSetup,
                ),
                _ => {
                    blockers.push(PrefixInitializationBlocker::AdapterUnsupported);
                    (
                        "unsupported".to_owned(),
                        PrefixInitializationStrategy::Unsupported,
                    )
                }
            };

        let verified_wine10_contract = matches!(
            (&record.runtime.runtime, record.runtime.adapter_preference),
            (
                PinnedRuntimeReference::Managed(runtime),
                Some(AdapterPreference::ManagedWine)
            ) if runtime.family == "wine"
                && runtime.lineage == "upstream-wine"
                && runtime.build == "wine-10.0"
                && runtime.trust
                    == endfield_unix_runtime_selection::Provenance::
                        IndependentlyAuthenticatedPublisher
        ) && runtime_valid;
        if verified_wine10_contract {
            // Wine 10.0's documented `wineboot.exe --init` semantics are
            // source-verified, but the selected launcher is not yet bound to
            // the audited inode at exec and helper lifetime is not owned.
            blockers.push(PrefixInitializationBlocker::ExecutableIdentityNotBound);
            blockers.push(PrefixInitializationBlocker::InitializerChildOwnershipUnavailable);
        } else {
            blockers.push(PrefixInitializationBlocker::InitializerUnverified);
        }
        blockers.sort();
        blockers.dedup();
        let state = if blockers.iter().all(|blocker| {
            matches!(
                blocker,
                PrefixInitializationBlocker::InitializerUnverified
                    | PrefixInitializationBlocker::ExecutableIdentityNotBound
                    | PrefixInitializationBlocker::InitializerChildOwnershipUnavailable
            )
        }) {
            PrefixInitializationPlanState::Unsupported
        } else {
            PrefixInitializationPlanState::Blocked
        };
        let runtime_reference_sha256 = runtime_reference_sha256(&record.runtime.runtime)?;
        let expected_effects: Vec<&'static str> = match strategy {
            PrefixInitializationStrategy::WineBootInitialize => vec![
                "The selected Wine runtime would write registry and system files into this prefix.",
                "The initializer may start wineserver-related helper processes.",
                "Wine may also write user-scoped cache or configuration data outside this prefix.",
            ],
            PrefixInitializationStrategy::UmuManagedPrefixSetup => vec![
                "UMU/Proton may create or mutate prefix contents and runtime caches.",
                "UMU may acquire or update Steam Linux Runtime components over the network.",
            ],
            PrefixInitializationStrategy::Unsupported => {
                vec!["No initialization command will be started."]
            }
        };
        let identity = PrefixInitializationPlanIdentity {
            schema_version: 2,
            prefix_id: id.as_str(),
            scope: &record.scope,
            registry_revision: snapshot.registry.revision,
            prefix_generation: record.generation,
            filesystem_identity: observed_identity,
            runtime_pin_id: &record.runtime.pin_id,
            runtime_pin_revision: record.runtime.pin_revision,
            runtime_reference_sha256: &runtime_reference_sha256,
            adapter_id: &adapter_id,
            strategy,
            state,
            blockers: &blockers,
            approval_required: true,
            execution_supported: false,
            expected_effects: &expected_effects,
        };
        let bytes = serde_json::to_vec(&identity).map_err(|_| PrefixError::RegistryCorrupt)?;
        let plan_id = format!("sha256:{:x}", Sha256::digest(bytes));
        Ok(PrefixInitializationPlan {
            schema_version: 2,
            plan_id,
            prefix_id: id.as_str().to_owned(),
            scope: record.scope.clone(),
            registry_revision: snapshot.registry.revision,
            prefix_generation: record.generation,
            filesystem_identity: observed_identity,
            runtime_pin_id: record.runtime.pin_id.clone(),
            runtime_pin_revision: record.runtime.pin_revision,
            runtime_reference_sha256,
            adapter_id,
            strategy,
            state,
            blockers,
            approval_required: true,
            execution_supported: false,
            expected_effects,
        })
    }

    pub fn plan(
        &self,
        id: PrefixId,
        scope: PrefixScope,
        runtime: PrefixRuntimeAssociation,
        expected_revision: Option<u64>,
    ) -> Result<PrefixOperationPlan, PrefixError> {
        scope.validate()?;
        runtime.validate()?;
        self.fs.prepare(false)?;
        self.fs.preflight_destination(&id)?;
        let snapshot = self.load_snapshot()?;
        if let Some(expected) = expected_revision {
            if expected != snapshot.registry.revision {
                return Err(PrefixError::RevisionConflict);
            }
        }
        let existing = snapshot.registry.record(&id);
        let mut already_created = false;
        if let Some(record) = existing {
            if record.scope != scope || record.runtime != runtime {
                return Err(PrefixError::IdentityConflict);
            }
            if record.ownership != PrefixOwnership::EndfieldUnixManaged
                || record.owner_uid != current_uid()
            {
                return Err(PrefixError::Blocked);
            }
            if record.lifecycle != PrefixLifecycleState::Created {
                return Err(PrefixError::InvalidState);
            }
            let Some(identity) = self.fs.inspect_directory(&id)? else {
                return Err(PrefixError::Missing);
            };
            if Some(identity) != record.filesystem_identity {
                return Err(PrefixError::Blocked);
            }
            already_created = true;
        } else if self.fs.inspect_directory(&id)?.is_some() {
            return Err(PrefixError::DestinationCollision);
        }
        Ok(PrefixOperationPlan {
            id: id.clone(),
            scope,
            destination: self.fs.target_path(&id),
            expected_registry_revision: snapshot.registry.revision,
            free_bytes: self.fs.free_bytes(),
            runtime,
            requires_wine_execution: false,
            already_created,
        })
    }

    pub fn create(
        &self,
        id: PrefixId,
        scope: PrefixScope,
        runtime: PrefixRuntimeAssociation,
        expected_revision: u64,
    ) -> Result<(PrefixRecord, u64), PrefixError> {
        scope.validate()?;
        runtime.validate()?;
        self.fs.prepare(false)?;
        self.fs.preflight_destination(&id)?;
        self.fs.prepare(true)?;
        let _registry_guard = self.fs.lock_registry()?;
        let _prefix_guard = self.fs.lock_prefix(&id)?;
        if self.fs.backup_recovery_pending(&id)? {
            return Err(PrefixError::RecoveryRequired);
        }
        let snapshot = self.load_snapshot()?;
        if let Some(record) = snapshot.registry.record(&id) {
            if record.ownership != PrefixOwnership::EndfieldUnixManaged
                || record.owner_uid != current_uid()
            {
                return Err(PrefixError::Blocked);
            }
            if record.scope == scope
                && record.runtime == runtime
                && record.lifecycle == PrefixLifecycleState::Created
                && self.fs.inspect_directory(&id)? == record.filesystem_identity
            {
                return Ok((record.clone(), snapshot.registry.revision));
            }
            return Err(PrefixError::IdentityConflict);
        }
        if snapshot.registry.revision != expected_revision {
            return Err(PrefixError::RevisionConflict);
        }
        if snapshot.registry.records.len() >= MAX_PREFIXES {
            return Err(PrefixError::RegistryCorrupt);
        }
        if self.fs.inspect_directory(&id)?.is_some() {
            return Err(PrefixError::DestinationCollision);
        }
        self.fs.preflight_destination(&id)?;

        let created_at = now();
        let mut journal = PrefixRecord {
            id: id.clone(),
            scope,
            path_identity: format!("data/{}", id.as_str()),
            ownership: PrefixOwnership::EndfieldUnixManaged,
            owner_uid: current_uid(),
            runtime,
            lifecycle: PrefixLifecycleState::RecoveryRequired,
            filesystem_identity: None,
            creation_time_unix_seconds: created_at,
            update_time_unix_seconds: created_at,
            generation: snapshot.registry.revision.saturating_add(1),
            initialization_evidence: None,
            initialization_intent: None,
        };
        let mut registry = snapshot.registry;
        registry.revision = registry.revision.saturating_add(1);
        registry.records.push(journal.clone());
        let journal_bytes = registry.encode()?;
        self.fs
            .save_registry_bytes(snapshot.bytes.as_deref(), &journal_bytes)?;

        let identity = self.fs.create_directory(&id)?;
        journal.lifecycle = PrefixLifecycleState::Created;
        journal.filesystem_identity = Some(identity);
        journal.update_time_unix_seconds = now();
        registry.revision = registry.revision.saturating_add(1);
        let record = registry
            .records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or(PrefixError::RegistryCorrupt)?;
        *record = journal.clone();
        let final_bytes = registry.encode()?;
        self.fs
            .save_registry_bytes(Some(&journal_bytes), &final_bytes)?;
        Ok((journal, registry.revision))
    }

    pub fn recover(
        &self,
        id: &PrefixId,
        expected_revision: u64,
    ) -> Result<(PrefixRecord, u64), PrefixError> {
        self.fs.prepare(true)?;
        let _registry_guard = self.fs.lock_registry()?;
        let _prefix_guard = self.fs.lock_prefix(id)?;
        if self.fs.backup_recovery_pending(id)? {
            return Err(PrefixError::RecoveryRequired);
        }
        let snapshot = self.load_snapshot()?;
        if snapshot.registry.revision != expected_revision {
            return Err(PrefixError::RevisionConflict);
        }
        let Some(current) = snapshot.registry.record(id).cloned() else {
            return Err(PrefixError::NotRegistered);
        };
        if current.ownership != PrefixOwnership::EndfieldUnixManaged
            || current.owner_uid != current_uid()
        {
            return Err(PrefixError::Blocked);
        }
        if current.lifecycle != PrefixLifecycleState::RecoveryRequired
            && current.lifecycle != PrefixLifecycleState::Incomplete
        {
            return Err(PrefixError::InvalidState);
        }
        if current.initialization_evidence.is_some() || current.initialization_intent.is_some() {
            return Err(PrefixError::RecoveryBlocked);
        }
        let Some(identity) = self.fs.inspect_directory(id)? else {
            return Err(PrefixError::RecoveryBlocked);
        };
        if !self.fs.directory_is_empty(id)?
            || current
                .filesystem_identity
                .is_some_and(|expected| expected != identity)
        {
            return Err(PrefixError::RecoveryBlocked);
        }
        let mut registry = snapshot.registry;
        registry.revision = registry.revision.saturating_add(1);
        let record = registry
            .records
            .iter_mut()
            .find(|record| &record.id == id)
            .ok_or(PrefixError::RegistryCorrupt)?;
        record.lifecycle = PrefixLifecycleState::Created;
        record.filesystem_identity = Some(identity);
        record.update_time_unix_seconds = now();
        let record = record.clone();
        let bytes = registry.encode()?;
        self.fs
            .save_registry_bytes(snapshot.bytes.as_deref(), &bytes)?;
        Ok((record, registry.revision))
    }

    /// Remove only the registry entry. Prefix bytes remain under the managed
    /// root; a later create refuses to adopt the unregistered directory.
    pub fn unregister(&self, id: &PrefixId, expected_revision: u64) -> Result<u64, PrefixError> {
        self.fs.prepare(true)?;
        let _registry_guard = self.fs.lock_registry()?;
        let _prefix_guard = self.fs.lock_prefix(id)?;
        if self.fs.backup_recovery_pending(id)? {
            return Err(PrefixError::RecoveryRequired);
        }
        let snapshot = self.load_snapshot()?;
        if snapshot.registry.revision != expected_revision {
            return Err(PrefixError::RevisionConflict);
        }
        if snapshot
            .registry
            .record(id)
            .is_some_and(|record| record.initialization_intent.is_some())
        {
            return Err(PrefixError::RecoveryRequired);
        }
        let mut registry = snapshot.registry;
        let original_len = registry.records.len();
        registry.records.retain(|record| &record.id != id);
        if registry.records.len() == original_len {
            return Ok(registry.revision);
        }
        registry.revision = registry.revision.saturating_add(1);
        let bytes = registry.encode()?;
        self.fs
            .save_registry_bytes(snapshot.bytes.as_deref(), &bytes)?;
        Ok(registry.revision)
    }

    /// Delete data only after the caller confirms the exact managed target.
    /// The registry is journaled as RecoveryRequired before any unlink.
    pub fn delete_data(
        &self,
        id: &PrefixId,
        expected_revision: u64,
        confirmed_path: &Path,
    ) -> Result<u64, PrefixError> {
        if confirmed_path != self.fs.target_path(id) {
            return Err(PrefixError::ConfirmationMismatch);
        }
        self.fs.prepare(true)?;
        let _registry_guard = self.fs.lock_registry()?;
        let _prefix_guard = self.fs.lock_prefix(id)?;
        if self.fs.backup_recovery_pending(id)? {
            return Err(PrefixError::RecoveryRequired);
        }
        let snapshot = self.load_snapshot()?;
        if snapshot.registry.revision != expected_revision {
            return Err(PrefixError::RevisionConflict);
        }
        let current = snapshot
            .registry
            .record(id)
            .cloned()
            .ok_or(PrefixError::NotRegistered)?;
        if current.ownership != PrefixOwnership::EndfieldUnixManaged
            || current.owner_uid != current_uid()
        {
            return Err(PrefixError::Blocked);
        }
        if current.initialization_intent.is_some() {
            // Initialization may have changed user data before it was
            // interrupted. Deletion requires a separate safe recovery story.
            return Err(PrefixError::RecoveryRequired);
        }
        match self.fs.prefix_in_use(id)? {
            Some(true) => return Err(PrefixError::ActiveUse),
            Some(false) => {}
            None => return Err(PrefixError::UsageUnknown),
        }
        if !matches!(
            current.lifecycle,
            PrefixLifecycleState::Created
                | PrefixLifecycleState::Initialized
                | PrefixLifecycleState::RecoveryRequired
                | PrefixLifecycleState::Incomplete
        ) {
            return Err(PrefixError::InvalidState);
        }
        let expected = current.filesystem_identity.ok_or(PrefixError::Blocked)?;
        if self.fs.inspect_directory(id)? != Some(expected) {
            return Err(PrefixError::Blocked);
        }

        let mut journal = snapshot.registry;
        journal.revision = journal.revision.saturating_add(1);
        let record = journal
            .records
            .iter_mut()
            .find(|record| &record.id == id)
            .ok_or(PrefixError::RegistryCorrupt)?;
        record.lifecycle = PrefixLifecycleState::RecoveryRequired;
        record.update_time_unix_seconds = now();
        let journal_bytes = journal.encode()?;
        self.fs
            .save_registry_bytes(snapshot.bytes.as_deref(), &journal_bytes)?;
        match self.fs.prefix_in_use(id)? {
            Some(true) => return Err(PrefixError::ActiveUse),
            Some(false) => {}
            None => return Err(PrefixError::UsageUnknown),
        }
        self.fs.remove_directory(id, expected)?;

        journal.records.retain(|record| &record.id != id);
        journal.revision = journal.revision.saturating_add(1);
        let final_bytes = journal.encode()?;
        self.fs
            .save_registry_bytes(Some(&journal_bytes), &final_bytes)?;
        Ok(journal.revision)
    }

    /// Durably publish a write-ahead initialization intent. This method never
    /// spawns Wine. A caller may spawn only after it receives this record, and
    /// only if a production initializer contract has made the plan executable.
    pub fn begin_initialization_intent(
        &self,
        plan: &PrefixInitializationPlan,
        approval: &PrefixInitializationApproval,
        recovery_state: Option<bool>,
        validate_runtime: impl FnMut(&PrefixRuntimeAssociation) -> Result<(), String>,
    ) -> Result<(PrefixInitializationIntent, u64), PrefixError> {
        self.begin_initialization_intent_inner(
            plan,
            approval.plan_id(),
            false,
            recovery_state,
            validate_runtime,
        )
    }

    /// Recheck the durable Starting intent immediately before the coordinator
    /// creates the direct child. The callback runs while the existing P028
    /// registry-then-prefix locks are held, matching the lock order used by
    /// intent publication and terminal transitions.
    pub(crate) fn validate_starting_initialization(
        &self,
        plan: &PrefixInitializationPlan,
        expected_revision: u64,
        additional_recovery_pending: Option<bool>,
        mut validate_runtime: impl FnMut(&PrefixRuntimeAssociation) -> Result<(), String>,
    ) -> Result<(), PrefixError> {
        plan.validate_integrity()?;
        if additional_recovery_pending != Some(false) {
            return Err(PrefixError::RecoveryRequired);
        }
        let id = PrefixId::new(plan.prefix_id.clone())?;
        self.fs.prepare(true)?;
        let _registry_guard = self.fs.lock_registry()?;
        let _prefix_guard = self.fs.lock_prefix(&id)?;
        if self.fs.backup_recovery_pending(&id)? {
            return Err(PrefixError::RecoveryRequired);
        }
        let snapshot = self.load_snapshot()?;
        if snapshot.registry.revision != expected_revision {
            return Err(PrefixError::RevisionConflict);
        }
        let current = snapshot
            .registry
            .record(&id)
            .ok_or(PrefixError::NotRegistered)?;
        let Some(intent) = current.initialization_intent.as_ref() else {
            return Err(PrefixError::InitializationIntentMismatch);
        };
        let expected_identity = plan.filesystem_identity.ok_or(PrefixError::Blocked)?;
        let runtime_hash = runtime_reference_sha256(&current.runtime.runtime)?;
        if current.ownership != PrefixOwnership::EndfieldUnixManaged
            || current.owner_uid != current_uid()
            || current.lifecycle != PrefixLifecycleState::Incomplete
            || current.scope != plan.scope
            || current.generation != plan.prefix_generation
            || current.filesystem_identity != Some(expected_identity)
            || current.runtime.pin_id != plan.runtime_pin_id
            || current.runtime.pin_revision != plan.runtime_pin_revision
            || runtime_hash != plan.runtime_reference_sha256
            || intent.plan_id != plan.plan_id
            || intent.phase != PrefixInitializationPhase::Starting
            || intent.scope != plan.scope
            || intent.runtime_pin_id != plan.runtime_pin_id
            || intent.runtime_pin_revision != plan.runtime_pin_revision
            || intent.runtime_reference_sha256 != plan.runtime_reference_sha256
            || intent.prefix_generation != plan.prefix_generation
            || intent.filesystem_identity != expected_identity
            || self.fs.inspect_directory(&id)? != Some(expected_identity)
            || !self.fs.directory_is_empty(&id)?
            || self.fs.prefix_in_use(&id)? != Some(false)
            || current.initialization_evidence.is_some()
        {
            return Err(PrefixError::Blocked);
        }
        validate_runtime(&current.runtime).map_err(|_| PrefixError::Blocked)?;
        // Runtime revalidation may take time or inspect independent stores.
        // Repeat prefix filesystem checks after it returns so any identity,
        // contents, or use change during that work is caught before spawn.
        if self.fs.inspect_directory(&id)? != Some(expected_identity)
            || !self.fs.directory_is_empty(&id)?
            || self.fs.prefix_in_use(&id)? != Some(false)
        {
            return Err(PrefixError::Blocked);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn begin_synthetic_initialization_intent(
        &self,
        plan: &PrefixInitializationPlan,
        approval: &SyntheticInitializationApproval,
        recovery_state: Option<bool>,
        validate_runtime: impl FnMut(&PrefixRuntimeAssociation) -> Result<(), String>,
    ) -> Result<(PrefixInitializationIntent, u64), PrefixError> {
        self.begin_initialization_intent_inner(
            plan,
            &approval.plan_id,
            true,
            recovery_state,
            validate_runtime,
        )
    }

    fn begin_initialization_intent_inner(
        &self,
        plan: &PrefixInitializationPlan,
        approval_plan_id: &str,
        allow_synthetic_test: bool,
        recovery_state: Option<bool>,
        mut validate_runtime: impl FnMut(&PrefixRuntimeAssociation) -> Result<(), String>,
    ) -> Result<(PrefixInitializationIntent, u64), PrefixError> {
        plan.validate_integrity()?;
        if approval_plan_id != plan.plan_id {
            return Err(PrefixError::ApprovalMismatch);
        }
        let production_ready = plan.execution_supported
            && plan.state == PrefixInitializationPlanState::Ready
            && plan.blockers.is_empty();
        let synthetic_test_ready = allow_synthetic_test
            && !plan.execution_supported
            && plan.state == PrefixInitializationPlanState::Unsupported
            && plan.blockers.len() == 2
            && plan
                .blockers
                .contains(&PrefixInitializationBlocker::ExecutableIdentityNotBound)
            && plan
                .blockers
                .contains(&PrefixInitializationBlocker::InitializerChildOwnershipUnavailable);
        if (!production_ready && !synthetic_test_ready)
            || plan.strategy != PrefixInitializationStrategy::WineBootInitialize
            || plan.adapter_id != "managed_wine"
        {
            return Err(PrefixError::InitializationUnsupported);
        }
        if recovery_state != Some(false) {
            return Err(PrefixError::RecoveryRequired);
        }
        let id = PrefixId::new(plan.prefix_id.clone())?;
        self.fs.prepare(true)?;
        let _registry_guard = self.fs.lock_registry()?;
        let _prefix_guard = self.fs.lock_prefix(&id)?;
        if self.fs.backup_recovery_pending(&id)? {
            return Err(PrefixError::RecoveryRequired);
        }
        let snapshot = self.load_snapshot()?;
        if snapshot.registry.revision != plan.registry_revision {
            return Err(PrefixError::RevisionConflict);
        }
        let record = snapshot
            .registry
            .record(&id)
            .cloned()
            .ok_or(PrefixError::NotRegistered)?;
        let reference_hash = runtime_reference_sha256(&record.runtime.runtime)?;
        let Some(expected_identity) = plan.filesystem_identity else {
            return Err(PrefixError::Blocked);
        };
        let managed_wine = matches!(
            (&record.runtime.runtime, record.runtime.adapter_preference),
            (
                PinnedRuntimeReference::Managed(runtime),
                Some(AdapterPreference::ManagedWine)
            ) if runtime.family == "wine"
                && runtime.lineage == "upstream-wine"
                && runtime.build == "wine-10.0"
                && runtime.trust
                    == endfield_unix_runtime_selection::Provenance::IndependentlyAuthenticatedPublisher
        );
        if !managed_wine
            || record.scope != plan.scope
            || record.lifecycle != PrefixLifecycleState::Created
            || record.initialization_evidence.is_some()
            || record.initialization_intent.is_some()
            || record.ownership != PrefixOwnership::EndfieldUnixManaged
            || record.owner_uid != current_uid()
            || record.generation != plan.prefix_generation
            || record.filesystem_identity != Some(expected_identity)
            || record.runtime.pin_id != plan.runtime_pin_id
            || record.runtime.pin_revision != plan.runtime_pin_revision
            || reference_hash != plan.runtime_reference_sha256
            || self.fs.inspect_directory(&id)? != Some(expected_identity)
            || !self.fs.directory_is_empty(&id)?
            || self.fs.prefix_in_use(&id)? != Some(false)
        {
            return Err(PrefixError::Blocked);
        }
        validate_runtime(&record.runtime).map_err(|_| PrefixError::Blocked)?;

        let timestamp = now().max(record.update_time_unix_seconds);
        let intent = PrefixInitializationIntent {
            plan_id: plan.plan_id.clone(),
            scope: record.scope.clone(),
            adapter_id: "managed_wine".into(),
            runtime_pin_id: record.runtime.pin_id.clone(),
            runtime_pin_revision: record.runtime.pin_revision,
            runtime_reference_sha256: reference_hash,
            prefix_generation: record.generation,
            filesystem_identity: expected_identity,
            phase: PrefixInitializationPhase::Starting,
            process_exit_code: None,
            started_at_unix_seconds: timestamp,
            updated_at_unix_seconds: timestamp,
        };
        let mut registry = snapshot.registry;
        registry.revision = registry.revision.saturating_add(1);
        let stored = registry
            .records
            .iter_mut()
            .find(|stored| stored.id == id)
            .ok_or(PrefixError::RegistryCorrupt)?;
        stored.lifecycle = PrefixLifecycleState::Incomplete;
        stored.initialization_intent = Some(intent.clone());
        stored.update_time_unix_seconds = timestamp;
        let bytes = registry.encode()?;
        self.fs
            .save_registry_bytes(snapshot.bytes.as_deref(), &bytes)?;
        Ok((intent, registry.revision))
    }

    /// Record that the approved initializer process was created. The durable
    /// Starting state is committed before the caller may spawn the child.
    pub fn mark_initialization_running(
        &self,
        id: &PrefixId,
        plan_id: &str,
        expected_revision: u64,
    ) -> Result<u64, PrefixError> {
        self.transition_initialization_intent(
            id,
            plan_id,
            expected_revision,
            InitializationTransition {
                allowed_phases: &[PrefixInitializationPhase::Starting],
                next_phase: PrefixInitializationPhase::Running,
                lifecycle: PrefixLifecycleState::Incomplete,
                process_exit_code: None,
            },
        )
    }

    /// Record process termination without claiming prefix initialization.
    /// Exit zero advances only to `VerificationPending`; every other result
    /// remains recovery-required and preserves all prefix bytes.
    pub fn record_initialization_process_outcome(
        &self,
        id: &PrefixId,
        plan_id: &str,
        expected_revision: u64,
        outcome: PrefixInitializationProcessOutcome,
    ) -> Result<u64, PrefixError> {
        let (phase, lifecycle, exit_code) = match outcome {
            PrefixInitializationProcessOutcome::ExitedZero => (
                PrefixInitializationPhase::VerificationPending,
                PrefixLifecycleState::Incomplete,
                Some(0),
            ),
            PrefixInitializationProcessOutcome::SpawnFailed => (
                PrefixInitializationPhase::Failed,
                PrefixLifecycleState::RecoveryRequired,
                None,
            ),
            PrefixInitializationProcessOutcome::ExitedNonzero(code) if code != 0 => (
                PrefixInitializationPhase::Failed,
                PrefixLifecycleState::RecoveryRequired,
                Some(code),
            ),
            PrefixInitializationProcessOutcome::ExitedNonzero(_) => {
                return Err(PrefixError::InvalidState);
            }
            PrefixInitializationProcessOutcome::Interrupted => (
                PrefixInitializationPhase::Interrupted,
                PrefixLifecycleState::RecoveryRequired,
                None,
            ),
        };
        let allowed_phases: &[PrefixInitializationPhase] = match outcome {
            PrefixInitializationProcessOutcome::ExitedZero
            | PrefixInitializationProcessOutcome::ExitedNonzero(_) => {
                &[PrefixInitializationPhase::Running]
            }
            PrefixInitializationProcessOutcome::SpawnFailed => {
                &[PrefixInitializationPhase::Starting]
            }
            PrefixInitializationProcessOutcome::Interrupted => &[
                PrefixInitializationPhase::Starting,
                PrefixInitializationPhase::Running,
            ],
        };
        self.transition_initialization_intent(
            id,
            plan_id,
            expected_revision,
            InitializationTransition {
                allowed_phases,
                next_phase: phase,
                lifecycle,
                process_exit_code: Some(exit_code),
            },
        )
    }

    /// Promote only a Wine 10.0 initialization intent that reached a zero-exit
    /// observation and whose prefix artifacts pass the runtime-specific,
    /// no-follow completion verifier. This method never executes Wine.
    pub fn complete_verified_wine10_initialization(
        &self,
        id: &PrefixId,
        plan_id: &str,
        expected_revision: u64,
        mut validate_runtime: impl FnMut(&PrefixRuntimeAssociation) -> Result<(), String>,
    ) -> Result<(PrefixRecord, u64), PrefixError> {
        self.fs.prepare(true)?;
        let _registry_guard = self.fs.lock_registry()?;
        let _prefix_guard = self.fs.lock_prefix(id)?;
        if self.fs.backup_recovery_pending(id)? {
            return Err(PrefixError::RecoveryRequired);
        }
        let snapshot = self.load_snapshot()?;
        if snapshot.registry.revision != expected_revision {
            return Err(PrefixError::RevisionConflict);
        }
        let current = snapshot
            .registry
            .record(id)
            .cloned()
            .ok_or(PrefixError::NotRegistered)?;
        let runtime = match &current.runtime.runtime {
            PinnedRuntimeReference::Managed(runtime)
                if runtime.family == "wine"
                    && runtime.lineage == "upstream-wine"
                    && runtime.build == "wine-10.0"
                    && runtime.trust
                        == endfield_unix_runtime_selection::Provenance::
                            IndependentlyAuthenticatedPublisher => runtime,
            _ => return Err(PrefixError::InitializationUnsupported),
        };
        let _ = runtime;
        let intent = current
            .initialization_intent
            .as_ref()
            .ok_or(PrefixError::InitializationIntentMismatch)?;
        let identity = current.filesystem_identity.ok_or(PrefixError::Blocked)?;
        if current.owner_uid != current_uid()
            || current.ownership != PrefixOwnership::EndfieldUnixManaged
            || current.lifecycle != PrefixLifecycleState::Incomplete
            || intent.phase != PrefixInitializationPhase::VerificationPending
            || intent.plan_id != plan_id
            || intent.adapter_id != "managed_wine"
            || intent.filesystem_identity != identity
            || self.fs.inspect_directory(id)? != Some(identity)
            || self.fs.prefix_in_use(id)? != Some(false)
        {
            return Err(PrefixError::InitializationIntentMismatch);
        }
        validate_runtime(&current.runtime).map_err(|_| PrefixError::Blocked)?;
        let verified = self.fs.verify_wine10_prefix(id, identity)?;
        if verified.directory_identity != identity
            || !is_sha256(&verified.system_registry_sha256)
            || !is_sha256(&verified.user_registry_sha256)
            || !is_sha256(&verified.evidence_sha256)
        {
            return Err(PrefixError::Blocked);
        }

        let timestamp = now().max(current.update_time_unix_seconds);
        let mut registry = snapshot.registry;
        registry.revision = registry.revision.saturating_add(1);
        let record = registry
            .records
            .iter_mut()
            .find(|record| &record.id == id)
            .ok_or(PrefixError::RegistryCorrupt)?;
        record.lifecycle = PrefixLifecycleState::Initialized;
        record.initialization_intent = None;
        record.initialization_evidence = Some(PrefixInitializationEvidence {
            adapter_id: "managed_wine".into(),
            runtime_reference_sha256: intent.runtime_reference_sha256.clone(),
            verification_evidence_sha256: verified.evidence_sha256,
            outcome: InitializationEvidenceOutcome::CompletedAndVerified,
            observed_at_unix_seconds: timestamp,
        });
        record.update_time_unix_seconds = timestamp;
        let completed = record.clone();
        let bytes = registry.encode()?;
        self.fs
            .save_registry_bytes(snapshot.bytes.as_deref(), &bytes)?;
        Ok((completed, registry.revision))
    }

    /// Convert an unobserved Starting/Running/VerificationPending transaction
    /// into an explicit interruption record. This never retries the operation.
    pub fn mark_initialization_interrupted(
        &self,
        id: &PrefixId,
        plan_id: &str,
        expected_revision: u64,
    ) -> Result<u64, PrefixError> {
        self.transition_initialization_intent(
            id,
            plan_id,
            expected_revision,
            InitializationTransition {
                allowed_phases: &[
                    PrefixInitializationPhase::Starting,
                    PrefixInitializationPhase::Running,
                    PrefixInitializationPhase::VerificationPending,
                ],
                next_phase: PrefixInitializationPhase::Interrupted,
                lifecycle: PrefixLifecycleState::RecoveryRequired,
                process_exit_code: None,
            },
        )
    }

    fn transition_initialization_intent(
        &self,
        id: &PrefixId,
        plan_id: &str,
        expected_revision: u64,
        transition: InitializationTransition,
    ) -> Result<u64, PrefixError> {
        self.fs.prepare(true)?;
        let _registry_guard = self.fs.lock_registry()?;
        let _prefix_guard = self.fs.lock_prefix(id)?;
        let snapshot = self.load_snapshot()?;
        if snapshot.registry.revision != expected_revision {
            return Err(PrefixError::RevisionConflict);
        }
        let mut registry = snapshot.registry;
        let record = registry
            .records
            .iter_mut()
            .find(|record| &record.id == id)
            .ok_or(PrefixError::NotRegistered)?;
        if record.owner_uid != current_uid()
            || record.ownership != PrefixOwnership::EndfieldUnixManaged
            || record.initialization_evidence.is_some()
            || self.fs.inspect_directory(id)? != record.filesystem_identity
        {
            return Err(PrefixError::Blocked);
        }
        let intent = record
            .initialization_intent
            .as_mut()
            .ok_or(PrefixError::InitializationIntentMismatch)?;
        if intent.plan_id != plan_id || !transition.allowed_phases.contains(&intent.phase) {
            return Err(PrefixError::InitializationIntentMismatch);
        }
        let timestamp = now().max(record.update_time_unix_seconds);
        intent.phase = transition.next_phase;
        if let Some(process_exit_code) = transition.process_exit_code {
            intent.process_exit_code = process_exit_code;
        }
        intent.updated_at_unix_seconds = timestamp;
        record.lifecycle = transition.lifecycle;
        record.update_time_unix_seconds = timestamp;
        registry.revision = registry.revision.saturating_add(1);
        let bytes = registry.encode()?;
        self.fs
            .save_registry_bytes(snapshot.bytes.as_deref(), &bytes)?;
        Ok(registry.revision)
    }

    pub fn initialize(&self, _id: &PrefixId) -> Result<(), PrefixError> {
        Err(PrefixError::InitializationUnsupported)
    }

    fn load_snapshot(&self) -> Result<RegistrySnapshot, PrefixError> {
        let bytes = self.fs.load_registry_bytes()?;
        let registry = match &bytes {
            Some(bytes) => PrefixRegistry::decode(bytes)?,
            None => PrefixRegistry::default(),
        };
        Ok(RegistrySnapshot { registry, bytes })
    }

    fn inspect_record(
        &self,
        record: &PrefixRecord,
        mut validate_runtime: impl FnMut(&PinnedRuntimeReference) -> Result<(), String>,
    ) -> Result<(PrefixLifecycleState, RuntimeValidation), PrefixError> {
        let mut lifecycle = match self.fs.inspect_directory(&record.id) {
            Ok(Some(identity)) if Some(identity) == record.filesystem_identity => {
                record.lifecycle.clone()
            }
            Ok(Some(_)) => PrefixLifecycleState::Blocked,
            Ok(None)
                if matches!(
                    record.lifecycle,
                    PrefixLifecycleState::RecoveryRequired | PrefixLifecycleState::Incomplete
                ) =>
            {
                record.lifecycle.clone()
            }
            Ok(None) => PrefixLifecycleState::Missing,
            Err(PrefixError::Blocked) => PrefixLifecycleState::Blocked,
            Err(error) => return Err(error),
        };
        if let Some(intent) = &record.initialization_intent {
            lifecycle = match intent.phase {
                PrefixInitializationPhase::Starting | PrefixInitializationPhase::Running => {
                    match self.fs.prefix_in_use(&record.id) {
                        Ok(Some(true)) => PrefixLifecycleState::Incomplete,
                        Ok(Some(false)) => PrefixLifecycleState::RecoveryRequired,
                        Ok(None) | Err(_) => PrefixLifecycleState::Blocked,
                    }
                }
                PrefixInitializationPhase::VerificationPending => PrefixLifecycleState::Incomplete,
                PrefixInitializationPhase::Failed | PrefixInitializationPhase::Interrupted => {
                    PrefixLifecycleState::RecoveryRequired
                }
            };
        }
        let lifecycle = if record.owner_uid != current_uid() {
            PrefixLifecycleState::Blocked
        } else {
            lifecycle
        };
        let runtime = match validate_runtime(&record.runtime.runtime) {
            Ok(()) => RuntimeValidation::Valid,
            Err(reason) => RuntimeValidation::Stale(reason.chars().take(256).collect()),
        };
        Ok((lifecycle, runtime))
    }
}

struct RegistrySnapshot {
    registry: PrefixRegistry,
    bytes: Option<Vec<u8>>,
}

fn current_uid() -> u32 {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Read at most `limit + 1` bytes to make bounded metadata parsing explicit.
pub fn read_bounded(mut reader: impl Read, limit: usize) -> Result<Vec<u8>, PrefixError> {
    let mut bytes = Vec::with_capacity(limit.min(8192));
    reader
        .by_ref()
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(PrefixError::RegistryCorrupt);
    }
    Ok(bytes)
}

/// A small helper used by tests to confirm plans remain observational.
pub fn path_exists_without_following(path: &Path) -> Result<bool, PrefixError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(PrefixError::Io(error)),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use endfield_unix_runtime_selection::{ManagedRuntimeReference, Provenance};
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt, symlink},
        path::Path,
        sync::{Arc, Barrier},
        thread,
    };
    use tempfile::TempDir;

    struct FixedUsageFilesystem {
        inner: LinuxPrefixFilesystem,
        in_use: Option<bool>,
        synthetic_wine_completion: bool,
    }

    pub(crate) fn coordinator_prefix_store(data_dir: PathBuf) -> PrefixStore {
        PrefixStore::with_filesystem(Arc::new(FixedUsageFilesystem {
            inner: LinuxPrefixFilesystem::new(data_dir),
            in_use: Some(false),
            synthetic_wine_completion: false,
        }))
    }

    impl PrefixFilesystem for FixedUsageFilesystem {
        fn root_path(&self) -> &Path {
            self.inner.root_path()
        }

        fn prepare(&self, create: bool) -> Result<(), PrefixError> {
            self.inner.prepare(create)
        }

        fn preflight_destination(&self, id: &PrefixId) -> Result<(), PrefixError> {
            self.inner.preflight_destination(id)
        }

        fn lock_registry(&self) -> Result<Box<dyn PrefixGuard>, PrefixError> {
            self.inner.lock_registry()
        }

        fn lock_prefix(&self, id: &PrefixId) -> Result<Box<dyn PrefixGuard>, PrefixError> {
            self.inner.lock_prefix(id)
        }

        fn load_registry_bytes(&self) -> Result<Option<Vec<u8>>, PrefixError> {
            self.inner.load_registry_bytes()
        }

        fn save_registry_bytes(
            &self,
            expected_current: Option<&[u8]>,
            replacement: &[u8],
        ) -> Result<(), PrefixError> {
            self.inner
                .save_registry_bytes(expected_current, replacement)
        }

        fn inspect_directory(
            &self,
            id: &PrefixId,
        ) -> Result<Option<FilesystemIdentity>, PrefixError> {
            self.inner.inspect_directory(id)
        }

        fn directory_is_empty(&self, id: &PrefixId) -> Result<bool, PrefixError> {
            self.inner.directory_is_empty(id)
        }

        fn create_directory(&self, id: &PrefixId) -> Result<FilesystemIdentity, PrefixError> {
            self.inner.create_directory(id)
        }

        fn remove_directory(
            &self,
            id: &PrefixId,
            expected: FilesystemIdentity,
        ) -> Result<(), PrefixError> {
            self.inner.remove_directory(id, expected)
        }

        fn prefix_in_use(&self, _id: &PrefixId) -> Result<Option<bool>, PrefixError> {
            Ok(self.in_use)
        }

        fn target_path(&self, id: &PrefixId) -> PathBuf {
            self.inner.target_path(id)
        }

        fn free_bytes(&self) -> Option<u64> {
            self.inner.free_bytes()
        }

        fn verify_wine10_prefix(
            &self,
            id: &PrefixId,
            expected: FilesystemIdentity,
        ) -> Result<Wine10PrefixCompletionEvidence, PrefixError> {
            if self.synthetic_wine_completion {
                return Ok(Wine10PrefixCompletionEvidence {
                    directory_identity: expected,
                    system_registry_sha256: "a".repeat(64),
                    user_registry_sha256: "b".repeat(64),
                    evidence_sha256: "c".repeat(64),
                });
            }
            self.inner.verify_wine10_prefix(id, expected)
        }
    }

    pub(crate) fn fixture() -> (TempDir, PrefixStore, PrefixScope, PrefixRuntimeAssociation) {
        let temporary = tempfile::tempdir().unwrap();
        let paths = endfield_unix_storage::AppPaths::from_base_dirs(
            temporary.path().join("config"),
            temporary.path().join("data"),
            temporary.path().join("cache"),
        )
        .unwrap();
        let machine = MachineId::from_digest("a".repeat(64)).unwrap();
        let scope = PrefixScope::machine(machine);
        let runtime = PrefixRuntimeAssociation {
            runtime: PinnedRuntimeReference::Managed(ManagedRuntimeReference {
                installation_id: "proton-fixture".into(),
                family: "proton".into(),
                lineage: "ge-proton".into(),
                provider: "fixture".into(),
                project: "fixture".into(),
                build: "10.26".into(),
                artifact_sha256: "a".repeat(64),
                manifest_sha256: "b".repeat(64),
                generation: 1,
                ownership_record_sha256: "c".repeat(64),
                trust: Provenance::UserPinnedLimitedTrust,
            }),
            selection_source: super::RuntimeSelectionSource::MachinePin,
            pin_id: "pin-fixture".into(),
            pin_revision: 7,
            adapter_preference: Some(AdapterPreference::UmuProton),
        };
        (temporary, PrefixStore::for_app(&paths), scope, runtime)
    }

    fn managed_wine_runtime() -> PrefixRuntimeAssociation {
        PrefixRuntimeAssociation {
            runtime: PinnedRuntimeReference::Managed(ManagedRuntimeReference {
                installation_id: "wine-fixture".into(),
                family: "wine".into(),
                lineage: "upstream-wine".into(),
                provider: "fixture".into(),
                project: "fixture".into(),
                build: "wine-10.0".into(),
                artifact_sha256: "a".repeat(64),
                manifest_sha256: "b".repeat(64),
                generation: 1,
                ownership_record_sha256: "c".repeat(64),
                trust: Provenance::IndependentlyAuthenticatedPublisher,
            }),
            selection_source: super::RuntimeSelectionSource::MachinePin,
            pin_id: "wine-pin".into(),
            pin_revision: 9,
            adapter_preference: Some(AdapterPreference::ManagedWine),
        }
    }

    fn wine_fixture(
        synthetic_completion: bool,
    ) -> (TempDir, PrefixStore, PrefixScope, PrefixRuntimeAssociation) {
        let temporary = tempfile::tempdir().unwrap();
        let paths = endfield_unix_storage::AppPaths::from_base_dirs(
            temporary.path().join("config"),
            temporary.path().join("data"),
            temporary.path().join("cache"),
        )
        .unwrap();
        let data_dir = paths.data_dir().to_path_buf();
        let store = PrefixStore::with_filesystem(Arc::new(FixedUsageFilesystem {
            inner: LinuxPrefixFilesystem::new(data_dir),
            in_use: Some(false),
            synthetic_wine_completion: synthetic_completion,
        }));
        let machine = MachineId::from_digest("a".repeat(64)).unwrap();
        (
            temporary,
            store,
            PrefixScope::machine(machine),
            managed_wine_runtime(),
        )
    }

    #[test]
    fn runtime_adapter_preferences_must_match_the_preserved_runtime_family() {
        let (_temporary, store, scope, mut runtime) = fixture();
        runtime.adapter_preference = Some(AdapterPreference::ExternalWine);
        assert!(matches!(
            store.plan(
                PrefixId::new("wrong-adapter").unwrap(),
                scope,
                runtime,
                Some(0)
            ),
            Err(PrefixError::InvalidRuntime)
        ));
    }

    #[test]
    fn recovery_records_can_preserve_partial_initialization_evidence() {
        let (_temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("partial-evidence").unwrap();
        let (mut record, _) = store.create(id, scope, runtime, 0).unwrap();
        record.lifecycle = PrefixLifecycleState::RecoveryRequired;
        record.initialization_evidence = Some(PrefixInitializationEvidence {
            adapter_id: "fixture-adapter".into(),
            runtime_reference_sha256: runtime_reference_sha256(&record.runtime.runtime).unwrap(),
            verification_evidence_sha256: "b".repeat(64),
            outcome: InitializationEvidenceOutcome::Partial,
            observed_at_unix_seconds: record.update_time_unix_seconds,
        });
        assert!(record.validate().is_ok());

        record
            .initialization_evidence
            .as_mut()
            .unwrap()
            .runtime_reference_sha256 = "a".repeat(64);
        assert!(record.validate().is_err());

        record.lifecycle = PrefixLifecycleState::Initialized;
        assert!(record.validate().is_err());
    }

    #[test]
    fn records_with_a_different_owner_uid_block_mutation_and_recovery() {
        let (_temporary, store, scope, runtime) = fixture();
        let created_id = PrefixId::new("foreign-owner").unwrap();
        let (created, revision) = store
            .create(created_id.clone(), scope.clone(), runtime.clone(), 0)
            .unwrap();
        let recovery_id = PrefixId::new("foreign-recovery").unwrap();
        store
            .create(recovery_id.clone(), scope.clone(), runtime, revision)
            .unwrap();

        let snapshot = store.load_snapshot().unwrap();
        let mut registry = snapshot.registry;
        let other_uid = if current_uid() == u32::MAX {
            0
        } else {
            current_uid() + 1
        };
        for record in &mut registry.records {
            record.owner_uid = other_uid;
            if record.id == recovery_id {
                record.lifecycle = PrefixLifecycleState::RecoveryRequired;
            }
        }
        let bytes = registry.encode().unwrap();
        store
            .fs
            .save_registry_bytes(snapshot.bytes.as_deref(), &bytes)
            .unwrap();

        assert!(matches!(
            store.plan(
                created_id.clone(),
                created.scope,
                created.runtime,
                Some(revision + 2)
            ),
            Err(PrefixError::Blocked)
        ));
        assert!(matches!(
            store.delete_data(&created_id, revision + 2, &store.target_path(&created_id)),
            Err(PrefixError::Blocked)
        ));
        assert!(matches!(
            store.recover(&recovery_id, revision + 2),
            Err(PrefixError::Blocked)
        ));
        assert!(store.target_path(&created_id).is_dir());
        assert!(store.target_path(&recovery_id).is_dir());
    }

    #[test]
    fn plan_is_read_only_and_directory_creation_is_not_initialization() {
        let (temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("first").unwrap();
        let plan = store
            .plan(id.clone(), scope.clone(), runtime.clone(), Some(0))
            .unwrap();
        assert_eq!(plan.expected_registry_revision, 0);
        assert!(plan.free_bytes.is_some());
        assert!(!plan.requires_wine_execution);
        assert!(!plan.already_created);
        assert!(!plan.destination.exists());
        assert!(
            !temporary
                .path()
                .join("data/endfield-unix/prefixes")
                .exists()
        );

        let (record, revision) = store
            .create(id.clone(), scope.clone(), runtime.clone(), 0)
            .unwrap();
        assert_eq!(record.lifecycle, PrefixLifecycleState::Created);
        assert_eq!(revision, 2);
        assert!(record.initialization_evidence.is_none());
        assert!(matches!(
            store.initialize(&id),
            Err(PrefixError::InitializationUnsupported)
        ));
        let inspection = store.show(&id, |_| Ok(())).unwrap();
        assert_eq!(inspection.observed_lifecycle, PrefixLifecycleState::Created);
        assert!(!inspection.path.join("drive_c").exists());

        let (again, unchanged_revision) = store.create(id, scope, runtime, 0).unwrap();
        assert_eq!(again, record);
        assert_eq!(unchanged_revision, 2);
    }

    #[test]
    fn initialization_plan_is_deterministic_read_only_and_never_authorizes_execution() {
        let (_temporary, original_store, scope, runtime) = fixture();
        let data_dir = original_store
            .fs
            .root_path()
            .parent()
            .unwrap()
            .to_path_buf();
        let store = PrefixStore::with_filesystem(Arc::new(FixedUsageFilesystem {
            inner: LinuxPrefixFilesystem::new(&data_dir),
            in_use: Some(false),
            synthetic_wine_completion: false,
        }));
        let id = PrefixId::new("init-plan").unwrap();
        let (record, revision) = store.create(id.clone(), scope, runtime, 0).unwrap();
        let registry_before = store.fs.load_registry_bytes().unwrap();
        let prefix_before = store.fs.inspect_directory(&id).unwrap();

        let plan = store
            .plan_initialization(&id, revision, &record.runtime, Some(false), |_| Ok(()))
            .unwrap();
        let repeated = store
            .plan_initialization(&id, revision, &record.runtime, Some(false), |_| Ok(()))
            .unwrap();

        assert_eq!(plan, repeated);
        assert_eq!(plan.state, PrefixInitializationPlanState::Unsupported);
        assert_eq!(
            plan.blockers,
            vec![PrefixInitializationBlocker::InitializerUnverified]
        );
        assert!(!plan.execution_supported);
        assert!(plan.approval_required);
        assert!(matches!(
            plan.approve(&plan.plan_id),
            Err(PrefixError::InitializationUnsupported)
        ));
        assert!(matches!(
            plan.approve("sha256:wrong"),
            Err(PrefixError::ApprovalMismatch)
        ));
        let mut altered = plan.clone();
        altered
            .blockers
            .push(PrefixInitializationBlocker::PrefixContainsData);
        assert!(matches!(
            altered.approve(&plan.plan_id),
            Err(PrefixError::PlanIntegrityMismatch)
        ));
        let mut changed_scope = plan.clone();
        changed_scope.scope = PrefixScope::machine(MachineId::from_digest("b".repeat(64)).unwrap());
        assert!(matches!(
            changed_scope.approve(&plan.plan_id),
            Err(PrefixError::PlanIntegrityMismatch)
        ));
        assert_eq!(store.fs.load_registry_bytes().unwrap(), registry_before);
        assert_eq!(store.fs.inspect_directory(&id).unwrap(), prefix_before);
        assert!(store.fs.directory_is_empty(&id).unwrap());
        assert_eq!(
            store.show(&id, |_| Ok(())).unwrap().observed_lifecycle,
            PrefixLifecycleState::Created
        );
    }

    #[test]
    fn managed_wine_plan_names_the_execution_gates_that_still_block_it() {
        let (_temporary, store, scope, runtime) = wine_fixture(false);
        let id = PrefixId::new("wine-plan").unwrap();
        let (record, revision) = store.create(id.clone(), scope, runtime, 0).unwrap();
        let plan = store
            .plan_initialization(&id, revision, &record.runtime, Some(false), |_| Ok(()))
            .unwrap();

        assert_eq!(plan.state, PrefixInitializationPlanState::Unsupported);
        assert!(!plan.execution_supported);
        assert!(
            plan.blockers
                .contains(&PrefixInitializationBlocker::ExecutableIdentityNotBound)
        );
        assert!(
            plan.blockers
                .contains(&PrefixInitializationBlocker::InitializerChildOwnershipUnavailable)
        );
        assert!(matches!(
            plan.approve(&plan.plan_id),
            Err(PrefixError::InitializationUnsupported)
        ));
        assert!(store.fs.directory_is_empty(&id).unwrap());
    }

    #[test]
    fn durable_initialization_intent_requires_exact_approval_and_verification() {
        let (_temporary, store, scope, runtime) = wine_fixture(true);
        let id = PrefixId::new("wine-intent").unwrap();
        let (record, revision) = store.create(id.clone(), scope, runtime, 0).unwrap();
        let plan = store
            .plan_initialization(&id, revision, &record.runtime, Some(false), |_| Ok(()))
            .unwrap();
        let approval = plan.synthetic_approval_for_test(&plan.plan_id).unwrap();
        let (intent, started_revision) = store
            .begin_synthetic_initialization_intent(&plan, &approval, Some(false), |_| Ok(()))
            .unwrap();
        assert_eq!(intent.phase, PrefixInitializationPhase::Starting);
        assert_eq!(intent.scope, record.scope);
        assert_eq!(started_revision, revision + 1);
        assert!(store.fs.directory_is_empty(&id).unwrap());
        assert!(matches!(
            store.begin_synthetic_initialization_intent(&plan, &approval, Some(false), |_| Ok(())),
            Err(PrefixError::RevisionConflict)
        ));

        let starting_bytes = store.fs.load_registry_bytes().unwrap();
        assert!(
            store
                .record_initialization_process_outcome(
                    &id,
                    &plan.plan_id,
                    started_revision,
                    PrefixInitializationProcessOutcome::ExitedZero,
                )
                .is_err()
        );
        assert_eq!(store.fs.load_registry_bytes().unwrap(), starting_bytes);
        assert!(
            store
                .record_initialization_process_outcome(
                    &id,
                    &plan.plan_id,
                    started_revision,
                    PrefixInitializationProcessOutcome::ExitedNonzero(9),
                )
                .is_err()
        );
        assert_eq!(store.fs.load_registry_bytes().unwrap(), starting_bytes);

        let failed_revision = store
            .record_initialization_process_outcome(
                &id,
                &plan.plan_id,
                started_revision,
                PrefixInitializationProcessOutcome::SpawnFailed,
            )
            .unwrap();
        let failed_record = store.load_snapshot().unwrap();
        let failed_record = failed_record.registry.record(&id).unwrap();
        assert_eq!(
            failed_record.lifecycle,
            PrefixLifecycleState::RecoveryRequired
        );
        assert_eq!(
            failed_record.initialization_intent.as_ref().unwrap().phase,
            PrefixInitializationPhase::Failed
        );
        assert!(matches!(
            store.mark_initialization_running(&id, &plan.plan_id, failed_revision),
            Err(PrefixError::InitializationIntentMismatch)
        ));

        let (_temporary, store, scope, runtime) = wine_fixture(true);
        let id = PrefixId::new("wine-intent-interrupted").unwrap();
        let (record, revision) = store.create(id.clone(), scope, runtime, 0).unwrap();
        let plan = store
            .plan_initialization(&id, revision, &record.runtime, Some(false), |_| Ok(()))
            .unwrap();
        let approval = plan.synthetic_approval_for_test(&plan.plan_id).unwrap();
        let (_, started_revision) = store
            .begin_synthetic_initialization_intent(&plan, &approval, Some(false), |_| Ok(()))
            .unwrap();
        let interrupted_revision = store
            .record_initialization_process_outcome(
                &id,
                &plan.plan_id,
                started_revision,
                PrefixInitializationProcessOutcome::Interrupted,
            )
            .unwrap();
        let interrupted = store.load_snapshot().unwrap();
        let interrupted = interrupted.registry.record(&id).unwrap();
        assert_eq!(
            interrupted.lifecycle,
            PrefixLifecycleState::RecoveryRequired
        );
        assert_eq!(
            interrupted.initialization_intent.as_ref().unwrap().phase,
            PrefixInitializationPhase::Interrupted
        );
        assert!(
            store
                .mark_initialization_running(&id, &plan.plan_id, interrupted_revision)
                .is_err()
        );

        // A new fixture below exercises successful process creation and exit.
        let (_temporary, store, scope, runtime) = wine_fixture(true);
        let id = PrefixId::new("wine-intent-running").unwrap();
        let (record, revision) = store.create(id.clone(), scope, runtime, 0).unwrap();
        let plan = store
            .plan_initialization(&id, revision, &record.runtime, Some(false), |_| Ok(()))
            .unwrap();
        let approval = plan.synthetic_approval_for_test(&plan.plan_id).unwrap();
        let (_, started_revision) = store
            .begin_synthetic_initialization_intent(&plan, &approval, Some(false), |_| Ok(()))
            .unwrap();

        let running_revision = store
            .mark_initialization_running(&id, &plan.plan_id, started_revision)
            .unwrap();
        let running_bytes = store.fs.load_registry_bytes().unwrap();
        assert!(matches!(
            store.record_initialization_process_outcome(
                &id,
                &plan.plan_id,
                running_revision,
                PrefixInitializationProcessOutcome::ExitedNonzero(0),
            ),
            Err(PrefixError::InvalidState)
        ));
        assert_eq!(store.fs.load_registry_bytes().unwrap(), running_bytes);
        let verification_revision = store
            .record_initialization_process_outcome(
                &id,
                &plan.plan_id,
                running_revision,
                PrefixInitializationProcessOutcome::ExitedZero,
            )
            .unwrap();
        let pending = store.load_snapshot().unwrap();
        let pending_record = pending.registry.record(&id).unwrap();
        assert_eq!(pending_record.lifecycle, PrefixLifecycleState::Incomplete);
        assert_eq!(
            pending_record.initialization_intent.as_ref().unwrap().phase,
            PrefixInitializationPhase::VerificationPending
        );

        let (completed, completed_revision) = store
            .complete_verified_wine10_initialization(
                &id,
                &plan.plan_id,
                verification_revision,
                |_| Ok(()),
            )
            .unwrap();
        assert_eq!(completed.lifecycle, PrefixLifecycleState::Initialized);
        assert!(completed.initialization_intent.is_none());
        assert_eq!(completed_revision, verification_revision + 1);
        assert_eq!(
            completed.initialization_evidence.unwrap().outcome,
            InitializationEvidenceOutcome::CompletedAndVerified
        );
    }

    #[test]
    fn interrupted_initialization_is_never_retried_or_recovered_implicitly() {
        let (_temporary, store, scope, runtime) = wine_fixture(false);
        let id = PrefixId::new("wine-interrupted").unwrap();
        let (record, revision) = store.create(id.clone(), scope, runtime, 0).unwrap();
        let plan = store
            .plan_initialization(&id, revision, &record.runtime, Some(false), |_| Ok(()))
            .unwrap();
        let approval = plan.synthetic_approval_for_test(&plan.plan_id).unwrap();
        let (_, started_revision) = store
            .begin_synthetic_initialization_intent(&plan, &approval, Some(false), |_| Ok(()))
            .unwrap();
        assert_eq!(
            store.show(&id, |_| Ok(())).unwrap().observed_lifecycle,
            PrefixLifecycleState::RecoveryRequired
        );
        let interrupted_revision = store
            .mark_initialization_interrupted(&id, &plan.plan_id, started_revision)
            .unwrap();
        let record = store.load_snapshot().unwrap().registry.records[0].clone();
        assert_eq!(record.lifecycle, PrefixLifecycleState::RecoveryRequired);
        assert_eq!(
            record.initialization_intent.as_ref().unwrap().phase,
            PrefixInitializationPhase::Interrupted
        );
        assert!(matches!(
            store.recover(&id, interrupted_revision),
            Err(PrefixError::RecoveryBlocked)
        ));
        assert!(matches!(
            store.begin_synthetic_initialization_intent(&plan, &approval, Some(false), |_| Ok(())),
            Err(PrefixError::RevisionConflict)
        ));
        assert!(store.fs.inspect_directory(&id).unwrap().is_some());
        assert!(store.fs.directory_is_empty(&id).unwrap());
    }

    #[test]
    fn wine10_completion_verifier_requires_nofollow_prefix_artifacts() {
        use std::os::unix::fs::symlink;

        let (_temporary, store, scope, runtime) = wine_fixture(false);
        let id = PrefixId::new("wine-structure").unwrap();
        let (_, _) = store.create(id.clone(), scope, runtime, 0).unwrap();
        let prefix = store.target_path(&id);
        fs::create_dir(prefix.join("drive_c")).unwrap();
        fs::create_dir(prefix.join("dosdevices")).unwrap();
        symlink("../drive_c", prefix.join("dosdevices/c:")).unwrap();
        fs::write(
            prefix.join("system.reg"),
            b"WINE REGISTRY Version 2\n\n[Software\\\\Wine\\\\WineConfig] 1234\n",
        )
        .unwrap();
        fs::write(
            prefix.join("user.reg"),
            b"WINE REGISTRY Version 2\n\n[Software\\\\Wine\\\\Drivers] 1234\n",
        )
        .unwrap();
        let identity = store.fs.inspect_directory(&id).unwrap().unwrap();
        let verified = store.fs.verify_wine10_prefix(&id, identity).unwrap();
        assert_eq!(verified.directory_identity, identity);
        assert_ne!(
            verified.system_registry_sha256,
            verified.user_registry_sha256
        );

        fs::remove_file(prefix.join("dosdevices/c:")).unwrap();
        symlink("/tmp", prefix.join("dosdevices/c:")).unwrap();
        assert!(matches!(
            store.fs.verify_wine10_prefix(&id, identity),
            Err(PrefixError::Blocked)
        ));
    }

    #[test]
    fn registry_v1_migrates_only_after_validating_its_original_checksum() {
        let (_temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("v1-registry").unwrap();
        let (_, _) = store.create(id, scope, runtime, 0).unwrap();
        let mut legacy = store.load_snapshot().unwrap().registry;
        legacy.schema_version = 1;
        legacy.refresh_checksum();
        let bytes = serde_json::to_vec(&legacy).unwrap();
        let migrated = PrefixRegistry::decode(&bytes).unwrap();
        assert_eq!(migrated.schema_version, REGISTRY_SCHEMA_VERSION);
        assert_eq!(migrated.revision, legacy.revision);
        assert_eq!(migrated.records.len(), 1);
        assert!(migrated.records[0].initialization_intent.is_none());

        let mut tampered = legacy;
        tampered.checksum_sha256 = "0".repeat(64);
        assert!(matches!(
            PrefixRegistry::decode(&serde_json::to_vec(&tampered).unwrap()),
            Err(PrefixError::RegistryCorrupt)
        ));
    }

    #[test]
    fn initialization_plan_blocks_existing_data_and_unknown_recovery_state() {
        let (_temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("init-data").unwrap();
        let (record, revision) = store.create(id.clone(), scope, runtime, 0).unwrap();
        let data = store.target_path(&id).join("user-data");
        fs::write(&data, b"must remain unchanged").unwrap();
        let before = fs::read(&data).unwrap();

        let data_plan = store
            .plan_initialization(&id, revision, &record.runtime, Some(false), |_| Ok(()))
            .unwrap();
        assert_eq!(data_plan.state, PrefixInitializationPlanState::Blocked);
        assert!(
            data_plan
                .blockers
                .contains(&PrefixInitializationBlocker::PrefixContainsData)
        );

        let unknown_recovery = store
            .plan_initialization(&id, revision, &record.runtime, None, |_| Ok(()))
            .unwrap();
        assert_eq!(
            unknown_recovery.state,
            PrefixInitializationPlanState::Blocked
        );
        assert!(
            unknown_recovery
                .blockers
                .contains(&PrefixInitializationBlocker::RecoveryUnknown)
        );
        let pending_recovery = store
            .plan_initialization(&id, revision, &record.runtime, Some(true), |_| Ok(()))
            .unwrap();
        assert!(
            pending_recovery
                .blockers
                .contains(&PrefixInitializationBlocker::RecoveryPending)
        );
        assert_eq!(fs::read(&data).unwrap(), before);
    }

    #[test]
    fn initialization_plan_binds_the_exact_runtime_and_revalidation_result() {
        let (_temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("init-runtime").unwrap();
        let (record, revision) = store.create(id.clone(), scope, runtime.clone(), 0).unwrap();

        let mut changed_association = runtime.clone();
        changed_association.pin_id = "replacement-pin".into();
        let mismatch = store
            .plan_initialization(&id, revision, &changed_association, Some(false), |_| Ok(()))
            .unwrap();
        assert!(
            mismatch
                .blockers
                .contains(&PrefixInitializationBlocker::RuntimeAssociationMismatch)
        );

        let stale = store
            .plan_initialization(&id, revision, &record.runtime, Some(false), |_| {
                Err("runtime audit failed".into())
            })
            .unwrap();
        assert!(
            stale
                .blockers
                .contains(&PrefixInitializationBlocker::RuntimeStale)
        );
    }

    #[test]
    fn concurrent_initialization_requests_only_produce_the_same_read_only_plan() {
        let (_temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("init-concurrent").unwrap();
        let (record, revision) = store.create(id.clone(), scope, runtime, 0).unwrap();
        let registry_before = store.fs.load_registry_bytes().unwrap();
        let prefix_before = store.fs.inspect_directory(&id).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let worker_store = PrefixStore::with_filesystem(Arc::clone(&store.fs));
            let worker_id = id.clone();
            let worker_runtime = record.runtime.clone();
            let worker_barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                worker_barrier.wait();
                worker_store
                    .plan_initialization(&worker_id, revision, &worker_runtime, Some(false), |_| {
                        Ok(())
                    })
                    .unwrap()
            }));
        }
        let plans = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(plans[0], plans[1]);
        assert!(!plans[0].execution_supported);
        assert_eq!(store.fs.load_registry_bytes().unwrap(), registry_before);
        assert_eq!(store.fs.inspect_directory(&id).unwrap(), prefix_before);
        assert!(store.fs.directory_is_empty(&id).unwrap());
    }

    #[test]
    fn stored_completed_initialization_claim_is_not_reported_as_verified() {
        let (_temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("forged-init-evidence").unwrap();
        let (record, revision) = store.create(id.clone(), scope, runtime, 0).unwrap();
        let snapshot = store.load_snapshot().unwrap();
        let mut registry = snapshot.registry;
        let stored = registry
            .records
            .iter_mut()
            .find(|record| record.id == id)
            .unwrap();
        stored.lifecycle = PrefixLifecycleState::Initialized;
        stored.initialization_evidence = Some(PrefixInitializationEvidence {
            adapter_id: "claimed-adapter".into(),
            runtime_reference_sha256: runtime_reference_sha256(&record.runtime.runtime).unwrap(),
            verification_evidence_sha256: "f".repeat(64),
            outcome: InitializationEvidenceOutcome::CompletedAndVerified,
            observed_at_unix_seconds: record.update_time_unix_seconds,
        });
        let encoded = registry.encode().unwrap();
        store
            .fs
            .save_registry_bytes(snapshot.bytes.as_deref(), &encoded)
            .unwrap();

        let report = store.validate(&id, |_| Ok(())).unwrap();
        assert_eq!(report.registry_revision, revision);
        assert_eq!(report.observed_lifecycle, PrefixLifecycleState::Initialized);
        assert_eq!(
            report.initialization_evidence,
            PrefixInitializationEvidenceStatus::CompletedClaimUnverified
        );
    }

    #[test]
    fn unregister_preserves_bytes_and_prevents_silent_adoption() {
        let (_temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("unregistered").unwrap();
        let (record, revision) = store
            .create(id.clone(), scope.clone(), runtime.clone(), 0)
            .unwrap();
        let target = store.target_path(&id);
        fs::write(target.join("keep"), b"preserved bytes").unwrap();

        let revision = store.unregister(&id, revision).unwrap();
        assert_eq!(revision, 3);
        assert_eq!(fs::read(target.join("keep")).unwrap(), b"preserved bytes");
        assert!(matches!(
            store.plan(id, scope, runtime, Some(revision)),
            Err(PrefixError::DestinationCollision)
        ));
        assert_eq!(record.lifecycle, PrefixLifecycleState::Created);
    }

    #[test]
    fn plan_rejects_storage_without_write_permission_and_does_not_fix_it() {
        let (temporary, store, scope, runtime) = fixture();
        store.fs.prepare(true).unwrap();
        let prefix_data = temporary.path().join("data/endfield-unix/prefixes/data");
        fs::set_permissions(&prefix_data, fs::Permissions::from_mode(0o500)).unwrap();
        let id = PrefixId::new("read-only").unwrap();
        assert!(matches!(
            store.plan(id.clone(), scope.clone(), runtime.clone(), Some(0)),
            Err(PrefixError::Blocked)
        ));
        assert!(matches!(
            store.create(id.clone(), scope, runtime, 0),
            Err(PrefixError::Blocked)
        ));
        assert_eq!(fs::metadata(&prefix_data).unwrap().mode() & 0o777, 0o500);
        assert!(!store.target_path(&id).exists());
    }

    #[test]
    fn conflicting_creation_and_revision_updates_fail_closed() {
        let (_temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("machine-one").unwrap();
        store
            .create(id.clone(), scope.clone(), runtime.clone(), 0)
            .unwrap();
        let other_scope = PrefixScope::profile(
            match &scope {
                PrefixScope::Machine { machine } => machine.clone(),
                _ => unreachable!(),
            },
            ProfileKey::new("game-one").unwrap(),
        );
        assert!(matches!(
            store.create(id.clone(), other_scope, runtime.clone(), 2),
            Err(PrefixError::IdentityConflict)
        ));
        assert!(matches!(
            store.create(PrefixId::new("different").unwrap(), scope, runtime, 0),
            Err(PrefixError::RevisionConflict)
        ));
    }

    #[test]
    fn interrupted_create_is_recovered_only_when_the_empty_directory_matches() {
        let (_temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("recover-me").unwrap();
        store.fs.prepare(true).unwrap();
        let _guard = store.fs.lock_registry().unwrap();
        let snapshot = store.load_snapshot().unwrap();
        let identity = store.fs.create_directory(&id).unwrap();
        let now = now();
        let record = PrefixRecord {
            id: id.clone(),
            scope,
            path_identity: "data/recover-me".into(),
            ownership: PrefixOwnership::EndfieldUnixManaged,
            owner_uid: current_uid(),
            runtime,
            lifecycle: PrefixLifecycleState::RecoveryRequired,
            filesystem_identity: Some(identity),
            creation_time_unix_seconds: now,
            update_time_unix_seconds: now,
            generation: 1,
            initialization_evidence: None,
            initialization_intent: None,
        };
        let mut registry = snapshot.registry;
        registry.revision = 1;
        registry.records.push(record);
        let bytes = registry.encode().unwrap();
        store.fs.save_registry_bytes(None, &bytes).unwrap();
        drop(_guard);
        let (recovered, revision) = store.recover(&id, 1).unwrap();
        assert_eq!(recovered.lifecycle, PrefixLifecycleState::Created);
        assert_eq!(revision, 2);
    }

    #[test]
    fn removal_never_follows_prefix_symlinks_to_external_data() {
        let (temporary, original_store, scope, runtime) = fixture();
        let data_dir = original_store
            .fs
            .root_path()
            .parent()
            .unwrap()
            .to_path_buf();
        let store = PrefixStore::with_filesystem(Arc::new(FixedUsageFilesystem {
            inner: LinuxPrefixFilesystem::new(&data_dir),
            in_use: Some(false),
            synthetic_wine_completion: false,
        }));
        let id = PrefixId::new("delete-me").unwrap();
        store.create(id.clone(), scope, runtime, 0).unwrap();
        let external = temporary.path().join("external-prefix");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("sentinel"), b"keep").unwrap();
        let link = store.target_path(&id).join("outside");
        symlink(&external, &link).unwrap();
        assert!(matches!(
            store.delete_data(&id, 2, Path::new("/wrong")),
            Err(PrefixError::ConfirmationMismatch)
        ));
        assert_eq!(
            store.delete_data(&id, 2, &store.target_path(&id)).unwrap(),
            4
        );
        assert_eq!(fs::read(external.join("sentinel")).unwrap(), b"keep");
        assert!(!link.exists());
    }

    #[test]
    fn destructive_removal_refuses_active_or_unverifiable_use() {
        let (_temporary, original_store, scope, runtime) = fixture();
        let data_dir = original_store
            .fs
            .root_path()
            .parent()
            .unwrap()
            .to_path_buf();
        let store = PrefixStore::with_filesystem(Arc::new(FixedUsageFilesystem {
            inner: LinuxPrefixFilesystem::new(&data_dir),
            in_use: Some(true),
            synthetic_wine_completion: false,
        }));
        let id = PrefixId::new("active").unwrap();
        store.create(id.clone(), scope, runtime, 0).unwrap();
        let target = store.target_path(&id);
        assert_eq!(store.fs.prefix_in_use(&id).unwrap(), Some(true));
        assert!(matches!(
            store.delete_data(&id, 2, &target),
            Err(PrefixError::ActiveUse)
        ));
        let unknown_store = PrefixStore::with_filesystem(Arc::new(FixedUsageFilesystem {
            inner: LinuxPrefixFilesystem::new(&data_dir),
            in_use: None,
            synthetic_wine_completion: false,
        }));
        assert!(matches!(
            unknown_store.delete_data(&id, 2, &target),
            Err(PrefixError::UsageUnknown)
        ));
        let inactive_store = PrefixStore::with_filesystem(Arc::new(FixedUsageFilesystem {
            inner: LinuxPrefixFilesystem::new(&data_dir),
            in_use: Some(false),
            synthetic_wine_completion: false,
        }));
        assert_eq!(inactive_store.delete_data(&id, 2, &target).unwrap(), 4);
    }

    #[test]
    fn external_destination_collision_and_symlink_root_are_blocked() {
        let (temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("collision").unwrap();
        store.fs.prepare(true).unwrap();
        let target = store.target_path(&id);
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(target.join("keep"), b"external").unwrap();
        assert!(matches!(
            store.create(id, scope, runtime, 0),
            Err(PrefixError::DestinationCollision)
        ));
        assert_eq!(fs::read(target.join("keep")).unwrap(), b"external");

        let outside = temporary.path().join("outside-root");
        fs::create_dir(&outside).unwrap();
        let app_data = temporary.path().join("data/endfield-unix");
        fs::remove_dir_all(app_data.join("prefixes")).unwrap();
        symlink(&outside, app_data.join("prefixes")).unwrap();
        assert!(matches!(store.list(|_| Ok(())), Err(PrefixError::Blocked)));
        assert!(fs::read_dir(outside).unwrap().next().is_none());
    }

    #[test]
    fn corrupt_registry_is_preserved_and_never_reset() {
        let (temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("corrupt").unwrap();
        store.create(id, scope, runtime, 0).unwrap();
        let registry = temporary
            .path()
            .join("data/endfield-unix/prefixes/registry.json");
        fs::write(&registry, b"{corrupt").unwrap();
        assert!(matches!(
            store.list(|_| Ok(())),
            Err(PrefixError::RegistryCorrupt)
        ));
        assert_eq!(fs::read(&registry).unwrap(), b"{corrupt");
    }

    #[test]
    fn unsupported_registry_schema_fails_closed_without_rewrite() {
        let (temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("schema").unwrap();
        store.create(id, scope, runtime, 0).unwrap();
        let registry = temporary
            .path()
            .join("data/endfield-unix/prefixes/registry.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&registry).unwrap()).unwrap();
        value["schema_version"] = serde_json::Value::from(3);
        let unsupported = serde_json::to_vec(&value).unwrap();
        fs::write(&registry, &unsupported).unwrap();
        assert!(matches!(
            store.list(|_| Ok(())),
            Err(PrefixError::UnsupportedSchemaVersion(3))
        ));
        assert_eq!(fs::read(registry).unwrap(), unsupported);
    }

    #[test]
    fn replaced_registry_symlink_is_blocked_and_external_file_is_preserved() {
        let (temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("registry-link").unwrap();
        store.create(id, scope, runtime, 0).unwrap();
        let root = temporary.path().join("data/endfield-unix/prefixes");
        let registry = root.join("registry.json");
        let saved = root.join("registry.saved");
        fs::rename(&registry, &saved).unwrap();
        let external = temporary.path().join("external-registry.json");
        fs::write(&external, b"preserve").unwrap();
        symlink(&external, &registry).unwrap();
        assert!(matches!(store.list(|_| Ok(())), Err(PrefixError::Blocked)));
        assert_eq!(fs::read(&external).unwrap(), b"preserve");
    }

    #[test]
    fn missing_prefix_is_reported_and_never_silently_recreated() {
        let (_temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("missing").unwrap();
        store
            .create(id.clone(), scope.clone(), runtime.clone(), 0)
            .unwrap();
        fs::remove_dir_all(store.target_path(&id)).unwrap();
        let report = store.validate(&id, |_| Ok(())).unwrap();
        assert_eq!(report.observed_lifecycle, PrefixLifecycleState::Missing);
        assert!(matches!(
            store.create(id, scope, runtime, 2),
            Err(PrefixError::IdentityConflict)
        ));
    }

    #[test]
    fn identity_rejects_traversal_and_multiple_independent_prefixes_are_supported() {
        assert!(matches!(
            PrefixId::new("../external"),
            Err(PrefixError::InvalidId)
        ));
        let (_temporary, store, scope, runtime) = fixture();
        let first = PrefixId::new("first").unwrap();
        let second = PrefixId::new("second").unwrap();
        store
            .create(first, scope.clone(), runtime.clone(), 0)
            .unwrap();
        store.create(second, scope, runtime, 2).unwrap();
        let (registry, items) = store.list(|_| Ok(())).unwrap();
        assert_eq!(registry.records.len(), 2);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn validation_reports_stale_runtime_without_mutating_prefix_or_registry() {
        let (_temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("stale-runtime").unwrap();
        store.create(id.clone(), scope, runtime, 0).unwrap();
        let path = store.target_path(&id);
        let before = fs::metadata(&path).unwrap().ino();
        let report = store
            .validate(&id, |_| Err("managed installation changed".into()))
            .unwrap();
        assert_eq!(report.observed_lifecycle, PrefixLifecycleState::Created);
        assert_eq!(
            report.runtime,
            RuntimeValidation::Stale("managed installation changed".into())
        );
        assert_eq!(fs::metadata(path).unwrap().ino(), before);
        assert_eq!(report.registry_revision, 2);
    }

    #[test]
    fn concurrent_conflicting_creates_have_one_owner() {
        let (_temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("race").unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for other_scope in [
            scope.clone(),
            PrefixScope::profile(
                match &scope {
                    PrefixScope::Machine { machine } => machine.clone(),
                    _ => unreachable!(),
                },
                ProfileKey::new("race-profile").unwrap(),
            ),
        ] {
            let store = PrefixStore::with_filesystem(Arc::clone(&store.fs));
            let barrier = Arc::clone(&barrier);
            let runtime = runtime.clone();
            let id = id.clone();
            workers.push(thread::spawn(move || {
                barrier.wait();
                store.create(id, other_scope, runtime, 0)
            }));
        }
        barrier.wait();
        let results = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(PrefixError::IdentityConflict)))
                .count(),
            1
        );
    }

    #[test]
    fn managed_roots_and_records_are_private_to_the_invoking_user() {
        let (temporary, store, scope, runtime) = fixture();
        let id = PrefixId::new("permissions").unwrap();
        store.create(id.clone(), scope, runtime, 0).unwrap();
        let root = temporary.path().join("data/endfield-unix/prefixes");
        let target = store.target_path(&id);
        assert_eq!(fs::metadata(root).unwrap().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(&target).unwrap().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(&target).unwrap().uid(), unsafe {
            libc::geteuid()
        });
        assert_eq!(
            fs::metadata(
                temporary
                    .path()
                    .join("data/endfield-unix/prefixes/registry.json")
            )
            .unwrap()
            .mode()
                & 0o777,
            0o600
        );
    }
}
