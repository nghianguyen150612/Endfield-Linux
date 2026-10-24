use std::env;
use std::fs::{self, OpenOptions};
use std::path::{Component, Path, PathBuf};

use crate::StorageError;
use crate::errors::io_error;

const APPLICATION_NAMESPACE: &str = "endfield-unix";

/// Environment values used by Linux XDG directory resolution.
///
/// This value can be constructed directly by tests and embedders. Relative or
/// parent-traversing XDG overrides are treated as invalid and use the XDG
/// default under `home` instead.
#[derive(Clone, Debug, Default)]
pub struct LinuxDirectoryEnvironment {
    pub home: Option<PathBuf>,
    pub config_home: Option<PathBuf>,
    pub data_home: Option<PathBuf>,
    pub cache_home: Option<PathBuf>,
}

impl LinuxDirectoryEnvironment {
    pub fn from_process() -> Self {
        Self {
            home: env::var_os("HOME").map(PathBuf::from),
            config_home: env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            data_home: env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            cache_home: env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
        }
    }
}

/// Resolved user-scoped paths for this application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    config_file: PathBuf,
    data_dir: PathBuf,
    cache_dir: PathBuf,
}

impl AppPaths {
    /// Resolves Linux XDG paths from the current process environment.
    ///
    /// On other targets this returns an explicit unsupported-platform error;
    /// native macOS and FreeBSD directory conventions can be added behind
    /// this API without changing storage consumers.
    pub fn discover() -> Result<Self, StorageError> {
        #[cfg(target_os = "linux")]
        {
            Self::from_linux_environment(&LinuxDirectoryEnvironment::from_process())
        }

        #[cfg(not(target_os = "linux"))]
        {
            Err(StorageError::UnsupportedPlatform(std::env::consts::OS))
        }
    }

    /// Resolves XDG paths using injectable environment values.
    pub fn from_linux_environment(
        environment: &LinuxDirectoryEnvironment,
    ) -> Result<Self, StorageError> {
        let config_override = valid_base(environment.config_home.as_deref());
        let data_override = valid_base(environment.data_home.as_deref());
        let cache_override = valid_base(environment.cache_home.as_deref());
        let needs_home =
            config_override.is_none() || data_override.is_none() || cache_override.is_none();

        let home = if needs_home {
            let value = environment
                .home
                .as_deref()
                .filter(|path| valid_base(Some(path)).is_some())
                .ok_or(StorageError::HomeUnavailable)?;
            Some(value.to_path_buf())
        } else {
            None
        };

        let config_base = config_override
            .map(Path::to_path_buf)
            .or_else(|| home.as_ref().map(|path| path.join(".config")))
            .ok_or(StorageError::HomeUnavailable)?;
        let data_base = data_override
            .map(Path::to_path_buf)
            .or_else(|| home.as_ref().map(|path| path.join(".local").join("share")))
            .ok_or(StorageError::HomeUnavailable)?;
        let cache_base = cache_override
            .map(Path::to_path_buf)
            .or_else(|| home.as_ref().map(|path| path.join(".cache")))
            .ok_or(StorageError::HomeUnavailable)?;

        Ok(Self {
            config_file: config_base.join(APPLICATION_NAMESPACE).join("config.toml"),
            data_dir: data_base.join(APPLICATION_NAMESPACE),
            cache_dir: cache_base.join(APPLICATION_NAMESPACE),
        })
    }

    /// Resolves explicit absolute base directories, primarily for embedding
    /// applications and isolated tests.
    pub fn from_base_dirs(
        config_base: impl AsRef<Path>,
        data_base: impl AsRef<Path>,
        cache_base: impl AsRef<Path>,
    ) -> Result<Self, StorageError> {
        let config_base = validate_explicit_base(config_base.as_ref())?;
        let data_base = validate_explicit_base(data_base.as_ref())?;
        let cache_base = validate_explicit_base(cache_base.as_ref())?;

        Ok(Self {
            config_file: config_base.join(APPLICATION_NAMESPACE).join("config.toml"),
            data_dir: data_base.join(APPLICATION_NAMESPACE),
            cache_dir: cache_base.join(APPLICATION_NAMESPACE),
        })
    }

