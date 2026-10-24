use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fs2::FileExt;

use crate::errors::io_error;
use crate::paths::{
    DiagnosticsPaths, LinuxDiagnosticsEnvironment, ensure_log_directory,
    validate_explicit_directory, verify_log_directory_if_present,
};
use crate::{DiagnosticEvent, DiagnosticsError};

pub const MAX_SEGMENT_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_RETAINED_SEGMENTS: usize = 5;
pub const MAX_LOG_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
pub const MAX_EVENT_BYTES: usize = 4096;

const CURRENT_LOG_FILE: &str = "current.jsonl";
const LOCK_FILE: &str = ".writer.lock";
const MAX_DIRECTORY_ENTRIES: usize = 4096;
static ARCHIVE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetentionPolicy {
    max_segment_bytes: u64,
    retained_segments: usize,
    max_age: Duration,
    max_event_bytes: usize,
}

impl RetentionPolicy {
    pub fn new(
        max_segment_bytes: u64,
        retained_segments: usize,
        max_age: Duration,
        max_event_bytes: usize,
    ) -> Result<Self, DiagnosticsError> {
        if max_segment_bytes == 0
            || retained_segments == 0
            || max_event_bytes == 0
            || u64::try_from(max_event_bytes.saturating_add(1))
                .map_or(true, |minimum| minimum > max_segment_bytes)
        {
            return Err(DiagnosticsError::InvalidRetentionPolicy);
        }
        Ok(Self {
            max_segment_bytes,
            retained_segments,
            max_age,
            max_event_bytes,
        })
    }

    pub const fn max_segment_bytes(self) -> u64 {
        self.max_segment_bytes
    }

    pub const fn retained_segments(self) -> usize {
        self.retained_segments
    }

    pub const fn max_age(self) -> Duration {
        self.max_age
    }

