use endfield_unix_platform_linux::FrameSourceFormat;

/// A topic supported by command-specific help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpTopic {
    Gui,
    About,
    Service,
    Diagnose,
    Prefix,
}

/// Typed command selected by the CLI parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help(Option<HelpTopic>),
    Version,
    About,
    HelpOutput(String),
    LaunchGui,
    RuntimeInstall(Box<super::runtime_install::Operation>),
    RuntimePin(Box<super::runtime_pin::Operation>),
    PrefixManagement(Box<super::prefix::Operation>),
    RuntimeAudit(String),
    RuntimeAuditSource {
        installation_id: String,
        inputs: Box<super::runtime_integrity::Options>,
    },
    DiagnoseReport {
        output: std::path::PathBuf,
        overwrite: bool,
    },
    DiagnosePlatform,
    DiagnoseHardware,
    DiagnoseGpu,
    DiagnoseDisplay,
    DiagnoseCompatibility {
        profile: Option<String>,
        prefix_id: Option<String>,
        json: bool,
    },
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
