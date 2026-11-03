//! Narrow quoted KeyValues subset, not a general Steam API or payload verifier.
use crate::{
    InstalledSourceEvidence, SCHEMA_VERSION, STEAM_APP_ID, SourceError, SourceVerificationStatus,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::CString,
    fs::File,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path, PathBuf},
};
const MAX_BYTES: usize = 64 * 1024;
const MANIFEST: &str = "appmanifest_4732690.acf";

#[derive(Debug)]
enum Value {
    Text(String),
    Object,
}
struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    tokens: usize,
}
impl Parser<'_> {
    fn whitespace(&mut self) {
        while self
            .bytes
            .get(self.pos)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.pos += 1;
        }
    }
    fn string(&mut self) -> Result<String, SourceError> {
        self.whitespace();
        self.tokens += 1;
        if self.tokens > 4096 || self.bytes.get(self.pos) != Some(&b'"') {
            return Err(SourceError::InvalidMetadata);
        }
        self.pos += 1;
        let mut value = Vec::new();
        loop {
            let byte = *self
                .bytes
                .get(self.pos)
                .ok_or(SourceError::InvalidMetadata)?;
            self.pos += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let escaped = *self
                        .bytes
                        .get(self.pos)
                        .ok_or(SourceError::InvalidMetadata)?;
                    self.pos += 1;
                    if !matches!(escaped, b'"' | b'\\') {
                        return Err(SourceError::InvalidMetadata);
                    }
                    value.push(escaped);
                }
                0..=31 | 127 => return Err(SourceError::InvalidMetadata),
                _ => value.push(byte),
            }
            if value.len() > 4096 {
                return Err(SourceError::InvalidMetadata);
            }
        }
        String::from_utf8(value).map_err(|_| SourceError::InvalidMetadata)
    }
    fn object(&mut self, depth: usize) -> Result<BTreeMap<String, Value>, SourceError> {
        self.whitespace();
        if depth > 8 || self.bytes.get(self.pos) != Some(&b'{') {
            return Err(SourceError::InvalidMetadata);
        }
        self.pos += 1;
        let mut fields = BTreeMap::new();
        loop {
            self.whitespace();
            if self.bytes.get(self.pos) == Some(&b'}') {
                self.pos += 1;
                return Ok(fields);
            }
            let key = self.string()?.to_ascii_lowercase();
            self.whitespace();
            let value = if self.bytes.get(self.pos) == Some(&b'{') {
                {
                    self.object(depth + 1)?;
                    Value::Object
                }
            } else {
                Value::Text(self.string()?)
            };
            if key.is_empty() || fields.insert(key, value).is_some() {
                return Err(SourceError::InvalidMetadata);
            }
        }
    }
}
fn parse(bytes: &[u8]) -> Result<BTreeMap<String, Value>, SourceError> {
    if bytes.len() > MAX_BYTES {
        return Err(SourceError::InvalidMetadata);
    }
    let mut parser = Parser {
        bytes,
        pos: 0,
        tokens: 0,
    };
    if parser.string()? != "AppState" {
        return Err(SourceError::InvalidMetadata);
    }
    let fields = parser.object(0)?;
    parser.whitespace();
    if parser.pos != bytes.len() {
        return Err(SourceError::InvalidMetadata);
    }
    Ok(fields)
}
fn text<'a>(
    fields: &'a BTreeMap<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, SourceError> {
    match fields.get(key) {
        None => Ok(None),
        Some(Value::Text(value)) => Ok(Some(value)),
        Some(Value::Object) => Err(SourceError::InvalidMetadata),
    }
}
fn numeric(fields: &BTreeMap<String, Value>, key: &str) -> Result<Option<String>, SourceError> {
    text(fields, key)?
        .map(|value| {
            if value.is_empty()
                || value.len() > 20
                || !value.bytes().all(|b| b.is_ascii_digit())
                || value.parse::<u64>().is_err()
            {
                return Err(SourceError::InvalidMetadata);
            }
            Ok(value.to_owned())
        })
        .transpose()
}
fn io(error: std::io::Error) -> SourceError {
    match error.raw_os_error() {
        Some(libc::ENOENT) => SourceError::Missing,
        Some(libc::ELOOP) | Some(libc::ENOTDIR) => SourceError::InvalidPath,
        _ => SourceError::Io,
    }
}
fn normalized(path: &Path) -> Result<PathBuf, SourceError> {
    let raw = path.to_str().ok_or(SourceError::InvalidPath)?;
    if !path.is_absolute() || raw.len() > 4096 || raw.chars().any(char::is_control) {
        return Err(SourceError::InvalidPath);
    }
    let mut result = PathBuf::from("/");
    let mut count = 0;
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                count += 1;
                if count > 256 || name.as_bytes().len() > 255 {
                    return Err(SourceError::InvalidPath);
                }
                result.push(name);
            }
            Component::RootDir => (),
            _ => return Err(SourceError::InvalidPath),
        }
    }
    Ok(result)
}
fn child(parent: &File, name: &std::ffi::OsStr, directory: bool) -> Result<File, SourceError> {
    let name = CString::new(name.as_bytes()).map_err(|_| SourceError::InvalidPath)?;
    // O_PATH validates type without opening a FIFO/device for I/O. Reopen only
    // that held regular inode through the kernel's own fd link, never a user link.
    let flags = libc::O_PATH
        | libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | if directory { libc::O_DIRECTORY } else { 0 };
    // SAFETY: name is NUL-terminated; parent owns a live descriptor.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(io(std::io::Error::last_os_error()));
    }
    // SAFETY: openat returned a fresh owned descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata().map_err(io)?;
    if (!directory && (!meta.is_file() || meta.nlink() != 1)) || (directory && !meta.is_dir()) {
        return Err(SourceError::InvalidPath);
    }
    if !directory {
        let readable = File::open(format!("/proc/self/fd/{}", file.as_raw_fd())).map_err(io)?;
        let reopened = readable.metadata().map_err(io)?;
        if meta.dev() != reopened.dev() || meta.ino() != reopened.ino() || reopened.nlink() != 1 {
            return Err(SourceError::Changed);
        }
        return Ok(readable);
    }
    Ok(file)
}
fn open(path: &Path, directory: bool) -> Result<File, SourceError> {
    let mut parent = File::open("/").map_err(io)?;
    let parts: Vec<_> = path
        .components()
        .filter_map(|part| {
            if let Component::Normal(name) = part {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    for (index, part) in parts.iter().enumerate() {
        parent = child(&parent, part, index + 1 != parts.len() || directory)?;
    }
    Ok(parent)
}
fn stamp(file: &File) -> Result<String, SourceError> {
    let m = file.metadata().map_err(io)?;
    Ok(format!(
        "{}:{}:{}:{}:{}:{}:{}",
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec()
    ))
}
fn directory_identity(file: &File) -> Result<String, SourceError> {
    let m = file.metadata().map_err(io)?;
    Ok(format!("{}:{}", m.dev(), m.ino()))
}
fn manifest_bytes(file: &File) -> Result<Vec<u8>, SourceError> {
    if file.metadata().map_err(io)?.len() > MAX_BYTES as u64 {
        return Err(SourceError::InvalidMetadata);
    }
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > MAX_BYTES {
        return Err(SourceError::InvalidMetadata);
    }
    Ok(bytes)
}
/// Inspect only a selected library root or the exact app manifest. No library discovery.
pub fn inspect_steam(path: &Path) -> Result<InstalledSourceEvidence, SourceError> {
    let selected = normalized(path)?;
    let manifest = if selected.file_name().is_some_and(|name| name == MANIFEST) {
        if selected
            .parent()
            .and_then(Path::file_name)
            .is_none_or(|name| name != "steamapps")
        {
            return Err(SourceError::InvalidPath);
        }
        selected
    } else {
        selected.join("steamapps").join(MANIFEST)
    };
    let apps = manifest.parent().ok_or(SourceError::InvalidPath)?;
    let file = open(&manifest, false)?;
    let before = stamp(&file)?;
    let bytes = manifest_bytes(&file)?;
    let fields = parse(&bytes)?;
    if text(&fields, "appid")? != Some(STEAM_APP_ID) {
        return Err(SourceError::WrongApp);
    }
    let install = text(&fields, "installdir")?.ok_or(SourceError::InvalidMetadata)?;
    // One literal child component; neither Windows separators nor traversal accepted.
    if install.is_empty()
        || install.len() > 255
        || matches!(install, "." | "..")
        || install.contains(['/', '\\', ':'])
        || install.chars().any(char::is_control)
    {
        return Err(SourceError::InvalidPath);
    }
    let installation = apps.join("common").join(install);
    let directory = match open(&installation, true) {
        Ok(file) => Some(file),
        Err(SourceError::Missing) => None,
        Err(error) => return Err(error),
    };
    let identity = directory.as_ref().map(directory_identity).transpose()?;
    let build = numeric(&fields, "buildid")?;
    let state = numeric(&fields, "stateflags")?;
    // Reopen the same absolute paths, rejecting replacement/alias changes during observation.
    if before != stamp(&file)? || before != stamp(&open(&manifest, false)?)? {
        return Err(SourceError::Changed);
    }
    let current = match open(&installation, true) {
        Ok(file) => Some(directory_identity(&file)?),
        Err(SourceError::Missing) => None,
        Err(error) => return Err(error),
    };
    if identity != current {
        return Err(SourceError::Changed);
    }
    let mut hash = Sha256::new();
    for part in [
        b"endfield-unix:steam-observation:v1\0".as_slice(),
        manifest.as_os_str().as_bytes(),
        before.as_bytes(),
        bytes.as_slice(),
        identity.as_deref().unwrap_or("missing").as_bytes(),
    ] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part);
    }
    Ok(InstalledSourceEvidence {
        schema_version: SCHEMA_VERSION,
        source_id: "steam",
        app_id: STEAM_APP_ID,
        status: SourceVerificationStatus::ObservedLocalMetadata,
        manifest_path: manifest
            .to_str()
            .ok_or(SourceError::InvalidPath)?
            .to_owned(),
        installation_path: installation
            .to_str()
            .ok_or(SourceError::InvalidPath)?
            .to_owned(),
        installation_present: identity.is_some(),
        reported_build_id: build,
        reported_state_flags: state,
        observation_token: format!("{:x}", hash.finalize()),
        publisher_authenticity: SourceVerificationStatus::Unverifiable,
        artifact_integrity: SourceVerificationStatus::Unverifiable,
        game_build: SourceVerificationStatus::Unknown,
        game_compatibility: "not_tested",
        execution_readiness: SourceVerificationStatus::Unknown,
        limitations: "Editable local metadata only; reported build/state are untrusted Steam claims, not game version or complete installation proof. No payload read or hash. Observation token detects metadata/object change, not publisher authenticity or game content change; no remount continuity guarantee. Steam owns updates.",
    })
}
/// Reopen the caller's exact path; token binds manifest path, bytes and object identities.
/// Missing/unsafe evidence remains a typed error. No search or persistence occurs.
pub fn revalidate_steam(
    path: &Path,
    expected: &str,
) -> Result<InstalledSourceEvidence, SourceError> {
    if expected.len() != 64
        || !expected
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(SourceError::InvalidMetadata);
    }
    let mut report = inspect_steam(path)?;
    if report.observation_token != expected {
        report.status = SourceVerificationStatus::Changed;
    }
    Ok(report)
}
