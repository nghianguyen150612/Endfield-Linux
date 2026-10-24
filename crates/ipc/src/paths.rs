use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::errors::IpcError;
use crate::transport::connect_bounded;

pub const SOCKET_NAME: &str = "endfield-unixd.sock";
pub const SOCKET_FILE_NAME: &str = SOCKET_NAME;
pub const SOCKET_MODE: u32 = 0o600;
pub const RUNTIME_DIRECTORY_MODE: u32 = 0o700;
const LOCK_FILE_NAME: &str = "endfield-unixd.lock";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePaths {
    runtime_directory: PathBuf,
    application_directory: PathBuf,
    socket_path: PathBuf,
    lock_path: PathBuf,
}

impl RuntimePaths {
    pub fn for_runtime_directory(path: impl Into<PathBuf>) -> Result<Self, IpcError> {
        let runtime_directory = path.into();
        validate_runtime_directory(&runtime_directory)?;
        let application_directory = runtime_directory.join("endfield-unix");
        let socket_path = application_directory.join(SOCKET_NAME);
        let lock_path = application_directory.join(LOCK_FILE_NAME);
        if socket_path.as_os_str().as_encoded_bytes().len() + 1 > 108 {
            return Err(IpcError::SocketPathTooLong);
        }
        Ok(Self {
            runtime_directory,
            application_directory,
            socket_path,
            lock_path,
        })
    }

    pub fn runtime_directory(&self) -> &Path {
        &self.runtime_directory
    }

    pub fn application_directory(&self) -> &Path {
        &self.application_directory
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn prepare_server_directory(&self) -> Result<(), IpcError> {
        match fs::symlink_metadata(&self.application_directory) {
            Ok(metadata) => validate_application_directory(&metadata)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut builder = fs::DirBuilder::new();
                builder.mode(RUNTIME_DIRECTORY_MODE);
                match builder.create(&self.application_directory) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(_) => return Err(IpcError::UnsafeRuntimeDirectory),
                }
                let metadata = fs::symlink_metadata(&self.application_directory)
                    .map_err(|_| IpcError::UnsafeRuntimeDirectory)?;
                validate_application_directory(&metadata)?;
                fs::set_permissions(
                    &self.application_directory,
                    fs::Permissions::from_mode(RUNTIME_DIRECTORY_MODE),
                )
                .map_err(|_| IpcError::UnsafeRuntimeDirectory)?;
            }
            Err(_) => return Err(IpcError::UnsafeRuntimeDirectory),
        }
        Ok(())
    }

    pub fn validate_client_socket(&self) -> Result<(), IpcError> {
        match fs::symlink_metadata(&self.application_directory) {
            Ok(metadata) => validate_application_directory(&metadata)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(IpcError::UnsafeRuntimeDirectory),
        }
        match fs::symlink_metadata(&self.socket_path) {
            Ok(metadata) => {
                use std::os::unix::fs::FileTypeExt;
                if !metadata.file_type().is_socket()
                    || metadata.uid() != effective_uid()
                    || metadata.mode() & 0o077 != 0
                {
                    return Err(IpcError::UnsafeRuntimeDirectory);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(IpcError::UnsafeRuntimeDirectory),
        }
        Ok(())
    }

    pub(crate) fn acquire_instance_lock(&self) -> Result<InstanceLock, IpcError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&self.lock_path)
            .map_err(|_| IpcError::ServerStartup)?;
        let metadata = file
            .metadata()
            .map_err(|_| IpcError::UnsafeRuntimeDirectory)?;
        if !metadata.is_file()
            || metadata.uid() != effective_uid()
            || metadata.nlink() != 1
            || metadata.mode() & 0o077 != 0
        {
            return Err(IpcError::UnsafeRuntimeDirectory);
        }
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| IpcError::UnsafeRuntimeDirectory)?;
        file.try_lock_exclusive().map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                IpcError::InstanceConflict
            } else {
                IpcError::ServerStartup
            }
        })?;
        Ok(InstanceLock(file))
    }

    pub(crate) fn recover_stale_socket(&self) -> Result<(), IpcError> {
        let metadata = match fs::symlink_metadata(&self.socket_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(IpcError::StaleSocketUnsafe),
        };
        use std::os::unix::fs::FileTypeExt;
        if !metadata.file_type().is_socket() || metadata.uid() != effective_uid() {
            return Err(IpcError::StaleSocketUnsafe);
        }

        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
        match connect_bounded(&self.socket_path, deadline) {
            Ok(stream) => {
                drop(stream);
                return Err(IpcError::InstanceConflict);
            }
            Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                if kernel_knows_socket(&self.socket_path).unwrap_or(true) {
                    return Err(IpcError::StaleSocketUnsafe);
                }
            }
            Err(_) => return Err(IpcError::StaleSocketUnsafe),
        }

        let current =
            fs::symlink_metadata(&self.socket_path).map_err(|_| IpcError::StaleSocketUnsafe)?;
        if !same_inode(&metadata, &current)
            || !current.file_type().is_socket()
            || current.uid() != effective_uid()
        {
            return Err(IpcError::StaleSocketUnsafe);
        }
        fs::remove_file(&self.socket_path).map_err(|_| IpcError::StaleSocketUnsafe)
    }

    pub(crate) fn socket_identity(&self) -> Result<SocketIdentity, IpcError> {
        let metadata =
            fs::symlink_metadata(&self.socket_path).map_err(|_| IpcError::ServerStartup)?;
        use std::os::unix::fs::FileTypeExt;
        if !metadata.file_type().is_socket()
            || metadata.uid() != effective_uid()
            || metadata.mode() & 0o077 != 0
        {
            return Err(IpcError::UnsafeRuntimeDirectory);
        }
        Ok(SocketIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            uid: metadata.uid(),
        })
    }

    pub(crate) fn remove_created_socket(&self) -> Result<(), IpcError> {
        let metadata = match fs::symlink_metadata(&self.socket_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(IpcError::ResourceCleanup),
        };
        use std::os::unix::fs::FileTypeExt;
        if !metadata.file_type().is_socket() || metadata.uid() != effective_uid() {
            return Err(IpcError::ResourceCleanup);
        }
        fs::remove_file(&self.socket_path).map_err(|_| IpcError::ResourceCleanup)
    }

    pub(crate) fn remove_owned_socket(&self, identity: SocketIdentity) -> Result<(), IpcError> {
        let metadata = match fs::symlink_metadata(&self.socket_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(IpcError::ResourceCleanup),
        };
        use std::os::unix::fs::FileTypeExt;
        if !metadata.file_type().is_socket()
            || metadata.dev() != identity.device
            || metadata.ino() != identity.inode
            || metadata.uid() != identity.uid
        {
            return Err(IpcError::ResourceCleanup);
        }
        fs::remove_file(&self.socket_path).map_err(|_| IpcError::ResourceCleanup)
    }
}

