use endfield_unix_platform_linux::FrameSourceFormat;

/// A topic supported by command-specific help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpTopic {
    Gui,
    About,
    Service,
    Diagnose,
}

/// Typed command selected by the CLI parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help(Option<HelpTopic>),
    Version,
    About,
    LaunchGui,
    DiagnoseReport {
        output: std::path::PathBuf,
        overwrite: bool,
    },
    DiagnosePlatform,
    DiagnoseHardware,
    DiagnoseGpu,
    DiagnoseDisplay,
    DiagnoseRuntimes {
        family: Option<endfield_unix_runtime::RuntimeFamily>,
        roots: Vec<std::path::PathBuf>,
    },
    DiagnoseUmu,
    DiagnoseRuntimeIntegrity(Box<super::runtime_integrity::Options>),
    DiagnoseTelemetryGpu {
        interval_ms: u64,
    },
    DiagnoseTelemetryCpuMemory {
        interval_ms: u64,
    },
    DiagnoseTelemetryFrames {
        format: FrameSourceFormat,
        input: std::path::PathBuf,
    },
    ServicePing,
    ServiceStatus,
    SystemdInstall {
        daemon: Option<std::path::PathBuf>,
        replace: bool,
    },
    SystemdUninstall,
    SystemdEnable,
    SystemdDisable,
    SystemdStart,
    SystemdStop,
    SystemdRestart,
    SystemdStatus,
}
