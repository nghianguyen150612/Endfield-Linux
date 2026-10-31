use std::io::{self, Write};

use endfield_unix_domain::DisplayConnection;
use endfield_unix_platform_linux::{
    CgroupLimit, CgroupStatus, ConnectorEnabled, ConnectorState, CpuTimeQuota, DisplayOutputSource,
    DisplayOutputState, EnvironmentClassification, FrameMetricStatus, FrameSourceFormat,
    FrameSourceMetadata, FrameSourceVersion, FrameTelemetryResult, GpuSourceStatus,
    LinuxDisplayCompleteness, LinuxDisplayMode, LinuxDisplayReport, LinuxHardwareCompleteness,
    LinuxHardwareReport, LinuxPlatformReport, LinuxReportCompleteness, OutputInventoryStatus,
    ProviderStatus, QueryAvailability, RuntimeDirectoryStatus, SessionTypeHint,
    VerifiedDisplayProtocol,
};
use endfield_unix_telemetry::{
    CollectionCompleteness, MetricId, MonotonicObservation, ObservationState, ObservationTarget,
    ProviderCollection, TelemetryMeasurement,
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
        Some(HelpTopic::Prefix) => write_prefix_help(output),
    }
}

pub fn write_version<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(output, "{APPLICATION_NAME} {APPLICATION_VERSION}")
}

