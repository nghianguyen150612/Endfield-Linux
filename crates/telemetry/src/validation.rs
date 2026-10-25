use endfield_unix_domain::{Measurement, MeasurementUnit, MeasurementValue};

use crate::errors::TelemetryError;
use crate::timestamp::DurationNanos;
use crate::units::TelemetryUnit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterResetEvidence {
    Continuous,
    ResetDetected,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterWrapBehavior {
    NoWrap,
    WidthKnown(u8),
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterDeltaOutcome {
    Delta(CounterDelta),
    Reset,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CounterDelta {
    amount: u64,
    unit: TelemetryUnit,
    elapsed: DurationNanos,
}

impl CounterDelta {
    pub const fn amount(self) -> u64 {
        self.amount
    }

    pub const fn unit(self) -> TelemetryUnit {
        self.unit
    }

    pub const fn elapsed(self) -> DurationNanos {
        self.elapsed
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateRounding {
    Exact,
    /// Rounds to nearest integer; exact halves round upward.
    Nearest,
}

/// Computes a monotonic counter delta only when its meaning is established.
/// A decrease after an unknown-width counter is never silently treated as
/// wraparound; the result is explicitly indeterminate.
pub fn checked_counter_delta(
    earlier: Measurement,
    later: Measurement,
    elapsed: DurationNanos,
    reset: CounterResetEvidence,
    wrap: CounterWrapBehavior,
) -> Result<CounterDeltaOutcome, TelemetryError> {
    if elapsed.is_zero() {
        return Err(TelemetryError::ZeroDuration);
    }
    if earlier.unit() != later.unit() {
        return Err(TelemetryError::InvalidCounterUnit);
    }
    if matches!(wrap, CounterWrapBehavior::WidthKnown(width) if width == 0 || width > 64) {
        return Err(TelemetryError::InvalidValue {
            field: "counter wrap width",
            reason: "must be between 1 and 64 bits",
        });
    }
    let unit = TelemetryUnit::from_domain(earlier.unit())?;
    if !matches!(
        unit,
        TelemetryUnit::Count
            | TelemetryUnit::Nanoseconds
            | TelemetryUnit::Joules
            | TelemetryUnit::Millijoules
    ) {
        return Err(TelemetryError::InvalidCounterUnit);
    }
    let (MeasurementValue::Unsigned(start), MeasurementValue::Unsigned(end)) =
        (earlier.value(), later.value())
    else {
        return Err(TelemetryError::InvalidCounterUnit);
    };

    if let CounterWrapBehavior::WidthKnown(width) = wrap {
        if width < 64 {
            let maximum = (1_u64 << width) - 1;
            if start > maximum || end > maximum {
                return Err(TelemetryError::InvalidValue {
                    field: "counter value",
                    reason: "value exceeds the declared counter width",
                });
            }
        }
    }

    if reset == CounterResetEvidence::ResetDetected {
        return Ok(CounterDeltaOutcome::Reset);
    }
    if end >= start {
        return Ok(CounterDeltaOutcome::Delta(CounterDelta {
            amount: end - start,
            unit,
            elapsed,
        }));
    }
    if reset == CounterResetEvidence::Continuous && wrap == CounterWrapBehavior::NoWrap {
        return Ok(CounterDeltaOutcome::Reset);
    }
    match wrap {
        CounterWrapBehavior::WidthKnown(width) if reset == CounterResetEvidence::Continuous => {
            let modulus = 1_u128 << width;
            let delta = modulus - u128::from(start) + u128::from(end);
            let amount = u64::try_from(delta).map_err(|_| TelemetryError::ConversionOverflow)?;
            Ok(CounterDeltaOutcome::Delta(CounterDelta {
                amount,
                unit,
                elapsed,
            }))
        }
        CounterWrapBehavior::NoWrap
        | CounterWrapBehavior::WidthKnown(_)
        | CounterWrapBehavior::Unknown => Ok(CounterDeltaOutcome::Indeterminate),
    }
}

/// Converts a counter delta into milli-frames-per-second. The counter must be
/// a P007 unsigned Count measurement. A zero elapsed interval is rejected.
pub fn checked_frame_rate(
    delta: Measurement,
    elapsed: DurationNanos,
    rounding: RateRounding,
) -> Result<Measurement, TelemetryError> {
    if delta.unit() != MeasurementUnit::Count {
        return Err(TelemetryError::InvalidCounterUnit);
    }
    let MeasurementValue::Unsigned(value) = delta.value() else {
        return Err(TelemetryError::InvalidCounterUnit);
    };
    let rate = crate::units::checked_rate_from_counter(value, elapsed.get(), rounding)?;
    Ok(Measurement::unsigned(
        rate,
        MeasurementUnit::MilliFramesPerSecond,
        endfield_unix_domain::MeasurementOrigin::Derived,
    ))
}
