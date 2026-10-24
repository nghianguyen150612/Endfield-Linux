use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::StorageError;
use crate::errors::io_error;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Reads a regular file without following a final-component symlink.
pub(crate) fn read_regular_file(path: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(StorageError::InvalidPath {
                path: path.to_path_buf(),
                reason: "expected a regular file and refused a symlink or special file",
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error("inspect file", path, error)),
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }

    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) if is_symlink_error(&error) => {
            return Err(StorageError::InvalidPath {
                path: path.to_path_buf(),
                reason: "refused to follow a symlink",
            });
        }
        Err(error) => return Err(io_error("open file for reading", path, error)),
    };

    if !file
        .metadata()
        .map_err(|error| io_error("inspect open file", path, error))?
        .is_file()
    {
        return Err(StorageError::InvalidPath {
            path: path.to_path_buf(),
            reason: "expected a regular file",
        });
    }

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| io_error("read file", path, error))?;
    Ok(Some(bytes))
}

/// Replaces a file using a same-directory temporary file and atomic rename.
/// The returned boolean is false only when this platform/filesystem does not
/// support synchronizing directory metadata.
pub(crate) fn replace_file(path: &Path, bytes: &[u8]) -> Result<bool, StorageError> {
    replace_file_with(path, bytes, || Ok(()))
}

fn replace_file_with<F>(path: &Path, bytes: &[u8], before_rename: F) -> Result<bool, StorageError>
where
    F: FnOnce() -> io::Result<()>,
{
    let parent = path.parent().ok_or_else(|| StorageError::InvalidPath {
        path: path.to_path_buf(),
        reason: "destination has no parent directory",
    })?;
    let (temp_path, mut file) = create_temp_file(path)?;

    let result = (|| {
        file.write_all(bytes)
            .map_err(|error| io_error("write temporary configuration", &temp_path, error))?;
        file.flush()
            .map_err(|error| io_error("flush temporary configuration", &temp_path, error))?;
        file.sync_all()
            .map_err(|error| io_error("synchronize temporary configuration", &temp_path, error))?;
        drop(file);

        before_rename()
            .map_err(|error| io_error("prepare atomic configuration replacement", path, error))?;

        fs::rename(&temp_path, path)
            .map_err(|error| io_error("atomically replace configuration", path, error))?;
        sync_parent_directory(parent)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    result
}

fn create_temp_file(destination: &Path) -> Result<(PathBuf, File), StorageError> {
    let parent = destination
        .parent()
        .ok_or_else(|| StorageError::InvalidPath {
            path: destination.to_path_buf(),
            reason: "destination has no parent directory",
        })?;
    let basename = destination
        .file_name()
        .ok_or_else(|| StorageError::InvalidPath {
            path: destination.to_path_buf(),
            reason: "destination has no file name",
        })?
        .to_string_lossy();

    for _ in 0..32 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp_path = parent.join(format!(".{basename}.tmp-{}-{sequence}", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }

        match options.open(&temp_path) {
            Ok(file) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Err(error) = file.set_permissions(fs::Permissions::from_mode(0o600)) {
                        let _ = fs::remove_file(&temp_path);
                        return Err(io_error(
                            "restrict temporary configuration permissions",
                            temp_path,
                            error,
                        ));
                    }
                }
                return Ok((temp_path, file));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error("create temporary configuration", temp_path, error)),
        }
    }

    Err(StorageError::Io {
        operation: "allocate a unique temporary configuration name",
        path: destination.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "temporary name retries exhausted",
        ),
    })
}

fn sync_parent_directory(parent: &Path) -> Result<bool, StorageError> {
    #[cfg(unix)]
    {
        match File::open(parent).and_then(|directory| directory.sync_all()) {
            Ok(()) => Ok(true),
            Err(error) if directory_sync_unsupported(&error) => Ok(false),
            Err(error) => Err(io_error(
                "synchronize configuration directory",
                parent,
                error,
            )),
        }
    }

    #[cfg(not(unix))]
    {
        let _ = parent;
        Ok(false)
    }
}

#[cfg(unix)]
fn directory_sync_unsupported(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::Unsupported
        || error.raw_os_error().is_some_and(|code| {
            code == libc::EINVAL || code == libc::ENOTSUP || code == libc::EOPNOTSUPP
        })
}

#[cfg(not(unix))]
fn directory_sync_unsupported(_: &io::Error) -> bool {
    true
}

#[cfg(unix)]
fn is_symlink_error(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ELOOP)
}

#[cfg(not(unix))]
fn is_symlink_error(_: &io::Error) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn failed_pre_replace_write_keeps_previous_file_and_removes_temp() {
        let directory = tempdir().expect("temporary directory");
        let destination = directory.path().join("config.toml");
        let original = b"schema_version = 1\n\n[ui]\ntheme = \"system\"\nlanguage = \"system\"\n";
        fs::write(&destination, original).expect("initial config");

        let result = replace_file_with(&destination, b"new = true\n", || {
            Err(io::Error::other("injected pre-rename failure"))
        });

        assert!(result.is_err());
        assert_eq!(
            fs::read(&destination).expect("old config remains"),
            original
        );
        let leftovers = fs::read_dir(directory.path())
            .expect("directory entries")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .count();
        assert_eq!(leftovers, 0);
    }
}
