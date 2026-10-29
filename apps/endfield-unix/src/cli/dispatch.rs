use std::error::Error;
use std::fmt;
use std::io::{self, Write};
use std::path::Path;

use super::commands::Command;
use super::{output, parser};
use crate::{APPLICATION_NAME, GuiStartupError, start_gui};
use endfield_unix_domain::{ProbeError, ProbeErrorKind};
use endfield_unix_platform_linux::{
    FrameInputError, FrameInputErrorKind, FrameLogTelemetryProvider, FrameSourceFormat,
    LinuxCpuMemoryTelemetryProvider, LinuxCpuSnapshotContext, LinuxDisplayProbe, LinuxGpuProbe,
    LinuxHardwareProbe, LinuxHardwareReport, LinuxPlatformProbe, LinuxPlatformReport,
    LinuxTelemetryConfigError, SystemLinuxDataSource, TelemetryInterval, read_frame_recording,
};
use endfield_unix_telemetry::{
    CollectionRequest, MetricId, ProviderError, ProviderErrorKind, SamplingContextId,
    SnapshotRefId, TelemetryError, TelemetryProvider,
};

pub const EXIT_SUCCESS: u8 = 0;
pub const EXIT_INVALID_ARGUMENTS: u8 = 2;
pub const EXIT_GUI_STARTUP_ERROR: u8 = 69;
pub const EXIT_IO_ERROR: u8 = 74;
pub const EXIT_IPC_UNAVAILABLE: u8 = 75;
pub const EXIT_IPC_PROTOCOL_ERROR: u8 = 76;
pub const EXIT_IPC_AUTHENTICATION_ERROR: u8 = 77;
pub const EXIT_IPC_REQUEST_REJECTED: u8 = 65;
pub const EXIT_PLATFORM_PROBE_FAILED: u8 = 70;
pub const EXIT_PLATFORM_UNSUPPORTED: u8 = 78;
pub const EXIT_SYSTEMD_ERROR: u8 = 79;

#[derive(Debug)]
pub enum CliError {
    InvalidArguments(clap::Error),
    Output(io::Error),
    GuiStartup(GuiStartupError),
    Diagnostics(endfield_unix_diagnostics::DiagnosticsError),
    Ipc(endfield_unix_ipc::ClientError),
    PlatformProbe(ProbeError),
    HardwareProbe(ProbeError),
    GpuProbe(ProbeError),
    DisplayProbe(ProbeError),
    TelemetryProvider(ProviderError),
    TelemetryContract(TelemetryError),
    TelemetryConfig(LinuxTelemetryConfigError),
    FrameInput(FrameInputError),
    RuntimeDiscovery(endfield_unix_runtime_linux::DiscoveryConfigError),
    RuntimeDiscoveryFailed,
    RuntimeVerification(u8),
    RuntimeDiscoveryUnsupported,
    Systemd(super::systemd::SystemdError),
}

