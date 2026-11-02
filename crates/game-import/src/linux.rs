//! Linux-only filesystem observation and application-owned registry storage.
use crate::{
    FilesystemCapabilities, GameIdentityConfidence, GameImportError, GameImportPlan,
    GameInstallationEvidence, GameInstallationId, GameInstallationRecord, GameInstallationScope,
    GameInstallationSnapshot, GameInstallationState, GameInstallationValidation,
    GameUnregisterPlan, MAX_INSTALLATIONS, MAX_REGISTRY_BYTES, REGISTRY_SCHEMA_VERSION,
    RootEntrySummary, RootIdentity, RootObservation, StructuralEvidenceState,
};
use endfield_unix_storage::AppPaths;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, CString},
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const REGISTRY_DIR: &str = "game-installations";
const MAX_PATH_BYTES: usize = 4096;
const MAX_PATH_COMPONENTS: usize = 256;
const MAX_COMPONENT_BYTES: usize = 255;
const MAX_ROOT_ENTRIES: u32 = 512;
const MAX_STALE_TEMP_FILES: usize = 32;
const IDENTITY_GUARANTEE: &str = "device, inode, and mount id compared for the current mount; persistence across remounts is not claimed";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    schema_version: u32,
    revision: u64,
    installations: Vec<GameInstallationRecord>,
    checksum: String,
}

#[derive(Serialize)]
struct RegistryChecksumPayload<'a> {
    schema_version: u32,
    revision: u64,
    installations: &'a [GameInstallationRecord],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

pub(super) fn load_snapshot(data_dir: &Path) -> Result<GameInstallationSnapshot, GameImportError> {
    let Some(data) = open_directory_path(data_dir).map_err(storage_error)? else {
        return Ok(empty_snapshot());
    };
    verify_private_directory(&data).map_err(storage_error)?;
    let Some(directory) =
        open_child_directory(&data, REGISTRY_DIR, false).map_err(storage_error)?
    else {
        return Ok(empty_snapshot());
    };
    verify_private_directory(&directory).map_err(storage_error)?;
    let Some((registry, _, _)) = read_registry(&directory)? else {
        return Ok(empty_snapshot());
    };
    Ok(GameInstallationSnapshot {
        schema_version: REGISTRY_SCHEMA_VERSION,
        revision: registry.revision,
        installations: registry.installations,
    })
}

fn empty_snapshot() -> GameInstallationSnapshot {
    GameInstallationSnapshot {
        schema_version: REGISTRY_SCHEMA_VERSION,
        revision: 0,
        installations: Vec::new(),
    }
}

pub(super) fn observe_root(path: &Path) -> Result<RootObservation, GameImportError> {
    let normalized = normalize_selected_path(path)?;
    let root = open_selected_directory(Path::new(&normalized))?;
    let metadata = root.metadata().map_err(map_selected_io)?;
    if !metadata.is_dir() {
        return Err(GameImportError::NotDirectory);
    }
    let identity = root_identity(&root, &metadata)?;
    let filesystem = filesystem_capabilities(&root, &identity)?;
    let root_entries = inspect_root_entries(&root)?;
    if root_entries.total == 0 {
        return Err(GameImportError::EmptyLocation);
    }

    // Confirm the path still resolves to the descriptor observed above. Every
    // component is reopened relative to a held descriptor with O_NOFOLLOW.
    let reopened = open_selected_directory(Path::new(&normalized))?;
    let reopened_meta = reopened.metadata().map_err(map_selected_io)?;
    let reopened_identity = root_identity(&reopened, &reopened_meta)?;
    if identity != reopened_identity {
        return Err(GameImportError::ChangedSincePlan);
    }

    Ok(RootObservation {
        selected_path: normalized,
        root_identity: identity,
        filesystem,
        root_entries,
    })
}

fn normalize_selected_path(path: &Path) -> Result<String, GameImportError> {
    if !path.is_absolute() || path.as_os_str().as_bytes().len() > MAX_PATH_BYTES {
        return Err(GameImportError::InvalidPath);
    }
    let mut normalized = PathBuf::from("/");
    let mut count = 0usize;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                let bytes = name.as_bytes();
                if bytes.is_empty()
                    || bytes.len() > MAX_COMPONENT_BYTES
                    || std::str::from_utf8(bytes).is_err()
                {
                    return Err(GameImportError::InvalidPath);
                }
                count += 1;
                if count > MAX_PATH_COMPONENTS {
                    return Err(GameImportError::InvalidPath);
                }
                normalized.push(name);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) => {
                return Err(GameImportError::InvalidPath);
            }
        }
    }
    normalized
        .to_str()
        .map(str::to_owned)
        .ok_or(GameImportError::InvalidPath)
}

fn open_selected_directory(path: &Path) -> Result<File, GameImportError> {
    let mut parent = File::open("/").map_err(map_selected_io)?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name = CString::new(name.as_bytes()).map_err(|_| GameImportError::InvalidPath)?;
        if let Some(kind) = entry_kind(parent.as_raw_fd(), &name)? {
            if kind == libc::S_IFLNK {
                return Err(GameImportError::InvalidPath);
            }
            if kind != libc::S_IFDIR {
                return Err(GameImportError::NotDirectory);
            }
        }
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY
                    | libc::O_DIRECTORY
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
                0,
            )
        };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            if matches!(
                error.raw_os_error(),
                Some(libc::ENOTDIR) | Some(libc::ELOOP)
            ) && entry_kind(parent.as_raw_fd(), &name)? == Some(libc::S_IFLNK)
            {
                return Err(GameImportError::InvalidPath);
            }
            return Err(map_selected_io(error));
        }
        parent = unsafe { File::from_raw_fd(fd) };
    }
    let metadata = parent.metadata().map_err(map_selected_io)?;
    if !metadata.is_dir() {
        return Err(GameImportError::NotDirectory);
    }
    Ok(parent)
}

fn entry_kind(directory_fd: i32, name: &CStr) -> Result<Option<u32>, GameImportError> {
    let mut metadata: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::fstatat(
            directory_fd,
            name.as_ptr(),
            &mut metadata,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        return Ok(Some(metadata.st_mode & libc::S_IFMT));
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ENOENT) {
        Ok(None)
    } else {
        Err(map_selected_io(error))
    }
}

fn map_selected_io(error: std::io::Error) -> GameImportError {
    match error.raw_os_error() {
        Some(libc::ENOENT) => GameImportError::Missing,
        Some(libc::ENOTDIR) => GameImportError::NotDirectory,
        Some(libc::EACCES) | Some(libc::EPERM) => GameImportError::AccessDenied,
        Some(libc::ELOOP) => GameImportError::InvalidPath,
        _ if error.kind() == std::io::ErrorKind::PermissionDenied => GameImportError::AccessDenied,
        _ => GameImportError::Io(error),
    }
}

fn root_identity(file: &File, metadata: &fs::Metadata) -> Result<RootIdentity, GameImportError> {
    let mount_id = read_mount_id(file).ok();
    if metadata.dev() == 0 || metadata.ino() == 0 {
        return Err(GameImportError::FilesystemIdentityUnavailable);
    }
    Ok(RootIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        mount_id,
    })
}

fn read_mount_id(file: &File) -> Result<u64, std::io::Error> {
    let path = format!("/proc/self/fdinfo/{}", file.as_raw_fd());
    let contents = fs::read_to_string(path)?;
    contents
        .lines()
        .find_map(|line| {
            line.strip_prefix("mnt_id:")
                .and_then(|value| value.trim().parse().ok())
        })
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Unsupported, "mount id unavailable"))
}

fn filesystem_capabilities(
    file: &File,
    identity: &RootIdentity,
) -> Result<FilesystemCapabilities, GameImportError> {
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatfs(file.as_raw_fd(), &mut stat) } != 0 {
        return Err(GameImportError::FilesystemIdentityUnavailable);
    }
    let magic = stat.f_type as u64;
    let filesystem_type = match magic {
        0x0000_ef53 => "ext-family",
        0x5846_5342 => "xfs",
        0x9123_683e => "btrfs",
        0xf2f5_2010 => "f2fs",
        0x5346_544e => "ntfs3",
        0x6573_5546 => "fuse-or-fuseblk",
        0x2011_bab0 => "exfat",
        0x0000_4d44 => "vfat",
        0xff53_4d42 => "cifs",
        0x0000_6969 => "nfs",
        0x0102_1994 => "tmpfs",
        0x794c_7630 => "overlay",
        _ => "unknown",
    }
    .to_owned();
    let unix_ownership_semantics_supported =
        crate::filesystem_has_unix_ownership_semantics(&filesystem_type);
    Ok(FilesystemCapabilities {
        filesystem_type,
        mount_id_observed: identity.mount_id.is_some(),
        object_identity_observed: identity.device != 0 && identity.inode != 0,
        unix_ownership_semantics_supported,
        identity_guarantee: IDENTITY_GUARANTEE.into(),
    })
}

