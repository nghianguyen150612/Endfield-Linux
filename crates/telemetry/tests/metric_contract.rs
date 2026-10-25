mod common;

use endfield_unix_domain::{MeasurementOrigin, MeasurementUnit, MeasurementValue};
use endfield_unix_telemetry::{
    MetricId, MetricKind, MetricScope, ObservationTarget, TelemetryError, TelemetryMeasurement,
    TelemetryObservation, TelemetryUnit, Utilization, UtilizationScope, checked_convert_to_domain,
    checked_convert_unsigned, metric_registry,
};

use common::{cpu_target, measurement, point_time, provenance, sample_context, window_time};

#[test]
fn registry_ids_units_kinds_and_scopes_are_typed() {
    let registry = metric_registry();
    assert_eq!(registry.len(), 24);
    assert!(
        registry
            .windows(2)
            .all(|pair| pair[0].id.stable_id() < pair[1].id.stable_id())
    );
    assert_eq!(
        MetricId::try_from(0x0101).unwrap(),
        MetricId::CpuUtilizationWholeMachine
    );
    assert!(matches!(
        MetricId::try_from(0x0100),
        Err(TelemetryError::UnknownMetricId(0x0100))
    ));
    assert_eq!(
        MetricId::CpuTemperature.definition().unit,
        TelemetryUnit::MilliCelsius
    );
    assert_eq!(MetricId::FrameCount.definition().kind, MetricKind::Counter);
    assert_eq!(MetricId::FrameTime.definition().kind, MetricKind::Duration);
    assert_eq!(
        MetricId::MeanFrameTime.definition().kind,
        MetricKind::DerivedStatistic
    );
    assert_eq!(
        MetricId::ProcessCpuUtilization.definition().scope,
        MetricScope::Process
    );
    assert_eq!(
        MetricId::CpuUtilizationLogicalCpu.definition().scope,
        MetricScope::LogicalCpu
    );
    assert_eq!(
        MetricId::GpuEngineUtilization.definition().scope,
        MetricScope::GpuEngine
    );
}

#[test]
fn every_existing_domain_measurement_unit_keeps_its_meaning() {
    let mappings = [
        (MeasurementUnit::Count, TelemetryUnit::Count),
        (MeasurementUnit::Bytes, TelemetryUnit::Bytes),
        (MeasurementUnit::Hertz, TelemetryUnit::Hertz),
        (MeasurementUnit::Pixels, TelemetryUnit::Pixels),
        (
            MeasurementUnit::MilliFramesPerSecond,
            TelemetryUnit::MilliFramesPerSecond,
        ),
        (MeasurementUnit::Milliseconds, TelemetryUnit::Milliseconds),
        (MeasurementUnit::Microseconds, TelemetryUnit::Microseconds),
        (MeasurementUnit::Nanoseconds, TelemetryUnit::Nanoseconds),
        (MeasurementUnit::MilliCelsius, TelemetryUnit::MilliCelsius),
        (MeasurementUnit::Milliwatts, TelemetryUnit::Milliwatts),
        (MeasurementUnit::Millivolts, TelemetryUnit::Millivolts),
        (MeasurementUnit::Joules, TelemetryUnit::Joules),
        (MeasurementUnit::Millijoules, TelemetryUnit::Millijoules),
    ];
    for (domain, telemetry) in mappings {
        assert_eq!(TelemetryUnit::from_domain(domain).unwrap(), telemetry);
        assert_eq!(telemetry.domain_unit(), Some(domain));
    }
    assert_eq!(TelemetryUnit::BasisPoints.domain_unit(), None);
}

#[test]
fn wrong_metric_unit_and_wrong_scope_are_rejected() {
    let wrong_unit = TelemetryObservation::measured(
        MetricId::RamUsed,
        ObservationTarget::Platform,
        TelemetryMeasurement::new(MeasurementValue::Unsigned(1), TelemetryUnit::Hertz),
        point_time(1),
        provenance("synthetic.units"),
        Default::default(),
    );
    assert!(matches!(
        wrong_unit,
        Err(TelemetryError::MetricUnitMismatch)
    ));

    let wrong_scope = TelemetryObservation::measured(
        MetricId::RamUsed,
        cpu_target(),
        measurement(1, TelemetryUnit::Bytes),
        point_time(1),
        provenance("synthetic.units"),
        Default::default(),
    );
    assert!(matches!(
        wrong_scope,
        Err(TelemetryError::InvalidTargetForMetric)
    ));
}

#[test]
fn integer_conversions_are_checked_and_exact_by_default() {
    let converted = checked_convert_unsigned(125, 1_000, 5).unwrap();
    assert_eq!(converted, 25_000);
    assert_eq!(checked_convert_unsigned(u64::MAX, 2, 2).unwrap(), u64::MAX);
    let measurement = checked_convert_to_domain(
        2,
        1_000,
        1,
        MeasurementUnit::Milliwatts,
        MeasurementOrigin::Normalized,
    )
    .unwrap();
    assert_eq!(measurement.value(), MeasurementValue::Unsigned(2_000));
    assert!(matches!(
        checked_convert_unsigned(u64::MAX, 2, 1),
        Err(TelemetryError::ConversionOverflow)
    ));
    assert!(matches!(
        checked_convert_unsigned(1, 1, 0),
        Err(TelemetryError::InvalidConversionScale)
    ));
    assert!(matches!(
        checked_convert_unsigned(5, 1, 2),
        Err(TelemetryError::InexactConversion)
    ));
}

