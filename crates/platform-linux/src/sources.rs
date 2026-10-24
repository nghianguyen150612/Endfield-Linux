use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::errors::{DataSourceError, DataSourceErrorKind};
use crate::kernel::RawKernelIdentity;
use crate::os_release::MAX_OS_RELEASE_BYTES;

const MAX_SYSTEMD_CONTAINER_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsReleaseFile {
    Etc,
    UsrLib,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerMarker {
    Docker,
    Podman,
    Systemd,
}

impl ContainerMarker {
    pub const fn source_id(self) -> &'static str {
        match self {
            Self::Docker => "linux-dockerenv",
            Self::Podman => "linux-containerenv",
            Self::Systemd => "linux-systemd-container",
        }
    }
}

/// Metadata needed to apply P010's runtime-directory policy without opening or
/// changing the directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeDirectoryMetadata {
    pub is_directory: bool,
    pub is_symlink: bool,
    pub owner_uid: u32,
    pub mode: u32,
}

/// Read-only, narrow source boundary for deterministic platform-probe tests.
/// Implementations cannot request arbitrary file reads through this trait.
pub trait LinuxDataSource: Send + Sync {
    fn read_os_release(&self, file: OsReleaseFile) -> Result<Vec<u8>, DataSourceError>;
    fn kernel_identity(&self) -> Result<RawKernelIdentity, DataSourceError>;
    fn container_marker_present(&self, marker: ContainerMarker) -> Result<bool, DataSourceError>;
    fn runtime_directory_path(&self) -> Result<Option<PathBuf>, DataSourceError>;
    fn runtime_directory_metadata(
        &self,
        path: &Path,
    ) -> Result<RuntimeDirectoryMetadata, DataSourceError>;
    fn effective_uid(&self) -> u32;

    /// Returns a truthful time of observation, if the system clock is usable.
    fn observation_time_unix_nanos(&self) -> Option<u64> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
    }
}

/// Native implementation using fixed Linux metadata sources and safe `nix`
/// wrappers for `uname` and effective UID.
#[derive(Clone, Debug)]
pub struct SystemLinuxDataSource {
    root: PathBuf,
}

impl Default for SystemLinuxDataSource {
    fn default() -> Self {
        Self {
            root: PathBuf::from("/"),
        }
    }
}

impl SystemLinuxDataSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Uses a virtual root while retaining the same fixed relative source list.
    /// This supports isolated filesystem-policy tests and does not enable
    /// general path-based file reads.
    #[doc(hidden)]
    pub fn rooted_at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, absolute: &str) -> PathBuf {
        self.root.join(absolute.trim_start_matches('/'))
    }
}

