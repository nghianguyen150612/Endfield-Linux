//! Minimal systemd `sd_notify(3)` protocol support for the foreground daemon.
//!
//! The daemon sends only fixed `READY=1` and `STOPPING=1` messages. It does not
//! accept notification destinations on the command line or create a socket.

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use nix::errno::Errno;
use nix::sys::socket::{AddressFamily, SockFlag, SockType, UnixAddr, connect, send, socket};
use nix::unistd::geteuid;

const MAX_NOTIFY_ADDRESS: usize = 107;
const MAX_NOTIFY_MESSAGE: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Destination {
    Filesystem(std::path::PathBuf),
    Abstract(Vec<u8>),
}

/// Optional notifier created from the service-manager-provided environment.
#[derive(Debug)]
pub struct NotificationClient {
    destination: Destination,
    ready_sent: bool,
    stopping_sent: bool,
}

/// Safe, bounded errors from parsing or sending a readiness notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationError {
    InvalidAddress,
    UnsafeAddress,
    Socket,
    Send,
    OversizedMessage,
    InvalidMessage,
}

impl fmt::Display for NotificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidAddress => "the service-manager notification address is invalid",
            Self::UnsafeAddress => "the service-manager notification address is unsafe",
            Self::Socket => "could not open the service-manager notification socket",
            Self::Send => "could not send a service-manager notification",
            Self::OversizedMessage => "the service-manager notification is too large",
            Self::InvalidMessage => "the service-manager notification is not supported",
        })
    }
}

impl std::error::Error for NotificationError {}

impl NotificationClient {
    /// Reads only `NOTIFY_SOCKET`. Missing means ordinary foreground mode.
    pub fn from_environment() -> Result<Option<Self>, NotificationError> {
        Self::from_optional_value(env::var_os("NOTIFY_SOCKET"))
    }

    fn from_optional_value(value: Option<OsString>) -> Result<Option<Self>, NotificationError> {
        value.map(Self::from_value).transpose()
    }

    fn from_value(value: OsString) -> Result<Self, NotificationError> {
        Self::from_value_with_policy(value, false)
    }

    #[cfg(test)]
    pub(crate) fn for_test(value: OsString) -> Result<Self, NotificationError> {
        Self::from_value_with_policy(value, true)
    }

