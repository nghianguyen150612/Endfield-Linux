//! Composite P012 probe: P011 identity plus independent read-only hardware facts.

use endfield_unix_domain::{
    Availability, Bytes, CapabilityId, CapabilityRecord, CapabilityValue, CpuInfo, CpuTopology,
    DiagnosticCategory, InstanceId, Measurement, MeasurementOrigin, MeasurementUnit,
    MemoryCapacity, MemoryInfo, ProbeError, ProbeErrorKind, Provenance, ProvenanceId, SwapInfo,
    SystemSnapshot,
};

use crate::cgroup::{CgroupCollection, CgroupResourceFacts, CgroupStatus, collect_cgroup};
use crate::cpu::{CpuInfoParseError, parse_cpu_model};
use crate::cpu_topology::{CpuTopologyCollection, CpuTopologyFacts, collect_cpu_topology};
use crate::cpufreq::{
    CpuFrequencyCollection, CpuFrequencyFacts, PlatformPowerCollection, PlatformPowerFacts,
    collect_cpufreq, collect_platform_power,
};
use crate::errors::DataSourceErrorKind;
use crate::hardware_common::HardwareIssue;
use crate::hardware_sources::LinuxHardwareDataSource;
use crate::memory::{MemInfoData, MemInfoFieldError, MemInfoParseError, parse_meminfo};
use crate::probe::{
    LINUX_PLATFORM_REPORT_SCHEMA_VERSION, LinuxPlatformProbe, LinuxReportCompleteness,
};
use crate::scheduler::{SchedulingCollection, SchedulingFacts, collect_scheduling};
use crate::sources::{LinuxDataSource, SystemLinuxDataSource};

pub const LINUX_HARDWARE_REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxHardwareCompleteness {
    Complete,
    Partial,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinuxMemoryFacts {
    /// `/proc/meminfo` host-visible values normalized from 1024-byte kB units.
    pub host_total_bytes: Option<u64>,
    pub host_available_bytes: Option<u64>,
    pub host_swap_total_bytes: Option<u64>,
    pub host_swap_free_bytes: Option<u64>,
}

/// P012's Linux-specific details are kept beside the stable P007 snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxHardwareReport {
    pub schema_version: u32,
    /// P011 identity and P012 normalized CPU/memory facts, validated together.
    pub snapshot: SystemSnapshot,
    pub distribution: Option<crate::LinuxDistributionMetadata>,
    pub runtime_environment: crate::RuntimeEnvironmentFacts,
    pub user_runtime_directory: crate::RuntimeDirectoryAssessment,
    pub cpu_model: Option<String>,
    pub cpu_topology: Option<CpuTopologyFacts>,
    pub scheduling: SchedulingFacts,
    pub memory: LinuxMemoryFacts,
    pub cgroup: CgroupResourceFacts,
    pub cpu_frequency: CpuFrequencyFacts,
    pub platform_power: PlatformPowerFacts,
    pub completeness: LinuxHardwareCompleteness,
}

pub struct LinuxHardwareProbe<S = SystemLinuxDataSource> {
    source: S,
}

impl LinuxHardwareProbe<SystemLinuxDataSource> {
    pub fn system() -> Self {
        Self {
            source: SystemLinuxDataSource::new(),
        }
    }
}

impl<S> LinuxHardwareProbe<S> {
    pub fn with_source(source: S) -> Self {
        Self { source }
    }

    pub fn source(&self) -> &S {
        &self.source
    }
}

