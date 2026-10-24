use endfield_unix_domain::{
    Architecture, Availability, Bytes, CapabilityId, CapabilityRecord, CapabilityValue, CpuInfo,
    CpuTopology, Diagnostic, DiagnosticCategory, DisplayConnection, DisplayInfo,
    DistributionIdentity, GpuClass, GpuInfo, GpuVendor, GraphicsDriver, Hertz, InstanceId,
    Measurement, MeasurementOrigin, MeasurementUnit, MemoryCapacity, MemoryInfo, OsIdentity,
    PlatformFamily, PlatformIdentity, ProbeReport, Provenance, ProvenanceId, Resolution, SwapInfo,
    SystemSnapshot,
};

fn linux_platform(os: &str, distribution: &str) -> PlatformIdentity {
    PlatformIdentity::new(
        PlatformFamily::Linux,
        OsIdentity::new(os, Some("rolling".to_owned())).unwrap(),
        Architecture::parse("x86_64").unwrap(),
        Some(
            endfield_unix_domain::KernelIdentity::new("Linux", Some("6.12.1".to_owned())).unwrap(),
        ),
        Some(DistributionIdentity::new(distribution, None, None).unwrap()),
    )
    .unwrap()
}

fn source(id: u32, name: &str) -> Provenance {
    Provenance::new(
        ProvenanceId::new(id).unwrap(),
        name,
        Some(1_766_000_000_000_000_000),
    )
    .unwrap()
}

fn record(
    id: CapabilityId,
    availability: Availability,
    instance: Option<u32>,
    value: Option<Measurement>,
    provenance: Option<u32>,
    diagnostic: Option<DiagnosticCategory>,
) -> CapabilityRecord {
    CapabilityRecord {
        id,
        availability,
        measurement: value,
        instance: instance.map(|id| InstanceId::new(id).unwrap()),
        provenance: provenance.map(|id| ProvenanceId::new(id).unwrap()),
        diagnostic: diagnostic.map(|category| Diagnostic {
            category,
            provider: None,
            message: Some("synthetic fixture observation".to_owned()),
        }),
    }
}

fn measured(value: u64, unit: MeasurementUnit) -> Option<Measurement> {
    Some(Measurement::unsigned(
        value,
        unit,
        MeasurementOrigin::Normalized,
    ))
}

pub fn desktop_fixture() -> SystemSnapshot {
    let mut snapshot = SystemSnapshot::new(linux_platform("Linux", "Arch Linux"));
    snapshot.cpu = Some(CpuInfo {
        instance: InstanceId::new(1).unwrap(),
        model_identifier: Some("Intel Core i7-12700F".to_owned()),
        topology: CapabilityValue::new(
            Availability::Available,
            Some(CpuTopology {
                logical_processor_count: 20,
                physical_core_count: Some(12),
            }),
        ),
        observed_frequency: CapabilityValue::new(
            Availability::Available,
            Some(Hertz(4_800_000_000)),
        ),
        scheduling: Availability::Available,
        power_management: Availability::Available,
    });
    snapshot.gpus.push(GpuInfo {
        instance: InstanceId::new(2).unwrap(),
        availability: Availability::Available,
        vendor: Some(GpuVendor::Nvidia),
        model: Some("NVIDIA RTX 5070".to_owned()),
        class: GpuClass::Discrete,
        driver_availability: Availability::Available,
        graphics_driver: Some(GraphicsDriver {
            name: "NVIDIA".to_owned(),
            version: Some("synthetic".to_owned()),
        }),
        vram: CapabilityValue::new(
            Availability::Available,
            Some(Bytes(12 * 1024 * 1024 * 1024)),
        ),
        hardware_acceleration: Availability::Available,
        vulkan_device: Availability::Available,
    });
    snapshot.memory = Some(MemoryInfo {
        ram: CapabilityValue::new(
            Availability::Available,
            Some(MemoryCapacity {
                total: Some(Bytes(32 * 1024 * 1024 * 1024)),
                available: Some(Bytes(24 * 1024 * 1024 * 1024)),
            }),
        ),
        swap: SwapInfo {
            availability: Availability::Available,
            present: Some(true),
            total: Some(Bytes(16 * 1024 * 1024 * 1024)),
        },
    });
    snapshot.displays.push(DisplayInfo {
        instance: InstanceId::new(3).unwrap(),
        label: Some("Synthetic desktop display".to_owned()),
        availability: Availability::Available,
        resolution: Some(Resolution::new(2560, 1440).unwrap()),
        refresh_rate: Some(Hertz(144)),
        primary: Some(true),
        connection: Some(DisplayConnection::External),
        connection_availability: Availability::Available,
        session_availability: Availability::Unknown,
    });
    snapshot
        .provenance
        .push(source(1, "synthetic-desktop-fixture"));
    snapshot.capabilities = vec![
        record(
            CapabilityId::CpuTopology,
            Availability::Available,
            Some(1),
            measured(20, MeasurementUnit::Count),
            Some(1),
            None,
        ),
        record(
            CapabilityId::CpuScheduling,
            Availability::Available,
            Some(1),
            None,
            Some(1),
            None,
        ),
        record(
            CapabilityId::CpuFrequencyPower,
            Availability::Available,
            Some(1),
            measured(4_800_000_000, MeasurementUnit::Hertz),
            Some(1),
            None,
        ),
        record(
            CapabilityId::GpuDevice,
            Availability::Available,
            Some(2),
            None,
            Some(1),
            None,
        ),
        record(
            CapabilityId::Vram,
            Availability::Available,
            Some(2),
            measured(12 * 1024 * 1024 * 1024, MeasurementUnit::Bytes),
            Some(1),
            None,
        ),
        record(
            CapabilityId::HardwareAcceleration,
            Availability::Available,
            Some(2),
            None,
            Some(1),
            None,
        ),
        record(
            CapabilityId::GraphicsDriver,
            Availability::Available,
            Some(2),
            None,
            Some(1),
            None,
        ),
        record(
            CapabilityId::VulkanDevice,
            Availability::Available,
            Some(2),
            None,
            Some(1),
            None,
        ),
        record(
            CapabilityId::Ram,
            Availability::Available,
            None,
            measured(32 * 1024 * 1024 * 1024, MeasurementUnit::Bytes),
            Some(1),
            None,
        ),
        record(
            CapabilityId::Swap,
            Availability::Available,
            None,
            measured(16 * 1024 * 1024 * 1024, MeasurementUnit::Bytes),
            Some(1),
            None,
        ),
        record(
            CapabilityId::Display,
            Availability::Available,
            Some(3),
            None,
            Some(1),
            None,
        ),
        record(
            CapabilityId::RuntimeDiscovery,
            Availability::Unknown,
            None,
            None,
            Some(1),
            Some(DiagnosticCategory::MissingProvider),
        ),
        record(
            CapabilityId::OptimizationTools,
            Availability::Available,
            None,
            measured(1, MeasurementUnit::Count),
            Some(1),
            None,
        ),
    ];
    snapshot
}

