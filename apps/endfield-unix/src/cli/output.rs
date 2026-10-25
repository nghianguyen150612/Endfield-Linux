use std::io::{self, Write};

use endfield_unix_platform_linux::{
    CgroupLimit, CgroupStatus, CpuTimeQuota, EnvironmentClassification, LinuxHardwareCompleteness,
    LinuxHardwareReport, LinuxPlatformReport, LinuxReportCompleteness, QueryAvailability,
    RuntimeDirectoryStatus,
};

use super::commands::HelpTopic;
use crate::{
    APPLICATION_LICENSE, APPLICATION_NAME, APPLICATION_PURPOSE, APPLICATION_REPOSITORY,
    APPLICATION_STAGE, APPLICATION_VERSION,
};

/// Writes the general help text, which lists only implemented commands.
pub fn write_help<W: Write>(topic: Option<HelpTopic>, output: &mut W) -> io::Result<()> {
    match topic {
        None => write_general_help(output),
        Some(HelpTopic::Gui) => write_gui_help(output),
        Some(HelpTopic::About) => write_about_help(output),
        Some(HelpTopic::Service) => write_service_help(output),
        Some(HelpTopic::Diagnose) => write_diagnose_help(output),
    }
}

pub fn write_version<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(output, "{APPLICATION_NAME} {APPLICATION_VERSION}")
}

pub fn write_about<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(output, "Application: {APPLICATION_NAME}")?;
    writeln!(output, "Version: {APPLICATION_VERSION}")?;
    writeln!(output, "Purpose: {APPLICATION_PURPOSE}")?;
    writeln!(output, "Development stage: {APPLICATION_STAGE}")?;
    writeln!(output, "License: {APPLICATION_LICENSE}")?;
    writeln!(output, "Repository: {APPLICATION_REPOSITORY}")
}

pub fn write_platform_report<W: Write>(
    report: &LinuxPlatformReport,
    output: &mut W,
) -> io::Result<()> {
    writeln!(output, "Platform: Linux")?;
    if let Some(distribution) = &report.distribution {
        let name = distribution
            .pretty_name
            .as_deref()
            .or(distribution.name.as_deref())
            .unwrap_or(&distribution.id);
        let version = distribution
            .version_id
            .as_deref()
            .or(distribution.version.as_deref());
        match version {
            Some(version) => writeln!(
                output,
                "Distribution: {name} (id: {}; version: {version})",
                distribution.id
            )?,
            None => writeln!(output, "Distribution: {name} (id: {})", distribution.id)?,
        }
    } else {
        writeln!(
            output,
            "Distribution: unknown (metadata unavailable or invalid)"
        )?;
    }

    let kernel = report
        .snapshot
        .platform
        .kernel
        .as_ref()
        .and_then(|kernel| kernel.version.as_deref())
        .unwrap_or("unknown");
    writeln!(output, "Kernel: Linux {kernel}")?;
    writeln!(
        output,
        "Architecture: {} (kernel-reported)",
        report.snapshot.platform.architecture.as_str()
    )?;

    let (environment_name, confidence) = match report.runtime_environment.classification {
        EnvironmentClassification::NativeLinux => (
            "native Linux process",
            confidence_name(report.runtime_environment.confidence),
        ),
        EnvironmentClassification::Container => (
            "container",
            confidence_name(report.runtime_environment.confidence),
        ),
        EnvironmentClassification::Wsl => (
            "WSL",
            confidence_name(report.runtime_environment.confidence),
        ),
        EnvironmentClassification::Unknown => (
            "unknown",
            confidence_name(report.runtime_environment.confidence),
        ),
    };
    write!(
        output,
        "Environment: {environment_name} ({confidence} confidence"
    )?;
    if !report.runtime_environment.source_ids.is_empty() {
        write!(
            output,
            "; sources: {}",
            report.runtime_environment.source_ids.join(", ")
        )?;
    }
    writeln!(output, ")")?;
    if let Some(uncertainty) = report.runtime_environment.uncertainty {
        writeln!(output, "Environment uncertainty: {uncertainty}")?;
    }
    if let Some(diagnostic) = report.runtime_environment.diagnostic {
        writeln!(output, "Environment note: {diagnostic}")?;
    }

    let runtime_directory = match report.user_runtime_directory.status {
        RuntimeDirectoryStatus::Available => "available and valid",
        RuntimeDirectoryStatus::Missing => "missing",
        RuntimeDirectoryStatus::InvalidOrUnsafe => "unsafe",
        RuntimeDirectoryStatus::PermissionDenied => "permission denied",
        RuntimeDirectoryStatus::Unknown => "unknown",
    };
    writeln!(
        output,
        "User runtime directory: {runtime_directory} ({})",
        report.user_runtime_directory.source_id
    )?;
    if let Some(diagnostic) = report.user_runtime_directory.diagnostic {
        writeln!(output, "Runtime-directory note: {diagnostic}")?;
    }
    let completeness = match report.completeness {
        LinuxReportCompleteness::Complete => "complete",
        LinuxReportCompleteness::Partial => "partial",
    };
    writeln!(output, "Probe result: {completeness}")
}