impl<S> LinuxHardwareProbe<S>
where
    S: LinuxDataSource + LinuxHardwareDataSource + Clone,
{
    /// Collects each hardware subsystem independently. Only P011's validated
    /// running-kernel identity is required for a normalized combined snapshot.
    pub fn inspect(&self) -> Result<LinuxHardwareReport, ProbeError> {
        let platform = LinuxPlatformProbe::with_source(self.source.clone()).inspect()?;
        if platform.schema_version != LINUX_PLATFORM_REPORT_SCHEMA_VERSION {
            return Err(probe_error(
                "linux-platform",
                ProbeErrorKind::MalformedData,
                "the P011 platform report schema was unsupported",
            ));
        }

        let mut cpu_topology = collect_cpu_topology(&self.source);
        let scheduling = collect_scheduling(&self.source);
        if let Some(cpu) = &mut cpu_topology.facts {
            cpu.process_allowed_cpu_ids = scheduling.facts.allowed_cpu_ids.clone();
        }
        let cpu_model = collect_cpu_model(&self.source);
        let memory = collect_memory(&self.source);
        let cgroup = collect_cgroup(&self.source);
        let cpufreq = collect_cpufreq(&self.source);
        let power = collect_platform_power(&self.source);

        let mut snapshot = platform.snapshot.clone();
        let observed_at = self.source.observation_time_unix_nanos();
        let provenance = add_hardware_provenance(
            &mut snapshot,
            HardwareProvenanceInput {
                cpu: &cpu_topology,
                cpu_model: &cpu_model,
                memory: &memory,
                cgroup: &cgroup,
                cpufreq: &cpufreq,
                scheduling: &scheduling,
                power: &power,
                observed_at,
            },
        )?;

        let mut issues = Vec::new();
        collect_cpu_issues(&cpu_topology, &mut issues);
        issues.extend(cpu_model.issues.iter().copied());
        issues.extend(memory.issues.iter().copied());
        issues.extend(cgroup.issues.iter().copied());
        issues.extend(cpufreq.issues.iter().copied());
        issues.extend(scheduling.issues.iter().copied());
        issues.extend(power.issues.iter().copied());
        for issue in issues {
            snapshot.diagnostics.push(issue.to_domain()?);
        }

        let cpu_id = InstanceId::new(1).map_err(|_| {
            probe_error(
                "linux-hardware",
                ProbeErrorKind::MalformedData,
                "the CPU snapshot instance could not be represented",
            )
        })?;
        if cpu_topology.facts.is_some()
            || cpu_model.value.is_some()
            || scheduling.observed
            || cpufreq.observed
        {
            let topology = cpu_topology.facts.as_ref().and_then(|facts| {
                u32::try_from(facts.online_cpu_ids.len())
                    .ok()
                    .map(|logical| CpuTopology {
                        logical_processor_count: logical,
                        physical_core_count: facts.physical_core_count,
                    })
            });
            snapshot.cpu = Some(CpuInfo {
                instance: cpu_id,
                model_identifier: cpu_model.value.clone(),
                topology: CapabilityValue::new(
                    if topology.is_some() {
                        Availability::Available
                    } else {
                        Availability::Unknown
                    },
                    topology,
                ),
                // P012 is an inventory probe. No instantaneous frequency
                // sample is taken; future telemetry will own that value.
                observed_frequency: CapabilityValue::new(Availability::Unknown, None),
                scheduling: scheduling.capability_availability,
                // The detailed Linux report records control interfaces and
                // explicitly leaves permission to change them unknown.
                power_management: Availability::Unknown,
            });
            add_cpu_capabilities(
                &mut snapshot,
                cpu_id,
                &cpu_topology,
                &scheduling,
                &cpufreq,
                &power,
                &provenance,
            )?;
        }
        snapshot.memory = Some(memory.info.clone());
        add_memory_capabilities(&mut snapshot, &memory, &provenance)?;

        snapshot = snapshot.normalize().map_err(|_| {
            probe_error(
                "linux-hardware",
                ProbeErrorKind::MalformedData,
                "the combined P011/P012 snapshot failed validation",
            )
        })?;

        let incomplete = platform.completeness == LinuxReportCompleteness::Partial
            || !snapshot.diagnostics.is_empty()
            || cpu_topology.facts.as_ref().is_none_or(|facts| {
                facts.physical_core_count.is_none()
                    || facts.present_cpu_ids.is_none()
                    || facts.possible_cpu_ids.is_none()
            })
            || memory.facts.host_total_bytes.is_none()
            || memory.facts.host_swap_total_bytes.is_none()
            || cgroup.facts.status == CgroupStatus::V1Unsupported
            || cgroup.facts.status == CgroupStatus::Unknown;

        Ok(LinuxHardwareReport {
            schema_version: LINUX_HARDWARE_REPORT_SCHEMA_VERSION,
            snapshot,
            distribution: platform.distribution,
            runtime_environment: platform.runtime_environment,
            user_runtime_directory: platform.user_runtime_directory,
            cpu_model: cpu_model.value,
            cpu_topology: cpu_topology.facts,
            scheduling: scheduling.facts,
            memory: memory.facts,
            cgroup: cgroup.facts,
            cpu_frequency: cpufreq.facts,
            platform_power: power.facts,
            completeness: if incomplete {
                LinuxHardwareCompleteness::Partial
            } else {
                LinuxHardwareCompleteness::Complete
            },
        })
    }
}