pub fn write_systemd_status<W: Write>(
    status: &super::systemd::ServiceStatus,
    ipc_healthy: bool,
    output: &mut W,
) -> io::Result<()> {
    use super::systemd::UnitFileState;

    writeln!(output, "systemd user manager: {}", status.manager)?;
    writeln!(
        output,
        "Unit file: {}",
        match status.unit_file {
            UnitFileState::Absent => "not installed",
            UnitFileState::Installed => "installed and matches the P020 unit",
            UnitFileState::ModifiedOrUnsafe => "modified or unsafe; contents were not trusted",
        }
    )?;
    if let Some(unit) = &status.unit {
        writeln!(output, "Manager load state: {}", unit.load_state)?;
        writeln!(output, "Autostart: {}", unit.enabled_state)?;
        writeln!(
            output,
            "Unit active state: {} ({})",
            unit.active_state, unit.sub_state
        )?;
        writeln!(output, "Main process: {}", unit.main_pid)?;
        writeln!(output, "Last result: {}", unit.result)?;
    } else if status.manager == super::systemd::ManagerAvailability::Available {
        writeln!(
            output,
            "Manager unit state: unavailable or invalid response"
        )?;
    }
    writeln!(
        output,
        "Authenticated IPC: {}",
        if ipc_healthy {
            "healthy (Ping/Pong succeeded)"
        } else {
            "unavailable or unhealthy"
        }
    )?;
    if status
        .unit
        .as_ref()
        .is_some_and(|unit| unit.active_state == "active")
        && !ipc_healthy
    {
        writeln!(
            output,
            "Discrepancy: the unit is active, but authenticated IPC failed; the daemon is not reported healthy."
        )?;
    } else if status
        .unit
        .as_ref()
        .is_none_or(|unit| unit.active_state != "active")
        && ipc_healthy
    {
        writeln!(
            output,
            "IPC is available while this unit is not active. A separately started daemon may own it; it was not adopted or stopped."
        )?;
    }
    Ok(())
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

/// Writes one validated P015 collection. The optional logical CPU count comes
/// from the P012 snapshot used to validate logical CPU identities.
pub fn write_telemetry_report<W: Write>(
    report: &ProviderCollection,
    expected_logical_cpus: Option<usize>,
    output: &mut W,
) -> io::Result<()> {
    writeln!(output, "CPU & memory telemetry: Linux")?;
    let observations = report.batch().observations();
    let aggregate = find_metric(observations, MetricId::CpuUtilizationWholeMachine);
    let interval = aggregate.and_then(cpu_window_nanos);
    let logical = observations
        .iter()
        .filter(|observation| observation.metric() == MetricId::CpuUtilizationLogicalCpu)
        .collect::<Vec<_>>();
    let logical_measured = logical
        .iter()
        .filter(|observation| observation.state().is_measured())
        .count();
    let logical_unassociated = logical
        .iter()
        .any(|observation| matches!(observation.target(), ObservationTarget::Unassociated(_)));

    writeln!(output, "CPU:")?;
    writeln!(
        output,
        "  Aggregate utilization: {}",
        aggregate
            .and_then(|observation| observation.value())
            .and_then(format_utilization)
            .unwrap_or_else(|| "unknown".to_owned())
    )?;
    writeln!(
        output,
        "  Sampling interval: {}",
        interval
            .map(format_interval)
            .unwrap_or_else(|| "unavailable".to_owned())
    )?;
    match expected_logical_cpus {
        Some(expected) => writeln!(
            output,
            "  Logical CPUs sampled: {logical_measured}/{expected}"
        )?,
        None => writeln!(output, "  Logical CPUs sampled: {logical_measured}")?,
    }
    let per_core_state = if expected_logical_cpus.is_some_and(|expected| {
        expected > 0 && logical_measured == expected && !logical_unassociated
    }) {
        "available"
    } else if logical_measured > 0 || !logical.is_empty() {
        "partial"
    } else {
        "unavailable"
    };
    writeln!(
        output,
        "  Per-core utilization: {per_core_state}{}",
        if logical_unassociated {
            " (CPU identity unresolved)"
        } else {
            ""
        }
    )?;
    let frequency = find_metric(observations, MetricId::CpuFrequency)
        .and_then(|observation| observation.value())
        .and_then(format_hertz)
        .unwrap_or_else(|| "unknown".to_owned());
    writeln!(output, "  Current frequency: {frequency}")?;

    let ram_available = find_metric(observations, MetricId::RamAvailable)
        .and_then(|observation| observation.value())
        .and_then(format_telemetry_bytes)
        .unwrap_or_else(|| "unknown".to_owned());
    let ram_used = find_metric(observations, MetricId::RamUsed)
        .and_then(|observation| observation.value())
        .and_then(format_telemetry_bytes)
        .unwrap_or_else(|| "unknown".to_owned());
    let swap_used = find_metric(observations, MetricId::SwapUsed)
        .and_then(|observation| observation.value())
        .and_then(format_telemetry_bytes)
        .unwrap_or_else(|| "unknown".to_owned());
    let cgroup_used = find_metric(observations, MetricId::CgroupMemoryUsed)
        .and_then(|observation| observation.value())
        .and_then(format_telemetry_bytes)
        .unwrap_or_else(|| "unknown".to_owned());

    writeln!(output, "Memory:")?;
    writeln!(output, "  RAM available: {ram_available}")?;
    writeln!(output, "  Estimated RAM used: {ram_used}")?;
    writeln!(output, "  Swap used: {swap_used}")?;
    writeln!(output, "  Cgroup memory usage: {cgroup_used}")?;

    let expected_logical_cpus_available = expected_logical_cpus.is_some_and(|expected| {
        expected > 0 && logical_measured == expected && !logical_unassociated
    });
    let cpu_quality = collection_quality(
        aggregate
            .into_iter()
            .map(|observation| observation.state())
            .chain(logical.iter().map(|observation| observation.state())),
        aggregate.is_some() && expected_logical_cpus_available,
    );
    let memory_quality = collection_quality(
        [
            MetricId::RamAvailable,
            MetricId::RamUsed,
            MetricId::SwapUsed,
        ]
        .into_iter()
        .filter_map(|metric| find_metric(observations, metric))
        .map(|observation| observation.state()),
        [
            MetricId::RamAvailable,
            MetricId::RamUsed,
            MetricId::SwapUsed,
        ]
        .iter()
        .all(|metric| find_metric(observations, *metric).is_some()),
    );
    writeln!(output, "Quality:")?;
    writeln!(output, "  CPU source: {cpu_quality}")?;
    writeln!(output, "  Memory source: {memory_quality}")?;
    writeln!(output, "  Clock: monotonic")?;
    writeln!(
        output,
        "  Probe result: {}",
        match report.completeness() {
            CollectionCompleteness::Complete => "complete",
            CollectionCompleteness::Partial => "partial",
        }
    )
}

pub fn write_frame_telemetry_report<W: Write>(
    report: &FrameTelemetryResult,
    output: &mut W,
) -> io::Result<()> {
    let metadata = report.source_metadata();
    let observations = report.collection().batch().observations();
    writeln!(output, "Frame telemetry: Linux")?;
    writeln!(output, "Source: {}", frame_source_name(metadata))?;
    writeln!(output, "Recording: user-supplied")?;
    writeln!(
        output,
        "Session association: import-local; game identity unknown"
    )?;
    writeln!(output, "Clock: recording-relative import-local timeline")?;
    writeln!(output, "Capture type: {}", frame_capture_name(metadata))?;
    writeln!(output)?;
    writeln!(output, "Frames:")?;
    writeln!(
        output,
        "  Valid frame interval observations: {}{}",
        report.valid_interval_count(),
        if metadata.granularity()
            == endfield_unix_platform_linux::FrameObservationGranularity::SampledIndividualFrameIntervals
        {
            " (sampled rows; not every frame)"
        } else {
            ""
        }
    )?;
    match report.recording_duration_ns() {
        Some(duration) => writeln!(
            output,
            "  First-to-last observation span: {}",
            format_frame_duration(duration)
        )?,
        None => writeln!(output, "  First-to-last observation span: unknown")?,
    }
    write_frame_metric_line(
        output,
        "FPS",
        report.fps_status(),
        find_metric(observations, MetricId::FramesPerSecond),
        true,
    )?;
    write_frame_metric_line(
        output,
        "Mean frametime",
        report.mean_status(),
        find_metric(observations, MetricId::MeanFrameTime),
        false,
    )?;
    write_frame_metric_line(
        output,
        "P99 frametime",
        report.p99_status(),
        find_metric(observations, MetricId::P99FrameTime),
        false,
    )?;
    writeln!(
        output,
        "  FrameCount: not published (recording lifetime is unknown)"
    )?;
    if let Some(count) = metadata.source_fps_sample_count() {
        writeln!(
            output,
            "  MangoHud FPS values: {count} sampled instantaneous last-frame reciprocals; no recording-wide rate window"
        )?;
    }

    writeln!(output, "Quality:")?;
    writeln!(
        output,
        "  Missing records: {}",
        metadata
            .missing_record_count()
            .map_or_else(|| "unknown".to_owned(), |count| count.to_string())
    )?;
    writeln!(
        output,
        "  Discontinuities: {}",
        metadata
            .discontinuity_count()
            .map_or_else(|| "unknown".to_owned(), |count| count.to_string())
    )?;
    writeln!(
        output,
        "  Invalid records: {}",
        metadata.invalid_record_count()
    )?;
    writeln!(
        output,
        "  Source sampling interval: {}",
        metadata
            .source_sampling_interval_ns()
            .map(format_frame_duration)
            .unwrap_or_else(|| "not reported".to_owned())
    )?;
    writeln!(
        output,
        "  Timing precision: {}",
        match metadata.format() {
            FrameSourceFormat::FrameEventsV1 => "source timestamps in nanoseconds",
            FrameSourceFormat::MangoHudCsv => {
                "decimal milliseconds converted to nanoseconds; source sampling is unknown"
            }
        }
    )?;
    writeln!(
        output,
        "  Result: {}",
        match report.collection().completeness() {
            CollectionCompleteness::Complete => "complete",
            CollectionCompleteness::Partial => "partial",
        }
    )
}

fn frame_source_name(metadata: FrameSourceMetadata) -> &'static str {
    match metadata.version() {
        FrameSourceVersion::CanonicalV1 => "frame-events-v1",
        FrameSourceVersion::MangoHudLegacyHeader => "MangoHud CSV (legacy header)",
        FrameSourceVersion::MangoHudVersionedV1 => "MangoHud CSV (versioned v1 header)",
    }
}

fn frame_capture_name(metadata: FrameSourceMetadata) -> &'static str {
    match metadata.semantics() {
        endfield_unix_platform_linux::FrameMeasurementSemantics::PresentedFrameEventTimeline => {
            "timestamped presented-frame events"
        }
        endfield_unix_platform_linux::FrameMeasurementSemantics::SampledLastFrameIntervalAndInstantaneousFps => {
            "sampled latest frame interval and instantaneous reciprocal FPS"
        }
        endfield_unix_platform_linux::FrameMeasurementSemantics::SampledFpsOnly => {
            "sampled FPS values"
        }
        endfield_unix_platform_linux::FrameMeasurementSemantics::AveragedFrametime => {
            "averaged frametime values"
        }
        endfield_unix_platform_linux::FrameMeasurementSemantics::AggregatedBenchmarkStatistics => {
            "aggregated benchmark statistics"
        }
        endfield_unix_platform_linux::FrameMeasurementSemantics::Unknown => "unknown source semantics",
    }
}

