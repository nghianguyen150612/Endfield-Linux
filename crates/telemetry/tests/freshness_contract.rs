mod common;

use endfield_unix_domain::MeasurementValue;
use endfield_unix_telemetry::{
    ClockDomainId, DurationNanos, FreshnessOutcome, FreshnessPolicy, FreshnessReason,
    InvalidTemporalRelationship, MetricId, ObservationTarget, ObservationTime, TelemetryError,
    TelemetryMeasurement, TelemetryObservation, TelemetryUnit, TemporalMetadata,
    WallClockTimestamp, evaluate_freshness,
};

use common::{example_cpu_sample, example_memory_sample, mono, point_time};

#[test]
fn fresh_and_stale_are_policy_driven_and_keep_the_historical_value() {
    let sample = example_cpu_sample();
    let policy = FreshnessPolicy::new(DurationNanos(1_000_000_000)).unwrap();
    let fresh = evaluate_freshness(&sample, mono(2_500_000_000), policy);
    assert!(matches!(
        fresh,
        FreshnessOutcome::Fresh {
            age: DurationNanos(500_000_000)
        }
    ));

    let stale = evaluate_freshness(&sample, mono(4_000_000_000), policy);
    assert!(matches!(
        stale,
        FreshnessOutcome::Stale {
            age: DurationNanos(2_000_000_000)
        }
    ));
    assert_eq!(sample.metric(), MetricId::CpuUtilizationWholeMachine);
    assert_eq!(
        sample.value().unwrap().value(),
        MeasurementValue::Unsigned(2_500)
    );
    assert_eq!(
        sample.quality().completeness(),
        endfield_unix_telemetry::Completeness::Complete
    );
}

#[test]
fn unrelated_domains_future_samples_and_wall_only_samples_are_conservative() {
    let sample = example_memory_sample(10);
    let policy = FreshnessPolicy::new(DurationNanos(10)).unwrap();
    let other_clock = ClockDomainId::new(42).unwrap();
    assert_eq!(
        evaluate_freshness(
            &sample,
            endfield_unix_telemetry::MonotonicTimestamp::new(other_clock, 3_000_000_010),
            policy,
        ),
        FreshnessOutcome::NotEvaluable(FreshnessReason::UnrelatedClockDomain)
    );

    let future = example_memory_sample(11);
    assert_eq!(
        evaluate_freshness(&future, mono(2_999_999_999), policy),
        FreshnessOutcome::InvalidTemporalRelationship(
            InvalidTemporalRelationship::ObservationIsInTheFuture
        )
    );

    let wall_only = TelemetryObservation::measured(
        MetricId::RamAvailable,
        ObservationTarget::Platform,
        TelemetryMeasurement::new(MeasurementValue::Unsigned(0), TelemetryUnit::Bytes),
        TemporalMetadata::new(
            ObservationTime::from_wall_clock(WallClockTimestamp::new(1_000)),
            None,
            None,
        )
        .unwrap(),
        common::provenance("synthetic.memory"),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        evaluate_freshness(&wall_only, mono(3_000_000_010), policy),
        FreshnessOutcome::NotEvaluable(FreshnessReason::NoMonotonicTimestamp)
    );
}

#[test]
fn zero_policy_is_invalid_and_boundary_age_is_fresh() {
    assert!(matches!(
        FreshnessPolicy::new(DurationNanos::ZERO),
        Err(TelemetryError::InvalidPolicyDuration)
    ));
    let sample = example_memory_sample(1);
    let policy = FreshnessPolicy::new(DurationNanos(1_000)).unwrap();
    assert!(matches!(
        evaluate_freshness(&sample, mono(3_000_001_000), policy),
        FreshnessOutcome::Fresh {
            age: DurationNanos(1_000)
        }
    ));
}

#[test]
fn missing_observations_cannot_be_reported_as_fresh() {
    let missing = TelemetryObservation::unavailable(
        MetricId::RamUsed,
        ObservationTarget::Platform,
        endfield_unix_telemetry::ObservationState::Unsupported,
        point_time(100),
        common::provenance("synthetic.memory"),
        None,
    )
    .unwrap();
    assert_eq!(
        evaluate_freshness(
            &missing,
            mono(101),
            FreshnessPolicy::new(DurationNanos(1)).unwrap(),
        ),
        FreshnessOutcome::NotEvaluable(FreshnessReason::NoMeasuredValue)
    );
}

#[test]
fn freshness_does_not_mutate_observation_state_or_quality() {
    let sample = example_cpu_sample();
    let state = sample.state();
    let quality = sample.quality().clone();
    let _ = evaluate_freshness(
        &sample,
        mono(9_000_000_000),
        FreshnessPolicy::new(DurationNanos(1)).unwrap(),
    );
    assert_eq!(sample.state(), state);
    assert_eq!(sample.quality(), &quality);
    assert_eq!(
        sample.value().unwrap().value(),
        MeasurementValue::Unsigned(2_500)
    );
}

#[test]
fn provider_known_stale_flag_cannot_be_overridden_by_a_short_age() {
    let sample = TelemetryObservation::measured(
        MetricId::RamUsed,
        ObservationTarget::Platform,
        TelemetryMeasurement::new(MeasurementValue::Unsigned(1), TelemetryUnit::Bytes),
        point_time(100),
        common::provenance("synthetic.memory"),
        endfield_unix_telemetry::QualityMetadata::complete()
            .with_flag(endfield_unix_telemetry::QualityFlag::KnownStale),
    )
    .unwrap();
    assert_eq!(
        evaluate_freshness(
            &sample,
            mono(101),
            FreshnessPolicy::new(DurationNanos(10_000)).unwrap(),
        ),
        FreshnessOutcome::KnownStaleBySource
    );
}

#[test]
fn no_hardcoded_threshold_is_assumed_for_different_metric_classes() {
    let cpu = example_cpu_sample();
    let benchmark_like_frame = common::example_frame_rate();
    let cpu_policy = FreshnessPolicy::new(DurationNanos(2_000_000_000)).unwrap();
    let result_policy = FreshnessPolicy::new(DurationNanos(60_000_000_000)).unwrap();
    assert!(matches!(
        evaluate_freshness(&cpu, mono(3_000_000_000), cpu_policy),
        FreshnessOutcome::Fresh { .. }
    ));
    assert!(matches!(
        evaluate_freshness(&benchmark_like_frame, mono(60_000_000_000), result_policy),
        FreshnessOutcome::Fresh { .. }
    ));
    let _ = point_time(1);
}