pub fn laptop_fixture() -> SystemSnapshot {
    let mut snapshot = SystemSnapshot::new(linux_platform("Linux", "Ubuntu"));
    snapshot.cpu = Some(CpuInfo {
        instance: InstanceId::new(1).unwrap(),
        model_identifier: Some("Intel Core i5-1135G7".to_owned()),
        topology: CapabilityValue::new(
            Availability::Available,
            Some(CpuTopology {
                logical_processor_count: 8,
                physical_core_count: Some(4),
            }),
        ),
        observed_frequency: CapabilityValue::new(Availability::Unknown, None),
        scheduling: Availability::Available,
        power_management: Availability::Degraded,
    });
    snapshot.gpus = vec![
        GpuInfo {
            instance: InstanceId::new(2).unwrap(),
            availability: Availability::Available,
            vendor: Some(GpuVendor::Intel),
            model: Some("Intel Iris Xe".to_owned()),
            class: GpuClass::Integrated,
            driver_availability: Availability::Unknown,
            graphics_driver: None,
            vram: CapabilityValue::new(Availability::Unknown, None),
            hardware_acceleration: Availability::Available,
            vulkan_device: Availability::Degraded,
        },
        GpuInfo {
            instance: InstanceId::new(3).unwrap(),
            availability: Availability::Available,
            vendor: Some(GpuVendor::Nvidia),
            model: Some("NVIDIA MX350".to_owned()),
            class: GpuClass::Discrete,
            driver_availability: Availability::Available,
            graphics_driver: Some(GraphicsDriver {
                name: "NVIDIA".to_owned(),
                version: None,
            }),
            vram: CapabilityValue::new(
                Availability::Available,
                Some(Bytes(2 * 1024 * 1024 * 1024)),
            ),
            hardware_acceleration: Availability::Degraded,
            vulkan_device: Availability::Unknown,
        },
    ];
    snapshot.memory = Some(MemoryInfo {
        ram: CapabilityValue::new(
            Availability::Available,
            Some(MemoryCapacity {
                total: Some(Bytes(16 * 1024 * 1024 * 1024)),
                available: None,
            }),
        ),
        swap: SwapInfo {
            availability: Availability::Unknown,
            present: None,
            total: None,
        },
    });
    snapshot.displays.push(DisplayInfo {
        instance: InstanceId::new(4).unwrap(),
        label: Some("Synthetic laptop panel".to_owned()),
        availability: Availability::Available,
        resolution: Some(Resolution::new(1920, 1080).unwrap()),
        refresh_rate: Some(Hertz(60)),
        primary: Some(true),
        connection: Some(DisplayConnection::Internal),
        connection_availability: Availability::Available,
        session_availability: Availability::Unknown,
    });
    snapshot
        .provenance
        .push(source(2, "synthetic-laptop-fixture"));
    snapshot.capabilities = vec![
        record(
            CapabilityId::CpuTopology,
            Availability::Available,
            Some(1),
            measured(8, MeasurementUnit::Count),
            Some(2),
            None,
        ),
        record(
            CapabilityId::CpuScheduling,
            Availability::Available,
            Some(1),
            None,
            Some(2),
            None,
        ),
        record(
            CapabilityId::CpuFrequencyPower,
            Availability::Degraded,
            Some(1),
            None,
            Some(2),
            Some(DiagnosticCategory::PermissionDenied),
        ),
        record(
            CapabilityId::GpuDevice,
            Availability::Available,
            Some(2),
            None,
            Some(2),
            None,
        ),
        record(
            CapabilityId::HardwareAcceleration,
            Availability::Available,
            Some(2),
            None,
            Some(2),
            None,
        ),
        record(
            CapabilityId::VulkanDevice,
            Availability::Degraded,
            Some(2),
            None,
            Some(2),
            Some(DiagnosticCategory::TemporaryFailure),
        ),
        record(
            CapabilityId::GpuDevice,
            Availability::Available,
            Some(3),
            None,
            Some(2),
            None,
        ),
        record(
            CapabilityId::GraphicsDriver,
            Availability::Available,
            Some(3),
            None,
            Some(2),
            None,
        ),
        record(
            CapabilityId::Vram,
            Availability::Available,
            Some(3),
            measured(2 * 1024 * 1024 * 1024, MeasurementUnit::Bytes),
            Some(2),
            None,
        ),
        record(
            CapabilityId::HardwareAcceleration,
            Availability::Degraded,
            Some(3),
            None,
            Some(2),
            Some(DiagnosticCategory::PermissionDenied),
        ),
        record(
            CapabilityId::GpuThermal,
            Availability::Unknown,
            Some(3),
            None,
            Some(2),
            Some(DiagnosticCategory::NotYetProbed),
        ),
        record(
            CapabilityId::PowerTelemetry,
            Availability::Unknown,
            None,
            None,
            Some(2),
            Some(DiagnosticCategory::MissingProvider),
        ),
        record(
            CapabilityId::Ram,
            Availability::Available,
            None,
            measured(16 * 1024 * 1024 * 1024, MeasurementUnit::Bytes),
            Some(2),
            None,
        ),
        record(
            CapabilityId::Display,
            Availability::Available,
            Some(4),
            None,
            Some(2),
            None,
        ),
    ];
    snapshot
}