fn write_frame_metric_line<W: Write>(
    output: &mut W,
    label: &str,
    status: FrameMetricStatus,
    observation: Option<&endfield_unix_telemetry::TelemetryObservation>,
    fps: bool,
) -> io::Result<()> {
    let value = observation
        .and_then(|observation| observation.value())
        .and_then(|measurement| match measurement.value() {
            endfield_unix_domain::MeasurementValue::Unsigned(value) => Some(value),
            endfield_unix_domain::MeasurementValue::Signed(_) => None,
        });
    let formatted = value.map(|value| {
        if fps {
            format!("{}.{:03} FPS", value / 1000, value % 1000)
        } else {
            format_frame_duration(value)
        }
    });
    let suffix = match (status, formatted.is_some()) {
        (FrameMetricStatus::Measured, _) => "",
        (FrameMetricStatus::DiscontinuousTimeline, true) => " (degraded: frame-index gap)",
        (FrameMetricStatus::DiscontinuousTimeline, false) => " (unknown: frame-index gap)",
        (FrameMetricStatus::InsufficientSamples, _) => " (unknown: insufficient observations)",
        (FrameMetricStatus::NoVerifiedObservationWindow, _) => {
            " (unknown: no verified frame-event window)"
        }
        (FrameMetricStatus::UnsupportedBySource, _) => " (unsupported by source)",
    };
    writeln!(
        output,
        "  {label}: {}{suffix}",
        formatted.unwrap_or_else(|| "unknown".to_owned())
    )
}

fn format_frame_duration(nanos: u64) -> String {
    format!("{}.{:06} ms", nanos / 1_000_000, nanos % 1_000_000)
}

fn find_metric(
    observations: &[endfield_unix_telemetry::TelemetryObservation],
    metric: MetricId,
) -> Option<&endfield_unix_telemetry::TelemetryObservation> {
    observations
        .iter()
        .find(|observation| observation.metric() == metric)
}

fn cpu_window_nanos(observation: &endfield_unix_telemetry::TelemetryObservation) -> Option<u64> {
    match observation
        .temporal()
        .observation()
        .monotonic_observation()?
    {
        MonotonicObservation::Window(window) => {
            window.duration().ok().map(|duration| duration.get())
        }
        MonotonicObservation::Instant(_) => None,
    }
}

fn format_utilization(measurement: TelemetryMeasurement) -> Option<String> {
    if measurement.unit() != endfield_unix_telemetry::TelemetryUnit::BasisPoints {
        return None;
    }
    match measurement.value() {
        endfield_unix_domain::MeasurementValue::Unsigned(value) => {
            Some(format!("{}.{:02}%", value / 100, value % 100))
        }
        _ => None,
    }
}

fn format_hertz(measurement: TelemetryMeasurement) -> Option<String> {
    if measurement.unit() != endfield_unix_telemetry::TelemetryUnit::Hertz {
        return None;
    }
    match measurement.value() {
        endfield_unix_domain::MeasurementValue::Unsigned(value) => {
            Some(format_binary(value, 1_000_000_000, "GHz"))
        }
        _ => None,
    }
}

fn format_telemetry_bytes(measurement: TelemetryMeasurement) -> Option<String> {
    if measurement.unit() != endfield_unix_telemetry::TelemetryUnit::Bytes {
        return None;
    }
    match measurement.value() {
        endfield_unix_domain::MeasurementValue::Unsigned(value) => {
            const MIB: u64 = 1024 * 1024;
            const GIB: u64 = 1024 * MIB;
            if value >= GIB {
                Some(format_binary(value, GIB, "GiB"))
            } else {
                Some(format_binary(value, MIB, "MiB"))
            }
        }
        _ => None,
    }
}

fn format_binary(value: u64, divisor: u64, unit: &str) -> String {
    let hundredths = (u128::from(value) * 100 + u128::from(divisor / 2)) / u128::from(divisor);
    format!("{}.{:02} {unit}", hundredths / 100, hundredths % 100)
}

fn format_interval(nanos: u64) -> String {
    format!(
        "{}.{:03} ms",
        nanos / 1_000_000,
        (nanos % 1_000_000) / 1_000
    )
}

fn collection_quality(
    states: impl Iterator<Item = ObservationState>,
    all_present: bool,
) -> &'static str {
    let states = states.collect::<Vec<_>>();
    if all_present && !states.is_empty() && states.iter().all(|state| state.is_measured()) {
        "measured"
    } else if states.iter().any(|state| state.value().is_some()) {
        "degraded"
    } else {
        "unavailable"
    }
}

