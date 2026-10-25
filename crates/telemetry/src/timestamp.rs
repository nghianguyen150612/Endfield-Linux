use std::time::Duration;

use crate::errors::TelemetryError;

/// Identifies one monotonic clock instance or sampling session. It has no
/// relation to the Unix epoch and must be supplied by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClockDomainId(u64);

impl ClockDomainId {
    pub fn new(value: u64) -> Result<Self, TelemetryError> {
        if value == 0 {
            return Err(TelemetryError::InvalidValue {
                field: "clock-domain identifier",
                reason: "zero is reserved",
            });
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A nanosecond count relative to an explicitly named monotonic domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonotonicTimestamp {
    domain: ClockDomainId,
    elapsed_nanos: u64,
}

impl MonotonicTimestamp {
    pub const fn new(domain: ClockDomainId, elapsed_nanos: u64) -> Self {
        Self {
            domain,
            elapsed_nanos,
        }
    }

    pub const fn domain(self) -> ClockDomainId {
        self.domain
    }

    pub const fn elapsed_nanos(self) -> u64 {
        self.elapsed_nanos
    }

    pub fn checked_add(self, duration: DurationNanos) -> Result<Self, TelemetryError> {
        let elapsed_nanos = self
            .elapsed_nanos
            .checked_add(duration.get())
            .ok_or(TelemetryError::ConversionOverflow)?;
        Ok(Self::new(self.domain, elapsed_nanos))
    }

    pub fn duration_since(self, earlier: Self) -> Result<DurationNanos, TelemetryError> {
        if self.domain != earlier.domain {
            return Err(TelemetryError::ClockDomainMismatch);
        }
        let elapsed = self
            .elapsed_nanos
            .checked_sub(earlier.elapsed_nanos)
            .ok_or(TelemetryError::ReversedWindow)?;
        Ok(DurationNanos(elapsed))
    }
}

/// A duration represented exactly as nanoseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DurationNanos(pub u64);

impl DurationNanos {
    pub const ZERO: Self = Self(0);

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub const fn to_std(self) -> Duration {
        Duration::from_nanos(self.0)
    }
}

/// A validated monotonic observation window. Equal endpoints are permitted as
/// a timestamp range, but rate calculations reject its zero duration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObservationWindow {
    start: MonotonicTimestamp,
    end: MonotonicTimestamp,
}

impl ObservationWindow {
    pub fn new(start: MonotonicTimestamp, end: MonotonicTimestamp) -> Result<Self, TelemetryError> {
        if start.domain() != end.domain() {
            return Err(TelemetryError::ClockDomainMismatch);
        }
        if end.elapsed_nanos() < start.elapsed_nanos() {
            return Err(TelemetryError::ReversedWindow);
        }
        Ok(Self { start, end })
    }

    pub const fn start(self) -> MonotonicTimestamp {
        self.start
    }

    pub const fn end(self) -> MonotonicTimestamp {
        self.end
    }

    pub fn duration(self) -> Result<DurationNanos, TelemetryError> {
        self.end.duration_since(self.start)
    }

    pub const fn is_zero_duration(self) -> bool {
        self.start.elapsed_nanos() == self.end.elapsed_nanos()
    }
}

/// Nanoseconds since the Unix epoch. This is for reporting and correlation;
/// elapsed-time calculations use monotonic timestamps instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WallClockTimestamp(u64);

impl WallClockTimestamp {
    pub const fn new(unix_nanos: u64) -> Self {
        Self(unix_nanos)
    }

    pub const fn unix_nanos(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MonotonicObservation {
    Instant(MonotonicTimestamp),
    Window(ObservationWindow),
}

impl MonotonicObservation {
    pub const fn domain(self) -> ClockDomainId {
        match self {
            Self::Instant(timestamp) => timestamp.domain(),
            Self::Window(window) => window.start().domain(),
        }
    }

    pub const fn freshness_anchor(self) -> MonotonicTimestamp {
        match self {
            Self::Instant(timestamp) => timestamp,
            Self::Window(window) => window.end(),
        }
    }

    pub const fn is_zero_duration_window(self) -> bool {
        matches!(self, Self::Window(window) if window.is_zero_duration())
    }
}
