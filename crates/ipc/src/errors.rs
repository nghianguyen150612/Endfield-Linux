use std::io;

use thiserror::Error;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    #[error("XDG_RUNTIME_DIR is missing or unsafe")]
    RuntimeDirectoryUnavailable,
    #[error("the application runtime directory is unsafe")]
    UnsafeRuntimeDirectory,
    #[error("the Unix socket path exceeds the platform limit")]
    SocketPathTooLong,
    #[error("another daemon instance already owns the socket")]
    InstanceConflict,
    #[error("an existing socket cannot be safely recovered")]
    StaleSocketUnsafe,
    #[error("the IPC server could not be initialized")]
    ServerStartup,
    #[error("the IPC server could not clean up its owned socket")]
    ResourceCleanup,
    #[error("the daemon is unavailable")]
    DaemonUnavailable,
    #[error("the IPC connection timed out")]
    ConnectionTimeout,
    #[error("the request timed out")]
    RequestTimeout,
    #[error("peer authentication was denied")]
    AuthenticationDenied,
    #[error("the daemon and client protocol versions are incompatible")]
    ProtocolMismatch,
    #[error("the daemon returned an invalid response")]
    InvalidResponse,
    #[error("the daemon is shutting down")]
    ServiceShuttingDown,
    #[error("the daemon is busy")]
    ServerBusy,
    #[error("the daemon rejected the request")]
    RequestRejected,
    #[error("this platform does not have a validated IPC transport")]
    UnsupportedPlatform,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ServerConnectionError {
    #[error("peer authentication was denied")]
    AuthenticationDenied,
    #[error("the client request timed out")]
    RequestTimeout,
    #[error("the client request was malformed")]
    MalformedRequest,
    #[error("the client requested an unsupported protocol version")]
    UnsupportedProtocolVersion,
    #[error("the client requested an unsupported operation")]
    UnsupportedOperation,
    #[error("the daemon is shutting down")]
    ServiceShuttingDown,
    #[error("peer credential authentication is unavailable on this platform")]
    UnsupportedPlatform,
    #[error("the IPC transport failed")]
    Transport,
}

pub(crate) fn classify_connect_error(error: &io::Error) -> IpcError {
    match error.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => IpcError::ConnectionTimeout,
        io::ErrorKind::NotFound
        | io::ErrorKind::ConnectionRefused
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::BrokenPipe => IpcError::DaemonUnavailable,
        io::ErrorKind::PermissionDenied => IpcError::AuthenticationDenied,
        _ => IpcError::DaemonUnavailable,
    }
}