pub fn write_gpu_report<W: Write>(
    report: &endfield_unix_platform_linux::LinuxGpuReport,
    output: &mut W,
) -> io::Result<()> {
    use endfield_unix_domain::{Availability, GpuClass, GpuVendor};
    use endfield_unix_platform_linux::{
        DeviceNodeAccess, DrmNodeKind, GpuSourceStatus, LinuxGpuCompleteness, VulkanDeviceType,
        VulkanLoaderStatus, VulkanMapping,
    };

    writeln!(output, "GPU discovery: Linux")?;
    match report.pci_status {
        GpuSourceStatus::Available => {
            writeln!(output, "PCI graphics devices: {}", report.pci_devices.len())?
        }
        GpuSourceStatus::Missing => {
            writeln!(output, "PCI graphics devices: unknown (source missing)")?
        }
        GpuSourceStatus::Degraded => writeln!(
            output,
            "PCI graphics devices: {} validated (partial)",
            report.pci_devices.len()
        )?,
    }
    for (index, (pci, gpu)) in report
        .pci_devices
        .iter()
        .zip(&report.snapshot.gpus)
        .enumerate()
    {
        writeln!(output, "GPU {}:", index + 1)?;
        let vendor = match gpu.vendor.as_ref() {
            Some(GpuVendor::Intel) => "Intel".to_owned(),
            Some(GpuVendor::Nvidia) => "NVIDIA".to_owned(),
            Some(GpuVendor::Amd) => "AMD".to_owned(),
            Some(GpuVendor::Apple) => "Apple".to_owned(),
            Some(GpuVendor::Other(value)) => value.clone(),
            None => "unknown".to_owned(),
        };
        writeln!(output, "  Vendor: {vendor}")?;
        writeln!(
            output,
            "  Model: {}",
            gpu.model.as_deref().unwrap_or("unknown")
        )?;
        writeln!(
            output,
            "  PCI identity: {} (vendor 0x{:04x}, device 0x{:04x})",
            pci.bdf, pci.vendor_id, pci.device_id
        )?;
        writeln!(
            output,
            "  Kernel driver: {}",
            pci.driver_name
                .as_deref()
                .unwrap_or(match pci.driver_binding {
                    Availability::Unavailable => "none bound (sysfs link absent)",
                    Availability::Unknown | Availability::Degraded => "unknown",
                    Availability::Available => "unknown",
                })
        )?;
        let render = report
            .drm_nodes
            .iter()
            .filter(|node| {
                node.pci_bdf.as_deref() == Some(&pci.bdf) && node.kind == DrmNodeKind::Render
            })
            .collect::<Vec<_>>();
        let render_state = if render.is_empty() {
            match report.drm_status {
                GpuSourceStatus::Available => "missing",
                GpuSourceStatus::Missing => "unknown (DRM source missing)",
                GpuSourceStatus::Degraded => "unknown (DRM topology partial)",
            }
        } else if render
            .iter()
            .any(|node| node.access == DeviceNodeAccess::Accessible)
        {
            "accessible (read permission check; node not opened)"
        } else if render
            .iter()
            .any(|node| node.access == DeviceNodeAccess::Denied)
        {
            "inaccessible"
        } else if render
            .iter()
            .all(|node| node.access == DeviceNodeAccess::Missing)
        {
            "missing"
        } else {
            "unknown"
        };
        writeln!(output, "  DRM render node: {render_state}")?;
        let class = match gpu.class {
            GpuClass::Integrated => "integrated",
            GpuClass::Discrete => "discrete",
            GpuClass::Unknown => "unknown",
        };
        writeln!(output, "  GPU class: {class}")?;
        writeln!(
            output,
            "  Dedicated VRAM: {}",
            gpu.vram
                .value
                .map(|bytes| format_bytes(Some(bytes.0)))
                .unwrap_or_else(|| "unknown".to_owned())
        )?;
        let acceleration = if gpu.hardware_acceleration == Availability::Available {
            "available (Vulkan hardware device enumerated)"
        } else {
            "unknown"
        };
        writeln!(output, "  Hardware acceleration: {acceleration}")?;
        let vulkan = match gpu.vulkan_device {
            Availability::Available => "available",
            Availability::Unavailable => "unavailable (enumeration found no Vulkan device)",
            Availability::Unknown | Availability::Degraded => "unknown",
        };
        writeln!(output, "  Vulkan: {vulkan}")?;
    }
    match report.drm_status {
        GpuSourceStatus::Available => writeln!(output, "DRM nodes: {}", report.drm_nodes.len())?,
        GpuSourceStatus::Missing => writeln!(output, "DRM nodes: unknown (source missing)")?,
        GpuSourceStatus::Degraded => writeln!(
            output,
            "DRM nodes: {} validated (partial)",
            report.drm_nodes.len()
        )?,
    }
    writeln!(output, "Vulkan:")?;
    let loader = match report.vulkan_status {
        VulkanLoaderStatus::Missing => "missing or could not be loaded",
        VulkanLoaderStatus::InitializationFailed => "present, initialization failed",
        VulkanLoaderStatus::AvailableNoDevices => "available",
        VulkanLoaderStatus::Enumerated => "available",
        VulkanLoaderStatus::Degraded => "degraded",
    };
    writeln!(output, "  Loader: {loader}")?;
    let measured_vulkan_count = matches!(
        report.vulkan_status,
        VulkanLoaderStatus::AvailableNoDevices | VulkanLoaderStatus::Enumerated
    );
    let software_count = report
        .vulkan_devices
        .iter()
        .filter(|device| device.physical.device_type == VulkanDeviceType::Cpu)
        .count();
    let unmatched_count = report
        .vulkan_devices
        .iter()
        .filter(|device| device.mapping != VulkanMapping::PciIdentity)
        .count();
    let count_label = |count: usize| match report.vulkan_status {
        VulkanLoaderStatus::Missing | VulkanLoaderStatus::InitializationFailed => {
            "unknown".to_owned()
        }
        VulkanLoaderStatus::Degraded => format!("{count} validated; total unknown"),
        VulkanLoaderStatus::AvailableNoDevices | VulkanLoaderStatus::Enumerated => {
            count.to_string()
        }
    };
    writeln!(
        output,
        "  Physical devices: {}",
        count_label(report.vulkan_devices.len())
    )?;
    writeln!(
        output,
        "  Software devices: {}",
        if measured_vulkan_count {
            software_count.to_string()
        } else if report.vulkan_status == VulkanLoaderStatus::Degraded {
            format!("{software_count} validated; total unknown")
        } else {
            "unknown".to_owned()
        }
    )?;
    writeln!(
        output,
        "  Unmatched devices: {}",
        if measured_vulkan_count {
            unmatched_count.to_string()
        } else if report.vulkan_status == VulkanLoaderStatus::Degraded {
            format!("{unmatched_count} validated; total unknown")
        } else {
            "unknown".to_owned()
        }
    )?;
    for device in &report.vulkan_devices {
        let kind = match device.physical.device_type {
            VulkanDeviceType::Integrated => "integrated",
            VulkanDeviceType::Discrete => "discrete",
            VulkanDeviceType::Virtual => "virtual",
            VulkanDeviceType::Cpu => "CPU/software",
            VulkanDeviceType::Other => "other",
        };
        let api = device
            .physical
            .api_version
            .map(|(major, minor, patch)| format!("{major}.{minor}.{patch}"))
            .unwrap_or_else(|| "unknown".to_owned());
        let mapping = match device.mapping {
            VulkanMapping::PciIdentity => "matched PCI identity",
            VulkanMapping::AmbiguousPciIdentity => "ambiguous PCI identity",
            VulkanMapping::ContradictoryPciIdentity => "contradictory PCI metadata",
            VulkanMapping::Unmatched => "unmatched",
        };
        writeln!(
            output,
            "  Device: {} ({kind}; Vulkan API {api}; {mapping})",
            device.physical.name
        )?;
        if let Some(driver) = &device.physical.driver_identity {
            writeln!(
                output,
                "    Vulkan driver identity: {} (raw driver ID {})",
                driver.name, driver.driver_id
            )?;
        }
        let heaps = if device.physical.device_local_heap_bytes.is_empty() {
            "unknown".to_owned()
        } else {
            device
                .physical
                .device_local_heap_bytes
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        writeln!(
            output,
            "    Vulkan device-local heap bytes: {heaps} (not dedicated VRAM)"
        )?;
    }
    if let Some(diagnostic) = report.vulkan_diagnostic {
        writeln!(output, "  Detail: {diagnostic}")?;
    }
    writeln!(
        output,
        "Probe result: {}",
        match report.completeness {
            LinuxGpuCompleteness::Complete => "complete",
            LinuxGpuCompleteness::Partial => "partial",
        }
    )
}

pub fn write_display_report<W: Write>(
    report: &LinuxDisplayReport,
    output: &mut W,
) -> io::Result<()> {
    writeln!(output, "Display discovery: Linux")?;
    writeln!(output, "Session:")?;
    writeln!(
        output,
        "  Type hint: {}",
        session_type_name(report.session.hint)
    )?;
    writeln!(
        output,
        "  Verified display protocol: {}",
        verified_protocol_name(report.session.verified_protocol)
    )?;
    let compositor = if let Some(identity) = &report.compositor.verified_identity {
        format!("{identity} (verified)")
    } else if report.compositor.advertised_desktops.is_empty() {
        "unknown".to_owned()
    } else {
        format!(
            "unverified hint: {}",
            report.compositor.advertised_desktops.join(", ")
        )
    };
    writeln!(output, "  Compositor: {compositor}")?;
    write_protocol_summary("Wayland", &report.wayland, output)?;
    write_protocol_summary("X11/RandR", &report.x11, output)?;

    match report.output_source {
        Some(DisplayOutputSource::Wayland) => writeln!(output, "Display output source: Wayland")?,
        Some(DisplayOutputSource::X11RandR) => {
            writeln!(output, "Display output source: X11 RandR")?
        }
        None => writeln!(
            output,
            "Display output source: unknown (no protocol inventory verified)"
        )?,
    }
    if report.outputs.is_empty() {
        writeln!(output, "Displays: none verified")?;
    }
    for (index, display) in report.outputs.iter().enumerate() {
        writeln!(output, "Display {}:", index + 1)?;
        writeln!(
            output,
            "  Name: {}",
            display.info.label.as_deref().unwrap_or("unknown")
        )?;
        writeln!(
            output,
            "  Connection: {}",
            display.info.connection.map_or("unknown", connection_name)
        )?;
        writeln!(output, "  State: {}", output_state_name(display.state))?;
        writeln!(
            output,
            "  Mode resolution: {}",
            display.info.resolution.map_or_else(
                || "unknown".to_owned(),
                |resolution| format!(
                    "{}×{} pixels",
                    resolution.width_pixels, resolution.height_pixels
                )
            )
        )?;
        let current_rate = display
            .modes
            .iter()
            .find(|mode| mode.current)
            .and_then(|mode| mode.refresh_rate)
            .map(format_exact_refresh_rate)
            .or_else(|| {
                display
                    .info
                    .refresh_rate
                    .map(|rate| format!("{} Hz", rate.0))
            })
            .unwrap_or_else(|| "unknown".to_owned());
        writeln!(output, "  Refresh rate: {current_rate}")?;
        writeln!(
            output,
            "  Logical resolution: {}",
            display.logical_geometry.map_or_else(
                || "unknown".to_owned(),
                |geometry| format!("{}×{} pixels", geometry.width, geometry.height)
            )
        )?;
        writeln!(
            output,
            "  Logical position: {}",
            display.logical_geometry.map_or_else(
                || "unknown".to_owned(),
                |geometry| format!("{}, {}", geometry.x, geometry.y)
            )
        )?;
        writeln!(
            output,
            "  Scale: {}",
            display.scale.map_or_else(
                || "unknown".to_owned(),
                |scale| format!("{}/{}", scale.numerator, scale.denominator)
            )
        )?;
        writeln!(
            output,
            "  Rotation: {}",
            display
                .rotation_degrees
                .map_or_else(|| "unknown".to_owned(), |degrees| format!("{degrees}°"))
        )?;
        writeln!(
            output,
            "  Primary: {}",
            display
                .info
                .primary
                .map_or("unknown", |primary| if primary { "yes" } else { "no" })
        )?;
        if display.modes.is_empty() {
            writeln!(output, "  Advertised modes: unknown")?;
        } else {
            writeln!(output, "  Advertised modes:")?;
            for mode in &display.modes {
                writeln!(output, "    {}", format_display_mode(mode))?;
            }
        }
    }

    match (&report.drm_connectors, report.drm_connector_status) {
        (Some(connectors), GpuSourceStatus::Available) => {
            writeln!(output, "DRM connectors: {}", connectors.len())?;
            for connector in connectors {
                write_drm_connector(connector, output)?;
            }
        }
        (Some(connectors), GpuSourceStatus::Degraded) => {
            writeln!(
                output,
                "DRM connectors: {} enumerated (partial)",
                connectors.len()
            )?;
            for connector in connectors {
                write_drm_connector(connector, output)?;
            }
        }
        (Some(_), GpuSourceStatus::Missing) | (None, _) => {
            writeln!(output, "DRM connectors: unknown (source unavailable)")?
        }
    }
    writeln!(
        output,
        "Probe result: {}",
        match report.completeness {
            LinuxDisplayCompleteness::Complete => "complete",
            LinuxDisplayCompleteness::Partial => "partial",
        }
    )
}

fn write_drm_connector<W: Write>(
    connector: &endfield_unix_platform_linux::DrmConnector,
    output: &mut W,
) -> io::Result<()> {
    writeln!(
        output,
        "  {}: {}, {}, {}; GPU mapping: {}",
        connector.label,
        connector_state_name(connector.state),
        connector_enabled_name(connector.enabled),
        connection_name(connector.connection),
        connector.gpu_instance.map_or_else(
            || "unknown".to_owned(),
            |instance| format!("GPU instance {}", instance.get())
        )
    )?;
    match &connector.advertised_modes {
        None => writeln!(output, "    Advertised kernel modes: unknown"),
        Some(modes) if modes.is_empty() => {
            writeln!(output, "    Advertised kernel modes: none reported")
        }
        Some(modes) => {
            writeln!(output, "    Advertised kernel modes:")?;
            for mode in modes {
                writeln!(
                    output,
                    "      {}×{} pixels",
                    mode.width_pixels, mode.height_pixels
                )?;
            }
            Ok(())
        }
    }
}

fn write_protocol_summary<W: Write>(
    name: &str,
    protocol: &endfield_unix_platform_linux::DisplayProtocolFacts,
    output: &mut W,
) -> io::Result<()> {
    writeln!(
        output,
        "  {name}: {} (output inventory: {})",
        provider_status_name(protocol.status),
        inventory_status_name(protocol.inventory)
    )?;
    if let Some((major, minor)) = protocol.protocol_version {
        writeln!(output, "    Protocol version: {major}.{minor}")?;
    }
    if let Some(count) = protocol.output_count {
        writeln!(output, "    Output records: {count}")?;
    }
    if let Some((width, height)) = protocol.screen_geometry {
        writeln!(output, "    Screen geometry: {width}×{height} pixels")?;
    }
    if let Some(logical_output) = protocol.optional_logical_output_protocol {
        writeln!(
            output,
            "    Optional logical-output protocol: {}",
            if logical_output {
                "available"
            } else {
                "unavailable"
            }
        )?;
    }
    if let Some(diagnostic) = protocol.diagnostic {
        writeln!(output, "    Note: {diagnostic}")?;
    }
    Ok(())
}

fn format_display_mode(mode: &LinuxDisplayMode) -> String {
    let refresh = mode
        .refresh_rate
        .map(format_exact_refresh_rate)
        .unwrap_or_else(|| "unknown refresh".to_owned());
    let mut details = Vec::new();
    if mode.current {
        details.push("current");
    }
    if mode.preferred {
        details.push("preferred");
    }
    if mode.interlaced == Some(true) {
        details.push("interlaced");
    }
    if mode.doublescan == Some(true) {
        details.push("doublescan");
    }
    if details.is_empty() {
        format!("{}×{} @ {refresh}", mode.width_pixels, mode.height_pixels)
    } else {
        format!(
            "{}×{} @ {refresh} ({})",
            mode.width_pixels,
            mode.height_pixels,
            details.join(", ")
        )
    }
}

fn format_exact_refresh_rate(rate: endfield_unix_platform_linux::ExactRefreshRate) -> String {
    if rate.numerator_hz == 0 || rate.denominator == 0 {
        return "unknown refresh".to_owned();
    }
    if let Some(millihertz) = rate
        .numerator_hz
        .checked_mul(1_000)
        .filter(|value| value % rate.denominator == 0)
        .map(|value| value / rate.denominator)
    {
        let whole = millihertz / 1_000;
        let fraction = millihertz % 1_000;
        if fraction == 0 {
            format!("{whole} Hz")
        } else {
            let fractional = format!("{fraction:03}").trim_end_matches('0').to_owned();
            format!("{whole}.{fractional} Hz")
        }
    } else {
        format!("{}/{} Hz (exact)", rate.numerator_hz, rate.denominator)
    }
}

fn session_type_name(value: SessionTypeHint) -> &'static str {
    match value {
        SessionTypeHint::Wayland => "Wayland (hint)",
        SessionTypeHint::X11 => "X11 (hint)",
        SessionTypeHint::Mixed => "mixed or ambiguous",
        SessionTypeHint::Headless => "headless",
        SessionTypeHint::Unknown => "unknown",
    }
}

