use std::ffi::OsString;

use crate::errors::ContractError;
use crate::validation::{MAX_ENVIRONMENT_NAME_BYTES, MAX_ENVIRONMENT_VALUE_BYTES, validate_os_str};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnvironmentName(String);

impl EnvironmentName {
    pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        let mut bytes = value.bytes();
        let first = bytes.next();
        let valid_start = first.is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
        let valid_rest = bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
        if value.len() > MAX_ENVIRONMENT_NAME_BYTES || !valid_start || !valid_rest {
            return Err(ContractError::InvalidIdentifier {
                field: "environment variable",
            });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensitiveValueKind {
    Credential,
    Token,
    AccountRelated,
    SensitivePath,
    OtherSecret,
}

impl SensitiveValueKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Credential => "credential",
            Self::Token => "token",
            Self::AccountRelated => "account-related",
            Self::SensitivePath => "sensitive-path",
            Self::OtherSecret => "other-secret",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueSensitivity {
    Public,
    Sensitive(SensitiveValueKind),
}

impl ValueSensitivity {
    pub const fn is_sensitive(self) -> bool {
        matches!(self, Self::Sensitive(_))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvironmentChange {
    Set {
        name: EnvironmentName,
        value: OsString,
        sensitivity: ValueSensitivity,
    },
    Remove {
        name: EnvironmentName,
    },
    PreserveInherited {
        name: EnvironmentName,
    },
}

impl EnvironmentChange {
    pub fn set(
        name: EnvironmentName,
        value: impl Into<OsString>,
        sensitivity: ValueSensitivity,
    ) -> Result<Self, ContractError> {
        let value = value.into();
        validate_os_str(
            &value,
            "environment value",
            MAX_ENVIRONMENT_VALUE_BYTES,
            true,
        )?;
        Ok(Self::Set {
            name,
            value,
            sensitivity,
        })
    }

    pub const fn remove(name: EnvironmentName) -> Self {
        Self::Remove { name }
    }

    pub const fn preserve_inherited(name: EnvironmentName) -> Self {
        Self::PreserveInherited { name }
    }

    pub const fn name(&self) -> &EnvironmentName {
        match self {
            Self::Set { name, .. } | Self::Remove { name } | Self::PreserveInherited { name } => {
                name
            }
        }
    }

    pub const fn sensitivity(&self) -> ValueSensitivity {
        match self {
            Self::Set { sensitivity, .. } => *sensitivity,
            Self::Remove { .. } | Self::PreserveInherited { .. } => ValueSensitivity::Public,
        }
    }

    pub const fn value(&self) -> Option<&OsString> {
        match self {
            Self::Set { value, .. } => Some(value),
            Self::Remove { .. } | Self::PreserveInherited { .. } => None,
        }
    }

    pub const fn operation(&self) -> &'static str {
        match self {
            Self::Set { .. } => "set",
            Self::Remove { .. } => "remove",
            Self::PreserveInherited { .. } => "preserve-inherited",
        }
    }
}
