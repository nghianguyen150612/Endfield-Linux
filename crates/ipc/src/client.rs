use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::errors::{IpcError, classify_connect_error};
use crate::paths::resolve_runtime_paths;
use crate::protocol::{
    DaemonStatus, ErrorCode, IpcResponse, Request, RequestId, RequestType, ResponsePayload,
    ResponseType, SUPPORTED_PROTOCOL,
};
use crate::transport::{TimedTransportError, connect_bounded, read_frame_until, write_frame_until};

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
pub enum ClientError {
    #[error("XDG_RUNTIME_DIR is missing or unsafe")]
    RuntimeDirectoryUnavailable,
    #[error("the Unix socket path is unsafe or too long")]
    UnsafeSocketPath,
    #[error("the daemon is unavailable")]
    DaemonUnavailable,
    #[error("the IPC connection timed out")]
    ConnectionTimeout,
    #[error("peer authentication was denied")]
    AuthenticationDenied,
    #[error("the daemon and client protocol versions are incompatible")]
    ProtocolMismatch,
    #[error("the daemon returned an invalid response")]
    InvalidResponse,
    #[error("the request timed out")]
    RequestTimeout,
    #[error("the daemon is shutting down")]
    ServiceShuttingDown,
    #[error("the daemon is busy")]
    ServerBusy,
    #[error("authenticated local IPC is not implemented on this platform")]
    UnsupportedPlatform,
    #[error("the daemon rejected the request")]
    RequestRejected,
}

impl From<IpcError> for ClientError {
    fn from(error: IpcError) -> Self {
        match error {
            IpcError::RuntimeDirectoryUnavailable => Self::RuntimeDirectoryUnavailable,
            IpcError::UnsafeRuntimeDirectory | IpcError::SocketPathTooLong => {
                Self::UnsafeSocketPath
            }
            IpcError::ConnectionTimeout => Self::ConnectionTimeout,
            IpcError::AuthenticationDenied => Self::AuthenticationDenied,
            IpcError::ProtocolMismatch => Self::ProtocolMismatch,
            IpcError::RequestTimeout => Self::RequestTimeout,
            IpcError::ServiceShuttingDown => Self::ServiceShuttingDown,
            IpcError::ServerBusy => Self::ServerBusy,
            IpcError::InvalidResponse | IpcError::RequestRejected => Self::InvalidResponse,
            IpcError::UnsupportedPlatform => Self::UnsupportedPlatform,
            _ => Self::DaemonUnavailable,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Client {
    socket_path: PathBuf,
    timeout: Duration,
}

impl Client {
    pub fn from_environment(timeout: Duration) -> Result<Self, ClientError> {
        if !cfg!(target_os = "linux") {
            return Err(ClientError::UnsupportedPlatform);
        }
        let paths = resolve_runtime_paths().map_err(ClientError::from)?;
        paths.validate_client_socket().map_err(ClientError::from)?;
        Ok(Self {
            socket_path: paths.socket_path().to_path_buf(),
            timeout,
        })
    }

    pub fn at_socket_path(path: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            socket_path: path.into(),
            timeout,
        }
    }

    pub fn ping(&self) -> Result<(), ClientError> {
        let response = self.transact(RequestType::Ping)?;
        match response {
            ResponsePayload::Pong => Ok(()),
            _ => Err(ClientError::InvalidResponse),
        }
    }

    pub fn status(&self) -> Result<DaemonStatus, ClientError> {
        let response = self.transact(RequestType::GetStatus)?;
        match response {
            ResponsePayload::Status(status) if status.validate() => Ok(status),
            _ => Err(ClientError::InvalidResponse),
        }
    }

    fn transact(&self, request_type: RequestType) -> Result<ResponsePayload, ClientError> {
        let deadline = Instant::now()
            .checked_add(self.timeout)
            .unwrap_or_else(Instant::now);
        let mut stream = connect_bounded(&self.socket_path, deadline)
            .map_err(|error| ClientError::from(classify_connect_error(&error)))?;
        let request_id = RequestId::new(format!(
            "{:016x}",
            NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
        ))
        .expect("counter-generated request id is bounded ascii");
        let request = Request::new(request_type, request_id.clone());
        let bytes = serde_json::to_vec(&request).map_err(|_| ClientError::InvalidResponse)?;
        write_frame_until(&mut stream, &bytes, deadline).map_err(map_transport_error)?;
        let bytes = read_frame_until(&mut stream, deadline).map_err(map_transport_error)?;
        let response: IpcResponse =
            serde_json::from_slice(&bytes).map_err(|_| ClientError::InvalidResponse)?;
        if response.protocol_major != SUPPORTED_PROTOCOL.major
            || response.protocol_minor > SUPPORTED_PROTOCOL.minor
        {
            return Err(ClientError::ProtocolMismatch);
        }
        if response.protocol_minor > request.protocol_minor
            || response.request_id != request_id
            || !response.validate_shape()
        {
            return Err(ClientError::InvalidResponse);
        }
        match response.payload {
            ResponsePayload::Error { code } => Err(map_error_code(code)),
            payload => {
                let expected = match request_type {
                    RequestType::Ping => ResponseType::Pong,
                    RequestType::GetStatus => ResponseType::Status,
                    RequestType::Unknown => return Err(ClientError::InvalidResponse),
                };
                if response.message_type != expected {
                    return Err(ClientError::InvalidResponse);
                }
                Ok(payload)
            }
        }
    }
}

fn map_error_code(code: ErrorCode) -> ClientError {
    match code {
        ErrorCode::MalformedRequest | ErrorCode::InternalFailure => ClientError::InvalidResponse,
        ErrorCode::UnsupportedProtocolVersion => ClientError::ProtocolMismatch,
        ErrorCode::UnsupportedOperation => ClientError::RequestRejected,
        ErrorCode::AuthenticationFailure => ClientError::AuthenticationDenied,
        ErrorCode::ServerBusy => ClientError::ServerBusy,
        ErrorCode::RequestTimeout => ClientError::RequestTimeout,
        ErrorCode::ServiceShuttingDown => ClientError::ServiceShuttingDown,
    }
}

fn map_transport_error(error: TimedTransportError) -> ClientError {
    match error {
        TimedTransportError::Timeout => ClientError::RequestTimeout,
        TimedTransportError::Frame(frame_error) => {
            let _ = frame_error;
            ClientError::InvalidResponse
        }
        TimedTransportError::Io => ClientError::ServiceShuttingDown,
    }
}
