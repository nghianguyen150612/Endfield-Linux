//! Read-only cgroup v2 resource-limit parsing.

use crate::cpu_topology::parse_cpu_list;
use crate::errors::{DataSourceError, DataSourceErrorKind};
use crate::hardware_common::HardwareIssue;
use crate::hardware_sources::{CgroupSource, CgroupV2Files, LinuxHardwareDataSource};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CgroupStatus {
    V2,
    V1Unsupported,
    NotDetected,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CgroupLimit<T> {
    Unknown,
    Unlimited,
    Limited(T),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuTimeQuota {
    /// Quota and period are retained as a rational CPU-time capacity.
    pub quota_microseconds: u64,
    pub period_microseconds: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CgroupResourceFacts {
    pub status: CgroupStatus,
    pub cpu_quota: CgroupLimit<CpuTimeQuota>,
    pub effective_cpu_set: Option<Vec<u32>>,
    pub memory_limit_bytes: CgroupLimit<u64>,
    pub memory_current_bytes: Option<u64>,
    pub memory_swap_limit_bytes: CgroupLimit<u64>,
    /// A leaf limit is not necessarily the tightest limit imposed by ancestors.
    pub ancestor_limits_assessed: bool,
}

impl Default for CgroupResourceFacts {
    fn default() -> Self {
        Self {
            status: CgroupStatus::Unknown,
            cpu_quota: CgroupLimit::Unknown,
            effective_cpu_set: None,
            memory_limit_bytes: CgroupLimit::Unknown,
            memory_current_bytes: None,
            memory_swap_limit_bytes: CgroupLimit::Unknown,
            ancestor_limits_assessed: false,
        }
    }
}

pub(crate) struct CgroupCollection {
    pub facts: CgroupResourceFacts,
    pub issues: Vec<HardwareIssue>,
    pub observed: bool,
}

pub(crate) fn collect_cgroup<S: LinuxHardwareDataSource>(source: &S) -> CgroupCollection {
    match source.cgroup_source() {
        Ok(CgroupSource::None) => CgroupCollection {
            facts: CgroupResourceFacts {
                status: CgroupStatus::NotDetected,
                ..CgroupResourceFacts::default()
            },
            issues: Vec::new(),
            observed: true,
        },
        Ok(CgroupSource::V1) => CgroupCollection {
            facts: CgroupResourceFacts {
                status: CgroupStatus::V1Unsupported,
                ..CgroupResourceFacts::default()
            },
            issues: vec![HardwareIssue::not_yet_probed(
                "linux-cgroup",
                "cgroup v1 limits are not interpreted by this probe",
            )],
            observed: true,
        },
        Ok(CgroupSource::V2(files)) => collect_v2(files),
        Err(error) => CgroupCollection {
            facts: CgroupResourceFacts::default(),
            issues: vec![HardwareIssue::from_source(
                "linux-cgroup",
                error.kind,
                "the current cgroup resource controls could not be resolved safely",
            )],
            observed: false,
        },
    }
}

fn collect_v2(files: CgroupV2Files) -> CgroupCollection {
    let mut issues = Vec::new();
    let cpu_quota = parse_source_field(
        files.cpu_max,
        "linux-cgroup",
        "the cgroup CPU quota could not be read",
        parse_cpu_max,
        &mut issues,
    )
    .unwrap_or(CgroupLimit::Unknown);
    let effective_cpu_set = parse_source_field(
        files.cpuset_cpus_effective,
        "linux-cgroup",
        "the effective cgroup CPU set could not be read",
        |bytes| {
            parse_cpu_list(bytes)
                .map_err(|_| DataSourceError::new(DataSourceErrorKind::MalformedData))
        },
        &mut issues,
    );
    let memory_limit_bytes = parse_source_field(
        files.memory_max,
        "linux-cgroup",
        "the cgroup memory limit could not be read",
        parse_memory_limit,
        &mut issues,
    )
    .unwrap_or(CgroupLimit::Unknown);
    let memory_current_bytes = parse_source_field(
        files.memory_current,
        "linux-cgroup",
        "the cgroup memory usage could not be read",
        parse_memory_current,
        &mut issues,
    );
    let memory_swap_limit_bytes = parse_source_field(
        files.memory_swap_max,
        "linux-cgroup",
        "the cgroup swap limit could not be read",
        parse_memory_swap_limit,
        &mut issues,
    )
    .unwrap_or(CgroupLimit::Unknown);
    let observed = true;
    let missing_controls =
        cpu_quota == CgroupLimit::Unknown || memory_limit_bytes == CgroupLimit::Unknown;
    CgroupCollection {
        facts: CgroupResourceFacts {
            status: CgroupStatus::V2,
            cpu_quota,
            effective_cpu_set,
            memory_limit_bytes,
            memory_current_bytes,
            memory_swap_limit_bytes,
            ancestor_limits_assessed: false,
        },
        issues: {
            if missing_controls {
                issues.push(HardwareIssue::not_yet_probed(
                    "linux-cgroup",
                    "not all cgroup v2 controls were available; ancestor limits were not assessed",
                ));
            }
            issues
        },
        observed,
    }
}

fn parse_source_field<T>(
    source: Result<Option<Vec<u8>>, DataSourceError>,
    provider: &'static str,
    message: &'static str,
    parser: impl FnOnce(&[u8]) -> Result<T, DataSourceError>,
    issues: &mut Vec<HardwareIssue>,
) -> Option<T> {
    match source {
        Ok(Some(bytes)) => match parser(&bytes) {
            Ok(value) => Some(value),
            Err(error) => {
                issues.push(HardwareIssue::from_source(provider, error.kind, message));
                None
            }
        },
        Ok(None) => None,
        Err(error) => {
            issues.push(HardwareIssue::from_source(provider, error.kind, message));
            None
        }
    }
}

fn parse_cpu_max(bytes: &[u8]) -> Result<CgroupLimit<CpuTimeQuota>, DataSourceError> {
    let text = bounded_text(bytes)?;
    let fields = text.split_ascii_whitespace().collect::<Vec<_>>();
    if fields.len() != 2 {
        return Err(DataSourceError::new(DataSourceErrorKind::MalformedData));
    }
    let period = parse_positive(fields[1])?;
    if fields[0] == "max" {
        return Ok(CgroupLimit::Unlimited);
    }
    let quota = parse_positive(fields[0])?;
    Ok(CgroupLimit::Limited(CpuTimeQuota {
        quota_microseconds: quota,
        period_microseconds: period,
    }))
}

fn parse_memory_limit(bytes: &[u8]) -> Result<CgroupLimit<u64>, DataSourceError> {
    let value = bounded_text(bytes)?;
    if value == "max" {
        return Ok(CgroupLimit::Unlimited);
    }
    Ok(CgroupLimit::Limited(parse_positive(value)?))
}

pub(crate) fn parse_memory_current(bytes: &[u8]) -> Result<u64, DataSourceError> {
    let value = bounded_text(bytes)?;
    parse_unsigned(value)
}

fn parse_memory_swap_limit(bytes: &[u8]) -> Result<CgroupLimit<u64>, DataSourceError> {
    let value = bounded_text(bytes)?;
    if value == "max" {
        return Ok(CgroupLimit::Unlimited);
    }
    Ok(CgroupLimit::Limited(parse_unsigned(value)?))
}

fn bounded_text(bytes: &[u8]) -> Result<&str, DataSourceError> {
    if bytes.len() > 4096 {
        return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
    }
    std::str::from_utf8(bytes)
        .map(str::trim)
        .map_err(|_| DataSourceError::new(DataSourceErrorKind::InvalidUtf8))
}

fn parse_positive(value: &str) -> Result<u64, DataSourceError> {
    let parsed = parse_unsigned(value)?;
    if parsed == 0 {
        return Err(DataSourceError::new(DataSourceErrorKind::MalformedData));
    }
    Ok(parsed)
}

fn parse_unsigned(value: &str) -> Result<u64, DataSourceError> {
    if value.is_empty()
        || value.starts_with('-')
        || value.starts_with('+')
        || value.bytes().any(|byte| !byte.is_ascii_digit())
    {
        return Err(DataSourceError::new(DataSourceErrorKind::MalformedData));
    }
    value
        .parse::<u64>()
        .map_err(|_| DataSourceError::new(DataSourceErrorKind::MalformedData))
}
