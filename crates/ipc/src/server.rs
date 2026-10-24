use std::io::{self, Write};
use std::os::fd::AsFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::authentication::{authenticated_peer_uid, peer_uid_is_authorized};
use crate::errors::{IpcError, ServerConnectionError};
use crate::paths::{InstanceLock, RuntimePaths, SocketIdentity, resolve_runtime_paths};
use crate::protocol::{
    DaemonLifecycle, DaemonStatus, ErrorCode, IpcResponse, Request, RequestId, RequestType,
    ResponsePayload, ResponseType, SUPPORTED_PROTOCOL,
};
use crate::transport::{TimedTransportError, read_frame_until, write_frame_until};
use nix::errno::Errno;
use nix::poll::{PollFd, PollFlags, poll};

pub const MAX_SIMULTANEOUS_CLIENTS: usize = 4;
pub const MAX_QUEUED_CLIENTS: usize = 12;
pub const REQUEST_DEADLINE: Duration = Duration::from_millis(1_500);

#[derive(Clone)]
pub struct ShutdownHandle {
    shared: Arc<ShutdownSignal>,
}

struct ShutdownSignal {
    writer: Mutex<UnixStream>,
    requested: AtomicBool,
}

impl ShutdownHandle {
    pub fn request_shutdown(&self) -> Result<(), IpcError> {
        if self.shared.requested.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let mut writer = self
            .shared
            .writer
            .lock()
            .map_err(|_| IpcError::ResourceCleanup)?;
        writer
            .write_all(&[1])
            .map_err(|_| IpcError::ResourceCleanup)
    }

    fn is_requested(&self) -> bool {
        self.shared.requested.load(Ordering::Acquire)
    }
}

pub struct LocalSocketServer {
    listener: Option<UnixListener>,
    wake_reader: UnixStream,
    shutdown: ShutdownHandle,
    paths: RuntimePaths,
    identity: Option<SocketIdentity>,
    _lock: Option<InstanceLock>,
}

impl LocalSocketServer {
    pub fn bind() -> Result<Self, IpcError> {
        let paths = resolve_runtime_paths()?;
        Self::bind_at(paths)
    }

    pub fn bind_at(paths: RuntimePaths) -> Result<Self, IpcError> {
        if !cfg!(target_os = "linux") {
            return Err(IpcError::UnsupportedPlatform);
        }
        paths.prepare_server_directory()?;
        let lock = paths.acquire_instance_lock()?;
        paths.recover_stale_socket()?;
        let (wake_reader, wake_writer) = UnixStream::pair().map_err(|_| IpcError::ServerStartup)?;
        let listener = UnixListener::bind(paths.socket_path()).map_err(|error| {
            if error.kind() == io::ErrorKind::AddrInUse {
                IpcError::InstanceConflict
            } else {
                IpcError::ServerStartup
            }
        })?;
        if std::fs::set_permissions(
            paths.socket_path(),
            std::fs::Permissions::from_mode(crate::SOCKET_MODE),
        )
        .is_err()
        {
            drop(listener);
            let _ = paths.remove_created_socket();
            return Err(IpcError::ServerStartup);
        }
        let identity = paths.socket_identity()?;
        let shutdown = ShutdownHandle {
            shared: Arc::new(ShutdownSignal {
                writer: Mutex::new(wake_writer),
                requested: AtomicBool::new(false),
            }),
        };
        Ok(Self {
            listener: Some(listener),
            wake_reader,
            shutdown,
            paths,
            identity: Some(identity),
            _lock: Some(lock),
        })
    }

    pub fn socket_path(&self) -> &Path {
        self.paths.socket_path()
    }

    pub fn shutdown_handle(&self) -> ShutdownHandle {
        self.shutdown.clone()
    }

    /// Blocks in poll until a client connects or the shutdown wake stream is signalled.
    pub fn accept(&self) -> Result<Option<UnixStream>, IpcError> {
        if self.shutdown.is_requested() {
            return Ok(None);
        }
        let listener = self.listener.as_ref().ok_or(IpcError::ResourceCleanup)?;
        loop {
            let mut descriptors = [
                PollFd::new(listener.as_fd(), PollFlags::POLLIN),
                PollFd::new(self.wake_reader.as_fd(), PollFlags::POLLIN),
            ];
            match poll(&mut descriptors, nix::poll::PollTimeout::NONE) {
                Ok(_) => {}
                Err(Errno::EINTR) => continue,
                Err(_) => return Err(IpcError::ServerStartup),
            }
            if self.shutdown.is_requested()
                || descriptors[1]
                    .revents()
                    .is_some_and(|flags| flags.intersects(PollFlags::POLLIN | PollFlags::POLLHUP))
            {
                return Ok(None);
            }
            if descriptors[0]
                .revents()
                .is_some_and(|flags| flags.contains(PollFlags::POLLIN))
            {
                match listener.accept() {
                    Ok((stream, _)) => return Ok(Some(stream)),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => return Err(IpcError::ServerStartup),
                }
            }
        }
    }

    pub fn close(mut self) -> Result<(), IpcError> {
        self.cleanup()
    }

    fn cleanup(&mut self) -> Result<(), IpcError> {
        drop(self.listener.take());
        let cleanup_result = match self.identity.take() {
            Some(identity) => self.paths.remove_owned_socket(identity),
            None => Ok(()),
        };
        self._lock.take();
        cleanup_result
    }
}