fn inspect_root_entries(root: &File) -> Result<RootEntrySummary, GameImportError> {
    let mut summary = RootEntrySummary {
        total: 0,
        directories: 0,
        regular_files: 0,
        symlinks: 0,
        special_files: 0,
    };
    scan_directory(
        root.as_raw_fd(),
        MAX_ROOT_ENTRIES as usize,
        |kind, _name| {
            summary.total += 1;
            match kind {
                libc::S_IFDIR => summary.directories += 1,
                libc::S_IFREG => summary.regular_files += 1,
                libc::S_IFLNK => summary.symlinks += 1,
                _ => summary.special_files += 1,
            }
            Ok(())
        },
    )?;
    Ok(summary)
}

/// Enumerates only names directly in an already-open directory. Child data is
/// never opened and symlinks are classified with AT_SYMLINK_NOFOLLOW.
fn scan_directory<F>(directory_fd: i32, maximum: usize, mut visit: F) -> Result<(), GameImportError>
where
    F: FnMut(u32, &CStr) -> Result<(), GameImportError>,
{
    let duplicate = unsafe { libc::dup(directory_fd) };
    if duplicate < 0 {
        return Err(GameImportError::RevalidationBlocked);
    }
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        unsafe { libc::close(duplicate) };
        return Err(GameImportError::RevalidationBlocked);
    }
    let mut count = 0usize;
    loop {
        unsafe { *libc::__errno_location() = 0 };
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            let errno = unsafe { *libc::__errno_location() };
            unsafe { libc::closedir(stream) };
            if errno != 0 {
                return Err(GameImportError::RevalidationBlocked);
            }
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        count += 1;
        if count > maximum {
            unsafe { libc::closedir(stream) };
            return Err(GameImportError::InspectionLimit);
        }
        let mut metadata: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::fstatat(
                directory_fd,
                name.as_ptr(),
                &mut metadata,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            unsafe { libc::closedir(stream) };
            return Err(GameImportError::RevalidationBlocked);
        }
        let kind = metadata.st_mode & libc::S_IFMT;
        if let Err(error) = visit(kind, name) {
            unsafe { libc::closedir(stream) };
            return Err(error);
        }
    }
    Ok(())
}

