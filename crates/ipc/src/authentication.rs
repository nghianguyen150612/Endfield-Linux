use std::os::unix::net::UnixStream;

use crate::errors::ServerConnectionError;

/// Applies the daemon's same-effective-user policy to a kernel-provided peer UID.
pub fn peer_uid_is_authorized(expected_uid: u32, peer_uid: u32) -> bool {
    expected_uid == peer_uid
}

/// Obtains the connected peer UID through the current platform's kernel API.
#[cfg(target_os = "linux")]
pub(crate) fn authenticated_peer_uid(stream: &UnixStream) -> Result<u32, ServerConnectionError> {
    use nix::sys::socket::{getsockopt, sockopt};

    getsockopt(stream, sockopt::PeerCredentials)
        .map(|credentials| credentials.uid())
        .map_err(|_| ServerConnectionError::AuthenticationDenied)
}

/// Other Unix platforms need a separately validated credential adapter.
#[cfg(not(target_os = "linux"))]
pub(crate) fn authenticated_peer_uid(_stream: &UnixStream) -> Result<u32, ServerConnectionError> {
    Err(ServerConnectionError::UnsupportedPlatform)
}
