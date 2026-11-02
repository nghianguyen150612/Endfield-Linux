use endfield_unix_domain::{Availability, CapabilityId, PlatformFamily};
use endfield_unix_platform_linux::{
    CgroupSource, CpuListSource, CpuTopologyField, DataSourceError, DataSourceErrorKind,
    DeviceNodeAccess, DrmNodeKind, DrmNodeObservation, LinuxDataSource, LinuxGpuDataSource,
    LinuxGpuProbe, OsReleaseFile, PciAttribute, PlatformProfileField, RawKernelIdentity,
    RawSchedulerState, RuntimeDirectoryMetadata, VulkanBackend, VulkanDeviceType, VulkanFacts,
    VulkanLoaderStatus, VulkanMetadataStatus, VulkanPhysicalDevice,
};
use std::path::{Path, PathBuf};

#[derive(Clone)]
struct HybridFixture {
    missing_drm_nodes: bool,
    missing_nvidia_driver: bool,
}

impl Default for HybridFixture {
    fn default() -> Self {
        Self {
            missing_drm_nodes: false,
            missing_nvidia_driver: true,
        }
    }
}

#[derive(Clone)]
struct HeadlessFixture;

impl LinuxDataSource for HeadlessFixture {
    fn read_os_release(&self, source: OsReleaseFile) -> Result<Vec<u8>, DataSourceError> {
        HybridFixture::default().read_os_release(source)
    }
    fn kernel_identity(&self) -> Result<RawKernelIdentity, DataSourceError> {
        HybridFixture::default().kernel_identity()
    }
    fn container_marker_present(
        &self,
        marker: endfield_unix_platform_linux::ContainerMarker,
    ) -> Result<bool, DataSourceError> {
        HybridFixture::default().container_marker_present(marker)
    }
    fn runtime_directory_path(&self) -> Result<Option<PathBuf>, DataSourceError> {
        HybridFixture::default().runtime_directory_path()
    }
    fn runtime_directory_metadata(
        &self,
        path: &Path,
    ) -> Result<RuntimeDirectoryMetadata, DataSourceError> {
        HybridFixture::default().runtime_directory_metadata(path)
    }
    fn effective_uid(&self) -> u32 {
        HybridFixture::default().effective_uid()
    }
}

impl endfield_unix_platform_linux::LinuxHardwareDataSource for HeadlessFixture {
    fn read_proc_cpuinfo(&self) -> Result<Vec<u8>, DataSourceError> {
        HybridFixture::default().read_proc_cpuinfo()
    }
    fn read_proc_meminfo(&self) -> Result<Vec<u8>, DataSourceError> {
        HybridFixture::default().read_proc_meminfo()
    }
    fn read_cpu_list(&self, source: CpuListSource) -> Result<Vec<u8>, DataSourceError> {
        HybridFixture::default().read_cpu_list(source)
    }
    fn read_cpu_topology_field(
        &self,
        cpu: u32,
        field: CpuTopologyField,
    ) -> Result<Option<i64>, DataSourceError> {
        HybridFixture::default().read_cpu_topology_field(cpu, field)
    }
    fn process_affinity(&self) -> Result<Vec<u32>, DataSourceError> {
        HybridFixture::default().process_affinity()
    }
    fn scheduler_state(&self) -> Result<RawSchedulerState, DataSourceError> {
        HybridFixture::default().scheduler_state()
    }
    fn read_platform_profile_field(
        &self,
        field: PlatformProfileField,
    ) -> Result<Option<Vec<u8>>, DataSourceError> {
        HybridFixture::default().read_platform_profile_field(field)
    }
    fn cgroup_source(&self) -> Result<CgroupSource, DataSourceError> {
        HybridFixture::default().cgroup_source()
    }
}

impl LinuxGpuDataSource for HeadlessFixture {
    fn enumerate_pci_entries(&self) -> Result<Vec<String>, DataSourceError> {
        Ok(Vec::new())
    }
    fn read_pci_attribute(&self, _: &str, _: PciAttribute) -> Result<Vec<u8>, DataSourceError> {
        Err(DataSourceError::new(DataSourceErrorKind::NotFound))
    }
    fn enumerate_drm_nodes(&self) -> Result<Vec<DrmNodeObservation>, DataSourceError> {
        Ok(Vec::new())
    }
}

