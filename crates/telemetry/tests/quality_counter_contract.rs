mod common;

use endfield_unix_domain::{Measurement, MeasurementOrigin, MeasurementUnit, MeasurementValue};
use endfield_unix_telemetry::{
    CounterDeltaOutcome, CounterResetEvidence, CounterWrapBehavior, DerivationMethod,
    DurationNanos, MetricId, MetricScope, ObservationTarget, ProviderUncertainty, QualityFlag,
    QualityMetadata, RateRounding, SamplingMetadata, TelemetryError, TelemetryMeasurement,
    TelemetryObservation, TelemetryUnit, checked_counter_delta, checked_frame_rate,
    checked_rate_from_counter,
};

use common::{
    derived_provenance, measurement, point_time, provenance, sample_context, window_time,
};

#[test]
fn quality_records_completeness_sampling_and_objective_flags_without_confidence() {
    let complete = QualityMetadata::complete();
    assert_eq!(
        complete.completeness(),
        endfield_unix_telemetry::Completeness::Complete
    );

    let sampling = SamplingMetadata::new(
        Some(DurationNanos(10)),
        Some(1_000),
        Some(3),
        Some(DurationNanos(30)),
        1,
        Some(DerivationMethod::ArithmeticMean),
    )
    .unwrap();
    let quality = QualityMetadata::partial()
        .with_flag(QualityFlag::Estimated)
        .with_provider_uncertainty(ProviderUncertainty::new(2).unwrap())
        .with_sampling(sampling);
    assert_eq!(
        quality.completeness(),
        endfield_unix_telemetry::Completeness::Partial
    );
    assert_eq!(quality.sampling().unwrap().source_resolution(), Some(1_000));
    assert_eq!(quality.sampling().unwrap().observation_count(), Some(3));
    assert_eq!(quality.sampling().unwrap().missing_sample_count(), 1);
    assert!(quality.flags().contains(&QualityFlag::Estimated));
    assert!(
        quality
            .flags()
            .contains(&QualityFlag::ProviderReportedUncertainty)
    );
    assert_eq!(quality.provider_uncertainty().unwrap().absolute_error(), 2);
    assert!(
        QualityMetadata::complete()
            .with_flag(QualityFlag::ProviderReportedUncertainty)
            .validate()
            .is_err()
    );
    assert!(ProviderUncertainty::new(0).is_err());
}

#[test]
fn invalid_quality_sample_counts_and_zero_resolutions_are_rejected() {
    assert!(matches!(
        SamplingMetadata::new(None, Some(0), None, None, 0, None),
        Err(TelemetryError::InvalidSamplingMetadata)
    ));
    assert!(matches!(
        SamplingMetadata::new(Some(DurationNanos::ZERO), None, None, None, 0, None),
        Err(TelemetryError::InvalidSamplingMetadata)
    ));
    assert!(matches!(
        SamplingMetadata::new(None, None, Some(0), None, 0, None),
        Err(TelemetryError::InvalidSamplingMetadata)
    ));
}

#[test]
fn derived_statistics_require_multiple_observations_and_documented_method() {
    let quality = QualityMetadata::complete().with_sampling(
        SamplingMetadata::new(
            None,
            Some(1),
            Some(3),
            Some(DurationNanos(100)),
            0,
            Some(DerivationMethod::ArithmeticMean),
        )
        .unwrap(),
    );
    let mean = TelemetryObservation::measured(
        MetricId::MeanFrameTime,
        ObservationTarget::GameSession(sample_context()),
        measurement(10_000_000, TelemetryUnit::Nanoseconds),
        point_time(200),
        derived_provenance("synthetic.frames"),
        quality,
    )
    .unwrap();
    assert_eq!(
        mean.provenance().method(),
        endfield_unix_telemetry::SamplingMethod::Derived
    );

    let insufficient = TelemetryObservation::measured(
        MetricId::MeanFrameTime,
        ObservationTarget::GameSession(sample_context()),
        measurement(10_000_000, TelemetryUnit::Nanoseconds),
        point_time(201),
        derived_provenance("synthetic.frames"),
        QualityMetadata::complete().with_sampling(
            SamplingMetadata::new(
                None,
                None,
                Some(1),
                None,
                0,
                Some(DerivationMethod::ArithmeticMean),
            )
            .unwrap(),
        ),
    );
    assert!(matches!(
        insufficient,
        Err(TelemetryError::InvalidSamplingMetadata)
    ));
}