#[test]
fn zero_is_valid_and_temperature_is_signed() {
    let zero = TelemetryObservation::measured(
        MetricId::RamUsed,
        ObservationTarget::Platform,
        measurement(0, TelemetryUnit::Bytes),
        point_time(1),
        provenance("synthetic.units"),
        Default::default(),
    )
    .unwrap();
    assert_eq!(zero.value().unwrap().value(), MeasurementValue::Unsigned(0));

    for temperature in [42_500, 0, -1_250] {
        let sample = TelemetryObservation::measured(
            MetricId::CpuTemperature,
            ObservationTarget::Unassociated(MetricScope::Cpu),
            TelemetryMeasurement::new(
                MeasurementValue::Signed(temperature),
                TelemetryUnit::MilliCelsius,
            ),
            point_time(2),
            provenance("synthetic.thermal"),
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            sample.value().unwrap().value(),
            MeasurementValue::Signed(temperature)
        );
        assert_eq!(
            endfield_unix_telemetry::to_domain_measurement(
                MeasurementValue::Signed(temperature),
                TelemetryUnit::MilliCelsius,
                MeasurementOrigin::Normalized,
            )
            .unwrap()
            .unwrap()
            .value(),
            MeasurementValue::Signed(temperature)
        );
    }
    assert!(
        endfield_unix_telemetry::to_domain_measurement(
            MeasurementValue::Unsigned(5_000),
            TelemetryUnit::BasisPoints,
            MeasurementOrigin::Normalized,
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn signed_non_temperature_and_unsigned_temperature_are_rejected() {
    let invalid_ram = TelemetryObservation::measured(
        MetricId::RamUsed,
        ObservationTarget::Platform,
        TelemetryMeasurement::new(MeasurementValue::Signed(-1), TelemetryUnit::Bytes),
        point_time(1),
        provenance("synthetic.units"),
        Default::default(),
    );
    assert!(matches!(
        invalid_ram,
        Err(TelemetryError::MetricValueKindMismatch)
    ));

    let invalid_temp = TelemetryObservation::measured(
        MetricId::CpuTemperature,
        ObservationTarget::Unassociated(MetricScope::Cpu),
        measurement(0, TelemetryUnit::MilliCelsius),
        point_time(1),
        provenance("synthetic.units"),
        Default::default(),
    );
    assert!(matches!(
        invalid_temp,
        Err(TelemetryError::MetricValueKindMismatch)
    ));
}

#[test]
fn utilization_has_explicit_normalization_and_checked_bounds() {
    let half = Utilization::from_ratio_exact(1, 2, UtilizationScope::WholeMachine).unwrap();
    assert_eq!(half.basis_points(), 5_000);
    assert_eq!(half.percent_whole(), 50);
    assert!(matches!(
        Utilization::new(10_001, UtilizationScope::OneLogicalCpu),
        Err(TelemetryError::ValueOutOfRange)
    ));
    assert_eq!(
        Utilization::new(25_000, UtilizationScope::ProcessPerLogicalCpu)
            .unwrap()
            .basis_points(),
        25_000
    );
    assert!(matches!(
        Utilization::from_ratio_exact(1, 3, UtilizationScope::GpuEngine),
        Err(TelemetryError::InexactConversion)
    ));
    assert_eq!(
        Utilization::from_ratio_rounded(1, 3, UtilizationScope::GpuEngine)
            .unwrap()
            .basis_points(),
        3_333
    );
    assert!(matches!(
        Utilization::from_ratio_exact(1, 0, UtilizationScope::OneLogicalCpu),
        Err(TelemetryError::InvalidConversionScale)
    ));
}

#[test]
fn rates_require_a_real_window_and_utilization_values_are_bounded() {
    let zero_window = TelemetryObservation::measured(
        MetricId::FramesPerSecond,
        ObservationTarget::GameSession(sample_context()),
        measurement(100_000, TelemetryUnit::MilliFramesPerSecond),
        window_time(1_000, 1_000),
        provenance("synthetic.frames"),
        Default::default(),
    );
    assert!(matches!(zero_window, Err(TelemetryError::ZeroDuration)));

    let out_of_range = TelemetryObservation::measured(
        MetricId::GpuUtilization,
        ObservationTarget::Unassociated(MetricScope::Gpu),
        measurement(10_001, TelemetryUnit::BasisPoints),
        window_time(1, 2),
        provenance("synthetic.gpu"),
        Default::default(),
    );
    assert!(matches!(out_of_range, Err(TelemetryError::ValueOutOfRange)));
}