impl LinuxDataSource for HybridFixture {
    fn read_os_release(&self, _: OsReleaseFile) -> Result<Vec<u8>, DataSourceError> {
        Ok(b"ID=ubuntu\nNAME=Ubuntu\nPRETTY_NAME=Ubuntu 24.04\nVERSION_ID=24.04\n".to_vec())
    }
    fn kernel_identity(&self) -> Result<RawKernelIdentity, DataSourceError> {
        Ok(RawKernelIdentity {
            system_name: "Linux".to_owned(),
            release: "fixture-kernel".to_owned(),
            machine: "x86_64".to_owned(),
        })
    }
    fn container_marker_present(
        &self,
        _: endfield_unix_platform_linux::ContainerMarker,
    ) -> Result<bool, DataSourceError> {
        Ok(false)
    }
    fn runtime_directory_path(&self) -> Result<Option<PathBuf>, DataSourceError> {
        Ok(None)
    }
    fn runtime_directory_metadata(
        &self,
        _: &Path,
    ) -> Result<RuntimeDirectoryMetadata, DataSourceError> {
        Err(DataSourceError::new(DataSourceErrorKind::NotFound))
    }
    fn effective_uid(&self) -> u32 {
        1000
    }
}

impl endfield_unix_platform_linux::LinuxHardwareDataSource for HybridFixture {
    fn read_proc_cpuinfo(&self) -> Result<Vec<u8>, DataSourceError> {
        Ok(b"model name : Synthetic hybrid CPU\n".to_vec())
    }
    fn read_proc_meminfo(&self) -> Result<Vec<u8>, DataSourceError> {
        Ok(
            b"MemTotal: 16384000 kB\nMemAvailable: 12000000 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n"
                .to_vec(),
        )
    }
    fn read_cpu_list(&self, _: CpuListSource) -> Result<Vec<u8>, DataSourceError> {
        Ok(b"0\n".to_vec())
    }
    fn read_cpu_topology_field(
        &self,
        _: u32,
        field: CpuTopologyField,
    ) -> Result<Option<i64>, DataSourceError> {
        Ok(Some(match field {
            CpuTopologyField::PhysicalPackageId
            | CpuTopologyField::CoreId
            | CpuTopologyField::DieId => 0,
        }))
    }
    fn process_affinity(&self) -> Result<Vec<u32>, DataSourceError> {
        Ok(vec![0])
    }
    fn scheduler_state(&self) -> Result<RawSchedulerState, DataSourceError> {
        Ok(RawSchedulerState {
            policy: 0,
            priority: 0,
        })
    }
    fn read_platform_profile_field(
        &self,
        _: PlatformProfileField,
    ) -> Result<Option<Vec<u8>>, DataSourceError> {
        Ok(None)
    }
    fn cgroup_source(&self) -> Result<CgroupSource, DataSourceError> {
        Ok(CgroupSource::None)
    }
}

impl LinuxGpuDataSource for HybridFixture {
    fn enumerate_pci_entries(&self) -> Result<Vec<String>, DataSourceError> {
        Ok(vec!["0000:00:02.0".to_owned(), "0001:01:00.0".to_owned()])
    }
    fn read_pci_attribute(
        &self,
        bdf: &str,
        field: PciAttribute,
    ) -> Result<Vec<u8>, DataSourceError> {
        let intel = bdf == "0000:00:02.0";
        match (field, intel) {
            (PciAttribute::Class, true) => Ok(b"0x030000\n".to_vec()),
            (PciAttribute::Class, false) => Ok(b"0x030200\n".to_vec()),
            (PciAttribute::Vendor, true) => Ok(b"0x8086\n".to_vec()),
            (PciAttribute::Vendor, false) => Ok(b"0x10de\n".to_vec()),
            (PciAttribute::Device, true) => Ok(b"0x9a49\n".to_vec()),
            (PciAttribute::Device, false) => Ok(b"0x1fef\n".to_vec()),
            (PciAttribute::SubsystemVendor | PciAttribute::SubsystemDevice, _) => {
                Ok(b"0x0000\n".to_vec())
            }
            (PciAttribute::Driver, true) => Ok(b"i915".to_vec()),
            (PciAttribute::Driver, false) if self.missing_nvidia_driver => {
                Err(DataSourceError::new(DataSourceErrorKind::NotFound))
            }
            (PciAttribute::Driver, false) => Ok(b"nvidia".to_vec()),
        }
    }
    fn enumerate_drm_nodes(&self) -> Result<Vec<DrmNodeObservation>, DataSourceError> {
        if self.missing_drm_nodes {
            return Ok(Vec::new());
        }
        Ok(vec![
            DrmNodeObservation {
                name: "card0".to_owned(),
                kind: DrmNodeKind::Primary,
                pci_bdf: Some("0001:01:00.0".to_owned()),
                access: DeviceNodeAccess::Accessible,
            },
            DrmNodeObservation {
                name: "card1".to_owned(),
                kind: DrmNodeKind::Primary,
                pci_bdf: Some("0000:00:02.0".to_owned()),
                access: DeviceNodeAccess::Accessible,
            },
            DrmNodeObservation {
                name: "renderD128".to_owned(),
                kind: DrmNodeKind::Render,
                pci_bdf: Some("0000:00:02.0".to_owned()),
                access: DeviceNodeAccess::Accessible,
            },
            DrmNodeObservation {
                name: "renderD129".to_owned(),
                kind: DrmNodeKind::Render,
                pci_bdf: Some("0001:01:00.0".to_owned()),
                access: DeviceNodeAccess::Denied,
            },
        ])
    }
}

