//! Durable, explicitly approved replay for interrupted snapshot deletion.
//!
//! The deletion intent keeps a copy of the verified manifest and per-entry
//! device/inode evidence outside the tree being removed. After the durable
//! clearing marker, recovery accepts only a subset of the original entries;
//! every remaining entry must still match its recorded identity and content.
use super::*;
use std::collections::{HashMap, HashSet};
use std::io::Seek;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeletionEntryIdentity {
    path: String,
    identity: FilesystemIdentity,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeletionIntent {
    schema_version: u32,
    id: BackupId,
    source: PrefixRecord,
    manifest: BackupManifest,
    prefix_registry_revision: u64,
    deletion_journals: FilesystemIdentity,
    snapshots: FilesystemIdentity,
    deleting: FilesystemIdentity,
    mount_id: u64,
    root: FilesystemIdentity,
    payload: FilesystemIdentity,
    intent_file: FilesystemIdentity,
    manifest_file: FilesystemIdentity,
    intent_sha256: String,
    manifest_sha256: String,
    entries: Vec<DeletionEntryIdentity>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeletionEnvelope {
    body: DeletionIntent,
    checksum_sha256: String,
}

fn intent_checksum(intent: &DeletionIntent) -> Result<String, BackupError> {
    let bytes = serde_json::to_vec(intent).map_err(|_| BackupError::RecoveryBlocked)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn load_intent(tx: &File) -> Result<DeletionIntent, BackupError> {
    let envelope: DeletionEnvelope = read_json(tx, "intent.json")?;
    if intent_checksum(&envelope.body)? != envelope.checksum_sha256 {
        return Err(BackupError::RecoveryBlocked);
    }
    Ok(envelope.body)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum DeletionRecoveryState {
    PendingRetirement,
    Retired,
    Clearing,
    Completed,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeletionRecoveryPlan {
    pub backup: BackupId,
    pub scope: PrefixScope,
    pub prefix_registry_revision: u64,
    pub confirmation: String,
    pub state: DeletionRecoveryState,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeletionJournalInspection {
    pub backup: BackupId,
    pub scope: Option<PrefixScope>,
    pub state: Option<DeletionRecoveryState>,
    pub issue: Option<&'static str>,
}

fn digest(file: &File) -> Result<String, BackupError> {
    verify_private_file(file)?;
    Ok(format!(
        "{:x}",
        Sha256::digest(read_bounded(file.try_clone()?, MAX_METADATA)?)
    ))
}

fn get_dir(parent: &File, name: &str) -> Result<Option<File>, BackupError> {
    let d = child_dir(parent, OsStr::new(name), false)?;
    if let Some(d) = &d {
        verify_private_dir(d)?;
        if mount_id(d)? != mount_id(parent)? {
            return Err(BackupError::RecoveryBlocked);
        }
    }
    Ok(d)
}

fn check_metadata_file(
    dir: &File,
    name: &str,
    expected: FilesystemIdentity,
    hash: &str,
    allow_missing: bool,
) -> Result<(), BackupError> {
    let f = match open_at(dir, name, libc::O_RDONLY, 0) {
        Ok(f) => f,
        Err(PrefixError::Io(e)) if allow_missing && e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    if identity(&f)? != expected
        || digest(&f)? != hash
        || identity(&open_at(dir, name, libc::O_RDONLY, 0)?)? != expected
    {
        return Err(BackupError::RecoveryBlocked);
    }
    Ok(())
}

fn canonical_relative(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.starts_with('/')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn validate_intent(j: &DeletionIntent, id: &BackupId) -> Result<(), BackupError> {
    j.source.validate()?;
    if j.schema_version != 1
        || &j.id != id
        || j.source.owner_uid != crate::current_uid()
        || j.source.lifecycle != PrefixLifecycleState::Created
        || j.manifest.schema_version != 1
        || j.manifest.id != j.id
        || j.manifest.source != j.source
        || j.manifest.state != BackupState::Complete
        || j.manifest.integrity != BackupIntegrity::Verified
        || total(&j.manifest.inventory)? != j.manifest.payload_bytes
        || j.manifest.inventory.len() != j.entries.len()
        || j.entries.len() > MAX_ENTRIES
        || j.root.device != j.payload.device
        || j.deletion_journals.device != j.root.device
        || j.snapshots.device != j.root.device
        || j.deleting.device != j.root.device
        || j.entries
            .iter()
            .zip(&j.manifest.inventory)
            .any(|(identity, entry)| {
                !canonical_relative(&entry.path)
                    || identity.path != entry.path
                    || identity.identity.device != j.payload.device
                    || (entry.directory && (entry.size != 0 || !entry.sha256.is_empty()))
                    || (!entry.directory && entry.sha256.len() != 64)
            })
    {
        return Err(BackupError::RecoveryBlocked);
    }
    Ok(())
}

fn capture_entry_identities(
    dir: &File,
    parent: &str,
    depth: usize,
    mount: u64,
    out: &mut Vec<DeletionEntryIdentity>,
) -> Result<(), BackupError> {
    if depth > 64 || mount_id(dir)? != mount {
        return Err(BackupError::RecoveryBlocked);
    }
    for name in names(dir)? {
        if out.len() >= MAX_ENTRIES {
            return Err(BackupError::Quota);
        }
        let path = if parent.is_empty() {
            name.clone()
        } else {
            format!("{parent}/{name}")
        };
        if !canonical_relative(&path) {
            return Err(BackupError::RecoveryBlocked);
        }
        let f = open_at(dir, &name, libc::O_RDONLY, 0)?;
        let metadata = f.metadata()?;
        if metadata.uid() != crate::current_uid()
            || metadata.mode() & 0o7000 != 0
            || mount_id(&f)? != mount
            || unsafe { libc::flistxattr(f.as_raw_fd(), std::ptr::null_mut(), 0) } != 0
        {
            return Err(BackupError::RecoveryBlocked);
        }
        if metadata.is_dir() {
            out.push(DeletionEntryIdentity {
                path: path.clone(),
                identity: identity(&f)?,
            });
            capture_entry_identities(&f, &path, depth + 1, mount, out)?;
        } else if metadata.is_file() && metadata.nlink() == 1 {
            out.push(DeletionEntryIdentity {
                path,
                identity: identity(&f)?,
            });
        } else {
            return Err(BackupError::RecoveryBlocked);
        }
    }
    Ok(())
}

fn entry_map(
    j: &DeletionIntent,
) -> Result<HashMap<&str, (&InventoryEntry, FilesystemIdentity)>, BackupError> {
    if j.entries.len() != j.manifest.inventory.len() {
        return Err(BackupError::RecoveryBlocked);
    }
    let mut map = HashMap::with_capacity(j.entries.len());
    for (record, entry) in j.entries.iter().zip(&j.manifest.inventory) {
        if record.path != entry.path
            || !canonical_relative(&entry.path)
            || map
                .insert(entry.path.as_str(), (entry, record.identity))
                .is_some()
        {
            return Err(BackupError::RecoveryBlocked);
        }
    }
    Ok(map)
}

fn hash_content(
    file: &mut File,
    expected_size: u64,
    expected_hash: &str,
) -> Result<(), BackupError> {
    let before = file.metadata()?;
    if !before.is_file()
        || before.len() != expected_size
        || before.nlink() != 1
        || before.uid() != crate::current_uid()
        || before.blocks().saturating_mul(512) < before.len()
        || unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) } != 0
    {
        return Err(BackupError::RecoveryBlocked);
    }
    file.rewind()?;
    let mut hasher = Sha256::new();
    let mut remaining = expected_size;
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        remaining = remaining
            .checked_sub(u64::try_from(n).map_err(|_| BackupError::RecoveryBlocked)?)
            .ok_or(BackupError::RecoveryBlocked)?;
        hasher.update(&buffer[..n]);
    }
    if remaining != 0
        || format!("{:x}", hasher.finalize()) != expected_hash
        || !unchanged(&before, &file.metadata()?)
    {
        return Err(BackupError::RecoveryBlocked);
    }
    Ok(())
}

fn validate_remaining_payload(
    dir: &File,
    j: &DeletionIntent,
    clearing: bool,
) -> Result<(), BackupError> {
    let expected = entry_map(j)?;
    let mut seen = HashSet::with_capacity(expected.len());
    validate_remaining_dir(dir, "", 0, j.mount_id, clearing, &expected, &mut seen)?;
    if !clearing && seen.len() != expected.len() {
        return Err(BackupError::RecoveryBlocked);
    }
    Ok(())
}

fn validate_remaining_dir(
    dir: &File,
    parent: &str,
    depth: usize,
    mount: u64,
    clearing: bool,
    expected: &HashMap<&str, (&InventoryEntry, FilesystemIdentity)>,
    seen: &mut HashSet<String>,
) -> Result<(), BackupError> {
    if depth > 64 || mount_id(dir)? != mount {
        return Err(BackupError::RecoveryBlocked);
    }
    for name in names(dir)? {
        if seen.len() >= MAX_ENTRIES {
            return Err(BackupError::Quota);
        }
        let path = if parent.is_empty() {
            name.clone()
        } else {
            format!("{parent}/{name}")
        };
        if !canonical_relative(&path) || !seen.insert(path.clone()) {
            return Err(BackupError::RecoveryBlocked);
        }
        let (record, expected_identity) = expected
            .get(path.as_str())
            .ok_or(BackupError::RecoveryBlocked)?;
        let mut file = open_at(dir, &name, libc::O_RDONLY, 0)?;
        let metadata = file.metadata()?;
        if identity(&file)? != *expected_identity
            || metadata.uid() != crate::current_uid()
            || metadata.mode() & 0o7000 != 0
            || mount_id(&file)? != mount
            || unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) } != 0
        {
            return Err(BackupError::RecoveryBlocked);
        }
        if metadata.is_dir() && record.directory {
            let mode = metadata.mode() & 0o777;
            let allowed_mode = clearing && mode == (record.mode | 0o700);
            if (mode != record.mode && !allowed_mode)
                || (!clearing
                    && (metadata.mtime() != record.modified_seconds
                        || metadata.mtime_nsec() != record.modified_nanoseconds))
            {
                return Err(BackupError::RecoveryBlocked);
            }
            validate_remaining_dir(&file, &path, depth + 1, mount, clearing, expected, seen)?;
        } else if metadata.is_file() && !record.directory {
            if metadata.mode() & 0o777 != record.mode
                || metadata.mtime() != record.modified_seconds
                || metadata.mtime_nsec() != record.modified_nanoseconds
                || metadata.len() != record.size
                || record.sha256.len() != 64
            {
                return Err(BackupError::RecoveryBlocked);
            }
            hash_content(&mut file, record.size, &record.sha256)?;
        } else {
            return Err(BackupError::RecoveryBlocked);
        }
        if identity(&file)? != *expected_identity
            || identity(&open_at(dir, &name, libc::O_RDONLY, 0)?)? != *expected_identity
        {
            return Err(BackupError::RecoveryBlocked);
        }
    }
    Ok(())
}

fn check_retired(dir: &File, j: &DeletionIntent, clearing: bool) -> Result<(), BackupError> {
    if identity(dir)? != j.root
        || names(dir)?
            .iter()
            .any(|n| !["payload", "intent.json", "manifest.json"].contains(&n.as_str()))
    {
        return Err(BackupError::RecoveryBlocked);
    }
    check_metadata_file(
        dir,
        "intent.json",
        j.intent_file,
        &j.intent_sha256,
        clearing,
    )?;
    check_metadata_file(
        dir,
        "manifest.json",
        j.manifest_file,
        &j.manifest_sha256,
        clearing,
    )?;
    let payload = get_dir(dir, "payload")?;
    match payload {
        Some(payload) if identity(&payload)? == j.payload => {
            validate_remaining_payload(&payload, j, clearing)?;
        }
        None if clearing => {}
        _ => return Err(BackupError::RecoveryBlocked),
    }
    if !clearing && names(dir)? != ["intent.json", "manifest.json", "payload"] {
        return Err(BackupError::RecoveryBlocked);
    }
    Ok(())
}

impl BackupStore {
    pub(super) fn start_deletion(
        &self,
        id: &BackupId,
        manifest: &BackupManifest,
        snapshots: &File,
        deleting: &File,
        dir: &File,
    ) -> Result<(), BackupError> {
        let payload = open_at(dir, "payload", libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        let root_identity = identity(dir)?;
        let payload_identity = identity(&payload)?;
        let mount = mount_id(snapshots)?;
        if mount_id(deleting)? != mount
            || mount_id(dir)? != mount
            || mount_id(&payload)? != mount
            || scan(&payload, None)? != manifest.inventory
        {
            return Err(BackupError::RecoveryBlocked);
        }
        let mut entries = Vec::with_capacity(manifest.inventory.len());
        capture_entry_identities(&payload, "", 0, mount, &mut entries)?;
        if entries
            .iter()
            .map(|entry| entry.path.as_str())
            .ne(manifest.inventory.iter().map(|entry| entry.path.as_str()))
            || scan(&payload, None)? != manifest.inventory
            || identity(dir)? != root_identity
            || identity(&payload)? != payload_identity
        {
            return Err(BackupError::RecoveryBlocked);
        }
        let intent_file = open_at(dir, "intent.json", libc::O_RDONLY, 0)?;
        let manifest_file = open_at(dir, "manifest.json", libc::O_RDONLY, 0)?;
        let recorded: PrefixRecord = read_json(dir, "intent.json")?;
        if recorded != manifest.source {
            return Err(BackupError::RecoveryBlocked);
        }
        let journal_root = self
            .sub("deletion-journals", true)?
            .ok_or(PrefixError::Blocked)?;
        let revision = self.prefixes.load_snapshot()?.registry.revision;
        let intent = DeletionIntent {
            schema_version: 1,
            id: id.clone(),
            source: manifest.source.clone(),
            manifest: manifest.clone(),
            prefix_registry_revision: revision,
            deletion_journals: identity(&journal_root)?,
            snapshots: identity(snapshots)?,
            deleting: identity(deleting)?,
            mount_id: mount,
            root: root_identity,
            payload: payload_identity,
            intent_file: identity(&intent_file)?,
            manifest_file: identity(&manifest_file)?,
            intent_sha256: digest(&intent_file)?,
            manifest_sha256: digest(&manifest_file)?,
            entries,
        };
        validate_intent(&intent, id)?;
        let envelope = DeletionEnvelope {
            checksum_sha256: intent_checksum(&intent)?,
            body: intent,
        };
        let encoded = serde_json::to_vec(&envelope).map_err(|_| BackupError::RecoveryBlocked)?;
        if encoded.len() > MAX_METADATA {
            return Err(BackupError::Quota);
        }
        let tx = exclusive_dir(&journal_root, id.as_str())?;
        write_json(&tx, "intent.json", &envelope)?;
        self.checkpoint("delete-intent")?;
        if manifest.inventory.is_empty() {
            self.checkpoint("delete-empty-intent")?;
        }
        self.resume_deletion_locked(id, &manifest.source.scope, revision, None)?;
        Ok(())
    }

    /// Inspect all durable deletion journals, including completed ones.
    pub fn deletion_journals(&self) -> Result<Vec<DeletionJournalInspection>, BackupError> {
        let Some(root) = self.sub("deletion-journals", false)? else {
            return Ok(vec![]);
        };
        let mut reports = vec![];
        for name in names(&root)? {
            let backup = BackupId::new(name)?;
            let report = match self.deletion_plan(&backup) {
                Ok(plan) => DeletionJournalInspection {
                    backup,
                    scope: Some(plan.scope),
                    state: Some(plan.state),
                    issue: None,
                },
                Err(_) => DeletionJournalInspection {
                    backup,
                    scope: None,
                    state: None,
                    issue: Some("blocked: journal is incomplete, corrupt, conflicted, or unsafe"),
                },
            };
            reports.push(report);
        }
        Ok(reports)
    }

    pub fn deletion_plan(&self, id: &BackupId) -> Result<DeletionRecoveryPlan, BackupError> {
        BackupId::new(id.as_str())?;
        let journal_root = self
            .sub("deletion-journals", false)?
            .ok_or(BackupError::RecoveryBlocked)?;
        let tx = get_dir(&journal_root, id.as_str())?.ok_or(BackupError::RecoveryBlocked)?;
        let j = load_intent(&tx)?;
        validate_intent(&j, id)?;
        if identity(&journal_root)? != j.deletion_journals || mount_id(&journal_root)? != j.mount_id
        {
            return Err(BackupError::RecoveryBlocked);
        }
        let snapshot = self.prefixes.load_snapshot()?;
        let revision = snapshot.registry.revision;
        let completed_marker = has_file(&tx, "completed.json")?;
        if !completed_marker && revision != j.prefix_registry_revision {
            return Err(BackupError::RecoveryBlocked);
        }
        let snapshots = self
            .sub("snapshots", false)?
            .ok_or(BackupError::RecoveryBlocked)?;
        let deleting = self
            .sub("deleting", false)?
            .ok_or(BackupError::RecoveryBlocked)?;
        if identity(&snapshots)? != j.snapshots
            || identity(&deleting)? != j.deleting
            || mount_id(&snapshots)? != j.mount_id
            || mount_id(&deleting)? != j.mount_id
        {
            return Err(BackupError::RecoveryBlocked);
        }
        let confirmation = format!(
            "resume-delete:{}:{}:{}:{}",
            id.as_str(),
            j.root.device,
            j.root.inode,
            j.payload.inode
        );
        let entries = names(&tx)?;
        if entries
            .iter()
            .any(|n| !["intent.json", "clearing.json", "completed.json"].contains(&n.as_str()))
        {
            return Err(BackupError::RecoveryBlocked);
        }
        let clearing = has_file(&tx, "clearing.json")?;
        if clearing && read_json::<FilesystemIdentity>(&tx, "clearing.json")? != j.root {
            return Err(BackupError::RecoveryBlocked);
        }
        let source = get_dir(&snapshots, id.as_str())?;
        let retired = get_dir(&deleting, id.as_str())?;
        if source.is_some() && retired.is_some() {
            return Err(BackupError::RecoveryBlocked);
        }
        if self.transaction_dir(&j.source.id, false)?.is_some() {
            return Err(BackupError::RecoveryBlocked);
        }
        if let Some(pins) = self.sub("pin-recovery", false)? {
            if names(&pins)?
                .iter()
                .any(|name| name.starts_with(&format!("{}-", id.as_str())))
            {
                return Err(BackupError::RecoveryBlocked);
            }
        }
        let completed = completed_marker;
        let state = if completed {
            if !clearing
                || read_json::<FilesystemIdentity>(&tx, "completed.json")? != j.root
                || source.is_some()
                || retired.is_some()
            {
                return Err(BackupError::RecoveryBlocked);
            }
            DeletionRecoveryState::Completed
        } else {
            match (source, retired, clearing) {
                (Some(dir), None, false) => {
                    check_retired(&dir, &j, false)?;
                    DeletionRecoveryState::PendingRetirement
                }
                (None, Some(dir), false) => {
                    check_retired(&dir, &j, false)?;
                    DeletionRecoveryState::Retired
                }
                (None, Some(dir), true) => {
                    check_retired(&dir, &j, true)?;
                    DeletionRecoveryState::Clearing
                }
                (None, None, true) => DeletionRecoveryState::Clearing,
                _ => return Err(BackupError::RecoveryBlocked),
            }
        };
        Ok(DeletionRecoveryPlan {
            backup: id.clone(),
            scope: j.source.scope,
            prefix_registry_revision: revision,
            confirmation,
            state,
        })
    }

    pub fn deletion_resume(
        &self,
        id: &BackupId,
        scope: &PrefixScope,
        expected: u64,
        confirmation: &str,
    ) -> Result<DeletionRecoveryPlan, BackupError> {
        let _guard = self.fs.lock_registry()?;
        self.resume_deletion_locked(id, scope, expected, Some(confirmation))
    }

    fn resume_deletion_locked(
        &self,
        id: &BackupId,
        scope: &PrefixScope,
        expected: u64,
        confirmation: Option<&str>,
    ) -> Result<DeletionRecoveryPlan, BackupError> {
        let plan = self.deletion_plan(id)?;
        if &plan.scope != scope {
            return Err(PrefixError::InvalidScope.into());
        }
        if plan.prefix_registry_revision != expected {
            return Err(PrefixError::RevisionConflict.into());
        }
        if confirmation.is_some_and(|value| value != plan.confirmation) {
            return Err(BackupError::Confirmation);
        }
        if plan.state == DeletionRecoveryState::Completed {
            return Ok(plan);
        }
        let snapshots = self
            .sub("snapshots", false)?
            .ok_or(BackupError::RecoveryBlocked)?;
        let deleting = self
            .sub("deleting", false)?
            .ok_or(BackupError::RecoveryBlocked)?;
        let journal_root = self
            .sub("deletion-journals", false)?
            .ok_or(BackupError::RecoveryBlocked)?;
        let tx = get_dir(&journal_root, id.as_str())?.ok_or(BackupError::RecoveryBlocked)?;
        let j = load_intent(&tx)?;
        if plan.state == DeletionRecoveryState::PendingRetirement {
            rename(
                &snapshots,
                id.as_str(),
                &deleting,
                id.as_str(),
                libc::RENAME_NOREPLACE,
            )?;
            self.checkpoint("delete-retired")?;
        }
        // Revalidate the entire remaining inventory before recording the first
        // unlink as authorized. The marker is durable before any chmod/unlink.
        let current = self.deletion_plan(id)?;
        if current.prefix_registry_revision != expected || current.confirmation != plan.confirmation
        {
            return Err(BackupError::RecoveryBlocked);
        }
        if !has_file(&tx, "clearing.json")? {
            write_json(&tx, "clearing.json", &j.root)?;
        }
        self.checkpoint("delete-ready")?;
        if j.manifest.inventory.is_empty() {
            self.checkpoint("delete-empty-ready")?;
        }
        if let Some(dir) = get_dir(&deleting, id.as_str())? {
            check_retired(&dir, &j, true)?;
            if let Some(payload) = get_dir(&dir, "payload")? {
                if identity(&payload)? != j.payload {
                    return Err(BackupError::RecoveryBlocked);
                }
                delete_journaled_tree(self, &payload, "", 0, &j)?;
                if !names(&payload)?.is_empty()
                    || identity(&open_at(
                        &dir,
                        "payload",
                        libc::O_RDONLY | libc::O_DIRECTORY,
                        0,
                    )?)? != j.payload
                {
                    return Err(BackupError::RecoveryBlocked);
                }
                unlink_at(&dir, "payload", libc::AT_REMOVEDIR)?;
                dir.sync_all()?;
            }
            self.checkpoint("delete-payload")?;
            if j.manifest.inventory.is_empty() {
                self.checkpoint("delete-empty-payload")?;
            }
            for (name, file_id, hash, checkpoint, empty_checkpoint) in [
                (
                    "intent.json",
                    j.intent_file,
                    j.intent_sha256.as_str(),
                    "delete-meta-intent",
                    "delete-empty-source",
                ),
                (
                    "manifest.json",
                    j.manifest_file,
                    j.manifest_sha256.as_str(),
                    "delete-meta-manifest",
                    "delete-empty-manifest",
                ),
            ] {
                check_metadata_file(&dir, name, file_id, hash, true)?;
                if has_file(&dir, name)? {
                    check_metadata_file(&dir, name, file_id, hash, false)?;
                    unlink_at(&dir, name, 0)?;
                    dir.sync_all()?;
                }
                self.checkpoint(checkpoint)?;
                if j.manifest.inventory.is_empty() {
                    self.checkpoint(empty_checkpoint)?;
                }
            }
            if !names(&dir)?.is_empty()
                || identity(&open_at(
                    &deleting,
                    id.as_str(),
                    libc::O_RDONLY | libc::O_DIRECTORY,
                    0,
                )?)? != j.root
            {
                return Err(BackupError::RecoveryBlocked);
            }
            unlink_at(&deleting, id.as_str(), libc::AT_REMOVEDIR)?;
            deleting.sync_all()?;
        }
        self.checkpoint("delete-root")?;
        if j.manifest.inventory.is_empty() {
            self.checkpoint("delete-empty-root")?;
        }
        if !has_file(&tx, "completed.json")? {
            write_json(&tx, "completed.json", &j.root)?;
        }
        self.checkpoint("delete-completed")?;
        if j.manifest.inventory.is_empty() {
            self.checkpoint("delete-empty-completed")?;
        }
        self.deletion_plan(id)
    }
}

fn delete_journaled_tree(
    store: &BackupStore,
    dir: &File,
    parent: &str,
    depth: usize,
    j: &DeletionIntent,
) -> Result<(), BackupError> {
    let expected = entry_map(j)?;
    delete_journaled_dir(store, dir, parent, depth, j, &expected)
}

fn delete_journaled_dir(
    store: &BackupStore,
    dir: &File,
    parent: &str,
    depth: usize,
    j: &DeletionIntent,
    expected: &HashMap<&str, (&InventoryEntry, FilesystemIdentity)>,
) -> Result<(), BackupError> {
    if depth > 64 || mount_id(dir)? != j.mount_id {
        return Err(BackupError::RecoveryBlocked);
    }
    for name in names(dir)? {
        let path = if parent.is_empty() {
            name.clone()
        } else {
            format!("{parent}/{name}")
        };
        let (entry, recorded_identity) = expected
            .get(path.as_str())
            .ok_or(BackupError::RecoveryBlocked)?;
        let mut file = open_at(dir, &name, libc::O_RDONLY, 0)?;
        let metadata = file.metadata()?;
        if identity(&file)? != *recorded_identity
            || metadata.uid() != crate::current_uid()
            || metadata.mode() & 0o7000 != 0
            || mount_id(&file)? != j.mount_id
            || unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) } != 0
        {
            return Err(BackupError::RecoveryBlocked);
        }
        let flags = if metadata.is_dir() && entry.directory {
            let mode = metadata.mode() & 0o777;
            if mode != entry.mode && mode != (entry.mode | 0o700) {
                return Err(BackupError::RecoveryBlocked);
            }
            file.set_permissions(fs::Permissions::from_mode(mode | 0o700))?;
            file.sync_all()?;
            delete_journaled_dir(store, &file, &path, depth + 1, j, expected)?;
            if !names(&file)?.is_empty()
                || identity(&open_at(dir, &name, libc::O_RDONLY | libc::O_DIRECTORY, 0)?)?
                    != *recorded_identity
            {
                return Err(BackupError::RecoveryBlocked);
            }
            libc::AT_REMOVEDIR
        } else if metadata.is_file() && !entry.directory {
            if metadata.mode() & 0o777 != entry.mode
                || metadata.mtime() != entry.modified_seconds
                || metadata.mtime_nsec() != entry.modified_nanoseconds
            {
                return Err(BackupError::RecoveryBlocked);
            }
            hash_content(&mut file, entry.size, &entry.sha256)?;
            if identity(&open_at(dir, &name, libc::O_RDONLY, 0)?)? != *recorded_identity {
                return Err(BackupError::RecoveryBlocked);
            }
            0
        } else {
            return Err(BackupError::RecoveryBlocked);
        };
        if identity(&file)? != *recorded_identity
            || identity(&open_at(dir, &name, libc::O_RDONLY, 0)?)? != *recorded_identity
        {
            return Err(BackupError::RecoveryBlocked);
        }
        unlink_at(dir, &name, flags)?;
        dir.sync_all()?;
        store.checkpoint("delete-unlink")?;
    }
    Ok(())
}
