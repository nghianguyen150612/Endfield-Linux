use std::error::Error;
use std::fmt;

use endfield_unix_domain::ValidationError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelemetryError {
    DomainValidation(ValidationError),
    UnknownMetricId(u16),
    InvalidValue {
        field: &'static str,
        reason: &'static str,
    },
    InvalidProviderId,
    MetricUnitMismatch,
    MetricValueKindMismatch,
    ValueOutOfRange,
    InvalidTargetForMetric,
    InvalidObservationState,
    MissingTimestamp,
    ClockDomainMismatch,
    ReversedWindow,
    InvalidPolicyDuration,
    InvalidSamplingMetadata,
    BatchTooLarge {
        count: usize,
        maximum: usize,
    },
    DuplicateObservation,
    UnsupportedSchemaVersion {
        found: u16,
        supported: u16,
    },
    SnapshotReferenceMismatch,
    UnresolvedDeviceReference,
    InvalidSnapshotContext,
    CounterReset,
    CounterWrapIndeterminate,
    ZeroDuration,
    InvalidCounterUnit,
    ConversionOverflow,
    InvalidConversionScale,
    InexactConversion,
}

impl fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DomainValidation(error) => {
                write!(f, "domain contract validation failed: {error}")
            }
            Self::UnknownMetricId(value) => {
                write!(f, "unknown telemetry metric identifier {value}")
            }
            Self::InvalidValue { field, reason } => write!(f, "invalid {field}: {reason}"),
            Self::InvalidProviderId => f.write_str("invalid telemetry provider identifier"),
            Self::MetricUnitMismatch => {
                f.write_str("measurement unit does not match the metric contract")
            }
            Self::MetricValueKindMismatch => {
                f.write_str("measurement value type does not match the metric contract")
            }
            Self::ValueOutOfRange => {
                f.write_str("measurement is outside the metric's permitted range")
            }
            Self::InvalidTargetForMetric => {
                f.write_str("observation target does not match the metric scope")
            }
            Self::InvalidObservationState => {
                f.write_str("observation state and value or diagnostic contradict each other")
            }
            Self::MissingTimestamp => {
                f.write_str("observation requires a wall-clock or monotonic timestamp")
            }
            Self::ClockDomainMismatch => {
                f.write_str("monotonic timestamps belong to different clock domains")
            }
            Self::ReversedWindow => f.write_str("observation window is reversed"),
            Self::InvalidPolicyDuration => {
                f.write_str("freshness policy duration must be greater than zero")
            }
            Self::InvalidSamplingMetadata => f.write_str("sampling quality metadata is invalid"),
            Self::BatchTooLarge { count, maximum } => write!(
                f,
                "telemetry batch has {count} observations; maximum is {maximum}"
            ),
            Self::DuplicateObservation => {
                f.write_str("batch contains indistinguishable observations from the same provider")
            }
            Self::UnsupportedSchemaVersion { found, supported } => write!(
                f,
                "telemetry schema {found} is unsupported (current schema is {supported})"
            ),
            Self::SnapshotReferenceMismatch => {
                f.write_str("device reference belongs to another snapshot context")
            }
            Self::UnresolvedDeviceReference => {
                f.write_str("device reference does not resolve in the validated snapshot context")
            }
            Self::InvalidSnapshotContext => {
                f.write_str("snapshot context is invalid or inconsistent")
            }
            Self::CounterReset => {
                f.write_str("counter reset was detected; a workload delta cannot be calculated")
            }
            Self::CounterWrapIndeterminate => {
                f.write_str("decreasing counter may indicate a reset or an ambiguous wrap")
            }
            Self::ZeroDuration => {
                f.write_str("a rate cannot be calculated over a zero-duration interval")
            }
            Self::InvalidCounterUnit => {
                f.write_str("counter source unit is incompatible with the requested rate")
            }
            Self::ConversionOverflow => f.write_str("checked telemetry conversion overflowed"),
            Self::InvalidConversionScale => {
                f.write_str("conversion multiplier and divisor must be nonzero")
            }
            Self::InexactConversion => {
                f.write_str("conversion would lose precision under the exact policy")
            }
        }
    }
}

impl Error for TelemetryError {}

impl From<ValidationError> for TelemetryError {
    fn from(value: ValidationError) -> Self {
        Self::DomainValidation(value)
    }
}