struct SyntheticVulkan;
impl VulkanBackend for SyntheticVulkan {
    fn enumerate(&self) -> VulkanFacts {
        VulkanFacts {
            status: VulkanLoaderStatus::Enumerated,
            diagnostic: None,
            devices: vec![
                VulkanPhysicalDevice {
                    name: "Synthetic NVIDIA MX350".to_owned(),
                    vendor_id: 0x10de,
                    device_id: 0x1fef,
                    pci_identity: Some("0001:01:00.0".parse().unwrap()),
                    driver_identity: None,
                    device_type: VulkanDeviceType::Discrete,
                    api_version: Some((1, 3, 280)),
                    supported_extensions: Vec::new(),
                    extensions_status: VulkanMetadataStatus::Unavailable,
                    supported_features: Vec::new(),
                    features_status: VulkanMetadataStatus::Available,
                    device_local_heap_bytes: vec![8_000_000_000],
                },
                VulkanPhysicalDevice {
                    name: "Synthetic Intel Iris Xe".to_owned(),
                    vendor_id: 0x8086,
                    device_id: 0x9a49,
                    pci_identity: Some("0000:00:02.0".parse().unwrap()),
                    driver_identity: None,
                    device_type: VulkanDeviceType::Integrated,
                    api_version: Some((1, 3, 280)),
                    supported_extensions: Vec::new(),
                    extensions_status: VulkanMetadataStatus::Unavailable,
                    supported_features: Vec::new(),
                    features_status: VulkanMetadataStatus::Available,
                    device_local_heap_bytes: vec![16_000_000_000],
                },
            ],
        }
    }
}

struct MissingLoader;
impl VulkanBackend for MissingLoader {
    fn enumerate(&self) -> VulkanFacts {
        VulkanFacts {
            status: VulkanLoaderStatus::Missing,
            devices: Vec::new(),
            diagnostic: Some("synthetic missing loader"),
        }
    }
}

struct NoVulkanDevices;
impl VulkanBackend for NoVulkanDevices {
    fn enumerate(&self) -> VulkanFacts {
        VulkanFacts {
            status: VulkanLoaderStatus::AvailableNoDevices,
            devices: Vec::new(),
            diagnostic: None,
        }
    }
}

struct VulkanInitFailure;
impl VulkanBackend for VulkanInitFailure {
    fn enumerate(&self) -> VulkanFacts {
        VulkanFacts {
            status: VulkanLoaderStatus::InitializationFailed,
            devices: Vec::new(),
            diagnostic: Some("synthetic initialization failure"),
        }
    }
}

struct MalformedVulkan;
impl VulkanBackend for MalformedVulkan {
    fn enumerate(&self) -> VulkanFacts {
        VulkanFacts {
            status: VulkanLoaderStatus::Enumerated,
            diagnostic: None,
            devices: vec![VulkanPhysicalDevice {
                name: "x".repeat(257),
                vendor_id: 0x10de,
                device_id: 1,
                pci_identity: None,
                driver_identity: None,
                device_type: VulkanDeviceType::Discrete,
                api_version: Some((1, 3, 0)),
                supported_extensions: Vec::new(),
                extensions_status: VulkanMetadataStatus::Unavailable,
                supported_features: Vec::new(),
                features_status: VulkanMetadataStatus::Available,
                device_local_heap_bytes: vec![1],
            }],
        }
    }
}