struct CpuModelCollection {
    value: Option<String>,
    issues: Vec<HardwareIssue>,
    observed: bool,
}

fn collect_cpu_model<S: LinuxHardwareDataSource>(source: &S) -> CpuModelCollection {
    match source.read_proc_cpuinfo() {
        Ok(bytes) => match parse_cpu_model(&bytes) {
            Ok(value) => CpuModelCollection {
                value,
                issues: Vec::new(),
                observed: true,
            },
            Err(error) => CpuModelCollection {
                value: None,
                issues: vec![HardwareIssue::malformed(
                    "linux-cpuinfo",
                    match error {
                        CpuInfoParseError::TooLarge => "CPU model source exceeded its size limit",
                        CpuInfoParseError::InvalidUtf8 => "CPU model source was not valid UTF-8",
                        CpuInfoParseError::LineTooLong => {
                            "CPU model source contained an oversized line"
                        }
                    },
                )],
                observed: false,
            },
        },
        Err(error) => CpuModelCollection {
            value: None,
            issues: vec![HardwareIssue::from_source(
                "linux-cpuinfo",
                error.kind,
                "CPU model information could not be read",
            )],
            observed: false,
        },
    }
}

struct MemoryCollection {
    info: MemoryInfo,
    facts: LinuxMemoryFacts,
    issues: Vec<HardwareIssue>,
    observed: bool,
    ram_degraded: bool,
}

fn collect_memory<S: LinuxHardwareDataSource>(source: &S) -> MemoryCollection {
    let bytes = match source.read_proc_meminfo() {
        Ok(bytes) => bytes,
        Err(error) => {
            return MemoryCollection {
                info: unknown_memory(),
                facts: LinuxMemoryFacts::default(),
                issues: vec![HardwareIssue::from_source(
                    "linux-meminfo",
                    error.kind,
                    "Linux memory information could not be read",
                )],
                observed: false,
                ram_degraded: false,
            };
        }
    };
    let parsed = match parse_meminfo(&bytes) {
        Ok(parsed) => parsed,
        Err(error) => {
            return MemoryCollection {
                info: unknown_memory(),
                facts: LinuxMemoryFacts::default(),
                issues: vec![HardwareIssue::malformed(
                    "linux-meminfo",
                    match error {
                        MemInfoParseError::TooLarge => "meminfo source exceeded its size limit",
                        MemInfoParseError::InvalidUtf8 => "meminfo source was not valid UTF-8",
                        MemInfoParseError::LineTooLong => {
                            "meminfo source contained an oversized line"
                        }
                    },
                )],
                observed: true,
                ram_degraded: false,
            };
        }
    };
    memory_from_parsed(parsed)
}