fn verified_protocol_name(value: VerifiedDisplayProtocol) -> &'static str {
    match value {
        VerifiedDisplayProtocol::Wayland => "Wayland",
        VerifiedDisplayProtocol::X11 => "X11",
        VerifiedDisplayProtocol::Both => "Wayland and X11",
        VerifiedDisplayProtocol::None => "none connected",
        VerifiedDisplayProtocol::Unknown => "unknown",
    }
}

fn provider_status_name(value: ProviderStatus) -> &'static str {
    match value {
        ProviderStatus::NotAdvertised => "not advertised",
        ProviderStatus::NotAttempted => "not attempted",
        ProviderStatus::Connected => "connected",
        ProviderStatus::RejectedRemote => "remote endpoint rejected",
        ProviderStatus::InvalidEndpoint => "invalid endpoint",
        ProviderStatus::AuthenticationFailed => "authentication failed",
        ProviderStatus::PermissionDenied => "permission denied",
        ProviderStatus::TimedOut => "timed out",
        ProviderStatus::Unavailable => "unavailable",
        ProviderStatus::Failed => "failed",
    }
}

fn inventory_status_name(value: OutputInventoryStatus) -> &'static str {
    match value {
        OutputInventoryStatus::Enumerated => "enumerated",
        OutputInventoryStatus::ProtocolUnavailable => "protocol unavailable",
        OutputInventoryStatus::NotAttempted => "not attempted",
        OutputInventoryStatus::Failed => "failed",
        OutputInventoryStatus::Inconsistent => "partial or inconsistent",
    }
}

