use std::ffi::{OsStr, OsString};
use std::io::{self, Write};

use crate::runtime::run_foreground;

pub const SUCCESS: i32 = 0;
pub const INVALID_ARGUMENTS: i32 = 2;
pub const OUTPUT_IO_FAILURE: i32 = 74;
pub const DAEMON_FAILURE: i32 = 70;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const HELP: &str = "endfield-unixd — foreground lifecycle daemon for endfield-unix

Usage:
  endfield-unixd [--foreground]
  endfield-unixd --help
  endfield-unixd --version

The daemon provides an independently running lifecycle process. It can be
stopped with SIGTERM or Ctrl+C (SIGINT). Running without options is the same
as --foreground; the process stays attached to its supervisor and terminal.

The daemon provides authenticated, read-only local IPC for ping and status.
Telemetry and game management are not implemented. The endfield-unix CLI can
explicitly install and manage an optional systemd user unit; installation does
not enable autostart or launch the daemon.
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Foreground,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CliError;

pub fn parse<I, S>(arguments: I) -> Result<Command, CliError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let arguments = arguments
        .into_iter()
        .map(|argument| argument.as_ref().to_os_string())
        .collect::<Vec<OsString>>();

    match arguments.as_slice() {
        [] => Ok(Command::Foreground),
        [argument] if argument == OsStr::new("--help") || argument == OsStr::new("-h") => {
            Ok(Command::Help)
        }
        [argument] if argument == OsStr::new("--version") => Ok(Command::Version),
        [argument] if argument == OsStr::new("--foreground") => Ok(Command::Foreground),
        _ => Err(CliError),
    }
}

pub fn run<I, S>(arguments: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    match parse(arguments) {
        Ok(Command::Help) => write_stdout(HELP.as_bytes()),
        Ok(Command::Version) => write_stdout(format!("endfield-unixd {VERSION}\n").as_bytes()),
        Ok(Command::Foreground) => match run_foreground() {
            Ok(()) => SUCCESS,
            Err(error) => {
                let _ = writeln!(io::stderr().lock(), "endfield-unixd: {error}");
                DAEMON_FAILURE
            }
        },
        Err(CliError) => {
            if writeln!(
                io::stderr().lock(),
                "endfield-unixd: invalid arguments; use --help"
            )
            .is_ok()
            {
                INVALID_ARGUMENTS
            } else {
                OUTPUT_IO_FAILURE
            }
        }
    }
}

fn write_stdout(bytes: &[u8]) -> i32 {
    write_stdout_to(&mut io::stdout().lock(), bytes)
}

fn write_stdout_to(writer: &mut impl Write, bytes: &[u8]) -> i32 {
    match writer.write_all(bytes) {
        Ok(()) => SUCCESS,
        Err(_) => OUTPUT_IO_FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ClosedOutput;

    impl Write for ClosedOutput {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn failed_static_output_uses_status_74() {
        assert_eq!(
            write_stdout_to(&mut ClosedOutput, b"help\n"),
            OUTPUT_IO_FAILURE
        );
    }
}