pub(super) fn apply_import(
    data_dir: &Path,
    app_paths: Option<&AppPaths>,
    id: GameInstallationId,
    path: &Path,
    user_declared_endfield: bool,
    expected_revision: u64,
    confirmation: &str,
) -> Result<(GameInstallationRecord, u64), GameImportError> {
    if !user_declared_endfield {
        return Err(GameImportError::DeclarationRequired);
    }
    let initial_snapshot = load_snapshot(data_dir)?;
    let initial_observation = observe_root(path)?;
    let initial_plan = crate::make_import_plan(
        initial_snapshot,
        id.clone(),
        initial_observation,
        user_declared_endfield,
    )?;
    check_import_approval(&initial_plan, expected_revision, confirmation)?;

    if let Some(paths) = app_paths {
        paths
            .prepare_data_dir()
            .map_err(|_| GameImportError::StorageUnavailable)?;
    }
    let data = open_directory_path(data_dir)
        .map_err(storage_error)?
        .ok_or(GameImportError::StorageUnavailable)?;
    verify_private_directory(&data).map_err(storage_error)?;
    verify_registry_filesystem(&data)?;
    let registry_dir = open_child_directory(&data, REGISTRY_DIR, true)
        .map_err(storage_error)?
        .ok_or(GameImportError::StorageUnavailable)?;
    verify_private_directory(&registry_dir).map_err(storage_error)?;
    verify_registry_filesystem(&registry_dir)?;
    sync_directory(&data).map_err(|_| GameImportError::PublicationFailed)?;
    let lock = open_lock_file(&registry_dir)?;
    FileExt::lock_exclusive(&lock).map_err(GameImportError::Io)?;

    let current = read_registry(&registry_dir)?;
    let current_snapshot = snapshot_from_registry(current.as_ref().map(|item| &item.0));
    if current_snapshot.revision != expected_revision {
        return Err(GameImportError::RevisionConflict);
    }
    cleanup_temporary_files(&registry_dir)?;
    let fresh_observation = observe_root(path)?;
    if fresh_observation.root_identity != initial_plan.root_identity
        || fresh_observation.selected_path != initial_plan.selected_path
    {
        return Err(GameImportError::ChangedSincePlan);
    }
    let fresh_plan = crate::make_import_plan(
        current_snapshot,
        id,
        fresh_observation,
        user_declared_endfield,
    )?;
    check_import_approval(&fresh_plan, expected_revision, confirmation)?;

    let record = GameInstallationRecord {
        id: fresh_plan.proposed_id.clone(),
        scope: fresh_plan.scope,
        state: GameInstallationState::Registered,
        selected_path: fresh_plan.selected_path,
        root_identity: fresh_plan.root_identity,
        filesystem: fresh_plan.filesystem,
        evidence: fresh_plan.evidence,
        registered_at_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    validate_record(&record)?;
    let mut registry = current
        .as_ref()
        .map(|item| item.0.clone())
        .unwrap_or_else(empty_registry);
    if registry.installations.len() >= MAX_INSTALLATIONS {
        return Err(GameImportError::InspectionLimit);
    }
    registry.revision = registry
        .revision
        .checked_add(1)
        .ok_or(GameImportError::RevisionExhausted)?;
    registry.installations.push(record.clone());
    publish_registry(&registry_dir, current.as_ref(), &registry)?;
    Ok((record, registry.revision))
}

fn check_import_approval(
    plan: &GameImportPlan,
    expected_revision: u64,
    confirmation: &str,
) -> Result<(), GameImportError> {
    if plan.registry_revision != expected_revision {
        return Err(GameImportError::RevisionConflict);
    }
    if plan.existing_id_collision {
        return Err(GameImportError::DuplicateId);
    }
    if plan.existing_location_collision.is_some() {
        return Err(GameImportError::DuplicateLocation);
    }
    if plan.evidence.root_entries.symlinks > 0 || plan.evidence.root_entries.special_files > 0 {
        return Err(GameImportError::UnsupportedEntries);
    }
    if !plan.filesystem.mount_id_observed || !plan.filesystem.object_identity_observed {
        return Err(GameImportError::FilesystemIdentityUnavailable);
    }
    if plan.filesystem.filesystem_type == "unknown" {
        return Err(GameImportError::UnsupportedFilesystem);
    }
    if !plan.can_register {
        return Err(GameImportError::DeclarationRequired);
    }
    if plan.required_confirmation.as_deref() != Some(confirmation) {
        return Err(GameImportError::ConfirmationMismatch);
    }
    Ok(())
}

fn snapshot_from_registry(registry: Option<&RegistryFile>) -> GameInstallationSnapshot {
    registry
        .map(|registry| GameInstallationSnapshot {
            schema_version: REGISTRY_SCHEMA_VERSION,
            revision: registry.revision,
            installations: registry.installations.clone(),
        })
        .unwrap_or_else(empty_snapshot)
}

fn empty_registry() -> RegistryFile {
    RegistryFile {
        schema_version: REGISTRY_SCHEMA_VERSION,
        revision: 0,
        installations: Vec::new(),
        checksum: String::new(),
    }
}

pub(super) fn validate_installation(
    record: &GameInstallationRecord,
    registry_revision: u64,
) -> Result<GameInstallationValidation, GameImportError> {
    let mut evidence = record.evidence.clone();
    let path = Path::new(&record.selected_path);
    let observed = match observe_root(path) {
        Ok(observed) => observed,
        Err(GameImportError::Missing) => {
            return Ok(GameInstallationValidation {
                schema_version: REGISTRY_SCHEMA_VERSION,
                id: record.id.clone(),
                state: GameInstallationState::Missing,
                registry_revision,
                registered_path: record.selected_path.clone(),
                observed_root_identity: None,
                filesystem: None,
                evidence,
                explanation:
                    "The exact registered path is missing; no alternate location was searched."
                        .into(),
            });
        }
        Err(GameImportError::AccessDenied) => {
            return Ok(validation_blocked(
                record,
                registry_revision,
                GameInstallationState::Blocked,
                evidence,
                "The exact registered path cannot be read with the current permissions.",
            ));
        }
        Err(GameImportError::EmptyLocation) => {
            evidence.root_entries.total = 0;
            evidence.root_entries.directories = 0;
            evidence.root_entries.regular_files = 0;
            evidence.root_entries.symlinks = 0;
            evidence.root_entries.special_files = 0;
            evidence.structural_state = StructuralEvidenceState::Changed;
            return Ok(validation_blocked(
                record,
                registry_revision,
                GameInstallationState::Changed,
                evidence,
                "The exact registered root is now empty. No alternate location was searched.",
            ));
        }
        Err(GameImportError::InspectionLimit | GameImportError::UnsupportedEntries) => {
            evidence.structural_state = StructuralEvidenceState::Unsupported;
            return Ok(validation_blocked(
                record,
                registry_revision,
                GameInstallationState::Unsupported,
                evidence,
                "The exact registered root now exceeds bounded inspection or contains root links/special entries.",
            ));
        }
        Err(GameImportError::ChangedSincePlan) => {
            return Ok(validation_blocked(
                record,
                registry_revision,
                GameInstallationState::Changed,
                evidence,
                "The path changed while its filesystem identity was being inspected.",
            ));
        }
        Err(_) => {
            return Ok(validation_blocked(
                record,
                registry_revision,
                GameInstallationState::Blocked,
                evidence,
                "The exact registered location could not be safely revalidated.",
            ));
        }
    };
    if observed.root_entries != record.evidence.root_entries {
        evidence.structural_state = StructuralEvidenceState::Changed;
    }
    evidence.root_entries = observed.root_entries.clone();
    if !observed.filesystem.mount_id_observed {
        return Ok(validation_blocked(
            record,
            registry_revision,
            GameInstallationState::Unsupported,
            evidence,
            "Current mount identity is unavailable; exact location identity cannot be established.",
        ));
    }
    if observed.root_identity.device != record.root_identity.device
        || observed.root_identity.inode != record.root_identity.inode
    {
        return Ok(GameInstallationValidation {
            schema_version: REGISTRY_SCHEMA_VERSION,
            id: record.id.clone(),
            state: GameInstallationState::Changed,
            registry_revision,
            registered_path: record.selected_path.clone(),
            observed_root_identity: Some(observed.root_identity),
            filesystem: Some(observed.filesystem),
            evidence,
            explanation: "The exact registered path now resolves to a different device, inode, or mount. No replacement location was selected.".into(),
        });
    }
    if observed.root_identity.mount_id != record.root_identity.mount_id {
        return Ok(validation_blocked(
            record,
            registry_revision,
            GameInstallationState::Unsupported,
            evidence,
            "The path now has a different mount identity. Mount IDs can change across remounts, so the same external volume cannot be established safely.",
        ));
    }
    if observed.filesystem.filesystem_type != record.filesystem.filesystem_type {
        return Ok(validation_blocked(
            record,
            registry_revision,
            GameInstallationState::Unsupported,
            evidence,
            "The filesystem type reported for the registered root changed; exact identity is not established.",
        ));
    }
    if observed.root_entries != record.evidence.root_entries {
        return Ok(GameInstallationValidation {
            schema_version: REGISTRY_SCHEMA_VERSION,
            id: record.id.clone(),
            state: GameInstallationState::Changed,
            registry_revision,
            registered_path: record.selected_path.clone(),
            observed_root_identity: Some(observed.root_identity),
            filesystem: Some(observed.filesystem),
            evidence,
            explanation: "The bounded top-level entry counts changed. Nested contents and file bytes were not scanned or hashed.".into(),
        });
    }
    Ok(GameInstallationValidation {
        schema_version: REGISTRY_SCHEMA_VERSION,
        id: record.id.clone(),
        state: GameInstallationState::Registered,
        registry_revision,
        registered_path: record.selected_path.clone(),
        observed_root_identity: Some(observed.root_identity),
        filesystem: Some(observed.filesystem),
        evidence,
        explanation: "The registered root object and bounded top-level entry counts match. Endfield identity, nested structure, file integrity, and launch readiness remain unverified.".into(),
    })
}

fn validation_blocked(
    record: &GameInstallationRecord,
    registry_revision: u64,
    state: GameInstallationState,
    evidence: GameInstallationEvidence,
    explanation: &str,
) -> GameInstallationValidation {
    GameInstallationValidation {
        schema_version: REGISTRY_SCHEMA_VERSION,
        id: record.id.clone(),
        state,
        registry_revision,
        registered_path: record.selected_path.clone(),
        observed_root_identity: None,
        filesystem: None,
        evidence,
        explanation: explanation.into(),
    }
}

pub(super) fn unregister(
    data_dir: &Path,
    app_paths: Option<&AppPaths>,
    id: &GameInstallationId,
    expected_revision: u64,
    confirmation: &str,
) -> Result<u64, GameImportError> {
    let before = load_snapshot(data_dir)?;
    let planned = unregister_plan(&before, id)?;
    check_unregister_approval(&planned, expected_revision, confirmation)?;
    if let Some(paths) = app_paths {
        paths
            .prepare_data_dir()
            .map_err(|_| GameImportError::StorageUnavailable)?;
    }
    let data = open_directory_path(data_dir)
        .map_err(storage_error)?
        .ok_or(GameImportError::StorageUnavailable)?;
    verify_private_directory(&data).map_err(storage_error)?;
    verify_registry_filesystem(&data)?;
    let registry_dir = open_child_directory(&data, REGISTRY_DIR, false)
        .map_err(storage_error)?
        .ok_or(GameImportError::NotRegistered)?;
    verify_private_directory(&registry_dir).map_err(storage_error)?;
    verify_registry_filesystem(&registry_dir)?;
    let lock = open_lock_file(&registry_dir)?;
    FileExt::lock_exclusive(&lock).map_err(GameImportError::Io)?;
    let current = read_registry(&registry_dir)?.ok_or(GameImportError::NotRegistered)?;
    let snapshot = snapshot_from_registry(Some(&current.0));
    if snapshot.revision != expected_revision {
        return Err(GameImportError::RevisionConflict);
    }
    let plan = unregister_plan(&snapshot, id)?;
    check_unregister_approval(&plan, expected_revision, confirmation)?;
    cleanup_temporary_files(&registry_dir)?;
    let mut replacement = current.0.clone();
    replacement.installations.retain(|record| &record.id != id);
    if replacement.installations.len() == current.0.installations.len() {
        return Err(GameImportError::NotRegistered);
    }
    replacement.revision = replacement
        .revision
        .checked_add(1)
        .ok_or(GameImportError::RevisionExhausted)?;
    publish_registry(&registry_dir, Some(&current), &replacement)?;
    Ok(replacement.revision)
}

fn unregister_plan(
    snapshot: &GameInstallationSnapshot,
    id: &GameInstallationId,
) -> Result<GameUnregisterPlan, GameImportError> {
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

fn check_unregister_approval(
    plan: &GameUnregisterPlan,
    expected_revision: u64,
    confirmation: &str,
) -> Result<(), GameImportError> {
    if plan.registry_revision != expected_revision {
        return Err(GameImportError::RevisionConflict);
    }
    if plan.required_confirmation != confirmation {
        return Err(GameImportError::ConfirmationMismatch);
    }
    Ok(())
}

fn open_lock_file(directory: &File) -> Result<File, GameImportError> {
    let name = CString::new("registry.lock").expect("static lock file name");
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err(storage_error(std::io::Error::last_os_error()));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    verify_private_file(&file).map_err(storage_error)?;
    Ok(file)
}

fn read_registry(
    directory: &File,
) -> Result<Option<(RegistryFile, Vec<u8>, FileIdentity)>, GameImportError> {
    let name = CString::new("registry.json").expect("static registry file name");
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOENT) {
            return Ok(None);
        }
        return Err(storage_error(error));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    verify_private_file(&file).map_err(storage_error)?;
    let metadata = file.metadata().map_err(GameImportError::Io)?;
    if metadata.len() > MAX_REGISTRY_BYTES as u64 {
        return Err(GameImportError::RegistryCorrupt);
    }
    let identity = FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take((MAX_REGISTRY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(GameImportError::Io)?;
    if bytes.len() > MAX_REGISTRY_BYTES {
        return Err(GameImportError::RegistryCorrupt);
    }
    let registry = decode_registry(&bytes)?;
    Ok(Some((registry, bytes, identity)))
}

fn decode_registry(bytes: &[u8]) -> Result<RegistryFile, GameImportError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| GameImportError::RegistryCorrupt)?;
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(GameImportError::RegistryCorrupt)?;
    if version > REGISTRY_SCHEMA_VERSION as u64 {
        return Err(GameImportError::UnsupportedSchemaVersion(version));
    }
    if version != REGISTRY_SCHEMA_VERSION as u64 {
        return Err(GameImportError::RegistryCorrupt);
    }
    let registry: RegistryFile =
        serde_json::from_value(value).map_err(|_| GameImportError::RegistryCorrupt)?;
    validate_registry(&registry)?;
    Ok(registry)
}

fn validate_registry(registry: &RegistryFile) -> Result<(), GameImportError> {
    if registry.schema_version != REGISTRY_SCHEMA_VERSION
        || registry.installations.len() > MAX_INSTALLATIONS
        || (registry.revision == 0 && !registry.installations.is_empty())
    {
        return Err(GameImportError::RegistryCorrupt);
    }
    if registry.checksum.len() != 64
        || !registry
            .checksum
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || registry.checksum != registry_checksum(registry)?
    {
        return Err(GameImportError::RegistryCorrupt);
    }
    for (index, record) in registry.installations.iter().enumerate() {
        validate_record(record)?;
        if registry.installations[..index].iter().any(|previous| {
            previous.id == record.id
                || previous.selected_path == record.selected_path
                || previous.root_identity == record.root_identity
        }) {
            return Err(GameImportError::RegistryCorrupt);
        }
    }
    Ok(())
}

fn registry_checksum(registry: &RegistryFile) -> Result<String, GameImportError> {
    use sha2::{Digest, Sha256};
    let payload = RegistryChecksumPayload {
        schema_version: registry.schema_version,
        revision: registry.revision,
        installations: &registry.installations,
    };
    let encoded = serde_json::to_vec(&payload).map_err(|_| GameImportError::RegistryCorrupt)?;
    let digest = Sha256::digest(encoded);
    let mut result = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(result, "{byte:02x}");
    }
    Ok(result)
}