#[test]
fn counter_delta_handles_increase_reset_and_wrap_ambiguity_explicitly() {
    let start = Measurement::unsigned(100, MeasurementUnit::Count, MeasurementOrigin::Raw);
    let end = Measurement::unsigned(175, MeasurementUnit::Count, MeasurementOrigin::Raw);
    let delta = checked_counter_delta(
        start,
        end,
        DurationNanos(1_000_000_000),
        CounterResetEvidence::Continuous,
        CounterWrapBehavior::NoWrap,
    )
    .unwrap();
    assert!(
        matches!(delta, CounterDeltaOutcome::Delta(delta) if delta.amount() == 75 && delta.elapsed() == DurationNanos(1_000_000_000))
    );

    assert_eq!(
        checked_counter_delta(
            end,
            start,
            DurationNanos(1),
            CounterResetEvidence::ResetDetected,
            CounterWrapBehavior::NoWrap,
        )
        .unwrap(),
        CounterDeltaOutcome::Reset
    );
    assert_eq!(
        checked_counter_delta(
            end,
            start,
            DurationNanos(1),
            CounterResetEvidence::Unknown,
            CounterWrapBehavior::WidthKnown(32),
        )
        .unwrap(),
        CounterDeltaOutcome::Indeterminate
    );
    let wrapped = checked_counter_delta(
        Measurement::unsigned(
            u64::from(u32::MAX) - 5,
            MeasurementUnit::Count,
            MeasurementOrigin::Raw,
        ),
        Measurement::unsigned(3, MeasurementUnit::Count, MeasurementOrigin::Raw),
        DurationNanos(10),
        CounterResetEvidence::Continuous,
        CounterWrapBehavior::WidthKnown(32),
    )
    .unwrap();
    assert!(matches!(wrapped, CounterDeltaOutcome::Delta(delta) if delta.amount() == 9));
    assert_eq!(
        checked_counter_delta(
            end,
            start,
            DurationNanos(1),
            CounterResetEvidence::Continuous,
            CounterWrapBehavior::NoWrap,
        )
        .unwrap(),
        CounterDeltaOutcome::Reset
    );
}

#[test]
fn counter_validation_rejects_zero_duration_negative_or_incompatible_units() {
    let count = Measurement::unsigned(1, MeasurementUnit::Count, MeasurementOrigin::Raw);
    assert!(matches!(
        checked_counter_delta(
            count,
            count,
            DurationNanos::ZERO,
            CounterResetEvidence::Continuous,
            CounterWrapBehavior::NoWrap,
        ),
        Err(TelemetryError::ZeroDuration)
    ));
    let signed =
        Measurement::signed(-1, MeasurementUnit::MilliCelsius, MeasurementOrigin::Raw).unwrap();
    assert!(matches!(
        checked_counter_delta(
            signed,
            signed,
            DurationNanos(1),
            CounterResetEvidence::Continuous,
            CounterWrapBehavior::NoWrap,
        ),
        Err(TelemetryError::InvalidCounterUnit)
    ));
    let bytes = Measurement::unsigned(1, MeasurementUnit::Bytes, MeasurementOrigin::Raw);
    assert!(matches!(
        checked_counter_delta(
            bytes,
            bytes,
            DurationNanos(1),
            CounterResetEvidence::Continuous,
            CounterWrapBehavior::NoWrap,
        ),
        Err(TelemetryError::InvalidCounterUnit)
    ));
    assert!(matches!(
        checked_counter_delta(
            count,
            count,
            DurationNanos(1),
            CounterResetEvidence::Continuous,
            CounterWrapBehavior::WidthKnown(0),
        ),
        Err(TelemetryError::InvalidValue { .. })
    ));
    let too_wide = Measurement::unsigned(256, MeasurementUnit::Count, MeasurementOrigin::Raw);
    assert!(matches!(
        checked_counter_delta(
            too_wide,
            too_wide,
            DurationNanos(1),
            CounterResetEvidence::Continuous,
            CounterWrapBehavior::WidthKnown(8),
        ),
        Err(TelemetryError::InvalidValue { .. })
    ));
}

#[test]
fn frame_rate_conversion_checks_units_rounding_and_overflow() {
    let one_hundred_frames =
        Measurement::unsigned(100, MeasurementUnit::Count, MeasurementOrigin::Raw);
    let rate = checked_frame_rate(
        one_hundred_frames,
        DurationNanos(1_000_000_000),
        RateRounding::Exact,
    )
    .unwrap();
    assert_eq!(rate.value(), MeasurementValue::Unsigned(100_000));
    assert_eq!(rate.unit(), MeasurementUnit::MilliFramesPerSecond);
    assert!(matches!(
        checked_rate_from_counter(1, 0, RateRounding::Nearest),
        Err(TelemetryError::ZeroDuration)
    ));
    assert!(matches!(
        checked_rate_from_counter(u64::MAX, 1, RateRounding::Nearest),
        Err(TelemetryError::ConversionOverflow)
    ));
    assert!(matches!(
        checked_frame_rate(
            Measurement::unsigned(1, MeasurementUnit::Bytes, MeasurementOrigin::Raw),
            DurationNanos(1),
            RateRounding::Exact,
        ),
        Err(TelemetryError::InvalidCounterUnit)
    ));
    assert!(matches!(
        checked_frame_rate(one_hundred_frames, DurationNanos(3), RateRounding::Exact),
        Err(TelemetryError::InexactConversion)
    ));
}

#[test]
fn utilization_quality_is_objective_and_does_not_infer_benchmark_accuracy() {
    let quality = QualityMetadata::complete()
        .with_flag(QualityFlag::InsufficientSamplingInterval)
        .with_sampling(
            SamplingMetadata::new(Some(DurationNanos(1)), None, Some(1), None, 0, None).unwrap(),
        );
    let gpu_sample = TelemetryObservation::measured(
        MetricId::GpuUtilization,
        ObservationTarget::Unassociated(MetricScope::Gpu),
        TelemetryMeasurement::new(
            MeasurementValue::Unsigned(5_000),
            TelemetryUnit::BasisPoints,
        ),
        window_time(100, 101),
        provenance("synthetic.gpu"),
        quality,
    )
    .unwrap();
    assert!(
        gpu_sample
            .quality()
            .flags()
            .contains(&QualityFlag::InsufficientSamplingInterval)
    );
    assert_eq!(
        gpu_sample.value().unwrap().value(),
        MeasurementValue::Unsigned(5_000)
    );
}
