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
    DiagnoseTelemetryCpuMemory {
        interval_ms: u64,
    },
    ServicePing,
    ServiceStatus,
}
