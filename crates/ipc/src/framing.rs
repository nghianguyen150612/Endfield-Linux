use std::io::{self, Read, Write};

pub const MAX_FRAME_SIZE: usize = 16 * 1024;

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    #[error("frame declared a zero length")]
    ZeroLength,
    #[error("frame exceeded the configured size limit")]
    Oversized,
    #[error("frame was truncated")]
    Truncated,
    #[error("frame was not valid UTF-8")]
    InvalidUtf8,
    #[error("frame transport failed")]
    Io,
}

pub fn read_frame(reader: &mut impl Read) -> Result<Vec<u8>, FrameError> {
    let mut header = [0u8; 4];
    read_exact_mapped(reader, &mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 {
        return Err(FrameError::ZeroLength);
    }
    if length > MAX_FRAME_SIZE {
        return Err(FrameError::Oversized);
    }
    let mut payload = vec![0u8; length];
    read_exact_mapped(reader, &mut payload)?;
    std::str::from_utf8(&payload).map_err(|_| FrameError::InvalidUtf8)?;
    Ok(payload)
}

pub fn write_frame(writer: &mut impl Write, payload: &[u8]) -> Result<(), FrameError> {
    if payload.is_empty() {
        return Err(FrameError::ZeroLength);
    }
    if payload.len() > MAX_FRAME_SIZE || payload.len() > u32::MAX as usize {
        return Err(FrameError::Oversized);
    }
    writer
        .write_all(&(payload.len() as u32).to_be_bytes())
        .and_then(|()| writer.write_all(payload))
        .map_err(|error| {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                FrameError::Truncated
            } else {
                FrameError::Io
            }
        })
}

fn read_exact_mapped(reader: &mut impl Read, buffer: &mut [u8]) -> Result<(), FrameError> {
    match reader.read_exact(buffer) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Err(FrameError::Truncated),
        Err(_) => Err(FrameError::Io),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_network_order_bounded_frames() {
        let payload = b"ping";
        let mut wire = Vec::new();
        write_frame(&mut wire, payload).expect("write frame");
        assert_eq!(&wire[..4], &[0, 0, 0, 4]);
        assert_eq!(read_frame(&mut wire.as_slice()), Ok(payload.to_vec()));
    }

    #[test]
    fn rejects_zero_oversized_truncated_and_invalid_utf8_frames() {
        assert_eq!(
            read_frame(&mut &[0, 0, 0, 0][..]),
            Err(FrameError::ZeroLength)
        );
        assert_eq!(
            read_frame(&mut &[0, 0, 0x40, 1][..]),
            Err(FrameError::Oversized)
        );
        assert_eq!(
            read_frame(&mut &[0, 0, 0, 3, b'{'][..]),
            Err(FrameError::Truncated)
        );
        assert_eq!(
            read_frame(&mut &[0, 0, 0, 1, 0xff][..]),
            Err(FrameError::InvalidUtf8)
        );
    }

    #[test]
    fn handles_partial_reads_and_writes() {
        struct PartialReader<'a>(&'a [u8]);
        impl Read for PartialReader<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if self.0.is_empty() {
                    return Ok(0);
                }
                let count = buffer.len().min(self.0.len()).min(1);
                buffer[..count].copy_from_slice(&self.0[..count]);
                self.0 = &self.0[count..];
                Ok(count)
            }
        }

        struct PartialWriter(Vec<u8>);
        impl Write for PartialWriter {
            fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
                let count = buffer.len().min(1);
                self.0.extend_from_slice(&buffer[..count]);
                Ok(count)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut output = PartialWriter(Vec::new());
        write_frame(&mut output, b"partial").expect("partial writes complete");
        assert_eq!(
            read_frame(&mut PartialReader(&output.0)),
            Ok(b"partial".to_vec())
        );
    }
}
