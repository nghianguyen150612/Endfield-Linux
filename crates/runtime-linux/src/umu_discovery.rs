//! Bounded, read-only discovery of existing `umu-run` executables.

use crate::RootOutcome;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const MAX_UMU_PATH_BYTES: usize = 4096;
pub const MAX_UMU_PATH_ENTRIES: usize = 128;
pub const MAX_UMU_CANDIDATES: usize = 128;
pub const MAX_UMU_SYMLINK_HOPS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UmuAvailability {
    NotInstalled,
    CandidateFound,
    StructurallyValidated,
    PermissionDenied,
    InvalidExecutable,
    UnsupportedWrapper,
    IncompleteMetadata,
}

impl UmuAvailability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotInstalled => "not-installed",
            Self::CandidateFound => "candidate-found",
            Self::StructurallyValidated => "structurally-validated",
            Self::PermissionDenied => "permission-denied",
            Self::InvalidExecutable => "invalid-executable",
            Self::UnsupportedWrapper => "unsupported-wrapper",
            Self::IncompleteMetadata => "incomplete-metadata",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UmuVersionStatus {
    Unknown,
    LocallyIdentified,
}

#[derive(Clone, PartialEq, Eq)]
pub struct UmuCandidate {
    path: PathBuf,
    resolved_path: Option<PathBuf>,
    availability: UmuAvailability,
    version: Option<String>,
    identity: Option<FileIdentity>,
}

impl std::fmt::Debug for UmuCandidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UmuCandidate")
            .field("path", &"[redacted]")
            .field(
                "resolved_path",
                &self.resolved_path.as_ref().map(|_| "[redacted]"),
            )
            .field("availability", &self.availability)
            .field("version", &self.version)
            .finish()
    }
}

impl UmuCandidate {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn resolved_path(&self) -> Option<&Path> {
        self.resolved_path.as_deref()
    }
    pub const fn availability(&self) -> UmuAvailability {
        self.availability
    }
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
    pub const fn version_status(&self) -> UmuVersionStatus {
        if self.version.is_some() {
            UmuVersionStatus::LocallyIdentified
        } else {
            UmuVersionStatus::Unknown
        }
    }
    pub const fn is_structurally_valid(&self) -> bool {
        matches!(self.availability, UmuAvailability::StructurallyValidated)
    }

