use endfield_unix_domain::MeasurementValue;
use endfield_unix_platform_linux::{
    LinuxCpuMemoryTelemetryProvider, SystemLinuxDataSource, TelemetryInterval,
};
use endfield_unix_telemetry::{
    CollectionCompleteness, CollectionRequest, MetricId, ObservationState, ObservationTarget,
    SamplingContextId, TelemetryProvider,
};

#[test]
fn live_linux_cpu_memory_telemetry_reads_bounded_sources_and_builds_a_valid_batch() {
    let provider = LinuxCpuMemoryTelemetryProvider::new(
        SystemLinuxDataSource::new(),
        TelemetryInterval::from_millis(100).unwrap(),
    );
    let request = CollectionRequest::new(
        SamplingContextId::new(51).unwrap(),
        vec![
            MetricId::CpuUtilizationWholeMachine,
            MetricId::RamAvailable,
            MetricId::RamUsed,
            MetricId::SwapUsed,
            MetricId::CgroupMemoryUsed,
        ],
        None,
    )
    .unwrap();
    let collection = provider
        .collect(&request)
        .expect("live telemetry collection");
    let batch = collection.batch();
    batch.validate().expect("P015 batch validation");
    assert!(matches!(
        collection.completeness(),
        CollectionCompleteness::Complete | CollectionCompleteness::Partial
    ));

    let aggregate = batch
        .observations()
        .iter()
        .find(|observation| {
            observation.metric() == MetricId::CpuUtilizationWholeMachine
                && observation.target() == ObservationTarget::Platform
        })
        .expect("aggregate CPU observation");
    assert!(
        aggregate.state().is_measured(),
        "aggregate CPU state: {:?}",
        aggregate.state()
    );
    let ObservationState::Measured(measurement) = aggregate.state() else {
        unreachable!("asserted measured above")
    };
    assert_eq!(
        measurement.unit(),
        endfield_unix_telemetry::TelemetryUnit::BasisPoints
    );
    assert!(matches!(measurement.value(), MeasurementValue::Unsigned(value) if value <= 10_000));
    assert!(batch.observations().iter().any(|observation| {
        observation.metric() == MetricId::RamAvailable && observation.state().is_measured()
    }));
    assert!(batch.observations().iter().any(|observation| {
        observation.metric() == MetricId::RamUsed && observation.state().is_measured()
    }));
    assert!(
        aggregate
            .temporal()
            .observation()
            .monotonic_observation()
            .is_some_and(|observation| matches!(
                observation,
                endfield_unix_telemetry::MonotonicObservation::Window(window)
                    if !window.is_zero_duration()
            ))
    );
}
