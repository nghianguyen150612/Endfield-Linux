/// A topic supported by command-specific help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpTopic {
    Gui,
    About,
    Service,
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
    ServicePing,
    ServiceStatus,
}
