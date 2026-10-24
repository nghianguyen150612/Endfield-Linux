use crate::{AppConfig, CONFIG_SCHEMA_VERSION, StorageError};

pub(crate) fn validate_config(
    config: &AppConfig,
    path: &std::path::Path,
) -> Result<(), StorageError> {
    if config.schema_version != CONFIG_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedConfigVersion {
            path: path.to_path_buf(),
            found: config.schema_version,
            supported: CONFIG_SCHEMA_VERSION,
        });
    }

    Ok(())
}