struct SoftwareVulkan;
impl VulkanBackend for SoftwareVulkan {
    fn enumerate(&self) -> VulkanFacts {
        VulkanFacts {
            status: VulkanLoaderStatus::Enumerated,
            diagnostic: None,
            devices: vec![VulkanPhysicalDevice {
                name: "Synthetic CPU Vulkan renderer".to_owned(),
                vendor_id: 0x10005,
                device_id: 1,
                pci_identity: None,
                driver_identity: None,
                device_type: VulkanDeviceType::Cpu,
                api_version: Some((1, 3, 0)),
                supported_extensions: Vec::new(),
                extensions_status: VulkanMetadataStatus::Unavailable,
                supported_features: Vec::new(),
                features_status: VulkanMetadataStatus::Available,
                device_local_heap_bytes: vec![1024 * 1024 * 1024],
            }],
        }
    }
}

#[test]
fn hybrid_multi_gpu_snapshot_correlates_out_of_order_vulkan_and_preserves_cpu_ram() {
    let report = LinuxGpuProbe::with_source_and_vulkan(HybridFixture::default(), SyntheticVulkan)
        .inspect()
        .unwrap();
    report.snapshot.validate().unwrap();
    assert_eq!(report.snapshot.platform.family, PlatformFamily::Linux);
    assert!(report.snapshot.cpu.is_some());
    assert!(
        report
            .snapshot
            .memory
            .as_ref()
            .unwrap()
            .ram
            .value
            .as_ref()
            .unwrap()
            .total
            .is_some()
    );
    assert_eq!(report.snapshot.gpus.len(), 2);
    assert_eq!(report.snapshot.gpus[0].instance.get(), 2);
    assert_eq!(report.snapshot.gpus[1].instance.get(), 3);
    assert_eq!(
        report.snapshot.gpus[0].class,
        endfield_unix_domain::GpuClass::Integrated
    );
    assert_eq!(
        report.snapshot.gpus[1].class,
        endfield_unix_domain::GpuClass::Discrete
    );
    assert_eq!(
        report.snapshot.gpus[0]
            .graphics_driver
            .as_ref()
            .unwrap()
            .name,
        "i915"
    );
    assert_eq!(
        report.snapshot.gpus[0]
            .graphics_driver
            .as_ref()
            .unwrap()
            .version,
        None
    );
    assert_eq!(
        report.snapshot.gpus[1].driver_availability,
        Availability::Unavailable
    );
    assert!(report.snapshot.gpus[1].graphics_driver.is_none());
    assert!(report.snapshot.displays.is_empty());
    assert!(
        report
            .snapshot
            .gpus
            .iter()
            .all(|gpu| gpu.vram.availability == Availability::Unknown && gpu.vram.value.is_none())
    );
    assert_eq!(
        report.vulkan_devices[0].gpu_instance,
        Some(report.snapshot.gpus[1].instance)
    );
    assert_eq!(
        report.vulkan_devices[1].gpu_instance,
        Some(report.snapshot.gpus[0].instance)
    );
    assert_eq!(report.pci_devices[0].bdf, "0000:00:02.0");
    assert!(
        report
            .drm_nodes
            .iter()
            .any(|node| node.name == "renderD129" && node.access == DeviceNodeAccess::Denied)
    );
    for capability in [
        CapabilityId::GpuDevice,
        CapabilityId::Vram,
        CapabilityId::HardwareAcceleration,
        CapabilityId::GraphicsDriver,
        CapabilityId::VulkanDevice,
    ] {
        assert_eq!(
            report
                .snapshot
                .capabilities
                .iter()
                .filter(|record| record.id == capability)
                .count(),
            2
        );
    }
    let provenance = report
        .snapshot
        .provenance
        .iter()
        .map(|item| item.id.get())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(provenance.len(), report.snapshot.provenance.len());
}

#[test]
fn missing_vulkan_does_not_fabricate_gpu_unavailable_or_erase_p012_facts() {
    let report = LinuxGpuProbe::with_source_and_vulkan(HybridFixture::default(), MissingLoader)
        .inspect()
        .unwrap();
    report.snapshot.validate().unwrap();
    assert_eq!(report.vulkan_status, VulkanLoaderStatus::Missing);
    assert!(report.snapshot.cpu.is_some());
    assert!(report.snapshot.memory.is_some());
    assert_eq!(report.snapshot.gpus.len(), 2);
    assert!(report.snapshot.gpus.iter().all(|gpu| {
        gpu.vulkan_device == Availability::Unknown
            && gpu.hardware_acceleration == Availability::Unknown
            && gpu.model.is_none()
    }));
}

