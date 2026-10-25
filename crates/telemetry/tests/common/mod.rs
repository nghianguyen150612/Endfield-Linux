#![allow(dead_code)]

use endfield_unix_domain::{
    Architecture, Availability, CapabilityId, CapabilityRecord, CapabilityValue, CpuInfo,
    CpuTopology, DistributionIdentity, GpuClass, GpuInfo, InstanceId, KernelIdentity,
    MeasurementOrigin, MeasurementUnit, OsIdentity, PlatformFamily, PlatformIdentity, Provenance,
    ProvenanceId, SystemSnapshot,
};
use endfield_unix_telemetry::{
    ClockDomainId, DurationNanos, MetricId, MetricScope, MonotonicTimestamp, ObservationState,
    ObservationTarget, ObservationTime, ProviderId, QualityMetadata, SamplingContextId,
    SamplingMethod, SnapshotContext, SnapshotRefId, TelemetryMeasurement, TelemetryObservation,
    TelemetryProvenance, TelemetryUnit, TemporalMetadata,
};

pub const CLOCK: u64 = 41;
pub const SNAPSHOT: u64 = 501;
pub const SAMPLE_CONTEXT: u64 = 902;

pub fn clock() -> ClockDomainId {
    ClockDomainId::new(CLOCK).unwrap()
}

pub fn sample_context() -> SamplingContextId {
    SamplingContextId::new(SAMPLE_CONTEXT).unwrap()
}

pub fn snapshot_ref() -> SnapshotRefId {
    SnapshotRefId::new(SNAPSHOT).unwrap()
}

pub fn mono(nanos: u64) -> MonotonicTimestamp {
    MonotonicTimestamp::new(clock(), nanos)
}

pub fn point_time(nanos: u64) -> TemporalMetadata {
    TemporalMetadata::new(ObservationTime::monotonic(mono(nanos)), None, None).unwrap()
}

pub fn window_time(start: u64, end: u64) -> TemporalMetadata {
    let window = endfield_unix_telemetry::ObservationWindow::new(mono(start), mono(end)).unwrap();
    TemporalMetadata::new(
        ObservationTime::window(window),
        Some(mono(end.saturating_add(1))),
        Some(mono(end.saturating_add(2))),
    )
    .unwrap()
}

pub fn provider(name: &str) -> ProviderId {
    ProviderId::new(name).unwrap()
}

pub fn provenance(name: &str) -> TelemetryProvenance {
    TelemetryProvenance::new(
        provider(name),
        SamplingMethod::Synthetic,
        MeasurementOrigin::Normalized,
    )
    .unwrap()
}

pub fn derived_provenance(name: &str) -> TelemetryProvenance {
    TelemetryProvenance::new(
        provider(name),
        SamplingMethod::Derived,
        MeasurementOrigin::Derived,
    )
    .unwrap()
}

pub fn measurement(value: u64, unit: TelemetryUnit) -> TelemetryMeasurement {
    TelemetryMeasurement::new(
        endfield_unix_domain::MeasurementValue::Unsigned(value),
        unit,
    )
}

pub fn measured(
    metric: MetricId,
    target: ObservationTarget,
    value: TelemetryMeasurement,
    timestamp: TemporalMetadata,
    provider_name: &str,
) -> TelemetryObservation {
    TelemetryObservation::measured(
        metric,
        target,
        value,
        timestamp,
        provenance(provider_name),
        QualityMetadata::complete(),
    )
    .unwrap()
}

