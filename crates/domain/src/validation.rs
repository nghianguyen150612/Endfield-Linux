use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    UnknownIdentifier {
        kind: &'static str,
        value: u64,
    },
    InvalidText {
        field: &'static str,
    },
    InvalidValue {
        field: &'static str,
        reason: &'static str,
    },
    InvalidUnitForCapability {
        capability_id: u32,
        unit_id: u32,
    },
    ValueNotAllowedForAvailability {
        availability: u32,
    },
    MissingDiagnostic {
        capability_id: u32,
        instance_id: Option<u32>,
    },
    MissingProvenance {
        capability_id: u32,
        instance_id: Option<u32>,
    },
    InvalidDiagnosticState {
        category: u8,
        availability: u32,
    },
    InvalidPlatformDistribution,
    UnsupportedSnapshotSchema {
        found: u32,
        supported: u32,
    },
    DuplicateInstance {
        instance_id: u32,
    },
    DuplicateCapability {
        capability_id: u32,
        instance_id: Option<u32>,
    },
    MissingCapabilityRecord {
        capability_id: u32,
        instance_id: Option<u32>,
    },
    CapabilityAvailabilityMismatch {
        capability_id: u32,
        instance_id: Option<u32>,
    },
    CapabilityMeasurementMismatch {
        capability_id: u32,
        instance_id: Option<u32>,
    },
    UnknownInstanceReference {
        instance_id: u32,
    },
    DuplicateProvenance {
        provenance_id: u32,
    },
    UnknownProvenanceReference {
        provenance_id: u32,
    },
    MeasurementOverflow,
    InvalidConversionScale,
    InexactConversion,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownIdentifier { kind, value } => {
                write!(f, "unknown {kind} identifier {value}")
            }
            Self::InvalidText { field } => write!(f, "invalid text for {field}"),
            Self::InvalidValue { field, reason } => write!(f, "invalid {field}: {reason}"),
            Self::InvalidUnitForCapability {
                capability_id,
                unit_id,
            } => write!(
                f,
                "unit {unit_id} is not valid for capability {capability_id}"
            ),
            Self::ValueNotAllowedForAvailability { availability } => write!(
                f,
                "availability {availability} cannot carry a measured value"
            ),
            Self::MissingDiagnostic {
                capability_id,
                instance_id,
            } => write!(
                f,
                "degraded capability {capability_id} at instance {instance_id:?} requires a diagnostic"
            ),
            Self::MissingProvenance {
                capability_id,
                instance_id,
            } => write!(
                f,
                "observed capability {capability_id} at instance {instance_id:?} requires provenance"
            ),
            Self::InvalidDiagnosticState {
                category,
                availability,
            } => write!(
                f,
                "diagnostic category {category} conflicts with availability {availability}"
            ),
            Self::InvalidPlatformDistribution => {
                f.write_str("distribution identity is only valid for Linux")
            }
            Self::UnsupportedSnapshotSchema { found, supported } => write!(
                f,
                "snapshot schema {found} is unsupported (current schema is {supported})"
            ),
            Self::DuplicateInstance { instance_id } => {
                write!(f, "duplicate snapshot instance identifier {instance_id}")
            }
            Self::DuplicateCapability {
                capability_id,
                instance_id,
            } => write!(
                f,
                "duplicate capability {capability_id} for instance {instance_id:?}"
            ),
            Self::MissingCapabilityRecord {
                capability_id,
                instance_id,
            } => write!(
                f,
                "typed inventory is missing capability {capability_id} for instance {instance_id:?}"
            ),
            Self::CapabilityAvailabilityMismatch {
                capability_id,
                instance_id,
            } => write!(
                f,
                "typed inventory availability disagrees with capability {capability_id} for instance {instance_id:?}"
            ),
            Self::CapabilityMeasurementMismatch {
                capability_id,
                instance_id,
            } => write!(
                f,
                "typed inventory measurement disagrees with capability {capability_id} for instance {instance_id:?}"
            ),
            Self::UnknownInstanceReference { instance_id } => {
                write!(f, "capability refers to unknown instance {instance_id}")
            }
            Self::DuplicateProvenance { provenance_id } => {
                write!(f, "duplicate provenance identifier {provenance_id}")
            }
            Self::UnknownProvenanceReference { provenance_id } => {
                write!(f, "capability refers to unknown provenance {provenance_id}")
            }
            Self::MeasurementOverflow => f.write_str("measurement conversion overflowed"),
            Self::InvalidConversionScale => f.write_str("measurement conversion scale is invalid"),
            Self::InexactConversion => f.write_str("measurement conversion would lose precision"),
        }
    }
}

impl Error for ValidationError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationErrors(pub Vec<ValidationError>);

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.0.iter().enumerate() {
            if index != 0 {
                f.write_str("; ")?;
            }
            error.fmt(f)?;
        }
        Ok(())
    }
}

impl Error for ValidationErrors {}

pub(crate) fn validate_text(
    value: &str,
    field: &'static str,
    max_bytes: usize,
) -> Result<(), ValidationError> {
    if value.trim().is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(ValidationError::InvalidText { field });
    }
    Ok(())
}

pub(crate) fn validate_provider_name(value: &str) -> Result<(), ValidationError> {
    validate_text(value, "provider name", 128)?;
    if !value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return Err(ValidationError::InvalidText {
            field: "provider name",
        });
    }
    Ok(())
}
