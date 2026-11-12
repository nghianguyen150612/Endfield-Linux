//! Descriptor-bound Linux execution for already-authorized ELF artifacts.
//!
//! This low-level adapter does not validate P025/P026/P027 evidence or approve
//! a launch. Callers must do that first and retain the returned child handle.
//! It refuses scripts because Linux `execveat(AT_EMPTY_PATH)` cannot execute a
//! shebang file when its descriptor is close-on-exec without leaking that
//! descriptor into the interpreter.

use sha2::{Digest, Sha256};
use std::{
    ffi::{CString, OsString},
    fs::File,
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::{OsStrExt, OsStringExt},
            fs::{FileExt, MetadataExt},
            process::CommandExt,
        },
    },
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use thiserror::Error;

const MAX_ARGUMENTS: usize = 256;
const MAX_ENVIRONMENT: usize = 128;
const MAX_STRING_BYTES: usize = 32 * 1024;
const MAX_EXECUTABLE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_PROGRAM_HEADERS: u16 = 1024;
const MAX_CAPTURED_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_WAIT_DURATION: Duration = Duration::from_secs(24 * 60 * 60);
static NEXT_ATTEMPT: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("trusted runtime root identity or path is invalid")]
    InvalidRoot,
    #[error("runtime execution as root or through mismatched real/effective UIDs is unsupported")]
    RootExecutionUnsupported,
    #[error("executable path is invalid or contains a symlink")]
    InvalidPath,
    #[error("executable is not an owner-controlled regular ELF file")]
    InvalidExecutable,
    #[error("scripts and non-ELF formats are unsupported")]
    UnsupportedFormat,
    #[error("execution arguments or environment exceed bounds")]
    Bounds,
    #[error("descriptor-relative executable open failed: {0}")]
    Open(#[source] io::Error),
    #[error("process creation or exec failed: {0}")]
    Spawn(#[source] io::Error),
    #[error("bounded child output capture could not be established: {0}")]
    Capture(#[source] io::Error),
    #[error("execution was cancelled before child creation")]
    Cancelled,
}

#[derive(Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionAttemptId {
    process_id: u32,
    sequence: u64,
}

impl ExecutionAttemptId {
    pub fn process_id(self) -> u32 {
        self.process_id
    }

    pub fn sequence(self) -> u64 {
        self.sequence
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapturedOutput {
    pub bytes: Vec<u8>,
    pub truncated: bool,
    /// False means a descendant may still hold the stream open after the
    /// directly owned process exited.
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedProcessResult {
    pub attempt_id: ExecutionAttemptId,
    pub status: ExitStatus,
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
}

#[derive(Default)]
struct OutputCapture {
    bytes: Vec<u8>,
    truncated: bool,
    complete: bool,
}

/// Owns the direct child and drains stdout/stderr into fixed-size buffers.
/// Dropping an unobserved owner terminates and reaps only this direct child;
/// helper descendants are not identified or terminated.
pub struct OwnedChild {
    child: Option<Child>,
    attempt_id: ExecutionAttemptId,
    status: Option<ExitStatus>,
    stdout: Arc<Mutex<OutputCapture>>,
    stderr: Arc<Mutex<OutputCapture>>,
}

impl OwnedChild {
    pub fn attempt_id(&self) -> ExecutionAttemptId {
        self.attempt_id
    }

    pub fn child_id(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    /// Requests termination of only the direct child represented by this
    /// owned handle. It does not signal helper processes or a process group.
    pub fn cancel(&mut self) -> Result<(), ExecutionError> {
        if self.try_wait()?.is_some() {
            return Ok(());
        }
        if let Some(child) = self.child.as_mut() {
            if let Err(error) = child.kill() {
                if child.try_wait().map_err(ExecutionError::Spawn)?.is_some() {
                    return Ok(());
                }
                return Err(ExecutionError::Spawn(error));
            }
        }
        Ok(())
    }

    /// Observe completion within `timeout`. `None` means the child remains
    /// owned and active; timeout never means successful completion.
    pub fn wait_for(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<ObservedProcessResult>, ExecutionError> {
        if timeout > MAX_WAIT_DURATION {
            return Err(ExecutionError::Bounds);
        }
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(result) = self.try_wait()? {
                return Ok(Some(result));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(
                Duration::from_millis(10).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }

    pub fn wait(&mut self) -> Result<ObservedProcessResult, ExecutionError> {
        if let Some(status) = self.status {
            self.wait_for_output_completion(Duration::from_millis(250));
            return Ok(self.result(status));
        }
        let child = self.child.as_mut().ok_or_else(|| {
            ExecutionError::Spawn(io::Error::other("owned child handle is unavailable"))
        })?;
        let status = child.wait().map_err(ExecutionError::Spawn)?;
        self.status = Some(status);
        self.wait_for_output_completion(Duration::from_millis(250));
        Ok(self.result(status))
    }

    pub fn try_wait(&mut self) -> Result<Option<ObservedProcessResult>, ExecutionError> {
        if let Some(status) = self.status {
            return Ok(Some(self.result(status)));
        }
        let child = self.child.as_mut().ok_or_else(|| {
            ExecutionError::Spawn(io::Error::other("owned child handle is unavailable"))
        })?;
        let Some(status) = child.try_wait().map_err(ExecutionError::Spawn)? else {
            return Ok(None);
        };
        self.status = Some(status);
        Ok(self.status.map(|status| self.result(status)))
    }

    fn result(&self, status: ExitStatus) -> ObservedProcessResult {
        ObservedProcessResult {
            attempt_id: self.attempt_id,
            status,
            stdout: capture_snapshot(&self.stdout),
            stderr: capture_snapshot(&self.stderr),
        }
    }

    fn wait_for_output_completion(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let stdout_done = self
                .stdout
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .complete;
            let stderr_done = self
                .stderr
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .complete;
            if stdout_done && stderr_done {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.status.is_some() {
            return;
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn capture_snapshot(capture: &Mutex<OutputCapture>) -> CapturedOutput {
    let capture = capture
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    CapturedOutput {
        bytes: capture.bytes.clone(),
        truncated: capture.truncated,
        complete: capture.complete,
    }
}

/// Identity captured from the opened executable object, not a later pathname
/// lookup. The descriptor itself remains open through the parent's lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutableIdentity {
    pub device: u64,
    pub inode: u64,
    pub owner_uid: u32,
    pub mode: u32,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpectedExecutableIdentity {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectoryIdentity {
    pub device: u64,
    pub inode: u64,
    pub owner_uid: u32,
}

/// An absolute directory opened component by component without following
/// symlinks. The expected identity must come from the caller's validated
/// P025/P026 evidence; observing metadata here alone is not authorization.
pub struct TrustedRoot {
    directory: File,
    identity: DirectoryIdentity,
    mode: u32,
    mount_id: u64,
}

impl TrustedRoot {
    pub fn open(
        absolute_path: &Path,
        expected: DirectoryIdentity,
        expected_uid: u32,
    ) -> Result<Self, ExecutionError> {
        // SAFETY: getuid/geteuid have no preconditions.
        let real_uid = unsafe { libc::getuid() };
        // SAFETY: getuid/geteuid have no preconditions.
        let effective_uid = unsafe { libc::geteuid() };
        if real_uid == 0 || effective_uid == 0 || real_uid != effective_uid {
            return Err(ExecutionError::RootExecutionUnsupported);
        }
        if !absolute_path.is_absolute()
            || expected.owner_uid != expected_uid
            || expected_uid != effective_uid
        {
            return Err(ExecutionError::InvalidRoot);
        }
        let mut current = File::open("/").map_err(ExecutionError::Open)?;
        let components: Vec<_> = absolute_path.components().collect();
        let normal_count = components
            .iter()
            .filter(|component| matches!(component, std::path::Component::Normal(_)))
            .count();
        if normal_count == 0 {
            return Err(ExecutionError::InvalidRoot);
        }
        let mut normal_index = 0;
        for component in components {
            let std::path::Component::Normal(name) = component else {
                if matches!(component, std::path::Component::RootDir) {
                    continue;
                }
                return Err(ExecutionError::InvalidRoot);
            };
            let name = CString::new(name.as_bytes()).map_err(|_| ExecutionError::InvalidRoot)?;
            // SAFETY: `name` is one normal path component and `current` is a
            // directory descriptor. O_NOFOLLOW refuses a symlink at each step.
            let fd = unsafe {
                libc::openat(
                    current.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(ExecutionError::Open(io::Error::last_os_error()));
            }
            // SAFETY: openat returned a new owned descriptor.
            current = unsafe { File::from_raw_fd(fd) };
            // SAFETY: `stat` is initialized by fstat on success.
            let mut stat: libc::stat = unsafe { std::mem::zeroed() };
            if unsafe { libc::fstat(current.as_raw_fd(), &mut stat) } != 0 {
                return Err(ExecutionError::Open(io::Error::last_os_error()));
            }
            normal_index += 1;
            let is_final = normal_index == normal_count;
            let owner_safe = if is_final {
                stat.st_uid == expected_uid
            } else {
                stat.st_uid == expected_uid
                    || (stat.st_uid == 0
                        && ((stat.st_mode & 0o022) == 0 || (stat.st_mode & libc::S_ISVTX) != 0))
            };
            let sticky_root_ancestor =
                !is_final && stat.st_uid == 0 && (stat.st_mode & libc::S_ISVTX) != 0;
            if (stat.st_mode & libc::S_IFMT) != libc::S_IFDIR
                || !owner_safe
                || ((stat.st_mode & 0o022) != 0 && !sticky_root_ancestor)
            {
                return Err(ExecutionError::InvalidRoot);
            }
        }
        // SAFETY: `stat` is initialized by fstat on success.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(current.as_raw_fd(), &mut stat) } != 0 {
            return Err(ExecutionError::Open(io::Error::last_os_error()));
        }
        let actual = DirectoryIdentity {
            device: stat.st_dev,
            inode: stat.st_ino,
            owner_uid: stat.st_uid,
        };
        if actual != expected {
            return Err(ExecutionError::InvalidRoot);
        }
        let mount_id = descriptor_mount_id(current.as_raw_fd())?;
        Ok(Self {
            directory: current,
            identity: actual,
            mode: stat.st_mode,
            mount_id,
        })
    }

    pub fn identity(&self) -> DirectoryIdentity {
        self.identity
    }

    fn validate_identity(&self) -> Result<(), ExecutionError> {
        // SAFETY: fstat initializes `stat` on success.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(self.directory.as_raw_fd(), &mut stat) } != 0 {
            return Err(ExecutionError::Open(io::Error::last_os_error()));
        }
        let identity = DirectoryIdentity {
            device: stat.st_dev,
            inode: stat.st_ino,
            owner_uid: stat.st_uid,
        };
        if identity != self.identity
            || stat.st_mode != self.mode
            || (stat.st_mode & libc::S_IFMT) != libc::S_IFDIR
            || descriptor_mount_id(self.directory.as_raw_fd())? != self.mount_id
        {
            return Err(ExecutionError::InvalidRoot);
        }
        Ok(())
    }
}

pub struct BoundExecutable {
    file: File,
    identity: ExecutableIdentity,
    dynamic_linker: Option<OsString>,
    expected_sha256: Option<String>,
    root: TrustedRoot,
}

impl BoundExecutable {
    /// Opens a relative path beneath a previously validated root descriptor
    /// without following symlinks or crossing a mount boundary.
    pub fn open(
        root: &TrustedRoot,
        relative: &Path,
        expected_uid: u32,
    ) -> Result<Self, ExecutionError> {
        if root.identity.owner_uid != expected_uid {
            return Err(ExecutionError::InvalidRoot);
        }
        let parts: Vec<_> = relative.components().collect();
        if parts.is_empty()
            || parts
                .iter()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(ExecutionError::InvalidPath);
        }
        let mut current = root.directory.try_clone().map_err(ExecutionError::Open)?;
        for part in &parts[..parts.len() - 1] {
            let name = CString::new(part.as_os_str().as_bytes())
                .map_err(|_| ExecutionError::InvalidPath)?;
            // SAFETY: `name` is a NUL-terminated component and `current` is an open directory.
            let fd = unsafe {
                libc::openat(
                    current.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(ExecutionError::Open(io::Error::last_os_error()));
            }
            // SAFETY: openat returned a new owned descriptor.
            current = unsafe { File::from_raw_fd(fd) };
            // SAFETY: `stat` is initialized by fstat on success.
            let mut parent_stat: libc::stat = unsafe { std::mem::zeroed() };
            if unsafe { libc::fstat(current.as_raw_fd(), &mut parent_stat) } != 0 {
                return Err(ExecutionError::Open(io::Error::last_os_error()));
            }
            if parent_stat.st_uid != expected_uid
                || (parent_stat.st_mode & 0o022) != 0
                || descriptor_mount_id(current.as_raw_fd())? != root.mount_id
            {
                return Err(ExecutionError::InvalidRoot);
            }
        }
        let last = CString::new(parts.last().unwrap().as_os_str().as_bytes())
            .map_err(|_| ExecutionError::InvalidPath)?;
        // Open relative to the verified final parent directory.
        let fd = unsafe {
            libc::openat(
                current.as_raw_fd(),
                last.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(ExecutionError::Open(io::Error::last_os_error()));
        }
        // SAFETY: openat returned a new owned descriptor.
        let file = unsafe { File::from_raw_fd(fd) };
        // SAFETY: `stat` is initialized by fstat on success.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(file.as_raw_fd(), &mut stat) } != 0 {
            return Err(ExecutionError::Open(io::Error::last_os_error()));
        }
        let mode = stat.st_mode;
        if (mode & libc::S_IFMT) != libc::S_IFREG
            || stat.st_uid != expected_uid
            || (mode & 0o022) != 0
            || (mode & 0o6000) != 0
            || (mode & 0o111) == 0
        {
            return Err(ExecutionError::InvalidExecutable);
        }
        let mut magic = [0u8; 4];
        let mut reader = &file;
        reader
            .read_exact(&mut magic)
            .map_err(ExecutionError::Open)?;
        if magic != *b"\x7fELF" {
            return Err(ExecutionError::UnsupportedFormat);
        }
        let identity = ExecutableIdentity {
            device: stat.st_dev,
            inode: stat.st_ino,
            owner_uid: stat.st_uid,
            mode,
            size: stat.st_size as u64,
        };
        if identity.device != root.identity.device {
            return Err(ExecutionError::InvalidExecutable);
        }
        if descriptor_mount_id(file.as_raw_fd())? != root.mount_id {
            return Err(ExecutionError::InvalidExecutable);
        }
        let dynamic_linker = inspect_elf(&file, identity.size)?;
        let held_root = TrustedRoot {
            directory: root.directory.try_clone().map_err(ExecutionError::Open)?,
            identity: root.identity,
            mode: root.mode,
            mount_id: root.mount_id,
        };
        Ok(Self {
            file,
            identity,
            dynamic_linker,
            expected_sha256: None,
            root: held_root,
        })
    }

    /// Open the exact object identified by a separate integrity inventory and
    /// compare its held contents with that inventory's digest. The evidence
    /// source must itself be authenticated by the P025/P026 validation path.
    pub fn open_matching(
        root: &TrustedRoot,
        relative: &Path,
        expected_uid: u32,
        expected_identity: ExpectedExecutableIdentity,
        expected_sha256: &str,
    ) -> Result<Self, ExecutionError> {
        if expected_sha256.len() != 64
            || !expected_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(ExecutionError::InvalidExecutable);
        }
        let mut executable = Self::open(root, relative, expected_uid)?;
        if executable.identity.device != expected_identity.device
            || executable.identity.inode != expected_identity.inode
            || executable.identity.size != expected_identity.size
            || executable.sha256()? != expected_sha256
        {
            return Err(ExecutionError::InvalidExecutable);
        }
        executable.expected_sha256 = Some(expected_sha256.to_owned());
        Ok(executable)
    }

    pub fn identity(&self) -> ExecutableIdentity {
        self.identity
    }

    /// Returns the ELF PT_INTERP path when this is a dynamically linked ELF.
    /// The interpreter and shared-library closure are outside the descriptor
    /// guarantee for the executable held by this object.
    pub fn dynamic_linker(&self) -> Option<&std::ffi::OsStr> {
        self.dynamic_linker.as_deref()
    }

    /// Hashes the held file object for comparison with P026 artifact evidence.
    /// This catches changes during the read but does not freeze the file or its
    /// dynamic dependencies after the hash completes.
    pub fn sha256(&self) -> Result<String, ExecutionError> {
        if self.identity.size > MAX_EXECUTABLE_BYTES {
            return Err(ExecutionError::Bounds);
        }
        let before = self.file.metadata().map_err(ExecutionError::Open)?;
        let mut offset = 0u64;
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        while offset < self.identity.size {
            let length = ((self.identity.size - offset) as usize).min(buffer.len());
            let read = self
                .file
                .read_at(&mut buffer[..length], offset)
                .map_err(ExecutionError::Open)?;
            if read == 0 {
                return Err(ExecutionError::InvalidExecutable);
            }
            digest.update(&buffer[..read]);
            offset = offset.saturating_add(read as u64);
        }
        let after = self.file.metadata().map_err(ExecutionError::Open)?;
        if before.dev() != after.dev()
            || before.ino() != after.ino()
            || before.mode() != after.mode()
            || before.size() != after.size()
            || before.mtime() != after.mtime()
            || before.mtime_nsec() != after.mtime_nsec()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
        {
            return Err(ExecutionError::InvalidExecutable);
        }
        Ok(format!("{:x}", digest.finalize()))
    }

    fn validate_identity(&self) -> Result<(), ExecutionError> {
        // SAFETY: fstat initializes `stat` on success.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(self.file.as_raw_fd(), &mut stat) } != 0 {
            return Err(ExecutionError::Open(io::Error::last_os_error()));
        }
        if stat.st_dev != self.identity.device
            || stat.st_ino != self.identity.inode
            || stat.st_uid != self.identity.owner_uid
            || stat.st_mode != self.identity.mode
            || stat.st_size as u64 != self.identity.size
            || (stat.st_mode & 0o111) == 0
        {
            return Err(ExecutionError::InvalidExecutable);
        }
        if let Some(expected_sha256) = self.expected_sha256.as_deref() {
            if self.sha256()? != expected_sha256 {
                return Err(ExecutionError::InvalidExecutable);
            }
        }
        Ok(())
    }

    /// Executes only the opened ELF object. A pathname replacement after
    /// `open` cannot redirect this call. The returned owner retains and reaps
    /// the direct child; helper descendants are outside its ownership scope.
    pub fn spawn(
        &self,
        arguments: &[OsString],
        environment: &[(OsString, OsString)],
        current_directory: &TrustedRoot,
    ) -> Result<OwnedChild, ExecutionError> {
        self.spawn_inner(arguments, environment, current_directory, None)
    }

    /// As `spawn`, with a cancellation check immediately before process
    /// creation. A request that arrives after the final check is handled by
    /// the returned owner through its direct-child `cancel` operation.
    pub fn spawn_cancellable(
        &self,
        arguments: &[OsString],
        environment: &[(OsString, OsString)],
        current_directory: &TrustedRoot,
        cancellation: &CancellationToken,
    ) -> Result<OwnedChild, ExecutionError> {
        self.spawn_inner(
            arguments,
            environment,
            current_directory,
            Some(cancellation),
        )
    }

    fn spawn_inner(
        &self,
        arguments: &[OsString],
        environment: &[(OsString, OsString)],
        current_directory: &TrustedRoot,
        cancellation: Option<&CancellationToken>,
    ) -> Result<OwnedChild, ExecutionError> {
        self.validate_identity()?;
        self.root.validate_identity()?;
        current_directory.validate_identity()?;
        if arguments.len() > MAX_ARGUMENTS || environment.len() > MAX_ENVIRONMENT {
            return Err(ExecutionError::Bounds);
        }
        let mut total = 0usize;
        let args = cstrings(arguments, &mut total)?;
        let mut env_names = std::collections::HashSet::new();
        let envs = environment
            .iter()
            .map(|(k, v)| {
                let mut entry = k.as_os_str().as_bytes().to_vec();
                if entry.contains(&b'=') || entry.is_empty() {
                    return Err(ExecutionError::Bounds);
                }
                if !valid_environment_name(k.as_os_str().as_bytes()) {
                    return Err(ExecutionError::Bounds);
                }
                if !env_names.insert(entry.clone()) {
                    return Err(ExecutionError::Bounds);
                }
                entry.push(b'=');
                entry.extend_from_slice(v.as_os_str().as_bytes());
                total = total.saturating_add(entry.len());
                CString::new(entry).map_err(|_| ExecutionError::Bounds)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if total > MAX_STRING_BYTES {
            return Err(ExecutionError::Bounds);
        }
        let argv: Vec<usize> = args
            .iter()
            .map(|s| s.as_ptr() as usize)
            .chain(std::iter::once(0))
            .collect();
        let envp: Vec<usize> = envs
            .iter()
            .map(|s| s.as_ptr() as usize)
            .chain(std::iter::once(0))
            .collect();
        let fd = self.file.as_raw_fd();
        let cwd_fd = current_directory.directory.as_raw_fd();
        let mut command = Command::new("/bin/false");
        command
            .arg("descriptor-bound-exec-sentinel")
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: this closure performs only async-signal-safe syscalls and
        // immediately replaces the child image; its buffers are owned by the
        // parent and remain valid across fork.
        unsafe {
            command.pre_exec(move || {
                if libc::fchdir(cwd_fd) != 0 {
                    return Err(io::Error::last_os_error());
                }
                let empty = b"\0";
                let result = libc::syscall(
                    libc::SYS_execveat,
                    fd,
                    empty.as_ptr() as *const libc::c_char,
                    argv.as_ptr() as *const *const libc::c_char,
                    envp.as_ptr() as *const *const libc::c_char,
                    libc::AT_EMPTY_PATH,
                );
                if result < 0 {
                    return Err(io::Error::last_os_error());
                }
                unreachable!("execveat returned after success")
            });
        }
        if cancellation.is_some_and(CancellationToken::is_cancelled) {
            return Err(ExecutionError::Cancelled);
        }
        let mut child = command.spawn().map_err(ExecutionError::Spawn)?;
        let stdout_capture = Arc::new(Mutex::new(OutputCapture::default()));
        let stderr_capture = Arc::new(Mutex::new(OutputCapture::default()));
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ExecutionError::Capture(io::Error::other(
                "stdout pipe missing",
            )));
        };
        if let Err(error) = start_output_capture(stdout, stdout_capture.clone(), "p034-stdout") {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ExecutionError::Capture(error));
        }
        let Some(stderr) = child.stderr.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ExecutionError::Capture(io::Error::other(
                "stderr pipe missing",
            )));
        };
        if let Err(error) = start_output_capture(stderr, stderr_capture.clone(), "p034-stderr") {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ExecutionError::Capture(error));
        }
        Ok(OwnedChild {
            child: Some(child),
            attempt_id: ExecutionAttemptId {
                process_id: std::process::id(),
                sequence: NEXT_ATTEMPT.fetch_add(1, Ordering::Relaxed),
            },
            status: None,
            stdout: stdout_capture,
            stderr: stderr_capture,
        })
    }
}

fn start_output_capture(
    mut reader: impl Read + Send + 'static,
    capture: Arc<Mutex<OutputCapture>>,
    thread_name: &'static str,
) -> io::Result<()> {
    thread::Builder::new()
        .name(thread_name.into())
        .spawn(move || {
            let mut buffer = [0u8; 8192];
            let reached_eof = loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break true,
                    Ok(read) => {
                        let mut capture = capture
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        let remaining =
                            MAX_CAPTURED_OUTPUT_BYTES.saturating_sub(capture.bytes.len());
                        let copied = remaining.min(read);
                        capture.bytes.extend_from_slice(&buffer[..copied]);
                        capture.truncated |= copied < read;
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => break false,
                }
            };
            capture
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .complete = reached_eof;
        })?;
    Ok(())
}

fn valid_environment_name(entry: &[u8]) -> bool {
    let name = entry;
    if name.is_empty()
        || !(name[0].is_ascii_alphabetic() || name[0] == b'_')
        || !name
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return false;
    }
    // Loader and compatibility variables can replace code or alter Wine's
    // runtime selection. WINEPREFIX is an explicit required binding.
    let upper = name.iter().map(u8::to_ascii_uppercase).collect::<Vec<_>>();
    if upper == b"PATH"
        || upper == b"GCONV_PATH"
        || upper == b"LOCPATH"
        || upper == b"GLIBC_TUNABLES"
        || upper.starts_with(b"LD_")
        || upper.starts_with(b"PROTON")
        || (upper.starts_with(b"WINE") && upper != b"WINEPREFIX")
    {
        return false;
    }
    true
}

fn descriptor_mount_id(fd: libc::c_int) -> Result<u64, ExecutionError> {
    let empty = b"\0";
    // SAFETY: `statx` is initialized by the kernel on success.
    let mut statx: libc::statx = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::statx(
            fd,
            empty.as_ptr().cast(),
            libc::AT_EMPTY_PATH | libc::AT_STATX_SYNC_AS_STAT,
            libc::STATX_MNT_ID,
            &mut statx,
        )
    };
    if result != 0 || statx.stx_mask & libc::STATX_MNT_ID == 0 {
        return Err(ExecutionError::InvalidRoot);
    }
    Ok(statx.stx_mnt_id)
}

fn inspect_elf(file: &File, size: u64) -> Result<Option<OsString>, ExecutionError> {
    let mut header = [0u8; 64];
    if size < header.len() as u64
        || file.read_at(&mut header, 0).map_err(ExecutionError::Open)? != header.len()
        || &header[..4] != b"\x7fELF"
        || header[4] != 2
        || header[5] != 1
        || header[6] != 1
        || u32::from_le_bytes(header[20..24].try_into().unwrap()) != 1
        || u16::from_le_bytes(header[52..54].try_into().unwrap()) != 64
    {
        return Err(ExecutionError::UnsupportedFormat);
    }
    let machine = u16::from_le_bytes([header[18], header[19]]);
    let expected_machine = match std::env::consts::ARCH {
        "x86_64" => 62,
        "aarch64" => 183,
        "riscv64" => 243,
        _ => return Err(ExecutionError::UnsupportedFormat),
    };
    let file_type = u16::from_le_bytes([header[16], header[17]]);
    let program_header_offset = u64::from_le_bytes(header[32..40].try_into().unwrap());
    let program_header_size = u16::from_le_bytes([header[54], header[55]]);
    let program_header_count = u16::from_le_bytes([header[56], header[57]]);
    if machine != expected_machine
        || !matches!(file_type, 2 | 3)
        || program_header_size != 56
        || program_header_count == 0
        || program_header_count > MAX_PROGRAM_HEADERS
        || program_header_offset
            .checked_add(program_header_size as u64 * program_header_count as u64)
            .is_none_or(|end| end > size)
    {
        return Err(ExecutionError::UnsupportedFormat);
    }
    let mut dynamic_linker = None;
    for index in 0..program_header_count {
        let offset = program_header_offset + program_header_size as u64 * index as u64;
        let mut program_header = [0u8; 56];
        if file
            .read_at(&mut program_header, offset)
            .map_err(ExecutionError::Open)?
            != program_header.len()
        {
            return Err(ExecutionError::UnsupportedFormat);
        }
        if u32::from_le_bytes(program_header[..4].try_into().unwrap()) != 3 {
            continue;
        }
        if dynamic_linker.is_some() {
            return Err(ExecutionError::UnsupportedFormat);
        }
        let segment_offset = u64::from_le_bytes(program_header[8..16].try_into().unwrap());
        let segment_size = u64::from_le_bytes(program_header[32..40].try_into().unwrap());
        if !(2..=4096).contains(&segment_size)
            || segment_offset
                .checked_add(segment_size)
                .is_none_or(|end| end > size)
        {
            return Err(ExecutionError::UnsupportedFormat);
        }
        let mut interpreter = vec![0u8; segment_size as usize];
        if file
            .read_at(&mut interpreter, segment_offset)
            .map_err(ExecutionError::Open)?
            != interpreter.len()
            || interpreter.last() != Some(&0)
            || interpreter[..interpreter.len() - 1].contains(&0)
            || interpreter[0] != b'/'
        {
            return Err(ExecutionError::UnsupportedFormat);
        }
        interpreter.pop();
        dynamic_linker = Some(OsString::from_vec(interpreter));
    }
    Ok(dynamic_linker)
}

fn cstrings(values: &[OsString], total: &mut usize) -> Result<Vec<CString>, ExecutionError> {
    values
        .iter()
        .map(|value| {
            let bytes = value.as_os_str().as_bytes();
            *total = total.saturating_add(bytes.len());
            CString::new(bytes).map_err(|_| ExecutionError::Bounds)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        os::unix::fs::{MetadataExt, PermissionsExt},
        path::PathBuf,
    };

    fn native_fixture(directory: &Path) -> PathBuf {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/identity_child.c");
        let executable = directory.join("identity-child");
        let status = Command::new("cc")
            .arg("-std=c17")
            .arg("-O2")
            .arg("-o")
            .arg(&executable)
            .arg(source)
            .status()
            .expect("C compiler is available for native execution fixture");
        assert!(status.success());
        executable
    }

    fn trusted_root(path: &Path) -> TrustedRoot {
        let metadata = std::fs::metadata(path).unwrap();
        TrustedRoot::open(
            path,
            DirectoryIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
                owner_uid: metadata.uid(),
            },
            // SAFETY: getuid has no preconditions.
            unsafe { libc::getuid() },
        )
        .unwrap()
    }

    #[test]
    fn executes_the_opened_elf_after_its_path_is_renamed() {
        let dir = tempfile::tempdir().unwrap();
        let original = native_fixture(dir.path());
        let root = trusted_root(dir.path());
        let bound = BoundExecutable::open(
            &root,
            Path::new("identity-child"),
            // SAFETY: getuid has no preconditions.
            unsafe { libc::getuid() },
        )
        .unwrap();
        let identity = bound.identity();
        // SAFETY: fcntl reads descriptor flags for the held executable.
        let fd_flags = unsafe { libc::fcntl(bound.file.as_raw_fd(), libc::F_GETFD) };
        assert_ne!(fd_flags, -1);
        assert_ne!(fd_flags & libc::FD_CLOEXEC, 0);
        std::fs::rename(&original, dir.path().join("renamed-away")).unwrap();
        std::fs::write(&original, b"replacement").unwrap();
        std::fs::set_permissions(&original, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut child = bound
            .spawn(
                &["identity-child".into(), "雪 ; $(literal)".into()],
                &[("P034_MARKER".into(), "controlled".into())],
                &root,
            )
            .unwrap();
        let output = child.wait().unwrap();
        assert_eq!(output.status.code(), Some(23));
        let stdout = String::from_utf8(output.stdout.bytes).unwrap();
        assert!(stdout.contains("arg1=雪 ; $(literal)"));
        assert!(stdout.contains("marker=controlled"));
        assert_eq!(identity.inode, bound.identity().inode);
        assert_ne!(std::fs::metadata(original).unwrap().ino(), identity.inode);
        assert!(bound.dynamic_linker().is_some());
        assert_eq!(bound.sha256().unwrap().len(), 64);
    }

    #[test]
    fn rejects_non_elf_and_symlink_inputs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("text"), b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(
            dir.path().join("text"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let root = trusted_root(dir.path());
        assert!(matches!(
            BoundExecutable::open(&root, Path::new("text"), unsafe { libc::getuid() }),
            Err(ExecutionError::UnsupportedFormat)
        ));
        std::os::unix::fs::symlink("text", dir.path().join("link")).unwrap();
        assert!(
            BoundExecutable::open(&root, Path::new("link"), unsafe { libc::getuid() }).is_err()
        );

        std::fs::create_dir(dir.path().join("real-parent")).unwrap();
        std::fs::copy(dir.path().join("text"), dir.path().join("real-parent/text")).unwrap();
        std::os::unix::fs::symlink("real-parent", dir.path().join("parent-link")).unwrap();
        assert!(
            BoundExecutable::open(
                &root,
                Path::new("parent-link/text"),
                // SAFETY: getuid has no preconditions.
                unsafe { libc::getuid() },
            )
            .is_err()
        );
    }

    #[test]
    fn trusted_root_rejects_symlink_and_detects_identity_replacement() {
        let parent = tempfile::tempdir().unwrap();
        let root_path = parent.path().join("managed");
        std::fs::create_dir(&root_path).unwrap();
        let identity = std::fs::metadata(&root_path).unwrap();
        let expected = DirectoryIdentity {
            device: identity.dev(),
            inode: identity.ino(),
            owner_uid: identity.uid(),
        };
        let alias = parent.path().join("alias");
        std::os::unix::fs::symlink(&root_path, &alias).unwrap();
        assert!(matches!(
            TrustedRoot::open(&alias, expected, unsafe { libc::getuid() }),
            Err(ExecutionError::Open(_)) | Err(ExecutionError::InvalidRoot)
        ));

        let held = TrustedRoot::open(&root_path, expected, unsafe { libc::getuid() }).unwrap();
        std::fs::rename(&root_path, parent.path().join("old-managed")).unwrap();
        std::fs::create_dir(&root_path).unwrap();
        assert_eq!(held.identity(), expected);
        assert!(matches!(
            TrustedRoot::open(&root_path, expected, unsafe { libc::getuid() }),
            Err(ExecutionError::InvalidRoot)
        ));
    }

    #[test]
    fn trusted_root_rejects_a_group_or_world_writable_ancestor() {
        let parent = tempfile::tempdir().unwrap();
        let writable = parent.path().join("writable");
        let root_path = writable.join("managed");
        std::fs::create_dir(&writable).unwrap();
        std::fs::set_permissions(&writable, std::fs::Permissions::from_mode(0o777)).unwrap();
        std::fs::create_dir(&root_path).unwrap();
        let metadata = std::fs::metadata(&root_path).unwrap();
        let expected = DirectoryIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            owner_uid: metadata.uid(),
        };
        assert!(matches!(
            TrustedRoot::open(&root_path, expected, unsafe { libc::getuid() }),
            Err(ExecutionError::InvalidRoot)
        ));
    }

    #[test]
    fn spawn_rejects_a_changed_held_runtime_root_mode() {
        let dir = tempfile::tempdir().unwrap();
        let executable = native_fixture(dir.path());
        let root = trusted_root(dir.path());
        let bound = BoundExecutable::open(
            &root,
            Path::new("identity-child"),
            // SAFETY: getuid has no preconditions.
            unsafe { libc::getuid() },
        )
        .unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(matches!(
            bound.spawn(&[], &[], &root),
            Err(ExecutionError::InvalidRoot)
        ));
        assert!(executable.exists());
    }

    #[test]
    fn held_root_and_cwd_descriptors_survive_parent_and_path_replacement() {
        let parent = tempfile::tempdir().unwrap();
        let root_path = parent.path().join("runtime");
        std::fs::create_dir(&root_path).unwrap();
        let original = native_fixture(&root_path);
        let root = trusted_root(&root_path);
        let bound = BoundExecutable::open(
            &root,
            Path::new("identity-child"),
            // SAFETY: getuid has no preconditions.
            unsafe { libc::getuid() },
        )
        .unwrap();
        std::fs::rename(&root_path, parent.path().join("runtime-held")).unwrap();
        std::fs::create_dir(&root_path).unwrap();
        std::fs::write(root_path.join("identity-child"), b"replacement").unwrap();
        std::fs::set_permissions(
            root_path.join("identity-child"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        assert!(original.exists());
        let mut child = bound.spawn(&["identity-child".into()], &[], &root).unwrap();
        assert_eq!(child.wait().unwrap().status.code(), Some(23));
    }

    #[test]
    fn opened_executable_survives_intermediate_directory_replacement() {
        let parent = tempfile::tempdir().unwrap();
        let root_path = parent.path().join("runtime");
        let bin_path = root_path.join("bin");
        std::fs::create_dir(&root_path).unwrap();
        std::fs::create_dir(&bin_path).unwrap();
        let fixture = native_fixture(parent.path());
        std::fs::rename(&fixture, bin_path.join("wine")).unwrap();
        let root = trusted_root(&root_path);
        let bound = BoundExecutable::open(
            &root,
            Path::new("bin/wine"),
            // SAFETY: getuid has no preconditions.
            unsafe { libc::getuid() },
        )
        .unwrap();

        std::fs::rename(&bin_path, root_path.join("bin-held")).unwrap();
        std::fs::create_dir(&bin_path).unwrap();
        std::fs::write(bin_path.join("wine"), b"replacement").unwrap();
        std::fs::set_permissions(
            bin_path.join("wine"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();

        let mut child = bound.spawn(&[], &[], &root).unwrap();
        assert_eq!(child.wait().unwrap().status.code(), Some(23));
    }

    #[test]
    fn rejects_changed_permissions_and_malformed_environment_before_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let executable = native_fixture(dir.path());
        let root = trusted_root(dir.path());
        let bound = BoundExecutable::open(
            &root,
            Path::new("identity-child"),
            // SAFETY: getuid has no preconditions.
            unsafe { libc::getuid() },
        )
        .unwrap();
        assert!(matches!(
            bound.spawn(&[], &[("BAD=NAME".into(), "value".into())], &root,),
            Err(ExecutionError::Bounds)
        ));
        assert!(matches!(
            bound.spawn(
                &[],
                &[("LD_PRELOAD".into(), "/tmp/untrusted.so".into())],
                &root,
            ),
            Err(ExecutionError::Bounds)
        ));
        std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            bound.spawn(&[], &[], &root),
            Err(ExecutionError::InvalidExecutable)
        ));
    }

    #[test]
    fn owned_child_captures_output_with_a_fixed_memory_limit() {
        let dir = tempfile::tempdir().unwrap();
        native_fixture(dir.path());
        let root = trusted_root(dir.path());
        let bound = BoundExecutable::open(
            &root,
            Path::new("identity-child"),
            // SAFETY: getuid has no preconditions.
            unsafe { libc::getuid() },
        )
        .unwrap();
        let mut child = bound
            .spawn(
                &["identity-child".into()],
                &[("P034_OUTPUT_BYTES".into(), "200000".into())],
                &root,
            )
            .unwrap();
        let attempt = child.attempt_id();
        let result = child.wait().unwrap();
        assert_eq!(result.attempt_id, attempt);
        assert_eq!(result.status.code(), Some(23));
        assert_eq!(result.stdout.bytes.len(), MAX_CAPTURED_OUTPUT_BYTES);
        assert_eq!(result.stderr.bytes.len(), MAX_CAPTURED_OUTPUT_BYTES);
        assert!(result.stdout.truncated);
        assert!(result.stderr.truncated);
        assert!(result.stdout.complete);
        assert!(result.stderr.complete);
    }

    #[test]
    fn cancellation_before_spawn_is_blocked_and_owned_child_can_be_cancelled() {
        use std::os::unix::process::ExitStatusExt;

        let dir = tempfile::tempdir().unwrap();
        let executable = native_fixture(dir.path());
        let root = trusted_root(dir.path());
        let bound = BoundExecutable::open(
            &root,
            Path::new("identity-child"),
            // SAFETY: getuid has no preconditions.
            unsafe { libc::getuid() },
        )
        .unwrap();
        let marker = dir.path().join("should-not-exist");
        let cancelled = CancellationToken::default();
        cancelled.cancel();
        assert!(matches!(
            bound.spawn_cancellable(
                &[],
                &[("P034_CREATE_MARKER".into(), marker.clone().into_os_string())],
                &root,
                &cancelled,
            ),
            Err(ExecutionError::Cancelled)
        ));
        assert!(!marker.exists());

        let mut child = bound
            .spawn(&[], &[("P034_DELAY_MS".into(), "500".into())], &root)
            .unwrap();
        assert!(child.wait_for(Duration::from_millis(10)).unwrap().is_none());
        assert!(child.child_id().is_some());
        child.cancel().unwrap();
        let result = child.wait().unwrap();
        assert_eq!(result.status.code(), None);
        assert_eq!(result.status.signal(), Some(libc::SIGKILL));
        assert!(executable.exists());
    }

    #[test]
    fn rejects_truncated_elf_headers_and_duplicate_environment_names() {
        let dir = tempfile::tempdir().unwrap();
        let malformed = dir.path().join("malformed");
        std::fs::write(&malformed, b"\x7fELF\x02\x01\x01truncated").unwrap();
        std::fs::set_permissions(&malformed, std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = trusted_root(dir.path());
        assert!(matches!(
            BoundExecutable::open(&root, Path::new("malformed"), unsafe { libc::getuid() }),
            Err(ExecutionError::UnsupportedFormat)
        ));

        let executable = native_fixture(dir.path());
        let bound = BoundExecutable::open(
            &root,
            Path::new("identity-child"),
            // SAFETY: getuid has no preconditions.
            unsafe { libc::getuid() },
        )
        .unwrap();
        assert!(matches!(
            bound.spawn(
                &[],
                &[
                    ("P034_MARKER".into(), "one".into()),
                    ("P034_MARKER".into(), "two".into()),
                ],
                &root,
            ),
            Err(ExecutionError::Bounds)
        ));
        assert!(executable.exists());
    }

    #[test]
    fn open_matching_requires_exact_file_identity_and_digest() {
        let dir = tempfile::tempdir().unwrap();
        native_fixture(dir.path());
        let root = trusted_root(dir.path());
        let opened = BoundExecutable::open(
            &root,
            Path::new("identity-child"),
            // SAFETY: getuid has no preconditions.
            unsafe { libc::getuid() },
        )
        .unwrap();
        let identity = opened.identity();
        let expected = ExpectedExecutableIdentity {
            device: identity.device,
            inode: identity.inode,
            size: identity.size,
        };
        let digest = opened.sha256().unwrap();
        assert!(
            BoundExecutable::open_matching(
                &root,
                Path::new("identity-child"),
                // SAFETY: getuid has no preconditions.
                unsafe { libc::getuid() },
                expected,
                &digest,
            )
            .is_ok()
        );
        assert!(matches!(
            BoundExecutable::open_matching(
                &root,
                Path::new("identity-child"),
                // SAFETY: getuid has no preconditions.
                unsafe { libc::getuid() },
                expected,
                &"0".repeat(64),
            ),
            Err(ExecutionError::InvalidExecutable)
        ));
        assert!(matches!(
            BoundExecutable::open_matching(
                &root,
                Path::new("identity-child"),
                // SAFETY: getuid has no preconditions.
                unsafe { libc::getuid() },
                ExpectedExecutableIdentity {
                    inode: identity.inode.saturating_add(1),
                    ..expected
                },
                &digest,
            ),
            Err(ExecutionError::InvalidExecutable)
        ));
    }

    #[test]
    fn matching_descriptor_rechecks_content_before_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let executable = native_fixture(dir.path());
        let root = trusted_root(dir.path());
        let opened = BoundExecutable::open(
            &root,
            Path::new("identity-child"),
            // SAFETY: getuid has no preconditions.
            unsafe { libc::getuid() },
        )
        .unwrap();
        let identity = opened.identity();
        let expected = ExpectedExecutableIdentity {
            device: identity.device,
            inode: identity.inode,
            size: identity.size,
        };
        let digest = opened.sha256().unwrap();
        let bound = BoundExecutable::open_matching(
            &root,
            Path::new("identity-child"),
            // SAFETY: getuid has no preconditions.
            unsafe { libc::getuid() },
            expected,
            &digest,
        )
        .unwrap();

        let mut replacement = std::fs::OpenOptions::new()
            .write(true)
            .open(executable)
            .unwrap();
        replacement
            .write_all(&vec![0u8; identity.size as usize])
            .unwrap();

        assert!(matches!(
            bound.spawn(&[], &[], &root),
            Err(ExecutionError::InvalidExecutable)
        ));
    }
}
