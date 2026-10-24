use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{Connection, Error as SqliteError, ErrorCode, OpenFlags, OptionalExtension};

use crate::errors::io_error;
use crate::migrations::apply_pending;
use crate::{AppPaths, StorageError};

pub const DATABASE_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_BUSY_TIMEOUT: Duration = Duration::from_secs(2);

/// The journal mode actually selected by SQLite for this connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DatabaseJournalMode {
    Wal,
    Delete,
    Other(String),
}

/// A synchronized, single-connection SQLite service.
///
/// The connection is protected by a mutex and each public operation holds it
/// only for the duration of its database work. Callers never receive the raw
/// connection or a transaction handle.
pub struct Database {
    path: PathBuf,
    connection: Mutex<Connection>,
    journal_mode: DatabaseJournalMode,
}

impl Database {
    /// Creates the database if needed and applies every supported migration.
    pub fn initialize(paths: &AppPaths) -> Result<Self, StorageError> {
        paths.ensure_data_dir()?;
        let path = paths.database_file();
        prepare_database_file(&path, true)?;
        Self::open_inner(path, true)
    }

    /// Opens an existing database and applies supported pending migrations.
    /// It never creates a missing database or replaces a corrupt one.
    pub fn open(paths: &AppPaths) -> Result<Self, StorageError> {
        let path = paths.database_file();
        paths.verify_data_dir_if_present()?;
        prepare_database_file(&path, false)?;
        Self::open_inner(path, false)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn journal_mode(&self) -> &DatabaseJournalMode {
        &self.journal_mode
    }

    pub fn schema_version(&self) -> Result<u32, StorageError> {
        let connection = self.lock_connection()?;
        read_schema_version(&connection, &self.path)
    }

    pub fn foreign_keys_enabled(&self) -> Result<bool, StorageError> {
        let connection = self.lock_connection()?;
        connection
            .pragma_query_value(None, "foreign_keys", |row| row.get::<_, bool>(0))
            .map_err(|source| classify_sqlite(&self.path, source))
    }

    /// Reads a namespaced storage metadata value.
    pub fn get_metadata(&self, key: &str) -> Result<Option<String>, StorageError> {
        validate_metadata_key(key)?;
        let connection = self.lock_connection()?;
        connection
            .query_row(
                "SELECT value FROM storage_metadata WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|source| classify_sqlite(&self.path, source))
    }

    /// Atomically inserts or replaces a storage metadata value.
    pub fn set_metadata(&self, key: &str, value: &str) -> Result<(), StorageError> {
        validate_metadata_key(key)?;
        if key == "storage_format" {
            return Err(StorageError::InvalidMetadata {
                field: "key",
                reason: "storage_format is reserved by the initial schema",
            });
        }
        if value.len() > 65_536 || value.contains('\0') {
            return Err(StorageError::InvalidMetadata {
                field: "value",
                reason: "must be NUL-free and at most 65536 bytes",
            });
        }

        let mut connection = self.lock_connection()?;
        let transaction = connection
            .transaction()
            .map_err(|source| classify_sqlite(&self.path, source))?;
        transaction
            .execute(
                "INSERT INTO storage_metadata(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (key, value),
            )
            .map_err(|source| classify_sqlite(&self.path, source))?;
        transaction
            .commit()
            .map_err(|source| classify_sqlite(&self.path, source))
    }

    fn open_inner(path: PathBuf, initialize: bool) -> Result<Self, StorageError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW;
        let mut connection = Connection::open_with_flags(&path, flags)
            .map_err(|source| classify_open(&path, source))?;
        connection
            .busy_timeout(DEFAULT_BUSY_TIMEOUT)
            .map_err(|source| classify_sqlite(&path, source))?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(|source| classify_sqlite(&path, source))?;

        let mut version = read_schema_version(&connection, &path)?;
        if version > DATABASE_SCHEMA_VERSION {
            return Err(StorageError::UnsupportedDatabaseVersion {
                path,
                found: version,
                supported: DATABASE_SCHEMA_VERSION,
            });
        }

        restrict_database_file_permissions(&path)?;

        if initialize || version < DATABASE_SCHEMA_VERSION {
            // Configure the journal before a pending migration. SQLite may
            // return a non-WAL mode on filesystems that do not support WAL.
            select_journal_mode(&connection, &path)?;
        }

        if version < DATABASE_SCHEMA_VERSION {
            version = apply_pending(&mut connection, &path)?;
        }
        if version != DATABASE_SCHEMA_VERSION {
            return Err(StorageError::Migration {
                path,
                version: DATABASE_SCHEMA_VERSION,
                source: SqliteError::InvalidQuery,
            });
        }

        let mode = read_journal_mode(&connection, &path)?;
        Ok(Self {
            path,
            connection: Mutex::new(connection),
            journal_mode: mode,
        })
    }

    fn lock_connection(&self) -> Result<MutexGuard<'_, Connection>, StorageError> {
        self.connection
            .lock()
            .map_err(|_| StorageError::DatabaseLockPoisoned)
    }
}

