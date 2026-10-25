use std::error::Error;
use std::fmt;
use std::io::{self, Write};
use std::path::Path;

use super::commands::Command;
use super::{output, parser};
use crate::{APPLICATION_NAME, GuiStartupError, start_gui};
use endfield_unix_domain::{ProbeError, ProbeErrorKind};
use endfield_unix_platform_linux::{
    LinuxGpuProbe, LinuxHardwareProbe, LinuxHardwareReport, LinuxPlatformProbe, LinuxPlatformReport,
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
}

impl CliError {
    pub fn exit_status(&self) -> u8 {
        match self {
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
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments(error) => fmt::Display::fmt(error, formatter),
            Self::Output(_) => formatter.write_str("failed to write command output."),
            Self::GuiStartup(error) => error.fmt(formatter),
            Self::Diagnostics(error) => fmt::Display::fmt(error, formatter),
            Self::Ipc(error) => fmt::Display::fmt(error, formatter),
            Self::PlatformProbe(error) => fmt::Display::fmt(error, formatter),
            Self::HardwareProbe(error) => fmt::Display::fmt(error, formatter),
            Self::GpuProbe(error) => fmt::Display::fmt(error, formatter),
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidArguments(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::GuiStartup(error) => Some(error),
            Self::Diagnostics(error) => Some(error),
            Self::Ipc(error) => Some(error),
            Self::PlatformProbe(error) => Some(error),
            Self::HardwareProbe(error) => Some(error),
            Self::GpuProbe(error) => Some(error),
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
    }
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