fn memory_from_parsed(parsed: MemInfoData) -> MemoryCollection {
    let mut issues = Vec::new();
    let total =
        field_value(parsed.mem_total_bytes, "MemTotal", true, &mut issues).filter(|value| {
            if *value == 0 {
                issues.push(HardwareIssue::malformed(
                    "linux-meminfo",
                    "MemTotal was zero",
                ));
                false
            } else {
                true
            }
        });
    let available = field_value(
        parsed.mem_available_bytes,
        "MemAvailable",
        false,
        &mut issues,
    );
    let swap_total = field_value(parsed.swap_total_bytes, "SwapTotal", false, &mut issues);
    let mut swap_free = field_value(parsed.swap_free_bytes, "SwapFree", false, &mut issues);
    let mut ram_degraded = false;
    let valid_available = match (total, available) {
        (Some(total), Some(available)) if available <= total => Some(available),
        (Some(_), Some(_)) => {
            issues.push(HardwareIssue::malformed(
                "linux-meminfo",
                "MemAvailable exceeded MemTotal",
            ));
            ram_degraded = true;
            None
        }
        (None, Some(_)) => None,
        (_, None) => None,
    };
    if let (Some(total), Some(free)) = (swap_total, swap_free) {
        if free > total {
            issues.push(HardwareIssue::malformed(
                "linux-meminfo",
                "SwapFree exceeded SwapTotal",
            ));
            swap_free = None;
        }
    }
    let ram_availability = if total.is_some() {
        if ram_degraded {
            Availability::Degraded
        } else {
            Availability::Available
        }
    } else {
        Availability::Unknown
    };
    let ram = total.map(|total| MemoryCapacity {
        total: Some(Bytes(total)),
        available: valid_available.map(Bytes),
    });
    let swap = match swap_total {
        Some(0) => SwapInfo {
            availability: Availability::Available,
            present: Some(false),
            total: None,
        },
        Some(total) => SwapInfo {
            availability: Availability::Available,
            present: Some(true),
            total: Some(Bytes(total)),
        },
        None => SwapInfo {
            availability: Availability::Unknown,
            present: None,
            total: None,
        },
    };
    MemoryCollection {
        info: MemoryInfo {
            ram: CapabilityValue::new(ram_availability, ram),
            swap,
        },
        facts: LinuxMemoryFacts {
            host_total_bytes: total,
            host_available_bytes: valid_available,
            host_swap_total_bytes: swap_total,
            host_swap_free_bytes: swap_free,
        },
        issues,
        observed: true,
        ram_degraded,
    }
}

fn field_value(
    field: Option<Result<u64, MemInfoFieldError>>,
    name: &'static str,
    required: bool,
    issues: &mut Vec<HardwareIssue>,
) -> Option<u64> {
    match field {
        Some(Ok(value)) => Some(value),
        Some(Err(_)) => {
            issues.push(HardwareIssue::malformed(
                "linux-meminfo",
                match name {
                    "MemTotal" => "MemTotal was malformed or duplicated",
                    "MemAvailable" => "MemAvailable was malformed or duplicated",
                    "SwapTotal" => "SwapTotal was malformed or duplicated",
                    _ => "SwapFree was malformed or duplicated",
                },
            ));
            None
        }
        None if required => {
            issues.push(HardwareIssue::from_source(
                "linux-meminfo",
                DataSourceErrorKind::MalformedData,
                "MemTotal was missing",
            ));
            None
        }
        None if name == "SwapTotal" => {
            issues.push(HardwareIssue::not_yet_probed(
                "linux-meminfo",
                "SwapTotal was absent; swap status remains unknown",
            ));
            None
        }
        None => None,
    }
}

fn unknown_memory() -> MemoryInfo {
    MemoryInfo {
        ram: CapabilityValue::new(Availability::Unknown, None),
        swap: SwapInfo {
            availability: Availability::Unknown,
            present: None,
            total: None,
        },
    }
}

fn collect_cpu_issues(collection: &CpuTopologyCollection, issues: &mut Vec<HardwareIssue>) {
    if let Some(error) = collection.online_error {
        issues.push(HardwareIssue::from_source(
            "linux-cpu-sysfs",
            error.kind,
            "the online CPU list could not be read",
        ));
    }
    if let Some(error) = collection.present_error {
        issues.push(HardwareIssue::from_source(
            "linux-cpu-sysfs",
            error.kind,
            "the present CPU list could not be read",
        ));
    }
    if let Some(error) = collection.possible_error {
        issues.push(HardwareIssue::from_source(
            "linux-cpu-sysfs",
            error.kind,
            "the possible CPU list could not be read",
        ));
    }
    if collection.topology_incomplete {
        issues.push(HardwareIssue::not_yet_probed(
            "linux-cpu-sysfs",
            "physical core identities could not be verified for every online CPU",
        ));
    }
    if collection.inconsistent_cpu_sets {
        issues.push(HardwareIssue::malformed(
            "linux-cpu-sysfs",
            "online CPUs were inconsistent with present or possible CPU sets",
        ));
    }
}