fn output_state_name(value: DisplayOutputState) -> &'static str {
    match value {
        DisplayOutputState::Active => "active",
        DisplayOutputState::ConnectedInactive => "connected, inactive",
        DisplayOutputState::Disconnected => "disconnected",
        DisplayOutputState::Unknown => "unknown",
    }
}

fn connection_name(value: DisplayConnection) -> &'static str {
    match value {
        DisplayConnection::Internal => "internal",
        DisplayConnection::External => "external",
        DisplayConnection::Virtual => "virtual",
        DisplayConnection::Unknown => "unknown",
    }
}

fn connector_state_name(value: ConnectorState) -> &'static str {
    match value {
        ConnectorState::Connected => "connected",
        ConnectorState::Disconnected => "disconnected",
        ConnectorState::Unknown => "unknown state",
    }
}

fn connector_enabled_name(value: ConnectorEnabled) -> &'static str {
    match value {
        ConnectorEnabled::Enabled => "enabled",
        ConnectorEnabled::Disabled => "disabled",
        ConnectorEnabled::Unknown => "enabled state unknown",
    }
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
        "  prefix <list|show|plan|create|validate|init|recover|remove|delete>  Manage isolated Wine prefix directories\n  prefix <backup|restore|recovery> <command>  Offline snapshots and explicit recovery"
    )?;
    writeln!(
        output,
        "  diagnose platform  Inspect Linux platform facts without the daemon"
    )?;
    writeln!(
        output,
        "  diagnose hardware  Inspect Linux CPU, memory, and resource controls"
    )?;
    writeln!(
        output,
        "  diagnose gpu  Inspect Linux PCI, DRM, graphics-driver, and Vulkan facts"
    )?;
    writeln!(
        output,
        "  diagnose display  Inspect Linux outputs, sessions, and DRM connectors"
    )?;
    writeln!(
        output,
        "  diagnose compatibility [--profile ID] [--prefix-id ID] [--json]  Explain selected runtime, driver, Vulkan, and prefix evidence"
    )?;
    writeln!(
        output,
        "  diagnose runtimes [--family wine|proton] [--root PATH]  Discover local Wine and Proton files read-only"
    )?;
    writeln!(
        output,
        "  diagnose umu  Inspect local UMU and Proton integration read-only"
    )?;
    writeln!(
        output,
        "  diagnose telemetry frames --input PATH  Import selected frame data headlessly"
    )?;
    writeln!(output, "  service ping  Verify authenticated daemon IPC")?;
    writeln!(
        output,
        "  service status  Show validated daemon lifecycle status"
    )?;
    writeln!(
        output,
        "  service systemd <install|uninstall|enable|disable|start|stop|restart|status>"
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
        "`service ping` and `service status` use authenticated daemon IPC. `service systemd` explicitly manages the optional user unit."
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
    writeln!(
        output,
        "Usage: {APPLICATION_NAME} service <ping|status|systemd>\n"
    )?;
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
        "Ping and status use authenticated daemon IPC. The systemd subcommands manage the foreground daemon as a per-user service and never initialize GTK."
    )?;
    writeln!(
        output,
        "Installation is explicit, does not enable or start the unit, and never uses root privileges."
    )?;
    writeln!(
        output,
        "Run `{APPLICATION_NAME} help service` for the complete command list."
    )
}

