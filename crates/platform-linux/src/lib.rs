//! Read-only Linux platform and hardware capability discovery.
//!
//! This adapter reuses P007 domain types and keeps Linux source access behind
//! the bounded [`LinuxDataSource`] and [`LinuxHardwareDataSource`] interfaces.
//! It does not launch commands, write files, or require elevated privileges.

mod architecture;
mod cgroup;
mod cpu;
mod cpu_topology;
mod cpufreq;
mod environment;
mod errors;
mod gpu_probe;
mod gpu_sources;
mod hardware_common;
mod hardware_probe;
mod hardware_sources;
mod kernel;
mod memory;
mod normalize;
mod os_release;
mod pci;
mod probe;
mod scheduler;
mod sources;
mod vulkan;

pub use cgroup::{CgroupLimit, CgroupResourceFacts, CgroupStatus, CpuTimeQuota};
pub use cpu::{CpuInfoParseError, parse_cpu_model};
pub use cpu_topology::{
    CpuListParseError, CpuTopologyFacts, MAX_CPU_IDS, MAX_CPU_INDEX, MAX_CPU_LIST_BYTES,
    parse_cpu_list,
};
pub use cpufreq::{CpuFrequencyFacts, CpuFrequencyPolicyFacts, FrequencyRange, PlatformPowerFacts};
pub use environment::{
    EnvironmentClassification, EnvironmentConfidence, EnvironmentStatus,
    RuntimeDirectoryAssessment, RuntimeDirectoryStatus, RuntimeEnvironmentFacts,
};
pub use errors::{DataSourceError, DataSourceErrorKind, OsReleaseParseError};
pub use gpu_probe::{
    DISPLAY_INSTANCE_ID_BASE, GPU_INSTANCE_ID_BASE, GpuSourceStatus,
    LINUX_GPU_REPORT_SCHEMA_VERSION, LinuxGpuCompleteness, LinuxGpuProbe, LinuxGpuReport,
    LinuxVulkanDevice, PciGraphicsDevice, VulkanMapping,
};
pub use gpu_sources::{
    DeviceNodeAccess, DrmNodeKind, DrmNodeObservation, LinuxGpuDataSource, MAX_DRM_NODES,
    PciAttribute,
};
pub use hardware_probe::{
    LINUX_HARDWARE_REPORT_SCHEMA_VERSION, LinuxHardwareCompleteness, LinuxHardwareProbe,
    LinuxHardwareReport, LinuxMemoryFacts,
};
pub use hardware_sources::{
    CgroupHierarchy, CgroupSource, CgroupV2Control, CgroupV2Files, CpuFrequencyField,
    CpuListSource, CpuTopologyField, LinuxHardwareDataSource, PlatformProfileField,
    RawSchedulerState,
};
pub use kernel::RawKernelIdentity;
pub use memory::{
    MAX_MEMINFO_BYTES, MemInfoData, MemInfoFieldError, MemInfoParseError, parse_meminfo,
};
pub use normalize::LinuxDistributionMetadata;
pub use os_release::{MAX_OS_RELEASE_BYTES, OsReleaseData, parse_os_release};
pub use pci::{MAX_PCI_DEVICES, PciBdf, PciBdfError};
pub use probe::{
    LINUX_PLATFORM_REPORT_SCHEMA_VERSION, LinuxPlatformProbe, LinuxPlatformReport,
    LinuxReportCompleteness,
};
pub use scheduler::{QueryAvailability, SchedulingFacts};
pub use sources::{
    ContainerMarker, LinuxDataSource, OsReleaseFile, RuntimeDirectoryMetadata,
    SystemLinuxDataSource,
};
pub use vulkan::{
    SystemVulkanBackend, VulkanBackend, VulkanDeviceType, VulkanDriverIdentity, VulkanFacts,
    VulkanLoaderStatus, VulkanPhysicalDevice,
};
