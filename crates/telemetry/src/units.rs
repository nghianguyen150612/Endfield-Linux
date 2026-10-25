use endfield_unix_domain::{Measurement, MeasurementOrigin, MeasurementUnit, MeasurementValue};

use crate::errors::TelemetryError;

pub const BASIS_POINTS_PER_PERCENT: u64 = 100;
pub const MAX_NORMALIZED_UTILIZATION_BASIS_POINTS: u64 = 10_000;

/// P007 measurement units retain their existing meanings. Basis points is a
/// telemetry-only fixed-point unit: 10,000 represents exactly 100 percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TelemetryUnit {
    Count,
    Bytes,
    Hertz,
    Pixels,
    MilliFramesPerSecond,
    Milliseconds,
    Microseconds,
    Nanoseconds,
    MilliCelsius,
    Milliwatts,
    Millivolts,
    Joules,
    Millijoules,
    BasisPoints,
}

/// A numeric value whose unit is carried with it rather than inferred from a
/// provider or silently attached by a renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemetryMeasurement {
    value: MeasurementValue,
    unit: TelemetryUnit,
}

impl TelemetryMeasurement {
    pub const fn new(value: MeasurementValue, unit: TelemetryUnit) -> Self {
        Self { value, unit }
    }

    pub const fn value(self) -> MeasurementValue {
        self.value
    }

    pub const fn unit(self) -> TelemetryUnit {
        self.unit
    }
}

impl TelemetryUnit {
    pub const fn from_domain_const(unit: MeasurementUnit) -> Self {
        match unit {
            MeasurementUnit::Count => Self::Count,
            MeasurementUnit::Bytes => Self::Bytes,
            MeasurementUnit::Hertz => Self::Hertz,
            MeasurementUnit::MilliFramesPerSecond => Self::MilliFramesPerSecond,
            MeasurementUnit::Milliseconds => Self::Milliseconds,
            MeasurementUnit::Microseconds => Self::Microseconds,
            MeasurementUnit::Nanoseconds => Self::Nanoseconds,
            MeasurementUnit::MilliCelsius => Self::MilliCelsius,
            MeasurementUnit::Milliwatts => Self::Milliwatts,
            MeasurementUnit::Millivolts => Self::Millivolts,
            MeasurementUnit::Joules => Self::Joules,
            MeasurementUnit::Millijoules => Self::Millijoules,
            MeasurementUnit::Pixels => Self::Pixels,
        }
    }

    pub fn from_domain(unit: MeasurementUnit) -> Result<Self, TelemetryError> {
        Ok(Self::from_domain_const(unit))
    }

    pub const fn domain_unit(self) -> Option<MeasurementUnit> {
        match self {
            Self::Count => Some(MeasurementUnit::Count),
            Self::Bytes => Some(MeasurementUnit::Bytes),
            Self::Hertz => Some(MeasurementUnit::Hertz),
            Self::Pixels => Some(MeasurementUnit::Pixels),
            Self::MilliFramesPerSecond => Some(MeasurementUnit::MilliFramesPerSecond),
            Self::Milliseconds => Some(MeasurementUnit::Milliseconds),
            Self::Microseconds => Some(MeasurementUnit::Microseconds),
            Self::Nanoseconds => Some(MeasurementUnit::Nanoseconds),
            Self::MilliCelsius => Some(MeasurementUnit::MilliCelsius),
            Self::Milliwatts => Some(MeasurementUnit::Milliwatts),
            Self::Millivolts => Some(MeasurementUnit::Millivolts),
            Self::Joules => Some(MeasurementUnit::Joules),
            Self::Millijoules => Some(MeasurementUnit::Millijoules),
            Self::BasisPoints => None,
        }
    }

