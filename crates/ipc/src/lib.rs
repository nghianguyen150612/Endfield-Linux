#![forbid(unsafe_code)]

mod authentication;
mod client;
mod errors;
mod framing;
mod paths;
mod protocol;
mod server;
mod transport;

pub use authentication::peer_uid_is_authorized;
pub use client::{Client, ClientError};
pub use errors::{IpcError, ServerConnectionError};
pub use framing::{FrameError, MAX_FRAME_SIZE, read_frame, write_frame};
pub use paths::{RuntimePaths, SOCKET_FILE_NAME, SOCKET_MODE, SOCKET_NAME, resolve_runtime_paths};
pub use protocol::{
    DaemonLifecycle, DaemonStatus, ErrorCode, ImplementedCapability, IpcResponse, ProtocolVersion,
    Request, RequestId, RequestIdError, RequestType, ResponsePayload, ResponseType,
    SUPPORTED_PROTOCOL, StatusCode, UnimplementedCapability,
};
pub use server::{
    LocalSocketServer, MAX_QUEUED_CLIENTS, MAX_SIMULTANEOUS_CLIENTS, REQUEST_DEADLINE,
    ShutdownHandle, handle_connection,
};

pub const MAX_REQUEST_ID_BYTES: usize = 64;