pub fn write_hardware_report<W: Write>(
    report: &LinuxHardwareReport,
    output: &mut W,
) -> io::Result<()> {
    writeln!(output, "Hardware probe: Linux")?;
    writeln!(output, "CPU:")?;
    writeln!(
        output,
        "  Model: {}",
        report.cpu_model.as_deref().unwrap_or("unknown")
    )?;
    let topology = report.cpu_topology.as_ref();
    writeln!(
        output,
        "  Logical CPUs online: {}",
        topology.map_or_else(
            || "unknown".to_owned(),
            |cpu| cpu.online_cpu_ids.len().to_string()
        )
    )?;
    writeln!(
        output,
        "  Physical cores: {}",
        topology
            .and_then(|cpu| cpu.physical_core_count)
            .map_or_else(|| "unknown".to_owned(), |count| count.to_string())
    )?;
    writeln!(
        output,
        "  CPUs present: {}",
        topology
            .and_then(|cpu| cpu.present_cpu_ids.as_ref())
            .map_or_else(|| "unknown".to_owned(), |cpus| cpus.len().to_string())
    )?;
    writeln!(
        output,
        "  CPUs possible: {}",
        topology
            .and_then(|cpu| cpu.possible_cpu_ids.as_ref())
            .map_or_else(|| "unknown".to_owned(), |cpus| cpus.len().to_string())
    )?;
    writeln!(
        output,
        "  CPUs offline: {}",
        topology
            .and_then(|cpu| cpu.offline_cpu_ids.as_ref())
            .map_or_else(|| "unknown".to_owned(), |cpus| cpus.len().to_string())
    )?;
    writeln!(
        output,
        "  CPUs allowed to process: {}",
        report
            .scheduling
            .allowed_cpu_ids
            .as_ref()
            .map_or_else(|| "unknown".to_owned(), |cpus| cpus.len().to_string())
    )?;

    writeln!(output, "CPU scheduling:")?;
    writeln!(
        output,
        "  Affinity query: {}",
        query_availability_name(report.scheduling.affinity_query)
    )?;
    writeln!(
        output,
        "  Scheduler policy query: {}",
        query_availability_name(report.scheduling.scheduler_policy_query)
    )?;
    writeln!(
        output,
        "  Current policy: {}",
        report
            .scheduling
            .scheduler_policy
            .as_deref()
            .unwrap_or("unknown")
    )?;
    writeln!(
        output,
        "  Current static priority: {}",
        report
            .scheduling
            .current_static_priority
            .map_or_else(|| "unknown".to_owned(), |priority| priority.to_string())
    )?;
    writeln!(
        output,
        "  Scheduling control permission: not assessed; no settings were changed"
    )?;

    writeln!(output, "CPU frequency/power:")?;
    writeln!(
        output,
        "  cpufreq interface: {}",
        availability_name(report.cpu_frequency.availability)
    )?;
    if report.cpu_frequency.policies.is_empty() {
        writeln!(output, "  Policies: none detected")?;
    } else {
        writeln!(output, "  Policies:")?;
        for policy in &report.cpu_frequency.policies {
            writeln!(output, "    Policy {}:", policy.policy_index)?;
            writeln!(
                output,
                "      Driver: {}",
                policy.scaling_driver.as_deref().unwrap_or("unknown")
            )?;
            writeln!(
                output,
                "      Governor: {}",
                policy.current_governor.as_deref().unwrap_or("unknown")
            )?;
            writeln!(
                output,
                "      Available governors: {}",
                display_list(&policy.available_governors)
            )?;
            writeln!(
                output,
                "      Hardware frequency limits: {}",
                format_frequency_range(policy.hardware_frequency_limits.as_ref())
            )?;
            writeln!(
                output,
                "      Policy frequency limits: {}",
                format_frequency_range(policy.policy_frequency_limits.as_ref())
            )?;
            writeln!(
                output,
                "      Energy preference: {}",
                policy
                    .energy_performance_preference
                    .as_deref()
                    .unwrap_or("unknown")
            )?;
        }
    }
    writeln!(
        output,
        "  Platform profile: {}",
        report
            .platform_power
            .current_profile
            .as_deref()
            .unwrap_or("unknown")
    )?;
    writeln!(
        output,
        "  Power interface: {}",
        availability_name(report.platform_power.availability)
    )?;
    writeln!(output, "  Control write permission: not assessed")?;
    writeln!(output, "  Momentary frequency: not sampled")?;

    writeln!(output, "Memory:")?;
    writeln!(
        output,
        "  Host-visible total RAM: {}",
        format_bytes(report.memory.host_total_bytes)
    )?;
    writeln!(
        output,
        "  Host-visible available RAM: {}",
        format_bytes(report.memory.host_available_bytes)
    )?;
    match (
        report.memory.host_swap_total_bytes,
        report.snapshot.memory.as_ref(),
    ) {
        (Some(0), _) => writeln!(output, "  Swap: absent (SwapTotal is zero)")?,
        (Some(total), Some(memory)) if memory.swap.present == Some(true) => writeln!(
            output,
            "  Swap: {} total, {} free",
            format_bytes(Some(total)),
            format_bytes(report.memory.host_swap_free_bytes)
        )?,
        (Some(_), _) => writeln!(output, "  Swap: unknown")?,
        (None, _) => writeln!(output, "  Swap: unknown")?,
    }
    writeln!(
        output,
        "  Cgroup status: {}",
        cgroup_status_name(report.cgroup.status)
    )?;
    writeln!(
        output,
        "  Cgroup CPU-time quota: {}",
        format_cpu_quota(&report.cgroup.cpu_quota)
    )?;
    writeln!(
        output,
        "  Cgroup effective CPU set: {}",
        report.cgroup.effective_cpu_set.as_ref().map_or_else(
            || "unknown".to_owned(),
            |cpus| format!("{} CPUs", cpus.len())
        )
    )?;
    writeln!(
        output,
        "  Cgroup memory limit: {}",
        format_limit(&report.cgroup.memory_limit_bytes)
    )?;
    writeln!(
        output,
        "  Cgroup memory usage: {}",
        format_bytes(report.cgroup.memory_current_bytes)
    )?;
    writeln!(
        output,
        "  Cgroup swap limit: {}",
        format_limit(&report.cgroup.memory_swap_limit_bytes)
    )?;
    writeln!(
        output,
        "  Cgroup ancestor limits: not assessed; leaf limits are shown separately from host RAM"
    )?;
    writeln!(
        output,
        "Probe result: {}",
        match report.completeness {
            LinuxHardwareCompleteness::Complete => "complete",
            LinuxHardwareCompleteness::Partial => "partial",
        }
    )
}