pub fn incomplete_fixture() -> SystemSnapshot {
    let platform = PlatformIdentity::new(
        PlatformFamily::OtherUnknown,
        OsIdentity::new("Unknown", None).unwrap(),
        Architecture::Unknown,
        None,
        None,
    )
    .unwrap();
    let mut snapshot = SystemSnapshot::new(platform);
    snapshot.memory = Some(MemoryInfo {
        ram: CapabilityValue::new(Availability::Unknown, None),
        swap: SwapInfo {
            availability: Availability::Unknown,
            present: None,
            total: None,
        },
    });
    snapshot
        .provenance
        .push(source(3, "synthetic-incomplete-fixture"));
    snapshot.capabilities = vec![
        record(
            CapabilityId::GpuDevice,
            Availability::Unknown,
            None,
            None,
            Some(3),
            Some(DiagnosticCategory::MissingProvider),
        ),
        record(
            CapabilityId::Display,
            Availability::Unknown,
            None,
            None,
            Some(3),
            Some(DiagnosticCategory::MissingProvider),
        ),
        record(
            CapabilityId::CpuThermal,
            Availability::Unavailable,
            None,
            None,
            Some(3),
            Some(DiagnosticCategory::UnsupportedCapability),
        ),
    ];
    snapshot.diagnostics.push(Diagnostic {
        category: DiagnosticCategory::TemporaryFailure,
        provider: Some("synthetic-incomplete-fixture".to_owned()),
        message: Some("synthetic recoverable inventory failure".to_owned()),
    });
    snapshot
}

pub fn report(snapshot: &SystemSnapshot) -> ProbeReport {
    ProbeReport {
        cpu: snapshot.cpu.clone(),
        gpus: snapshot.gpus.clone(),
        memory: snapshot.memory.clone(),
        displays: snapshot.displays.clone(),
        capabilities: snapshot.capabilities.clone(),
        provenance: snapshot.provenance.clone(),
        diagnostics: snapshot.diagnostics.clone(),
    }
}
