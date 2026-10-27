use std::ffi::OsStr;
use std::path::{Component, Path};

use crate::errors::ContractError;

pub const MAX_IDENTIFIER_BYTES: usize = 96;
pub const MAX_PROVIDER_CAPABILITIES: usize = 64;
pub const MAX_RUNTIME_CANDIDATES: usize = 512;
pub const MAX_ARGUMENTS: usize = 256;
pub const MAX_ARGUMENT_BYTES: usize = 32 * 1024;
pub const MAX_ENVIRONMENT_CHANGES: usize = 128;
pub const MAX_ENVIRONMENT_NAME_BYTES: usize = 128;
pub const MAX_ENVIRONMENT_VALUE_BYTES: usize = 16 * 1024;
pub const MAX_MOUNT_REQUIREMENTS: usize = 64;
pub const MAX_RUNTIME_METADATA_BYTES: usize = 512 * 1024;

pub(crate) fn validate_text(
    value: &str,
    field: &'static str,
    maximum: usize,
) -> Result<(), ContractError> {
    if value.trim().is_empty()
        || value.len() > maximum
        || value.chars().any(char::is_control)
        || value.as_bytes().contains(&0)
    {
        return Err(ContractError::InvalidText { field });
    }
    Ok(())
}

pub(crate) fn validate_identifier(value: &str, field: &'static str) -> Result<(), ContractError> {
    if value.is_empty()
        || value.len() > MAX_IDENTIFIER_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
    {
        return Err(ContractError::InvalidIdentifier { field });
    }
    Ok(())
}

pub(crate) fn validate_os_str(
    value: &OsStr,
    field: &'static str,
    maximum: usize,
    allow_empty: bool,
) -> Result<(), ContractError> {
    let bytes = value.as_encoded_bytes();
    if (!allow_empty && bytes.is_empty()) || bytes.len() > maximum || bytes.contains(&0) {
        return Err(ContractError::InvalidText { field });
    }
    Ok(())
}

pub(crate) fn validate_path(
    path: &Path,
    field: &'static str,
    must_be_absolute: bool,
) -> Result<(), ContractError> {
    validate_os_str(path.as_os_str(), field, 16 * 1024, false)?;
    if must_be_absolute && !path.is_absolute() {
        return Err(ContractError::InvalidValue {
            field,
            reason: "an absolute path is required",
        });
    }
    Ok(())
}

pub(crate) fn contains_parent_component(path: &Path) -> bool {
    path.components()
        .any(|component| component == Component::ParentDir)
}
