use std::error::Error;
use std::fmt;
use std::time::Duration;

pub mod cli;
pub mod ui;

pub const APPLICATION_NAME: &str = env!("CARGO_PKG_NAME");
pub const APPLICATION_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APPLICATION_PURPOSE: &str = env!("CARGO_PKG_DESCRIPTION");
pub const APPLICATION_LICENSE: &str = env!("CARGO_PKG_LICENSE");
pub const APPLICATION_REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
pub const APPLICATION_STAGE: &str = "Early development. The native desktop shell, typed CLI, and authenticated Linux daemon IPC are available; game, runtime, telemetry, and optimization services are under development.";
pub const APPLICATION_ID: &str = "io.github.nghianguyen150612.endfieldunix";

pub use cli::dispatch::{
    EXIT_GUI_STARTUP_ERROR, EXIT_INVALID_ARGUMENTS, EXIT_IO_ERROR, EXIT_SUCCESS, run, run_with_io,
};

/// Starts the existing GTK application on the calling thread.
pub fn start_gui() -> Result<(), GuiStartupError> {
    if endfield_unix_diagnostics::initialize_global_logging().is_err() {
        endfield_unix_diagnostics::record_lifecycle(
            endfield_unix_diagnostics::LifecycleEvent::DiagnosticsUnavailable,
        );
    }
    endfield_unix_diagnostics::record_lifecycle(
        endfield_unix_diagnostics::LifecycleEvent::AppStarting,
    );
    let result = ui::run().map_err(|error| GuiStartupError::new(error.to_string()));
    endfield_unix_diagnostics::record_lifecycle(if result.is_ok() {
        endfield_unix_diagnostics::LifecycleEvent::WindowClosedNormally
    } else {
        endfield_unix_diagnostics::LifecycleEvent::GuiStartupFailed
    });
    let _ = endfield_unix_diagnostics::flush_global_logging(Duration::from_millis(250));
    result
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_id_meets_glib_requirements() {
        assert!(gtk::gio::Application::id_is_valid(APPLICATION_ID));
    }
}
