use std::ffi::OsString;
use std::iter;
use std::path::PathBuf;

use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{ArgAction, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};

use super::commands::{Command, HelpTopic};
use crate::APPLICATION_NAME;

#[derive(Debug, Parser)]
#[command(
    disable_help_flag = true,
    disable_help_subcommand = true,
    disable_version_flag = true
)]
struct Arguments {
    #[arg(
        short = 'h',
        long = "help",
        action = ArgAction::SetTrue,
        conflicts_with = "version"
    )]
    help: bool,

    #[arg(
        long = "version",
        action = ArgAction::SetTrue,
        conflicts_with = "help"
    )]
    version: bool,

    #[command(subcommand)]
    command: Option<SubcommandArgs>,
}

#[derive(Debug, Subcommand)]
enum SubcommandArgs {
    /// Launch the native GTK4 desktop application.
    Gui,
    /// Show command help.
    Help {
        /// Command to describe.
        #[arg(value_enum)]
        topic: Option<HelpTopicArg>,
    },
    /// Show static application and repository information.
    About,
    /// Generate an allowlist-only support report without probing the system.
    Diagnose {
        #[command(subcommand)]
        command: DiagnoseArgs,
    },
    /// Query the authenticated local daemon.
    Service {
        #[command(subcommand)]
        command: ServiceArgs,
    },
}

#[derive(Debug, Subcommand)]
enum DiagnoseArgs {
    /// Write a privacy-safe JSON support report to an explicit path.
    Report {
        /// Destination file for the generated report.
        #[arg(long, value_name = "PATH")]
        output: PathBuf,
        /// Replace an existing regular file at the destination.
        #[arg(long)]
        overwrite: bool,
    },
    /// Inspect local Linux platform identity without starting the daemon.
    Platform,
}

#[derive(Debug, Subcommand)]
enum ServiceArgs {
    /// Complete an authenticated Ping/Pong exchange with the daemon.
    Ping,
    /// Print validated lifecycle and capability status from the daemon.
    Status,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum HelpTopicArg {
    Gui,
    About,
    Service,
    Diagnose,
}

/// Parses operating-system arguments into a typed command without exiting the process.
pub fn parse<I, T>(arguments: I) -> Result<Command, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let arguments = arguments.into_iter().map(Into::into).collect::<Vec<_>>();
    let mut parser = parser_command();
    let parser_arguments =
        iter::once(OsString::from(APPLICATION_NAME)).chain(arguments.iter().cloned());
    let matches = match parser.clone().try_get_matches_from(parser_arguments) {
        Ok(matches) => matches,
        Err(error) => {
            return Err(normalize_unrecognized_argument(error));
        }
    };
    let parsed = Arguments::from_arg_matches(&matches)?;

    if (parsed.help || parsed.version) && parsed.command.is_some() {
        return Err(parser.error(
            clap::error::ErrorKind::ArgumentConflict,
            "--help and --version cannot be combined with a subcommand",
        ));
    }

    if parsed.help {
        return Ok(Command::Help(None));
    }
    if parsed.version {
        return Ok(Command::Version);
    }

    Ok(match parsed.command {
        None => Command::LaunchGui,
        Some(SubcommandArgs::Gui) => Command::LaunchGui,
        Some(SubcommandArgs::Help { topic }) => Command::Help(topic.map(|topic| match topic {
            HelpTopicArg::Gui => HelpTopic::Gui,
            HelpTopicArg::About => HelpTopic::About,
            HelpTopicArg::Service => HelpTopic::Service,
            HelpTopicArg::Diagnose => HelpTopic::Diagnose,
        })),
        Some(SubcommandArgs::About) => Command::About,
        Some(SubcommandArgs::Diagnose {
            command: DiagnoseArgs::Report { output, overwrite },
        }) => Command::DiagnoseReport { output, overwrite },
        Some(SubcommandArgs::Diagnose {
            command: DiagnoseArgs::Platform,
        }) => Command::DiagnosePlatform,
        Some(SubcommandArgs::Service {
            command: ServiceArgs::Ping,
        }) => Command::ServicePing,
        Some(SubcommandArgs::Service {
            command: ServiceArgs::Status,
        }) => Command::ServiceStatus,
    })
}