fn query_availability_name(value: QueryAvailability) -> &'static str {
    match value {
        QueryAvailability::Available => "available and queryable",
        QueryAvailability::Unavailable => "unavailable",
        QueryAvailability::PermissionRestricted => "permission restricted",
        QueryAvailability::Unknown => "unknown",
    }
}

fn availability_name(value: endfield_unix_domain::Availability) -> &'static str {
    match value {
        endfield_unix_domain::Availability::Unknown => "unknown",
        endfield_unix_domain::Availability::Unavailable => "unavailable",
        endfield_unix_domain::Availability::Available => "detected",
        endfield_unix_domain::Availability::Degraded => "partially detected",
    }
}

fn cgroup_status_name(value: CgroupStatus) -> &'static str {
    match value {
        CgroupStatus::V2 => "v2",
        CgroupStatus::V1Unsupported => "v1 (resource limits unsupported)",
        CgroupStatus::NotDetected => "not detected",
        CgroupStatus::Unknown => "unknown",
    }
}

fn format_bytes(bytes: Option<u64>) -> String {
    let Some(bytes) = bytes else {
        return "unknown".to_owned();
    };
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    const KIB: u64 = 1024;
    let (divisor, unit) = if bytes >= GIB {
        (GIB, "GiB")
    } else if bytes >= MIB {
        (MIB, "MiB")
    } else if bytes >= KIB {
        (KIB, "KiB")
    } else {
        return format!("{bytes} bytes");
    };
    let hundredths = (u128::from(bytes % divisor) * 100 / u128::from(divisor)) as u64;
    format!("{}.{hundredths:02} {unit}", bytes / divisor)
}

