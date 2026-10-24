use std::io;

use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoFailureKind {
    PermissionDenied,
    DiskFull,
    AlreadyExists,
    NotFound,
    Other,
}

impl IoFailureKind {
    pub(crate) fn from_error(error: &io::Error) -> Self {
        if error.kind() == io::ErrorKind::PermissionDenied {
            Self::PermissionDenied
        } else if error.kind() == io::ErrorKind::AlreadyExists {
            Self::AlreadyExists
        } else if error.kind() == io::ErrorKind::NotFound {
            Self::NotFound
        } else if error.raw_os_error() == Some(libc::ENOSPC) {
            Self::DiskFull
        } else {
            Self::Other
        }
    }
}

impl std::fmt::Display for IoFailureKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::PermissionDenied => "permission denied",
            Self::DiskFull => "storage is full",
            Self::AlreadyExists => "destination already exists",
            Self::NotFound => "path was not found",
            Self::Other => "operating-system I/O error",
        })
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DiagnosticsError {
    #[error("diagnostics cannot resolve a safe home or state directory")]
    HomeUnavailable,

    #[error("diagnostics are not supported on this platform")]
    UnsupportedPlatform,

    #[error("invalid diagnostics path: {0}")]
    InvalidPath(&'static str),

    #[error("refused an unsafe diagnostics path: {0}")]
    UnsafePath(&'static str),

    #[error("I/O failed while {operation} ({kind})")]
    Io {
        operation: &'static str,
        kind: IoFailureKind,
    },

    #[error("the requested output file already exists; pass --overwrite to replace it")]
    OutputExists,

    #[error("an event is larger than the configured safe event limit")]
    EventTooLarge,

    #[error("the diagnostics retention policy is invalid")]
    InvalidRetentionPolicy,

    #[error("logging queue capacity must be greater than zero")]
    InvalidQueueCapacity,

    #[error("the diagnostics event is invalid: {0}")]
    InvalidEvent(&'static str),

    #[error("support report serialization failed")]
    Serialization,

    #[error("support report exceeds its output size limit")]
    ReportTooLarge,

    #[error("the logging queue is full; the event was dropped")]
    QueueFull,

    #[error("the logging worker has stopped")]
    WorkerStopped,

    #[error("the logging worker could not be started")]
    WorkerStart,

    #[error("the logging worker did not complete before the timeout")]
    Timeout,
}

pub(crate) fn io_error(operation: &'static str, error: &io::Error) -> DiagnosticsError {
    DiagnosticsError::Io {
        operation,
        kind: IoFailureKind::from_error(error),
    }
}
