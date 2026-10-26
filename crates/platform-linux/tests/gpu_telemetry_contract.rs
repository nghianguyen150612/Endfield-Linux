mod common;
use endfield_unix_domain::MeasurementValue;
use endfield_unix_platform_linux::{gpu_nvidia::*, gpu_telemetry::*, gpu_telemetry_sources::*, *};
use endfield_unix_telemetry::*;
use std::{
    fs,
    os::unix::fs::symlink,
    sync::atomic::{AtomicUsize, Ordering},
};
use tempfile::TempDir;
fn base() -> endfield_unix_domain::SystemSnapshot {
    LinuxPlatformProbe::with_source(common::FixtureSource::default())
        .inspect()
        .unwrap()
        .snapshot
}
fn fixture(devices: &[(&str, &str, &str, &str)]) -> (TempDir, SystemLinuxDataSource) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("sys/bus/pci/devices")).unwrap();
    fs::create_dir_all(dir.path().join("sys/bus/pci/drivers")).unwrap();
    fs::create_dir_all(dir.path().join("sys/devices")).unwrap();
    for (bdf, vendor, driver, power) in devices {
        let root = dir.path().join(format!("sys/devices/pci0000:00/{bdf}"));
        fs::create_dir_all(root.join("power")).unwrap();
        for (name, value) in [
            ("class", "0x030000"),
            ("vendor", vendor),
            ("device", "0x1234"),
            ("subsystem_vendor", vendor),
            ("subsystem_device", "0x0001"),
            ("power/runtime_status", power),
        ] {
            fs::write(root.join(name), value).unwrap();
        }
        let drv = dir.path().join(format!("sys/bus/pci/drivers/{driver}"));
        fs::create_dir_all(&drv).unwrap();
        symlink(drv, root.join("driver")).unwrap();
        symlink(root, dir.path().join(format!("sys/bus/pci/devices/{bdf}"))).unwrap();
    }
    let source = SystemLinuxDataSource::rooted_at(dir.path());
    (dir, source)
}
fn context(source: &impl LinuxGpuDataSource) -> GpuIdentityContext {
    GpuIdentityContext::discover(source, base(), SnapshotRefId::new(42).unwrap()).unwrap()
}
fn request(metrics: Vec<MetricId>) -> CollectionRequest {
    CollectionRequest::new(SamplingContextId::new(1).unwrap(), metrics, None).unwrap()
}
#[derive(Default)]
struct Native {
    calls: AtomicUsize,
    readings: Vec<NvidiaReading>,
    error: Option<SensorError>,
}
impl NvidiaBackend for Native {
    fn collect(&self, _: &[PciBdf], _: &[MetricId]) -> Result<Vec<NvidiaReading>, SensorError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.error.map_or_else(|| Ok(self.readings.clone()), Err)
    }
}
fn value(
    report: &ProviderCollection,
    metric: MetricId,
    target: ObservationTarget,
) -> Option<MeasurementValue> {
    report
        .batch()
        .observations()
        .iter()
        .find(|o| o.metric() == metric && o.target() == target)
        .and_then(|o| match o.state() {
            ObservationState::Measured(v) => Some(v.value()),
            _ => None,
        })
}
#[test]
fn synthetic_nvidia_rtx5070_gauges_are_pci_correlated_not_enumeration_correlated() {
    // Synthetic RTX 5070 fixture, not a physical hardware result.
    let (_dir, source) = fixture(&[("0000:01:00.0", "0x10de", "nvidia", "active")]);
    let ctx = context(&source);
    let target = ctx.target(0).unwrap();
    let native = Native {
        readings: vec![NvidiaReading {
            bdf: "0000:01:00.0".parse().unwrap(),
            values: vec![
                (MetricId::GpuMemoryUsed, Ok(MeasurementValue::Unsigned(0))),
                (
                    MetricId::GpuDedicatedMemoryCapacity,
                    Ok(MeasurementValue::Unsigned(12 << 30)),
                ),
                (
                    MetricId::GpuTemperature,
                    Ok(MeasurementValue::Signed(42000)),
                ),
                (MetricId::GpuPower, Ok(MeasurementValue::Unsigned(45000))),
                (
                    MetricId::GpuFrequency,
                    Ok(MeasurementValue::Unsigned(1_200_000_000)),
                ),
            ],
        }],
        ..Default::default()
    };
    let p = LinuxGpuTelemetryProvider::new(source, native, Some(ctx));
    let report = p.collect(&request(GPU_METRICS.to_vec())).unwrap();
    report.batch().validate().unwrap();
    assert_eq!(
        value(&report, MetricId::GpuMemoryUsed, target),
        Some(MeasurementValue::Unsigned(0))
    );
    assert_eq!(
        value(&report, MetricId::GpuTemperature, target),
        Some(MeasurementValue::Signed(42000))
    );
    assert_eq!(value(&report, MetricId::GpuUtilization, target), None);
    assert_eq!(report.completeness(), CollectionCompleteness::Partial);
}
#[test]
fn hybrid_iris_xe_mx350_suspension_skips_native_and_keeps_targets_separate() {
    let (dir, source) = fixture(&[
        ("0000:00:02.0", "0x8086", "i915", "active"),
        ("0000:01:00.0", "0x10de", "nvidia", "suspended"),
    ]);
    let drm = dir
        .path()
        .join("sys/devices/pci0000:00/0000:00:02.0/drm/card7");
    fs::create_dir_all(&drm).unwrap();
    fs::write(drm.join("gt_act_freq_mhz"), "350").unwrap();
    let ctx = context(&source);
    let intel = ctx.target(0).unwrap();
    let nvidia = ctx.target(1).unwrap();
    struct MustNotQuery;
    impl NvidiaBackend for MustNotQuery {
        fn collect(&self, _: &[PciBdf], _: &[MetricId]) -> Result<Vec<NvidiaReading>, SensorError> {
            panic!("suspended native GPU must not be queried")
        }
    }
    let report = LinuxGpuTelemetryProvider::new(source, MustNotQuery, Some(ctx))
        .collect(&request(GPU_METRICS.to_vec()))
        .unwrap();
    assert_eq!(
        value(&report, MetricId::GpuFrequency, intel),
        Some(MeasurementValue::Unsigned(350_000_000))
    );
    for m in GPU_METRICS {
        assert_eq!(value(&report, m, nvidia), None);
    }
    assert_eq!(
        value(&report, MetricId::GpuDedicatedMemoryCapacity, intel),
        None
    );
    report.batch().validate().unwrap();
}
#[test]
fn amdgpu_vram_signed_edge_temperature_and_power_are_independent() {
    let (dir, source) = fixture(&[("0002:02:00.0", "0x1002", "amdgpu", "active")]);
    let root = dir.path().join("sys/devices/pci0000:00/0002:02:00.0");
    fs::write(root.join("mem_info_vram_total"), "8589934592").unwrap();
    fs::write(root.join("mem_info_vram_used"), "0").unwrap();
    fs::write(root.join("gpu_busy_percent"), "50").unwrap();
    fs::create_dir_all(root.join("hwmon/hwmon12")).unwrap();
    for (n, v) in [
        ("name", "amdgpu\n"),
        ("temp1_input", "-1250"),
        ("power1_input", "12345999"),
        ("power1_cap", "999999999"),
    ] {
        fs::write(root.join("hwmon/hwmon12").join(n), v).unwrap();
    }
    let ctx = context(&source);
    let t = ctx.target(0).unwrap();
    let p = LinuxGpuTelemetryProvider::new(source.clone(), Native::default(), Some(ctx));
    let r = p.collect(&request(GPU_METRICS.to_vec())).unwrap();
    assert_eq!(
        value(&r, MetricId::GpuTemperature, t),
        Some(MeasurementValue::Signed(-1250))
    );
    assert_eq!(
        value(&r, MetricId::GpuPower, t),
        Some(MeasurementValue::Unsigned(12345))
    );
    assert_eq!(
        value(&r, MetricId::GpuMemoryUsed, t),
        Some(MeasurementValue::Unsigned(0))
    );
    assert_eq!(value(&r, MetricId::GpuUtilization, t), None);
    assert_eq!(value(&r, MetricId::GpuFrequency, t), None);
    fs::write(root.join("mem_info_vram_used"), "8589934593").unwrap();
    fs::write(root.join("hwmon/hwmon12/power1_input"), "bad").unwrap();
    let r = p.collect(&request(GPU_METRICS.to_vec())).unwrap();
    assert_eq!(value(&r, MetricId::GpuMemoryUsed, t), None);
    assert_eq!(value(&r, MetricId::GpuPower, t), None);
    assert!(value(&r, MetricId::GpuTemperature, t).is_some());
}
#[test]
fn intel_xe_and_shared_memory_are_explicitly_unsupported() {
    let (_dir, source) = fixture(&[("0000:00:02.0", "0x8086", "xe", "active")]);
    let ctx = context(&source);
    let r = LinuxGpuTelemetryProvider::new(source, Native::default(), Some(ctx))
        .collect(&request(GPU_METRICS.to_vec()))
        .unwrap();
    assert!(
        r.batch()
            .observations()
            .iter()
            .all(|o| !o.state().is_measured())
    );
    r.batch().validate().unwrap();
}
#[test]
fn headless_missing_inventory_and_unrequested_metrics_do_not_fabricate_data() {
    let (_dir, source) = fixture(&[]);
    let ctx = context(&source);
    let r = LinuxGpuTelemetryProvider::new(source.clone(), Native::default(), Some(ctx))
        .collect(&request(GPU_METRICS.to_vec()))
        .unwrap();
    assert!(r.batch().observations().is_empty());
    assert_eq!(r.completeness(), CollectionCompleteness::Partial);
    let p = LinuxGpuTelemetryProvider::new(source, Native::default(), None);
    assert!(
        p.collect(&request(vec![MetricId::GpuTemperature]))
            .unwrap()
            .batch()
            .observations()
            .is_empty()
    );
    assert_eq!(
        p.collect(&request(vec![MetricId::RamUsed]))
            .unwrap()
            .completeness(),
        CollectionCompleteness::Complete
    );
}
#[test]
fn same_vendor_domains_and_conflicting_native_handles_never_cross_assign() {
    let (_dir, source) = fixture(&[
        ("0000:01:00.0", "0x10de", "nvidia", "active"),
        ("0002:01:00.0", "0x10de", "nvidia", "active"),
    ]);
    let ctx = context(&source);
    let a = ctx.target(0).unwrap();
    let b = ctx.target(1).unwrap();
    assert_ne!(a, b);
    let row = NvidiaReading {
        bdf: "0000:01:00.0".parse().unwrap(),
        values: vec![(
            MetricId::GpuTemperature,
            Ok(MeasurementValue::Signed(40000)),
        )],
    };
    let p = LinuxGpuTelemetryProvider::new(
        source,
        Native {
            readings: vec![row.clone(), row],
            ..Default::default()
        },
        Some(ctx),
    );
    let r = p.collect(&request(vec![MetricId::GpuTemperature])).unwrap();
    assert_eq!(value(&r, MetricId::GpuTemperature, a), None);
    assert_eq!(value(&r, MetricId::GpuTemperature, b), None);
}
#[test]
fn native_failure_categories_preserve_partial_valid_batches() {
    for error in [
        SensorError::Missing,
        SensorError::Denied,
        SensorError::Temporary,
        SensorError::Unsupported,
        SensorError::Malformed,
        SensorError::Ambiguous,
    ] {
        let (_dir, source) = fixture(&[("0000:01:00.0", "0x10de", "nvidia", "active")]);
        let ctx = context(&source);
        let r = LinuxGpuTelemetryProvider::new(
            source,
            Native {
                error: Some(error),
                ..Default::default()
            },
            Some(ctx),
        )
        .collect(&request(vec![MetricId::GpuTemperature]))
        .unwrap();
        r.batch().validate().unwrap();
        assert!(!r.batch().observations()[0].state().is_measured());
        assert_eq!(r.issues().len(), 1);
    }
}
#[test]
fn conversions_reject_bad_units_overflow_and_out_of_range_utilization() {
    for (p, b) in [(0, 0), (25, 2500), (50, 5000), (100, 10000)] {
        assert_eq!(percent_to_basis_points(p), Ok(b));
    }
    assert!(percent_to_basis_points(101).is_err());
    assert_eq!(mhz_to_hz(350), Ok(350000000));
    assert!(mhz_to_hz(u64::MAX).is_err());
    assert_eq!(microwatts_to_milliwatts(u64::MAX), u64::MAX / 1000);
    for bad in [b"12 W".as_slice(), b"-1", b"", b"18446744073709551616"] {
        assert!(unsigned(bad).is_err());
    }
    assert_eq!(signed(b"-1200"), Ok(-1200));
}
#[test]
fn ambiguous_hwmon_and_power_cap_only_are_not_measurements() {
    let (dir, source) = fixture(&[("0000:02:00.0", "0x1002", "amdgpu", "active")]);
    let root = dir.path().join("sys/devices/pci0000:00/0000:02:00.0/hwmon");
    fs::create_dir_all(root.join("hwmon0")).unwrap();
    fs::write(root.join("hwmon0/name"), "amdgpu\n").unwrap();
    fs::write(root.join("hwmon0/power1_cap"), "1000000").unwrap();
    let bdf = "0000:02:00.0".parse().unwrap();
    assert_eq!(
        source.hwmon(bdf, "amdgpu", MetricId::GpuPower),
        Err(SensorError::Missing)
    );
    fs::create_dir_all(root.join("hwmon1")).unwrap();
    fs::write(root.join("hwmon1/name"), "amdgpu\n").unwrap();
    assert_eq!(
        source.hwmon(bdf, "amdgpu", MetricId::GpuTemperature),
        Err(SensorError::Ambiguous)
    );
}
#[test]
fn sensor_symlinks_outside_the_verified_gpu_are_rejected() {
    let (dir, source) = fixture(&[("0000:02:00.0", "0x1002", "amdgpu", "active")]);
    let root = dir.path().join("sys/devices/pci0000:00/0000:02:00.0");
    symlink("/etc/passwd", root.join("mem_info_vram_used")).unwrap();
    assert_eq!(
        source.attribute("0000:02:00.0".parse().unwrap(), GpuAttribute::VramUsed),
        Err(SensorError::Malformed)
    );
}