fn format_limit(value: &CgroupLimit<u64>) -> String {
    match value {
        CgroupLimit::Unknown => "unknown".to_owned(),
        CgroupLimit::Unlimited => "unlimited".to_owned(),
        CgroupLimit::Limited(bytes) => format_bytes(Some(*bytes)),
    }
}

fn format_cpu_quota(value: &CgroupLimit<CpuTimeQuota>) -> String {
    match value {
        CgroupLimit::Unknown => "unknown".to_owned(),
        CgroupLimit::Unlimited => "unlimited".to_owned(),
        CgroupLimit::Limited(quota) => format_rational_cpu_capacity(*quota),
    }
}

fn format_rational_cpu_capacity(quota: CpuTimeQuota) -> String {
    let whole = quota.quota_microseconds / quota.period_microseconds;
    let remainder = quota.quota_microseconds % quota.period_microseconds;
    if remainder == 0 {
        return format!("{whole} CPUs");
    }
    let mut remainder = u128::from(remainder);
    let denominator = u128::from(quota.period_microseconds);
    let mut decimal = String::new();
    for _ in 0..6 {
        remainder *= 10;
        decimal.push(char::from(b'0' + (remainder / denominator) as u8));
        remainder %= denominator;
        if remainder == 0 {
            let decimal = decimal.trim_end_matches('0');
            return format!("{whole}.{decimal} CPUs");
        }
    }
    format!(
        "{whole} + {}/{} CPUs",
        quota.quota_microseconds % quota.period_microseconds,
        quota.period_microseconds
    )
}

fn format_frequency_range(range: Option<&endfield_unix_platform_linux::FrequencyRange>) -> String {
    range.map_or_else(
        || "unknown".to_owned(),
        |range| {
            format!(
                "{}–{}",
                format_frequency(range.minimum_hz),
                format_frequency(range.maximum_hz)
            )
        },
    )
}

