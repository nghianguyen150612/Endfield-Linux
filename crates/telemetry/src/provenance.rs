use endfield_unix_domain::{DiagnosticCategory, MeasurementOrigin, Provenance, ProvenanceId};

use crate::errors::TelemetryError;
use crate::timestamp::WallClockTimestamp;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderId(String);

impl ProviderId {
    /// Applies P007's provider-name validation policy; identifiers cannot be
    /// arbitrary paths, error strings, or environment values.
    pub fn new(value: impl Into<String>) -> Result<Self, TelemetryError> {
        let value = value.into();
        let id = ProvenanceId::new(1).map_err(TelemetryError::from)?;
        Provenance::new(id, value.clone(), None).map_err(|_| TelemetryError::InvalidProviderId)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SamplingContextId(u64);

impl SamplingContextId {
    pub fn new(value: u64) -> Result<Self, TelemetryError> {
        if value == 0 {
            return Err(TelemetryError::InvalidValue {
                field: "sampling-context identifier",
                reason: "zero is reserved",
            });
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotRefId(u64);

impl SnapshotRefId {
    pub fn new(value: u64) -> Result<Self, TelemetryError> {
        if value == 0 {
            return Err(TelemetryError::InvalidValue {
                field: "snapshot-reference identifier",
                reason: "zero is reserved",
            });
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SamplingMethod {
    DirectSensor,
    OperatingSystemInterface,
    HardwareCounter,
    ApplicationInstrumentation,
    Synthetic,
    Derived,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelemetryProvenance {
    provider: ProviderId,
    method: SamplingMethod,
    origin: MeasurementOrigin,
}

impl TelemetryProvenance {
    pub fn new(
        provider: ProviderId,
        method: SamplingMethod,
        origin: MeasurementOrigin,
    ) -> Result<Self, TelemetryError> {
        if (method == SamplingMethod::Derived) != (origin == MeasurementOrigin::Derived) {
            return Err(TelemetryError::InvalidValue {
                field: "measurement origin",
                reason: "derived provenance and P007 derived origin must agree",
            });
        }
        Ok(Self {
            provider,
            method,
            origin,
        })
    }

    pub const fn provider(&self) -> &ProviderId {
        &self.provider
    }

    pub const fn method(&self) -> SamplingMethod {
        self.method
    }

    pub const fn origin(&self) -> MeasurementOrigin {
        self.origin
    }

    /// Produces the compatible P007 provenance record without adding private
    /// details. IDs remain allocated by the owning snapshot or caller.
    pub fn to_domain_provenance(
        &self,
        id: ProvenanceId,
        observed_at: Option<WallClockTimestamp>,
    ) -> Result<Provenance, TelemetryError> {
        Provenance::new(
            id,
            self.provider.as_str().to_owned(),
            observed_at.map(WallClockTimestamp::unix_nanos),
        )
        .map_err(TelemetryError::from)
    }
}

/// A fixed P007 category only. Raw provider error text is intentionally absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SafeDiagnostic {
    category: DiagnosticCategory,
}

impl SafeDiagnostic {
    pub const fn new(category: DiagnosticCategory) -> Self {
        Self { category }
    }

    pub const fn category(self) -> DiagnosticCategory {
        self.category
    }
}