impl CliError {
    pub fn exit_status(&self) -> u8 {
        match self {
            Self::RuntimeVerification(code) => *code,
            Self::InvalidArguments(_) => EXIT_INVALID_ARGUMENTS,
            Self::Output(_) => EXIT_IO_ERROR,
            Self::GuiStartup(_) => EXIT_GUI_STARTUP_ERROR,
            Self::Diagnostics(_) => EXIT_IO_ERROR,
            Self::Ipc(error) => match error {
                endfield_unix_ipc::ClientError::RuntimeDirectoryUnavailable
                | endfield_unix_ipc::ClientError::UnsafeSocketPath
                | endfield_unix_ipc::ClientError::DaemonUnavailable
                | endfield_unix_ipc::ClientError::ConnectionTimeout
                | endfield_unix_ipc::ClientError::RequestTimeout
                | endfield_unix_ipc::ClientError::ServiceShuttingDown
                | endfield_unix_ipc::ClientError::ServerBusy
                | endfield_unix_ipc::ClientError::UnsupportedPlatform => EXIT_IPC_UNAVAILABLE,
                endfield_unix_ipc::ClientError::AuthenticationDenied => {
                    EXIT_IPC_AUTHENTICATION_ERROR
                }
                endfield_unix_ipc::ClientError::ProtocolMismatch
                | endfield_unix_ipc::ClientError::InvalidResponse => EXIT_IPC_PROTOCOL_ERROR,
                endfield_unix_ipc::ClientError::RequestRejected => EXIT_IPC_REQUEST_REJECTED,
            },
            Self::PlatformProbe(error) => {
                if error.kind() == ProbeErrorKind::Unsupported {
                    EXIT_PLATFORM_UNSUPPORTED
                } else {
                    EXIT_PLATFORM_PROBE_FAILED
                }
            }
            Self::HardwareProbe(error) => {
                if error.kind() == ProbeErrorKind::Unsupported {
                    EXIT_PLATFORM_UNSUPPORTED
                } else {
                    EXIT_PLATFORM_PROBE_FAILED
                }
            }
            Self::GpuProbe(error) => {
                if error.kind() == ProbeErrorKind::Unsupported {
                    EXIT_PLATFORM_UNSUPPORTED
                } else {
                    EXIT_PLATFORM_PROBE_FAILED
                }
            }
            Self::DisplayProbe(error) => {
                if error.kind() == ProbeErrorKind::Unsupported {
                    EXIT_PLATFORM_UNSUPPORTED
                } else {
                    EXIT_PLATFORM_PROBE_FAILED
                }
            }
            Self::TelemetryProvider(error) => {
                if error.kind() == ProviderErrorKind::Unsupported {
                    EXIT_PLATFORM_UNSUPPORTED
                } else {
                    EXIT_PLATFORM_PROBE_FAILED
                }
            }
            Self::TelemetryContract(_) => EXIT_PLATFORM_PROBE_FAILED,
            Self::TelemetryConfig(_) => EXIT_INVALID_ARGUMENTS,
            Self::FrameInput(error) => match error.kind() {
                FrameInputErrorKind::Io => EXIT_IO_ERROR,
                FrameInputErrorKind::UnsupportedFormat => EXIT_PLATFORM_UNSUPPORTED,
                FrameInputErrorKind::Malformed => EXIT_PLATFORM_PROBE_FAILED,
            },
            Self::RuntimeDiscovery(_) => EXIT_INVALID_ARGUMENTS,
            Self::RuntimeDiscoveryFailed => EXIT_PLATFORM_PROBE_FAILED,
            Self::RuntimeDiscoveryUnsupported => EXIT_PLATFORM_UNSUPPORTED,
            Self::Systemd(_) => EXIT_SYSTEMD_ERROR,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeVerification(_) => {
                formatter.write_str("runtime verification completed; see report.")
            }
            Self::InvalidArguments(error) => fmt::Display::fmt(error, formatter),
            Self::Output(_) => formatter.write_str("failed to write command output."),
            Self::GuiStartup(error) => error.fmt(formatter),
            Self::Diagnostics(error) => fmt::Display::fmt(error, formatter),
            Self::Ipc(error) => fmt::Display::fmt(error, formatter),
            Self::PlatformProbe(error) => fmt::Display::fmt(error, formatter),
            Self::HardwareProbe(error) => fmt::Display::fmt(error, formatter),
            Self::GpuProbe(error) => fmt::Display::fmt(error, formatter),
            Self::DisplayProbe(error) => fmt::Display::fmt(error, formatter),
            Self::TelemetryProvider(_) => formatter.write_str("Linux telemetry provider failed."),
            Self::TelemetryContract(_) => {
                formatter.write_str("Linux telemetry could not satisfy the telemetry contract.")
            }
            Self::TelemetryConfig(LinuxTelemetryConfigError::InvalidInterval) => {
                formatter.write_str("telemetry interval must be between 100 and 2000 ms.")
            }
            Self::TelemetryConfig(LinuxTelemetryConfigError::InvalidCpuSnapshotContext) => {
                formatter.write_str("CPU snapshot context could not be validated.")
            }
            Self::FrameInput(error) => match error.kind() {
                FrameInputErrorKind::Io => {
                    formatter.write_str("could not read the explicitly selected frame recording.")
                }
                FrameInputErrorKind::UnsupportedFormat => {
                    formatter.write_str("the selected frame recording format is unsupported.")
                }
                FrameInputErrorKind::Malformed => formatter.write_str(
                    "the selected frame recording is malformed or exceeds the input limits.",
                ),
            },
            Self::RuntimeDiscovery(_) => {
                formatter.write_str("runtime discovery options are invalid.")
            }
            Self::RuntimeDiscoveryFailed => {
                formatter.write_str("all accessible runtime discovery sources failed.")
            }
            Self::RuntimeDiscoveryUnsupported => {
                formatter.write_str("Linux runtime discovery is not supported on this platform.")
            }
            Self::Systemd(error) => fmt::Display::fmt(error, formatter),
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RuntimeVerification(_) => None,
            Self::InvalidArguments(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::GuiStartup(error) => Some(error),
            Self::Diagnostics(error) => Some(error),
            Self::Ipc(error) => Some(error),
            Self::PlatformProbe(error) => Some(error),
            Self::HardwareProbe(error) => Some(error),
            Self::GpuProbe(error) => Some(error),
            Self::DisplayProbe(error) => Some(error),
            Self::TelemetryProvider(_) => None,
            Self::TelemetryContract(error) => Some(error),
            Self::TelemetryConfig(_) => None,
            Self::FrameInput(_) => None,
            Self::RuntimeDiscovery(_) => None,
            Self::RuntimeDiscoveryFailed => None,
            Self::RuntimeDiscoveryUnsupported => None,
            Self::Systemd(error) => Some(error),
        }
    }
}

/// Runs the CLI or launches the GUI using process standard streams.
pub fn run<I>(arguments: I) -> u8
where
    I: IntoIterator<Item = std::ffi::OsString>,
{
    let stdout = io::stdout();
    let stderr = io::stderr();
    let mut stdout = stdout.lock();
    let mut stderr = stderr.lock();

    run_with_io(arguments, &mut stdout, &mut stderr)
}

/// Runs command dispatch with injectable writers for deterministic tests and callers.
pub fn run_with_io<I, O, E>(arguments: I, stdout: &mut O, stderr: &mut E) -> u8
where
    I: IntoIterator<Item = std::ffi::OsString>,
    O: Write,
    E: Write,
{
    run_with_io_and_gui_and_probes(
        arguments,
        stdout,
        stderr,
        start_gui,
        || LinuxPlatformProbe::system().inspect(),
        || LinuxHardwareProbe::system().inspect(),
    )
}

fn run_gpu_telemetry<O: Write>(interval_ms: u64, stdout: &mut O) -> Result<(), CliError> {
    use endfield_unix_platform_linux::gpu_nvidia::SystemNvml;
    use endfield_unix_platform_linux::gpu_telemetry::{
        GPU_METRICS, GpuIdentityContext, LinuxGpuTelemetryProvider,
    };
    use endfield_unix_platform_linux::gpu_telemetry_sources::GpuSensorSource;
    TelemetryInterval::from_millis(interval_ms).map_err(CliError::TelemetryConfig)?;
    let source = SystemLinuxDataSource::new();
    let platform = LinuxPlatformProbe::system()
        .inspect()
        .map_err(CliError::PlatformProbe)?;
    let context =
        GpuIdentityContext::discover(&source, platform.snapshot, SnapshotRefId::new(1).unwrap())
            .ok();
    let provider = LinuxGpuTelemetryProvider::new(source.clone(), SystemNvml, context);
    let request = CollectionRequest::new(
        SamplingContextId::new(1).unwrap(),
        GPU_METRICS.to_vec(),
        None,
    )
    .map_err(CliError::TelemetryContract)?;
    let report = provider
        .collect(&request)
        .map_err(CliError::TelemetryProvider)?;
    writeln!(stdout, "GPU telemetry: Linux").map_err(CliError::Output)?;
    if let Some(context) = provider.context() {
        if context.devices().is_empty() {
            writeln!(stdout, "GPUs: none verified").map_err(CliError::Output)?;
        }
        for (i, device) in context.devices().iter().enumerate() {
            let vendor = match device.vendor_id {
                0x10de => "NVIDIA",
                0x8086 => "Intel",
                0x1002 => "AMD",
                _ => "Other PCI vendor",
            };
            writeln!(stdout,"\nGPU {}:\n  Vendor: {}\n  PCI identity: {}\n  Kernel driver: {}\n  Power state: {:?}",i+1,vendor,device.bdf,device.driver_name.as_deref().unwrap_or("unknown"),source.power(device.bdf.parse().unwrap())).map_err(CliError::Output)?;
            for observation in report
                .batch()
                .observations()
                .iter()
                .filter(|o| Some(o.target()) == context.target(i))
            {
                writeln!(
                    stdout,
                    "  {}: {}",
                    gpu_metric_label(observation.metric()),
                    gpu_metric_value(observation)
                )
                .map_err(CliError::Output)?;
            }
        }
    } else {
        writeln!(
            stdout,
            "GPU identity: unavailable; no measurements assigned"
        )
        .map_err(CliError::Output)?;
    }
    writeln!(stdout, "\nSources:").map_err(CliError::Output)?;
    for (vendor, label, provider_id) in [
        (0x10de, "NVIDIA NVML", "linux-nvidia-nvml"),
        (0x1002, "AMDGPU sysfs/hwmon", "linux-amdgpu-sysfs"),
        (0x8086, "Intel interfaces", "linux-intel-sysfs"),
    ] {
        let queried = provider.context().is_some_and(|c| {
            c.devices().iter().any(|d| {
                d.vendor_id == vendor
                    && source.power(d.bdf.parse().unwrap())
                        == endfield_unix_platform_linux::gpu_telemetry_sources::RuntimePower::Active
            })
        });
        let measured =
            report.batch().observations().iter().any(|o| {
                o.provenance().provider().as_str() == provider_id && o.state().is_measured()
            });
        let status = if measured {
            "available (partial metrics possible)"
        } else if queried {
            "unavailable or unsupported"
        } else {
            "not queried"
        };
        writeln!(stdout, "  {label}: {status}").map_err(CliError::Output)?;
    }
    writeln!(stdout,"Utilization: unsupported normalized window; one-shot busy percentages are not interval averages\nProbe result: {:?}",report.completeness()).map_err(CliError::Output)
}
fn gpu_metric_label(metric: MetricId) -> &'static str {
    match metric {
        MetricId::GpuUtilization => "Utilization",
        MetricId::GpuMemoryUsed => "GPU memory used",
        MetricId::GpuDedicatedMemoryCapacity => "Dedicated VRAM",
        MetricId::GpuTemperature => "Temperature",
        MetricId::GpuPower => "Power",
        MetricId::GpuFrequency => "GPU frequency",
        _ => "Unknown metric",
    }
}
fn gpu_metric_value(observation: &endfield_unix_telemetry::TelemetryObservation) -> String {
    use endfield_unix_domain::MeasurementValue;
    use endfield_unix_telemetry::ObservationState;
    let ObservationState::Measured(value) = observation.state() else {
        return match observation.state() {
            ObservationState::Unsupported => "Unsupported",
            ObservationState::PermissionDenied => "Unavailable (permission denied)",
            ObservationState::MalformedSourceData => "Unavailable (malformed source)",
            ObservationState::TemporaryFailure => "Unavailable (temporary failure)",
            ObservationState::Degraded(_) => "Unavailable (degraded)",
            _ => "Unknown",
        }
        .to_owned();
    };
    let raw = match value.value() {
        MeasurementValue::Unsigned(v) => v as f64,
        MeasurementValue::Signed(v) => v as f64,
    };
    match observation.metric() {
        MetricId::GpuMemoryUsed | MetricId::GpuDedicatedMemoryCapacity => {
            format!("{:.2} MiB", raw / 1048576.0)
        }
        MetricId::GpuTemperature => format!("{:.3} °C", raw / 1000.0),
        MetricId::GpuPower => format!("{:.3} W", raw / 1000.0),
        MetricId::GpuFrequency => format!("{:.3} MHz", raw / 1000000.0),
        MetricId::GpuUtilization => format!("{:.2}%", raw / 100.0),
        _ => "Unknown".to_owned(),
    }
}