pub fn two_gpu_snapshot() -> (SystemSnapshot, SnapshotContext) {
    let platform = PlatformIdentity::new(
        PlatformFamily::Linux,
        OsIdentity::new("Linux", None).unwrap(),
        Architecture::parse("x86_64").unwrap(),
        Some(KernelIdentity::new("Linux", Some("6.12.0".to_owned())).unwrap()),
        Some(DistributionIdentity::new("Arch", None, None).unwrap()),
    )
    .unwrap();
    let mut snapshot = SystemSnapshot::new(platform);
    snapshot.cpu = Some(CpuInfo {
        instance: InstanceId::new(1).unwrap(),
        model_identifier: None,
        topology: CapabilityValue::new(
            Availability::Available,
            Some(CpuTopology {
                logical_processor_count: 8,
                physical_core_count: None,
            }),
        ),
        observed_frequency: CapabilityValue::new(Availability::Unknown, None),
        scheduling: Availability::Unknown,
        power_management: Availability::Unknown,
    });
    for instance in [2, 3] {
        snapshot.gpus.push(GpuInfo {
            instance: InstanceId::new(instance).unwrap(),
            availability: Availability::Available,
            vendor: None,
            model: None,
            class: GpuClass::Unknown,
            graphics_driver: None,
            driver_availability: Availability::Unknown,
            vram: CapabilityValue::new(Availability::Unknown, None),
            hardware_acceleration: Availability::Unknown,
            vulkan_device: Availability::Unknown,
        });
    }
    snapshot.provenance.push(
        Provenance::new(
            ProvenanceId::new(1).unwrap(),
            "synthetic.snapshot",
            Some(1_766_000_000_000_000_000),
        )
        .unwrap(),
    );
    let mut cpu_topology =
        CapabilityRecord::new(CapabilityId::CpuTopology, Availability::Available);
    cpu_topology.instance = Some(InstanceId::new(1).unwrap());
    cpu_topology.provenance = Some(ProvenanceId::new(1).unwrap());
    cpu_topology.measurement = Some(endfield_unix_domain::Measurement::unsigned(
        8,
        MeasurementUnit::Count,
        MeasurementOrigin::Normalized,
    ));
    snapshot.capabilities.push(cpu_topology);
    for instance in [2, 3] {
        let mut gpu = CapabilityRecord::new(CapabilityId::GpuDevice, Availability::Available);
        gpu.instance = Some(InstanceId::new(instance).unwrap());
        gpu.provenance = Some(ProvenanceId::new(1).unwrap());
        snapshot.capabilities.push(gpu);
    }
    let context = SnapshotContext::from_snapshot(snapshot_ref(), &snapshot).unwrap();
    (snapshot, context)
}

pub fn gpu_target(instance: u32) -> ObservationTarget {
    ObservationTarget::Gpu(endfield_unix_telemetry::DeviceReference::new(
        snapshot_ref(),
        InstanceId::new(instance).unwrap(),
    ))
}

pub fn cpu_target() -> ObservationTarget {
    ObservationTarget::Cpu(endfield_unix_telemetry::DeviceReference::new(
        snapshot_ref(),
        InstanceId::new(1).unwrap(),
    ))
}

pub fn example_cpu_sample() -> TelemetryObservation {
    measured(
        MetricId::CpuUtilizationWholeMachine,
        ObservationTarget::Platform,
        measurement(2_500, TelemetryUnit::BasisPoints),
        window_time(1_000_000_000, 2_000_000_000),
        "synthetic.cpu",
    )
}

pub fn example_memory_sample(value: u64) -> TelemetryObservation {
    measured(
        MetricId::RamUsed,
        ObservationTarget::Platform,
        measurement(value, TelemetryUnit::Bytes),
        point_time(3_000_000_000),
        "synthetic.memory",
    )
}

pub fn example_frame_rate() -> TelemetryObservation {
    measured(
        MetricId::FramesPerSecond,
        ObservationTarget::GameSession(sample_context()),
        measurement(100_000, TelemetryUnit::MilliFramesPerSecond),
        window_time(4_000_000_000, 5_000_000_000),
        "synthetic.frames",
    )
}

#[allow(dead_code)]
pub fn example_interval() -> DurationNanos {
    DurationNanos(1_000_000_000)
}

#[allow(dead_code)]
pub fn state_example() -> ObservationState {
    ObservationState::Unknown
}

#[allow(dead_code)]
pub fn scope_example() -> MetricScope {
    MetricScope::Platform
}