impl LinuxDataSource for SystemLinuxDataSource {
    fn read_os_release(&self, file: OsReleaseFile) -> Result<Vec<u8>, DataSourceError> {
        let etc = self.path("/etc/os-release");
        let usr_lib = self.path("/usr/lib/os-release");
        match file {
            OsReleaseFile::UsrLib => read_regular_bounded(&usr_lib, MAX_OS_RELEASE_BYTES),
            OsReleaseFile::Etc => match fs::symlink_metadata(&etc).map_err(map_io_error)? {
                metadata if metadata.file_type().is_symlink() => {
                    let target = fs::read_link(&etc).map_err(map_io_error)?;
                    if !is_allowed_os_release_target(&target) {
                        return Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType));
                    }
                    read_regular_bounded(&usr_lib, MAX_OS_RELEASE_BYTES)
                }
                metadata if metadata.is_file() => read_regular_bounded(&etc, MAX_OS_RELEASE_BYTES),
                _ => Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType)),
            },
        }
    }

    fn kernel_identity(&self) -> Result<RawKernelIdentity, DataSourceError> {
        #[cfg(target_os = "linux")]
        {
            let uname = nix::sys::utsname::uname().map_err(|error| {
                let kind = if matches!(error, nix::errno::Errno::EACCES | nix::errno::Errno::EPERM)
                {
                    DataSourceErrorKind::PermissionDenied
                } else {
                    DataSourceErrorKind::Other
                };
                DataSourceError::new(kind)
            })?;
            let to_string = |value: &std::ffi::OsStr| {
                value
                    .to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| DataSourceError::new(DataSourceErrorKind::InvalidUtf8))
            };
            Ok(RawKernelIdentity {
                system_name: to_string(uname.sysname())?,
                release: to_string(uname.release())?,
                machine: to_string(uname.machine())?,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(DataSourceError::new(DataSourceErrorKind::Unsupported))
        }
    }

    fn container_marker_present(&self, marker: ContainerMarker) -> Result<bool, DataSourceError> {
        let path = match marker {
            ContainerMarker::Docker => self.path("/.dockerenv"),
            ContainerMarker::Podman => self.path("/run/.containerenv"),
            ContainerMarker::Systemd => {
                let path = self.path("/run/systemd/container");
                let bytes = match read_regular_bounded(&path, MAX_SYSTEMD_CONTAINER_BYTES) {
                    Ok(bytes) => bytes,
                    Err(error) if error.kind == DataSourceErrorKind::NotFound => return Ok(false),
                    Err(error) => return Err(error),
                };
                let value = std::str::from_utf8(&bytes)
                    .map_err(|_| DataSourceError::new(DataSourceErrorKind::InvalidUtf8))?
                    .trim()
                    .to_ascii_lowercase();
                if value.is_empty() {
                    return Err(DataSourceError::new(DataSourceErrorKind::MalformedData));
                }
                return Ok(matches!(
                    value.as_str(),
                    "docker"
                        | "podman"
                        | "lxc"
                        | "lxc-libvirt"
                        | "systemd-nspawn"
                        | "nspawn"
                        | "openvz"
                        | "rkt"
                ));
            }
        };

        match validate_regular_marker(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind == DataSourceErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn runtime_directory_path(&self) -> Result<Option<PathBuf>, DataSourceError> {
        Ok(std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
    }

    fn runtime_directory_metadata(
        &self,
        path: &Path,
    ) -> Result<RuntimeDirectoryMetadata, DataSourceError> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
        Ok(RuntimeDirectoryMetadata {
            is_directory: metadata.is_dir(),
            is_symlink: metadata.file_type().is_symlink(),
            owner_uid: metadata.uid(),
            mode: metadata.permissions().mode(),
        })
    }

    fn effective_uid(&self) -> u32 {
        #[cfg(unix)]
        {
            nix::unistd::Uid::effective().as_raw()
        }
        #[cfg(not(unix))]
        {
            0
        }
    }
}

fn read_regular_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, DataSourceError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType));
    }
    if metadata.size() > limit as u64 {
        return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(map_io_error)?;
    read_open_file_bounded(file, limit)
}

fn validate_regular_marker(path: &Path) -> Result<(), DataSourceError> {
    use std::os::unix::fs::OpenOptionsExt;

    let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(map_io_error)?;
    if !file.metadata().map_err(map_io_error)?.is_file() {
        return Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType));
    }
    Ok(())
}

fn read_open_file_bounded(file: File, limit: usize) -> Result<Vec<u8>, DataSourceError> {
    if !file.metadata().map_err(map_io_error)?.file_type().is_file() {
        return Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType));
    }
    let mut bytes = Vec::with_capacity(limit.min(4096));
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(map_io_error)?;
    if bytes.len() > limit {
        return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
    }
    Ok(bytes)
}

fn is_allowed_os_release_target(target: &Path) -> bool {
    if target == Path::new("/usr/lib/os-release") {
        return true;
    }
    if target.is_absolute() {
        return false;
    }
    let mut resolved = PathBuf::from("/");
    resolved.push("etc");
    for component in target.components() {
        match component {
            Component::Normal(value) => resolved.push(value),
            Component::CurDir => {}
            Component::ParentDir => {
                if resolved == Path::new("/") {
                    return false;
                }
                resolved.pop();
            }
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    resolved == Path::new("/usr/lib/os-release")
}

fn map_io_error(error: io::Error) -> DataSourceError {
    let kind = match error.kind() {
        io::ErrorKind::NotFound => DataSourceErrorKind::NotFound,
        io::ErrorKind::PermissionDenied => DataSourceErrorKind::PermissionDenied,
        io::ErrorKind::InvalidData => DataSourceErrorKind::InvalidUtf8,
        _ => DataSourceErrorKind::Other,
    };
    DataSourceError::new(kind)
}