#[cfg(test)]
pub(crate) fn run_with_io_and_gui<I, O, E, G>(
    arguments: I,
    stdout: &mut O,
    stderr: &mut E,
    gui_start: G,
) -> u8
where
    I: IntoIterator<Item = std::ffi::OsString>,
    O: Write,
    E: Write,
    G: FnOnce() -> Result<(), GuiStartupError>,
{
    run_with_io_and_gui_and_probes(
        arguments,
        stdout,
        stderr,
        gui_start,
        || LinuxPlatformProbe::system().inspect(),
        || LinuxHardwareProbe::system().inspect(),
    )
}

#[cfg(test)]
pub(crate) fn run_with_io_and_gui_and_platform<I, O, E, G, P>(
    arguments: I,
    stdout: &mut O,
    stderr: &mut E,
    gui_start: G,
    platform_inspect: P,
) -> u8
where
    I: IntoIterator<Item = std::ffi::OsString>,
    O: Write,
    E: Write,
    G: FnOnce() -> Result<(), GuiStartupError>,
    P: FnOnce() -> Result<LinuxPlatformReport, ProbeError>,
{
    run_with_io_and_gui_and_probes(
        arguments,
        stdout,
        stderr,
        gui_start,
        platform_inspect,
        || LinuxHardwareProbe::system().inspect(),
    )
}

