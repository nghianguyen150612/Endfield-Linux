use crate::provenance::{Diagnostic, DiagnosticCategory, ProvenanceId};
use crate::validation::ValidationError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId(u32);

impl InstanceId {
    /// Creates a nonzero snapshot-local device or sensor identifier.
    pub fn new(value: u32) -> Result<Self, ValidationError> {
        if value == 0 {
            return Err(ValidationError::InvalidValue {
                field: "instance identifier",
                reason: "zero represents a platform-wide capability",
            });
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Availability {
    Unknown = 0,
    Unavailable = 1,
    Available = 2,
    Degraded = 3,
}

impl Availability {
    pub const fn abi_id(self) -> u32 {
        match self {
            Self::Unknown => 0,
            Self::Unavailable => 1,
            Self::Available => 2,
            Self::Degraded => 3,
        }
    }
}

impl TryFrom<u32> for Availability {
    type Error = ValidationError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Unknown),
            1 => Ok(Self::Unavailable),
            2 => Ok(Self::Available),
            3 => Ok(Self::Degraded),
            _ => Err(ValidationError::UnknownIdentifier {
                kind: "availability",
                value: u64::from(value),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CapabilityId {
    CpuTopology = 1,
    CpuScheduling = 2,
    CpuFrequencyPower = 3,
    GpuDevice = 10,
    Vram = 11,
    HardwareAcceleration = 12,
    GraphicsDriver = 13,
    VulkanDevice = 14,
    Ram = 20,
    Swap = 21,
    CpuThermal = 30,
    GpuThermal = 31,
    PowerTelemetry = 32,
    Display = 40,
    CompositorSession = 41,
    RuntimeDiscovery = 50,
    OptimizationTools = 51,
}

impl CapabilityId {
    pub const fn abi_id(self) -> u32 {
        match self {
            Self::CpuTopology => 1,
            Self::CpuScheduling => 2,
            Self::CpuFrequencyPower => 3,
            Self::GpuDevice => 10,
            Self::Vram => 11,
            Self::HardwareAcceleration => 12,
            Self::GraphicsDriver => 13,
            Self::VulkanDevice => 14,
            Self::Ram => 20,
            Self::Swap => 21,
            Self::CpuThermal => 30,
            Self::GpuThermal => 31,
            Self::PowerTelemetry => 32,
            Self::Display => 40,
            Self::CompositorSession => 41,
            Self::RuntimeDiscovery => 50,
            Self::OptimizationTools => 51,
        }
    }

    pub const fn accepts_unit(self, unit: MeasurementUnit) -> bool {
        match self {
            Self::CpuTopology
            | Self::GpuDevice
            | Self::VulkanDevice
            | Self::RuntimeDiscovery
            | Self::OptimizationTools => matches!(unit, MeasurementUnit::Count),
            Self::CpuScheduling
            | Self::HardwareAcceleration
            | Self::GraphicsDriver
            | Self::CompositorSession => false,
            Self::CpuFrequencyPower => matches!(
                unit,
                MeasurementUnit::Hertz
                    | MeasurementUnit::Milliwatts
                    | MeasurementUnit::Millivolts
                    | MeasurementUnit::Joules
                    | MeasurementUnit::Millijoules
            ),
            Self::Vram | Self::Ram | Self::Swap => matches!(unit, MeasurementUnit::Bytes),
            Self::CpuThermal | Self::GpuThermal => {
                matches!(unit, MeasurementUnit::MilliCelsius)
            }
            Self::PowerTelemetry => matches!(
                unit,
                MeasurementUnit::Milliwatts
                    | MeasurementUnit::Millivolts
                    | MeasurementUnit::Joules
                    | MeasurementUnit::Millijoules
            ),
            Self::Display => matches!(unit, MeasurementUnit::Pixels),
        }
    }
}

impl TryFrom<u32> for CapabilityId {
    type Error = ValidationError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::CpuTopology),
            2 => Ok(Self::CpuScheduling),
            3 => Ok(Self::CpuFrequencyPower),
            10 => Ok(Self::GpuDevice),
            11 => Ok(Self::Vram),
            12 => Ok(Self::HardwareAcceleration),
            13 => Ok(Self::GraphicsDriver),
            14 => Ok(Self::VulkanDevice),
            20 => Ok(Self::Ram),
            21 => Ok(Self::Swap),
            30 => Ok(Self::CpuThermal),
            31 => Ok(Self::GpuThermal),
            32 => Ok(Self::PowerTelemetry),
            40 => Ok(Self::Display),
            41 => Ok(Self::CompositorSession),
            50 => Ok(Self::RuntimeDiscovery),
            51 => Ok(Self::OptimizationTools),
            _ => Err(ValidationError::UnknownIdentifier {
                kind: "capability",
                value: u64::from(value),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum MeasurementUnit {
    Count = 1,
    Bytes = 2,
    Hertz = 3,
    Pixels = 4,
    MilliFramesPerSecond = 5,
    Milliseconds = 6,
    Microseconds = 7,
    Nanoseconds = 8,
    MilliCelsius = 9,
    Milliwatts = 10,
    Millivolts = 11,
    Joules = 12,
    Millijoules = 13,
}

impl MeasurementUnit {
    pub const fn abi_id(self) -> u32 {
        match self {
            Self::Count => 1,
            Self::Bytes => 2,
            Self::Hertz => 3,
            Self::Pixels => 4,
            Self::MilliFramesPerSecond => 5,
            Self::Milliseconds => 6,
            Self::Microseconds => 7,
            Self::Nanoseconds => 8,
            Self::MilliCelsius => 9,
            Self::Milliwatts => 10,
            Self::Millivolts => 11,
            Self::Joules => 12,
            Self::Millijoules => 13,
        }
    }
}

impl TryFrom<u32> for MeasurementUnit {
    type Error = ValidationError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Count),
            2 => Ok(Self::Bytes),
            3 => Ok(Self::Hertz),
            4 => Ok(Self::Pixels),
            5 => Ok(Self::MilliFramesPerSecond),
            6 => Ok(Self::Milliseconds),
            7 => Ok(Self::Microseconds),
            8 => Ok(Self::Nanoseconds),
            9 => Ok(Self::MilliCelsius),
            10 => Ok(Self::Milliwatts),
            11 => Ok(Self::Millivolts),
            12 => Ok(Self::Joules),
            13 => Ok(Self::Millijoules),
            _ => Err(ValidationError::UnknownIdentifier {
                kind: "measurement unit",
                value: u64::from(value),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasurementValue {
    Unsigned(u64),
    Signed(i64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasurementOrigin {
    Raw,
    Normalized,
    Derived,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measurement {
    value: MeasurementValue,
    unit: MeasurementUnit,
    origin: MeasurementOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbiMeasurement {
    pub value: u64,
    pub unit_id: u32,
}

impl Measurement {
    pub fn unsigned(value: u64, unit: MeasurementUnit, origin: MeasurementOrigin) -> Self {
        Self {
            value: MeasurementValue::Unsigned(value),
            unit,
            origin,
        }
    }

    pub fn signed(
        value: i64,
        unit: MeasurementUnit,
        origin: MeasurementOrigin,
    ) -> Result<Self, ValidationError> {
        if unit != MeasurementUnit::MilliCelsius {
            return Err(ValidationError::InvalidValue {
                field: "signed measurement",
                reason: "only milli-Celsius supports negative values",
            });
        }
        Ok(Self {
            value: MeasurementValue::Signed(value),
            unit,
            origin,
        })
    }

    /// Converts an integer source value without floating point or silent
    /// truncation. Overflow and non-integral results are rejected.
    pub fn checked_convert_unsigned(
        value: u64,
        multiplier: u64,
        divisor: u64,
        unit: MeasurementUnit,
        origin: MeasurementOrigin,
    ) -> Result<Self, ValidationError> {
        if multiplier == 0 || divisor == 0 {
            return Err(ValidationError::InvalidConversionScale);
        }
        let scaled = value
            .checked_mul(multiplier)
            .ok_or(ValidationError::MeasurementOverflow)?;
        if scaled % divisor != 0 {
            return Err(ValidationError::InexactConversion);
        }
        Ok(Self::unsigned(scaled / divisor, unit, origin))
    }

    pub const fn value(self) -> MeasurementValue {
        self.value
    }

    pub const fn unit(self) -> MeasurementUnit {
        self.unit
    }

    pub const fn origin(self) -> MeasurementOrigin {
        self.origin
    }

    pub fn to_abi(self) -> Result<AbiMeasurement, ValidationError> {
        let value = match self.value {
            MeasurementValue::Unsigned(value) => value,
            MeasurementValue::Signed(value) => {
                u64::try_from(value).map_err(|_| ValidationError::InvalidValue {
                    field: "ABI measurement",
                    reason: "the C17 ABI v1 value field is unsigned",
                })?
            }
        };
        Ok(AbiMeasurement {
            value,
            unit_id: self.unit.abi_id(),
        })
    }

    pub fn from_abi(value: u64, unit_id: u32) -> Result<Self, ValidationError> {
        Ok(Self::unsigned(
            value,
            MeasurementUnit::try_from(unit_id)?,
            MeasurementOrigin::Raw,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityRecord {
    pub id: CapabilityId,
    pub availability: Availability,
    pub measurement: Option<Measurement>,
    /// None maps to ABI instance zero, the platform-wide scope.
    pub instance: Option<InstanceId>,
    pub provenance: Option<ProvenanceId>,
    pub diagnostic: Option<Diagnostic>,
}

impl CapabilityRecord {
    pub fn new(id: CapabilityId, availability: Availability) -> Self {
        Self {
            id,
            availability,
            measurement: None,
            instance: None,
            provenance: None,
            diagnostic: None,
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if matches!(
            self.availability,
            Availability::Unknown | Availability::Unavailable
        ) && self.measurement.is_some()
        {
            return Err(ValidationError::ValueNotAllowedForAvailability {
                availability: self.availability.abi_id(),
            });
        }

        if self.availability == Availability::Degraded && self.diagnostic.is_none() {
            return Err(ValidationError::MissingDiagnostic {
                capability_id: self.id.abi_id(),
                instance_id: self.instance.map(InstanceId::get),
            });
        }

        if self.availability != Availability::Unknown && self.provenance.is_none() {
            return Err(ValidationError::MissingProvenance {
                capability_id: self.id.abi_id(),
                instance_id: self.instance.map(InstanceId::get),
            });
        }

        if let Some(measurement) = self.measurement {
            if !self.id.accepts_unit(measurement.unit()) {
                return Err(ValidationError::InvalidUnitForCapability {
                    capability_id: self.id.abi_id(),
                    unit_id: measurement.unit().abi_id(),
                });
            }
        }

        if let Some(diagnostic) = &self.diagnostic {
            diagnostic.validate()?;
            validate_diagnostic_state(diagnostic.category, self.availability)?;
        }
        Ok(())
    }
}

fn validate_diagnostic_state(
    category: DiagnosticCategory,
    availability: Availability,
) -> Result<(), ValidationError> {
    let compatible = match category {
        DiagnosticCategory::UnsupportedCapability => availability == Availability::Unavailable,
        DiagnosticCategory::MissingProvider | DiagnosticCategory::NotYetProbed => {
            availability == Availability::Unknown
        }
        DiagnosticCategory::PermissionDenied
        | DiagnosticCategory::TemporaryFailure
        | DiagnosticCategory::MalformedSourceData
        | DiagnosticCategory::StaleObservation => {
            matches!(availability, Availability::Unknown | Availability::Degraded)
        }
    };
    if compatible {
        Ok(())
    } else {
        Err(ValidationError::InvalidDiagnosticState {
            category: category.code(),
            availability: availability.abi_id(),
        })
    }
}