fn validate_record(record: &GameInstallationRecord) -> Result<(), GameImportError> {
    crate::GameInstallationId::new(record.id.as_str().to_owned())
        .map_err(|_| GameImportError::RegistryCorrupt)?;
    if record.state != GameInstallationState::Registered
        || record.scope != GameInstallationScope::User
        || record.root_identity.device == 0
        || record.root_identity.inode == 0
        || record.root_identity.mount_id.is_none()
        || !record.filesystem.mount_id_observed
        || !record.filesystem.object_identity_observed
        || !crate::filesystem_type_supported(&record.filesystem.filesystem_type)
        || record.filesystem.unix_ownership_semantics_supported
            != crate::filesystem_has_unix_ownership_semantics(&record.filesystem.filesystem_type)
        || record.filesystem.identity_guarantee != IDENTITY_GUARANTEE
        || !record.evidence.user_declared_endfield
        || record.evidence.identity_confidence != GameIdentityConfidence::UserDeclaredUnverified
        || record.evidence.structural_state != StructuralEvidenceState::NoAuthoritativeFormatKnown
        || record.evidence.publisher_authenticity_verified
        || record.evidence.game_build.is_some()
        || !record.evidence.executable_candidates.is_empty()
        || record.evidence.root_entries.symlinks != 0
        || record.evidence.root_entries.special_files != 0
        || record.evidence.limitations.is_empty()
        || record.evidence.limitations.len() > 16
        || record
            .evidence
            .limitations
            .iter()
            .any(|text| text.len() > 512)
    {
        return Err(GameImportError::RegistryCorrupt);
    }
    let normalized = normalize_selected_path(Path::new(&record.selected_path))
        .map_err(|_| GameImportError::RegistryCorrupt)?;
    if normalized != record.selected_path
        || record.selected_path.len() > MAX_PATH_BYTES
        || !validate_entry_summary(&record.evidence.root_entries)
    {
        return Err(GameImportError::RegistryCorrupt);
    }
    Ok(())
}

fn validate_entry_summary(summary: &RootEntrySummary) -> bool {
    summary.total > 0
        && summary.total <= MAX_ROOT_ENTRIES
        && summary
            .directories
            .saturating_add(summary.regular_files)
            .saturating_add(summary.symlinks)
            .saturating_add(summary.special_files)
            == summary.total
}

fn publish_registry(
    directory: &File,
    previous: Option<&(RegistryFile, Vec<u8>, FileIdentity)>,
    replacement: &RegistryFile,
) -> Result<(), GameImportError> {
    publish_registry_with_rename(directory, previous, replacement, rename_registry_entry)
}

fn publish_registry_with_rename<F>(
    directory: &File,
    previous: Option<&(RegistryFile, Vec<u8>, FileIdentity)>,
    replacement: &RegistryFile,
    mut rename: F,
) -> Result<(), GameImportError>
where
    F: FnMut(&File, &CStr, &CStr, u32) -> Result<(), std::io::Error>,
{
    let mut sealed = replacement.clone();
    sealed.checksum = registry_checksum(&sealed)?;
    validate_registry(&sealed)?;
    let bytes = serde_json::to_vec(&sealed).map_err(|_| GameImportError::RegistryCorrupt)?;
    if bytes.len() > MAX_REGISTRY_BYTES {
        return Err(GameImportError::RegistryCorrupt);
    }
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary_name = format!("registry.tmp-{}-{sequence}", std::process::id());
    let temporary_c =
        CString::new(temporary_name.as_str()).map_err(|_| GameImportError::PublicationFailed)?;
    let temporary_fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            temporary_c.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if temporary_fd < 0 {
        return Err(GameImportError::PublicationFailed);
    }
    let mut temporary = unsafe { File::from_raw_fd(temporary_fd) };
    if temporary
        .write_all(&bytes)
        .and_then(|()| temporary.sync_all())
        .is_err()
    {
        drop(temporary);
        let _ = unlink_at(directory, &temporary_name);
        return Err(GameImportError::PublicationFailed);
    }
    drop(temporary);

    let registry_c = CString::new("registry.json").expect("static registry file name");
    let flags = if previous.is_some() {
        libc::RENAME_EXCHANGE
    } else {
        libc::RENAME_NOREPLACE
    };
    if let Err(error) = rename(directory, &temporary_c, &registry_c, flags) {
        let _ = unlink_at(directory, &temporary_name);
        if matches!(
            error.raw_os_error(),
            Some(libc::EEXIST) | Some(libc::ENOENT)
        ) {
            return Err(GameImportError::RevisionConflict);
        }
        return Err(GameImportError::PublicationFailed);
    }

    if let Some(previous) = previous {
        let previous_match = open_regular_file_at(directory, &temporary_name).and_then(|file| {
            let metadata = file.metadata().map_err(GameImportError::Io)?;
            let old_identity = FileIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
            };
            let old_bytes = read_file_bounded(file)?;
            Ok(old_identity == previous.2 && old_bytes == previous.1)
        });
        if !matches!(&previous_match, Ok(true)) {
            if rename(directory, &temporary_c, &registry_c, libc::RENAME_EXCHANGE).is_err() {
                return Err(GameImportError::CommitUncertain);
            }
            unlink_at(directory, &temporary_name).map_err(|_| GameImportError::RecoveryRequired)?;
            sync_directory(directory).map_err(|_| GameImportError::CommitUncertain)?;
            return if matches!(&previous_match, Ok(false)) {
                Err(GameImportError::RevisionConflict)
            } else {
                Err(GameImportError::PublicationFailed)
            };
        }
        unlink_at(directory, &temporary_name).map_err(|_| GameImportError::CommitUncertain)?;
    }
    sync_directory(directory).map_err(|_| GameImportError::CommitUncertain)
}

