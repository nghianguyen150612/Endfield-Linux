//! Narrow, bounded source contracts used by the P012 hardware adapter.
//!
//! The methods name fixed kernel source families rather than accepting paths.

use crate::errors::DataSourceError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CpuListSource {
    Online,
    Present,
    Possible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CpuTopologyField {
    PhysicalPackageId,
    CoreId,
    DieId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CpuFrequencyField {
    ScalingDriver,
    ScalingGovernor,
    AvailableGovernors,
    CpuInfoMinFrequencyKhz,
    CpuInfoMaxFrequencyKhz,
    CpuInfoCurrentFrequencyKhz,
    ScalingMinFrequencyKhz,
    ScalingMaxFrequencyKhz,
    RelatedCpus,
    AffectedCpus,
    EnergyPerformancePreference,
    AvailableEnergyPerformancePreferences,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlatformProfileField {
    Current,
    Available,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CgroupHierarchy {
    V1,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CgroupV2Control {
    CpuMax,
    CpusetCpusEffective,
    MemoryMax,
    MemoryCurrent,
    MemorySwapMax,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CgroupV2Files {
    pub cpu_max: Result<Option<Vec<u8>>, DataSourceError>,
    pub cpuset_cpus_effective: Result<Option<Vec<u8>>, DataSourceError>,
    pub memory_max: Result<Option<Vec<u8>>, DataSourceError>,
    pub memory_current: Result<Option<Vec<u8>>, DataSourceError>,
    pub memory_swap_max: Result<Option<Vec<u8>>, DataSourceError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CgroupSource {
    V2(CgroupV2Files),
    V1,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawSchedulerState {
    pub policy: i32,
    pub priority: i32,
}

/// P012 read-only source boundary. Implementations must cap every read and
/// must not expose arbitrary path access or write operations.
pub trait LinuxHardwareDataSource: Send + Sync {
    fn read_proc_cpuinfo(&self) -> Result<Vec<u8>, DataSourceError> {
        Err(DataSourceError::new(
            crate::errors::DataSourceErrorKind::Unsupported,
        ))
    }

    fn read_proc_meminfo(&self) -> Result<Vec<u8>, DataSourceError> {
        Err(DataSourceError::new(
            crate::errors::DataSourceErrorKind::Unsupported,
        ))
    }

    fn read_proc_stat(&self) -> Result<Vec<u8>, DataSourceError> {
        Err(DataSourceError::new(
            crate::errors::DataSourceErrorKind::Unsupported,
        ))
    }

    fn read_cpu_list(&self, _source: CpuListSource) -> Result<Vec<u8>, DataSourceError> {
        Err(DataSourceError::new(
            crate::errors::DataSourceErrorKind::Unsupported,
        ))
    }

    fn read_cpu_topology_field(
        &self,
        _cpu: u32,
        _field: CpuTopologyField,
    ) -> Result<Option<i64>, DataSourceError> {
        Err(DataSourceError::new(
            crate::errors::DataSourceErrorKind::Unsupported,
        ))
    }

    fn process_affinity(&self) -> Result<Vec<u32>, DataSourceError> {
        Err(DataSourceError::new(
            crate::errors::DataSourceErrorKind::Unsupported,
        ))
    }

    fn scheduler_state(&self) -> Result<RawSchedulerState, DataSourceError> {
        Err(DataSourceError::new(
            crate::errors::DataSourceErrorKind::Unsupported,
        ))
    }

    fn enumerate_cpufreq_policies(&self) -> Result<Vec<u32>, DataSourceError> {
        Err(DataSourceError::new(
            crate::errors::DataSourceErrorKind::Unsupported,
        ))
    }

    fn read_cpufreq_policy_field(
        &self,
        _policy: u32,
        _field: CpuFrequencyField,
    ) -> Result<Option<Vec<u8>>, DataSourceError> {
        Err(DataSourceError::new(
            crate::errors::DataSourceErrorKind::Unsupported,
        ))
    }

    fn read_platform_profile_field(
        &self,
        _field: PlatformProfileField,
    ) -> Result<Option<Vec<u8>>, DataSourceError> {
        Err(DataSourceError::new(
            crate::errors::DataSourceErrorKind::Unsupported,
        ))
    }

    fn cgroup_source(&self) -> Result<CgroupSource, DataSourceError> {
        Err(DataSourceError::new(
            crate::errors::DataSourceErrorKind::Unsupported,
        ))
    }
}
