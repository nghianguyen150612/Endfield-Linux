/// A topic supported by command-specific help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpTopic {
    Gui,
    About,
}

/// Typed command selected by the CLI parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Help(Option<HelpTopic>),
    Version,
    About,
    LaunchGui,
}