fn rename_registry_entry(
    directory: &File,
    source: &CStr,
    destination: &CStr,
    flags: u32,
) -> Result<(), std::io::Error> {
    if unsafe {
        libc::renameat2(
            directory.as_raw_fd(),
            source.as_ptr(),
            directory.as_raw_fd(),
            destination.as_ptr(),
            flags,
        )
    } == 0
    {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn open_regular_file_at(directory: &File, name: &str) -> Result<File, GameImportError> {
    let name = CString::new(name).map_err(|_| GameImportError::StorageUnavailable)?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0,
        )
    };
    if fd < 0 {
        return Err(GameImportError::Io(std::io::Error::last_os_error()));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    verify_private_file(&file).map_err(storage_error)?;
    Ok(file)
}

fn read_file_bounded(mut file: File) -> Result<Vec<u8>, GameImportError> {
    let metadata = file.metadata().map_err(GameImportError::Io)?;
    if metadata.len() > MAX_REGISTRY_BYTES as u64 {
        return Err(GameImportError::RegistryCorrupt);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take((MAX_REGISTRY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(GameImportError::Io)?;
    if bytes.len() > MAX_REGISTRY_BYTES {
        return Err(GameImportError::RegistryCorrupt);
    }
    Ok(bytes)
}

fn cleanup_temporary_files(directory: &File) -> Result<(), GameImportError> {
    let mut names = Vec::new();
    scan_directory(directory.as_raw_fd(), 4096, |_, name| {
        if name.to_bytes().starts_with(b"registry.tmp-") {
            if names.len() >= MAX_STALE_TEMP_FILES {
                return Err(GameImportError::RecoveryRequired);
            }
            names.push(
                name.to_str()
                    .map_err(|_| GameImportError::RecoveryRequired)?
                    .to_owned(),
            );
        }
        Ok(())
    })?;
    let had_names = !names.is_empty();
    for name in names {
        let file = open_regular_file_at(directory, &name)
            .map_err(|_| GameImportError::RecoveryRequired)?;
        let _ = read_file_bounded(file).map_err(|_| GameImportError::RecoveryRequired)?;
        unlink_at(directory, &name).map_err(|_| GameImportError::RecoveryRequired)?;
    }
    if had_names {
        sync_directory(directory).map_err(|_| GameImportError::RecoveryRequired)?;
    }
    Ok(())
}

fn unlink_at(directory: &File, name: &str) -> Result<(), std::io::Error> {
    let name = CString::new(name)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid file name"))?;
    if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn open_directory_path(path: &Path) -> Result<Option<File>, std::io::Error> {
    if !path.is_absolute() || path.as_os_str().as_bytes().len() > MAX_PATH_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid application data path",
        ));
    }
    let mut directory = File::open("/")?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                let name = CString::new(name.as_bytes()).map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path component")
                })?;
                let fd = unsafe {
                    libc::openat(
                        directory.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY
                            | libc::O_DIRECTORY
                            | libc::O_NOFOLLOW
                            | libc::O_CLOEXEC
                            | libc::O_NONBLOCK,
                        0,
                    )
                };
                if fd < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() == Some(libc::ENOENT) {
                        return Ok(None);
                    }
                    return Err(error);
                }
                directory = unsafe { File::from_raw_fd(fd) };
            }
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "unsafe application data path",
                ));
            }
        }
    }
    Ok(Some(directory))
}

fn open_child_directory(
    parent: &File,
    name: &str,
    create: bool,
) -> Result<Option<File>, std::io::Error> {
    let name_c = CString::new(name)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid child name"))?;
    let mut fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name_c.as_ptr(),
            libc::O_RDONLY
                | libc::O_DIRECTORY
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | libc::O_NONBLOCK,
            0,
        )
    };
    if fd < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) && create {
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name_c.as_ptr(), 0o700) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::EEXIST) {
                return Err(error);
            }
        } else {
            parent.sync_all()?;
        }
        fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name_c.as_ptr(),
                libc::O_RDONLY
                    | libc::O_DIRECTORY
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
                0,
            )
        };
    }
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOENT) {
            return Ok(None);
        }
        return Err(error);
    }
    Ok(Some(unsafe { File::from_raw_fd(fd) }))
}

fn verify_private_directory(file: &File) -> Result<(), std::io::Error> {
    let metadata = file.metadata()?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "application metadata directory is not private",
        ));
    }
    Ok(())
}

fn verify_private_file(file: &File) -> Result<(), std::io::Error> {
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "application metadata file is unsafe",
        ));
    }
    Ok(())
}

fn sync_directory(directory: &File) -> Result<(), std::io::Error> {
    directory.sync_all()
}

fn verify_registry_filesystem(directory: &File) -> Result<(), GameImportError> {
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatfs(directory.as_raw_fd(), &mut stat) } != 0 {
        return Err(GameImportError::StorageUnavailable);
    }
    let magic = stat.f_type as u64;
    if matches!(
        magic,
        0x0000_ef53 | 0x5846_5342 | 0x9123_683e | 0xf2f5_2010 | 0x0102_1994 | 0x794c_7630
    ) {
        Ok(())
    } else {
        Err(GameImportError::StorageUnavailable)
    }
}