    fn from_value_with_policy(
        value: OsString,
        allow_non_systemd_abstract_for_tests: bool,
    ) -> Result<Self, NotificationError> {
        let bytes = value.as_os_str().as_bytes();
        if bytes.is_empty() || bytes.len() > MAX_NOTIFY_ADDRESS || bytes.contains(&0) {
            return Err(NotificationError::InvalidAddress);
        }

        let destination = if let Some(name) = bytes.strip_prefix(b"@") {
            if name.is_empty()
                || (!allow_non_systemd_abstract_for_tests
                    && name != b"/org/freedesktop/systemd1/notify")
            {
                return Err(NotificationError::InvalidAddress);
            }
            Destination::Abstract(name.to_vec())
        } else {
            let path = Path::new(value.as_os_str());
            if !path.is_absolute()
                || path.components().any(|component| {
                    matches!(
                        component,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
            {
                return Err(NotificationError::InvalidAddress);
            }
            if !allow_non_systemd_abstract_for_tests && !is_user_manager_notify_path(path) {
                return Err(NotificationError::InvalidAddress);
            }
            let metadata =
                fs::symlink_metadata(path).map_err(|_| NotificationError::UnsafeAddress)?;
            use std::os::unix::fs::{FileTypeExt, MetadataExt};
            if !metadata.file_type().is_socket()
                || metadata.uid() != geteuid().as_raw()
                || metadata.mode() & 0o022 != 0
            {
                return Err(NotificationError::UnsafeAddress);
            }
            Destination::Filesystem(path.to_path_buf())
        };

        Ok(Self {
            destination,
            ready_sent: false,
            stopping_sent: false,
        })
    }

    /// Sends readiness exactly once, after the caller has made IPC available.
    pub fn ready(&mut self) -> Result<(), NotificationError> {
        if self.ready_sent {
            return Ok(());
        }
        self.send_message(b"READY=1")?;
        self.ready_sent = true;
        Ok(())
    }

    /// Sends the shutdown transition at most once. Cleanup must continue if it
    /// fails; callers should treat this as best-effort during shutdown.
    pub fn stopping(&mut self) -> Result<(), NotificationError> {
        if self.stopping_sent {
            return Ok(());
        }
        self.send_message(b"STOPPING=1")?;
        self.stopping_sent = true;
        Ok(())
    }

    fn send_message(&self, message: &[u8]) -> Result<(), NotificationError> {
        if message.len() > MAX_NOTIFY_MESSAGE {
            return Err(NotificationError::OversizedMessage);
        }
        if message != b"READY=1" && message != b"STOPPING=1" {
            return Err(NotificationError::InvalidMessage);
        }

        let socket = socket(
            AddressFamily::Unix,
            SockType::Datagram,
            SockFlag::SOCK_CLOEXEC,
            None,
        )
        .map_err(map_socket_error)?;
        let address = match &self.destination {
            Destination::Filesystem(path) => UnixAddr::new(path),
            Destination::Abstract(name) => UnixAddr::new_abstract(name),
        }
        .map_err(|_| NotificationError::InvalidAddress)?;
        connect(socket.as_raw_fd(), &address).map_err(map_socket_error)?;

        let sent = send(
            socket.as_raw_fd(),
            message,
            nix::sys::socket::MsgFlags::empty(),
        )
        .map_err(map_socket_error)?;
        if sent != message.len() {
            return Err(NotificationError::Send);
        }
        Ok(())
    }
}

fn is_user_manager_notify_path(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Some(runtime_value) = env::var_os("XDG_RUNTIME_DIR") else {
        return false;
    };
    let runtime = Path::new(&runtime_value);
    if !runtime.is_absolute()
        || runtime.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
        || path != runtime.join("systemd/notify")
    {
        return false;
    }

    let uid = geteuid().as_raw();
    let mut current = std::path::PathBuf::from("/");
    for component in runtime.components().skip(1) {
        current.push(component.as_os_str());
        let Ok(metadata) = fs::symlink_metadata(&current) else {
            return false;
        };
        let sticky_root_directory = metadata.uid() == 0 && metadata.mode() & 0o1000 != 0;
        if !metadata.is_dir()
            || (metadata.uid() != 0 && metadata.uid() != uid)
            || (metadata.mode() & 0o022 != 0 && !sticky_root_directory)
        {
            return false;
        }
    }
    let Ok(runtime_metadata) = fs::symlink_metadata(runtime) else {
        return false;
    };
    if !runtime_metadata.is_dir()
        || runtime_metadata.uid() != uid
        || runtime_metadata.mode() & 0o077 != 0
    {
        return false;
    }

    let unit_runtime = runtime.join("systemd");
    let Ok(systemd_metadata) = fs::symlink_metadata(unit_runtime) else {
        return false;
    };
    systemd_metadata.is_dir()
        && systemd_metadata.uid() == uid
        && systemd_metadata.mode() & 0o022 == 0
}

fn map_socket_error(error: Errno) -> NotificationError {
    match error {
        Errno::EINVAL | Errno::ENAMETOOLONG | Errno::EAFNOSUPPORT => {
            NotificationError::InvalidAddress
        }
        Errno::EACCES | Errno::EPERM => NotificationError::UnsafeAddress,
        _ => NotificationError::Socket,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::net::UnixDatagram;
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn missing_notify_socket_keeps_foreground_mode_optional() {
        assert!(
            NotificationClient::from_optional_value(None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn filesystem_socket_receives_fixed_readiness_and_stopping_messages() {
        let directory = tempdir().expect("temporary notification directory");
        let path = directory.path().join("notify.sock");
        let listener = UnixDatagram::bind(&path).expect("bind isolated notification socket");
        let mut client = NotificationClient::for_test(path.as_os_str().to_os_string())
            .expect("validate owner-controlled socket");

        client.ready().expect("send readiness");
        client.ready().expect("readiness is one-shot");
        client.stopping().expect("send stopping");
        client.stopping().expect("stopping is one-shot");

        let mut buffer = [0_u8; MAX_NOTIFY_MESSAGE];
        let (first, _) = listener.recv_from(&mut buffer).expect("read READY");
        assert_eq!(&buffer[..first], b"READY=1");
        let (second, _) = listener.recv_from(&mut buffer).expect("read STOPPING");
        assert_eq!(&buffer[..second], b"STOPPING=1");
    }

    #[test]
    fn abstract_socket_receives_notification() {
        let name = format!("endfield-unix-notify-{}", std::process::id());
        let address = UnixAddr::new_abstract(name.as_bytes()).expect("abstract address");
        let socket = socket(
            AddressFamily::Unix,
            SockType::Datagram,
            SockFlag::SOCK_CLOEXEC,
            None,
        )
        .expect("create abstract listener");
        nix::sys::socket::bind(socket.as_raw_fd(), &address).expect("bind abstract listener");
        let raw_address = OsString::from(format!("@{name}"));
        let mut client = NotificationClient::from_value_with_policy(raw_address, true)
            .expect("parse isolated test address");
        client.ready().expect("send abstract readiness");
        let mut buffer = [0_u8; MAX_NOTIFY_MESSAGE];
        let received = nix::sys::socket::recv(
            socket.as_raw_fd(),
            &mut buffer,
            nix::sys::socket::MsgFlags::empty(),
        )
        .expect("receive abstract readiness");
        assert_eq!(&buffer[..received], b"READY=1");
    }

    #[test]
    fn unsafe_and_malformed_destinations_are_rejected() {
        for value in ["relative.sock", "", "@", "@not-systemd"] {
            assert!(NotificationClient::from_value(OsString::from(value)).is_err());
        }
        let directory = tempdir().expect("temporary directory");
        let regular = directory.path().join("regular");
        fs::write(&regular, b"not a socket").expect("write regular file");
        assert_eq!(
            NotificationClient::for_test(regular.as_os_str().to_os_string()).unwrap_err(),
            NotificationError::UnsafeAddress
        );
        let symlink = directory.path().join("linked.sock");
        std::os::unix::fs::symlink(Path::new("regular"), &symlink).expect("create symlink");
        assert_eq!(
            NotificationClient::for_test(symlink.as_os_str().to_os_string()).unwrap_err(),
            NotificationError::UnsafeAddress
        );
    }

    #[test]
    fn invalid_start_does_not_mark_readiness_sent() {
        let error = NotificationClient::for_test(OsString::from("/no/such/notify.sock"));
        assert_eq!(error.unwrap_err(), NotificationError::UnsafeAddress);
    }

    #[test]
    fn notification_send_failure_is_typed_and_does_not_mark_ready() {
        let directory = tempdir().expect("temporary notification directory");
        let path = directory.path().join("notify.sock");
        let listener = UnixDatagram::bind(&path).expect("bind isolated socket");
        let mut client = NotificationClient::for_test(path.as_os_str().to_os_string())
            .expect("validate isolated socket");
        drop(listener);
        fs::remove_file(&path).expect("remove notification socket");
        assert!(matches!(client.ready(), Err(NotificationError::Socket)));

        let listener = UnixDatagram::bind(&path).expect("rebind isolated socket");
        client
            .ready()
            .expect("retry after notification destination returns");
        let mut message = [0; MAX_NOTIFY_MESSAGE];
        let (size, _) = listener.recv_from(&mut message).expect("receive retry");
        assert_eq!(&message[..size], b"READY=1");
    }

    #[test]
    fn oversized_and_non_protocol_notifications_are_rejected() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("notify.sock");
        let _listener = UnixDatagram::bind(&path).expect("bind isolated socket");
        let client =
            NotificationClient::for_test(path.as_os_str().to_os_string()).expect("validate socket");
        assert_eq!(
            client.send_message(&[b'x'; MAX_NOTIFY_MESSAGE + 1]),
            Err(NotificationError::OversizedMessage)
        );
        assert_eq!(
            client.send_message(b"STATUS=private data"),
            Err(NotificationError::InvalidMessage)
        );
    }

    #[test]
    fn arbitrary_user_filesystem_notification_sockets_are_rejected() {
        let directory = tempdir().expect("temporary notification directory");
        let path = directory.path().join("unrelated.sock");
        let _listener = UnixDatagram::bind(&path).expect("bind unrelated private socket");
        assert_eq!(
            NotificationClient::from_value(path.as_os_str().to_os_string()).unwrap_err(),
            NotificationError::InvalidAddress
        );
    }
}
