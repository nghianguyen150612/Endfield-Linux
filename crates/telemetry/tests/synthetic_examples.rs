mod common;

use endfield_unix_domain::{DiagnosticCategory, MeasurementValue};
use endfield_unix_telemetry::{
    DurationNanos, FreshnessOutcome, FreshnessPolicy, MetricId, MetricScope, ObservationState,
    ObservationTarget, QualityMetadata, SafeDiagnostic, TelemetryBatch, TelemetryError,
    TelemetryObservation, TelemetryUnit, evaluate_freshness,
};

use common::{
    cpu_target, example_cpu_sample, example_frame_rate, example_memory_sample, gpu_target,
    measurement, mono, point_time, provenance, sample_context, two_gpu_snapshot, window_time,
};

#[test]
fn synthetic_cpu_memory_gpu_thermal_and_frame_fixtures_are_contract_examples() {
    let cpu = example_cpu_sample();
    assert_eq!(cpu.metric(), MetricId::CpuUtilizationWholeMachine);
    assert_eq!(cpu.value().unwrap().unit(), TelemetryUnit::BasisPoints);
    assert_eq!(
        cpu.value().unwrap().value(),
        MeasurementValue::Unsigned(2_500)
    );

    let memory_zero = example_memory_sample(0);
    assert_eq!(
        memory_zero.value().unwrap().value(),
        MeasurementValue::Unsigned(0)
    );
    let memory_known = example_memory_sample(1_024);
    assert_eq!(
        memory_known.value().unwrap().value(),
        MeasurementValue::Unsigned(1_024)
    );
    let memory_unknown = TelemetryObservation::unavailable(
        MetricId::RamAvailable,
        ObservationTarget::Platform,
        ObservationState::Unknown,
        point_time(6),
        provenance("synthetic.memory"),
        None,
    )
    .unwrap();
    assert_eq!(memory_unknown.value(), None);

    let (_, snapshot) = two_gpu_snapshot();
    let gpu_a = common::measured(
        MetricId::GpuUtilization,
        gpu_target(2),
        measurement(2_000, TelemetryUnit::BasisPoints),
        window_time(10, 20),
        "synthetic.gpu",
    );
    let gpu_b = common::measured(
        MetricId::GpuUtilization,
        gpu_target(3),
        measurement(8_000, TelemetryUnit::BasisPoints),
        window_time(10, 20),
        "synthetic.gpu",
    );
    let gpu_batch = TelemetryBatch::new(
        sample_context(),
        Some(common::clock()),
        Some(snapshot),
        vec![gpu_a, gpu_b],
    )
    .unwrap();
    assert_eq!(gpu_batch.observations().len(), 2);
    assert_ne!(
        gpu_batch.observations()[0].target(),
        gpu_batch.observations()[1].target()
    );

    for value in [42_000, 0, -5_000] {
        let thermal = TelemetryObservation::measured(
            MetricId::CpuTemperature,
            ObservationTarget::Unassociated(MetricScope::Cpu),
            endfield_unix_telemetry::TelemetryMeasurement::new(
                MeasurementValue::Signed(value),
                TelemetryUnit::MilliCelsius,
            ),
            point_time(30 + value.unsigned_abs()),
            provenance("synthetic.thermal"),
            QualityMetadata::complete(),
        )
        .unwrap();
        assert_eq!(
            thermal.value().unwrap().value(),
            MeasurementValue::Signed(value)
        );
    }
    let failed_sensor = TelemetryObservation::unavailable(
        MetricId::CpuTemperature,
        ObservationTarget::Unassociated(MetricScope::Cpu),
        ObservationState::TemporaryFailure,
        point_time(40),
        provenance("synthetic.thermal"),
        Some(SafeDiagnostic::new(DiagnosticCategory::TemporaryFailure)),
    )
    .unwrap();
    assert_eq!(failed_sensor.value(), None);

    let fps = example_frame_rate();
    let frametime = common::measured(
        MetricId::FrameTime,
        ObservationTarget::GameSession(sample_context()),
        measurement(10_000_000, TelemetryUnit::Nanoseconds),
        point_time(50),
        "synthetic.frames",
    );
    assert_eq!(
        fps.value().unwrap().value(),
        MeasurementValue::Unsigned(100_000)
    );
    assert_eq!(
        frametime.value().unwrap().value(),
        MeasurementValue::Unsigned(10_000_000)
    );
}

#[test]
fn stale_fixture_retains_its_value_after_freshness_evaluation() {
    let sample = example_cpu_sample();
    let stale = evaluate_freshness(
        &sample,
        mono(8_000_000_000),
        FreshnessPolicy::new(DurationNanos(1_000)).unwrap(),
    );
    assert!(matches!(stale, FreshnessOutcome::Stale { .. }));
    assert_eq!(
        sample.value().unwrap().value(),
        MeasurementValue::Unsigned(2_500)
    );
}

#[test]
fn incompatible_monotonic_domains_cannot_form_an_elapsed_duration() {
    let first = mono(100);
    let second = endfield_unix_telemetry::MonotonicTimestamp::new(
        endfield_unix_telemetry::ClockDomainId::new(999).unwrap(),
        200,
    );
    assert!(matches!(
        second.duration_since(first),
        Err(TelemetryError::ClockDomainMismatch)
    ));
    let _ = cpu_target();
}
