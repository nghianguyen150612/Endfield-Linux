use std::fmt;

use endfield_unix_diagnostics::DiagnosticsError;

use crate::lifecycle::LifecycleError;

#[derive(Debug)]
pub enum DaemonError {
    SignalHandlerInitialization,
    WorkerStartup,
    Lifecycle(LifecycleError),
    WorkerFailure,
    ShutdownTimeout,
    ResourceCleanup,
}

#[derive(Debug)]
pub enum OptionalDiagnosticsFailure {
    Startup(DiagnosticsError),
    EventRejected(DiagnosticsError),
    Shutdown(DiagnosticsError),
}

impl fmt::Display for DaemonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SignalHandlerInitialization => "could not initialize shutdown signal handling",
            Self::WorkerStartup => "could not start an owned daemon worker",
            Self::Lifecycle(_) => "the daemon lifecycle transition failed",
            Self::WorkerFailure => "an owned daemon worker failed",
            Self::ShutdownTimeout => "daemon shutdown did not complete within its configured limit",
            Self::ResourceCleanup => "daemon resource cleanup failed",
        })
    }
}

impl std::error::Error for DaemonError {}

impl From<LifecycleError> for DaemonError {
    fn from(value: LifecycleError) -> Self {
        match value {
            LifecycleError::ShutdownTimeout => Self::ShutdownTimeout,
            LifecycleError::WorkerFailed(_) => Self::WorkerFailure,
            _ => Self::Lifecycle(value),
        }
    }
}
