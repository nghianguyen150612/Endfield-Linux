use endfield_unix_platform_linux::{LinuxGpuCompleteness, LinuxGpuProbe};

#[test]
fn live_gpu_probe_is_read_only_bounded_and_preserves_platform_hardware() {
    let report = LinuxGpuProbe::system()
        .inspect()
        .expect("Linux platform identity is available");
    report
        .snapshot
        .validate()
        .expect("combined snapshot validates");
    assert!(
        report.snapshot.memory.is_some(),
        "P012 memory record remains present"
    );
    assert!(report.pci_devices.len() <= 512);
    assert!(report.drm_nodes.len() <= 128);
    assert!(report.vulkan_devices.len() <= 64);
    assert_eq!(report.snapshot.gpus.len(), report.pci_devices.len());
    if matches!(
        report.vulkan_status,
        endfield_unix_platform_linux::VulkanLoaderStatus::Missing
    ) {
        assert!(report.vulkan_devices.is_empty());
    }
    if report.snapshot.gpus.is_empty() {
        assert!(report.snapshot.capabilities.iter().all(|capability| {
            capability
                .instance
                .is_none_or(|instance| instance.get() == 1)
        }));
    }
    println!(
        "live GPU smoke: platform={:?}, pci_status={:?}, pci_gpus={}, drm_status={:?}, drm_nodes={}, vulkan_status={:?}, vulkan_devices={}, completeness={:?}",
        report.snapshot.platform.family,
        report.pci_status,
        report.pci_devices.len(),
        report.drm_status,
        report.drm_nodes.len(),
        report.vulkan_status,
        report.vulkan_devices.len(),
        report.completeness
    );
    if let Some(detail) = report.vulkan_diagnostic {
        println!("live GPU smoke: Vulkan detail={detail}");
    }
    println!(
        "live GPU smoke: cpu_present={}, memory_present={}, snapshot_valid=true",
        report.snapshot.cpu.is_some(),
        report.snapshot.memory.is_some()
    );
    if report.completeness == LinuxGpuCompleteness::Partial {
        println!("live GPU smoke: optional providers reported partial observations");
    }
}
