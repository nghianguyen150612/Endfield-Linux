use std::io;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("the Linux home directory could not be resolved safely")]
    HomeUnavailable,

    #[error("unsupported storage platform: {0}")]
    UnsupportedPlatform(&'static str),

    #[error("invalid storage path {path}: {reason}")]
    InvalidPath { path: PathBuf, reason: &'static str },

    #[error("permission denied while {operation} at {path}: {source}")]
    PermissionDenied {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("I/O error while {operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("configuration at {path} is malformed TOML: {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("configuration at {path} is not valid UTF-8")]
    ConfigEncoding { path: PathBuf },

    #[error("configuration at {path} could not be serialized: {source}")]
    ConfigSerialize {
        path: PathBuf,
        #[source]
        source: toml::ser::Error,
    },

    #[error("invalid configuration value for {field}: {value}")]
    InvalidConfigValue { field: &'static str, value: String },

    #[error(
        "configuration at {path} uses unsupported schema version {found}; supported version is {supported}. Preserve the file and upgrade with a version that understands it."
    )]
    UnsupportedConfigVersion {
        path: PathBuf,
        found: u32,
        supported: u32,
    },

    #[error(
        "configuration at {path} changed since it was loaded; reload it and reapply the intended changes"
    )]
    ConcurrentUpdate { path: PathBuf },

    #[error("database does not exist at {path}; initialize storage explicitly first")]
    DatabaseNotInitialized { path: PathBuf },

    #[error("could not open SQLite database at {path}: {source}")]
    DatabaseOpen {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },

    #[error("SQLite database at {path} is corrupt or is not a database: {source}")]
    DatabaseCorrupt {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },

    #[error("SQLite operation at {path} remained busy past the configured timeout: {source}")]
    DatabaseBusy {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },

    #[error("SQLite operation failed at {path}: {source}")]
    Database {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },

    #[error(
        "database at {path} has unsupported schema version {found}; this build supports through version {supported}. Preserve the database and use a compatible newer build."
    )]
    UnsupportedDatabaseVersion {
        path: PathBuf,
        found: u32,
        supported: u32,
    },

    #[error("migration to database schema version {version} failed at {path}: {source}")]
    Migration {
        path: PathBuf,
        version: u32,
        #[source]
        source: rusqlite::Error,
    },

    #[error("database connection lock was poisoned")]
    DatabaseLockPoisoned,

    #[error("invalid storage metadata {field}: {reason}")]
    InvalidMetadata {
        field: &'static str,
        reason: &'static str,
    },

    #[error("could not lock configuration writer lock at {path}: {source}")]
    ConfigLock {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub(crate) fn io_error(
    operation: &'static str,
    path: impl Into<PathBuf>,
    source: io::Error,
) -> StorageError {
    let path = path.into();
    if source.kind() == io::ErrorKind::PermissionDenied {
        StorageError::PermissionDenied {
            operation,
            path,
            source,
        }
    } else {
        StorageError::Io {
            operation,
            path,
            source,
        }
    }
}
