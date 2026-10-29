use std::ffi::OsString;
use std::iter;
use std::path::PathBuf;

use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{ArgAction, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};

use super::commands::{Command, HelpTopic};
use crate::APPLICATION_NAME;
use endfield_unix_platform_linux::FrameSourceFormat;

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
    /// Manage explicitly selected runtimes without executing them.
    Runtime {
        #[command(subcommand)]
        command: Box<RuntimeArgs>,
    },
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
enum RuntimeArgs {
    /// Show only records inside the Endfield-Unix managed runtime root.
    ListManaged,
    /// Plan or apply a verified runtime installation.
    Install {
        #[command(subcommand)]
        operation: Box<super::runtime_install::Operation>,
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
    /// Inspect local Linux CPU, memory, cgroup, scheduling, and power interfaces.
    Hardware,
    /// Inspect local Linux PCI, DRM, kernel graphics drivers, and Vulkan devices.
    Gpu,
    /// Inspect local Linux display outputs, session protocols, and DRM connectors.
    Display,
    /// Discover installed Wine and Proton files without executing them.
    Runtimes {
        /// Restrict the output to a runtime family.
        #[arg(long, value_enum)]
        family: Option<RuntimeFamilyArg>,
        /// Add an absolute root for this invocation only; may be repeated.
        #[arg(long = "root", value_name = "PATH", action = ArgAction::Append, value_parser = absolute_path)]
        roots: Vec<PathBuf>,
    },
    /// Inspect installed UMU and local Proton integration without executing them.
    Umu,
    /// Verify an explicit signed manifest and local artifact; user-pinned trust only.
    RuntimeIntegrity(Box<super::runtime_integrity::Options>),

    /// Collect one bounded Linux CPU and memory telemetry sample.
    Telemetry {
        #[command(subcommand)]
        command: TelemetryArgs,
    },
}

#[derive(Debug, Subcommand)]
enum TelemetryArgs {
    /// Collect supported GPU gauges without graphics initialization.
    Gpu {
        #[arg(long, default_value_t = 250, value_parser = clap::value_parser!(u64).range(100..=2000))]
        interval_ms: u64,
    },
    /// Sample CPU counters and collect current memory gauges.
    CpuMemory {
        /// Time between the two bounded CPU counter reads (100–2000 ms).
        #[arg(
            long,
            default_value_t = 250,
            value_parser = clap::value_parser!(u64).range(100..=2000)
        )]
        interval_ms: u64,
    },
    /// Import one explicitly selected frame recording without a daemon or GUI.
    Frames {
        /// Supported source schema. MangoHud must be selected explicitly.
        #[arg(long, value_enum, default_value = "frame-events-v1")]
        format: FrameFormatArg,
        /// User-selected regular file; paths are never discovered automatically.
        #[arg(long, value_name = "PATH")]
        input: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum FrameFormatArg {
    #[value(name = "frame-events-v1")]
    FrameEventsV1,
    #[value(name = "mangohud-csv")]
    MangoHudCsv,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RuntimeFamilyArg {
    Wine,
    Proton,
}

pub(super) fn absolute_path(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err("runtime roots must be absolute paths".into())
    }
}

impl From<FrameFormatArg> for FrameSourceFormat {
    fn from(value: FrameFormatArg) -> Self {
        match value {
            FrameFormatArg::FrameEventsV1 => Self::FrameEventsV1,
            FrameFormatArg::MangoHudCsv => Self::MangoHudCsv,
        }
    }
}

#[derive(Debug, Subcommand)]
enum ServiceArgs {
    /// Complete an authenticated Ping/Pong exchange with the daemon.
    Ping,
    /// Print validated lifecycle and capability status from the daemon.
    Status,
    /// Install the foreground daemon as an opt-in systemd user service.
    Systemd {
        #[command(subcommand)]
        command: SystemdArgs,
    },
}

#[derive(Debug, Subcommand)]
enum SystemdArgs {
    /// Install the daemon and unit; autostart remains disabled.
    Install {
        /// Daemon executable to copy into ~/.local/bin.
        #[arg(long, value_name = "PATH")]
        daemon: Option<PathBuf>,
        /// Replace conflicting installer destinations after saving recoverable backups.
        #[arg(long)]
        replace: bool,
    },
    /// Remove the owned unit and verified installed daemon after it is inactive.
    Uninstall,
    /// Enable autostart without starting the service now.
    Enable,
    /// Disable autostart without stopping an active service.
    Disable,
    /// Start and wait for systemd readiness.
    Start,
    /// Stop the systemd-managed daemon gracefully.
    Stop,
    /// Restart and wait for systemd readiness.
    Restart,
    /// Inspect manager, unit, process, and authenticated IPC state.
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
    if let Some(help) = runtime_install_help(&arguments) {
        return Ok(Command::HelpOutput(help));
    }
    let mut parser = parser_command();
    let parser_arguments =
        iter::once(OsString::from(APPLICATION_NAME)).chain(arguments.iter().cloned());
    let matches = match parser.clone().try_get_matches_from(parser_arguments) {
        Ok(matches) => matches,
        Err(error) if error.kind() == ErrorKind::DisplayHelp => {
            return Ok(Command::HelpOutput(error.to_string()));
        }
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
        Some(SubcommandArgs::Runtime { command }) => match *command {
            RuntimeArgs::Install { operation } => Command::RuntimeInstall(operation),
            RuntimeArgs::ListManaged => {
                Command::RuntimeInstall(Box::new(super::runtime_install::Operation::Status))
            }
        },
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
        Some(SubcommandArgs::Diagnose {
            command: DiagnoseArgs::Hardware,
        }) => Command::DiagnoseHardware,
        Some(SubcommandArgs::Diagnose {
            command: DiagnoseArgs::Gpu,
        }) => Command::DiagnoseGpu,
        Some(SubcommandArgs::Diagnose {
            command: DiagnoseArgs::Display,
        }) => Command::DiagnoseDisplay,
        Some(SubcommandArgs::Diagnose {
            command: DiagnoseArgs::Runtimes { family, roots },
        }) => Command::DiagnoseRuntimes {
            family: family.map(|family| match family {
                RuntimeFamilyArg::Wine => endfield_unix_runtime::RuntimeFamily::Wine,
                RuntimeFamilyArg::Proton => endfield_unix_runtime::RuntimeFamily::Proton,
            }),
            roots,
        },
        Some(SubcommandArgs::Diagnose {
            command: DiagnoseArgs::RuntimeIntegrity(options),
        }) => Command::DiagnoseRuntimeIntegrity(options),
        Some(SubcommandArgs::Diagnose {
            command: DiagnoseArgs::Umu,
        }) => Command::DiagnoseUmu,
        Some(SubcommandArgs::Diagnose {
            command:
                DiagnoseArgs::Telemetry {
                    command: TelemetryArgs::CpuMemory { interval_ms },
                },
        }) => Command::DiagnoseTelemetryCpuMemory { interval_ms },
        Some(SubcommandArgs::Diagnose {
            command:
                DiagnoseArgs::Telemetry {
                    command: TelemetryArgs::Gpu { interval_ms },
                },
        }) => Command::DiagnoseTelemetryGpu { interval_ms },
        Some(SubcommandArgs::Diagnose {
            command:
                DiagnoseArgs::Telemetry {
                    command: TelemetryArgs::Frames { format, input },
                },
        }) => Command::DiagnoseTelemetryFrames {
            format: format.into(),
            input,
        },
        Some(SubcommandArgs::Service {
            command: ServiceArgs::Ping,
        }) => Command::ServicePing,
        Some(SubcommandArgs::Service {
            command: ServiceArgs::Status,
        }) => Command::ServiceStatus,
        Some(SubcommandArgs::Service {
            command:
                ServiceArgs::Systemd {
                    command: SystemdArgs::Install { daemon, replace },
                },
        }) => Command::SystemdInstall { daemon, replace },
        Some(SubcommandArgs::Service {
            command:
                ServiceArgs::Systemd {
                    command: SystemdArgs::Uninstall,
                },
        }) => Command::SystemdUninstall,
        Some(SubcommandArgs::Service {
            command:
                ServiceArgs::Systemd {
                    command: SystemdArgs::Enable,
                },
        }) => Command::SystemdEnable,
        Some(SubcommandArgs::Service {
            command:
                ServiceArgs::Systemd {
                    command: SystemdArgs::Disable,
                },
        }) => Command::SystemdDisable,
        Some(SubcommandArgs::Service {
            command:
                ServiceArgs::Systemd {
                    command: SystemdArgs::Start,
                },
        }) => Command::SystemdStart,
        Some(SubcommandArgs::Service {
            command:
                ServiceArgs::Systemd {
                    command: SystemdArgs::Stop,
                },
        }) => Command::SystemdStop,
        Some(SubcommandArgs::Service {
            command:
                ServiceArgs::Systemd {
                    command: SystemdArgs::Restart,
                },
        }) => Command::SystemdRestart,
        Some(SubcommandArgs::Service {
            command:
                ServiceArgs::Systemd {
                    command: SystemdArgs::Status,
                },
        }) => Command::SystemdStatus,
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

fn runtime_install_help(arguments: &[OsString]) -> Option<String> {
    let (help_flag, path) = arguments.split_last()?;
    if !matches!(help_flag.to_str()?, "--help" | "-h") || path.first()?.to_str()? != "runtime" {
        return None;
    }
    let mut command = Arguments::command().name(APPLICATION_NAME).term_width(80);
    for component in path {
        command = command.find_subcommand(component.to_str()?)?.clone();
    }
    Some(command.render_help().to_string())
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
    fn nested_runtime_install_help_is_a_successful_output_command() {
        for values in [
            &["runtime", "install", "--help"][..],
            &["runtime", "install", "plan", "--help"][..],
            &["runtime", "install", "apply", "--help"][..],
        ] {
            match parse(args(&values)) {
                Ok(Command::HelpOutput(help)) => assert!(help.contains("Usage:"), "{help}"),
                other => panic!("expected nested help output for {values:?}, got {other:?}"),
            }
        }
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
    fn systemd_user_service_commands_are_typed_and_reject_arbitrary_commands() {
        assert_command(
            &[
                "service",
                "systemd",
                "install",
                "--daemon",
                "/opt/endfield-unixd",
                "--replace",
            ],
            Command::SystemdInstall {
                daemon: Some(PathBuf::from("/opt/endfield-unixd")),
                replace: true,
            },
        );
        assert_command(
            &["service", "systemd", "install"],
            Command::SystemdInstall {
                daemon: None,
                replace: false,
            },
        );
        for (operation, command) in [
            ("uninstall", Command::SystemdUninstall),
            ("enable", Command::SystemdEnable),
            ("disable", Command::SystemdDisable),
            ("start", Command::SystemdStart),
            ("stop", Command::SystemdStop),
            ("restart", Command::SystemdRestart),
            ("status", Command::SystemdStatus),
        ] {
            assert_command(&["service", "systemd", operation], command);
        }
        assert!(parse(args(&["service", "systemd", "exec", "sh"])).is_err());
        assert!(parse(args(&["service", "systemd", "install", "--now"])).is_err());
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
    fn diagnose_hardware_is_a_typed_local_command() {
        assert_command(&["diagnose", "hardware"], Command::DiagnoseHardware);
    }

    #[test]
    fn diagnose_gpu_is_a_typed_local_command() {
        assert_command(&["diagnose", "gpu"], Command::DiagnoseGpu);
    }

    #[test]
    fn diagnose_display_is_a_typed_local_command() {
        assert_command(&["diagnose", "display"], Command::DiagnoseDisplay);
    }

    #[test]
    fn diagnose_runtimes_parses_filter_and_absolute_roots() {
        assert_command(&["diagnose", "umu"], Command::DiagnoseUmu);
        assert_command(
            &["diagnose", "runtimes"],
            Command::DiagnoseRuntimes {
                family: None,
                roots: Vec::new(),
            },
        );
        assert_command(
            &[
                "diagnose",
                "runtimes",
                "--family",
                "proton",
                "--root",
                "/tmp/runners",
            ],
            Command::DiagnoseRuntimes {
                family: Some(endfield_unix_runtime::RuntimeFamily::Proton),
                roots: vec![PathBuf::from("/tmp/runners")],
            },
        );
        assert!(parse(args(&["diagnose", "runtimes", "--root", "relative"])).is_err());
        assert!(parse(args(&["diagnose", "runtimes", "--family", "unknown"])).is_err());
    }

    #[test]
    fn cpu_memory_telemetry_uses_the_default_and_custom_bounded_intervals() {
        assert_command(
            &["diagnose", "telemetry", "cpu-memory"],
            Command::DiagnoseTelemetryCpuMemory { interval_ms: 250 },
        );
        assert_command(
            &[
                "diagnose",
                "telemetry",
                "cpu-memory",
                "--interval-ms",
                "500",
            ],
            Command::DiagnoseTelemetryCpuMemory { interval_ms: 500 },
        );
    }

    #[test]
    fn cpu_memory_telemetry_rejects_intervals_outside_policy() {
        for value in ["0", "99", "2001"] {
            assert!(
                parse(args(&[
                    "diagnose",
                    "telemetry",
                    "cpu-memory",
                    "--interval-ms",
                    value,
                ]))
                .is_err()
            );
        }
    }

    #[test]
    fn frame_import_defaults_to_the_canonical_schema_and_requires_a_file() {
        assert_command(
            &[
                "diagnose",
                "telemetry",
                "frames",
                "--input",
                "recording.csv",
            ],
            Command::DiagnoseTelemetryFrames {
                format: FrameSourceFormat::FrameEventsV1,
                input: PathBuf::from("recording.csv"),
            },
        );
        assert!(parse(args(&["diagnose", "telemetry", "frames"])).is_err());
    }

    #[test]
    fn frame_import_accepts_only_explicit_supported_format_names() {
        assert_command(
            &[
                "diagnose",
                "telemetry",
                "frames",
                "--format",
                "mangohud-csv",
                "--input",
                "recording.csv",
            ],
            Command::DiagnoseTelemetryFrames {
                format: FrameSourceFormat::MangoHudCsv,
                input: PathBuf::from("recording.csv"),
            },
        );
        assert!(
            parse(args(&[
                "diagnose",
                "telemetry",
                "frames",
                "--format",
                "auto",
                "--input",
                "recording.csv",
            ]))
            .is_err()
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

#[cfg(test)]
mod gpu_telemetry_tests {
    use super::*;
    #[test]
    fn bounds() {
        for value in ["100", "2000", "500"] {
            assert!(parse(["diagnose", "telemetry", "gpu", "--interval-ms", value]).is_ok());
        }
        for value in ["99", "2001", "bad"] {
            assert!(parse(["diagnose", "telemetry", "gpu", "--interval-ms", value]).is_err());
        }
    }

    #[test]
    fn gpu_default_custom_and_invalid_interval() {
        assert_eq!(
            parse(["diagnose", "telemetry", "gpu"]).unwrap(),
            Command::DiagnoseTelemetryGpu { interval_ms: 250 }
        );
    }
}