    pub fn config_file(&self) -> &Path {
        &self.config_file
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn database_file(&self) -> PathBuf {
        self.data_dir.join("state.sqlite3")
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub(crate) fn ensure_config_dir(&self) -> Result<(), StorageError> {
        ensure_private_dir(
            self.config_file
                .parent()
                .expect("config path always has a parent"),
        )
    }

    pub(crate) fn ensure_data_dir(&self) -> Result<(), StorageError> {
        ensure_private_dir(&self.data_dir)
    }

    pub(crate) fn verify_data_dir_if_present(&self) -> Result<(), StorageError> {
        verify_app_dir_if_present(&self.data_dir)
    }
}

fn valid_base(path: Option<&Path>) -> Option<&Path> {
    path.filter(|path| path.is_absolute() && !path.components().any(|c| c == Component::ParentDir))
}

fn validate_explicit_base(path: &Path) -> Result<PathBuf, StorageError> {
    if let Some(valid) = valid_base(Some(path)) {
        Ok(valid.to_path_buf())
    } else {
        Err(StorageError::InvalidPath {
            path: path.to_path_buf(),
            reason: "base directories must be absolute and may not contain `..`",
        })
    }
}

pub(crate) fn ensure_private_dir(path: &Path) -> Result<(), StorageError> {
    let parent = path.parent().ok_or_else(|| StorageError::InvalidPath {
        path: path.to_path_buf(),
        reason: "application directory has no parent",
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| io_error("create application base directory", parent, error))?;

    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(StorageError::InvalidPath {
                path: path.to_path_buf(),
                reason: "application directory must be a real directory, not a symlink",
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match create_private_directory(path) {
                Ok(()) => {}
                Err(create_error) if create_error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let raced = fs::symlink_metadata(path)
                        .map_err(|error| io_error("inspect application directory", path, error))?;
                    if raced.file_type().is_symlink() || !raced.is_dir() {
                        return Err(StorageError::InvalidPath {
                            path: path.to_path_buf(),
                            reason: "application directory must be a real directory, not a symlink",
                        });
                    }
                }
                Err(error) => {
                    return Err(io_error(
                        "create private application directory",
                        path,
                        error,
                    ));
                }
            }
        }
        Err(error) => return Err(io_error("inspect application directory", path, error)),
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::fs::PermissionsExt;
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|error| {
                if error.raw_os_error() == Some(libc::ELOOP) {
                    StorageError::InvalidPath {
                        path: path.to_path_buf(),
                        reason: "application directory must not be a symlink",
                    }
                } else {
                    io_error("open application directory safely", path, error)
                }
            })?;
        if !directory
            .metadata()
            .map_err(|error| io_error("inspect application directory", path, error))?
            .is_dir()
        {
            return Err(StorageError::InvalidPath {
                path: path.to_path_buf(),
                reason: "application directory must be a directory",
            });
        }
        directory
            .set_permissions(fs::Permissions::from_mode(0o700))
            .map_err(|error| io_error("restrict application directory permissions", path, error))?;
    }

    Ok(())
}

fn create_private_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700).create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

pub(crate) fn verify_app_dir_if_present(path: &Path) -> Result<(), StorageError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error("inspect application directory", path, error)),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(StorageError::InvalidPath {
                path: path.to_path_buf(),
                reason: "application directory must be a real directory, not a symlink",
            });
        }
        Ok(_) => {}
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|error| {
                if error.raw_os_error() == Some(libc::ELOOP) {
                    StorageError::InvalidPath {
                        path: path.to_path_buf(),
                        reason: "application directory must not be a symlink",
                    }
                } else {
                    io_error("open application directory safely", path, error)
                }
            })?;
        if !directory
            .metadata()
            .map_err(|error| io_error("inspect application directory", path, error))?
            .is_dir()
        {
            return Err(StorageError::InvalidPath {
                path: path.to_path_buf(),
                reason: "application directory must be a directory",
            });
        }
    }

    Ok(())
}