fn storage_error(error: std::io::Error) -> GameImportError {
    match error.raw_os_error() {
        Some(libc::EACCES) | Some(libc::EPERM) => GameImportError::StorageUnavailable,
        Some(libc::ELOOP) | Some(libc::ENOTDIR) | Some(libc::EINVAL) => {
            GameImportError::StorageUnavailable
        }
        _ if error.kind() == std::io::ErrorKind::PermissionDenied => {
            GameImportError::StorageUnavailable
        }
        _ => GameImportError::Io(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GameInstallationRegistry;
    use std::{
        ffi::OsString,
        fs,
        os::unix::{
            ffi::OsStringExt,
            fs::{MetadataExt, PermissionsExt, symlink},
        },
        sync::{
            Arc, Barrier,
            atomic::{AtomicBool, Ordering},
        },
        thread,
    };
    use tempfile::tempdir;

    fn setup() -> (tempfile::TempDir, GameInstallationRegistry, PathBuf) {
        let temp = tempdir().unwrap();
        let app = temp.path().join("app-data/endfield-unix");
        fs::create_dir_all(&app).unwrap();
        fs::set_permissions(&app, fs::Permissions::from_mode(0o700)).unwrap();
        let game = temp.path().join("Games/Endfield Δ");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("opaque.dat"), b"preserve this inert fixture").unwrap();
        let registry = GameInstallationRegistry::at_data_dir(app).unwrap();
        (temp, registry, game)
    }

    fn plan(registry: &GameInstallationRegistry, game: &Path) -> GameImportPlan {
        registry
            .plan_import(
                GameInstallationId::new("endfield-main").unwrap(),
                game,
                true,
            )
            .unwrap()
    }

    fn apply_plan(registry: &GameInstallationRegistry, plan: &GameImportPlan) {
        registry
            .apply_import(
                plan.proposed_id.clone(),
                &plan.selected_path,
                true,
                plan.registry_revision,
                plan.required_confirmation.as_deref().unwrap(),
            )
            .unwrap();
    }

    #[test]
    fn empty_registry_and_read_only_plan_have_no_storage_side_effects() {
        let temp = tempdir().unwrap();
        let app = temp.path().join("not-created/endfield-unix");
        let registry = GameInstallationRegistry::at_data_dir(&app).unwrap();
        let game = temp.path().join("selected game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("payload"), b"untouched").unwrap();
        assert_eq!(registry.load_read_only().unwrap().installations.len(), 0);
        let plan = plan(&registry, &game);
        assert!(plan.can_register);
        assert_eq!(plan.registry_revision, 0);
        assert!(!app.exists());
        assert_eq!(fs::read(game.join("payload")).unwrap(), b"untouched");
    }

    #[test]
    fn user_declaration_is_explicit_and_does_not_verify_identity() {
        let (_temp, registry, game) = setup();
        let plan = registry
            .plan_import(
                GameInstallationId::new("endfield-main").unwrap(),
                &game,
                false,
            )
            .unwrap();
        assert!(!plan.can_register);
        assert!(plan.required_confirmation.is_none());
        assert_eq!(
            plan.evidence.identity_confidence,
            GameIdentityConfidence::NotEstablished
        );
        assert!(!plan.evidence.publisher_authenticity_verified);
        assert!(plan.evidence.game_build.is_none());
        assert!(plan.evidence.executable_candidates.is_empty());
    }

    #[test]
    fn empty_missing_file_relative_and_symlink_paths_return_typed_errors() {
        let (_temp, registry, game) = setup();
        let empty = game.join("empty");
        fs::create_dir(&empty).unwrap();
        assert!(matches!(
            registry.plan_import(GameInstallationId::new("empty").unwrap(), &empty, true),
            Err(GameImportError::EmptyLocation)
        ));
        assert!(matches!(
            registry.plan_import(
                GameInstallationId::new("missing").unwrap(),
                game.join("missing"),
                true
            ),
            Err(GameImportError::Missing)
        ));
        let file = game.join("opaque.dat");
        assert!(matches!(
            registry.plan_import(GameInstallationId::new("file").unwrap(), &file, true),
            Err(GameImportError::NotDirectory)
        ));
        assert!(matches!(
            registry.plan_import(
                GameInstallationId::new("relative").unwrap(),
                "relative",
                true
            ),
            Err(GameImportError::InvalidPath)
        ));
        let link = game.join("linked");
        symlink(&game, &link).unwrap();
        assert!(matches!(
            registry.plan_import(GameInstallationId::new("link").unwrap(), &link, true),
            Err(GameImportError::InvalidPath)
        ));
    }

    #[test]
    fn root_symlink_and_root_special_entry_are_blocked_without_following() {
        let (_temp, registry, game) = setup();
        let target = game.join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("payload"), b"untouched").unwrap();
        let link = game.join("root-link");
        symlink(&target, &link).unwrap();
        assert!(matches!(
            registry.plan_import(GameInstallationId::new("link").unwrap(), &link, true),
            Err(GameImportError::InvalidPath)
        ));

        let fifo = game.join("pipe");
        let fifo_c = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
        let plan = plan(&registry, &game);
        assert!(!plan.can_register);
        assert_eq!(plan.evidence.root_entries.special_files, 1);
        assert!(matches!(
            registry.apply_import(
                plan.proposed_id.clone(),
                &plan.selected_path,
                true,
                plan.registry_revision,
                "wrong"
            ),
            Err(GameImportError::UnsupportedEntries)
        ));
        assert_eq!(fs::read(target.join("payload")).unwrap(), b"untouched");
    }

    #[test]
    fn root_internal_and_external_links_block_registration_without_traversal() {
        let (_temp, registry, game) = setup();
        let inside = game.join("inside-target");
        fs::create_dir(&inside).unwrap();
        fs::write(inside.join("payload"), b"inside bytes").unwrap();
        let outside = game.parent().unwrap().join("outside-target");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("payload"), b"outside bytes").unwrap();
        symlink(&inside, game.join("internal-link")).unwrap();
        symlink(&outside, game.join("external-link")).unwrap();

        let plan = plan(&registry, &game);
        assert!(!plan.can_register);
        assert_eq!(plan.evidence.root_entries.symlinks, 2);
        assert!(matches!(
            registry.apply_import(
                plan.proposed_id.clone(),
                &plan.selected_path,
                true,
                plan.registry_revision,
                "incorrect"
            ),
            Err(GameImportError::UnsupportedEntries)
        ));
        assert_eq!(fs::read(inside.join("payload")).unwrap(), b"inside bytes");
        assert_eq!(fs::read(outside.join("payload")).unwrap(), b"outside bytes");
    }

    #[test]
    fn nested_links_and_deep_subtrees_are_not_traversed() {
        let (_temp, registry, game) = setup();
        let outside = game.parent().unwrap().join("nested-link-target");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("payload"), b"outside bytes").unwrap();
        let mut nested = game.join("deep");
        fs::create_dir(&nested).unwrap();
        for index in 0..128 {
            nested.push(format!("d{index}"));
            fs::create_dir(&nested).unwrap();
        }
        symlink(&outside, nested.join("external-link")).unwrap();

        let plan = plan(&registry, &game);
        assert!(plan.can_register);
        assert_eq!(plan.evidence.root_entries.total, 2);
        assert_eq!(plan.evidence.root_entries.directories, 1);
        assert_eq!(plan.evidence.root_entries.symlinks, 0);
        apply_plan(&registry, &plan);
        assert_eq!(
            registry.validate(&plan.proposed_id).unwrap().state,
            GameInstallationState::Registered
        );
        assert_eq!(fs::read(outside.join("payload")).unwrap(), b"outside bytes");
    }

    #[test]
    fn registration_revalidates_confirmation_and_keeps_external_bytes_unchanged() {
        let (_temp, registry, game) = setup();
        let payload = game.join("opaque.dat");
        let before = fs::read(&payload).unwrap();
        let metadata_before = fs::metadata(&payload).unwrap();
        let plan = plan(&registry, &game);
        assert!(matches!(
            registry.apply_import(
                plan.proposed_id.clone(),
                &plan.selected_path,
                true,
                plan.registry_revision,
                "wrong-token"
            ),
            Err(GameImportError::ConfirmationMismatch)
        ));
        let (record, revision) = registry
            .apply_import(
                plan.proposed_id.clone(),
                &plan.selected_path,
                true,
                plan.registry_revision,
                plan.required_confirmation.as_deref().unwrap(),
            )
            .unwrap();
        assert_eq!(revision, 1);
        assert_eq!(record.state, GameInstallationState::Registered);
        assert_eq!(
            record.evidence.identity_confidence,
            GameIdentityConfidence::UserDeclaredUnverified
        );
        assert_eq!(fs::read(payload).unwrap(), before);
        let metadata_after = fs::metadata(game.join("opaque.dat")).unwrap();
        assert_eq!(metadata_after.dev(), metadata_before.dev());
        assert_eq!(metadata_after.ino(), metadata_before.ino());
        assert_eq!(metadata_after.mode(), metadata_before.mode());
        assert_eq!(metadata_after.uid(), metadata_before.uid());
        assert_eq!(metadata_after.gid(), metadata_before.gid());
        assert_eq!(metadata_after.mtime(), metadata_before.mtime());
        assert_eq!(metadata_after.ctime(), metadata_before.ctime());
        assert!(!game.join("registry.json").exists());
    }

    #[test]
    fn duplicate_ids_and_locations_are_rejected() {
        let (_temp, registry, game) = setup();
        let first = plan(&registry, &game);
        apply_plan(&registry, &first);
        assert!(matches!(
            registry.plan_import(first.proposed_id.clone(), &game, true),
            Ok(GameImportPlan {
                can_register: false,
                existing_id_collision: true,
                ..
            })
        ));
        let new_game = game.parent().unwrap().join("other");
        fs::create_dir(&new_game).unwrap();
        fs::write(new_game.join("file"), b"data").unwrap();
        let duplicate_id = registry
            .plan_import(first.proposed_id.clone(), &new_game, true)
            .unwrap();
        assert!(duplicate_id.existing_id_collision);
        assert!(!duplicate_id.can_register);
        let duplicate_location = registry
            .plan_import(
                GameInstallationId::new("different-id").unwrap(),
                &game,
                true,
            )
            .unwrap();
        assert!(duplicate_location.existing_location_collision.is_some());
        assert!(!duplicate_location.can_register);
    }

    #[test]
    fn stale_revision_and_registration_change_between_plan_and_apply_fail_closed() {
        let (_temp, registry, game) = setup();
        let plan = plan(&registry, &game);
        let other = game.parent().unwrap().join("second");
        fs::create_dir(&other).unwrap();
        fs::write(other.join("file"), b"other").unwrap();
        let second = registry
            .plan_import(GameInstallationId::new("second").unwrap(), &other, true)
            .unwrap();
        apply_plan(&registry, &second);
        assert!(matches!(
            registry.apply_import(
                plan.proposed_id.clone(),
                &plan.selected_path,
                true,
                plan.registry_revision,
                plan.required_confirmation.as_deref().unwrap()
            ),
            Err(GameImportError::RevisionConflict)
        ));
    }

    #[test]
    fn concurrent_imports_with_one_revision_allow_only_one_writer() {
        let (_temp, registry, game) = setup();
        let second = game.parent().unwrap().join("second");
        fs::create_dir(&second).unwrap();
        fs::write(second.join("file"), b"other").unwrap();
        let first_plan = plan(&registry, &game);
        let second_plan = registry
            .plan_import(GameInstallationId::new("second").unwrap(), &second, true)
            .unwrap();
        let registry = Arc::new(registry);
        let barrier = Arc::new(Barrier::new(3));
        let handles = [first_plan, second_plan].map(|plan| {
            let registry = Arc::clone(&registry);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                registry.apply_import(
                    plan.proposed_id,
                    plan.selected_path,
                    true,
                    plan.registry_revision,
                    plan.required_confirmation.as_deref().unwrap(),
                )
            })
        });
        barrier.wait();
        let results = handles.map(|handle| handle.join().unwrap());
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(GameImportError::RevisionConflict)))
                .count(),
            1
        );
    }

    #[test]
    fn changed_root_object_is_reported_without_relocation_search() {
        let (_temp, registry, game) = setup();
        let plan = plan(&registry, &game);
        apply_plan(&registry, &plan);
        let moved = game.with_file_name("moved");
        fs::rename(&game, &moved).unwrap();
        fs::create_dir(&game).unwrap();
        fs::write(game.join("opaque.dat"), b"replacement").unwrap();
        let result = registry.validate(&plan.proposed_id).unwrap();
        assert_eq!(result.state, GameInstallationState::Changed);
        assert_eq!(result.registered_path, plan.selected_path);
        assert_eq!(
            fs::read(moved.join("opaque.dat")).unwrap(),
            b"preserve this inert fixture"
        );
    }

    #[test]
    fn different_current_mount_identity_is_unsupported_not_a_relocation() {
        let (_temp, registry, game) = setup();
        let plan = plan(&registry, &game);
        apply_plan(&registry, &plan);
        let snapshot = registry.load_read_only().unwrap();
        let mut record = snapshot.installations[0].clone();
        record.root_identity.mount_id = record.root_identity.mount_id.map(|id| id + 1);
        let result = validate_installation(&record, snapshot.revision).unwrap();
        assert_eq!(result.state, GameInstallationState::Unsupported);
        assert!(result.explanation.contains("different mount identity"));
        assert_eq!(result.registered_path, plan.selected_path);
    }

    #[test]
    fn missing_registered_location_is_reported_and_never_replaced() {
        let (_temp, registry, game) = setup();
        let plan = plan(&registry, &game);
        apply_plan(&registry, &plan);
        fs::remove_dir_all(&game).unwrap();
        let result = registry.validate(&plan.proposed_id).unwrap();
        assert_eq!(result.state, GameInstallationState::Missing);
    }

    #[test]
    fn changed_top_level_entry_types_are_detected_during_revalidation() {
        let (_temp, registry, game) = setup();
        let plan = plan(&registry, &game);
        apply_plan(&registry, &plan);
        fs::create_dir(game.join("new-directory")).unwrap();
        let validation = registry.validate(&plan.proposed_id).unwrap();
        assert_eq!(validation.state, GameInstallationState::Changed);
        assert_eq!(validation.evidence.root_entries.total, 2);
        assert_eq!(validation.evidence.root_entries.directories, 1);
        assert!(
            validation
                .explanation
                .contains("top-level entry counts changed")
        );
    }

    #[test]
    fn large_sparse_files_are_not_read_or_hashed_for_registration() {
        let (_temp, registry, game) = setup();
        let large = game.join("large-sparse-payload");
        let file = fs::File::create(&large).unwrap();
        file.set_len(8 * 1024 * 1024 * 1024).unwrap();
        let plan = plan(&registry, &game);
        assert!(plan.can_register);
        assert_eq!(plan.evidence.root_entries.regular_files, 2);
        apply_plan(&registry, &plan);
        assert_eq!(fs::metadata(large).unwrap().len(), 8 * 1024 * 1024 * 1024);
    }

    #[test]
    fn unregister_plan_is_read_only_and_removes_only_registry_metadata() {
        let (_temp, registry, game) = setup();
        let plan = plan(&registry, &game);
        apply_plan(&registry, &plan);
        let payload = game.join("opaque.dat");
        let before = fs::read(&payload).unwrap();
        let removal = registry.plan_unregister(&plan.proposed_id).unwrap();
        assert_eq!(removal.registry_revision, 1);
        assert_eq!(fs::read(&payload).unwrap(), before);
        let revision = registry
            .unregister(
                &removal.id,
                removal.registry_revision,
                &removal.required_confirmation,
            )
            .unwrap();
        assert_eq!(revision, 2);
        assert!(registry.load_read_only().unwrap().installations.is_empty());
        assert_eq!(fs::read(payload).unwrap(), before);
    }

    #[test]
    fn stale_unregister_confirmation_cannot_remove_a_newer_record() {
        let (_temp, registry, game) = setup();
        let plan = plan(&registry, &game);
        apply_plan(&registry, &plan);
        let removal = registry.plan_unregister(&plan.proposed_id).unwrap();
        let other = game.parent().unwrap().join("other");
        fs::create_dir(&other).unwrap();
        fs::write(other.join("file"), b"other").unwrap();
        let second = registry
            .plan_import(GameInstallationId::new("second").unwrap(), &other, true)
            .unwrap();
        apply_plan(&registry, &second);
        assert!(matches!(
            registry.unregister(
                &removal.id,
                removal.registry_revision,
                &removal.required_confirmation
            ),
            Err(GameImportError::RevisionConflict)
        ));
        assert_eq!(registry.load_read_only().unwrap().installations.len(), 2);
    }

    #[test]
    fn concurrent_unregisters_at_one_revision_allow_only_one_writer() {
        let (_temp, registry, game) = setup();
        let plan = plan(&registry, &game);
        apply_plan(&registry, &plan);
        let other = game.parent().unwrap().join("second");
        fs::create_dir(&other).unwrap();
        fs::write(other.join("file"), b"other").unwrap();
        let other_plan = registry
            .plan_import(GameInstallationId::new("second").unwrap(), &other, true)
            .unwrap();
        apply_plan(&registry, &other_plan);
        let remove_first = registry.plan_unregister(&plan.proposed_id).unwrap();
        let remove_second = registry.plan_unregister(&other_plan.proposed_id).unwrap();
        let registry = Arc::new(registry);
        let barrier = Arc::new(Barrier::new(3));
        let plans = [remove_first, remove_second];
        let handles = plans.map(|remove| {
            let registry = Arc::clone(&registry);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                registry.unregister(
                    &remove.id,
                    remove.registry_revision,
                    &remove.required_confirmation,
                )
            })
        });
        barrier.wait();
        let results = handles.map(|handle| handle.join().unwrap());
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(GameImportError::RevisionConflict)))
                .count(),
            1
        );
        assert_eq!(registry.load_read_only().unwrap().installations.len(), 1);
        assert_eq!(
            fs::read(game.join("opaque.dat")).unwrap(),
            b"preserve this inert fixture"
        );
    }

    #[test]
    fn corrupt_and_future_registries_fail_without_reset() {
        let (_temp, registry, _game) = setup();
        let data = registry.data_dir.clone();
        let root = data.join(REGISTRY_DIR);
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let file = root.join("registry.json");
        fs::write(&file, b"not-json").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            registry.load_read_only(),
            Err(GameImportError::RegistryCorrupt)
        ));
        fs::write(&file, br#"{"schema_version":2}"#).unwrap();
        assert!(matches!(
            registry.load_read_only(),
            Err(GameImportError::UnsupportedSchemaVersion(2))
        ));
        assert_eq!(fs::read(file).unwrap(), br#"{"schema_version":2}"#);
    }

    #[test]
    fn registry_checksum_detects_valid_looking_edits_without_reset() {
        let (_temp, registry, game) = setup();
        let plan = plan(&registry, &game);
        apply_plan(&registry, &plan);
        let path = registry.data_dir.join(REGISTRY_DIR).join("registry.json");
        let original = fs::read(&path).unwrap();
        let mut edited: serde_json::Value = serde_json::from_slice(&original).unwrap();
        edited["installations"][0]["selected_path"] =
            serde_json::json!(game.with_file_name("different-location").to_string_lossy());
        fs::write(&path, serde_json::to_vec(&edited).unwrap()).unwrap();
        let edited_bytes = fs::read(&path).unwrap();
        assert!(matches!(
            registry.load_read_only(),
            Err(GameImportError::RegistryCorrupt)
        ));
        assert_eq!(fs::read(path).unwrap(), edited_bytes);
    }

    #[test]
    fn atomic_publication_failure_preserves_previous_registry_bytes() {
        let (_temp, registry, game) = setup();
        let plan = plan(&registry, &game);
        apply_plan(&registry, &plan);
        let directory = open_directory_path(&registry.data_dir.join(REGISTRY_DIR))
            .unwrap()
            .unwrap();
        let previous = read_registry(&directory).unwrap().unwrap();
        let mut replacement = previous.0.clone();
        replacement.revision += 1;
        let result = publish_registry_with_rename(
            &directory,
            Some(&previous),
            &replacement,
            |_directory, _source, _destination, _flags| {
                Err(std::io::Error::other("injected rename failure"))
            },
        );
        assert!(matches!(result, Err(GameImportError::PublicationFailed)));
        let after = read_registry(&directory).unwrap().unwrap();
        assert_eq!(after.0.revision, previous.0.revision);
        assert_eq!(after.1, previous.1);
        let mut temporary_names = Vec::new();
        scan_directory(directory.as_raw_fd(), 4096, |_, name| {
            if name.to_bytes().starts_with(b"registry.tmp-") {
                temporary_names.push(name.to_bytes().to_vec());
            }
            Ok(())
        })
        .unwrap();
        assert!(temporary_names.is_empty());
        assert_eq!(
            fs::read(game.join("opaque.dat")).unwrap(),
            b"preserve this inert fixture"
        );
    }

    #[test]
    fn metadata_corruption_and_unsafe_registry_modes_fail_closed() {
        let (_temp, registry, game) = setup();
        let plan = plan(&registry, &game);
        apply_plan(&registry, &plan);
        let file = registry.data_dir.join(REGISTRY_DIR).join("registry.json");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            registry.load_read_only(),
            Err(GameImportError::StorageUnavailable)
        ));
    }

    #[test]
    fn path_component_limits_and_unicode_are_handled_without_recursive_scan() {
        let (_temp, registry, game) = setup();
        assert!(
            registry
                .plan_import(GameInstallationId::new("unicode").unwrap(), &game, true)
                .is_ok()
        );
        let too_long = PathBuf::from("/").join("x".repeat(MAX_COMPONENT_BYTES + 1));
        assert!(matches!(
            registry.plan_import(GameInstallationId::new("long").unwrap(), too_long, true),
            Err(GameImportError::InvalidPath)
        ));
        let mut deep = PathBuf::from("/");
        for index in 0..(MAX_PATH_COMPONENTS + 1) {
            deep.push(format!("d{index}"));
        }
        assert!(matches!(
            registry.plan_import(GameInstallationId::new("deep").unwrap(), deep, true),
            Err(GameImportError::InvalidPath)
        ));
        let non_utf8 = PathBuf::from(OsString::from_vec(b"/workspace/game-\xff".to_vec()));
        assert!(matches!(
            registry.plan_import(GameInstallationId::new("non-utf8").unwrap(), non_utf8, true),
            Err(GameImportError::InvalidPath)
        ));
    }

    #[test]
    fn unreadable_roots_and_large_flat_roots_fail_with_bounded_results() {
        let (_temp, registry, game) = setup();
        fs::set_permissions(&game, fs::Permissions::from_mode(0o000)).unwrap();
        assert!(matches!(
            registry.plan_import(GameInstallationId::new("denied").unwrap(), &game, true),
            Err(GameImportError::AccessDenied)
        ));
        fs::set_permissions(&game, fs::Permissions::from_mode(0o700)).unwrap();
        for index in 0..=MAX_ROOT_ENTRIES {
            fs::write(game.join(format!("entry-{index}")), b"x").unwrap();
        }
        assert!(matches!(
            registry.plan_import(GameInstallationId::new("large").unwrap(), &game, true),
            Err(GameImportError::InspectionLimit)
        ));
    }

    #[test]
    fn directory_replacement_between_plan_and_apply_invalidates_confirmation() {
        let (_temp, registry, game) = setup();
        let plan = plan(&registry, &game);
        let moved = game.with_file_name("selected-old");
        fs::rename(&game, &moved).unwrap();
        fs::create_dir(&game).unwrap();
        fs::write(game.join("opaque.dat"), b"replacement bytes").unwrap();
        assert!(matches!(
            registry.apply_import(
                plan.proposed_id.clone(),
                &plan.selected_path,
                true,
                plan.registry_revision,
                plan.required_confirmation.as_deref().unwrap()
            ),
            Err(GameImportError::ConfirmationMismatch)
        ));
        assert!(registry.load_read_only().unwrap().installations.is_empty());
        assert_eq!(
            fs::read(moved.join("opaque.dat")).unwrap(),
            b"preserve this inert fixture"
        );
        assert_eq!(
            fs::read(game.join("opaque.dat")).unwrap(),
            b"replacement bytes"
        );
    }

    #[test]
    fn moving_selected_directory_to_another_path_invalidates_confirmation() {
        let (_temp, registry, game) = setup();
        let plan = plan(&registry, &game);
        let moved = game.with_file_name("renamed-game-directory");
        fs::rename(&game, &moved).unwrap();
        assert_eq!(
            fs::metadata(&moved).unwrap().ino(),
            plan.root_identity.inode,
            "the test retains the same filesystem object"
        );
        assert!(matches!(
            registry.apply_import(
                plan.proposed_id.clone(),
                &moved,
                true,
                plan.registry_revision,
                plan.required_confirmation.as_deref().unwrap()
            ),
            Err(GameImportError::ConfirmationMismatch)
        ));
        assert!(registry.load_read_only().unwrap().installations.is_empty());
        assert_eq!(
            fs::read(moved.join("opaque.dat")).unwrap(),
            b"preserve this inert fixture"
        );
    }

    #[test]
    fn symlink_swaps_are_never_followed_as_the_selected_root() {
        let (_temp, registry, game) = setup();
        let target = game.with_file_name("external-target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("outside"), b"outside").unwrap();
        let expected = fs::metadata(&game).unwrap();
        let expected_device = expected.dev();
        let expected_inode = expected.ino();
        let selected = game.clone();
        let held = game.with_file_name("selected-held");
        let swap_target = target.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let swap_stop = Arc::clone(&stop);
        let swapper = thread::spawn(move || {
            while !swap_stop.load(Ordering::Relaxed) {
                if fs::rename(&selected, &held).is_ok() {
                    if symlink(&swap_target, &selected).is_ok() {
                        let _ = fs::remove_file(&selected);
                    }
                    let _ = fs::rename(&held, &selected);
                }
            }
        });
        for _ in 0..200 {
            match registry.plan_import(GameInstallationId::new("race").unwrap(), &game, true) {
                Ok(plan) => {
                    assert_eq!(plan.root_identity.device, expected_device);
                    assert_eq!(plan.root_identity.inode, expected_inode);
                }
                Err(
                    GameImportError::Missing
                    | GameImportError::NotDirectory
                    | GameImportError::InvalidPath
                    | GameImportError::ChangedSincePlan
                    | GameImportError::RevalidationBlocked,
                ) => {}
                Err(error) => panic!("unexpected race result: {error:?}"),
            }
        }
        stop.store(true, Ordering::Relaxed);
        swapper.join().unwrap();
        assert_eq!(fs::read(target.join("outside")).unwrap(), b"outside");
    }

    #[test]
    fn temporary_registry_files_are_reconciled_only_under_exclusive_mutation() {
        let (_temp, registry, game) = setup();
        let plan = plan(&registry, &game);
        let directory = registry.data_dir.join(REGISTRY_DIR);
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(directory.join("registry.tmp-crashed"), b"partial").unwrap();
        fs::set_permissions(
            directory.join("registry.tmp-crashed"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        apply_plan(&registry, &plan);
        assert!(!directory.join("registry.tmp-crashed").exists());
    }
}
