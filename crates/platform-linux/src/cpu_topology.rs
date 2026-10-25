//! Bounded Linux CPU-list parsing and verified topology collection.

use std::collections::BTreeSet;

use crate::errors::{DataSourceError, DataSourceErrorKind};
use crate::hardware_sources::{CpuListSource, CpuTopologyField, LinuxHardwareDataSource};

pub const MAX_CPU_LIST_BYTES: usize = 16 * 1024;
pub const MAX_CPU_IDS: usize = 1_000_000;
pub const MAX_CPU_INDEX: u32 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuListParseError {
    TooLarge,
    InvalidUtf8,
    Empty,
    InvalidToken,
    InvalidRange,
    DuplicateCpu,
    TooManyCpus,
    CpuIndexTooLarge,
}

/// Parses the Linux cpulist grammar used by sysfs and cpuset interfaces.
pub fn parse_cpu_list(bytes: &[u8]) -> Result<Vec<u32>, CpuListParseError> {
    if bytes.len() > MAX_CPU_LIST_BYTES {
        return Err(CpuListParseError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| CpuListParseError::InvalidUtf8)?;
    let text = text.trim_matches([' ', '\t', '\r', '\n']);
    if text.is_empty() {
        return Err(CpuListParseError::Empty);
    }
    let mut cpus = BTreeSet::new();
    for token in text.split(',') {
        if token.is_empty() || token.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(CpuListParseError::InvalidToken);
        }
        let (first, last) = match token.split_once('-') {
            Some((first, last)) if !last.is_empty() && !last.contains('-') => {
                let first = first
                    .parse::<u32>()
                    .map_err(|_| CpuListParseError::InvalidToken)?;
                let last = last
                    .parse::<u32>()
                    .map_err(|_| CpuListParseError::InvalidToken)?;
                if first > last {
                    return Err(CpuListParseError::InvalidRange);
                }
                (first, last)
            }
            Some(_) => return Err(CpuListParseError::InvalidToken),
            None => {
                let cpu = token
                    .parse::<u32>()
                    .map_err(|_| CpuListParseError::InvalidToken)?;
                (cpu, cpu)
            }
        };
        if last > MAX_CPU_INDEX {
            return Err(CpuListParseError::CpuIndexTooLarge);
        }
        let range_count =
            usize::try_from(last - first + 1).map_err(|_| CpuListParseError::TooManyCpus)?;
        if cpus.len().saturating_add(range_count) > MAX_CPU_IDS {
            return Err(CpuListParseError::TooManyCpus);
        }
        for cpu in first..=last {
            if !cpus.insert(cpu) {
                return Err(CpuListParseError::DuplicateCpu);
            }
        }
    }
    Ok(cpus.into_iter().collect())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpuTopologyFacts {
    pub possible_cpu_ids: Option<Vec<u32>>,
    pub present_cpu_ids: Option<Vec<u32>>,
    pub online_cpu_ids: Vec<u32>,
    pub offline_cpu_ids: Option<Vec<u32>>,
    pub process_allowed_cpu_ids: Option<Vec<u32>>,
    pub physical_core_count: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpuTopologyCollection {
    pub facts: Option<CpuTopologyFacts>,
    pub online_error: Option<DataSourceError>,
    pub present_error: Option<DataSourceError>,
    pub possible_error: Option<DataSourceError>,
    pub topology_incomplete: bool,
    pub inconsistent_cpu_sets: bool,
}

pub(crate) fn collect_cpu_topology<S: LinuxHardwareDataSource>(
    source: &S,
) -> CpuTopologyCollection {
    let online = read_cpu_list(source, CpuListSource::Online);
    let present = read_cpu_list(source, CpuListSource::Present);
    let possible = read_cpu_list(source, CpuListSource::Possible);
    let (online_cpu_ids, online_error) = match online {
        Ok(cpus) => (Some(cpus), None),
        Err(error) => (None, Some(error)),
    };
    let (present_cpu_ids, present_error) = match present {
        Ok(cpus) => (Some(cpus), None),
        Err(error) => (None, Some(error)),
    };
    let (possible_cpu_ids, possible_error) = match possible {
        Ok(cpus) => (Some(cpus), None),
        Err(error) => (None, Some(error)),
    };
    let process_allowed_cpu_ids = None;

    let Some(online_cpu_ids) = online_cpu_ids else {
        return CpuTopologyCollection {
            facts: None,
            online_error,
            present_error,
            possible_error,
            topology_incomplete: true,
            inconsistent_cpu_sets: false,
        };
    };

    let mut inconsistent_cpu_sets = false;
    if let Some(present) = &present_cpu_ids {
        inconsistent_cpu_sets |= !online_cpu_ids
            .iter()
            .all(|cpu| present.binary_search(cpu).is_ok());
    }
    if let Some(possible) = &possible_cpu_ids {
        inconsistent_cpu_sets |= !online_cpu_ids
            .iter()
            .all(|cpu| possible.binary_search(cpu).is_ok());
    }
    let offline_cpu_ids = present_cpu_ids.as_ref().map(|present| {
        present
            .iter()
            .copied()
            .filter(|cpu| online_cpu_ids.binary_search(cpu).is_err())
            .collect()
    });

    let (physical_core_count, topology_incomplete) = count_physical_cores(source, &online_cpu_ids);
    CpuTopologyCollection {
        facts: Some(CpuTopologyFacts {
            possible_cpu_ids,
            present_cpu_ids,
            online_cpu_ids,
            offline_cpu_ids,
            process_allowed_cpu_ids,
            physical_core_count,
        }),
        online_error,
        present_error,
        possible_error,
        topology_incomplete,
        inconsistent_cpu_sets,
    }
}

fn read_cpu_list<S: LinuxHardwareDataSource>(
    source: &S,
    list: CpuListSource,
) -> Result<Vec<u32>, DataSourceError> {
    let bytes = source.read_cpu_list(list)?;
    parse_cpu_list(&bytes).map_err(|_| DataSourceError::new(DataSourceErrorKind::MalformedData))
}

fn count_physical_cores<S: LinuxHardwareDataSource>(
    source: &S,
    online: &[u32],
) -> (Option<u32>, bool) {
    let mut package_core_pairs = BTreeSet::new();
    let mut package_die_core_triples = BTreeSet::new();
    let mut die_presence = None;
    for cpu in online {
        let package =
            match source.read_cpu_topology_field(*cpu, CpuTopologyField::PhysicalPackageId) {
                Ok(Some(value)) if value >= 0 => value,
                _ => return (None, true),
            };
        let core = match source.read_cpu_topology_field(*cpu, CpuTopologyField::CoreId) {
            Ok(Some(value)) if value >= 0 => value,
            _ => return (None, true),
        };
        package_core_pairs.insert((package, core));
        match source.read_cpu_topology_field(*cpu, CpuTopologyField::DieId) {
            Ok(Some(value)) if value >= 0 => {
                if die_presence == Some(false) {
                    return (None, true);
                }
                die_presence = Some(true);
                package_die_core_triples.insert((package, value, core));
            }
            Ok(None) => {
                if die_presence == Some(true) {
                    return (None, true);
                }
                die_presence = Some(false);
            }
            _ => return (None, true),
        }
    }
    let count = if die_presence == Some(true) {
        package_die_core_triples.len()
    } else {
        package_core_pairs.len()
    };
    let Ok(count) = u32::try_from(count) else {
        return (None, true);
    };
    if count == 0 || count as usize > online.len() {
        (None, true)
    } else {
        (Some(count), false)
    }
}