fn normalize_unrecognized_argument(error: clap::Error) -> clap::Error {
    // Preserve the pre-P005 wording while leaving actual argument validation to Clap.
    let context = match error.kind() {
        ErrorKind::UnknownArgument => Some(ContextKind::InvalidArg),
        ErrorKind::InvalidSubcommand => Some(ContextKind::InvalidSubcommand),
        _ => None,
    };
    if let Some(ContextValue::String(argument)) = context.and_then(|kind| error.get(kind)) {
        let mut command = parser_command();
        return command.error(
            ErrorKind::UnknownArgument,
            format!("unrecognized argument {argument:?}"),
        );
    }

    error
}

fn parser_command() -> clap::Command {
    Arguments::command().name(APPLICATION_NAME).term_width(80)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn assert_command(values: &[&str], expected: Command) {
        match parse(args(values)) {
            Ok(command) => assert_eq!(command, expected),
            Err(error) => panic!("expected a command, got parser error: {error}"),
        }
    }

    #[test]
    fn empty_arguments_select_gui_mode() {
        assert_command(&[], Command::LaunchGui);
    }

    #[test]
    fn gui_command_selects_the_same_gui_mode() {
        assert_command(&["gui"], Command::LaunchGui);
    }

    #[test]
    fn long_help_flag_selects_general_help() {
        assert_command(&["--help"], Command::Help(None));
    }

    #[test]
    fn short_help_flag_selects_general_help() {
        assert_command(&["-h"], Command::Help(None));
    }

    #[test]
    fn help_command_selects_general_help() {
        assert_command(&["help"], Command::Help(None));
    }

    #[test]
    fn help_command_accepts_gui_topic() {
        assert_command(&["help", "gui"], Command::Help(Some(HelpTopic::Gui)));
    }

    #[test]
    fn help_command_accepts_about_topic() {
        assert_command(&["help", "about"], Command::Help(Some(HelpTopic::About)));
    }

    #[test]
    fn help_command_accepts_service_topic() {
        assert_command(
            &["help", "service"],
            Command::Help(Some(HelpTopic::Service)),
        );
    }

    #[test]
    fn service_ping_and_status_are_typed_headless_commands() {
        assert_command(&["service", "ping"], Command::ServicePing);
        assert_command(&["service", "status"], Command::ServiceStatus);
    }

    #[test]
    fn diagnose_platform_is_a_typed_local_command() {
        assert_command(&["diagnose", "platform"], Command::DiagnosePlatform);
        assert_command(
            &["help", "diagnose"],
            Command::Help(Some(HelpTopic::Diagnose)),
        );
    }

    #[test]
    fn version_flag_selects_version_output() {
        assert_command(&["--version"], Command::Version);
    }

    #[test]
    fn about_command_selects_about_output() {
        assert_command(&["about"], Command::About);
    }

    #[test]
    fn parsing_is_deterministic() {
        let arguments = args(&["--version"]);
        let first = parse(arguments.clone()).map_err(|error| error.to_string());
        let second = parse(arguments).map_err(|error| error.to_string());
        assert_eq!(first, second);
    }

    #[test]
    fn unknown_commands_are_rejected_with_the_argument() {
        let error = parse(args(&["unknown"])).expect_err("unknown command should fail");
        assert!(
            error
                .to_string()
                .contains("unrecognized argument \"unknown\"")
        );
    }

    #[test]
    fn help_and_version_flags_cannot_be_combined() {
        assert!(parse(args(&["--help", "--version"])).is_err());
        assert!(parse(args(&["--version", "--help"])).is_err());
    }

    #[test]
    fn flags_cannot_be_combined_with_subcommands() {
        assert!(parse(args(&["--help", "about"])).is_err());
        assert!(parse(args(&["--version", "gui"])).is_err());
    }

    #[test]
    fn extra_arguments_are_rejected() {
        for values in [
            args(&["gui", "extra"]),
            args(&["--help", "extra"]),
            args(&["help", "gui", "extra"]),
        ] {
            assert!(parse(values).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_argument_is_rejected_without_panicking() {
        use std::os::unix::ffi::OsStringExt;

        let error = parse([OsString::from_vec(vec![0xff])])
            .expect_err("non-UTF-8 command should be rejected");
        assert!(!error.to_string().is_empty());
    }
}