#[test]
fn broken_gpu_stack_keeps_hardware_and_p012_facts_without_inventing_rendering() {
    let source = HybridFixture {
        missing_drm_nodes: true,
        missing_nvidia_driver: true,
    };
    let report = LinuxGpuProbe::with_source_and_vulkan(source, VulkanInitFailure)
        .inspect()
        .unwrap();
    report.snapshot.validate().unwrap();
    assert!(report.snapshot.cpu.is_some());
    assert!(report.snapshot.memory.is_some());
    assert_eq!(report.snapshot.gpus.len(), 2);
    assert!(report.drm_nodes.is_empty());
    assert_eq!(
        report.vulkan_status,
        VulkanLoaderStatus::InitializationFailed
    );
    assert_eq!(
        report.snapshot.gpus[1].driver_availability,
        Availability::Unavailable
    );
    assert!(report.snapshot.gpus.iter().all(|gpu| {
        gpu.vulkan_device == Availability::Unknown
            && gpu.hardware_acceleration == Availability::Unknown
            && gpu.vram.value.is_none()
    }));
    assert!(report.snapshot.gpus.iter().all(|gpu| {
        gpu.class == endfield_unix_domain::GpuClass::Unknown && gpu.model.is_none()
    }));
}

#[test]
fn cpu_vulkan_is_reported_as_software_without_claiming_gpu_acceleration() {
    let report = LinuxGpuProbe::with_source_and_vulkan(HybridFixture::default(), SoftwareVulkan)
        .inspect()
        .unwrap();
    report.snapshot.validate().unwrap();
    assert_eq!(
        report.vulkan_devices[0].physical.device_type,
        VulkanDeviceType::Cpu
    );
    assert_eq!(
        report.vulkan_devices[0].mapping,
        endfield_unix_platform_linux::VulkanMapping::Unmatched
    );
    assert!(
        report
            .snapshot
            .gpus
            .iter()
            .all(|gpu| gpu.hardware_acceleration == Availability::Unknown)
    );
    assert!(
        report
            .snapshot
            .gpus
            .iter()
            .all(|gpu| gpu.vram.value.is_none())
    );
}

#[test]
fn headless_system_keeps_cpu_and_ram_and_reports_only_software_vulkan() {
    let report = LinuxGpuProbe::with_source_and_vulkan(HeadlessFixture, SoftwareVulkan)
        .inspect()
        .unwrap();
    report.snapshot.validate().unwrap();
    assert!(report.snapshot.cpu.is_some());
    assert!(report.snapshot.memory.is_some());
    assert!(report.snapshot.gpus.is_empty());
    assert!(report.pci_devices.is_empty());
    assert_eq!(
        report.pci_status,
        endfield_unix_platform_linux::GpuSourceStatus::Available
    );
    assert_eq!(report.vulkan_devices.len(), 1);
    assert_eq!(
        report.vulkan_devices[0].mapping,
        endfield_unix_platform_linux::VulkanMapping::Unmatched
    );
}

#[test]
fn zero_devices_and_loader_initialization_failure_are_distinct_from_missing_loader() {
    let no_devices =
        LinuxGpuProbe::with_source_and_vulkan(HybridFixture::default(), NoVulkanDevices)
            .inspect()
            .unwrap();
    let failed = LinuxGpuProbe::with_source_and_vulkan(HeadlessFixture, VulkanInitFailure)
        .inspect()
        .unwrap();
    assert_eq!(
        no_devices.vulkan_status,
        VulkanLoaderStatus::AvailableNoDevices
    );
    assert!(no_devices.vulkan_devices.is_empty());
    assert!(no_devices.snapshot.gpus.iter().all(|gpu| {
        gpu.vulkan_device == Availability::Unavailable
            && gpu.hardware_acceleration == Availability::Unknown
    }));
    assert_eq!(
        failed.vulkan_status,
        VulkanLoaderStatus::InitializationFailed
    );
    assert!(failed.vulkan_devices.is_empty());
    assert_eq!(
        failed.vulkan_diagnostic,
        Some("synthetic initialization failure")
    );
    assert_ne!(no_devices.vulkan_status, VulkanLoaderStatus::Missing);
}

#[test]
fn invalid_oversized_vulkan_metadata_degrades_without_fabricating_a_match() {
    let report = LinuxGpuProbe::with_source_and_vulkan(HeadlessFixture, MalformedVulkan)
        .inspect()
        .unwrap();
    report.snapshot.validate().unwrap();
    assert_eq!(report.vulkan_status, VulkanLoaderStatus::Degraded);
    assert!(report.vulkan_devices.is_empty());
    assert!(!report.snapshot.diagnostics.is_empty());
}
