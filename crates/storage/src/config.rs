use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::atomic::{read_regular_file, replace_file};
use crate::errors::io_error;
use crate::paths::{ensure_private_dir, verify_app_dir_if_present};
use crate::validation::validate_config;
use crate::{AppPaths, StorageError};

pub const CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub ui: UiPreferences,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            ui: UiPreferences::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiPreferences {
    #[serde(default)]
    pub theme: ThemePreference,
    #[serde(default)]
    pub language: LanguagePreference,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            theme: ThemePreference::System,
            language: LanguagePreference::System,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguagePreference {
    #[default]
    System,
    #[serde(rename = "en")]
    English,
}

/// A loaded configuration and the exact on-disk revision from which it came.
///
/// Save this document through the same `ConfigStore`. Its private revision
/// provides compare-and-swap conflict detection, including for first-run
/// creation races.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigDocument {
    pub config: AppConfig,
    revision: ConfigRevision,
}

impl ConfigDocument {
    pub fn is_new(&self) -> bool {
        matches!(&self.revision, ConfigRevision::Missing)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ConfigRevision {
    Missing,
    Existing(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveOutcome {
    /// Whether the parent directory was synchronized after the rename.
    /// False means the platform/filesystem does not support that operation.
    pub directory_synced: bool,
}

#[derive(Clone, Debug)]
pub struct ConfigStore {
    path: PathBuf,
    lock_path: PathBuf,
}

impl ConfigStore {
    pub fn new(paths: &AppPaths) -> Self {
        let path = paths.config_file().to_path_buf();
        let lock_path = path.with_extension("toml.lock");
        Self { path, lock_path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Validates a typed configuration against the schema understood by this
    /// storage API.
    pub fn validate(&self, config: &AppConfig) -> Result<(), StorageError> {
        validate_config(config, &self.path)
    }

    /// Loads the current typed configuration without changing the file.
    /// Missing configuration returns validated defaults with a missing-file
    /// revision; parse, permission, and version errors remain distinct.
    pub fn load(&self) -> Result<ConfigDocument, StorageError> {
        verify_app_dir_if_present(self.path.parent().expect("config path always has a parent"))?;
        let Some(bytes) = read_regular_file(&self.path)? else {
            let config = AppConfig::default();
            validate_config(&config, &self.path)?;
            return Ok(ConfigDocument {
                config,
                revision: ConfigRevision::Missing,
            });
        };

        let text = std::str::from_utf8(&bytes).map_err(|_| StorageError::ConfigEncoding {
            path: self.path.clone(),
        })?;
        let config = parse_config(text, &self.path)?;
        validate_config(&config, &self.path)?;
        Ok(ConfigDocument {
            config,
            revision: ConfigRevision::Existing(bytes),
        })
    }

    /// Creates the default configuration only if the file is still missing.
    pub fn initialize(&self) -> Result<ConfigDocument, StorageError> {
        let mut document = self.load()?;
        if document.is_new() {
            match self.save(&mut document) {
                Ok(_) => {}
                Err(StorageError::ConcurrentUpdate { .. }) => document = self.load()?,
                Err(error) => return Err(error),
            }
        }
        Ok(document)
    }

    /// Saves a loaded document using a locked compare-and-swap policy.
    /// A stale document returns `ConcurrentUpdate` instead of losing another
    /// writer's changes.
    pub fn save(&self, document: &mut ConfigDocument) -> Result<SaveOutcome, StorageError> {
        self.validate(&document.config)?;
        let serialized = toml::to_string_pretty(&document.config).map_err(|source| {
            StorageError::ConfigSerialize {
                path: self.path.clone(),
                source,
            }
        })?;
        let serialized = serialized.into_bytes();

        let parent = self.path.parent().expect("config path always has a parent");
        ensure_private_dir(parent)?;
        let lock = self.acquire_writer_lock()?;

        let current = read_regular_file(&self.path)?;
        let matches_revision = match (&document.revision, current.as_ref()) {
            (ConfigRevision::Missing, None) => true,
            (ConfigRevision::Existing(expected), Some(actual)) => expected == actual,
            _ => false,
        };
        if !matches_revision {
            drop(lock);
            return Err(StorageError::ConcurrentUpdate {
                path: self.path.clone(),
            });
        }

        let directory_synced = replace_file(&self.path, &serialized)?;
        document.revision = ConfigRevision::Existing(serialized);
        FileExt::unlock(&lock).map_err(|source| StorageError::ConfigLock {
            path: self.lock_path.clone(),
            source,
        })?;

        Ok(SaveOutcome { directory_synced })
    }

    fn acquire_writer_lock(&self) -> Result<File, StorageError> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
        }

        let lock = options.open(&self.lock_path).map_err(|source| {
            if source.kind() == io::ErrorKind::PermissionDenied {
                StorageError::PermissionDenied {
                    operation: "open configuration writer lock",
                    path: self.lock_path.clone(),
                    source,
                }
            } else {
                StorageError::ConfigLock {
                    path: self.lock_path.clone(),
                    source,
                }
            }
        })?;

        let metadata = lock.metadata().map_err(|source| {
            io_error("inspect configuration writer lock", &self.lock_path, source)
        })?;
        if !metadata.is_file() {
            return Err(StorageError::InvalidPath {
                path: self.lock_path.clone(),
                reason: "configuration writer lock must be a regular file",
            });
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            lock.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|source| {
                    io_error(
                        "restrict configuration writer lock",
                        &self.lock_path,
                        source,
                    )
                })?;
        }

        FileExt::lock_exclusive(&lock).map_err(|source| StorageError::ConfigLock {
            path: self.lock_path.clone(),
            source,
        })?;
        Ok(lock)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    schema_version: Option<u32>,
    #[serde(default)]
    ui: RawUiPreferences,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawUiPreferences {
    theme: Option<String>,
    language: Option<String>,
}

fn parse_config(text: &str, path: &Path) -> Result<AppConfig, StorageError> {
    let parsed: toml::Value = toml::from_str(text).map_err(|source| StorageError::ConfigParse {
        path: path.to_path_buf(),
        source,
    })?;
    let schema_version = parsed
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or_else(|| StorageError::InvalidConfigValue {
            field: "schema_version",
            value: "missing, out of range, or not an integer".to_owned(),
        })?;
    if schema_version != CONFIG_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedConfigVersion {
            path: path.to_path_buf(),
            found: schema_version,
            supported: CONFIG_SCHEMA_VERSION,
        });
    }

    let raw: RawConfig = toml::from_str(text).map_err(|source| StorageError::ConfigParse {
        path: path.to_path_buf(),
        source,
    })?;
    debug_assert_eq!(raw.schema_version, Some(schema_version));

    let theme = match raw.ui.theme.as_deref().unwrap_or("system") {
        "system" => ThemePreference::System,
        "light" => ThemePreference::Light,
        "dark" => ThemePreference::Dark,
        value => {
            return Err(StorageError::InvalidConfigValue {
                field: "ui.theme",
                value: value.to_owned(),
            });
        }
    };
    let language = match raw.ui.language.as_deref().unwrap_or("system") {
        "system" => LanguagePreference::System,
        "en" => LanguagePreference::English,
        value => {
            return Err(StorageError::InvalidConfigValue {
                field: "ui.language",
                value: value.to_owned(),
            });
        }
    };

    Ok(AppConfig {
        schema_version,
        ui: UiPreferences { theme, language },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_text_uses_stable_pretty_toml() {
        let serialized = toml::to_string_pretty(&AppConfig::default()).expect("serialize");
        assert_eq!(
            serialized,
            "schema_version = 1\n\n[ui]\ntheme = \"system\"\nlanguage = \"system\"\n"
        );
    }
}
