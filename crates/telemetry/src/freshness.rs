use crate::errors::TelemetryError;
use crate::observation::TelemetryObservation;
use crate::timestamp::{DurationNanos, MonotonicTimestamp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreshnessPolicy {
    maximum_age: DurationNanos,
}

impl FreshnessPolicy {
    pub fn new(maximum_age: DurationNanos) -> Result<Self, TelemetryError> {
        if maximum_age.is_zero() {
            return Err(TelemetryError::InvalidPolicyDuration);
        }
        Ok(Self { maximum_age })
    }

    pub const fn maximum_age(self) -> DurationNanos {
        self.maximum_age
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreshnessReason {
    NoMeasuredValue,
    NoMonotonicTimestamp,
    UnrelatedClockDomain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidTemporalRelationship {
    ObservationIsInTheFuture,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreshnessOutcome {
    Fresh { age: DurationNanos },
    Stale { age: DurationNanos },
    KnownStaleBySource,
    NotEvaluable(FreshnessReason),
    InvalidTemporalRelationship(InvalidTemporalRelationship),
}

/// Evaluates age against caller-injected monotonic time. Wall-clock time is
/// never used as a fallback for freshness because it can move backwards.
pub fn evaluate_freshness(
    observation: &TelemetryObservation,
    evaluated_at: MonotonicTimestamp,
    policy: FreshnessPolicy,
) -> FreshnessOutcome {
    if observation.value().is_none() {
        return FreshnessOutcome::NotEvaluable(FreshnessReason::NoMeasuredValue);
    }
    if observation
        .quality()
        .flags()
        .contains(&crate::quality::QualityFlag::KnownStale)
    {
        return FreshnessOutcome::KnownStaleBySource;
    }
    let Some(monotonic) = observation.temporal().observation().monotonic_observation() else {
        return FreshnessOutcome::NotEvaluable(FreshnessReason::NoMonotonicTimestamp);
    };
    let anchor = monotonic.freshness_anchor();
    if anchor.domain() != evaluated_at.domain() {
        return FreshnessOutcome::NotEvaluable(FreshnessReason::UnrelatedClockDomain);
    }
    match evaluated_at.duration_since(anchor) {
        Ok(age) if age <= policy.maximum_age() => FreshnessOutcome::Fresh { age },
        Ok(age) => FreshnessOutcome::Stale { age },
        Err(TelemetryError::ReversedWindow) => FreshnessOutcome::InvalidTemporalRelationship(
            InvalidTemporalRelationship::ObservationIsInTheFuture,
        ),
        Err(_) => FreshnessOutcome::NotEvaluable(FreshnessReason::UnrelatedClockDomain),
    }
}