fn write_diagnose_help<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(
        output,
        "Runtime verification: diagnose runtime-integrity --help (user-pinned matches exit 3)."
    )?;
    writeln!(
        output,
        "Usage: {APPLICATION_NAME} diagnose <report|platform|hardware|gpu|display|compatibility|runtimes|umu|runtime-integrity|telemetry>\nUsage: {APPLICATION_NAME} runtime <install|list-managed|audit|audit-source>\n"
    )?;
    writeln!(
        output,
        "Platform, hardware, GPU, display, compatibility, runtime discovery, and live telemetry run without GTK or the daemon. Compatibility diagnostics inspect only the selected P027 runner and an optional registered managed prefix; they never execute Wine, Proton, UMU, or the game."
    )?;
    writeln!(
        output,
        "Compatibility: {APPLICATION_NAME} diagnose compatibility [--profile PROFILE_ID] [--prefix-id PREFIX_ID] [--json]. Static observations do not prove game launch or rendering."
    )?;
    writeln!(
        output,
        "Telemetry usage: {APPLICATION_NAME} diagnose telemetry cpu-memory [--interval-ms 250] (100–2000 ms)."
    )?;
    writeln!(
        output,
        "Frame import: {APPLICATION_NAME} diagnose telemetry frames --input PATH [--format frame-events-v1|mangohud-csv]."
    )?;
    writeln!(
        output,
        "Report writes only to its explicit output path and does not inspect the host platform."
    )
}