    pub const fn max_event_bytes(self) -> usize {
        self.max_event_bytes
    }
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_segment_bytes: MAX_SEGMENT_BYTES,
            retained_segments: MAX_RETAINED_SEGMENTS,
            max_age: MAX_LOG_AGE,
            max_event_bytes: MAX_EVENT_BYTES,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogReadResult {
    pub events: Vec<DiagnosticEvent>,
    pub omitted_records: u64,
}

impl LogReadResult {
    pub fn empty() -> Self {
        Self {
            events: Vec::new(),
            omitted_records: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogStore {
    directory: PathBuf,
    policy: RetentionPolicy,
}

impl LogStore {
    pub fn from_environment() -> Result<Self, DiagnosticsError> {
        let paths = DiagnosticsPaths::discover()?;
        Self::at_directory(paths.log_directory().to_path_buf())
    }

    pub fn from_linux_environment(
        environment: &LinuxDiagnosticsEnvironment,
    ) -> Result<Self, DiagnosticsError> {
        let paths = DiagnosticsPaths::from_linux_environment(environment)?;
        Self::at_directory(paths.log_directory().to_path_buf())
    }

    pub fn discover_existing_from_environment() -> Result<Option<Self>, DiagnosticsError> {
        let paths = DiagnosticsPaths::discover()?;
        let store = Self::at_directory(paths.log_directory().to_path_buf())?;
        if verify_log_directory_if_present(&store.directory)? {
            Ok(Some(store))
        } else {
            Ok(None)
        }
    }

    pub fn at_directory(directory: impl Into<PathBuf>) -> Result<Self, DiagnosticsError> {
        Self::with_policy(directory, RetentionPolicy::default())
    }

    pub fn with_policy(
        directory: impl Into<PathBuf>,
        policy: RetentionPolicy,
    ) -> Result<Self, DiagnosticsError> {
        let directory = directory.into();
        validate_explicit_directory(&directory)?;
        Ok(Self { directory, policy })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub const fn policy(&self) -> RetentionPolicy {
        self.policy
    }

    /// Creates the private log directory and removes expired or excess app-owned segments.
    pub fn initialize(&self) -> Result<(), DiagnosticsError> {
        self.with_exclusive_lock(|_| self.cleanup_locked(SystemTime::now()))
    }

    /// Appends one allowlisted JSON event. Writers using this crate coordinate
    /// through an advisory lock file shared across processes.
    pub fn append_event(&self, event: &DiagnosticEvent) -> Result<(), DiagnosticsError> {
        self.append_event_at(event, SystemTime::now())
    }

    pub fn append_event_at(
        &self,
        event: &DiagnosticEvent,
        now: SystemTime,
    ) -> Result<(), DiagnosticsError> {
        event.validate()?;
        let mut line = serde_json::to_vec(event).map_err(|_| DiagnosticsError::Serialization)?;
        if line.len() > self.policy.max_event_bytes {
            return Err(DiagnosticsError::EventTooLarge);
        }
        line.push(b'\n');
        if u64::try_from(line.len()).map_or(true, |length| length > self.policy.max_segment_bytes) {
            return Err(DiagnosticsError::EventTooLarge);
        }

        self.with_exclusive_lock(|_| {
            self.cleanup_locked(now)?;
            let current_path = self.directory.join(CURRENT_LOG_FILE);
            let mut current = open_regular_log_file(&current_path, true)?;
            let length = current
                .metadata()
                .map_err(|error| io_error("inspect current log segment", &error))?
                .len();
            if length > self.policy.max_segment_bytes {
                return Err(DiagnosticsError::UnsafePath(
                    "current log segment exceeds its configured size limit",
                ));
            }
            if length.saturating_add(line.len() as u64) > self.policy.max_segment_bytes {
                drop(current);
                self.rotate_current(&current_path, now)?;
                current = open_regular_log_file(&current_path, true)?;
            }
            current
                .write_all(&line)
                .map_err(|error| io_error("append structured log event", &error))?;
            current
                .flush()
                .map_err(|error| io_error("flush structured log event", &error))?;
            current
                .sync_all()
                .map_err(|error| io_error("synchronize structured log event", &error))?;
            self.cleanup_locked(now)
        })
    }

    /// Applies the configured age and file-count limits at the supplied time.
    /// This also provides a deterministic maintenance hook for tests and embedders.
    pub fn enforce_retention_at(&self, now: SystemTime) -> Result<(), DiagnosticsError> {
        self.with_exclusive_lock(|_| self.cleanup_locked(now))
    }

    /// Reads only recognized JSONL segment names. Invalid, truncated, oversized,
    /// or unknown-schema lines are counted and omitted.
    pub fn read_events(&self) -> Result<LogReadResult, DiagnosticsError> {
        if !verify_log_directory_if_present(&self.directory)? {
            return Ok(LogReadResult::empty());
        }
        let mut segments = list_known_segments(&self.directory, self.policy)?;
        segments.sort_by(|left, right| right.modified.cmp(&left.modified));
        segments.truncate(self.policy.retained_segments);

        let mut result = LogReadResult::empty();
        for segment in segments {
            if segment.oversized {
                result.omitted_records = result.omitted_records.saturating_add(1);
                continue;
            }
            let bytes = match read_bounded_segment(&segment.path, self.policy.max_segment_bytes) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(DiagnosticsError::Io {
                    kind: crate::IoFailureKind::NotFound,
                    ..
                }) => continue,
                Err(error) => return Err(error),
            };
            parse_segment_events(&bytes, self.policy.max_event_bytes, &mut result);
        }
        result.events.sort_by(|left, right| {
            left.timestamp_unix_nanos()
                .cmp(&right.timestamp_unix_nanos())
                .then_with(|| left.cmp(right))
        });
        Ok(result)
    }

    fn with_exclusive_lock<T>(
        &self,
        operation: impl FnOnce(&File) -> Result<T, DiagnosticsError>,
    ) -> Result<T, DiagnosticsError> {
        ensure_log_directory(&self.directory)?;
        let lock_path = self.directory.join(LOCK_FILE);
        let lock = open_regular_log_file(&lock_path, true)?;
        FileExt::lock_exclusive(&lock).map_err(|error| io_error("lock log writer", &error))?;
        let result = operation(&lock);
        let _ = FileExt::unlock(&lock);
        result
    }

    fn rotate_current(&self, current_path: &Path, now: SystemTime) -> Result<(), DiagnosticsError> {
        let mut source = open_regular_log_file(current_path, false)?;
        let source_metadata = source
            .metadata()
            .map_err(|error| io_error("inspect current log segment", &error))?;
        if !source_metadata.is_file() || source_metadata.len() > self.policy.max_segment_bytes {
            return Err(DiagnosticsError::UnsafePath(
                "current log segment is not a bounded regular file",
            ));
        }
        if source_metadata.len() == 0 {
            return Ok(());
        }

        let (archive_path, mut archive) = create_archive_file(&self.directory, now)?;
        let copied = std::io::copy(&mut source, &mut archive)
            .map_err(|error| io_error("copy log segment during rotation", &error))?;
        if copied != source_metadata.len() {
            let _ = fs::remove_file(&archive_path);
            return Err(DiagnosticsError::UnsafePath(
                "log segment changed during rotation",
            ));
        }
        archive
            .flush()
            .map_err(|error| io_error("flush rotated log segment", &error))?;
        archive
            .sync_all()
            .map_err(|error| io_error("synchronize rotated log segment", &error))?;
        drop(archive);

        if !same_file_identity(current_path, &source_metadata)? {
            return Err(DiagnosticsError::UnsafePath(
                "current log segment changed during rotation",
            ));
        }
        fs::remove_file(current_path)
            .map_err(|error| io_error("remove rotated current segment", &error))?;
        Ok(())
    }

    fn cleanup_locked(&self, now: SystemTime) -> Result<(), DiagnosticsError> {
        let segments = list_known_segments(&self.directory, self.policy)?;
        let mut retained = Vec::with_capacity(segments.len());
        for segment in segments {
            let expired = now
                .duration_since(segment.modified)
                .is_ok_and(|age| age > self.policy.max_age);
            if expired {
                remove_known_segment(&segment.path)?;
            } else {
                retained.push(segment);
            }
        }
        retained.sort_by(|left, right| right.modified.cmp(&left.modified));
        for segment in retained.into_iter().skip(self.policy.retained_segments) {
            remove_known_segment(&segment.path)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct SegmentEntry {
    path: PathBuf,
    modified: SystemTime,
    oversized: bool,
}

fn list_known_segments(
    directory: &Path,
    policy: RetentionPolicy,
) -> Result<Vec<SegmentEntry>, DiagnosticsError> {
    let entries =
        fs::read_dir(directory).map_err(|error| io_error("list log directory", &error))?;
    let mut known = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= MAX_DIRECTORY_ENTRIES {
            return Err(DiagnosticsError::UnsafePath(
                "log directory contains too many entries",
            ));
        }
        let entry = entry.map_err(|error| io_error("inspect log directory entry", &error))?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if file_name != CURRENT_LOG_FILE && !is_archive_name(file_name) {
            continue;
        }
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| io_error("inspect log segment", &error))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || link_count(&metadata) > 1 {
            return Err(DiagnosticsError::UnsafePath(
                "recognized log segments must be private regular files without hard links",
            ));
        }
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        known.push(SegmentEntry {
            path,
            modified,
            oversized: metadata.len() > policy.max_segment_bytes,
        });
    }
    Ok(known)
}

fn is_archive_name(name: &str) -> bool {
    let Some(body) = name
        .strip_prefix("segment-")
        .and_then(|name| name.strip_suffix(".jsonl"))
    else {
        return false;
    };
    let mut parts = body.split('-');
    let parts_valid = parts
        .by_ref()
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    parts_valid && body.matches('-').count() == 2
}

fn create_archive_file(
    directory: &Path,
    now: SystemTime,
) -> Result<(PathBuf, File), DiagnosticsError> {
    let nanos = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for _ in 0..64 {
        let sequence = ARCHIVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "segment-{nanos:020}-{:010}-{sequence:010}.jsonl",
            std::process::id()
        );
        let path = directory.join(name);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        match options.open(&path) {
            Ok(file) => {
                if let Err(error) = set_private_file_permissions(&file) {
                    let _ = fs::remove_file(&path);
                    return Err(error);
                }
                return Ok((path, file));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error("create rotated log segment", &error)),
        }
    }
    Err(DiagnosticsError::Io {
        operation: "allocate rotated log segment name",
        kind: crate::IoFailureKind::AlreadyExists,
    })
}

fn open_regular_log_file(path: &Path, create: bool) -> Result<File, DiagnosticsError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(DiagnosticsError::UnsafePath(
                "log files must be regular files, not symlinks or special files",
            ));
        }
        Ok(metadata) if link_count(&metadata) > 1 => {
            return Err(DiagnosticsError::UnsafePath(
                "log files with multiple hard links are refused",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => {
            return Err(io_error("open log file", &error));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error("inspect log file", &error)),
    }

    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if create {
        options.append(true).create(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        if create {
            options.mode(0o600);
        }
    }
    let file = options.open(path).map_err(|error| {
        if is_symlink_error(&error) {
            DiagnosticsError::UnsafePath("refused to follow a log file symlink")
        } else {
            io_error("open log file safely", &error)
        }
    })?;
    let metadata = file
        .metadata()
        .map_err(|error| io_error("inspect open log file", &error))?;
    if !metadata.is_file() || link_count(&metadata) > 1 {
        return Err(DiagnosticsError::UnsafePath(
            "log files must be private regular files without hard links",
        ));
    }
    set_private_file_permissions(&file)?;
    Ok(file)
}

fn set_private_file_permissions(file: &File) -> Result<(), DiagnosticsError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| io_error("restrict log file permissions", &error))?;
    }
    Ok(())
}

fn read_bounded_segment(
    path: &Path,
    maximum_bytes: u64,
) -> Result<Option<Vec<u8>>, DiagnosticsError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error("inspect log segment", &error)),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(DiagnosticsError::UnsafePath(
                "refused to read a symlink or special log segment",
            ));
        }
        Ok(metadata) if link_count(&metadata) > 1 => {
            return Err(DiagnosticsError::UnsafePath(
                "refused to read a log segment with multiple hard links",
            ));
        }
        Ok(metadata) if metadata.len() > maximum_bytes => return Ok(Some(Vec::new())),
        Ok(_) => {}
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) if is_symlink_error(&error) => {
            return Err(DiagnosticsError::UnsafePath(
                "refused to follow a log file symlink",
            ));
        }
        Err(error) => return Err(io_error("open log segment safely", &error)),
    };
    let opened_metadata = file
        .metadata()
        .map_err(|error| io_error("inspect open log segment", &error))?;
    if !opened_metadata.is_file() || link_count(&opened_metadata) > 1 {
        return Err(DiagnosticsError::UnsafePath(
            "log segment is not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(maximum_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| io_error("read bounded log segment", &error))?;
    if u64::try_from(bytes.len()).map_or(true, |length| length > maximum_bytes) {
        return Ok(Some(Vec::new()));
    }
    Ok(Some(bytes))
}

fn parse_segment_events(bytes: &[u8], max_event_bytes: usize, result: &mut LogReadResult) {
    if bytes.is_empty() {
        result.omitted_records = result.omitted_records.saturating_add(1);
        return;
    }
    let mut start = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        let line = &bytes[start..index];
        if line.is_empty() || line.len() > max_event_bytes {
            result.omitted_records = result.omitted_records.saturating_add(1);
        } else {
            match serde_json::from_slice::<DiagnosticEvent>(line) {
                Ok(event) if event.validate().is_ok() => result.events.push(event),
                _ => result.omitted_records = result.omitted_records.saturating_add(1),
            }
        }
        start = index + 1;
    }
    if start < bytes.len() {
        // A final partial line may have been interrupted while being written.
        result.omitted_records = result.omitted_records.saturating_add(1);
    }
}

fn remove_known_segment(path: &Path) -> Result<(), DiagnosticsError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| io_error("inspect retained log segment", &error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || link_count(&metadata) > 1 {
        return Err(DiagnosticsError::UnsafePath(
            "refused to remove an unsafe log segment",
        ));
    }
    fs::remove_file(path).map_err(|error| io_error("remove expired log segment", &error))
}

fn same_file_identity(path: &Path, opened: &fs::Metadata) -> Result<bool, DiagnosticsError> {
    let current = fs::symlink_metadata(path)
        .map_err(|error| io_error("verify current log segment", &error))?;
    if current.file_type().is_symlink() || !current.is_file() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(current.dev() == opened.dev() && current.ino() == opened.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = opened;
        Ok(true)
    }
}

#[cfg(unix)]
fn link_count(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink()
}

#[cfg(not(unix))]
fn link_count(_: &fs::Metadata) -> u64 {
    1
}

#[cfg(unix)]
fn is_symlink_error(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(libc::ELOOP)
}

#[cfg(not(unix))]
fn is_symlink_error(_: &std::io::Error) -> bool {
    false
}