fn format_frequency(hertz: u64) -> String {
    const GHZ: u64 = 1_000_000_000;
    const MHZ: u64 = 1_000_000;
    const KHZ: u64 = 1_000;
    let (divisor, unit) = if hertz >= GHZ {
        (GHZ, "GHz")
    } else if hertz >= MHZ {
        (MHZ, "MHz")
    } else if hertz >= KHZ {
        (KHZ, "kHz")
    } else {
        return format!("{hertz} Hz");
    };
    let hundredths = (u128::from(hertz % divisor) * 100 / u128::from(divisor)) as u64;
    format!("{}.{hundredths:02} {unit}", hertz / divisor)
}

fn display_list(values: &[String]) -> String {
    if values.is_empty() {
        "unknown".to_owned()
    } else {
        values.join(", ")
    }
}

fn confidence_name(
    confidence: endfield_unix_platform_linux::EnvironmentConfidence,
) -> &'static str {
    match confidence {
        endfield_unix_platform_linux::EnvironmentConfidence::High => "high",
        endfield_unix_platform_linux::EnvironmentConfidence::Moderate => "moderate",
        endfield_unix_platform_linux::EnvironmentConfidence::Low => "low",
    }
}

fn write_general_help<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(output, "{APPLICATION_NAME} — {APPLICATION_PURPOSE}.")?;
    writeln!(output, "{APPLICATION_STAGE}\n")?;
    writeln!(output, "Usage: {APPLICATION_NAME} [OPTIONS] [COMMAND]\n")?;
    writeln!(output, "Commands:")?;
    writeln!(
        output,
        "  gui           Launch the native GTK4 desktop application"
    )?;
    writeln!(
        output,
        "  help [TOPIC]  Show help for the application or a command"
    )?;
    writeln!(
        output,
        "  about         Show application and repository information\n"
    )?;
    writeln!(
        output,
        "  diagnose report --output PATH  Write a privacy-safe support report"
    )?;
    writeln!(
        output,
        "  diagnose platform  Inspect Linux platform facts without the daemon"
    )?;
    writeln!(
        output,
        "  diagnose hardware  Inspect Linux CPU, memory, and resource controls"
    )?;
    writeln!(output, "  service ping  Verify authenticated daemon IPC")?;
    writeln!(
        output,
        "  service status  Show validated daemon lifecycle status"
    )?;
    writeln!(output, "Options:")?;
    writeln!(output, "  -h, --help    Show this help message")?;
    writeln!(output, "  --version     Show the application version\n")?;
    writeln!(
        output,
        "Running without arguments or with `gui` starts the same GTK4 application."
    )?;
    writeln!(
        output,
        "Static help, version, and about commands work without a graphical session."
    )?;
    writeln!(
        output,
        "The service commands require an independently running endfield-unixd daemon."
    )
}

fn write_gui_help<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(output, "Usage: {APPLICATION_NAME} gui\n")?;
    writeln!(output, "Launch the native GTK4 desktop application.")?;
    writeln!(
        output,
        "This uses the same startup path as running {APPLICATION_NAME} without arguments."
    )?;
    writeln!(output, "A graphical session is required.")
}

fn write_about_help<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(output, "Usage: {APPLICATION_NAME} about\n")?;
    writeln!(
        output,
        "Display the application's static purpose, version, development stage, license, and repository metadata."
    )
}

fn write_service_help<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(output, "Usage: {APPLICATION_NAME} service <ping|status>\n")?;
    writeln!(
        output,
        "Ping verifies a real authenticated local Ping/Pong exchange."
    )?;
    writeln!(
        output,
        "Status prints lifecycle and implemented component capabilities."
    )?;
    writeln!(
        output,
        "The commands never start the daemon or initialize GTK."
    )
}

