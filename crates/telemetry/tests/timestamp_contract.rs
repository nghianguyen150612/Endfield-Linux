mod common;

use endfield_unix_domain::MeasurementValue;
use endfield_unix_telemetry::{
    ClockDomainId, DurationNanos, MetricId, MonotonicTimestamp, ObservationTarget, ObservationTime,
    TelemetryError, TelemetryMeasurement, TelemetryObservation, TelemetryUnit, TemporalMetadata,
    WallClockTimestamp,
};

use common::{measurement, mono, point_time, provenance, sample_context, window_time};

#[test]
fn wall_clock_and_monotonic_timestamps_are_distinct_and_validated() {
    let wall = WallClockTimestamp::new(1_766_000_000_000_000_000);
    let wall_only = ObservationTime::from_wall_clock(wall);
    assert_eq!(wall_only.wall_clock(), Some(wall));
    assert_eq!(wall_only.monotonic_observation(), None);

    let both = ObservationTime::monotonic(mono(5)).with_wall_clock(wall);
    assert_eq!(both.clock_domain().unwrap().get(), common::CLOCK);
    assert_eq!(both.wall_clock().unwrap(), wall);
    assert!(matches!(
        ObservationTime::new(None, None),
        Err(TelemetryError::MissingTimestamp)
    ));
}

#[test]
fn monotonic_duration_requires_the_same_domain_and_checks_subtraction() {
    let start = mono(100);
    let end = mono(350);
    assert_eq!(end.duration_since(start).unwrap(), DurationNanos(250));

    let other_domain = MonotonicTimestamp::new(ClockDomainId::new(42).unwrap(), 350);
    assert!(matches!(
        other_domain.duration_since(start),
        Err(TelemetryError::ClockDomainMismatch)
    ));
    assert!(matches!(
        start.duration_since(end),
        Err(TelemetryError::ReversedWindow)
    ));
    assert_eq!(
        MonotonicTimestamp::new(start.domain(), u64::MAX)
            .duration_since(MonotonicTimestamp::new(start.domain(), 0))
            .unwrap(),
        DurationNanos(u64::MAX)
    );
    assert!(matches!(
        MonotonicTimestamp::new(start.domain(), u64::MAX).checked_add(DurationNanos(1)),
        Err(TelemetryError::ConversionOverflow)
    ));
    assert_eq!(
        MonotonicTimestamp::new(start.domain(), u64::MAX)
            .checked_add(DurationNanos::ZERO)
            .unwrap()
            .elapsed_nanos(),
        u64::MAX
    );
}

#[test]
fn windows_reject_reversal_and_preserve_zero_duration_for_rate_validation() {
    assert!(matches!(
        endfield_unix_telemetry::ObservationWindow::new(mono(9), mono(8)),
        Err(TelemetryError::ReversedWindow)
    ));
    let zero = endfield_unix_telemetry::ObservationWindow::new(mono(9), mono(9)).unwrap();
    assert!(zero.is_zero_duration());
    assert_eq!(zero.duration().unwrap(), DurationNanos::ZERO);
}

#[test]
fn wall_clock_adjustment_does_not_change_elapsed_time() {
    let domain = mono(0).domain();
    let t0 = ObservationTime::monotonic(mono(10)).with_wall_clock(WallClockTimestamp::new(9_000));
    let t1 = ObservationTime::monotonic(mono(40)).with_wall_clock(WallClockTimestamp::new(1));
    assert_eq!(
        t1.monotonic_observation()
            .unwrap()
            .freshness_anchor()
            .duration_since(t0.monotonic_observation().unwrap().freshness_anchor())
            .unwrap(),
        DurationNanos(30)
    );
    assert_eq!(t0.clock_domain(), Some(domain));
}

#[test]
fn observation_and_collection_times_remain_separate() {
    let temporal = window_time(10, 20);
    assert_eq!(
        temporal
            .observation()
            .monotonic_observation()
            .unwrap()
            .freshness_anchor(),
        mono(20)
    );
    assert_eq!(temporal.collection_completed_at(), Some(mono(21)));
    assert_eq!(temporal.ingested_at(), Some(mono(22)));

    let sample = TelemetryObservation::measured(
        MetricId::FramesPerSecond,
        ObservationTarget::GameSession(sample_context()),
        measurement(100_000, TelemetryUnit::MilliFramesPerSecond),
        temporal,
        provenance("synthetic.timestamps"),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        sample.value().unwrap().value(),
        MeasurementValue::Unsigned(100_000)
    );
}

#[test]
fn monotonic_only_samples_need_no_wall_clock() {
    let temporal = point_time(4);
    assert_eq!(temporal.observation().wall_clock(), None);
    assert!(temporal.observation().monotonic_observation().is_some());
    assert!(
        TelemetryObservation::measured(
            MetricId::RamAvailable,
            ObservationTarget::Platform,
            TelemetryMeasurement::new(MeasurementValue::Unsigned(0), TelemetryUnit::Bytes),
            temporal,
            provenance("synthetic.timestamps"),
            Default::default(),
        )
        .is_ok()
    );
}

#[test]
fn wall_clock_only_metadata_is_supported_without_claiming_elapsed_time() {
    let observation = ObservationTime::from_wall_clock(WallClockTimestamp::new(u64::MAX));
    let temporal = TemporalMetadata::new(observation, None, None).unwrap();
    assert_eq!(temporal.clock_domain(), None);
    assert_eq!(
        temporal.observation().wall_clock().unwrap().unix_nanos(),
        u64::MAX
    );
}