pub fn resolve_runtime_paths() -> Result<RuntimePaths, IpcError> {
    let runtime =
        std::env::var_os("XDG_RUNTIME_DIR").ok_or(IpcError::RuntimeDirectoryUnavailable)?;
    RuntimePaths::for_runtime_directory(PathBuf::from(runtime))
}

pub(crate) struct InstanceLock(File);

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SocketIdentity {
    device: u64,
    inode: u64,
    uid: u32,
}

fn validate_runtime_directory(path: &Path) -> Result<(), IpcError> {
    if !path.is_absolute() {
        return Err(IpcError::RuntimeDirectoryUnavailable);
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| IpcError::RuntimeDirectoryUnavailable)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != effective_uid()
        || metadata.mode() & 0o077 != 0
    {
        return Err(IpcError::RuntimeDirectoryUnavailable);
    }
    Ok(())
}

fn validate_application_directory(metadata: &fs::Metadata) -> Result<(), IpcError> {
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != effective_uid()
        || metadata.mode() & 0o077 != 0
    {
        return Err(IpcError::UnsafeRuntimeDirectory);
    }
    Ok(())
}

fn kernel_knows_socket(path: &Path) -> io::Result<bool> {
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 socket path"))?;
    let table = fs::read_to_string("/proc/net/unix")?;
    Ok(table.lines().any(|line| {
        line.ends_with(path)
            && line
                .as_bytes()
                .get(line.len().saturating_sub(path.len() + 1))
                == Some(&b' ')
    }))
}

fn same_inode(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

fn effective_uid() -> u32 {
    nix::unistd::Uid::effective().as_raw()
}
