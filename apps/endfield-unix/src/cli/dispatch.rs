use std::error::Error;
use std::fmt;
use std::io::{self, Write};
use std::path::Path;

use super::commands::Command;
use super::{output, parser};
use crate::{APPLICATION_NAME, GuiStartupError, start_gui};

pub const EXIT_SUCCESS: u8 = 0;
pub const EXIT_INVALID_ARGUMENTS: u8 = 2;
pub const EXIT_GUI_STARTUP_ERROR: u8 = 69;
pub const EXIT_IO_ERROR: u8 = 74;
pub const EXIT_IPC_UNAVAILABLE: u8 = 75;
pub const EXIT_IPC_PROTOCOL_ERROR: u8 = 76;
pub const EXIT_IPC_AUTHENTICATION_ERROR: u8 = 77;
pub const EXIT_IPC_REQUEST_REJECTED: u8 = 65;

#[derive(Debug)]
pub enum CliError {
    InvalidArguments(clap::Error),
    Output(io::Error),
    GuiStartup(GuiStartupError),
    Diagnostics(endfield_unix_diagnostics::DiagnosticsError),
    Ipc(endfield_unix_ipc::ClientError),
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
    run_with_io_and_gui(arguments, stdout, stderr, start_gui)
}

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
    let command = match parser::parse(arguments).map_err(CliError::InvalidArguments) {
        Ok(command) => command,
        Err(error) => return report_error(stderr, &error.to_string(), error.exit_status()),
    };

    let result = execute(command, stdout, gui_start);
    match result {
        Ok(()) => EXIT_SUCCESS,
        Err(error) => report_error(stderr, &error.to_string(), error.exit_status()),
    }
}

fn execute<O, G>(command: Command, stdout: &mut O, gui_start: G) -> Result<(), CliError>
where
    O: Write,
    G: FnOnce() -> Result<(), GuiStartupError>,
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
