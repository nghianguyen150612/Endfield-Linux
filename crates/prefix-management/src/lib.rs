//! Explicit, isolated Wine prefix directory management.
//!
//! This crate never runs Wine, Proton, UMU, `wineboot`, or a game. Prefix
//! directory creation and Wine initialization are separate lifecycle steps.

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

mod linux_fs;
pub use linux_fs::LinuxPrefixFilesystem;

pub const REGISTRY_SCHEMA_VERSION: u32 = 1;
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

    fn validate(&self) -> Result<(), PrefixError> {
        if self.schema_version != REGISTRY_SCHEMA_VERSION || self.records.len() > MAX_PREFIXES {
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
        if schema_version != u64::from(REGISTRY_SCHEMA_VERSION) {
            return Err(PrefixError::UnsupportedSchemaVersion(schema_version));
        }
        let registry: Self =
            serde_json::from_value(value).map_err(|_| PrefixError::RegistryCorrupt)?;
        registry.validate()?;
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
}

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
        if current.initialization_evidence.is_some() {
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
        let snapshot = self.load_snapshot()?;
        if snapshot.registry.revision != expected_revision {
            return Err(PrefixError::RevisionConflict);
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
        let lifecycle = match self.fs.inspect_directory(&record.id) {
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
mod tests {
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
    }

    fn fixture() -> (TempDir, PrefixStore, PrefixScope, PrefixRuntimeAssociation) {
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
        }));
        assert!(matches!(
            unknown_store.delete_data(&id, 2, &target),
            Err(PrefixError::UsageUnknown)
        ));
        let inactive_store = PrefixStore::with_filesystem(Arc::new(FixedUsageFilesystem {
            inner: LinuxPrefixFilesystem::new(&data_dir),
            in_use: Some(false),
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
        value["schema_version"] = serde_json::Value::from(2);
        let unsupported = serde_json::to_vec(&value).unwrap();
        fs::write(&registry, &unsupported).unwrap();
        assert!(matches!(
            store.list(|_| Ok(())),
            Err(PrefixError::UnsupportedSchemaVersion(2))
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
