//! Read-only installed runtime inventory and baseline comparison.
#![cfg(target_os = "linux")]
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::Path,
};

pub const BASELINE_FILE: &str = ".endfield-runtime-baseline.json";
pub const RECORD_FILE: &str = ".endfield-installation.json";
pub const MAX_ENTRIES: usize = 200_000;
pub const MAX_FILE: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_TOTAL: u64 = 16 * 1024 * 1024 * 1024;
pub const MAX_PATH: usize = 4096;
pub const MAX_DEPTH: usize = 32;
pub const MAX_TOTAL_PATH_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_FINDINGS: usize = 4096;
pub const MAX_MANIFEST: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrustClassification {
    IndependentlyAuthenticatedPublisher,
    UserPinnedLimitedTrust,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BaselineOrigin {
    VerifiedInstallation,
    ReconstructedFromReverifiedArtifact,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntryKind {
    File {
        size: u64,
        sha256: String,
        executable: bool,
        setid: bool,
    },
    Directory {
        mode: u32,
    },
    Symlink {
        target: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub path: String,
    pub kind: EntryKind,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    pub schema_version: u32,
    pub installation_id: String,
    pub generation: u64,
    pub family: String,
    pub build: String,
    pub artifact_sha256: String,
    pub manifest_sha256: String,
    pub trust: TrustClassification,
    pub origin: BaselineOrigin,
    pub inventory_sha256: String,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    Complete,
    Partial,
    Unverifiable,
    Failed,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    Match,
    Modified,
    Missing,
    Unexpected,
    TypeChanged,
    PermissionsChanged,
    SymlinkChanged,
    BaselineMissing,
    BaselineInvalid,
    Unverifiable,
    Unreadable,
    ChangedDuringAudit,
    LimitExceeded,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Finding {
    pub kind: FindingKind,
    pub path: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditReport {
    pub completeness: Completeness,
    pub inspected: usize,
    pub matching: usize,
    pub findings: Vec<Finding>,
    pub trust: Option<TrustClassification>,
}

fn digest(s: &[u8]) -> String {
    format!("{:x}", Sha256::digest(s))
}
fn valid_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}
fn valid_path(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_PATH
        && !s.starts_with('/')
        && !s.contains('\\')
        && s.split('/').count() <= MAX_DEPTH
        && s.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
}
fn inventory_digest(entries: &[Entry]) -> String {
    digest(&serde_json::to_vec(entries).expect("entry serialization"))
}
impl Baseline {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.generation == 0
            || !valid_digest(&self.artifact_sha256)
            || !valid_digest(&self.manifest_sha256)
            || !valid_digest(&self.inventory_sha256)
        {
            return Err("baseline identity or digest invalid".into());
        }
        if self.entries.len() > MAX_ENTRIES
            || serde_json::to_vec(self).map_err(|e| e.to_string())?.len() as u64 > MAX_MANIFEST
        {
            return Err("baseline limit exceeded".into());
        }
        let mut previous = None;
        for e in &self.entries {
            if !valid_path(&e.path) || previous.is_some_and(|p: &str| p >= e.path.as_str()) {
                return Err("baseline paths invalid or duplicated".into());
            }
            previous = Some(e.path.as_str());
            match &e.kind {
                EntryKind::File { size, sha256, .. }
                    if *size > MAX_FILE || !valid_digest(sha256) =>
                {
                    return Err("file baseline invalid".into());
                }
                EntryKind::Symlink { target }
                    if target.is_empty()
                        || target.len() > 4096
                        || target.starts_with('/')
                        || target.split('/').any(|p| p == "..") =>
                {
                    return Err("symlink baseline invalid".into());
                }
                _ => (),
            }
        }
        if self.inventory_sha256 != inventory_digest(&self.entries) {
            return Err("inventory digest mismatch".into());
        }
        Ok(())
    }
}

/// Build a local installation snapshot during private staging. The two control files
/// have an explicit exclusion policy: the ownership record is separately validated;
/// this baseline describes payload entries and does not recursively hash itself.
#[allow(clippy::too_many_arguments)]
pub fn create_baseline(
    root: &Path,
    installation_id: &str,
    generation: u64,
    family: &str,
    build: &str,
    artifact_sha256: &str,
    manifest_sha256: &str,
    trust: TrustClassification,
) -> Result<Baseline, String> {
    let root_file = File::open(root).map_err(|_| "installation root unavailable")?;
    let mut entries = Vec::new();
    let mut total = 0u64;
    let mut path_bytes = 0;
    walk_fd(
        &root_file,
        String::new(),
        0,
        &mut entries,
        &mut total,
        &mut path_bytes,
    )?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let mut b = Baseline {
        schema_version: 1,
        installation_id: installation_id.into(),
        generation,
        family: family.into(),
        build: build.into(),
        artifact_sha256: artifact_sha256.into(),
        manifest_sha256: manifest_sha256.into(),
        trust,
        origin: BaselineOrigin::VerifiedInstallation,
        inventory_sha256: String::new(),
        entries,
    };
    b.inventory_sha256 = inventory_digest(&b.entries);
    b.validate()?;
    Ok(b)
}
fn walk_fd(
    dir: &File,
    prefix: String,
    depth: usize,
    out: &mut Vec<Entry>,
    total: &mut u64,
    path_bytes: &mut usize,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("depth limit".into());
    }
    let it = std::fs::read_dir(format!("/proc/self/fd/{}", dir.as_raw_fd()))
        .map_err(|_| "directory unreadable")?;
    for item in it {
        let item = item.map_err(|_| "directory unreadable")?;
        let name = item
            .file_name()
            .into_string()
            .map_err(|_| "non-UTF8 path")?;
        if depth == 0 && (name == BASELINE_FILE || name == RECORD_FILE) {
            continue;
        }
        if name.contains('/') || name == "." || name == ".." {
            return Err("invalid directory entry".into());
        }
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if !valid_path(&rel) || out.len() >= MAX_ENTRIES {
            return Err("inventory limit or path invalid".into());
        }
        *path_bytes = path_bytes
            .checked_add(rel.len())
            .ok_or("path size overflow")?;
        if *path_bytes > MAX_TOTAL_PATH_BYTES {
            return Err("metadata limit exceeded".into());
        }
        let c = std::ffi::CString::new(name.as_bytes()).map_err(|_| "invalid path")?;
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                dir.as_raw_fd(),
                c.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err("entry changed during audit".into());
        }
        let st = unsafe { stat.assume_init() };
        let mode = st.st_mode;
        let kind = if mode & libc::S_IFMT == libc::S_IFLNK {
            let mut buf = [0u8; 4097];
            let n = unsafe {
                libc::readlinkat(
                    dir.as_raw_fd(),
                    c.as_ptr(),
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                )
            };
            if n < 0 || n as usize > 4096 {
                return Err("symlink unreadable".into());
            }
            let target = std::str::from_utf8(&buf[..n as usize])
                .map_err(|_| "invalid symlink")?
                .to_owned();
            let mut after = std::mem::MaybeUninit::<libc::stat>::uninit();
            if unsafe {
                libc::fstatat(
                    dir.as_raw_fd(),
                    c.as_ptr(),
                    after.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } != 0
            {
                return Err("symlink changed during audit".into());
            }
            let after = unsafe { after.assume_init() };
            if after.st_dev != st.st_dev
                || after.st_ino != st.st_ino
                || after.st_ctime != st.st_ctime
                || after.st_ctime_nsec != st.st_ctime_nsec
            {
                return Err("symlink changed during audit".into());
            }
            if target.starts_with('/') || target.split('/').any(|x| x == "..") {
                return Err("unsafe symlink".into());
            }
            EntryKind::Symlink { target }
        } else if mode & libc::S_IFMT == libc::S_IFDIR {
            EntryKind::Directory {
                mode: mode & 0o7777,
            }
        } else if mode & libc::S_IFMT == libc::S_IFREG {
            if st.st_size < 0 || st.st_size as u64 > MAX_FILE {
                return Err("file size limit".into());
            }
            *total = total
                .checked_add(st.st_size as u64)
                .ok_or("byte count overflow")?;
            if *total > MAX_TOTAL {
                return Err("total size limit".into());
            }
            let fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    c.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                )
            };
            if fd < 0 {
                return Err("file unreadable or changed".into());
            }
            let f = unsafe { File::from_raw_fd(fd) };
            let (size, sha256) = hash_file(f, st.st_dev, st.st_ino, st.st_size as u64)?;
            EntryKind::File {
                size,
                sha256,
                executable: mode & 0o111 != 0,
                setid: mode & 0o6000 != 0,
            }
        } else {
            return Err("unsupported filesystem entry".into());
        };
        out.push(Entry {
            path: rel.clone(),
            kind,
        });
        if mode & libc::S_IFMT == libc::S_IFDIR {
            let fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    c.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err("directory changed during audit".into());
            }
            let child = unsafe { File::from_raw_fd(fd) };
            let opened = child.metadata().map_err(|_| "directory metadata failed")?;
            if opened.dev() != st.st_dev || opened.ino() != st.st_ino {
                return Err("directory changed during audit".into());
            }
            walk_fd(&child, rel, depth + 1, out, total, path_bytes)?;
        }
    }
    Ok(())
}
fn hash_file(
    mut f: File,
    expected_dev: u64,
    expected_ino: u64,
    expected_size: u64,
) -> Result<(u64, String), String> {
    let before = f.metadata().map_err(|_| "metadata failed")?;
    if !before.is_file()
        || before.dev() != expected_dev
        || before.ino() != expected_ino
        || before.len() != expected_size
    {
        return Err("entry changed type".into());
    }
    let mut h = Sha256::new();
    let mut n = 0u64;
    let mut buf = [0u8; 65536];
    loop {
        let r = f.read(&mut buf).map_err(|_| "file read failed")?;
        if r == 0 {
            break;
        }
        n = n.checked_add(r as u64).ok_or("counter overflow")?;
        if n > MAX_FILE {
            return Err("file limit".into());
        }
        h.update(&buf[..r]);
    }
    let after = f.metadata().map_err(|_| "metadata failed")?;
    if n != before.len()
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err("changed during audit".into());
    }
    Ok((n, format!("{:x}", h.finalize())))
}
pub fn write_baseline(root: &Path, b: &Baseline) -> Result<(), String> {
    b.validate()?;
    let p = root.join(BASELINE_FILE);
    let data = serde_json::to_vec(b).map_err(|_| "serialize failed")?;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(p)
        .map_err(|_| "baseline create failed")?;
    f.write_all(&data).map_err(|_| "baseline write failed")?;
    f.sync_all().map_err(|_| "baseline sync failed".to_owned())
}
pub fn load_baseline(root: &Path) -> Result<Baseline, String> {
    let p = root.join(BASELINE_FILE);
    let m = std::fs::symlink_metadata(&p).map_err(|_| "baseline missing")?;
    if !m.is_file()
        || m.file_type().is_symlink()
        || m.len() > MAX_MANIFEST
        || m.permissions().mode() & 0o077 != 0
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
    {
        return Err("baseline invalid".into());
    }
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(p)
        .map_err(|_| "baseline invalid")?;
    let before = f.metadata().map_err(|_| "baseline unreadable")?;
    let mut bytes = Vec::with_capacity(before.len() as usize);
    (&mut f)
        .take(MAX_MANIFEST + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "baseline unreadable")?;
    let after = f.metadata().map_err(|_| "baseline unreadable")?;
    if bytes.len() as u64 > MAX_MANIFEST
        || before.len() != bytes.len() as u64
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err("baseline changed during read".into());
    }
    let b: Baseline = serde_json::from_slice(&bytes).map_err(|_| "baseline malformed")?;
    b.validate()?;
    Ok(b)
}