struct HardwareProvenanceInput<'a> {
    cpu: &'a CpuTopologyCollection,
    cpu_model: &'a CpuModelCollection,
    memory: &'a MemoryCollection,
    cgroup: &'a CgroupCollection,
    cpufreq: &'a CpuFrequencyCollection,
    scheduling: &'a SchedulingCollection,
    power: &'a PlatformPowerCollection,
    observed_at: Option<u64>,
}

fn add_hardware_provenance(
    snapshot: &mut SystemSnapshot,
    input: HardwareProvenanceInput<'_>,
) -> Result<HardwareProvenance, ProbeError> {
    let mut provenance = HardwareProvenance::default();
    let mut add = |id, name, observed: bool| -> Result<Option<ProvenanceId>, ProbeError> {
        if !observed {
            return Ok(None);
        }
        let id = ProvenanceId::new(id).map_err(|_| {
            probe_error(
                name,
                ProbeErrorKind::MalformedData,
                "Linux hardware provenance ID was invalid",
            )
        })?;
        snapshot
            .provenance
            .push(Provenance::new(id, name, input.observed_at).map_err(|_| {
                probe_error(
                    name,
                    ProbeErrorKind::MalformedData,
                    "Linux hardware provenance was invalid",
                )
            })?);
        Ok(Some(id))
    };
    provenance.cpu_sysfs = add(3, "linux-cpu-sysfs", input.cpu.facts.is_some())?;
    provenance.cpuinfo = add(4, "linux-cpuinfo", input.cpu_model.observed)?;
    provenance.meminfo = add(5, "linux-meminfo", input.memory.observed)?;
    provenance.cgroup = add(6, "linux-cgroup", input.cgroup.observed)?;
    provenance.cpufreq = add(7, "linux-cpufreq", input.cpufreq.observed)?;
    provenance.scheduler = add(8, "linux-scheduler", input.scheduling.observed)?;
    provenance.power = add(9, "linux-power", input.power.observed)?;
    Ok(provenance)
}

#[derive(Default)]
struct HardwareProvenance {
    cpu_sysfs: Option<ProvenanceId>,
    cpuinfo: Option<ProvenanceId>,
    meminfo: Option<ProvenanceId>,
    cgroup: Option<ProvenanceId>,
    cpufreq: Option<ProvenanceId>,
    scheduler: Option<ProvenanceId>,
    power: Option<ProvenanceId>,
}

fn add_cpu_capabilities(
    snapshot: &mut SystemSnapshot,
    cpu_id: InstanceId,
    topology: &CpuTopologyCollection,
    scheduling: &SchedulingCollection,
    cpufreq: &CpuFrequencyCollection,
    power: &PlatformPowerCollection,
    provenance: &HardwareProvenance,
) -> Result<(), ProbeError> {
    if let (Some(facts), Some(source_id)) = (&topology.facts, provenance.cpu_sysfs) {
        let count = u32::try_from(facts.online_cpu_ids.len()).map_err(|_| {
            probe_error(
                "linux-cpu-sysfs",
                ProbeErrorKind::MalformedData,
                "online CPU count exceeded the domain range",
            )
        })?;
        let mut record = CapabilityRecord::new(CapabilityId::CpuTopology, Availability::Available);
        record.instance = Some(cpu_id);
        record.provenance = Some(source_id);
        record.measurement = Some(Measurement::unsigned(
            u64::from(count),
            MeasurementUnit::Count,
            MeasurementOrigin::Normalized,
        ));
        snapshot.capabilities.push(record);
    }
    if scheduling.capability_availability != Availability::Unknown {
        let mut record = CapabilityRecord::new(
            CapabilityId::CpuScheduling,
            scheduling.capability_availability,
        );
        record.instance = Some(cpu_id);
        record.provenance = provenance.scheduler;
        if scheduling.capability_availability == Availability::Degraded {
            record.diagnostic = scheduling
                .issues
                .first()
                .copied()
                .map(HardwareIssue::to_domain)
                .transpose()?;
        }
        snapshot.capabilities.push(record);
    }
    let frequency_availability =
        combine_interface_availability(cpufreq.facts.availability, power.facts.availability);
    if frequency_availability != Availability::Unknown {
        let (source_id, issue) = if cpufreq.facts.availability == Availability::Degraded {
            (provenance.cpufreq, cpufreq.issues.first().copied())
        } else if power.facts.availability == Availability::Degraded {
            (provenance.power, power.issues.first().copied())
        } else if cpufreq.facts.availability == Availability::Available {
            (provenance.cpufreq, None)
        } else if power.facts.availability == Availability::Available {
            (provenance.power, None)
        } else if cpufreq.facts.availability != Availability::Unknown {
            (provenance.cpufreq, None)
        } else {
            (provenance.power, None)
        };
        let mut record =
            CapabilityRecord::new(CapabilityId::CpuFrequencyPower, frequency_availability);
        record.provenance = source_id;
        if frequency_availability == Availability::Degraded {
            let provider = if cpufreq.facts.availability == Availability::Degraded {
                "linux-cpufreq"
            } else {
                "linux-power"
            };
            let diagnostic = compatible_degraded_issue(
                issue,
                provider,
                "some CPU frequency or power interface details could not be validated",
            );
            record.diagnostic = Some(diagnostic.to_domain()?);
        }
        snapshot.capabilities.push(record);
    }
    Ok(())
}

