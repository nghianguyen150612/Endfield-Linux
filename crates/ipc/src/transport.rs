use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Instant;

use nix::errno::Errno;
use nix::poll::{PollFd, PollFlags, poll};
use nix::sys::socket::{
    AddressFamily, SockFlag, SockType, UnixAddr, connect, getsockopt, socket, sockopt,
};

use crate::framing::{FrameError, MAX_FRAME_SIZE};

#[derive(Debug)]
pub(crate) enum TimedTransportError {
    Timeout,
    Frame(FrameError),
    Io,
}

pub(crate) fn connect_bounded(path: &Path, deadline: Instant) -> io::Result<UnixStream> {
    let socket_path_bytes = path.as_os_str().as_encoded_bytes();
    if socket_path_bytes.len() + 1 > 108 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "socket path too long",
        ));
    }
    let address = UnixAddr::new(path)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid socket path"))?;
    let fd = socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK,
        None,
    )
    .map_err(errno_to_io)?;

    match connect(fd.as_raw_fd(), &address) {
        Ok(()) => {}
        Err(Errno::EINPROGRESS | Errno::EALREADY | Errno::EAGAIN) => {
            let mut poll_fds = [PollFd::new(fd.as_fd(), PollFlags::POLLOUT)];
            loop {
                let Some(timeout) = remaining_poll_timeout(deadline) else {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "connect timed out"));
                };
                match poll(&mut poll_fds, timeout) {
                    Ok(0) => {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "connect timed out"));
                    }
                    Ok(_) => break,
                    Err(Errno::EINTR) => continue,
                    Err(error) => return Err(errno_to_io(error)),
                }
            }
            let socket_error: i32 = getsockopt(&fd, sockopt::SocketError).map_err(errno_to_io)?;
            if socket_error != 0 {
                return Err(io::Error::from_raw_os_error(socket_error));
            }
        }
        Err(error) => return Err(errno_to_io(error)),
    }

    let stream = UnixStream::from(fd);
    stream.set_nonblocking(false)?;
    Ok(stream)
}

pub(crate) fn read_frame_until(
    stream: &mut UnixStream,
    deadline: Instant,
) -> Result<Vec<u8>, TimedTransportError> {
    let mut header = [0u8; 4];
    read_exact_until(stream, &mut header, deadline)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 {
        return Err(TimedTransportError::Frame(FrameError::ZeroLength));
    }
    if length > MAX_FRAME_SIZE {
        return Err(TimedTransportError::Frame(FrameError::Oversized));
    }
    let mut payload = vec![0u8; length];
    read_exact_until(stream, &mut payload, deadline)?;
    std::str::from_utf8(&payload)
        .map_err(|_| TimedTransportError::Frame(FrameError::InvalidUtf8))?;
    Ok(payload)
}

pub(crate) fn write_frame_until(
    stream: &mut UnixStream,
    payload: &[u8],
    deadline: Instant,
) -> Result<(), TimedTransportError> {
    if payload.is_empty() {
        return Err(TimedTransportError::Frame(FrameError::ZeroLength));
    }
    if payload.len() > MAX_FRAME_SIZE || payload.len() > u32::MAX as usize {
        return Err(TimedTransportError::Frame(FrameError::Oversized));
    }
    write_all_until(stream, &(payload.len() as u32).to_be_bytes(), deadline)?;
    write_all_until(stream, payload, deadline)
}

fn read_exact_until(
    stream: &mut UnixStream,
    buffer: &mut [u8],
    deadline: Instant,
) -> Result<(), TimedTransportError> {
    let mut read = 0;
    while read < buffer.len() {
        let timeout = remaining_duration(deadline).ok_or(TimedTransportError::Timeout)?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|_| TimedTransportError::Io)?;
        match stream.read(&mut buffer[read..]) {
            Ok(0) => return Err(TimedTransportError::Frame(FrameError::Truncated)),
            Ok(count) => read += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(TimedTransportError::Timeout);
            }
            Err(_) => return Err(TimedTransportError::Io),
        }
    }
    Ok(())
}

fn write_all_until(
    stream: &mut UnixStream,
    buffer: &[u8],
    deadline: Instant,
) -> Result<(), TimedTransportError> {
    let mut written = 0;
    while written < buffer.len() {
        let timeout = remaining_duration(deadline).ok_or(TimedTransportError::Timeout)?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|_| TimedTransportError::Io)?;
        match stream.write(&buffer[written..]) {
            Ok(0) => return Err(TimedTransportError::Io),
            Ok(count) => written += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(TimedTransportError::Timeout);
            }
            Err(_) => return Err(TimedTransportError::Io),
        }
    }
    Ok(())
}

fn remaining_duration(deadline: Instant) -> Option<std::time::Duration> {
    let remaining = deadline.checked_duration_since(Instant::now())?;
    (!remaining.is_zero()).then_some(remaining)
}

fn remaining_poll_timeout(deadline: Instant) -> Option<u16> {
    let remaining = remaining_duration(deadline)?;
    let millis = remaining
        .as_millis()
        .saturating_add(1)
        .clamp(1, u16::MAX as u128);
    Some(millis as u16)
}

fn errno_to_io(error: Errno) -> io::Error {
    io::Error::from_raw_os_error(error as i32)
}