fn write_prefix_help<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(
        output,
        "Usage: {APPLICATION_NAME} prefix <list|show|plan|create|validate|init|recover|remove|delete>\n       {APPLICATION_NAME} prefix <backup|restore|recovery> <command>\n"
    )?;
    writeln!(
        output,
        "Prefix directories are owned by Endfield-Unix under the P006 XDG data directory. External Wine, Steam, Bottles, Lutris, and Heroic prefixes are read-only and are not imported."
    )?;
    writeln!(
        output,
        "Plan is read-only. Create requires an explicit machine/profile scope, a valid P027 selection, and the current prefix-registry revision. It creates an empty managed directory and does not initialize Wine."
    )?;
    writeln!(
        output,
        "Init currently returns initialization_unsupported because no validated Wine/Proton initialization adapter is available."
    )?;
    writeln!(
        output,
        "Remove drops only registry ownership and preserves data. Delete removes data only after --confirm-path exactly matches the previewed managed target."
    )?;
    writeln!(
        output,
        "Backup list/pending and restore plan are read-only. Backup create and recovery run require --confirm; restore apply requires --expected-revision and --confirm restore:ID:BACKUP. Recovery pin separately requires --expected-pin-revision and --confirm pin:BACKUP. Use nested --help for every command. Prefix recover handles P028 creation; prefix recovery handles snapshot transactions."
    )?;
    writeln!(
        output,
        "Examples: {APPLICATION_NAME} prefix plan --id game --scope machine; {APPLICATION_NAME} prefix create --id game --scope machine --expected-revision REV; {APPLICATION_NAME} prefix delete --id game --expected-revision REV --confirm-path /exact/managed/path"
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
        assert!(String::from_utf8_lossy(&output).contains("diagnose gpu"));
        assert!(String::from_utf8_lossy(&output).contains("diagnose display"));
    }

    #[test]
    fn telemetry_formatting_uses_basis_points_and_binary_memory_units() {
        assert_eq!(
            format_utilization(TelemetryMeasurement::new(
                endfield_unix_domain::MeasurementValue::Unsigned(2_537),
                endfield_unix_telemetry::TelemetryUnit::BasisPoints,
            )),
            Some("25.37%".to_owned())
        );
        assert_eq!(
            format_telemetry_bytes(TelemetryMeasurement::new(
                endfield_unix_domain::MeasurementValue::Unsigned(1536 * 1024),
                endfield_unix_telemetry::TelemetryUnit::Bytes,
            )),
            Some("1.50 MiB".to_owned())
        );
        assert_eq!(
            format_telemetry_bytes(TelemetryMeasurement::new(
                endfield_unix_domain::MeasurementValue::Unsigned(1024 * 1024 * 1024),
                endfield_unix_telemetry::TelemetryUnit::Bytes,
            )),
            Some("1.00 GiB".to_owned())
        );
    }

    #[test]
    fn diagnose_help_documents_the_on_demand_telemetry_command() {
        let mut output = Vec::new();
        write_diagnose_help(&mut output).expect("diagnose help");
        let output = String::from_utf8(output).expect("UTF-8 help");
        assert!(output.contains("diagnose telemetry cpu-memory"));
        assert!(output.contains("100–2000 ms"));
        assert!(output.contains("without GTK or the daemon"));
    }

    #[test]
    fn display_rate_formatting_preserves_exact_fractional_values() {
        use endfield_unix_platform_linux::ExactRefreshRate;

        assert_eq!(
            format_exact_refresh_rate(ExactRefreshRate::new(60, 1).unwrap()),
            "60 Hz"
        );
        assert_eq!(
            format_exact_refresh_rate(ExactRefreshRate::new(2_997, 50).unwrap()),
            "59.94 Hz"
        );
        assert_eq!(
            format_exact_refresh_rate(ExactRefreshRate::new(1, 3).unwrap()),
            "1/3 Hz (exact)"
        );
        assert_eq!(
            format_exact_refresh_rate(ExactRefreshRate {
                numerator_hz: 60,
                denominator: 0,
            }),
            "unknown refresh"
        );
        assert_eq!(
            format_display_mode(&LinuxDisplayMode {
                width_pixels: 2560,
                height_pixels: 1440,
                refresh_rate: ExactRefreshRate::new(1_999, 20),
                current: true,
                preferred: true,
                interlaced: None,
                doublescan: None,
            }),
            "2560×1440 @ 99.95 Hz (current, preferred)"
        );
    }

    #[test]
    fn drm_connector_output_distinguishes_unavailable_and_empty_mode_lists() {
        let connector = |advertised_modes| endfield_unix_platform_linux::DrmConnector {
            label: "DP-1".to_owned(),
            state: ConnectorState::Connected,
            enabled: ConnectorEnabled::Disabled,
            advertised_modes,
            connection: DisplayConnection::External,
            gpu_instance: None,
        };

        let mut unavailable = Vec::new();
        write_drm_connector(&connector(None), &mut unavailable).unwrap();
        assert!(
            String::from_utf8(unavailable)
                .unwrap()
                .contains("Advertised kernel modes: unknown")
        );

        let mut empty = Vec::new();
        write_drm_connector(&connector(Some(Vec::new())), &mut empty).unwrap();
        assert!(
            String::from_utf8(empty)
                .unwrap()
                .contains("Advertised kernel modes: none reported")
        );
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
    fn gpu_output_formats_a_partial_multi_provider_fixture_without_fabricating_facts() {
        use endfield_unix_domain::{
            Architecture, Availability, Bytes, CapabilityValue, GpuClass, GpuInfo, GpuVendor,
            InstanceId, KernelIdentity, OsIdentity, PlatformFamily, PlatformIdentity,
            SystemSnapshot,
        };
        use endfield_unix_platform_linux::{
            CgroupResourceFacts, GpuSourceStatus, LinuxGpuCompleteness, LinuxGpuReport,
            LinuxHardwareCompleteness, LinuxHardwareReport, LinuxMemoryFacts, LinuxVulkanDevice,
            PciGraphicsDevice, QueryAvailability, RuntimeDirectoryAssessment,
            RuntimeDirectoryStatus, RuntimeEnvironmentFacts, SchedulingFacts, VulkanDeviceType,
            VulkanDriverIdentity, VulkanLoaderStatus, VulkanMapping, VulkanMetadataStatus,
            VulkanPhysicalDevice,
        };
        use endfield_unix_platform_linux::{
            EnvironmentClassification, EnvironmentConfidence, EnvironmentStatus,
        };

        let platform = PlatformIdentity::new(
            PlatformFamily::Linux,
            OsIdentity::new("Linux", None).expect("OS identity"),
            Architecture::X86_64,
            Some(KernelIdentity::new("Linux", None).expect("kernel")),
            None,
        )
        .expect("platform");
        let hardware = LinuxHardwareReport {
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
                availability: Availability::Unknown,
                policies: Vec::new(),
            },
            platform_power: endfield_unix_platform_linux::PlatformPowerFacts {
                availability: Availability::Unknown,
                current_profile: None,
                available_profiles: Vec::new(),
            },
            completeness: LinuxHardwareCompleteness::Partial,
        };
        let mut snapshot = hardware.snapshot.clone();
        snapshot.gpus.push(GpuInfo {
            instance: InstanceId::new(2).expect("instance"),
            availability: Availability::Available,
            vendor: Some(GpuVendor::Intel),
            model: None,
            class: GpuClass::Unknown,
            graphics_driver: None,
            driver_availability: Availability::Unknown,
            vram: CapabilityValue::<Bytes>::new(Availability::Unknown, None),
            hardware_acceleration: Availability::Unknown,
            vulkan_device: Availability::Unknown,
        });
        let report = LinuxGpuReport {
            schema_version: 1,
            snapshot,
            hardware,
            pci_devices: vec![PciGraphicsDevice {
                bdf: "0000:00:02.0".to_owned(),
                class_code: 0x030000,
                vendor_id: 0x8086,
                device_id: 0x9a49,
                subsystem_vendor_id: None,
                subsystem_device_id: None,
                driver_name: None,
                driver_binding: Availability::Unknown,
                driver_diagnostic: None,
            }],
            pci_status: GpuSourceStatus::Available,
            drm_nodes: Vec::new(),
            drm_status: GpuSourceStatus::Missing,
            vulkan_status: VulkanLoaderStatus::Missing,
            vulkan_devices: Vec::new(),
            vulkan_diagnostic: None,
            completeness: LinuxGpuCompleteness::Partial,
        };
        let mut output = Vec::new();
        write_gpu_report(&report, &mut output).expect("GPU output");
        let output = String::from_utf8(output).expect("UTF-8");
        assert!(output.contains("GPU 1:"));
        assert!(output.contains("Vendor: Intel"));
        assert!(output.contains("Model: unknown"));
        assert!(output.contains("PCI identity: 0000:00:02.0"));
        assert!(output.contains("Kernel driver: unknown"));
        assert!(output.contains("DRM render node: unknown (DRM source missing)"));
        assert!(output.contains("GPU class: unknown"));
        assert!(output.contains("Dedicated VRAM: unknown"));
        assert!(output.contains("Vulkan: unknown"));
        assert!(output.contains("Loader: missing or could not be loaded"));
        assert!(output.contains("Physical devices: unknown"));
        assert!(output.contains("Probe result: partial"));
        assert!(!output.contains("/sys/"));
        assert!(!output.contains("/dev/"));

        let mut enumerated = report;
        enumerated.vulkan_status = VulkanLoaderStatus::Enumerated;
        enumerated.vulkan_devices.push(LinuxVulkanDevice {
            physical: VulkanPhysicalDevice {
                name: "Mesa software renderer".to_owned(),
                vendor_id: 0x10005,
                device_id: 1,
                pci_identity: None,
                driver_identity: Some(VulkanDriverIdentity {
                    driver_id: 13,
                    name: "Mesa lavapipe".to_owned(),
                }),
                device_type: VulkanDeviceType::Cpu,
                api_version: Some((1, 3, 0)),
                supported_extensions: Vec::new(),
                extensions_status: VulkanMetadataStatus::Unavailable,
                supported_features: Vec::new(),
                features_status: VulkanMetadataStatus::Available,
                device_local_heap_bytes: vec![1_073_741_824],
            },
            mapping: VulkanMapping::Unmatched,
            gpu_instance: None,
        });
        let mut output = Vec::new();
        write_gpu_report(&enumerated, &mut output).expect("enumerated GPU output");
        let output = String::from_utf8(output).expect("UTF-8");
        assert!(output.contains("Vulkan driver identity: Mesa lavapipe (raw driver ID 13)"));
        assert!(output.contains("CPU/software; Vulkan API 1.3.0; unmatched"));
        assert!(output.contains("Vulkan device-local heap bytes: 1073741824 (not dedicated VRAM)"));
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