fn write_diagnose_help<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(
        output,
        "Usage: {APPLICATION_NAME} diagnose <report|platform|hardware>\n"
    )?;
    writeln!(
        output,
        "Platform and hardware perform bounded, read-only Linux discovery without GTK or the daemon."
    )?;
    writeln!(
        output,
        "Report writes only to its explicit output path and does not inspect the host platform."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comes_from_package_metadata() {
        let mut output = Vec::new();

        assert!(write_version(&mut output).is_ok());
        assert_eq!(
            String::from_utf8(output),
            Ok(format!("{APPLICATION_NAME} {APPLICATION_VERSION}\n"))
        );
    }

    #[test]
    fn platform_output_labels_unknown_facts_and_partial_results() {
        use endfield_unix_domain::{
            Architecture, KernelIdentity, OsIdentity, PlatformFamily, PlatformIdentity,
            SystemSnapshot,
        };
        use endfield_unix_platform_linux::{
            EnvironmentClassification, EnvironmentConfidence, EnvironmentStatus,
            LINUX_PLATFORM_REPORT_SCHEMA_VERSION, LinuxPlatformReport, LinuxReportCompleteness,
            RuntimeDirectoryAssessment, RuntimeDirectoryStatus, RuntimeEnvironmentFacts,
        };

        let platform = PlatformIdentity::new(
            PlatformFamily::Linux,
            OsIdentity::new("Linux", None).expect("OS identity"),
            Architecture::Other("loongarch64".to_owned()),
            Some(KernelIdentity::new("Linux", Some("6.1.0".to_owned())).expect("kernel")),
            None,
        )
        .expect("platform");
        let report = LinuxPlatformReport {
            schema_version: LINUX_PLATFORM_REPORT_SCHEMA_VERSION,
            snapshot: SystemSnapshot::new(platform),
            distribution: None,
            runtime_environment: RuntimeEnvironmentFacts {
                classification: EnvironmentClassification::Unknown,
                status: EnvironmentStatus::Uncertain,
                confidence: EnvironmentConfidence::Low,
                source_ids: vec!["linux-environment"],
                diagnostic: Some("environment indicators could not all be checked"),
                uncertainty: Some("bare-metal status is not established"),
            },
            user_runtime_directory: RuntimeDirectoryAssessment {
                status: RuntimeDirectoryStatus::Unknown,
                source_id: "linux-xdg-runtime-dir",
                diagnostic: None,
            },
            completeness: LinuxReportCompleteness::Partial,
        };
        let mut output = Vec::new();
        write_platform_report(&report, &mut output).expect("report output");
        let output = String::from_utf8(output).expect("UTF-8");
        assert!(output.contains("Distribution: unknown"));
        assert!(output.contains("Architecture: loongarch64 (kernel-reported)"));
        assert!(output.contains("Environment: unknown (low confidence"));
        assert!(output.contains("User runtime directory: unknown"));
        assert!(output.contains("Probe result: partial"));
    }

    #[test]
    fn general_help_lists_platform_diagnosis() {
        let mut output = Vec::new();
        assert!(write_help(None, &mut output).is_ok());
        assert!(String::from_utf8_lossy(&output).contains("diagnose platform"));
        assert!(String::from_utf8_lossy(&output).contains("diagnose hardware"));
    }

    #[test]
    fn hardware_output_formats_fixture_values_and_keeps_cgroup_limit_separate_from_host_ram() {
        use endfield_unix_domain::{
            Architecture, KernelIdentity, OsIdentity, PlatformFamily, PlatformIdentity,
            SystemSnapshot,
        };
        use endfield_unix_platform_linux::{
            CgroupLimit, CgroupResourceFacts, CgroupStatus, CpuTimeQuota,
            EnvironmentClassification, EnvironmentConfidence, EnvironmentStatus,
            LinuxHardwareCompleteness, LinuxHardwareReport, LinuxMemoryFacts, QueryAvailability,
            RuntimeDirectoryAssessment, RuntimeDirectoryStatus, RuntimeEnvironmentFacts,
            SchedulingFacts,
        };

        let platform = PlatformIdentity::new(
            PlatformFamily::Linux,
            OsIdentity::new("Linux", None).expect("OS identity"),
            Architecture::X86_64,
            Some(KernelIdentity::new("Linux", Some("6.1.0".to_owned())).expect("kernel")),
            None,
        )
        .expect("platform");
        let mut report = LinuxHardwareReport {
            schema_version: 1,
            snapshot: SystemSnapshot::new(platform),
            distribution: None,
            runtime_environment: RuntimeEnvironmentFacts {
                classification: EnvironmentClassification::Unknown,
                status: EnvironmentStatus::Uncertain,
                confidence: EnvironmentConfidence::Low,
                source_ids: Vec::new(),
                diagnostic: None,
                uncertainty: None,
            },
            user_runtime_directory: RuntimeDirectoryAssessment {
                status: RuntimeDirectoryStatus::Missing,
                source_id: "linux-xdg-runtime-dir",
                diagnostic: None,
            },
            cpu_model: None,
            cpu_topology: None,
            scheduling: SchedulingFacts {
                affinity_query: QueryAvailability::Unknown,
                scheduler_policy_query: QueryAvailability::Unknown,
                allowed_cpu_ids: None,
                scheduler_policy: None,
                current_static_priority: None,
                mutation_permission: QueryAvailability::Unknown,
            },
            memory: LinuxMemoryFacts {
                host_total_bytes: Some(32 * 1024 * 1024 * 1024),
                host_available_bytes: Some(24 * 1024 * 1024 * 1024),
                host_swap_total_bytes: Some(0),
                host_swap_free_bytes: Some(0),
            },
            cgroup: CgroupResourceFacts {
                status: CgroupStatus::V2,
                cpu_quota: CgroupLimit::Limited(CpuTimeQuota {
                    quota_microseconds: 150_000,
                    period_microseconds: 100_000,
                }),
                effective_cpu_set: Some(vec![0, 2]),
                memory_limit_bytes: CgroupLimit::Limited(4 * 1024 * 1024 * 1024),
                memory_current_bytes: Some(2 * 1024 * 1024 * 1024),
                memory_swap_limit_bytes: CgroupLimit::Limited(0),
                ancestor_limits_assessed: false,
            },
            cpu_frequency: endfield_unix_platform_linux::CpuFrequencyFacts {
                availability: endfield_unix_domain::Availability::Unavailable,
                policies: Vec::new(),
            },
            platform_power: endfield_unix_platform_linux::PlatformPowerFacts {
                availability: endfield_unix_domain::Availability::Unavailable,
                current_profile: None,
                available_profiles: Vec::new(),
            },
            completeness: LinuxHardwareCompleteness::Partial,
        };
        report.cpu_topology = Some(endfield_unix_platform_linux::CpuTopologyFacts {
            possible_cpu_ids: Some(vec![0, 1, 2, 3]),
            present_cpu_ids: Some(vec![0, 1, 2]),
            online_cpu_ids: vec![0, 2],
            offline_cpu_ids: Some(vec![1]),
            process_allowed_cpu_ids: Some(vec![0, 2]),
            physical_core_count: Some(2),
        });
        let mut output = Vec::new();
        write_hardware_report(&report, &mut output).expect("hardware output");
        let output = String::from_utf8(output).expect("UTF-8");
        assert!(output.contains("Logical CPUs online: 2"));
        assert!(output.contains("Physical cores: 2"));
        assert!(output.contains("32.00 GiB"));
        assert!(output.contains("4.00 GiB"));
        assert!(output.contains("Cgroup CPU-time quota: 1.5 CPUs"));
        assert!(output.contains("Swap: absent"));
        assert!(output.contains("ancestor limits: not assessed"));
        assert!(output.contains("Probe result: partial"));
        assert!(!output.contains("/proc/"));
        assert!(!output.contains("/sys/"));
    }

    #[test]
    fn frequency_ranges_use_readable_units() {
        assert_eq!(format_frequency(3_600_000_000), "3.60 GHz");
        assert_eq!(format_frequency(850_000_000), "850.00 MHz");
        assert_eq!(format_frequency(850_000), "850.00 kHz");
        assert_eq!(format_frequency(800), "800 Hz");
    }

    #[test]
    fn hardware_output_labels_unknown_cpu_memory_and_cgroup_values() {
        use endfield_unix_domain::{
            Architecture, KernelIdentity, OsIdentity, PlatformFamily, PlatformIdentity,
            SystemSnapshot,
        };
        use endfield_unix_platform_linux::{
            CgroupResourceFacts, EnvironmentClassification, EnvironmentConfidence,
            EnvironmentStatus, LinuxHardwareCompleteness, LinuxHardwareReport, LinuxMemoryFacts,
            QueryAvailability, RuntimeDirectoryAssessment, RuntimeDirectoryStatus,
            RuntimeEnvironmentFacts, SchedulingFacts,
        };
        let platform = PlatformIdentity::new(
            PlatformFamily::Linux,
            OsIdentity::new("Linux", None).unwrap(),
            Architecture::X86_64,
            Some(KernelIdentity::new("Linux", None).unwrap()),
            None,
        )
        .unwrap();
        let report = LinuxHardwareReport {
            schema_version: 1,
            snapshot: SystemSnapshot::new(platform),
            distribution: None,
            runtime_environment: RuntimeEnvironmentFacts {
                classification: EnvironmentClassification::Unknown,
                status: EnvironmentStatus::Uncertain,
                confidence: EnvironmentConfidence::Low,
                source_ids: Vec::new(),
                diagnostic: None,
                uncertainty: None,
            },
            user_runtime_directory: RuntimeDirectoryAssessment {
                status: RuntimeDirectoryStatus::Missing,
                source_id: "linux-xdg-runtime-dir",
                diagnostic: None,
            },
            cpu_model: None,
            cpu_topology: None,
            scheduling: SchedulingFacts {
                affinity_query: QueryAvailability::Unknown,
                scheduler_policy_query: QueryAvailability::Unknown,
                allowed_cpu_ids: None,
                scheduler_policy: None,
                current_static_priority: None,
                mutation_permission: QueryAvailability::Unknown,
            },
            memory: LinuxMemoryFacts::default(),
            cgroup: CgroupResourceFacts::default(),
            cpu_frequency: endfield_unix_platform_linux::CpuFrequencyFacts {
                availability: endfield_unix_domain::Availability::Unknown,
                policies: Vec::new(),
            },
            platform_power: endfield_unix_platform_linux::PlatformPowerFacts {
                availability: endfield_unix_domain::Availability::Unknown,
                current_profile: None,
                available_profiles: Vec::new(),
            },
            completeness: LinuxHardwareCompleteness::Partial,
        };
        let mut output = Vec::new();
        write_hardware_report(&report, &mut output).expect("hardware output");
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Model: unknown"));
        assert!(output.contains("Logical CPUs online: unknown"));
        assert!(output.contains("Host-visible total RAM: unknown"));
        assert!(output.contains("Swap: unknown"));
        assert!(output.contains("Cgroup CPU-time quota: unknown"));
        assert!(output.contains("Probe result: partial"));
    }

    #[test]
    fn about_contains_static_package_metadata() {
        let mut output = Vec::new();

        assert!(write_about(&mut output).is_ok());
        let output = String::from_utf8_lossy(&output);
        assert!(output.contains(APPLICATION_NAME));
        assert!(output.contains(APPLICATION_VERSION));
        assert!(output.contains(APPLICATION_PURPOSE));
        assert!(output.contains(APPLICATION_LICENSE));
        assert!(output.contains(APPLICATION_REPOSITORY));
        assert!(output.contains("under development"));
    }
}
