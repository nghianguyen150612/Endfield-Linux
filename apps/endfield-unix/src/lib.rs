use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Write};

pub mod ui;

pub const APPLICATION_NAME: &str = "endfield-unix";
pub const APPLICATION_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APPLICATION_ID: &str = "io.github.nghianguyen150612.endfieldunix";

pub const EXIT_SUCCESS: u8 = 0;
pub const EXIT_INVALID_ARGUMENTS: u8 = 2;
pub const EXIT_GUI_STARTUP_ERROR: u8 = 69;
pub const EXIT_IO_ERROR: u8 = 74;

const HELP_TEXT: &str = concat!(
    "endfield-unix: Native launcher and optimization application for Arknights: Endfield on Unix-like systems.\n",
    "The native desktop shell is available; game and optimization services are under development.\n\n",
    "Usage: endfield-unix [--help | --version]\n\n",
    "Options:\n",
    "  --help       Show this help message\n",
    "  --version    Show the application version\n\n",
    "Running without options starts the GTK4 desktop application.\n",
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    LaunchGui,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DispatchError {
    argument: OsString,
}

impl fmt::Display for DispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unrecognized argument {:?}", self.argument)
    }
}

impl Error for DispatchError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiStartupError(String);

impl GuiStartupError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for GuiStartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for GuiStartupError {}

/// Converts command-line arguments, excluding the executable name, into an action.
///
/// Arguments are compared as OS strings so non-UTF-8 input is not rejected or
/// lossy-converted during dispatch.
pub fn dispatch<I>(arguments: I) -> Result<Command, DispatchError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut arguments = arguments.into_iter();
    let Some(argument) = arguments.next() else {
        return Ok(Command::LaunchGui);
    };

    let command = if argument == OsStr::new("--help") {
        Command::Help
    } else if argument == OsStr::new("--version") {
        Command::Version
    } else {
        return Err(DispatchError { argument });
    };

    if let Some(argument) = arguments.next() {
        return Err(DispatchError { argument });
    }

    Ok(command)
}

/// Starts the GTK application on the calling thread.
pub fn start_gui() -> Result<(), GuiStartupError> {
    ui::run().map_err(|error| GuiStartupError::new(error.to_string()))
}

/// Starts the shared application entry point using the process standard streams.
pub fn run<I>(arguments: I) -> u8
where
    I: IntoIterator<Item = OsString>,
{
    let stdout = io::stdout();
    let stderr = io::stderr();
    let mut stdout = stdout.lock();
    let mut stderr = stderr.lock();

    run_with_io(arguments, &mut stdout, &mut stderr)
}

/// Runs dispatch with explicit writers so command behavior can be tested without a terminal.
pub fn run_with_io<I, O, E>(arguments: I, stdout: &mut O, stderr: &mut E) -> u8
where
    I: IntoIterator<Item = OsString>,
    O: Write,
    E: Write,
{
    run_with_io_and_gui(arguments, stdout, stderr, start_gui)
}

fn run_with_io_and_gui<I, O, E, G>(arguments: I, stdout: &mut O, stderr: &mut E, gui_start: G) -> u8
where
    I: IntoIterator<Item = OsString>,
    O: Write,
    E: Write,
    G: FnOnce() -> Result<(), GuiStartupError>,
{
    let command = match dispatch(arguments) {
        Ok(command) => command,
        Err(error) => {
            return report_error(stderr, &error.to_string(), EXIT_INVALID_ARGUMENTS);
        }
    };

    match command {
        Command::Help => match stdout.write_all(HELP_TEXT.as_bytes()) {
            Ok(()) => EXIT_SUCCESS,
            Err(_) => report_output_error(stderr),
        },
        Command::Version => match writeln!(stdout, "{APPLICATION_NAME} {APPLICATION_VERSION}") {
            Ok(()) => EXIT_SUCCESS,
            Err(_) => report_output_error(stderr),
        },
        Command::LaunchGui => match gui_start() {
            Ok(()) => EXIT_SUCCESS,
            Err(error) => report_error(stderr, &error.to_string(), EXIT_GUI_STARTUP_ERROR),
        },
    }
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

fn report_output_error<E>(stderr: &mut E) -> u8
where
    E: Write,
{
    let _ = writeln!(
        stderr,
        "{APPLICATION_NAME}: failed to write command output."
    );
    EXIT_IO_ERROR
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn help_dispatches_successfully() {
        assert_eq!(dispatch(args(&["--help"])), Ok(Command::Help));
    }

    #[test]
    fn version_dispatches_successfully() {
        assert_eq!(dispatch(args(&["--version"])), Ok(Command::Version));
    }

    #[test]
    fn no_arguments_selects_gui_boundary() {
        assert_eq!(dispatch(Vec::<OsString>::new()), Ok(Command::LaunchGui));
    }

    #[test]
    fn application_id_meets_glib_requirements() {
        assert!(gtk::gio::Application::id_is_valid(APPLICATION_ID));
    }

    #[test]
    fn no_arguments_dispatches_to_a_successful_gui_startup() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit_status =
            run_with_io_and_gui(Vec::<OsString>::new(), &mut stdout, &mut stderr, || Ok(()));

        assert_eq!(exit_status, EXIT_SUCCESS);
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
    }

    #[test]
    fn gui_startup_errors_have_a_nonzero_status_and_clear_diagnostic() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit_status =
            run_with_io_and_gui(Vec::<OsString>::new(), &mut stdout, &mut stderr, || {
                Err(GuiStartupError::new(
                    "could not initialize GTK display: no display",
                ))
            });

        assert_eq!(exit_status, EXIT_GUI_STARTUP_ERROR);
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr),
            Ok(format!(
                "{APPLICATION_NAME}: could not initialize GTK display: no display\n"
            ))
        );
    }

    #[test]
    fn invalid_arguments_report_error_and_nonzero_status() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit_status = run_with_io(args(&["--unknown"]), &mut stdout, &mut stderr);

        assert_eq!(exit_status, EXIT_INVALID_ARGUMENTS);
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr),
            Ok(format!(
                "{APPLICATION_NAME}: unrecognized argument \"--unknown\"\n"
            ))
        );
    }

    #[test]
    fn dispatch_is_deterministic() {
        let arguments = args(&["--version"]);
        let first = dispatch(arguments.clone());
        let second = dispatch(arguments);

        assert_eq!(first, second);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_argument_is_reported_without_failing_dispatch() {
        use std::os::unix::ffi::OsStringExt;

        let argument = OsString::from_vec(vec![0xff]);
        let result = dispatch([argument]);

        assert!(
            matches!(result, Err(error) if error.to_string().starts_with("unrecognized argument "))
        );
    }
}
