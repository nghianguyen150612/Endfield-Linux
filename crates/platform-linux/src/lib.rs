//! Read-only Linux platform and hardware capability discovery.
//!
//! This adapter reuses P007 domain types and keeps Linux source access behind
//! the bounded [`LinuxDataSource`] and [`LinuxHardwareDataSource`] interfaces.
//! It does not launch commands, write files, or require elevated privileges.

mod architecture;
mod cgroup;
mod cpu;
mod cpu_stat;
mod cpu_telemetry;
mod cpu_topology;
mod cpufreq;
mod display;
mod display_mapping;
mod display_probe;
mod display_sources;
mod drm_connectors;
mod environment;
mod errors;
mod frame_csv;
mod frame_events;
mod frame_mangohud;
mod frame_sources;
mod frame_statistics;
mod frame_telemetry;
mod gpu_probe;
mod gpu_sources;
mod hardware_common;
mod hardware_probe;
mod hardware_sources;
mod kernel;
mod memory;
mod memory_telemetry;
mod normalize;
mod os_release;
mod pci;
mod probe;
mod scheduler;
mod session;
mod sources;
mod telemetry_clock;
mod telemetry_probe;
mod vulkan;
mod wayland;
mod x11;

pub use cgroup::{CgroupLimit, CgroupResourceFacts, CgroupStatus, CpuTimeQuota};
pub use cpu::{CpuInfoParseError, parse_cpu_model};
pub use cpu_stat::{
    CpuCounters, CpuRecordError, CpuStatParseError, CpuStatSnapshot, MAX_CPU_STAT_RECORDS,
    MAX_PROC_STAT_BYTES, MAX_PROC_STAT_LINE_BYTES, calculate_cpu_utilization_basis_points,
    parse_proc_stat,
};
pub use cpu_telemetry::{CpuUtilizationError, calculate_cpu_delta_basis_points};
pub use cpu_topology::{
    CpuListParseError, CpuTopologyFacts, MAX_CPU_IDS, MAX_CPU_INDEX, MAX_CPU_LIST_BYTES,
    parse_cpu_list,
};
pub use cpufreq::{CpuFrequencyFacts, CpuFrequencyPolicyFacts, FrequencyRange, PlatformPowerFacts};
pub use display::{
    BackendDisplayOutput, BackendError, BackendErrorKind, BackendObservation, CompositorAssessment,
    ConnectorEnabled, ConnectorState, DisplayOutputSource, DisplayOutputState, DisplayProtocol,
    DisplayProtocolFacts, DrmConnector, ExactRefreshRate, LINUX_DISPLAY_REPORT_SCHEMA_VERSION,
    LinuxDisplayCompleteness, LinuxDisplayMode, LinuxDisplayOutput, LogicalGeometry,
    MAX_DISPLAY_MODES, MAX_DISPLAY_OUTPUTS, MAX_DRM_CONNECTORS, OutputInventoryStatus,
    ProviderStatus, ScaleFactor, SessionAssessment, SessionTypeHint, VerifiedDisplayProtocol,
};
pub use display_probe::{LinuxDisplayProbe, LinuxDisplayReport};
pub use display_sources::{
    LinuxDisplayDataSource, MAX_DRM_CONNECTOR_ATTRIBUTE_BYTES, RawDrmConnector,
};
pub use environment::{
    EnvironmentClassification, EnvironmentConfidence, EnvironmentStatus,
    RuntimeDirectoryAssessment, RuntimeDirectoryStatus, RuntimeEnvironmentFacts,
};
pub use errors::{DataSourceError, DataSourceErrorKind, OsReleaseParseError};
pub use frame_csv::{
    MAX_CSV_FIELDS, MAX_CSV_RECORD_BYTES, MAX_FRAME_EVENT_RECORDS, MAX_FRAME_INPUT_BYTES,
    MAX_FRAME_INTERVALS,
};
pub use frame_events::{FrameEvent, FrameEventsV1Source, FrameTimeline};
pub use frame_mangohud::{MangoHudCsvSource, MangoHudFrameSample, MangoHudRecording};
pub use frame_sources::{
    FrameDataSource, FrameInputError, FrameInputErrorKind, FrameMeasurementSemantics,
    FrameObservationGranularity, FrameRecording, FrameSourceFormat, FrameSourceMetadata,
    FrameSourceVersion, FrameTimestampUnit, parse_frame_recording, read_frame_recording,
};
pub use frame_statistics::{
    MIN_P99_FRAME_INTERVALS, arithmetic_mean_nanos, measured_frame_rate_milli,
    nearest_rank_p99_nanos,
};
pub use frame_telemetry::{FrameLogTelemetryProvider, FrameMetricStatus, FrameTelemetryResult};
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
pub use session::{LocalWaylandEndpoint, LocalX11Endpoint, SessionEnvironmentHints};
pub use sources::{
    ContainerMarker, LinuxDataSource, OsReleaseFile, RuntimeDirectoryMetadata,
    SystemLinuxDataSource,
};
pub use telemetry_clock::{
    ClockSessionError, LinuxTelemetryClockFactory, MonotonicClockSession, TelemetryClockFactory,
};
pub use telemetry_probe::{
    LinuxCpuMemoryTelemetryProvider, LinuxCpuSnapshotContext, LinuxTelemetryConfigError,
    MAX_LOGICAL_CPU_OBSERVATIONS, MAX_TELEMETRY_INTERVAL_MS, MIN_TELEMETRY_INTERVAL_MS,
    TelemetryInterval,
};
pub use vulkan::{
    SystemVulkanBackend, VulkanBackend, VulkanDeviceType, VulkanDriverIdentity, VulkanFacts,
    VulkanLoaderStatus, VulkanMetadataStatus, VulkanPhysicalDevice,
};
pub use wayland::{SystemWaylandBackend, WaylandDisplayBackend};
pub use x11::{SystemX11Backend, X11DisplayBackend};

pub mod gpu_nvidia;
pub mod gpu_telemetry;
pub mod gpu_telemetry_sources;