fn prepare_database_file(path: &Path, create: bool) -> Result<(), StorageError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(StorageError::InvalidPath {
                path: path.to_path_buf(),
                reason: "database path must be a regular file, not a symlink or special file",
            })
        }
        Ok(_) => {
            let mut options = OpenOptions::new();
            options.read(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
            }
            let file = options
                .open(path)
                .map_err(|error| io_error("open SQLite database file", path, error))?;
            let _file = file;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => {
            Err(StorageError::DatabaseNotInitialized {
                path: path.to_path_buf(),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => create_database_file(path),
        Err(error) => Err(io_error("inspect database path", path, error)),
    }
}

fn restrict_database_file_permissions(path: &Path) -> Result<(), StorageError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
        let file = options.open(path).map_err(|error| {
            io_error(
                "open SQLite database for permission protection",
                path,
                error,
            )
        })?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| io_error("restrict database file permissions", path, error))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn create_database_file(path: &Path) -> Result<(), StorageError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }

    match options.open(path) {
        Ok(file) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(fs::Permissions::from_mode(0o600))
                    .map_err(|error| io_error("restrict database file permissions", path, error))?;
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            prepare_database_file(path, false)
        }
        Err(error) => Err(io_error("create SQLite database file", path, error)),
    }
}

fn select_journal_mode(
    connection: &Connection,
    path: &Path,
) -> Result<DatabaseJournalMode, StorageError> {
    let selected = connection
        .query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|source| classify_sqlite(path, source))?;
    Ok(parse_journal_mode(selected))
}

fn read_journal_mode(
    connection: &Connection,
    path: &Path,
) -> Result<DatabaseJournalMode, StorageError> {
    let selected = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
        .map_err(|source| classify_sqlite(path, source))?;
    Ok(parse_journal_mode(selected))
}

fn parse_journal_mode(mode: String) -> DatabaseJournalMode {
    match mode.to_ascii_lowercase().as_str() {
        "wal" => DatabaseJournalMode::Wal,
        "delete" => DatabaseJournalMode::Delete,
        _ => DatabaseJournalMode::Other(mode),
    }
}

fn read_schema_version(connection: &Connection, path: &Path) -> Result<u32, StorageError> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
        .map_err(|source| classify_sqlite(path, source))
}

fn validate_metadata_key(key: &str) -> Result<(), StorageError> {
    if key.is_empty()
        || key.len() > 128
        || !key.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.-".contains(&byte)
        })
    {
        return Err(StorageError::InvalidMetadata {
            field: "key",
            reason: "must be 1–128 bytes using lowercase ASCII, digits, `.`, `_`, or `-`",
        });
    }
    Ok(())
}

pub(crate) fn classify_open(path: &Path, source: SqliteError) -> StorageError {
    match source.sqlite_error_code() {
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => {
            StorageError::DatabaseCorrupt {
                path: path.to_path_buf(),
                source,
            }
        }
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => StorageError::DatabaseBusy {
            path: path.to_path_buf(),
            source,
        },
        _ => StorageError::DatabaseOpen {
            path: path.to_path_buf(),
            source,
        },
    }
}

pub(crate) fn classify_sqlite(path: &Path, source: SqliteError) -> StorageError {
    match source.sqlite_error_code() {
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => {
            StorageError::DatabaseCorrupt {
                path: path.to_path_buf(),
                source,
            }
        }
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => StorageError::DatabaseBusy {
            path: path.to_path_buf(),
            source,
        },
        _ => StorageError::Database {
            path: path.to_path_buf(),
            source,
        },
    }
}

pub(crate) fn classify_migration(path: &Path, version: u32, source: SqliteError) -> StorageError {
    match source.sqlite_error_code() {
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => {
            StorageError::DatabaseCorrupt {
                path: path.to_path_buf(),
                source,
            }
        }
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => StorageError::DatabaseBusy {
            path: path.to_path_buf(),
            source,
        },
        _ => StorageError::Migration {
            path: path.to_path_buf(),
            version,
            source,
        },
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn configured_connection_enforces_foreign_key_constraints() {
        let directory = tempdir().expect("temporary directory");
        let paths = AppPaths::from_base_dirs(
            directory.path().join("config"),
            directory.path().join("data"),
            directory.path().join("cache"),
        )
        .expect("test paths");
        let database = Database::initialize(&paths).expect("initialize database");
        let connection = database.lock_connection().expect("locked connection");
        connection
            .execute_batch(
                "CREATE TABLE test_parent (id INTEGER PRIMARY KEY);
                 CREATE TABLE test_child (
                     parent_id INTEGER NOT NULL REFERENCES test_parent(id)
                 );",
            )
            .expect("test foreign key tables");

        assert!(
            connection
                .execute("INSERT INTO test_child(parent_id) VALUES (42)", [])
                .is_err()
        );
    }
}