pub(crate) fn run_with_io_and_gui_and_probes<I, O, E, G, P, H>(
    arguments: I,
    stdout: &mut O,
    stderr: &mut E,
    gui_start: G,
    platform_inspect: P,
    hardware_inspect: H,
) -> u8
where
    I: IntoIterator<Item = std::ffi::OsString>,
    O: Write,
    E: Write,
    G: FnOnce() -> Result<(), GuiStartupError>,
    P: FnOnce() -> Result<LinuxPlatformReport, ProbeError>,
    H: FnOnce() -> Result<LinuxHardwareReport, ProbeError>,
{
    let command = match parser::parse(arguments).map_err(CliError::InvalidArguments) {
        Ok(command) => command,
        Err(error) => return report_error(stderr, &error.to_string(), error.exit_status()),
    };

    let result = execute(
        command,
        stdout,
        gui_start,
        platform_inspect,
        hardware_inspect,
    );
    match result {
        Ok(()) => EXIT_SUCCESS,
        Err(error) => report_error(stderr, &error.to_string(), error.exit_status()),
    }
}

fn execute<O, G, P, H>(
    command: Command,
    stdout: &mut O,
    gui_start: G,
    platform_inspect: P,
    hardware_inspect: H,
) -> Result<(), CliError>
where
    O: Write,
    G: FnOnce() -> Result<(), GuiStartupError>,
    P: FnOnce() -> Result<LinuxPlatformReport, ProbeError>,
    H: FnOnce() -> Result<LinuxHardwareReport, ProbeError>,
{
    match command {
        Command::Help(topic) => output::write_help(topic, stdout).map_err(CliError::Output),
        Command::HelpOutput(help) => stdout.write_all(help.as_bytes()).map_err(CliError::Output),
        Command::Version => output::write_version(stdout).map_err(CliError::Output),
        Command::About => output::write_about(stdout).map_err(CliError::Output),
        Command::LaunchGui => gui_start().map_err(CliError::GuiStartup),
        Command::DiagnoseReport {
            output: path,
            overwrite,
        } => {
            endfield_unix_diagnostics::export_support_report(&path, overwrite)
                .map_err(CliError::Diagnostics)?;
            write_report_success(stdout, &path).map_err(CliError::Output)
        }
        Command::DiagnosePlatform => {
            let report = platform_inspect().map_err(CliError::PlatformProbe)?;
            output::write_platform_report(&report, stdout).map_err(CliError::Output)
        }
        Command::DiagnoseHardware => {
            let report = hardware_inspect().map_err(CliError::HardwareProbe)?;
            output::write_hardware_report(&report, stdout).map_err(CliError::Output)
        }
        Command::DiagnoseGpu => {
            let report = LinuxGpuProbe::system()
                .inspect()
                .map_err(CliError::GpuProbe)?;
            output::write_gpu_report(&report, stdout).map_err(CliError::Output)
        }
        Command::DiagnoseDisplay => {
            let report = LinuxDisplayProbe::system()
                .inspect()
                .map_err(CliError::DisplayProbe)?;
            output::write_display_report(&report, stdout).map_err(CliError::Output)
        }
        Command::DiagnoseRuntimes { family, roots } => {
            if !cfg!(target_os = "linux") {
                return Err(CliError::RuntimeDiscoveryUnsupported);
            }
            if roots.len() > endfield_unix_runtime_linux::MAX_CUSTOM_ROOTS
                || roots.iter().any(|path| !path.is_absolute())
            {
                return Err(CliError::RuntimeDiscovery(
                    if roots.len() > endfield_unix_runtime_linux::MAX_CUSTOM_ROOTS {
                        endfield_unix_runtime_linux::DiscoveryConfigError::TooManyCustomRoots
                    } else {
                        endfield_unix_runtime_linux::DiscoveryConfigError::InvalidCustomRoot
                    },
                ));
            }
            let provider = endfield_unix_runtime_linux::LinuxRuntimeProvider::new(
                endfield_unix_runtime_linux::DiscoveryOptions {
                    additional_roots: roots,
                    include_path: true,
                    include_conventional: true,
                },
            )
            .map_err(CliError::RuntimeDiscovery)?;
            let report = provider.scan();
            if report.all_sources_failed() {
                return Err(CliError::RuntimeDiscoveryFailed);
            }
            writeln!(stdout, "Runtime discovery: Linux").map_err(CliError::Output)?;
            let runtimes: Vec<_> = report
                .runtimes
                .iter()
                .filter(|runtime| family.is_none_or(|family| runtime.candidate.family() == family))
                .collect();
            for (index, runtime) in runtimes.iter().enumerate() {
                writeln!(stdout, "\nRuntime {}:", index + 1).map_err(CliError::Output)?;
                writeln!(stdout, "  Family: {}", runtime.candidate.family().as_str())
                    .map_err(CliError::Output)?;
                let lineage = match &runtime.lineage {
                    endfield_unix_runtime::RuntimeLineage::UpstreamWine => "Upstream Wine",
                    endfield_unix_runtime::RuntimeLineage::WineStaging => "Wine Staging",
                    endfield_unix_runtime::RuntimeLineage::ValveProton => "Valve Proton",
                    endfield_unix_runtime::RuntimeLineage::GeProton => "GE-Proton",
                    endfield_unix_runtime::RuntimeLineage::DwProton => "DWProton",
                    endfield_unix_runtime::RuntimeLineage::OtherVerifiedDerivative(_) => {
                        "Other derivative"
                    }
                    endfield_unix_runtime::RuntimeLineage::Unknown => "unknown",
                };
                if runtime.lineage == endfield_unix_runtime::RuntimeLineage::Unknown {
                    writeln!(stdout, "  Lineage: {lineage}").map_err(CliError::Output)?;
                } else {
                    writeln!(
                        stdout,
                        "  Lineage: {lineage} (claimed by {})",
                        runtime.lineage_basis
                    )
                    .map_err(CliError::Output)?;
                }
                writeln!(
                    stdout,
                    "  Version: {}",
                    runtime
                        .version
                        .as_ref()
                        .map_or("unknown", |version| version.as_str())
                )
                .map_err(CliError::Output)?;
                let architectures = runtime
                    .architectures
                    .iter()
                    .map(|architecture| architecture.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(
                    stdout,
                    "  Runtime architecture: {}",
                    if architectures.is_empty() {
                        "unknown"
                    } else {
                        &architectures
                    }
                )
                .map_err(CliError::Output)?;
                let sources = runtime
                    .sources
                    .iter()
                    .map(|source| source.label)
                    .collect::<Vec<_>>()
                    .join("; ");
                writeln!(
                    stdout,
                    "  Source: {}",
                    if sources.is_empty() {
                        "unknown source"
                    } else {
                        &sources
                    }
                )
                .map_err(CliError::Output)?;
                writeln!(
                    stdout,
                    "  Ownership: {}",
                    runtime.candidate.ownership().as_str()
                )
                .map_err(CliError::Output)?;
                writeln!(stdout, "  Structure: {}", runtime.structure).map_err(CliError::Output)?;
                writeln!(stdout, "  Local label: {}", runtime.display_label)
                    .map_err(CliError::Output)?;
                writeln!(stdout, "  Source authenticity: unverified\n  Artifact integrity: unverified\n  Compatibility: not evaluated\n  Launch readiness: unverified").map_err(CliError::Output)?;
            }
            writeln!(stdout, "\nDiscovered runtimes: {}", runtimes.len())
                .map_err(CliError::Output)?;
            let result = match report.contract.completeness() {
                endfield_unix_runtime::DiscoveryCompleteness::Complete => "complete",
                endfield_unix_runtime::DiscoveryCompleteness::Partial { .. } => "partial",
            };
            writeln!(stdout, "Discovery result: {result}").map_err(CliError::Output)?;
            if !report.issues.is_empty() {
                writeln!(
                    stdout,
                    "Sources with safe discovery issues: {}",
                    report.issues.len()
                )
                .map_err(CliError::Output)?;
                for issue in &report.issues {
                    writeln!(stdout, "  {}: {}", issue.source.label(), issue.reason)
                        .map_err(CliError::Output)?;
                }
            }
            Ok(())
        }
        Command::RuntimeInstall(operation) => {
            let code = super::runtime_install::run(*operation, stdout).map_err(CliError::Output)?;
            if code == 0 {
                Ok(())
            } else {
                Err(CliError::RuntimeVerification(code))
            }
        }
        Command::DiagnoseRuntimeIntegrity(options) => {
            let code = super::runtime_integrity::run(*options, stdout).map_err(CliError::Output)?;
            if code == 0 {
                Ok(())
            } else {
                Err(CliError::RuntimeVerification(code))
            }
        }
        Command::DiagnoseUmu => {
            if !cfg!(target_os = "linux") {
                return Err(CliError::RuntimeDiscoveryUnsupported);
            }
            let umu = endfield_unix_runtime_linux::UmuProvider::default().scan();
            let runtimes = endfield_unix_runtime_linux::LinuxRuntimeProvider::new(
                endfield_unix_runtime_linux::DiscoveryOptions::default(),
            )
            .map_err(CliError::RuntimeDiscovery)?
            .scan();
            let proton_count = runtimes
                .runtimes
                .iter()
                .filter(|runtime| {
                    runtime.candidate.family() == endfield_unix_runtime::RuntimeFamily::Proton
                })
                .count();
            let validated = umu.discovery.validated().next().is_some();
            writeln!(stdout, "UMU integration: Linux").map_err(CliError::Output)?;
            let executable_status = if validated {
                "candidate found"
            } else if umu.discovery.outcome()
                == endfield_unix_runtime_linux::RootOutcome::NotPresent
            {
                "not found"
            } else if umu.discovery.outcome() == endfield_unix_runtime_linux::RootOutcome::Truncated
            {
                "discovery truncated before validation completed"
            } else if umu.discovery.outcome() == endfield_unix_runtime_linux::RootOutcome::Failed {
                "candidate inspection failed"
            } else {
                "candidate validation incomplete"
            };
            writeln!(stdout, "UMU executable: {executable_status}").map_err(CliError::Output)?;
            writeln!(stdout, "UMU version: unknown").map_err(CliError::Output)?;
            writeln!(
                stdout,
                "UMU structural validation: {}",
                if validated {
                    "validated; execution readiness unverified"
                } else {
                    "unavailable"
                }
            )
            .map_err(CliError::Output)?;
            writeln!(
                stdout,
                "  Availability: {}",
                umu.discovery.availability().as_str()
            )
            .map_err(CliError::Output)?;
            writeln!(
                stdout,
                "  Discovery result: {}",
                umu.discovery.outcome_label()
            )
            .map_err(CliError::Output)?;
            writeln!(stdout, "\nProton discovery:").map_err(CliError::Output)?;
            writeln!(stdout, "  Candidates: {proton_count}").map_err(CliError::Output)?;
            writeln!(stdout, "  Selected runtime: none (identity not evaluated)")
                .map_err(CliError::Output)?;
            writeln!(stdout, "  Source verification: not performed").map_err(CliError::Output)?;
            writeln!(stdout, "  Compatibility: not evaluated").map_err(CliError::Output)?;
            writeln!(stdout, "\nAdapter:").map_err(CliError::Output)?;
            writeln!(
                stdout,
                "  UMU-Proton: {}",
                if validated && proton_count > 0 {
                    "candidate mapping possible; explicit selection required"
                } else {
                    "unavailable"
                }
            )
            .map_err(CliError::Output)?;
            writeln!(
                stdout,
                "  Launch preparation: requires UMU, Proton, prefix, and game selection"
            )
            .map_err(CliError::Output)?;
            writeln!(stdout, "\nNetwork access: not performed\nRuntime execution: not performed\nGame execution: not performed\nPrefix mutation: not performed\n\nResult: partial").map_err(CliError::Output)
        }
        Command::DiagnoseTelemetryGpu { interval_ms } => run_gpu_telemetry(interval_ms, stdout),
        Command::DiagnoseTelemetryCpuMemory { interval_ms } => {
            run_cpu_memory_telemetry(interval_ms, stdout)
        }
        Command::DiagnoseTelemetryFrames { format, input } => {
            run_frame_telemetry(format, &input, stdout)
        }
        Command::ServicePing => {
            let client =
                endfield_unix_ipc::Client::from_environment(std::time::Duration::from_secs(2))
                    .map_err(CliError::Ipc)?;
            client.ping().map_err(CliError::Ipc)?;
            writeln!(stdout, "Pong from endfield-unixd.").map_err(CliError::Output)
        }
        Command::ServiceStatus => {
            let client =
                endfield_unix_ipc::Client::from_environment(std::time::Duration::from_secs(2))
                    .map_err(CliError::Ipc)?;
            let status = client.status().map_err(CliError::Ipc)?;
            writeln!(stdout, "Daemon: {}", lifecycle_name(status.lifecycle))
                .map_err(CliError::Output)?;
            writeln!(stdout, "IPC: connected").map_err(CliError::Output)?;
            writeln!(
                stdout,
                "Protocol: {}.{}",
                endfield_unix_ipc::SUPPORTED_PROTOCOL.major,
                endfield_unix_ipc::SUPPORTED_PROTOCOL.minor
            )
            .map_err(CliError::Output)?;
            writeln!(stdout, "Service: {}", status.service).map_err(CliError::Output)?;
            writeln!(stdout, "Daemon version: {}", status.daemon_package_version)
                .map_err(CliError::Output)?;
            writeln!(
                stdout,
                "Application version: {}",
                status.application_package_version
            )
            .map_err(CliError::Output)?;
            let implemented = status
                .implemented_capabilities
                .iter()
                .map(|capability| match capability {
                    endfield_unix_ipc::ImplementedCapability::LocalIpc => "local_ipc",
                    endfield_unix_ipc::ImplementedCapability::Ping => "ping",
                    endfield_unix_ipc::ImplementedCapability::Status => "status",
                })
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(stdout, "Implemented capabilities: {implemented}")
                .map_err(CliError::Output)?;
            let unimplemented = status
                .unimplemented_capabilities
                .iter()
                .map(|capability| match capability {
                    endfield_unix_ipc::UnimplementedCapability::Telemetry => "telemetry",
                    endfield_unix_ipc::UnimplementedCapability::GameSessionSupervision => {
                        "game_session_supervision"
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(stdout, "Not implemented: {unimplemented}").map_err(CliError::Output)?;
            Ok(())
        }
        Command::SystemdInstall { daemon, replace } => {
            let manager =
                super::systemd::SystemdUserManager::current().map_err(CliError::Systemd)?;
            let result = manager
                .install(daemon.as_deref(), replace)
                .map_err(CliError::Systemd)?;
            writeln!(stdout, "Unit: {}", manager.unit_path().display())
                .map_err(CliError::Output)?;
            writeln!(stdout, "Daemon: {}", manager.daemon_path().display())
                .map_err(CliError::Output)?;
            writeln!(stdout, "systemd user manager: {}", result.manager)
                .map_err(CliError::Output)?;
            writeln!(
                stdout,
                "Manager reload: {}",
                if result.manager_reloaded {
                    "completed"
                } else {
                    "unavailable; run `systemctl --user daemon-reload` from a user session"
                }
            )
            .map_err(CliError::Output)?;
            writeln!(
                stdout,
                "Autostart: disabled; install does not enable or start the service"
            )
            .map_err(CliError::Output)
        }
        Command::SystemdUninstall => {
            let manager =
                super::systemd::SystemdUserManager::current().map_err(CliError::Systemd)?;
            let result = manager.uninstall().map_err(CliError::Systemd)?;
            writeln!(stdout, "Unit: removed").map_err(CliError::Output)?;
            writeln!(
                stdout,
                "Daemon executable: {}",
                if result.daemon_removed {
                    "removed (verified installer-owned file)"
                } else {
                    "retained (not verified as installer-owned)"
                }
            )
            .map_err(CliError::Output)?;
            writeln!(
                stdout,
                "Autostart: disabled; uninstall did not stop an active daemon"
            )
            .map_err(CliError::Output)
        }
        Command::SystemdEnable => run_systemd_operation(
            super::systemd::ServiceOperation::Enable,
            "Autostart enabled; the service was not started.",
            stdout,
        ),
        Command::SystemdDisable => run_systemd_operation(
            super::systemd::ServiceOperation::Disable,
            "Autostart disabled; an active service was not stopped.",
            stdout,
        ),
        Command::SystemdStart => run_systemd_operation(
            super::systemd::ServiceOperation::Start,
            "systemd start completed after the Type=notify readiness handshake.",
            stdout,
        )
        .and_then(|()| report_ipc_health(stdout, "start")),
        Command::SystemdStop => run_systemd_operation(
            super::systemd::ServiceOperation::Stop,
            "systemd stop completed after graceful SIGTERM shutdown.",
            stdout,
        )
        .and_then(|()| report_ipc_after_stop(stdout)),
        Command::SystemdRestart => run_systemd_operation(
            super::systemd::ServiceOperation::Restart,
            "systemd restart completed after the Type=notify readiness handshake.",
            stdout,
        )
        .and_then(|()| report_ipc_health(stdout, "restart")),
        Command::SystemdStatus => {
            let manager =
                super::systemd::SystemdUserManager::current().map_err(CliError::Systemd)?;
            let status = manager.status();
            let ipc_healthy = ipc_is_healthy();
            output::write_systemd_status(&status, ipc_healthy, stdout).map_err(CliError::Output)
        }
    }
}

fn run_systemd_operation<W: Write>(
    operation: super::systemd::ServiceOperation,
    message: &str,
    stdout: &mut W,
) -> Result<(), CliError> {
    let manager = super::systemd::SystemdUserManager::current().map_err(CliError::Systemd)?;
    manager.operate(operation).map_err(CliError::Systemd)?;
    writeln!(stdout, "{message}").map_err(CliError::Output)
}

fn ipc_is_healthy() -> bool {
    endfield_unix_ipc::Client::from_environment(std::time::Duration::from_secs(2))
        .and_then(|client| client.ping())
        .is_ok()
}

fn report_ipc_health<W: Write>(stdout: &mut W, action: &str) -> Result<(), CliError> {
    if ipc_is_healthy() {
        writeln!(stdout, "Authenticated IPC Ping: healthy.").map_err(CliError::Output)
    } else {
        writeln!(stdout, "Authenticated IPC Ping: unavailable after systemd {action}; unit state does not prove IPC health. Inspect the user journal for diagnostics.")
            .map_err(CliError::Output)
    }
}

fn report_ipc_after_stop<W: Write>(stdout: &mut W) -> Result<(), CliError> {
    if ipc_is_healthy() {
        writeln!(stdout, "IPC remains available. A separately started daemon may own it; it was not adopted or stopped.")
            .map_err(CliError::Output)
    } else {
        writeln!(stdout, "Authenticated IPC Ping: unavailable after stop.")
            .map_err(CliError::Output)
    }
}

fn run_cpu_memory_telemetry<O: Write>(interval_ms: u64, stdout: &mut O) -> Result<(), CliError> {
    let interval =
        TelemetryInterval::from_millis(interval_ms).map_err(CliError::TelemetryConfig)?;
    let snapshot_reference = SnapshotRefId::new(1).expect("nonzero snapshot reference");
    let cpu_snapshot = LinuxHardwareProbe::system()
        .inspect()
        .ok()
        .and_then(|report| {
            LinuxCpuSnapshotContext::from_hardware_report(&report, snapshot_reference).ok()
        });
    let expected_logical_cpus = cpu_snapshot
        .as_ref()
        .map(|context| context.online_cpu_ids().len());
    let mut provider = LinuxCpuMemoryTelemetryProvider::new(SystemLinuxDataSource::new(), interval);
    if let Some(context) = cpu_snapshot {
        provider = provider.with_cpu_snapshot_context(context);
    }
    let request = CollectionRequest::new(
        SamplingContextId::new(1).expect("nonzero sampling context"),
        vec![
            MetricId::CpuUtilizationWholeMachine,
            MetricId::CpuUtilizationLogicalCpu,
            MetricId::CpuFrequency,
            MetricId::RamUsed,
            MetricId::RamAvailable,
            MetricId::SwapUsed,
            MetricId::CgroupMemoryUsed,
        ],
        None,
    )
    .map_err(CliError::TelemetryContract)?;
    let report = provider
        .collect(&request)
        .map_err(CliError::TelemetryProvider)?;
    output::write_telemetry_report(&report, expected_logical_cpus, stdout).map_err(CliError::Output)
}

fn run_frame_telemetry<O: Write>(
    format: FrameSourceFormat,
    input: &Path,
    stdout: &mut O,
) -> Result<(), CliError> {
    let recording = read_frame_recording(input, format).map_err(CliError::FrameInput)?;
    let sampling_context = SamplingContextId::new(1).expect("nonzero import-local context");
    let clock_domain =
        endfield_unix_telemetry::ClockDomainId::new(1).expect("nonzero import-local clock domain");
    let provider = FrameLogTelemetryProvider::new(recording, clock_domain);
    let request = CollectionRequest::new(
        sampling_context,
        vec![
            MetricId::FramesPerSecond,
            MetricId::FrameTime,
            MetricId::MeanFrameTime,
            MetricId::FrameCount,
            MetricId::P99FrameTime,
        ],
        None,
    )
    .map_err(CliError::TelemetryContract)?;
    let report = provider
        .collect_report(&request)
        .map_err(CliError::TelemetryProvider)?;
    output::write_frame_telemetry_report(&report, stdout).map_err(CliError::Output)
}

fn lifecycle_name(state: endfield_unix_ipc::DaemonLifecycle) -> &'static str {
    match state {
        endfield_unix_ipc::DaemonLifecycle::Starting => "starting",
        endfield_unix_ipc::DaemonLifecycle::Running => "running",
        endfield_unix_ipc::DaemonLifecycle::Stopping => "stopping",
        endfield_unix_ipc::DaemonLifecycle::Stopped => "stopped",
        endfield_unix_ipc::DaemonLifecycle::Failed => "failed",
    }
}

fn write_report_success<W: Write>(output: &mut W, path: &Path) -> io::Result<()> {
    writeln!(output, "Support report written to {path:?}.")
}

fn report_error<E>(stderr: &mut E, message: &str, exit_status: u8) -> u8
where
    E: Write,
{
    match writeln!(stderr, "{APPLICATION_NAME}: {message}") {
        Ok(()) => exit_status,
        Err(_) => EXIT_IO_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::io;

    use super::*;

    #[test]
    fn default_and_explicit_gui_commands_share_the_injected_startup_path() {
        for arguments in [Vec::new(), vec![OsString::from("gui")]] {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut gui_started = false;

            let status = run_with_io_and_gui(arguments, &mut stdout, &mut stderr, || {
                gui_started = true;
                Ok(())
            });

            assert_eq!(status, EXIT_SUCCESS);
            assert!(gui_started);
            assert!(stdout.is_empty());
            assert!(stderr.is_empty());
        }
    }

    #[test]
    fn gui_startup_error_uses_the_existing_exit_status_and_diagnostic() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let status = run_with_io_and_gui(Vec::<OsString>::new(), &mut stdout, &mut stderr, || {
            Err(GuiStartupError::new(
                "could not initialize GTK display: no display",
            ))
        });

        assert_eq!(status, EXIT_GUI_STARTUP_ERROR);
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr),
            Ok(format!(
                "{APPLICATION_NAME}: could not initialize GTK display: no display\n"
            ))
        );
    }

    #[test]
    fn output_broken_pipe_returns_io_status_without_panicking() {
        let mut stdout = BrokenPipeWriter;
        let mut stderr = Vec::new();

        let status = run_with_io([OsString::from("--version")], &mut stdout, &mut stderr);

        assert_eq!(status, EXIT_IO_ERROR);
        assert_eq!(
            String::from_utf8(stderr),
            Ok(format!(
                "{APPLICATION_NAME}: failed to write command output.\n"
            ))
        );
    }

    #[test]
    fn stderr_write_failure_maps_to_io_status_without_panicking() {
        let mut stdout = Vec::new();
        let mut stderr = BrokenPipeWriter;

        let status = run_with_io([OsString::from("--invalid")], &mut stdout, &mut stderr);

        assert_eq!(status, EXIT_IO_ERROR);
        assert!(stdout.is_empty());
    }

    #[test]
    fn platform_command_renders_an_injected_partial_report_without_gui_or_daemon() {
        use endfield_unix_domain::{
            Architecture, DistributionIdentity, KernelIdentity, OsIdentity, PlatformFamily,
            PlatformIdentity, SystemSnapshot,
        };
        use endfield_unix_platform_linux::{
            EnvironmentClassification, EnvironmentConfidence, EnvironmentStatus,
            LINUX_PLATFORM_REPORT_SCHEMA_VERSION, LinuxDistributionMetadata, LinuxPlatformReport,
            LinuxReportCompleteness, RuntimeDirectoryAssessment, RuntimeDirectoryStatus,
            RuntimeEnvironmentFacts,
        };

        let platform = PlatformIdentity::new(
            PlatformFamily::Linux,
            OsIdentity::new("Linux", None).expect("OS identity"),
            Architecture::Aarch64,
            Some(KernelIdentity::new("Linux", Some("6.18.3-arch1-1".to_owned())).expect("kernel")),
            Some(
                DistributionIdentity::new("arch", None, Some("Arch Linux".to_owned()))
                    .expect("distribution"),
            ),
        )
        .expect("platform");
        let report = LinuxPlatformReport {
            schema_version: LINUX_PLATFORM_REPORT_SCHEMA_VERSION,
            snapshot: SystemSnapshot::new(platform),
            distribution: Some(LinuxDistributionMetadata {
                id: "arch".to_owned(),
                name: Some("Arch Linux".to_owned()),
                pretty_name: Some("Arch Linux".to_owned()),
                version_id: None,
                version: None,
                id_like: Vec::new(),
                variant_id: None,
                source: "etc-os-release",
            }),
            runtime_environment: RuntimeEnvironmentFacts {
                classification: EnvironmentClassification::Wsl,
                status: EnvironmentStatus::Identified,
                confidence: EnvironmentConfidence::High,
                source_ids: vec!["linux-uname"],
                diagnostic: None,
                uncertainty: Some("WSL generation is not inferred"),
            },
            user_runtime_directory: RuntimeDirectoryAssessment {
                status: RuntimeDirectoryStatus::Missing,
                source_id: "linux-xdg-runtime-dir",
                diagnostic: Some("XDG_RUNTIME_DIR is not set"),
            },
            completeness: LinuxReportCompleteness::Partial,
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut gui_started = false;
        let report_for_probe = report.clone();
        let status = run_with_io_and_gui_and_platform(
            [OsString::from("diagnose"), OsString::from("platform")],
            &mut stdout,
            &mut stderr,
            || {
                gui_started = true;
                Ok(())
            },
            move || Ok(report_for_probe),
        );

        assert_eq!(status, EXIT_SUCCESS);
        assert!(!gui_started);
        assert!(stderr.is_empty());
        let output = String::from_utf8(stdout).expect("UTF-8 output");
        assert!(output.contains("Distribution: Arch Linux (id: arch)"));
        assert!(output.contains("Kernel: Linux 6.18.3-arch1-1"));
        assert!(output.contains("Architecture: aarch64 (kernel-reported)"));
        assert!(output.contains("Environment: WSL (high confidence"));
        assert!(output.contains("User runtime directory: missing"));
        assert!(output.contains("Probe result: partial"));
    }

    #[test]
    fn platform_probe_errors_have_distinct_unsupported_and_fatal_statuses() {
        for (kind, expected_status) in [
            (ProbeErrorKind::Unsupported, EXIT_PLATFORM_UNSUPPORTED),
            (ProbeErrorKind::TemporaryFailure, EXIT_PLATFORM_PROBE_FAILED),
        ] {
            let error = ProbeError::new("linux-uname", kind, Some("safe failure".to_owned()))
                .expect("valid probe error");
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let status = run_with_io_and_gui_and_platform(
                [OsString::from("diagnose"), OsString::from("platform")],
                &mut stdout,
                &mut stderr,
                || Ok(()),
                move || Err(error),
            );
            assert_eq!(status, expected_status);
            assert!(stdout.is_empty());
            assert!(!stderr.is_empty());
        }
    }

    struct BrokenPipeWriter;

    impl Write for BrokenPipeWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))
        }
    }
}