pub fn audit(root: &Path, b: &Baseline) -> AuditReport {
    let mut report = AuditReport {
        completeness: Completeness::Complete,
        inspected: 0,
        matching: 0,
        findings: Vec::new(),
        trust: Some(b.trust),
    };
    if b.validate().is_err() {
        report.completeness = Completeness::Failed;
        report.findings.push(Finding {
            kind: FindingKind::BaselineInvalid,
            path: None,
        });
        return report;
    }
    let mut actual = Vec::new();
    let mut total = 0;
    if let Ok(root_file) = File::open(root) {
        let mut path_bytes = 0;
        if let Err(e) = walk_fd(
            &root_file,
            String::new(),
            0,
            &mut actual,
            &mut total,
            &mut path_bytes,
        ) {
            report.completeness = Completeness::Partial;
            report.findings.push(Finding {
                kind: if e.contains("changed") {
                    FindingKind::ChangedDuringAudit
                } else if e.contains("limit") {
                    FindingKind::LimitExceeded
                } else {
                    FindingKind::Unreadable
                },
                path: None,
            });
        }
    } else {
        report.completeness = Completeness::Failed;
        report.findings.push(Finding {
            kind: FindingKind::Unreadable,
            path: None,
        });
    }
    let expected: BTreeMap<_, _> = b.entries.iter().map(|e| (e.path.as_str(), e)).collect();
    for e in &b.entries {
        report.inspected += 1;
        let Some(a) = actual.iter().find(|a| a.path == e.path) else {
            report.findings.push(Finding {
                kind: if report.completeness == Completeness::Complete {
                    FindingKind::Missing
                } else {
                    FindingKind::Unreadable
                },
                path: Some(e.path.clone()),
            });
            continue;
        };
        let kind = match (&e.kind, &a.kind) {
            (
                EntryKind::File {
                    size,
                    sha256,
                    executable,
                    setid,
                },
                EntryKind::File {
                    size: s,
                    sha256: h,
                    executable: x,
                    setid: si,
                },
            ) => {
                if size != s || sha256 != h {
                    Some(FindingKind::Modified)
                } else if executable != x || setid != si {
                    Some(FindingKind::PermissionsChanged)
                } else {
                    None
                }
            }
            (EntryKind::Directory { mode }, EntryKind::Directory { mode: actual_mode }) => {
                if mode == actual_mode {
                    None
                } else {
                    Some(FindingKind::PermissionsChanged)
                }
            }
            (EntryKind::Symlink { target }, EntryKind::Symlink { target: t }) => {
                if target != t {
                    Some(FindingKind::SymlinkChanged)
                } else {
                    None
                }
            }
            _ => Some(FindingKind::TypeChanged),
        };
        if let Some(kind) = kind {
            report.findings.push(Finding {
                kind,
                path: Some(e.path.clone()),
            });
        } else {
            report.matching += 1;
            report.findings.push(Finding {
                kind: FindingKind::Match,
                path: Some(e.path.clone()),
            });
        }
    }
    for e in actual {
        if !expected.contains_key(e.path.as_str()) {
            report.findings.push(Finding {
                kind: FindingKind::Unexpected,
                path: Some(e.path),
            });
        }
    }
    if report.findings.len() > MAX_FINDINGS {
        report.findings.truncate(MAX_FINDINGS - 1);
        report.findings.push(Finding {
            kind: FindingKind::LimitExceeded,
            path: None,
        });
        report.completeness = Completeness::Partial;
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_mutation_missing_and_additions() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a"), b"ok").unwrap();
        let b = create_baseline(
            d.path(),
            "id",
            1,
            "proton",
            "build",
            &"a".repeat(64),
            &"b".repeat(64),
            TrustClassification::UserPinnedLimitedTrust,
        )
        .unwrap();
        std::fs::write(d.path().join("a"), b"bad").unwrap();
        std::fs::write(d.path().join("extra"), b"new").unwrap();
        let r = audit(d.path(), &b);
        assert!(r.findings.iter().any(|f| f.kind == FindingKind::Modified));
        assert!(r.findings.iter().any(|f| f.kind == FindingKind::Unexpected));
    }
    #[test]
    fn validates_baseline_paths_and_identity() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("x"), b"x").unwrap();
        let mut b = create_baseline(
            d.path(),
            "id",
            1,
            "wine",
            "build",
            &"a".repeat(64),
            &"b".repeat(64),
            TrustClassification::UserPinnedLimitedTrust,
        )
        .unwrap();
        b.entries[0].path = "../escape".into();
        assert!(b.validate().is_err());
    }

    #[test]
    fn detects_missing_file_permission_change_and_symlink_retarget() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("file"), b"payload").unwrap();
        symlink("file", d.path().join("alias")).unwrap();
        let b = create_baseline(
            d.path(),
            "id",
            1,
            "proton",
            "b",
            &"a".repeat(64),
            &"b".repeat(64),
            TrustClassification::UserPinnedLimitedTrust,
        )
        .unwrap();
        std::fs::remove_file(d.path().join("file")).unwrap();
        symlink("missing", d.path().join("file")).unwrap();
        std::fs::remove_file(d.path().join("alias")).unwrap();
        symlink("other", d.path().join("alias")).unwrap();
        let r = audit(d.path(), &b);
        assert!(
            r.findings
                .iter()
                .any(|f| f.kind == FindingKind::TypeChanged)
        );
        assert!(
            r.findings
                .iter()
                .any(|f| f.kind == FindingKind::SymlinkChanged)
        );
        let p = d.path().join("executable");
        std::fs::write(&p, b"x").unwrap();
        let baseline = create_baseline(
            d.path(),
            "id",
            1,
            "wine",
            "b",
            &"a".repeat(64),
            &"b".repeat(64),
            TrustClassification::UserPinnedLimitedTrust,
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            audit(d.path(), &baseline)
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::PermissionsChanged)
        );
    }
}
