//! Local-first configuration and SQLite persistence for `endfield-unix`.
//!
//! The crate is synchronous and does not depend on GTK. Call potentially
//! blocking APIs from a worker when integrating them with the GUI.

#![forbid(unsafe_code)]

mod atomic;
mod config;
mod database;
mod errors;
mod migrations;
mod paths;
mod validation;

pub use config::{
    AppConfig, CONFIG_SCHEMA_VERSION, ConfigDocument, ConfigStore, LanguagePreference, SaveOutcome,
    ThemePreference, UiPreferences,
};
pub use database::{DATABASE_SCHEMA_VERSION, DEFAULT_BUSY_TIMEOUT, Database, DatabaseJournalMode};
pub use errors::StorageError;
pub use paths::{AppPaths, LinuxDirectoryEnvironment};

/// A ready-to-use pair of configuration and database services.
///
/// `initialize` is an explicit, synchronous operation. It creates private
/// application directories, writes a default configuration only when none
/// exists, and initializes the database. GUI callers must run it away from
/// the GTK main thread.
pub struct Storage {
    config: ConfigStore,
    database: Database,
}

impl Storage {
    /// Creates missing application state, preserving any existing files.
    pub fn initialize(paths: &AppPaths) -> Result<Self, StorageError> {
        paths.ensure_config_dir()?;
        paths.ensure_data_dir()?;

        let config = ConfigStore::new(paths);
        config.initialize()?;
        let database = Database::initialize(paths)?;

        Ok(Self { config, database })
    }

    /// Opens existing storage without creating a missing database or config.
    /// A missing config is represented by validated defaults; a missing
    /// database is an error.
    pub fn open(paths: &AppPaths) -> Result<Self, StorageError> {
        let config = ConfigStore::new(paths);
        let database = Database::open(paths)?;
        Ok(Self { config, database })
    }

    pub fn config(&self) -> &ConfigStore {
        &self.config
    }

    pub fn database(&self) -> &Database {
        &self.database
    }
}
