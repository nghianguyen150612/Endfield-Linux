//! Explicit, metadata-only registration of an existing game directory.
//!
//! The subsystem records a user declaration and bounded filesystem
//! observations. It does not know an authoritative Endfield layout, execute
//! files, modify the selected directory, or establish publisher authenticity.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

mod linux;

pub const REGISTRY_SCHEMA_VERSION: u32 = 1;
pub const MAX_REGISTRY_BYTES: usize = 1024 * 1024;
pub const MAX_INSTALLATIONS: usize = 1024;
pub const MAX_ID_BYTES: usize = 96;

fn filesystem_type_supported(filesystem_type: &str) -> bool {
    matches!(
        filesystem_type,
        "ext-family"
            | "xfs"
            | "btrfs"
            | "f2fs"
            | "ntfs3"
            | "fuse-or-fuseblk"
            | "exfat"
            | "vfat"
            | "cifs"
            | "nfs"
            | "tmpfs"
            | "overlay"
    )
}

fn filesystem_has_unix_ownership_semantics(filesystem_type: &str) -> bool {
    matches!(filesystem_type, "ext-family" | "xfs" | "btrfs" | "f2fs")
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GameInstallationId(String);

impl GameInstallationId {
    pub fn new(value: impl Into<String>) -> Result<Self, GameImportError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_ID_BYTES
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        {
            return Err(GameImportError::InvalidId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameInstallationScope {
    User,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameInstallationState {
    Registered,
    StructureObserved,
    IdentityUnverified,
    IdentityVerified,
    Changed,
    Missing,
    Blocked,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameIdentityConfidence {
    UserDeclaredUnverified,
    Verified,
    NotEstablished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuralEvidenceState {
    RootObserved,
    NoAuthoritativeFormatKnown,
    Changed,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootIdentity {
    pub device: u64,
    pub inode: u64,
    pub mount_id: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesystemCapabilities {
    pub filesystem_type: String,
    pub mount_id_observed: bool,
    pub object_identity_observed: bool,
    pub unix_ownership_semantics_supported: bool,
    pub identity_guarantee: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootEntrySummary {
    pub total: u32,
    pub directories: u32,
    pub regular_files: u32,
    pub symlinks: u32,
    pub special_files: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameInstallationEvidence {
    pub user_declared_endfield: bool,
    pub identity_confidence: GameIdentityConfidence,
    pub structural_state: StructuralEvidenceState,
    pub root_entries: RootEntrySummary,
    pub game_build: Option<String>,
    pub publisher_authenticity_verified: bool,
    pub executable_candidates: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameInstallationRecord {
    pub id: GameInstallationId,
    pub scope: GameInstallationScope,
    pub state: GameInstallationState,
    pub selected_path: String,
    pub root_identity: RootIdentity,
    pub filesystem: FilesystemCapabilities,
    pub evidence: GameInstallationEvidence,
    pub registered_at_unix_seconds: u64,
}

impl GameInstallationRecord {
    /// The external location is disclosed only to explicit local inspection.
    pub fn selected_path(&self) -> &Path {
        Path::new(&self.selected_path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameImportPlan {
    pub schema_version: u32,
    pub proposed_id: GameInstallationId,
    pub selected_path: String,
    pub root_identity: RootIdentity,
    pub scope: GameInstallationScope,
    pub filesystem: FilesystemCapabilities,
    pub evidence: GameInstallationEvidence,
    pub registry_revision: u64,
    pub existing_id_collision: bool,
    pub existing_location_collision: Option<GameInstallationId>,
    pub can_register: bool,
    pub required_confirmation: Option<String>,
    pub expected_storage_effects: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameInstallationSnapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub installations: Vec<GameInstallationRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameInstallationValidation {
    pub schema_version: u32,
    pub id: GameInstallationId,
    pub state: GameInstallationState,
    pub registry_revision: u64,
    pub registered_path: String,
    pub observed_root_identity: Option<RootIdentity>,
    pub filesystem: Option<FilesystemCapabilities>,
    pub evidence: GameInstallationEvidence,
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameUnregisterPlan {
    pub schema_version: u32,
    pub id: GameInstallationId,
    pub registry_revision: u64,
    pub selected_path: String,
    pub required_confirmation: String,
    pub effects: String,
}

#[derive(Debug, Error)]
pub enum GameImportError {
    #[error("installation ID is invalid")]
    InvalidId,
    #[error("selected path is not an absolute supported path")]
    InvalidPath,
    #[error("selected location is missing")]
    Missing,
    #[error("selected location is not a directory")]
    NotDirectory,
    #[error("selected directory is empty")]
    EmptyLocation,
    #[error("selected directory exceeds the bounded inspection limit")]
    InspectionLimit,
    #[error("selected directory contains links or special entries at its root")]
    UnsupportedEntries,
    #[error("selected filesystem identity cannot be observed safely")]
    FilesystemIdentityUnavailable,
    #[error("selected filesystem type is not recognized for bounded identity checks")]
    UnsupportedFilesystem,
    #[error("selected location is not readable")]
    AccessDenied,
    #[error("installation ID is already registered")]
    DuplicateId,
    #[error("selected location is already registered under another ID")]
    DuplicateLocation,
    #[error("installation is not registered")]
    NotRegistered,
    #[error("explicit user declaration that the directory contains Endfield is required")]
    DeclarationRequired,
    #[error("registry revision changed; reload the plan and retry")]
    RevisionConflict,
    #[error("confirmation does not exactly match the current plan")]
    ConfirmationMismatch,
    #[error("registered location changed since import planning")]
    ChangedSincePlan,
    #[error("registered location is missing")]
    RegisteredLocationMissing,
    #[error("registered location cannot be safely revalidated")]
    RevalidationBlocked,
    #[error("registry is corrupt or uses an unsupported schema")]
    RegistryCorrupt,
    #[error("registry uses unsupported schema version {0}")]
    UnsupportedSchemaVersion(u64),
    #[error("registry operation requires recovery from interrupted metadata publication")]
    RecoveryRequired,
    #[error("application metadata storage is unsafe or unavailable")]
    StorageUnavailable,
    #[error("registry publication failed before commit")]
    PublicationFailed,
    #[error("registry publication may have committed but durability is uncertain")]
    CommitUncertain,
    #[error("registry revision is exhausted")]
    RevisionExhausted,
    #[error("I/O operation failed")]
    Io(#[source] std::io::Error),
}

impl GameImportError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidId => "invalid_id",
            Self::InvalidPath => "invalid_path",
            Self::Missing => "missing",
            Self::NotDirectory => "not_directory",
            Self::EmptyLocation => "empty_location",
            Self::InspectionLimit => "inspection_limit",
            Self::UnsupportedEntries => "unsupported_entries",
            Self::FilesystemIdentityUnavailable => "filesystem_identity_unavailable",
            Self::UnsupportedFilesystem => "unsupported_filesystem",
            Self::AccessDenied => "access_denied",
            Self::DuplicateId => "duplicate_id",
            Self::DuplicateLocation => "duplicate_location",
            Self::NotRegistered => "not_registered",
            Self::DeclarationRequired => "declaration_required",
            Self::RevisionConflict => "revision_conflict",
            Self::ConfirmationMismatch => "confirmation_mismatch",
            Self::ChangedSincePlan => "changed_since_plan",
            Self::RegisteredLocationMissing => "registered_location_missing",
            Self::RevalidationBlocked => "revalidation_blocked",
            Self::RegistryCorrupt => "registry_corrupt",
            Self::UnsupportedSchemaVersion(_) => "unsupported_schema_version",
            Self::RecoveryRequired => "recovery_required",
            Self::StorageUnavailable => "storage_unavailable",
            Self::PublicationFailed => "publication_failed",
            Self::CommitUncertain => "commit_uncertain",
            Self::RevisionExhausted => "revision_exhausted",
            Self::Io(_) => "io_error",
        }
    }

    pub fn exit_status(&self) -> u8 {
        match self {
            Self::InvalidId
            | Self::InvalidPath
            | Self::DeclarationRequired
            | Self::ConfirmationMismatch => 2,
            Self::Io(_) | Self::PublicationFailed | Self::CommitUncertain => 74,
            Self::Missing
            | Self::NotDirectory
            | Self::EmptyLocation
            | Self::AccessDenied
            | Self::RegisteredLocationMissing => 3,
            Self::RevisionConflict
            | Self::ChangedSincePlan
            | Self::DuplicateId
            | Self::DuplicateLocation
            | Self::NotRegistered => 4,
            Self::InspectionLimit
            | Self::UnsupportedEntries
            | Self::FilesystemIdentityUnavailable
            | Self::UnsupportedFilesystem
            | Self::RevalidationBlocked => 78,
            Self::RegistryCorrupt
            | Self::UnsupportedSchemaVersion(_)
            | Self::RecoveryRequired
            | Self::StorageUnavailable
            | Self::RevisionExhausted => 70,
        }
    }
}

#[derive(Clone, Debug)]
pub struct GameInstallationRegistry {
    data_dir: PathBuf,
    app_paths: Option<endfield_unix_storage::AppPaths>,
}

impl GameInstallationRegistry {
    /// Uses the P006 application-owned data directory without creating it.
    pub fn for_app(paths: &endfield_unix_storage::AppPaths) -> Self {
        Self {
            data_dir: paths.data_dir().to_path_buf(),
            app_paths: Some(paths.clone()),
        }
    }

    /// Creates an isolated registry at an already chosen data-directory root.
    /// Primarily useful to embedders and isolated tests.
    pub fn at_data_dir(path: impl AsRef<Path>) -> Result<Self, GameImportError> {
        if !path.as_ref().is_absolute() {
            return Err(GameImportError::InvalidPath);
        }
        Ok(Self {
            data_dir: path.as_ref().to_path_buf(),
            app_paths: None,
        })
    }

    pub fn load_read_only(&self) -> Result<GameInstallationSnapshot, GameImportError> {
        linux::load_snapshot(&self.data_dir)
    }

    pub fn plan_import(
        &self,
        id: GameInstallationId,
        path: impl AsRef<Path>,
        user_declared_endfield: bool,
    ) -> Result<GameImportPlan, GameImportError> {
        let snapshot = self.load_read_only()?;
        let observed = linux::observe_root(path.as_ref())?;
        make_import_plan(snapshot, id, observed, user_declared_endfield)
    }

    pub fn apply_import(
        &self,
        id: GameInstallationId,
        path: impl AsRef<Path>,
        user_declared_endfield: bool,
        expected_revision: u64,
        confirmation: &str,
    ) -> Result<(GameInstallationRecord, u64), GameImportError> {
        linux::apply_import(
            &self.data_dir,
            self.app_paths.as_ref(),
            id,
            path.as_ref(),
            user_declared_endfield,
            expected_revision,
            confirmation,
        )
    }

    pub fn validate(
        &self,
        id: &GameInstallationId,
    ) -> Result<GameInstallationValidation, GameImportError> {
        let snapshot = self.load_read_only()?;
        let record = snapshot
            .installations
            .iter()
            .find(|record| &record.id == id)
            .ok_or(GameImportError::NotRegistered)?;
        linux::validate_installation(record, snapshot.revision)
    }

    pub fn plan_unregister(
        &self,
        id: &GameInstallationId,
    ) -> Result<GameUnregisterPlan, GameImportError> {
        let snapshot = self.load_read_only()?;
        let record = snapshot
            .installations
            .iter()
            .find(|record| &record.id == id)
            .ok_or(GameImportError::NotRegistered)?;
        Ok(GameUnregisterPlan {
            schema_version: REGISTRY_SCHEMA_VERSION,
            id: id.clone(),
            registry_revision: snapshot.revision,
            selected_path: record.selected_path.clone(),
            required_confirmation: format!("unregister:{}:{}", id.as_str(), snapshot.revision),
            effects: "Remove only this Endfield-Unix registry record. The selected directory and all game files remain untouched.".into(),
        })
    }

    pub fn unregister(
        &self,
        id: &GameInstallationId,
        expected_revision: u64,
        confirmation: &str,
    ) -> Result<u64, GameImportError> {
        linux::unregister(
            &self.data_dir,
            self.app_paths.as_ref(),
            id,
            expected_revision,
            confirmation,
        )
    }
}

struct RootObservation {
    selected_path: String,
    root_identity: RootIdentity,
    filesystem: FilesystemCapabilities,
    root_entries: RootEntrySummary,
}

fn make_import_plan(
    snapshot: GameInstallationSnapshot,
    id: GameInstallationId,
    observed: RootObservation,
    user_declared_endfield: bool,
) -> Result<GameImportPlan, GameImportError> {
    let existing_id_collision = snapshot.installations.iter().any(|record| record.id == id);
    let existing_location_collision = snapshot
        .installations
        .iter()
        .find(|record| {
            record.selected_path == observed.selected_path
                || (record.root_identity.device == observed.root_identity.device
                    && record.root_identity.inode == observed.root_identity.inode)
        })
        .map(|record| record.id.clone());
    let unsupported_entries =
        observed.root_entries.symlinks > 0 || observed.root_entries.special_files > 0;
    let unsupported_filesystem = !filesystem_type_supported(&observed.filesystem.filesystem_type);
    let identity_confidence = if user_declared_endfield {
        GameIdentityConfidence::UserDeclaredUnverified
    } else {
        GameIdentityConfidence::NotEstablished
    };
    let mut warnings = vec![
        "No authoritative Endfield directory layout or marker format is configured; root contents are structural observations only.".into(),
        "The selected directory's Endfield identity is not verified; folder and executable names are not treated as authentication.".into(),
        "No executable candidates, game build, publisher authenticity, or compatibility are inferred.".into(),
    ];
    if !user_declared_endfield {
        warnings.push("An explicit declaration that this selected directory contains Endfield is required before registration.".into());
    }
    if observed.root_identity.mount_id.is_none() {
        warnings.push(
            "The current mount identity could not be observed; safe revalidation is unavailable."
                .into(),
        );
    }
    if unsupported_entries {
        warnings.push(
            "Symlinks or special files were found at the selected root and are never followed; registration is blocked."
                .into(),
        );
    }
    if unsupported_filesystem {
        warnings.push(
            "The filesystem type is not recognized, so this location cannot be registered safely."
                .into(),
        );
    }
    if observed.filesystem.filesystem_type == "tmpfs" {
        warnings.push(
            "The selected location is on volatile tmpfs storage and may disappear after shutdown."
                .into(),
        );
    }
    warnings.push(
        "Only the selected directory's top-level entry types are inspected. Nested changes and file-byte changes may not be detected during revalidation.".into(),
    );
    warnings.push(
        "Application metadata writeability and registry-filesystem support are checked during approved apply; planning creates no storage to test them.".into(),
    );
    if existing_id_collision {
        warnings.push("The proposed installation ID is already registered.".into());
    }
    if existing_location_collision.is_some() {
        warnings.push("The selected filesystem object is already registered.".into());
    }
    let can_register = user_declared_endfield
        && !existing_id_collision
        && existing_location_collision.is_none()
        && !unsupported_entries
        && !unsupported_filesystem
        && observed.root_identity.mount_id.is_some();
    let confirmation = can_register.then(|| {
        let digest = confirmation_digest(
            &id,
            snapshot.revision,
            &observed.selected_path,
            &observed.root_identity,
            &observed.filesystem,
            &observed.root_entries,
        );
        format!("register:{}:{}:{digest}", id.as_str(), snapshot.revision)
    });
    Ok(GameImportPlan {
        schema_version: REGISTRY_SCHEMA_VERSION,
        proposed_id: id,
        selected_path: observed.selected_path,
        root_identity: observed.root_identity,
        scope: GameInstallationScope::User,
        filesystem: observed.filesystem,
        evidence: GameInstallationEvidence {
            user_declared_endfield,
            identity_confidence,
            structural_state: StructuralEvidenceState::NoAuthoritativeFormatKnown,
            root_entries: observed.root_entries,
            game_build: None,
            publisher_authenticity_verified: false,
            executable_candidates: Vec::new(),
            limitations: vec![
                "Only the selected root object and bounded top-level entry types were observed.".into(),
                "Nested directories and file contents were not traversed; changes below the selected root may not be detected.".into(),
                "No official Endfield layout or manifest was available in the repository evidence.".into(),
                "No game files were opened, hashed, or executed.".into(),
            ],
        },
        registry_revision: snapshot.revision,
        existing_id_collision,
        existing_location_collision,
        can_register,
        required_confirmation: confirmation,
        expected_storage_effects: "On explicit approval, write a private versioned registry record under Endfield-Unix application data. No game-directory files, permissions, ownership, or links are changed.".into(),
        warnings,
    })
}

fn confirmation_digest(
    id: &GameInstallationId,
    revision: u64,
    selected_path: &str,
    identity: &RootIdentity,
    filesystem: &FilesystemCapabilities,
    entries: &RootEntrySummary,
) -> String {
    use sha2::{Digest, Sha256};
    let encoded = serde_json::to_vec(&(
        id.as_str(),
        revision,
        selected_path,
        identity,
        filesystem,
        entries,
    ))
    .unwrap_or_default();
    let digest = Sha256::digest(encoded);
    let mut result = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(result, "{byte:02x}");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installation_ids_are_bounded_ascii_keys() {
        assert!(GameInstallationId::new("endfield-main_1").is_ok());
        assert!(GameInstallationId::new("").is_err());
        assert!(GameInstallationId::new("../game").is_err());
        assert!(GameInstallationId::new("é").is_err());
        assert!(GameInstallationId::new("x".repeat(MAX_ID_BYTES + 1)).is_err());
    }

    #[test]
    fn identity_and_structure_confidence_are_independent() {
        let evidence = GameInstallationEvidence {
            user_declared_endfield: true,
            identity_confidence: GameIdentityConfidence::UserDeclaredUnverified,
            structural_state: StructuralEvidenceState::NoAuthoritativeFormatKnown,
            root_entries: RootEntrySummary {
                total: 1,
                directories: 0,
                regular_files: 1,
                symlinks: 0,
                special_files: 0,
            },
            game_build: None,
            publisher_authenticity_verified: false,
            executable_candidates: Vec::new(),
            limitations: vec!["test".into()],
        };
        assert_eq!(
            evidence.identity_confidence,
            GameIdentityConfidence::UserDeclaredUnverified
        );
        assert_eq!(
            evidence.structural_state,
            StructuralEvidenceState::NoAuthoritativeFormatKnown
        );
        assert!(!evidence.publisher_authenticity_verified);
        assert!(evidence.game_build.is_none());
    }

    #[test]
    fn filesystem_support_is_explicit_and_covers_mounted_ntfs_and_exfat() {
        for filesystem in ["ext-family", "ntfs3", "fuse-or-fuseblk", "exfat"] {
            assert!(filesystem_type_supported(filesystem), "{filesystem}");
        }
        assert!(!filesystem_type_supported("unknown"));
        assert!(!filesystem_type_supported("unrecognized-fs"));
        assert!(filesystem_has_unix_ownership_semantics("ext-family"));
        assert!(!filesystem_has_unix_ownership_semantics("ntfs3"));
        assert!(!filesystem_has_unix_ownership_semantics("exfat"));
        assert!(!filesystem_has_unix_ownership_semantics("fuse-or-fuseblk"));
    }
}