impl Drop for LocalSocketServer {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

pub fn handle_connection<F>(
    mut stream: UnixStream,
    expected_uid: u32,
    accepted_at: Instant,
    status_provider: F,
) -> Result<(), ServerConnectionError>
where
    F: FnOnce() -> DaemonStatus,
{
    let peer_uid = authenticated_peer_uid(&stream)?;
    if !peer_uid_is_authorized(expected_uid, peer_uid) {
        return Err(ServerConnectionError::AuthenticationDenied);
    }

    let deadline = accepted_at
        .checked_add(REQUEST_DEADLINE)
        .unwrap_or(accepted_at);
    let bytes = read_frame_until(&mut stream, deadline).map_err(map_transport_error)?;
    let request: Request = match serde_json::from_slice(&bytes) {
        Ok(request) => request,
        Err(_) => {
            send_error(
                &mut stream,
                RequestId::new("malformed").expect("fixed request id is valid"),
                ErrorCode::MalformedRequest,
                deadline,
            )?;
            return Err(ServerConnectionError::MalformedRequest);
        }
    };

    if request.protocol_major != SUPPORTED_PROTOCOL.major
        || request.protocol_minor > SUPPORTED_PROTOCOL.minor
    {
        send_error(
            &mut stream,
            request.request_id,
            ErrorCode::UnsupportedProtocolVersion,
            deadline,
        )?;
        return Err(ServerConnectionError::UnsupportedProtocolVersion);
    }
    if request.message_type == RequestType::Unknown {
        send_error(
            &mut stream,
            request.request_id,
            ErrorCode::UnsupportedOperation,
            deadline,
        )?;
        return Err(ServerConnectionError::UnsupportedOperation);
    }

    let status = status_provider();
    if status.lifecycle != DaemonLifecycle::Running {
        send_error(
            &mut stream,
            request.request_id,
            ErrorCode::ServiceShuttingDown,
            deadline,
        )?;
        return Err(ServerConnectionError::ServiceShuttingDown);
    }

    match request.message_type {
        RequestType::Ping => send_response(
            &mut stream,
            IpcResponse::new(
                request.request_id,
                ResponseType::Pong,
                ResponsePayload::Pong,
            ),
            deadline,
        ),
        RequestType::GetStatus => {
            if !status.validate() {
                send_error(
                    &mut stream,
                    request.request_id,
                    ErrorCode::InternalFailure,
                    deadline,
                )?;
                return Err(ServerConnectionError::Transport);
            }
            send_response(
                &mut stream,
                IpcResponse::new(
                    request.request_id,
                    ResponseType::Status,
                    ResponsePayload::Status(status),
                ),
                deadline,
            )
        }
        RequestType::Unknown => unreachable!("unknown requests are rejected above"),
    }
}

fn send_error(
    stream: &mut UnixStream,
    request_id: crate::RequestId,
    code: ErrorCode,
    deadline: Instant,
) -> Result<(), ServerConnectionError> {
    send_response(
        stream,
        IpcResponse::new(
            request_id,
            ResponseType::Error,
            ResponsePayload::Error { code },
        ),
        deadline,
    )
}

fn send_response(
    stream: &mut UnixStream,
    response: IpcResponse,
    deadline: Instant,
) -> Result<(), ServerConnectionError> {
    let bytes = serde_json::to_vec(&response).map_err(|_| ServerConnectionError::Transport)?;
    write_frame_until(stream, &bytes, deadline).map_err(map_transport_error)
}

fn map_transport_error(error: TimedTransportError) -> ServerConnectionError {
    match error {
        TimedTransportError::Timeout => ServerConnectionError::RequestTimeout,
        TimedTransportError::Frame(frame_error) => {
            let _ = frame_error;
            ServerConnectionError::MalformedRequest
        }
        TimedTransportError::Io => ServerConnectionError::Transport,
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixStream;
    use std::thread;

    use super::*;
    use crate::protocol::DaemonStatus;

    #[test]
    fn credential_policy_compares_only_kernel_uid_to_expected_uid() {
        assert!(peer_uid_is_authorized(1000, 1000));
        assert!(!peer_uid_is_authorized(1000, 1001));
        assert!(!peer_uid_is_authorized(1000, 0));
    }

    #[test]
    fn same_uid_socket_peer_completes_ping_before_response() {
        let (server, mut client) = UnixStream::pair().expect("socket pair");
        let worker = thread::spawn(move || {
            handle_connection(
                server,
                nix::unistd::Uid::effective().as_raw(),
                Instant::now(),
                || DaemonStatus::running("0.1.0", "0.1.0"),
            )
        });
        let request = Request::new(
            RequestType::Ping,
            RequestId::new("auth-test").expect("valid request id"),
        );
        let bytes = serde_json::to_vec(&request).expect("serialize request");
        crate::write_frame(&mut client, &bytes).expect("send request");
        let response = crate::read_frame(&mut client).expect("read response");
        let response: IpcResponse = serde_json::from_slice(&response).expect("typed response");
        assert_eq!(response.message_type, ResponseType::Pong);
        assert!(worker.join().expect("server thread").is_ok());
    }

    #[test]
    fn request_deadline_is_overall_and_expires_queued_or_slow_connections() {
        let (server, _client) = UnixStream::pair().expect("socket pair");
        let accepted_at = Instant::now() - REQUEST_DEADLINE - Duration::from_millis(1);
        assert!(matches!(
            handle_connection(
                server,
                nix::unistd::Uid::effective().as_raw(),
                accepted_at,
                || DaemonStatus::running("0.1.0", "0.1.0"),
            ),
            Err(ServerConnectionError::RequestTimeout)
        ));
    }
}