    /// Rechecks file identity immediately before a future launch boundary.
    pub fn revalidate(&self) -> bool {
        let Some(path) = &self.resolved_path else {
            return false;
        };
        let Some(expected) = &self.identity else {
            return false;
        };
        let Ok(link_metadata) = fs::symlink_metadata(path) else {
            return false;
        };
        if !link_metadata.is_file() || link_metadata.file_type().is_symlink() {
            return false;
        }
        fs::metadata(path).ok().is_some_and(|m| {
            FileIdentity::from(&m) == *expected
                && m.is_file()
                && m.permissions().mode() & 0o111 != 0
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
    length: u64,
    modified: i64,
    modified_nsec: i64,
    changed: i64,
    changed_nsec: i64,
}
impl From<&Metadata> for FileIdentity {
    fn from(m: &Metadata) -> Self {
        Self {
            device: m.dev(),
            inode: m.ino(),
            length: m.len(),
            modified: m.mtime(),
            modified_nsec: m.mtime_nsec(),
            changed: m.ctime(),
            changed_nsec: m.ctime_nsec(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct UmuDiscoveryEnvironment {
    home: Option<PathBuf>,
    path: Option<OsString>,
    include_system: bool,
}
impl std::fmt::Debug for UmuDiscoveryEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UmuDiscoveryEnvironment")
            .field("home_configured", &self.home.is_some())
            .field("path_configured", &self.path.is_some())
            .finish()
    }
}
impl UmuDiscoveryEnvironment {
    pub fn from_process() -> Self {
        Self {
            home: std::env::var_os("HOME").map(PathBuf::from),
            path: std::env::var_os("PATH"),
            include_system: true,
        }
    }
    pub fn isolated(home: Option<PathBuf>, path: Option<OsString>) -> Self {
        Self {
            home,
            path,
            include_system: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UmuDiscoveryReport {
    pub candidates: Vec<UmuCandidate>,
    pub issues: Vec<&'static str>,
    outcome: RootOutcome,
}
impl UmuDiscoveryReport {
    pub fn validated(&self) -> impl Iterator<Item = &UmuCandidate> {
        self.candidates.iter().filter(|c| c.is_structurally_valid())
    }
    pub fn availability(&self) -> UmuAvailability {
        self.candidates
            .iter()
            .find(|c| c.is_structurally_valid())
            .map_or_else(
                || {
                    self.candidates.first().map_or_else(
                        || {
                            if self.outcome == RootOutcome::NotPresent {
                                UmuAvailability::NotInstalled
                            } else {
                                UmuAvailability::IncompleteMetadata
                            }
                        },
                        |c| c.availability,
                    )
                },
                |c| c.availability,
            )
    }

    pub const fn outcome(&self) -> RootOutcome {
        self.outcome
    }

    pub const fn outcome_label(&self) -> &'static str {
        match self.outcome {
            RootOutcome::NotPresent => "not-present",
            RootOutcome::Inspected => "inspected",
            RootOutcome::PartiallyInspected => "partially-inspected",
            RootOutcome::Skipped => "skipped",
            RootOutcome::Failed => "failed",
            RootOutcome::Truncated => "truncated",
        }
    }
}

pub fn discover_umu(
    environment: &UmuDiscoveryEnvironment,
    explicit: Option<&Path>,
) -> UmuDiscoveryReport {
    let mut paths = Vec::new();
    let mut issues = Vec::new();
    if let Some(path) = explicit {
        if !path.is_absolute() || path.as_os_str().len() > MAX_UMU_PATH_BYTES || has_parent(path) {
            return UmuDiscoveryReport {
                candidates: vec![invalid(
                    path.to_path_buf(),
                    UmuAvailability::InvalidExecutable,
                )],
                issues: vec!["invalid-explicit-path"],
                outcome: RootOutcome::Failed,
            };
        }
        paths.push(path.to_path_buf());
    } else {
        if environment.include_system {
            paths.extend([
                PathBuf::from("/usr/bin/umu-run"),
                PathBuf::from("/usr/local/bin/umu-run"),
            ]);
        }
        if let Some(home) = environment.home.as_ref().filter(|p| {
            p.is_absolute() && p.as_os_str().len() <= MAX_UMU_PATH_BYTES && !has_parent(p)
        }) {
            paths.push(home.join(".local/bin/umu-run"));
        }
        if let Some(path) = &environment.path {
            let bytes = path.as_encoded_bytes();
            if bytes.len() > MAX_UMU_PATH_BYTES {
                issues.push("path-limit");
            } else {
                for (index, entry) in std::env::split_paths(path).enumerate() {
                    if index >= MAX_UMU_PATH_ENTRIES {
                        issues.push("path-entry-limit");
                        break;
                    }
                    if entry.as_os_str().is_empty()
                        || !entry.is_absolute()
                        || has_parent(&entry)
                        || entry.as_os_str().len() > MAX_UMU_PATH_BYTES
                    {
                        continue;
                    }
                    paths.push(entry.join("umu-run"));
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    for path in paths {
        if candidates.len() >= MAX_UMU_CANDIDATES {
            issues.push("candidate-limit");
            break;
        }
        if !path.is_absolute()
            || path.as_os_str().len() > MAX_UMU_PATH_BYTES
            || has_parent(&path)
            || !seen.insert(path.clone())
        {
            continue;
        }
        if let Some(candidate) = inspect(path) {
            candidates.push(candidate);
        }
    }
    let truncated = issues.iter().any(|issue| {
        matches!(
            *issue,
            "path-limit" | "path-entry-limit" | "candidate-limit"
        )
    });
    let has_invalid = candidates
        .iter()
        .any(|candidate| candidate.availability() != UmuAvailability::StructurallyValidated);
    let outcome = if truncated {
        RootOutcome::Truncated
    } else if explicit.is_some() && has_invalid {
        RootOutcome::Failed
    } else if !issues.is_empty() || has_invalid {
        RootOutcome::PartiallyInspected
    } else if candidates.is_empty() {
        RootOutcome::NotPresent
    } else {
        RootOutcome::Inspected
    };
    UmuDiscoveryReport {
        candidates,
        issues,
        outcome,
    }
}

fn inspect(path: PathBuf) -> Option<UmuCandidate> {
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Some(invalid(path, UmuAvailability::PermissionDenied));
        }
        Err(_) => return Some(invalid(path, UmuAvailability::InvalidExecutable)),
    }
    let resolved = match resolve(path.clone()) {
        Ok(p) => p,
        Err(_) => return Some(invalid(path, UmuAvailability::InvalidExecutable)),
    };
    let before = match fs::metadata(&resolved) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Some(invalid(path, UmuAvailability::PermissionDenied));
        }
        Err(_) => return Some(invalid(path, UmuAvailability::InvalidExecutable)),
    };
    if !before.is_file() || before.permissions().mode() & 0o111 == 0 {
        return Some(invalid(path, UmuAvailability::InvalidExecutable));
    }
    let after = match fs::metadata(&resolved) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return Some(invalid(path, UmuAvailability::PermissionDenied));
        }
        Err(_) => return Some(invalid(path, UmuAvailability::InvalidExecutable)),
    };
    if FileIdentity::from(&before) != FileIdentity::from(&after) {
        return Some(invalid(path, UmuAvailability::InvalidExecutable));
    }
    Some(UmuCandidate {
        path,
        resolved_path: Some(resolved),
        availability: UmuAvailability::StructurallyValidated,
        version: None,
        identity: Some(FileIdentity::from(&after)),
    })
}

fn invalid(path: PathBuf, availability: UmuAvailability) -> UmuCandidate {
    UmuCandidate {
        path,
        resolved_path: None,
        availability,
        version: None,
        identity: None,
    }
}
fn has_parent(path: &Path) -> bool {
    path.components()
        .any(|c| c == std::path::Component::ParentDir)
}
fn resolve(mut path: PathBuf) -> Result<PathBuf, ()> {
    for _ in 0..=MAX_UMU_SYMLINK_HOPS {
        let meta = fs::symlink_metadata(&path).map_err(|_| ())?;
        if !meta.file_type().is_symlink() {
            return fs::canonicalize(path).map_err(|_| ());
        }
        let target = fs::read_link(&path).map_err(|_| ())?;
        path = if target.is_absolute() {
            target
        } else {
            path.parent().ok_or(())?.join(target)
        };
        if !path.is_absolute() || path.as_os_str().len() > MAX_UMU_PATH_BYTES {
            return Err(());
        }
    }
    Err(())
}