fn combine_interface_availability(left: Availability, right: Availability) -> Availability {
    if left == Availability::Available || right == Availability::Available {
        Availability::Available
    } else if left == Availability::Degraded || right == Availability::Degraded {
        Availability::Degraded
    } else if left == Availability::Unavailable && right == Availability::Unavailable {
        Availability::Unavailable
    } else {
        Availability::Unknown
    }
}

fn compatible_degraded_issue(
    issue: Option<HardwareIssue>,
    provider: &'static str,
    message: &'static str,
) -> HardwareIssue {
    match issue {
        Some(issue)
            if matches!(
                issue.category,
                DiagnosticCategory::PermissionDenied
                    | DiagnosticCategory::TemporaryFailure
                    | DiagnosticCategory::MalformedSourceData
                    | DiagnosticCategory::StaleObservation
            ) =>
        {
            issue
        }
        _ => HardwareIssue {
            provider,
            category: DiagnosticCategory::TemporaryFailure,
            message,
        },
    }
}

fn add_memory_capabilities(
    snapshot: &mut SystemSnapshot,
    memory: &MemoryCollection,
    provenance: &HardwareProvenance,
) -> Result<(), ProbeError> {
    if let (Some(total), Some(source_id)) = (memory.facts.host_total_bytes, provenance.meminfo) {
        let mut record = CapabilityRecord::new(CapabilityId::Ram, memory.info.ram.availability);
        record.provenance = Some(source_id);
        record.measurement = Some(Measurement::unsigned(
            total,
            MeasurementUnit::Bytes,
            MeasurementOrigin::Normalized,
        ));
        if memory.ram_degraded {
            record.diagnostic = memory
                .issues
                .iter()
                .find(|issue| issue.message == "MemAvailable exceeded MemTotal")
                .copied()
                .map(HardwareIssue::to_domain)
                .transpose()?;
        }
        snapshot.capabilities.push(record);
    }
    if memory.info.swap.availability != Availability::Unknown {
        let mut record = CapabilityRecord::new(CapabilityId::Swap, memory.info.swap.availability);
        record.provenance = provenance.meminfo;
        record.measurement = memory.info.swap.total.map(|bytes| {
            Measurement::unsigned(
                bytes.0,
                MeasurementUnit::Bytes,
                MeasurementOrigin::Normalized,
            )
        });
        snapshot.capabilities.push(record);
    }
    Ok(())
}

fn probe_error(provider: &str, kind: ProbeErrorKind, message: &str) -> ProbeError {
    ProbeError::new(provider, kind, Some(message.to_owned()))
        .unwrap_or_else(|_| ProbeError::new("linux-hardware", kind, None).expect("static provider"))
}
