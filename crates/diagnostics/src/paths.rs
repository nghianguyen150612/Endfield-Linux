use std::env;
use std::fs::{self, OpenOptions};
use std::path::{Component, Path, PathBuf};

use crate::DiagnosticsError;
use crate::errors::io_error;

const APPLICATION_NAMESPACE: &str = "endfield-unix";
const LOG_DIRECTORY: &str = "logs";

#[derive(Clone, Debug, Default)]
pub struct LinuxDiagnosticsEnvironment {
    pub home: Option<PathBuf>,
    pub state_home: Option<PathBuf>,
}

impl LinuxDiagnosticsEnvironment {
    pub fn from_process() -> Self {
        Self {
            home: env::var_os("HOME").map(PathBuf::from),
            state_home: env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticsPaths {
    log_directory: PathBuf,
}

impl DiagnosticsPaths {
    pub fn discover() -> Result<Self, DiagnosticsError> {
        #[cfg(target_os = "linux")]
        {
            Self::from_linux_environment(&LinuxDiagnosticsEnvironment::from_process())
        }

        #[cfg(not(target_os = "linux"))]
        {
            Err(DiagnosticsError::UnsupportedPlatform)
        }
    }

    pub fn from_linux_environment(
        environment: &LinuxDiagnosticsEnvironment,
    ) -> Result<Self, DiagnosticsError> {
        let state_override = valid_base(environment.state_home.as_deref());
        let state_base = if let Some(path) = state_override {
            path.to_path_buf()
        } else {
            let home = environment
                .home
                .as_deref()
                .filter(|path| valid_base(Some(path)).is_some())
                .ok_or(DiagnosticsError::HomeUnavailable)?;
            home.join(".local").join("state")
        };

        Ok(Self {
            log_directory: state_base.join(APPLICATION_NAMESPACE).join(LOG_DIRECTORY),
        })
    }

    pub fn log_directory(&self) -> &Path {
        &self.log_directory
    }
}

pub(crate) fn validate_explicit_directory(path: &Path) -> Result<(), DiagnosticsError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| component == Component::ParentDir)
    {
        return Err(DiagnosticsError::InvalidPath(
            "directory must be absolute and must not contain `..`",
        ));
    }
    Ok(())
}

pub(crate) fn ensure_log_directory(path: &Path) -> Result<(), DiagnosticsError> {
    validate_explicit_directory(path)?;
    let namespace = path
        .parent()
        .ok_or(DiagnosticsError::InvalidPath("log directory has no parent"))?;
    let state_base = namespace.parent().ok_or(DiagnosticsError::InvalidPath(
        "state directory has no parent",
    ))?;

    fs::create_dir_all(state_base).map_err(|error| io_error("create state directory", &error))?;
    ensure_private_directory(namespace)?;
    ensure_private_directory(path)
}

pub(crate) fn verify_log_directory_if_present(path: &Path) -> Result<bool, DiagnosticsError> {
    validate_explicit_directory(path)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(io_error("inspect log directory", &error)),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(DiagnosticsError::UnsafePath(
                "log directory must be a real directory, not a symlink or special file",
            ));
        }
        Ok(_) => {}
    }
    let namespace = path
        .parent()
        .ok_or(DiagnosticsError::InvalidPath("log directory has no parent"))?;
    match fs::symlink_metadata(namespace) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(DiagnosticsError::UnsafePath(
                "application state directory must be a real directory",
            ));
        }
        Ok(_) => {}
        Err(error) => return Err(io_error("inspect application state directory", &error)),
    }
    open_directory_nofollow(namespace)?;
    open_directory_nofollow(path)?;
    Ok(true)
}

fn ensure_private_directory(path: &Path) -> Result<(), DiagnosticsError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(DiagnosticsError::UnsafePath(
                "application log directories must be real directories",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_private_directory(path)?;
        }
        Err(error) => return Err(io_error("inspect log directory", &error)),
    }

    let directory = open_directory_nofollow(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        directory
            .set_permissions(fs::Permissions::from_mode(0o700))
            .map_err(|error| io_error("restrict log directory permissions", &error))?;
    }
    Ok(())
}

fn create_private_directory(path: &Path) -> Result<(), DiagnosticsError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(path)
                    .map_err(|error| io_error("inspect log directory", &error))?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    Err(DiagnosticsError::UnsafePath(
                        "application log directories must be real directories",
                    ))
                } else {
                    Ok(())
                }
            }
            Err(error) => Err(io_error("create private log directory", &error)),
        }
    }
    #[cfg(not(unix))]
    {
        match fs::create_dir(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(path)
                    .map_err(|error| io_error("inspect log directory", &error))?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    Err(DiagnosticsError::UnsafePath(
                        "application log directories must be real directories",
                    ))
                } else {
                    Ok(())
                }
            }
            Err(error) => Err(io_error("create private log directory", &error)),
        }
    }
}

fn open_directory_nofollow(path: &Path) -> Result<std::fs::File, DiagnosticsError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    options.open(path).map_err(|error| {
        if is_symlink_error(&error) {
            DiagnosticsError::UnsafePath("refused to follow a log directory symlink")
        } else {
            io_error("open log directory safely", &error)
        }
    })
}

fn valid_base(path: Option<&Path>) -> Option<&Path> {
    path.filter(|path| path.is_absolute() && !path.components().any(|c| c == Component::ParentDir))
}

#[cfg(unix)]
fn is_symlink_error(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(libc::ELOOP)
}

#[cfg(not(unix))]
fn is_symlink_error(_: &std::io::Error) -> bool {
    false
}
