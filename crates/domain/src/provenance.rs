use crate::validation::{ValidationError, validate_provider_name, validate_text};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProvenanceId(u32);

impl ProvenanceId {
    pub fn new(value: u32) -> Result<Self, ValidationError> {
        if value == 0 {
            return Err(ValidationError::InvalidValue {
                field: "provenance identifier",
                reason: "zero is reserved",
            });
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    pub id: ProvenanceId,
    pub provider: String,
    /// Unix epoch nanoseconds, when the provider can supply a trustworthy time.
    pub observed_at_unix_nanos: Option<u64>,
}

impl Provenance {
    pub fn new(
        id: ProvenanceId,
        provider: impl Into<String>,
        observed_at_unix_nanos: Option<u64>,
    ) -> Result<Self, ValidationError> {
        let provider = provider.into();
        validate_provider_name(&provider)?;
        Ok(Self {
            id,
            provider,
            observed_at_unix_nanos,
        })
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_provider_name(&self.provider)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagnosticCategory {
    UnsupportedCapability,
    MissingProvider,
    PermissionDenied,
    TemporaryFailure,
    MalformedSourceData,
    StaleObservation,
    NotYetProbed,
}

impl DiagnosticCategory {
    pub const fn code(self) -> u8 {
        match self {
            Self::UnsupportedCapability => 1,
            Self::MissingProvider => 2,
            Self::PermissionDenied => 3,
            Self::TemporaryFailure => 4,
            Self::MalformedSourceData => 5,
            Self::StaleObservation => 6,
            Self::NotYetProbed => 7,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub category: DiagnosticCategory,
    /// Stable provider name when a diagnostic belongs to a particular source.
    pub provider: Option<String>,
    /// A bounded, non-sensitive explanation suitable for local diagnostics.
    pub message: Option<String>,
}

impl Diagnostic {
    pub fn new(
        category: DiagnosticCategory,
        provider: Option<String>,
        message: Option<String>,
    ) -> Result<Self, ValidationError> {
        if let Some(provider) = &provider {
            validate_provider_name(provider)?;
        }
        if let Some(message) = &message {
            validate_text(message, "diagnostic message", 1024)?;
        }
        Ok(Self {
            category,
            provider,
            message,
        })
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if let Some(provider) = &self.provider {
            validate_provider_name(provider)?;
        }
        if let Some(message) = &self.message {
            validate_text(message, "diagnostic message", 1024)?;
        }
        Ok(())
    }
}