    pub const fn stable_name(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Bytes => "bytes",
            Self::Hertz => "hertz",
            Self::Pixels => "pixels",
            Self::MilliFramesPerSecond => "milli_frames_per_second",
            Self::Milliseconds => "milliseconds",
            Self::Microseconds => "microseconds",
            Self::Nanoseconds => "nanoseconds",
            Self::MilliCelsius => "milli_celsius",
            Self::Milliwatts => "milliwatts",
            Self::Millivolts => "millivolts",
            Self::Joules => "joules",
            Self::Millijoules => "millijoules",
            Self::BasisPoints => "basis_points_of_utilization",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UtilizationScope {
    OneLogicalCpu,
    WholeMachine,
    ProcessPerLogicalCpu,
    GpuEngine,
    TotalGpuDevice,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Utilization {
    basis_points: u64,
    scope: UtilizationScope,
}

impl Utilization {
    pub fn new(basis_points: u64, scope: UtilizationScope) -> Result<Self, TelemetryError> {
        if !matches!(scope, UtilizationScope::ProcessPerLogicalCpu)
            && basis_points > MAX_NORMALIZED_UTILIZATION_BASIS_POINTS
        {
            return Err(TelemetryError::ValueOutOfRange);
        }
        Ok(Self {
            basis_points,
            scope,
        })
    }

    pub fn from_ratio_exact(
        numerator: u64,
        denominator: u64,
        scope: UtilizationScope,
    ) -> Result<Self, TelemetryError> {
        Self::from_ratio(numerator, denominator, scope, false)
    }

    /// Converts a ratio to basis points using round-to-nearest, ties upward.
    pub fn from_ratio_rounded(
        numerator: u64,
        denominator: u64,
        scope: UtilizationScope,
    ) -> Result<Self, TelemetryError> {
        Self::from_ratio(numerator, denominator, scope, true)
    }

    fn from_ratio(
        numerator: u64,
        denominator: u64,
        scope: UtilizationScope,
        round_nearest: bool,
    ) -> Result<Self, TelemetryError> {
        if denominator == 0 {
            return Err(TelemetryError::InvalidConversionScale);
        }
        let scaled = u128::from(numerator)
            .checked_mul(u128::from(MAX_NORMALIZED_UTILIZATION_BASIS_POINTS))
            .ok_or(TelemetryError::ConversionOverflow)?;
        let divisor = u128::from(denominator);
        let quotient = scaled / divisor;
        let remainder = scaled % divisor;
        if !round_nearest && remainder != 0 {
            return Err(TelemetryError::InexactConversion);
        }
        let quotient = if round_nearest && remainder.saturating_mul(2) >= divisor {
            quotient
                .checked_add(1)
                .ok_or(TelemetryError::ConversionOverflow)?
        } else {
            quotient
        };
        let value = u64::try_from(quotient).map_err(|_| TelemetryError::ConversionOverflow)?;
        Self::new(value, scope)
    }

    pub const fn basis_points(self) -> u64 {
        self.basis_points
    }

    pub const fn scope(self) -> UtilizationScope {
        self.scope
    }

    pub const fn percent_whole(self) -> u64 {
        self.basis_points / BASIS_POINTS_PER_PERCENT
    }
}

/// Checked integer conversion. Non-integral results are rejected; callers
/// that need rounding must choose and document a separate rounding policy.
pub fn checked_convert_unsigned(
    value: u64,
    multiplier: u64,
    divisor: u64,
) -> Result<u64, TelemetryError> {
    if multiplier == 0 || divisor == 0 {
        return Err(TelemetryError::InvalidConversionScale);
    }
    let scaled = u128::from(value) * u128::from(multiplier);
    let divisor = u128::from(divisor);
    if scaled % divisor != 0 {
        return Err(TelemetryError::InexactConversion);
    }
    u64::try_from(scaled / divisor).map_err(|_| TelemetryError::ConversionOverflow)
}

/// Maps a telemetry value to the authoritative P007 measurement when the
/// telemetry unit exists in that registry. Basis points intentionally remain
/// telemetry-only and return `Ok(None)`.
pub fn to_domain_measurement(
    value: MeasurementValue,
    unit: TelemetryUnit,
    origin: MeasurementOrigin,
) -> Result<Option<Measurement>, TelemetryError> {
    let Some(domain_unit) = unit.domain_unit() else {
        return Ok(None);
    };
    let measurement = match value {
        MeasurementValue::Unsigned(value) => Measurement::unsigned(value, domain_unit, origin),
        MeasurementValue::Signed(value) => Measurement::signed(value, domain_unit, origin)?,
    };
    Ok(Some(measurement))
}

/// Converts a source integer into P007 units without floating point or
/// truncation, then keeps the domain `Measurement` as the returned value.
pub fn checked_convert_to_domain(
    value: u64,
    multiplier: u64,
    divisor: u64,
    unit: MeasurementUnit,
    origin: MeasurementOrigin,
) -> Result<Measurement, TelemetryError> {
    let value = checked_convert_unsigned(value, multiplier, divisor)?;
    Ok(Measurement::unsigned(value, unit, origin))
}

/// Converts event counts to milli-frames per second. Exact mode rejects a
/// fractional milli-FPS result; nearest mode rounds ties upward.
pub fn checked_rate_from_counter(
    frame_delta: u64,
    elapsed_nanos: u64,
    rounding: crate::validation::RateRounding,
) -> Result<u64, TelemetryError> {
    if elapsed_nanos == 0 {
        return Err(TelemetryError::ZeroDuration);
    }
    let scaled = u128::from(frame_delta)
        .checked_mul(1_000_000_000_000)
        .ok_or(TelemetryError::ConversionOverflow)?;
    let denominator = u128::from(elapsed_nanos);
    let quotient = scaled / denominator;
    let remainder = scaled % denominator;
    if rounding == crate::validation::RateRounding::Exact && remainder != 0 {
        return Err(TelemetryError::InexactConversion);
    }
    let quotient = if rounding == crate::validation::RateRounding::Nearest
        && remainder.saturating_mul(2) >= denominator
    {
        quotient
            .checked_add(1)
            .ok_or(TelemetryError::ConversionOverflow)?
    } else {
        quotient
    };
    u64::try_from(quotient).map_err(|_| TelemetryError::ConversionOverflow)
}

/// Validates the source counter unit before deriving a frame rate.
pub fn checked_rate_measurement_from_counter(
    delta: Measurement,
    elapsed_nanos: u64,
    rounding: crate::validation::RateRounding,
) -> Result<Measurement, TelemetryError> {
    if delta.unit() != MeasurementUnit::Count {
        return Err(TelemetryError::InvalidCounterUnit);
    }
    let MeasurementValue::Unsigned(delta) = delta.value() else {
        return Err(TelemetryError::InvalidCounterUnit);
    };
    let rate = checked_rate_from_counter(delta, elapsed_nanos, rounding)?;
    Ok(Measurement::unsigned(
        rate,
        MeasurementUnit::MilliFramesPerSecond,
        MeasurementOrigin::Derived,
    ))
}
