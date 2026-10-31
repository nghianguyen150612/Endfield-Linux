use super::*;
use crate::{PrefixLifecycleState, PrefixRecord, PrefixScope, PrefixStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;

const MAX_ENTRIES: usize = 100_000;
const MAX_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const RESERVE: u64 = 64 * 1024 * 1024;
const MAX_METADATA: usize = 32 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error(transparent)]
    Prefix(#[from] PrefixError),
    #[error("backup integrity: {0:?}")]
    Integrity(BackupIntegrity),
    #[error("saved runtime failed exact revalidation")]
    RuntimeBlocked,
    #[error("unsupported snapshot entry or metadata")]
    Unsupported,
    #[error("snapshot quota or available space exceeded")]
    Quota,
    #[error("explicit confirmation required")]
    Confirmation,
    #[error("operation requires inspection; preserved data was not removed")]
    RecoveryBlocked,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BackupId(String);
impl BackupId {
    pub fn new(s: impl Into<String>) -> Result<Self, BackupError> {
        let s = s.into();
        PrefixId::new(&s)?;
        Ok(Self(s))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupState {
    Complete,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupIntegrity {
    Verified,
    Corrupt,
    Incomplete,
    Unsupported,
    Missing,
    Unsafe,
    Unverifiable,
}
pub type BackupRuntimeReference = crate::PrefixRuntimeAssociation;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryEntry {
    pub path: String,
    pub directory: bool,
    pub size: u64,
    pub mode: u32,
    pub sha256: String,
    pub modified_seconds: i64,
    pub modified_nanoseconds: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    pub schema_version: u32,
    pub id: BackupId,
    pub source: PrefixRecord,
    pub prefix_registry_revision: u64,
    pub created_at: u64,
    pub state: BackupState,
    pub integrity: BackupIntegrity,
    pub inventory: Vec<InventoryEntry>,
    pub payload_bytes: u64,
    pub observation_limits: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct BackupPlan {
    pub source: PrefixRecord,
    pub registry_revision: u64,
    pub entries: usize,
    pub bytes: u64,
    pub available: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum SelectionAssessmentState {
    Selected,
    NoPin,
    Blocked,
    Unverifiable,
}
#[derive(Clone, Debug, Serialize)]
pub struct RestoreSelectionAssessment {
    pub state: SelectionAssessmentState,
    pub pin_registry_revision: Option<u64>,
    pub source: String,
    pub matches_saved_association: Option<bool>,
    pub saved_runtime_valid: bool,
    pub automatic_fallback: bool,
    pub diagnostic: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct RestorePlan {
    pub backup: BackupId,
    pub integrity: BackupIntegrity,
    pub backup_runtime: BackupRuntimeReference,
    pub original_lifecycle: PrefixLifecycleState,
    pub current_selection_status: String,
    pub current_selection: Option<RestoreSelectionAssessment>,
    pub saved_adapter_status: String,
    pub recovery_strategy: String,
    pub destination: PrefixRecord,
    pub registry_revision: u64,
    pub bytes: u64,
    pub replaces_existing_data: bool,
    pub confirmation: String,
}
impl RestorePlan {
    /// Assessment only. A stale saved reference is retained; no pin is mutated
    /// and no replacement runtime is selected for the restored prefix.
    pub fn assess_selection(
        &mut self,
        registry: Option<&endfield_unix_runtime_selection::PinRegistry>,
        mut validate: impl FnMut(&crate::PinnedRuntimeReference) -> Result<(), String>,
    ) {
        use endfield_unix_runtime_selection::{ResolutionState, resolve};
        let saved = validate(&self.backup_runtime.runtime);
        let mut assessment = RestoreSelectionAssessment {
            state: SelectionAssessmentState::Unverifiable,
            pin_registry_revision: None,
            source: "unknown".into(),
            matches_saved_association: None,
            saved_runtime_valid: saved.is_ok(),
            automatic_fallback: false,
            diagnostic: saved.err().map(|s| s.chars().take(256).collect()),
        };
        if let Some(registry) = registry {
            let (machine, profile) = match &self.destination.scope {
                PrefixScope::Machine { machine } => (machine, None),
                PrefixScope::Profile { machine, profile } => (machine, Some(profile)),
            };
            let report = resolve(registry, machine, profile, None, &mut validate);
            assessment.pin_registry_revision = Some(report.registry_revision);
            assessment.source = format!("{:?}", report.source);
            assessment.state = match &report.state {
                ResolutionState::Selected => SelectionAssessmentState::Selected,
                ResolutionState::NoPin => SelectionAssessmentState::NoPin,
                ResolutionState::Stale(reason) => {
                    assessment.diagnostic = Some(reason.chars().take(256).collect());
                    SelectionAssessmentState::Blocked
                }
            };
            if let Some(runtime) = report.runtime {
                let pin = registry
                    .pins
                    .iter()
                    .find(|p| Some(&p.pin_id) == report.pin_id.as_ref());
                let source_matches = matches!(
                    (report.source, self.backup_runtime.selection_source),
                    (
                        endfield_unix_runtime_selection::SelectionSource::MachinePin,
                        crate::RuntimeSelectionSource::MachinePin
                    ) | (
                        endfield_unix_runtime_selection::SelectionSource::ProfilePin,
                        crate::RuntimeSelectionSource::ProfilePin
                    )
                );
                assessment.matches_saved_association = Some(
                    runtime == self.backup_runtime.runtime
                        && report.pin_id.as_deref() == Some(self.backup_runtime.pin_id.as_str())
                        && source_matches
                        && pin.is_some_and(|p| {
                            p.adapter_preference == self.backup_runtime.adapter_preference
                        }),
                );
            }
        }
        self.current_selection_status = format!(
            "{:?}; saved runtime {}; pins unchanged",
            assessment.state,
            if assessment.saved_runtime_valid {
                "valid"
            } else {
                "blocked"
            }
        );
        self.current_selection = Some(assessment);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreTransaction {
    pub schema_version: u32,
    pub backup: BackupId,
    pub original: PrefixRecord,
    pub expected_revision: u64,
    pub staged: FilesystemIdentity,
    /// Present in schema 2; schema 1 journals have no original-content evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_inventory: Option<Vec<InventoryEntry>>,
}
#[derive(Clone, Debug, Serialize)]
pub enum RecoveryAction {
    None,
    InspectPreservedStaging,
    ResumeStagedRestore,
    FinalizePublishedRestore,
    RollbackInProgress,
    RollbackCompleted,
}
#[derive(Clone, Debug, Serialize)]
pub struct RollbackPlan {
    pub prefix: PrefixId,
    pub backup: BackupId,
    pub expected_revision: u64,
    pub confirmation: String,
    pub already_exchanged: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum DeletionState {
    Incomplete,
    Unverifiable,
    Unsafe,
}
#[derive(Clone, Debug, Serialize)]
pub struct DeletionInspection {
    pub backup: BackupId,
    pub state: DeletionState,
    pub remaining_entries: Option<usize>,
    pub original_entries: Option<usize>,
    pub detail: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct RecoveryReport {
    pub action: RecoveryAction,
    pub original_preserved: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackupOwner {
    schema_version: u32,
    uid: u32,
    parent: FilesystemIdentity,
    root: FilesystemIdentity,
}

pub struct BackupStore {
    fs: LinuxPrefixFilesystem,
    prefixes: PrefixStore,
    #[cfg(test)]
    usage_override: Option<Option<bool>>,
    #[cfg(test)]
    fault: Option<&'static str>,
}
impl BackupStore {
    pub fn for_app(paths: &endfield_unix_storage::AppPaths) -> Self {
        Self {
            fs: LinuxPrefixFilesystem::new(paths.data_dir()),
            prefixes: PrefixStore::for_app(paths),
            #[cfg(test)]
            usage_override: None,
            #[cfg(test)]
            fault: None,
        }
    }
    fn checkpoint(&self, _name: &str) -> Result<(), BackupError> {
        #[cfg(test)]
        if std::env::var("ENDFIELD_P029_EXIT_AT").ok().as_deref() == Some(_name) {
            unsafe { libc::_exit(91) }
        }
        #[cfg(test)]
        if self.fault == Some(_name) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "synthetic interruption",
            )
            .into());
        }
        Ok(())
    }
    fn area(&self, create: bool) -> Result<Option<File>, BackupError> {
        self.fs.reject_external_alias()?;
        let Some(app) = open_path_directory(&self.fs.data_dir, create, true)? else {
            return Ok(None);
        };
        let root = match child_dir(&app, OsStr::new("backups"), false)? {
            Some(root) => root,
            None if !create => return Ok(None),
            None => {
                let root = exclusive_dir(&app, "backups")?;
                write_json(
                    &root,
                    "owner.json",
                    &BackupOwner {
                        schema_version: 1,
                        uid: crate::current_uid(),
                        parent: identity(&app)?,
                        root: identity(&root)?,
                    },
                )?;
                root
            }
        };
        verify_private_dir(&root)?;
        let owner: BackupOwner = read_json(&root, "owner.json")?;
        if owner.schema_version != 1
            || owner.uid != crate::current_uid()
            || owner.parent != identity(&app)?
            || owner.root != identity(&root)?
        {
            return Err(PrefixError::Blocked.into());
        }
        if mount_id(&app)? != mount_id(&root)? {
            return Err(PrefixError::Blocked.into());
        }
        Ok(Some(root))
    }
    fn sub(&self, name: &str, create: bool) -> Result<Option<File>, BackupError> {
        let Some(root) = self.area(create)? else {
            return Ok(None);
        };
        let dir = child_dir(&root, OsStr::new(name), create)?;
        if let Some(dir) = &dir {
            verify_private_dir(dir)?;
            if mount_id(dir)? != mount_id(&root)? {
                return Err(PrefixError::Blocked.into());
            }
        }
        Ok(dir)
    }
    fn source(&self, id: &PrefixId) -> Result<File, BackupError> {
        let l = self.fs.layout(false)?.ok_or(PrefixError::Missing)?;
        Ok(self.fs.open_prefix(&l, id)?.ok_or(PrefixError::Missing)?)
    }
    fn inactive(&self, id: &PrefixId) -> Result<(), BackupError> {
        #[cfg(test)]
        let usage = self.usage_override.unwrap_or(self.fs.prefix_in_use(id)?);
        #[cfg(not(test))]
        let usage = self.fs.prefix_in_use(id)?;
        match usage {
            Some(false) => Ok(()),
            Some(true) => Err(PrefixError::ActiveUse.into()),
            None => Err(PrefixError::UsageUnknown.into()),
        }
    }
    fn record(&self, id: &PrefixId) -> Result<(PrefixRecord, u64), BackupError> {
        let snap = self.prefixes.load_snapshot()?;
        let r = snap
            .registry
            .record(id)
            .ok_or(PrefixError::NotRegistered)?
            .clone();
        if r.owner_uid != crate::current_uid()
            || r.lifecycle != PrefixLifecycleState::Created
            || self.fs.inspect_directory(id)? != r.filesystem_identity
        {
            return Err(PrefixError::Blocked.into());
        }
        Ok((r, snap.registry.revision))
    }
    pub fn plan(&self, id: &PrefixId, scope: &PrefixScope) -> Result<BackupPlan, BackupError> {
        self.ensure_no_restore(id)?;
        if let Some(staging) = self.sub("staging", false)? {
            for pending in names(&staging)? {
                let d = open_at(&staging, &pending, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
                let record: PrefixRecord = read_json(&d, "intent.json")?;
                if record.id == *id {
                    return Err(BackupError::RecoveryBlocked);
                }
            }
        }
        let (source, registry_revision) = self.record(id)?;
        if &source.scope != scope {
            return Err(PrefixError::InvalidScope.into());
        }
        self.inactive(id)?;
        let inventory = scan(&self.source(id)?, None)?;
        let bytes = total(&inventory)?;
        let available = self.fs.free_bytes().ok_or(BackupError::Quota)?;
        if available < bytes.checked_add(RESERVE).ok_or(BackupError::Quota)? {
            return Err(BackupError::Quota);
        }
        Ok(BackupPlan {
            source,
            registry_revision,
            entries: inventory.len(),
            bytes,
            available,
        })
    }
    pub fn create(
        &self,
        id: &PrefixId,
        scope: &PrefixScope,
        expected: u64,
        confirmed: bool,
    ) -> Result<BackupManifest, BackupError> {
        if !confirmed {
            return Err(BackupError::Confirmation);
        }
        let _rg = self.fs.lock_registry()?;
        let _pg = self.fs.lock_prefix(id)?;
        let plan = self.plan(id, scope)?;
        if plan.registry_revision != expected {
            return Err(PrefixError::RevisionConflict.into());
        }
        let staging = self.sub("staging", true)?.ok_or(PrefixError::Blocked)?;
        let snapshots = self.sub("snapshots", true)?.ok_or(PrefixError::Blocked)?;
        let id = BackupId::new(format!(
            "b-{}-{}-{}",
            crate::now(),
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))?;
        let dir = exclusive_dir(&staging, id.as_str())?;
        write_json(&dir, "intent.json", &plan.source)?;
        self.checkpoint("backup-intent")?;
        let payload = exclusive_dir(&dir, "payload")?;
        let src = self.source(&plan.source.id)?;
        let before = scan(&src, None)?;
        let copied = scan(&src, Some(&payload))?;
        if before != copied || copied != scan(&src, None)? || copied != scan(&payload, None)? {
            return Err(BackupError::Integrity(BackupIntegrity::Unverifiable));
        }
        self.inactive(&plan.source.id)?;
        if self.fs.inspect_directory(&plan.source.id)? != plan.source.filesystem_identity {
            return Err(PrefixError::Blocked.into());
        }
        let manifest = BackupManifest { schema_version: 1, id: id.clone(), source: plan.source, prefix_registry_revision: expected, created_at: crate::now(), state: BackupState::Complete, integrity: BackupIntegrity::Verified, payload_bytes: total(&copied)?, inventory: copied, observation_limits: "Visible same-UID WINEPREFIX environment scan and repeated tree hashes; advisory locks do not exclude arbitrary same-user writers. Offline cooperative use required.".into() };
        write_json(&dir, "manifest.json", &manifest)?;
        self.checkpoint("backup-staged")?;
        rename(
            &staging,
            id.as_str(),
            &snapshots,
            id.as_str(),
            libc::RENAME_NOREPLACE,
        )?;
        self.checkpoint("backup-published")?;
        Ok(manifest)
    }
    pub fn list(&self) -> Result<Vec<BackupId>, BackupError> {
        let Some(root) = self.sub("snapshots", false)? else {
            return Ok(vec![]);
        };
        names(&root)?.into_iter().map(BackupId::new).collect()
    }
    fn snapshot(&self, id: &BackupId) -> Result<File, BackupError> {
        BackupId::new(id.as_str())?;
        let root = self
            .sub("snapshots", false)?
            .ok_or(BackupError::Integrity(BackupIntegrity::Missing))?;
        let d = open_at(&root, id.as_str(), libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        verify_private_dir(&d)?;
        if mount_id(&d)? != mount_id(&root)? {
            return Err(PrefixError::Blocked.into());
        }
        Ok(d)
    }
    pub fn show(&self, id: &BackupId) -> Result<BackupManifest, BackupError> {
        let d = self.snapshot(id)?;
        let m: BackupManifest = read_json(&d, "manifest.json")?;
        if m.schema_version != 1 {
            return Err(BackupError::Integrity(BackupIntegrity::Unsupported));
        }
        m.source.validate()?;
        if m.id != *id
            || m.source.owner_uid != crate::current_uid()
            || m.source.lifecycle != PrefixLifecycleState::Created
            || m.inventory.len() > MAX_ENTRIES
            || m.payload_bytes != total(&m.inventory)?
            || m.integrity != BackupIntegrity::Verified
        {
            return Err(BackupError::Integrity(BackupIntegrity::Corrupt));
        }
        Ok(m)
    }
    pub fn verify(
        &self,
        id: &BackupId,
        scope: &PrefixScope,
    ) -> Result<BackupManifest, BackupError> {
        let m = self.show(id)?;
        if &m.source.scope != scope {
            return Err(PrefixError::InvalidScope.into());
        }
        let d = self.snapshot(id)?;
        if names(&d)? != ["intent.json", "manifest.json", "payload"] {
            return Err(BackupError::Integrity(BackupIntegrity::Unsafe));
        }
        let payload = open_at(&d, "payload", libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        verify_private_dir(&payload)?;
        if mount_id(&payload)? != mount_id(&d)? || scan(&payload, None)? != m.inventory {
            return Err(BackupError::Integrity(BackupIntegrity::Corrupt));
        }
        Ok(m)
    }
    pub fn pending_backups(&self) -> Result<Vec<BackupId>, BackupError> {
        let Some(root) = self.sub("staging", false)? else {
            return Ok(vec![]);
        };
        names(&root)?.into_iter().map(BackupId::new).collect()
    }
    pub fn recover_backup(
        &self,
        id: &BackupId,
        scope: &PrefixScope,
        confirmed: bool,
    ) -> Result<BackupManifest, BackupError> {
        if !confirmed {
            return Err(BackupError::Confirmation);
        }
        let _guard = self.fs.lock_registry()?;
        if self.list()?.contains(id) {
            return self.verify(id, scope);
        }
        let staging = self
            .sub("staging", false)?
            .ok_or(BackupError::Integrity(BackupIntegrity::Missing))?;
        let dir = open_at(&staging, id.as_str(), libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        verify_private_dir(&dir)?;
        if mount_id(&dir)? != mount_id(&staging)? {
            return Err(PrefixError::Blocked.into());
        }
        let m: BackupManifest = read_json(&dir, "manifest.json")
            .map_err(|_| BackupError::Integrity(BackupIntegrity::Incomplete))?;
        m.source.validate()?;
        if m.schema_version != 1
            || m.id != *id
            || &m.source.scope != scope
            || m.source.owner_uid != crate::current_uid()
            || m.source.lifecycle != PrefixLifecycleState::Created
            || m.integrity != BackupIntegrity::Verified
        {
            return Err(BackupError::RecoveryBlocked);
        }
        let payload = open_at(&dir, "payload", libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        if mount_id(&payload)? != mount_id(&dir)?
            || scan(&payload, None)? != m.inventory
            || total(&m.inventory)? != m.payload_bytes
        {
            return Err(BackupError::Integrity(BackupIntegrity::Corrupt));
        }
        let published = self.sub("snapshots", true)?.ok_or(PrefixError::Blocked)?;
        rename(
            &staging,
            id.as_str(),
            &published,
            id.as_str(),
            libc::RENAME_NOREPLACE,
        )?;
        self.verify(id, scope)
    }
    /// Read-only inspection of retired or partially removed snapshots. This
    /// never resumes unlink, invents a manifest, or follows inventory paths.
    pub fn deletions(&self) -> Result<Vec<DeletionInspection>, BackupError> {
        let Some(root) = self.sub("deleting", false)? else {
            return Ok(vec![]);
        };
        let mut reports = vec![];
        for name in names(&root)? {
            let backup = BackupId::new(&name)?;
            let dir = open_at(&root, &name, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
            verify_private_dir(&dir)?;
            if mount_id(&dir)? != mount_id(&root)? {
                return Err(PrefixError::Blocked.into());
            }
            let mut report = DeletionInspection {
                backup: backup.clone(),
                state: DeletionState::Unverifiable,
                remaining_entries: None,
                original_entries: None,
                detail: "Preserved for manual inspection; automatic deletion resume is unsupported"
                    .into(),
            };
            if names(&dir)?
                .iter()
                .any(|n| !["intent.json", "manifest.json", "payload"].contains(&n.as_str()))
            {
                report.state = DeletionState::Unsafe;
            } else if let Ok(m) = read_json::<BackupManifest>(&dir, "manifest.json") {
                if m.schema_version == 1
                    && m.id == backup
                    && m.source.validate().is_ok()
                    && m.source.owner_uid == crate::current_uid()
                    && total(&m.inventory).ok() == Some(m.payload_bytes)
                {
                    report.original_entries = Some(m.inventory.len());
                    if let Ok(payload) =
                        open_at(&dir, "payload", libc::O_RDONLY | libc::O_DIRECTORY, 0)
                    {
                        if mount_id(&payload)? != mount_id(&dir)? {
                            return Err(PrefixError::Blocked.into());
                        }
                        match scan(&payload, None) {
                            Ok(remaining) => {
                                report.remaining_entries = Some(remaining.len());
                                let subset = remaining.iter().all(|e| {
                                    m.inventory.iter().any(|old| {
                                        old.path == e.path
                                            && old.directory == e.directory
                                            && if e.directory {
                                                e.mode == old.mode || e.mode == old.mode | 0o700
                                            } else {
                                                old == e
                                            }
                                    })
                                });
                                report.state = if subset {
                                    DeletionState::Incomplete
                                } else {
                                    DeletionState::Unsafe
                                };
                            }
                            Err(_) => report.state = DeletionState::Unsafe,
                        }
                    }
                }
            }
            reports.push(report);
        }
        Ok(reports)
    }

    /// Deletion is journaled by moving the fully verified snapshot to deleting/.
    /// A partial unlink is retained there and requires inspection, never guessing.
    pub fn delete(
        &self,
        id: &BackupId,
        scope: &PrefixScope,
        confirmation: &str,
    ) -> Result<(), BackupError> {
        if confirmation != format!("delete:{}", id.as_str()) {
            return Err(BackupError::Confirmation);
        }
        let _guard = self.fs.lock_registry()?;
        let m = self.verify(id, scope)?;
        if let Some(pins) = self.sub("pin-recovery", false)? {
            if names(&pins)?
                .iter()
                .any(|n| n.starts_with(&format!("{}-", id.as_str())))
            {
                return Err(BackupError::RecoveryBlocked);
            }
        }
        if self.transaction_dir(&m.source.id, false)?.is_some() {
            return Err(BackupError::RecoveryBlocked);
        }
        let snapshots = self.sub("snapshots", false)?.ok_or(PrefixError::Missing)?;
        let deleting = self.sub("deleting", true)?.ok_or(PrefixError::Blocked)?;
        let dir = self.snapshot(id)?;
        if names(&dir)? != ["intent.json", "manifest.json", "payload"] {
            return Err(BackupError::RecoveryBlocked);
        }
        let expected = identity(&dir)?;
        rename(
            &snapshots,
            id.as_str(),
            &deleting,
            id.as_str(),
            libc::RENAME_NOREPLACE,
        )?;
        self.checkpoint("delete-retired")?;
        let retired = open_at(
            &deleting,
            id.as_str(),
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?;
        if identity(&retired)? != expected {
            return Err(BackupError::RecoveryBlocked);
        }
        let payload = open_at(&retired, "payload", libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        if scan(&payload, None)? != m.inventory {
            return Err(BackupError::Integrity(BackupIntegrity::Corrupt));
        }
        delete_verified_tree(&payload, mount_id(&retired)?, 0, "", &m.inventory)?;
        unlink_at(&retired, "payload", libc::AT_REMOVEDIR)?;
        for name in ["intent.json", "manifest.json"] {
            let f = open_at(&retired, name, libc::O_RDONLY, 0)?;
            verify_private_file(&f)?;
            if identity(&f)? != identity(&open_at(&retired, name, libc::O_RDONLY, 0)?)? {
                return Err(BackupError::RecoveryBlocked);
            }
            unlink_at(&retired, name, 0)?;
        }
        retired.sync_all()?;
        if identity(&open_at(
            &deleting,
            id.as_str(),
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?)? != expected
        {
            return Err(BackupError::RecoveryBlocked);
        }
        unlink_at(&deleting, id.as_str(), libc::AT_REMOVEDIR)?;
        deleting.sync_all()?;
        Ok(())
    }

    pub fn restore_plan(
        &self,
        id: &PrefixId,
        backup: &BackupId,
        scope: &PrefixScope,
    ) -> Result<RestorePlan, BackupError> {
        self.ensure_no_restore(id)?;
        let m = self.verify(backup, scope)?;
        let p = self.plan(id, scope)?;
        if m.source.id != *id {
            return Err(PrefixError::IdentityConflict.into());
        }
        if p.available
            < m.payload_bytes
                .checked_add(RESERVE)
                .ok_or(BackupError::Quota)?
        {
            return Err(BackupError::Quota);
        }
        Ok(RestorePlan {
            backup: backup.clone(),
            integrity: BackupIntegrity::Verified,
            backup_runtime: m.source.runtime,
            original_lifecycle: m.source.lifecycle,
            current_selection_status:
                "unverifiable until a P027 snapshot and runtime validator are supplied".into(),
            current_selection: None,
            saved_adapter_status: "not assessed; no execution authorization".into(),
            recovery_strategy:
                "same-mount exchange; preserve original payload; journaled registry finalization"
                    .into(),
            destination: p.source,
            registry_revision: p.registry_revision,
            bytes: m.payload_bytes,
            replaces_existing_data: true,
            confirmation: format!("restore:{}:{}", id.as_str(), backup.as_str()),
        })
    }
    fn transaction_dir(&self, id: &PrefixId, create: bool) -> Result<Option<File>, BackupError> {
        let Some(root) = self.sub("restores", create)? else {
            return Ok(None);
        };
        let d = child_dir(&root, OsStr::new(id.as_str()), false)?;
        if let Some(d) = &d {
            verify_private_dir(d)?;
            if mount_id(d)? != mount_id(&root)? {
                return Err(PrefixError::Blocked.into());
            }
        }
        Ok(d)
    }
    fn ensure_no_restore(&self, id: &PrefixId) -> Result<(), BackupError> {
        if self.transaction_dir(id, false)?.is_some() {
            return Err(BackupError::RecoveryBlocked);
        }
        Ok(())
    }
    pub fn restore(
        &self,
        id: &PrefixId,
        backup: &BackupId,
        scope: &PrefixScope,
        expected: u64,
        confirmation: &str,
    ) -> Result<RecoveryReport, BackupError> {
        let _rg = self.fs.lock_registry()?;
        let _pg = self.fs.lock_prefix(id)?;
        let plan = self.restore_plan(id, backup, scope)?;
        if confirmation != plan.confirmation {
            return Err(BackupError::Confirmation);
        }
        if plan.registry_revision != expected {
            return Err(PrefixError::RevisionConflict.into());
        }
        let roots = self.sub("restores", true)?.ok_or(PrefixError::Blocked)?;
        let tx = exclusive_dir(&roots, id.as_str())?;
        write_json(&tx, "intent.json", &plan.destination)?;
        self.checkpoint("restore-intent")?;
        let original_inventory = scan(&self.source(id)?, None)?;
        let payload = exclusive_dir(&tx, "payload")?;
        let snap = self.snapshot(backup)?;
        let src = open_at(&snap, "payload", libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        let m = self.verify(backup, scope)?;
        if scan(&src, Some(&payload))? != m.inventory || scan(&payload, None)? != m.inventory {
            return Err(BackupError::Integrity(BackupIntegrity::Corrupt));
        }
        let journal = RestoreTransaction {
            schema_version: 2,
            backup: backup.clone(),
            original: plan.destination,
            expected_revision: expected,
            staged: identity(&payload)?,
            original_inventory: Some(original_inventory),
        };
        write_json(&tx, "staged.json", &journal)?;
        self.checkpoint("restore-staged")?;
        self.inactive(id)?;
        let (current, revision) = self.record(id)?;
        if current != journal.original || revision != expected {
            return Err(PrefixError::RevisionConflict.into());
        }
        if scan(&self.source(id)?, None)?
            != *journal
                .original_inventory
                .as_ref()
                .ok_or(BackupError::RecoveryBlocked)?
        {
            return Err(BackupError::RecoveryBlocked);
        }
        let l = self.fs.layout(false)?.ok_or(PrefixError::Blocked)?;
        if mount_id(&payload)? != l.root_mount {
            return Err(PrefixError::Blocked.into());
        }
        rename(&tx, "payload", &l.data, id.as_str(), libc::RENAME_EXCHANGE)?;
        self.checkpoint("restore-published")?;
        self.finalize(id, &tx, &journal, scope)
    }
    fn validate_restore_journal(
        &self,
        id: &PrefixId,
        tx: &File,
        j: &RestoreTransaction,
    ) -> Result<(), BackupError> {
        j.original.validate()?;
        if !matches!(j.schema_version, 1 | 2)
            || (j.schema_version == 1 && j.original_inventory.is_some())
            || (j.schema_version == 2 && j.original_inventory.is_none())
            || j.original_inventory
                .as_ref()
                .is_some_and(|entries| total(entries).is_err())
            || j.original.id != *id
            || j.original.owner_uid != crate::current_uid()
            || j.original.lifecycle != PrefixLifecycleState::Created
            || j.expected_revision < j.original.generation
            || Some(j.staged) == j.original.filesystem_identity
            || j.original
                .filesystem_identity
                .is_none_or(|old| old.device != j.staged.device)
            || j.expected_revision.checked_add(1).is_none()
        {
            return Err(BackupError::RecoveryBlocked);
        }
        let intent: PrefixRecord = read_json(tx, "intent.json")?;
        if intent != j.original
            || names(tx)?.iter().any(|name| {
                ![
                    "intent.json",
                    "staged.json",
                    "payload",
                    "completed.json",
                    "rollback-intent.json",
                    "rollback-completed.json",
                ]
                .contains(&name.as_str())
            })
        {
            return Err(BackupError::RecoveryBlocked);
        }
        if (has_file(tx, "rollback-completed.json")? && !has_file(tx, "rollback-intent.json")?)
            || (has_file(tx, "rollback-intent.json")? && has_file(tx, "completed.json")?)
        {
            return Err(BackupError::RecoveryBlocked);
        }
        if has_file(tx, "completed.json")? {
            let completed: FilesystemIdentity = read_json(tx, "completed.json")?;
            if completed != j.staged {
                return Err(BackupError::RecoveryBlocked);
            }
        }
        Ok(())
    }

    fn finalize(
        &self,
        id: &PrefixId,
        tx: &File,
        journal: &RestoreTransaction,
        scope: &PrefixScope,
    ) -> Result<RecoveryReport, BackupError> {
        self.validate_restore_journal(id, tx, journal)?;
        if has_file(tx, "rollback-intent.json")? {
            return Err(BackupError::RecoveryBlocked);
        }
        if !matches!(journal.schema_version, 1 | 2)
            || journal.original.id != *id
            || &journal.original.scope != scope
        {
            return Err(BackupError::RecoveryBlocked);
        }
        journal.original.validate()?;
        self.inactive(id)?;
        let preserved = open_at(tx, "payload", libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        if Some(identity(&preserved)?) != journal.original.filesystem_identity
            || self.fs.inspect_directory(id)? != Some(journal.staged)
        {
            return Err(BackupError::RecoveryBlocked);
        }
        if let Some(expected) = &journal.original_inventory {
            if scan(&preserved, None)? != *expected {
                return Err(BackupError::RecoveryBlocked);
            }
        }
        let m = self.verify(&journal.backup, scope)?;
        if m.source.id != *id || scan(&self.source(id)?, None)? != m.inventory {
            return Err(BackupError::RecoveryBlocked);
        }
        let snap = self.prefixes.load_snapshot()?;
        if has_file(tx, "completed.json")? && snap.registry.revision == journal.expected_revision {
            return Err(BackupError::RecoveryBlocked);
        }
        let mut replacement = m.source;
        replacement.filesystem_identity = Some(journal.staged);
        replacement.generation = journal
            .expected_revision
            .checked_add(1)
            .ok_or(BackupError::RecoveryBlocked)?;
        replacement.update_time_unix_seconds = crate::now();
        if snap.registry.revision == journal.expected_revision
            && snap.registry.record(id) == Some(&journal.original)
        {
            let mut reg = snap.registry;
            *reg.records
                .iter_mut()
                .find(|r| r.id == *id)
                .ok_or(PrefixError::NotRegistered)? = replacement.clone();
            reg.revision = replacement.generation;
            self.fs
                .save_registry_bytes(snap.bytes.as_deref(), &reg.encode()?)?;
            self.checkpoint("restore-registry")?;
        } else {
            let actual = snap
                .registry
                .record(id)
                .ok_or(BackupError::RecoveryBlocked)?;
            // Timestamp is assigned at the original commit, not at recovery.
            replacement.update_time_unix_seconds = actual.update_time_unix_seconds;
            if snap.registry.revision != replacement.generation || actual != &replacement {
                return Err(BackupError::RecoveryBlocked);
            }
        }
        if has_file(tx, "completed.json")? {
            let completed: FilesystemIdentity = read_json(tx, "completed.json")?;
            if completed != journal.staged {
                return Err(BackupError::RecoveryBlocked);
            }
        } else {
            write_json(tx, "completed.json", &journal.staged)?;
        }
        Ok(RecoveryReport {
            action: RecoveryAction::None,
            original_preserved: true,
        })
    }
    /// Separately approved P027 mutation. Prefix restoration never calls this.
    pub fn restore_pin(
        &self,
        backup: &BackupId,
        authorized_scope: &PrefixScope,
        pins: &endfield_unix_runtime_selection::PinStore,
        expected: u64,
        confirmation: &str,
        mut validate: impl FnMut(&crate::PrefixRuntimeAssociation) -> Result<(), String>,
    ) -> Result<u64, BackupError> {
        use endfield_unix_runtime_selection::{RuntimePin, RuntimePinScope};
        if confirmation != format!("pin:{}", backup.as_str()) {
            return Err(BackupError::Confirmation);
        }
        let _guard = self.fs.lock_registry()?;
        let m = self.verify(backup, authorized_scope)?;
        validate(&m.source.runtime).map_err(|_| BackupError::RuntimeBlocked)?;
        let machine = match authorized_scope {
            PrefixScope::Machine { machine } | PrefixScope::Profile { machine, .. } => {
                machine.clone()
            }
        };
        let scope = match (&m.source.runtime.selection_source, authorized_scope) {
            (crate::RuntimeSelectionSource::MachinePin, _) => RuntimePinScope::Machine { machine },
            (crate::RuntimeSelectionSource::ProfilePin, PrefixScope::Profile { profile, .. }) => {
                RuntimePinScope::Profile {
                    machine,
                    profile: profile.clone(),
                }
            }
            _ => return Err(PrefixError::InvalidScope.into()),
        };
        let root = self
            .sub("pin-recovery", true)?
            .ok_or(PrefixError::Blocked)?;
        let name = format!("{}-{expected}", backup.as_str());
        let tx = match child_dir(&root, OsStr::new(&name), false)? {
            Some(tx) => {
                verify_private_dir(&tx)?;
                tx
            }
            None => exclusive_dir(&root, &name)?,
        };
        if mount_id(&tx)? != mount_id(&root)? {
            return Err(PrefixError::Blocked.into());
        }
        let desired = RuntimePin::new(
            m.source.runtime.pin_id.clone(),
            scope,
            m.source.runtime.runtime.clone(),
            m.source.runtime.adapter_preference,
            crate::now(),
        )
        .map_err(|_| BackupError::RuntimeBlocked)?;
        let desired: RuntimePin = if has_file(&tx, "intent.json")? {
            let saved: RuntimePin = read_json(&tx, "intent.json")?;
            if saved.scope != desired.scope
                || saved.runtime != desired.runtime
                || saved.adapter_preference != desired.adapter_preference
                || saved.pin_id != desired.pin_id
            {
                return Err(BackupError::RecoveryBlocked);
            }
            saved
        } else {
            write_json(&tx, "intent.json", &desired)?;
            desired
        };
        self.checkpoint("pin-intent")?;
        let completion = if has_file(&tx, "completed.json")? {
            Some(read_json::<u64>(&tx, "completed.json")?)
        } else {
            None
        };
        if completion.is_some_and(|r| Some(r) != expected.checked_add(1)) {
            return Err(BackupError::RecoveryBlocked);
        }
        let current = pins.load().map_err(|_| BackupError::RuntimeBlocked)?;
        if completion.is_some() && current.revision == expected {
            return Err(BackupError::RecoveryBlocked);
        }
        let resulting_revision = expected
            .checked_add(1)
            .ok_or(BackupError::RecoveryBlocked)?;
        if current.revision == expected {
            validate(&m.source.runtime).map_err(|_| BackupError::RuntimeBlocked)?;
            pins.set(desired.clone(), expected, true)
                .map_err(|_| BackupError::RuntimeBlocked)?;
            self.checkpoint("pin-committed")?;
        } else {
            let pin = current
                .pin(&desired.scope)
                .ok_or(BackupError::RecoveryBlocked)?;
            if current.revision != resulting_revision
                || pin.runtime != desired.runtime
                || pin.adapter_preference != desired.adapter_preference
                || pin.pin_id != desired.pin_id
            {
                return Err(BackupError::RecoveryBlocked);
            }
        }
        if has_file(&tx, "completed.json")? {
            let completed: u64 = read_json(&tx, "completed.json")?;
            if completed != resulting_revision {
                return Err(BackupError::RecoveryBlocked);
            }
        } else {
            write_json(&tx, "completed.json", &resulting_revision)?;
        }
        Ok(resulting_revision)
    }

    /// Rollback is limited to a published restore whose registry has not yet
    /// committed. Both directory trees are retained and checked on every replay.
    pub fn rollback_plan(
        &self,
        id: &PrefixId,
        scope: &PrefixScope,
    ) -> Result<RollbackPlan, BackupError> {
        let tx = self
            .transaction_dir(id, false)?
            .ok_or(BackupError::RecoveryBlocked)?;
        let j: RestoreTransaction = read_json(&tx, "staged.json")?;
        self.validate_restore_journal(id, &tx, &j)?;
        if j.schema_version != 2 || &j.original.scope != scope || has_file(&tx, "completed.json")? {
            return Err(BackupError::RecoveryBlocked);
        }
        let original = j
            .original_inventory
            .as_ref()
            .ok_or(BackupError::RecoveryBlocked)?;
        let registry = self.prefixes.load_snapshot()?.registry;
        if registry.revision != j.expected_revision || registry.record(id) != Some(&j.original) {
            return Err(BackupError::RecoveryBlocked);
        }
        self.inactive(id)?;
        let m = self.verify(&j.backup, scope)?;
        if m.source.id != *id {
            return Err(BackupError::RecoveryBlocked);
        }
        let live = self.source(id)?;
        let saved = open_at(&tx, "payload", libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        let live_id = identity(&live)?;
        let saved_id = identity(&saved)?;
        let old = j
            .original
            .filesystem_identity
            .ok_or(BackupError::RecoveryBlocked)?;
        let exchanged = if live_id == j.staged && saved_id == old {
            if scan(&live, None)? != m.inventory || scan(&saved, None)? != *original {
                return Err(BackupError::RecoveryBlocked);
            }
            false
        } else if live_id == old && saved_id == j.staged && has_file(&tx, "rollback-intent.json")? {
            if scan(&live, None)? != *original || scan(&saved, None)? != m.inventory {
                return Err(BackupError::RecoveryBlocked);
            }
            true
        } else {
            return Err(BackupError::RecoveryBlocked);
        };
        let confirmation = format!(
            "rollback:{}:{}:{}:{}",
            id.as_str(),
            j.backup.as_str(),
            old.inode,
            j.staged.inode
        );
        if has_file(&tx, "rollback-intent.json")? {
            let recorded: String = read_json(&tx, "rollback-intent.json")?;
            if recorded != confirmation {
                return Err(BackupError::RecoveryBlocked);
            }
        }
        if has_file(&tx, "rollback-completed.json")? {
            let completed: String = read_json(&tx, "rollback-completed.json")?;
            if completed != confirmation || !exchanged {
                return Err(BackupError::RecoveryBlocked);
            }
        }
        Ok(RollbackPlan {
            prefix: id.clone(),
            backup: j.backup,
            expected_revision: j.expected_revision,
            confirmation,
            already_exchanged: exchanged,
        })
    }
    pub fn rollback(
        &self,
        id: &PrefixId,
        scope: &PrefixScope,
        expected: u64,
        confirmation: &str,
    ) -> Result<RollbackPlan, BackupError> {
        let _rg = self.fs.lock_registry()?;
        let _pg = self.fs.lock_prefix(id)?;
        let plan = self.rollback_plan(id, scope)?;
        if plan.expected_revision != expected {
            return Err(PrefixError::RevisionConflict.into());
        }
        if confirmation != plan.confirmation {
            return Err(BackupError::Confirmation);
        }
        let tx = self
            .transaction_dir(id, false)?
            .ok_or(BackupError::RecoveryBlocked)?;
        if !has_file(&tx, "rollback-intent.json")? {
            write_json(&tx, "rollback-intent.json", &plan.confirmation)?;
        }
        self.checkpoint("rollback-intent")?;
        if !plan.already_exchanged {
            // Re-evaluate after the durable intent, immediately before exchange.
            let fresh = self.rollback_plan(id, scope)?;
            if fresh.already_exchanged || fresh.confirmation != plan.confirmation {
                return Err(BackupError::RecoveryBlocked);
            }
            let layout = self.fs.layout(false)?.ok_or(PrefixError::Blocked)?;
            if mount_id(&tx)? != layout.root_mount {
                return Err(PrefixError::Blocked.into());
            }
            rename(
                &tx,
                "payload",
                &layout.data,
                id.as_str(),
                libc::RENAME_EXCHANGE,
            )?;
            self.checkpoint("rollback-published")?;
        }
        let result = self.rollback_plan(id, scope)?;
        if !result.already_exchanged {
            return Err(BackupError::RecoveryBlocked);
        }
        if !has_file(&tx, "rollback-completed.json")? {
            write_json(&tx, "rollback-completed.json", &result.confirmation)?;
        }
        self.checkpoint("rollback-completed")?;
        Ok(result)
    }

    /// Archive completed evidence and preserved bytes without deleting them.
    pub fn release(
        &self,
        id: &PrefixId,
        scope: &PrefixScope,
        expected: u64,
        confirmation: &str,
    ) -> Result<(), BackupError> {
        if confirmation != format!("release:{}", id.as_str()) {
            return Err(BackupError::Confirmation);
        }
        let _rg = self.fs.lock_registry()?;
        let _pg = self.fs.lock_prefix(id)?;
        if self.prefixes.load_snapshot()?.registry.revision != expected {
            return Err(PrefixError::RevisionConflict.into());
        }
        let tx = self
            .transaction_dir(id, false)?
            .ok_or(BackupError::RecoveryBlocked)?;
        let j: RestoreTransaction = read_json(&tx, "staged.json")?;
        if has_file(&tx, "rollback-completed.json")? {
            let plan = self.rollback_plan(id, scope)?;
            if !plan.already_exchanged {
                return Err(BackupError::RecoveryBlocked);
            }
        } else {
            if !has_file(&tx, "completed.json")? {
                return Err(BackupError::RecoveryBlocked);
            }
            self.finalize(id, &tx, &j, scope)?;
        }
        let roots = self.sub("restores", false)?.ok_or(PrefixError::Missing)?;
        let history = self.sub("history", true)?.ok_or(PrefixError::Blocked)?;
        rename(
            &roots,
            id.as_str(),
            &history,
            &format!("{}-{}", id.as_str(), j.staged.inode),
            libc::RENAME_NOREPLACE,
        )?;
        Ok(())
    }

    pub fn recovery_status(&self, id: &PrefixId) -> Result<RecoveryReport, BackupError> {
        let Some(tx) = self.transaction_dir(id, false)? else {
            return Ok(RecoveryReport {
                action: RecoveryAction::None,
                original_preserved: false,
            });
        };
        let journal: RestoreTransaction = match read_json(&tx, "staged.json") {
            Ok(j) => j,
            Err(_) => {
                return Ok(RecoveryReport {
                    action: RecoveryAction::InspectPreservedStaging,
                    original_preserved: false,
                });
            }
        };
        if !matches!(journal.schema_version, 1 | 2) || journal.original.id != *id {
            return Err(BackupError::RecoveryBlocked);
        }
        self.validate_restore_journal(id, &tx, &journal)?;
        journal.original.validate()?;
        if has_file(&tx, "rollback-intent.json")? {
            let plan = self.rollback_plan(id, &journal.original.scope)?;
            return Ok(RecoveryReport {
                action: if has_file(&tx, "rollback-completed.json")? {
                    RecoveryAction::RollbackCompleted
                } else {
                    RecoveryAction::RollbackInProgress
                },
                original_preserved: plan.already_exchanged
                    || self.fs.inspect_directory(id)? == Some(journal.staged),
            });
        }
        let live = self.fs.inspect_directory(id)?;
        let payload = open_at(&tx, "payload", libc::O_RDONLY | libc::O_DIRECTORY, 0).ok();
        let payload_identity = payload.as_ref().map(identity).transpose()?;
        let original_preserved = live == journal.original.filesystem_identity
            || payload_identity == journal.original.filesystem_identity;
        let action = if live == Some(journal.staged)
            && payload_identity == journal.original.filesystem_identity
        {
            if has_file(&tx, "completed.json")? {
                let completed: FilesystemIdentity = read_json(&tx, "completed.json")?;
                if completed != journal.staged {
                    return Err(BackupError::RecoveryBlocked);
                }
                let snapshot = self.prefixes.load_snapshot()?;
                let record = snapshot
                    .registry
                    .record(id)
                    .ok_or(BackupError::RecoveryBlocked)?;
                if snapshot.registry.revision != journal.expected_revision + 1
                    || record.generation != journal.expected_revision + 1
                    || record.filesystem_identity != Some(journal.staged)
                    || record.lifecycle != PrefixLifecycleState::Created
                {
                    return Err(BackupError::RecoveryBlocked);
                }
                RecoveryAction::None
            } else {
                RecoveryAction::FinalizePublishedRestore
            }
        } else if live == journal.original.filesystem_identity
            && payload_identity == Some(journal.staged)
        {
            RecoveryAction::ResumeStagedRestore
        } else {
            RecoveryAction::InspectPreservedStaging
        };
        Ok(RecoveryReport {
            action,
            original_preserved,
        })
    }

    pub fn recover(
        &self,
        id: &PrefixId,
        scope: &PrefixScope,
        expected: u64,
        confirmed: bool,
    ) -> Result<RecoveryReport, BackupError> {
        if !confirmed {
            return Err(BackupError::Confirmation);
        }
        let _rg = self.fs.lock_registry()?;
        let _pg = self.fs.lock_prefix(id)?;
        if self.prefixes.load_snapshot()?.registry.revision != expected {
            return Err(PrefixError::RevisionConflict.into());
        }
        let tx = self
            .transaction_dir(id, false)?
            .ok_or(BackupError::RecoveryBlocked)?;
        let journal: RestoreTransaction = read_json(&tx, "staged.json")?;
        self.validate_restore_journal(id, &tx, &journal)?;
        if has_file(&tx, "rollback-intent.json")? {
            return Err(BackupError::RecoveryBlocked);
        }
        if has_file(&tx, "completed.json")?
            && self.fs.inspect_directory(id)? != Some(journal.staged)
        {
            return Err(BackupError::RecoveryBlocked);
        }
        if !matches!(journal.schema_version, 1 | 2)
            || journal.original.id != *id
            || &journal.original.scope != scope
        {
            return Err(BackupError::RecoveryBlocked);
        }
        if self.fs.inspect_directory(id)? == journal.original.filesystem_identity {
            journal.original.validate()?;
            let snap = self.prefixes.load_snapshot()?;
            if snap.registry.revision != journal.expected_revision
                || snap.registry.record(id) != Some(&journal.original)
            {
                return Err(BackupError::RecoveryBlocked);
            }
            let m = self.verify(&journal.backup, scope)?;
            let payload = open_at(&tx, "payload", libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
            if m.source.id != *id
                || identity(&payload)? != journal.staged
                || scan(&payload, None)? != m.inventory
            {
                return Err(BackupError::RecoveryBlocked);
            }
            self.inactive(id)?;
            if let Some(expected) = &journal.original_inventory {
                if scan(&self.source(id)?, None)? != *expected {
                    return Err(BackupError::RecoveryBlocked);
                }
            }
            let l = self.fs.layout(false)?.ok_or(PrefixError::Blocked)?;
            if mount_id(&payload)? != l.root_mount {
                return Err(PrefixError::Blocked.into());
            }
            rename(&tx, "payload", &l.data, id.as_str(), libc::RENAME_EXCHANGE)?;
        }
        self.finalize(id, &tx, &journal, scope)
    }
}

fn delete_verified_tree(
    dir: &File,
    mount: u64,
    depth: usize,
    parent: &str,
    inventory: &[InventoryEntry],
) -> Result<(), BackupError> {
    if depth > 64 || mount_id(dir)? != mount {
        return Err(BackupError::RecoveryBlocked);
    }
    for name in names(dir)? {
        let mut f = open_at(dir, &name, libc::O_RDONLY, 0)?;
        let m = f.metadata()?;
        let path = if parent.is_empty() {
            name.clone()
        } else {
            format!("{parent}/{name}")
        };
        let expected = inventory
            .iter()
            .find(|e| e.path == path)
            .ok_or(BackupError::RecoveryBlocked)?;
        if expected.directory != m.is_dir()
            || expected.mode != m.mode() & 0o777
            || expected.modified_seconds != m.mtime()
            || expected.modified_nanoseconds != m.mtime_nsec()
        {
            return Err(BackupError::RecoveryBlocked);
        }
        if m.uid() != crate::current_uid() || mount_id(&f)? != mount {
            return Err(BackupError::RecoveryBlocked);
        }
        let flags = if m.is_dir() {
            // Retirement already durably records deletion intent. Grant only
            // owner traversal/write rights to remove an otherwise readonly tree.
            f.set_permissions(fs::Permissions::from_mode((m.mode() & 0o777) | 0o700))?;
            delete_verified_tree(&f, mount, depth + 1, &path, inventory)?;
            libc::AT_REMOVEDIR
        } else if m.is_file() && m.nlink() == 1 {
            if m.len() != expected.size {
                return Err(BackupError::RecoveryBlocked);
            }
            let mut hash = Sha256::new();
            let mut remaining = m.len();
            let mut buffer = [0u8; 65536];
            loop {
                let n = f.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                remaining = remaining
                    .checked_sub(n as u64)
                    .ok_or(BackupError::RecoveryBlocked)?;
                hash.update(&buffer[..n]);
            }
            if remaining != 0
                || format!("{:x}", hash.finalize()) != expected.sha256
                || !unchanged(&m, &f.metadata()?)
            {
                return Err(BackupError::RecoveryBlocked);
            }
            0
        } else {
            return Err(BackupError::RecoveryBlocked);
        };
        if identity(&f)? != identity(&open_at(dir, &name, libc::O_RDONLY, 0)?)? {
            return Err(BackupError::RecoveryBlocked);
        }
        unlink_at(dir, &name, flags)?;
        dir.sync_all()?;
        #[cfg(test)]
        if std::env::var("ENDFIELD_P029_EXIT_AT").ok().as_deref() == Some("delete-unlink") {
            unsafe { libc::_exit(91) }
        }
    }
    Ok(())
}

fn identity(f: &File) -> Result<FilesystemIdentity, BackupError> {
    let m = f.metadata()?;
    Ok(FilesystemIdentity {
        device: m.dev(),
        inode: m.ino(),
    })
}
fn total(entries: &[InventoryEntry]) -> Result<u64, BackupError> {
    if entries.len() > MAX_ENTRIES {
        return Err(BackupError::Quota);
    }
    let n = entries
        .iter()
        .try_fold(0u64, |n, e| n.checked_add(e.size).ok_or(BackupError::Quota))?;
    if n > MAX_BYTES {
        return Err(BackupError::Quota);
    }
    Ok(n)
}
fn names(d: &File) -> Result<Vec<String>, BackupError> {
    let mut names = Vec::new();
    for entry in fs::read_dir(format!("/proc/self/fd/{}", d.as_raw_fd()))? {
        let name = entry?
            .file_name()
            .into_string()
            .map_err(|_| BackupError::Unsupported)?;
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(BackupError::Unsupported);
        }
        names.push(name);
        if names.len() > MAX_ENTRIES {
            return Err(BackupError::Quota);
        }
    }
    names.sort();
    Ok(names)
}
fn scan(root: &File, destination: Option<&File>) -> Result<Vec<InventoryEntry>, BackupError> {
    let mut out = vec![];
    let mount = mount_id(root)?;
    walk(root, destination, "", 0, mount, &mut out)?;
    total(&out)?;
    Ok(out)
}
fn walk(
    dir: &File,
    dest: Option<&File>,
    parent: &str,
    depth: usize,
    mount: u64,
    out: &mut Vec<InventoryEntry>,
) -> Result<(), BackupError> {
    if depth > 64 {
        return Err(BackupError::Quota);
    }
    let before = dir.metadata()?;
    for name in names(dir)? {
        if out.len() >= MAX_ENTRIES {
            return Err(BackupError::Quota);
        }
        let path = if parent.is_empty() {
            name.clone()
        } else {
            format!("{parent}/{name}")
        };
        if path.len() > 4096 {
            return Err(BackupError::Quota);
        }
        let mut f = open_at(dir, &name, libc::O_RDONLY, 0)?;
        let m = f.metadata()?;
        if m.uid() != crate::current_uid() || m.mode() & 0o7000 != 0 || mount_id(&f)? != mount {
            return Err(BackupError::Unsupported);
        }
        let attrs = unsafe { libc::flistxattr(f.as_raw_fd(), std::ptr::null_mut(), 0) };
        if attrs != 0 {
            return Err(BackupError::Unsupported);
        }
        let mode = m.mode() & 0o777;
        if m.is_dir() {
            out.push(InventoryEntry {
                path: path.clone(),
                directory: true,
                size: 0,
                mode,
                sha256: String::new(),
                modified_seconds: m.mtime(),
                modified_nanoseconds: m.mtime_nsec(),
            });
            let target = dest.map(|d| exclusive_dir(d, &name)).transpose()?;
            walk(&f, target.as_ref(), &path, depth + 1, mount, out)?;
            if let Some(target) = target {
                target.set_permissions(fs::Permissions::from_mode(mode))?;
                preserve_mtime(&target, &m)?;
                target.sync_all()?;
            }
        } else if m.is_file()
            && m.nlink() == 1
            && (m.len() == 0 || m.blocks().saturating_mul(512) >= m.len())
        {
            if total(out)?
                .checked_add(m.len())
                .is_none_or(|n| n > MAX_BYTES)
            {
                return Err(BackupError::Quota);
            }
            let mut target = dest
                .map(|d| {
                    open_at(
                        d,
                        &name,
                        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                        0o600,
                    )
                })
                .transpose()?;
            let mut hash = Sha256::new();
            let mut count = 0u64;
            let mut buffer = [0u8; 65536];
            loop {
                let n = f.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                count = count.checked_add(n as u64).ok_or(BackupError::Quota)?;
                if count > m.len() {
                    return Err(BackupError::Integrity(BackupIntegrity::Unverifiable));
                }
                hash.update(&buffer[..n]);
                if let Some(t) = &mut target {
                    t.write_all(&buffer[..n])?;
                    #[cfg(test)]
                    if std::env::var("ENDFIELD_P029_EXIT_AT").ok().as_deref() == Some("copy-chunk")
                    {
                        unsafe { libc::_exit(91) }
                    }
                }
            }
            let after = f.metadata()?;
            if count != m.len() || !unchanged(&m, &after) {
                return Err(BackupError::Integrity(BackupIntegrity::Unverifiable));
            }
            if let Some(t) = target {
                t.set_permissions(fs::Permissions::from_mode(mode))?;
                preserve_mtime(&t, &m)?;
                t.sync_all()?;
            }
            out.push(InventoryEntry {
                path,
                directory: false,
                size: count,
                mode,
                sha256: format!("{:x}", hash.finalize()),
                modified_seconds: m.mtime(),
                modified_nanoseconds: m.mtime_nsec(),
            });
            total(out)?;
        } else {
            return Err(BackupError::Unsupported);
        }
        // Reopen through the parent after processing to detect replacement.
        if identity(&open_at(dir, &name, libc::O_RDONLY, 0)?)? != identity(&f)? {
            return Err(PrefixError::Blocked.into());
        }
    }
    if !unchanged(&before, &dir.metadata()?) {
        return Err(BackupError::Integrity(BackupIntegrity::Unverifiable));
    }
    if let Some(d) = dest {
        d.sync_all()?;
    }
    Ok(())
}
fn preserve_mtime(file: &File, m: &fs::Metadata) -> Result<(), BackupError> {
    let times = [
        libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_OMIT,
        },
        libc::timespec {
            tv_sec: m.mtime(),
            tv_nsec: m.mtime_nsec(),
        },
    ];
    if unsafe { libc::futimens(file.as_raw_fd(), times.as_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
fn unchanged(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.len() == b.len()
        && a.mode() == b.mode()
        && a.nlink() == b.nlink()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}
fn exclusive_dir(parent: &File, name: &str) -> Result<File, BackupError> {
    let c = cstring(name)?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), c.as_ptr(), 0o700) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    parent.sync_all()?;
    Ok(open_at(
        parent,
        name,
        libc::O_RDONLY | libc::O_DIRECTORY,
        0,
    )?)
}
fn write_json(parent: &File, name: &str, value: &impl Serialize) -> Result<(), BackupError> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| BackupError::Integrity(BackupIntegrity::Corrupt))?;
    if bytes.len() > MAX_METADATA {
        return Err(BackupError::Quota);
    }
    let mut f = open_at(
        parent,
        name,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    parent.sync_all()?;
    Ok(())
}
fn read_json<T: serde::de::DeserializeOwned>(parent: &File, name: &str) -> Result<T, BackupError> {
    let f = open_at(parent, name, libc::O_RDONLY, 0)?;
    verify_private_file(&f)?;
    serde_json::from_slice(&read_bounded(f, MAX_METADATA)?)
        .map_err(|_| BackupError::Integrity(BackupIntegrity::Corrupt))
}
fn has_file(parent: &File, name: &str) -> Result<bool, BackupError> {
    match open_at(parent, name, libc::O_RDONLY, 0) {
        Ok(f) => {
            verify_private_file(&f)?;
            Ok(true)
        }
        Err(PrefixError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn rename(from: &File, a: &str, to: &File, b: &str, flags: u32) -> Result<(), BackupError> {
    let a = cstring(a)?;
    let b = cstring(b)?;
    if unsafe {
        libc::renameat2(
            from.as_raw_fd(),
            a.as_ptr(),
            to.as_raw_fd(),
            b.as_ptr(),
            flags,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    from.sync_all()?;
    to.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    pub(super) fn fixture() -> (tempfile::TempDir, BackupStore, PrefixId, PrefixScope, u64) {
        let (tmp, prefixes, scope, runtime) = crate::tests::fixture();
        let id = PrefixId::new("game").unwrap();
        let (_, rev) = prefixes
            .create(id.clone(), scope.clone(), runtime, 0)
            .unwrap();
        let fs = LinuxPrefixFilesystem::new(prefixes.fs.root_path().parent().unwrap());
        let b = BackupStore {
            fs,
            prefixes,
            usage_override: Some(Some(false)),
            fault: None,
        };
        (tmp, b, id, scope, rev)
    }
    #[test]
    fn empty_and_nested_binary_roundtrip_preserves_original_and_runtime() {
        let (_tmp, b, id, scope, rev) = fixture();
        let path = b.fs.target_path(&id);
        let empty = b.create(&id, &scope, rev, true).unwrap();
        assert!(empty.inventory.is_empty());
        fs::create_dir(path.join("nested")).unwrap();
        fs::write(path.join("nested/.binary"), b"\0\xff\x01payload").unwrap();
        let snapshot = b.create(&id, &scope, rev, true).unwrap();
        assert_ne!(empty.id, snapshot.id);
        let manifest_bytes = fs::read(
            b.fs.data_dir
                .join("backups/snapshots")
                .join(snapshot.id.as_str())
                .join("manifest.json"),
        )
        .unwrap();
        b.verify(&snapshot.id, &scope).unwrap();
        b.verify(&snapshot.id, &scope).unwrap();
        assert_eq!(
            manifest_bytes,
            fs::read(
                b.fs.data_dir
                    .join("backups/snapshots")
                    .join(snapshot.id.as_str())
                    .join("manifest.json")
            )
            .unwrap()
        );
        fs::write(path.join("new-save"), b"preserve me").unwrap();
        let p = b.restore_plan(&id, &snapshot.id, &scope).unwrap();
        b.restore(&id, &snapshot.id, &scope, rev, &p.confirmation)
            .unwrap();
        assert_eq!(
            fs::read(path.join("nested/.binary")).unwrap(),
            b"\0\xff\x01payload"
        );
        assert!(!path.join("new-save").exists());
        assert_eq!(
            fs::read(b.fs.data_dir.join("backups/restores/game/payload/new-save")).unwrap(),
            b"preserve me"
        );
        let r = b.prefixes.show(&id, |_| Ok(())).unwrap().record;
        assert_eq!(r.lifecycle, PrefixLifecycleState::Created);
        assert_eq!(r.runtime, snapshot.source.runtime);
        assert!(r.initialization_evidence.is_none());
        assert!(matches!(
            b.prefixes.unregister(&id, rev + 1),
            Err(PrefixError::RecoveryRequired)
        ));
    }
    #[test]
    fn corrupt_payload_scope_confirmation_and_revision_fail_closed() {
        let (_tmp, b, id, scope, rev) = fixture();
        fs::write(b.fs.target_path(&id).join("file"), b"original").unwrap();
        assert!(b.create(&id, &scope, rev, false).is_err());
        assert!(b.create(&id, &scope, rev + 1, true).is_err());
        let m = b.create(&id, &scope, rev, true).unwrap();
        let wrong = PrefixScope::machine(crate::MachineId::from_digest("b".repeat(64)).unwrap());
        assert!(b.verify(&m.id, &wrong).is_err());
        fs::write(
            b.fs.data_dir
                .join("backups/snapshots")
                .join(m.id.as_str())
                .join("payload/file"),
            b"changed!",
        )
        .unwrap();
        assert!(b.verify(&m.id, &scope).is_err());
        assert!(b.restore_plan(&id, &m.id, &scope).is_err());
        assert_eq!(
            fs::read(b.fs.target_path(&id).join("file")).unwrap(),
            b"original"
        );
    }
    #[test]
    fn unsafe_entries_and_unknown_usage_are_rejected() {
        let (_tmp, mut b, id, scope, rev) = fixture();
        b.usage_override = Some(None);
        assert!(matches!(
            b.plan(&id, &scope),
            Err(BackupError::Prefix(PrefixError::UsageUnknown))
        ));
        b.usage_override = Some(Some(true));
        assert!(matches!(
            b.plan(&id, &scope),
            Err(BackupError::Prefix(PrefixError::ActiveUse))
        ));
        b.usage_override = Some(Some(false));
        let path = b.fs.target_path(&id);
        symlink("/etc/passwd", path.join("escape")).unwrap();
        assert!(b.create(&id, &scope, rev, true).is_err());
        fs::remove_file(path.join("escape")).unwrap();
        fs::write(path.join("file"), b"abc").unwrap();
        fs::hard_link(path.join("file"), path.join("alias")).unwrap();
        assert!(b.plan(&id, &scope).is_err());
        fs::remove_file(path.join("alias")).unwrap();
        let f = File::create(path.join("sparse")).unwrap();
        f.set_len(1024 * 1024).unwrap();
        assert!(b.plan(&id, &scope).is_err());
        assert!(BackupId::new("../bad").is_err());
        assert!(BackupId::new("/bad").is_err());
    }
    #[test]
    fn interrupted_publication_and_registry_commit_recover_idempotently() {
        for fault in ["restore-published", "restore-registry"] {
            let (_tmp, mut b, id, scope, rev) = fixture();
            fs::write(b.fs.target_path(&id).join("file"), b"snapshot").unwrap();
            let m = b.create(&id, &scope, rev, true).unwrap();
            fs::write(b.fs.target_path(&id).join("file"), b"original").unwrap();
            let plan = b.restore_plan(&id, &m.id, &scope).unwrap();
            b.fault = Some(fault);
            assert!(
                b.restore(&id, &m.id, &scope, rev, &plan.confirmation)
                    .is_err()
            );
            b.fault = None;
            let current = b.prefixes.load_snapshot().unwrap().registry.revision;
            b.recover(&id, &scope, current, true).unwrap();
            b.recover(&id, &scope, rev + 1, true).unwrap();
            assert_eq!(
                fs::read(b.fs.data_dir.join("backups/restores/game/payload/file")).unwrap(),
                b"original"
            );
            assert_eq!(
                fs::read(b.fs.target_path(&id).join("file")).unwrap(),
                b"snapshot"
            );
        }
    }
    #[test]
    fn interrupted_staging_preserves_live_data_and_blocks_guessing() {
        for fault in ["restore-intent", "restore-staged"] {
            let (_tmp, mut b, id, scope, rev) = fixture();
            fs::write(b.fs.target_path(&id).join("file"), b"original").unwrap();
            let m = b.create(&id, &scope, rev, true).unwrap();
            let p = b.restore_plan(&id, &m.id, &scope).unwrap();
            b.fault = Some(fault);
            assert!(b.restore(&id, &m.id, &scope, rev, &p.confirmation).is_err());
            b.fault = None;
            if fault == "restore-staged" {
                b.recover(&id, &scope, rev, true).unwrap();
            } else {
                assert!(b.recover(&id, &scope, rev, true).is_err());
            }
            assert_eq!(
                fs::read(b.fs.target_path(&id).join("file")).unwrap(),
                b"original"
            );
        }
    }
    #[test]
    fn backup_commit_boundaries_never_publish_partial_snapshots() {
        for fault in ["backup-intent", "backup-staged", "backup-published"] {
            let (_tmp, mut b, id, scope, rev) = fixture();
            b.fault = Some(fault);
            assert!(b.create(&id, &scope, rev, true).is_err());
            b.fault = None;
            let list = b.list().unwrap();
            assert_eq!(list.len(), usize::from(fault == "backup-published"));
            for id in list {
                b.verify(&id, &scope).unwrap();
            }
        }
    }
}

#[cfg(test)]
#[path = "backup_tests.rs"]
mod recovery_tests;