#[test]
fn missing_unknown_power_driver_rebind_and_unrequested_sensor_are_conservative() {
    let (dir, source) = fixture(&[("0000:01:00.0", "0x10de", "nvidia", "unknown")]);
    let ctx = context(&source);
    struct NoNative;
    impl NvidiaBackend for NoNative {
        fn collect(&self, _: &[PciBdf], _: &[MetricId]) -> Result<Vec<NvidiaReading>, SensorError> {
            panic!("native collection forbidden")
        }
    }
    let p = LinuxGpuTelemetryProvider::new(source.clone(), NoNative, Some(ctx));
    let r = p.collect(&request(vec![MetricId::GpuTemperature])).unwrap();
    assert_eq!(r.batch().observations().len(), 1);
    assert!(!r.batch().observations()[0].state().is_measured());
    let root = dir.path().join("sys/devices/pci0000:00/0000:01:00.0");
    fs::write(root.join("power/runtime_status"), "active").unwrap();
    fs::write(root.join("device"), "0x5678").unwrap();
    let r = p.collect(&request(vec![MetricId::GpuTemperature])).unwrap();
    assert!(!r.batch().observations()[0].state().is_measured());
}
#[test]
fn identity_ordering_is_stable_and_invalid_inventory_is_rejected() {
    let (_dir, source) = fixture(&[
        ("0002:01:00.0", "0x8086", "i915", "active"),
        ("0000:01:00.0", "0x8086", "i915", "suspended"),
    ]);
    let ctx = context(&source);
    assert_eq!(ctx.devices()[0].bdf, "0000:01:00.0");
    assert_eq!(ctx.target(0), context(&source).target(0));
    assert!(ctx.target(2).is_none());
    struct Bad;
    impl LinuxGpuDataSource for Bad {
        fn enumerate_pci_entries(&self) -> Result<Vec<String>, DataSourceError> {
            Ok(vec!["../../bad".to_owned()])
        }
        fn read_pci_attribute(&self, _: &str, _: PciAttribute) -> Result<Vec<u8>, DataSourceError> {
            panic!("invalid identity must be rejected first")
        }
        fn enumerate_drm_nodes(&self) -> Result<Vec<DrmNodeObservation>, DataSourceError> {
            panic!("telemetry must not enumerate DRM inventory")
        }
    }
    assert!(GpuIdentityContext::discover(&Bad, base(), SnapshotRefId::new(1).unwrap()).is_err());
}
#[test]
fn intermediate_sensor_directory_symlinks_cannot_escape_gpu() {
    let (dir, source) = fixture(&[("0000:02:00.0", "0x1002", "amdgpu", "active")]);
    let root = dir.path().join("sys/devices/pci0000:00/0000:02:00.0");
    fs::remove_file(root.join("power/runtime_status")).unwrap();
    fs::remove_dir(root.join("power")).unwrap();
    let outside = dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("runtime_status"), "active").unwrap();
    symlink(outside, root.join("power")).unwrap();
    assert_eq!(
        source.power("0000:02:00.0".parse().unwrap()),
        RuntimePower::Unknown
    );
}
#[test]
fn existing_p013_report_mapping_is_validated_and_amd_capacity_needs_discrete_evidence() {
    let (dir, source) = fixture(&[("0000:02:00.0", "0x1002", "amdgpu", "active")]);
    struct NoVulkan;
    impl VulkanBackend for NoVulkan {
        fn enumerate(&self) -> VulkanFacts {
            VulkanFacts {
                status: VulkanLoaderStatus::Missing,
                devices: vec![],
                diagnostic: None,
            }
        }
    }
    let mut report = LinuxGpuProbe::with_source_and_vulkan(source.clone(), NoVulkan)
        .inspect()
        .unwrap();
    let reference = SnapshotRefId::new(13).unwrap();
    assert!(GpuIdentityContext::from_report(&report, reference).is_ok());
    report.pci_devices[0].bdf = "invalid".to_owned();
    assert!(GpuIdentityContext::from_report(&report, reference).is_err());
    report.pci_devices[0].bdf = "0000:02:00.0".to_owned();
    report.snapshot.gpus[0].class = endfield_unix_domain::GpuClass::Discrete;
    let context = GpuIdentityContext::from_report(&report, reference).unwrap();
    let target = context.target(0).unwrap();
    fs::write(
        dir.path()
            .join("sys/devices/pci0000:00/0000:02:00.0/mem_info_vram_total"),
        "8589934592",
    )
    .unwrap();
    let r = LinuxGpuTelemetryProvider::new(source, Native::default(), Some(context))
        .collect(&request(vec![MetricId::GpuDedicatedMemoryCapacity]))
        .unwrap();
    assert_eq!(
        value(&r, MetricId::GpuDedicatedMemoryCapacity, target),
        Some(MeasurementValue::Unsigned(8589934592))
    );
}
