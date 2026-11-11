//! Descriptor-bound Linux execution for already-authorized ELF artifacts.
//!
//! This low-level adapter does not validate P025/P026/P027 evidence or approve
//! a launch. Callers must do that first and retain the returned child handle.
//! It refuses scripts because Linux `execveat(AT_EMPTY_PATH)` cannot execute a
//! shebang file when its descriptor is close-on-exec without leaking that
//! descriptor into the interpreter.

use std::{
    ffi::{CString, OsString},
    fs::File,
    io,
    os::{
        fd::AsRawFd,
        unix::{ffi::OsStrExt, process::CommandExt},
    },
    path::Path,
    process::{Child, Command, Stdio},
};

use thiserror::Error;

const MAX_ARGUMENTS: usize = 256;
const MAX_ENVIRONMENT: usize = 128;
const MAX_STRING_BYTES: usize = 32 * 1024;

#[derive(Debug, Error)]
pub enum ExecutionError {
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

pub struct BoundExecutable {
    file: File,
    identity: ExecutableIdentity,
}

impl BoundExecutable {
    /// Opens a relative path beneath `root` without following any symlink.
    /// Every intermediate component is opened as a directory with O_NOFOLLOW.
    pub fn open(root: &Path, relative: &Path, expected_uid: u32) -> Result<Self, ExecutionError> {
        let parts: Vec<_> = relative.components().collect();
        if parts.is_empty()
            || parts
                .iter()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(ExecutionError::InvalidPath);
        }
        let mut current = File::open(root).map_err(ExecutionError::Open)?;
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
            || (mode & 0o111) == 0
        {
            return Err(ExecutionError::InvalidExecutable);
        }
        let mut magic = [0u8; 4];
        use std::io::Read;
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
        Ok(Self { file, identity })
    }

    pub fn identity(&self) -> ExecutableIdentity {
        self.identity
    }

    /// Executes only the opened ELF object. A pathname replacement after
    /// `open` cannot redirect this call. The returned `Child` owns and reaps
    /// the direct child; helper descendants are outside this ownership scope.
    pub fn spawn(
        &self,
        arguments: &[OsString],
        environment: &[(OsString, OsString)],
        current_directory: &Path,
    ) -> Result<Child, ExecutionError> {
        if arguments.len() > MAX_ARGUMENTS || environment.len() > MAX_ENVIRONMENT {
            return Err(ExecutionError::Bounds);
        }
        let mut total = 0usize;
        let args = cstrings(arguments, &mut total)?;
        let envs = environment
            .iter()
            .map(|(k, v)| {
                let mut entry = k.as_os_str().as_bytes().to_vec();
                if entry.contains(&b'=') || entry.is_empty() {
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
        let mut command = Command::new("/bin/false");
        command
            .arg("descriptor-bound-exec-sentinel")
            .env_clear()
            .current_dir(current_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: this closure performs only async-signal-safe syscalls and
        // immediately replaces the child image; its buffers are owned by the
        // parent and remain valid across fork.
        unsafe {
            command.pre_exec(move || {
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
        command.spawn().map_err(ExecutionError::Spawn)
    }
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

use std::os::fd::FromRawFd;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
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

    #[test]
    fn executes_the_opened_elf_after_its_path_is_renamed() {
        let dir = tempfile::tempdir().unwrap();
        let original = native_fixture(dir.path());
        let bound = BoundExecutable::open(
            dir.path(),
            Path::new("identity-child"),
            // SAFETY: getuid has no preconditions.
            unsafe { libc::getuid() },
        )
        .unwrap();
        let identity = bound.identity();
        std::fs::rename(&original, dir.path().join("renamed-away")).unwrap();
        std::fs::write(&original, b"replacement").unwrap();
        std::fs::set_permissions(&original, std::fs::Permissions::from_mode(0o700)).unwrap();
        let child = bound
            .spawn(
                &["identity-child".into(), "雪 ; $(literal)".into()],
                &[("P034_MARKER".into(), "controlled".into())],
                dir.path(),
            )
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(23));
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("arg1=雪 ; $(literal)"));
        assert!(stdout.contains("marker=controlled"));
        assert_eq!(identity.inode, bound.identity().inode);
        assert_ne!(std::fs::metadata(original).unwrap().ino(), identity.inode);
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
        assert!(matches!(
            BoundExecutable::open(dir.path(), Path::new("text"), unsafe { libc::getuid() }),
            Err(ExecutionError::UnsupportedFormat)
        ));
        std::os::unix::fs::symlink("text", dir.path().join("link")).unwrap();
        assert!(
            BoundExecutable::open(dir.path(), Path::new("link"), unsafe { libc::getuid() })
                .is_err()
        );
    }
}
