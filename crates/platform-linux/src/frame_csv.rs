//! Shared bounded CSV reader for user-supplied frame logs.

use std::io::{self, Read};

use csv::{ByteRecord, Reader, ReaderBuilder};

use crate::frame_sources::{FrameInputError, FrameInputErrorKind};

pub const MAX_FRAME_INPUT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_CSV_RECORD_BYTES: usize = 64 * 1024;
pub const MAX_CSV_FIELDS: usize = 64;
pub const MAX_FRAME_INTERVALS: usize = 4093;
pub const MAX_FRAME_EVENT_RECORDS: usize = MAX_FRAME_INTERVALS + 1;

/// Counts bytes and fields before the CSV parser can grow a record without
/// bound. Quoted commas and newlines are handled for RFC 4180 double-quote
/// escaping; malformed quoting is subsequently rejected by `csv`.
pub(crate) struct BoundedCsvRead<R> {
    inner: R,
    total_bytes: u64,
    record_bytes: usize,
    fields: usize,
    in_quotes: bool,
    quote_pending: bool,
    at_field_start: bool,
}

impl<R> BoundedCsvRead<R> {
    pub(crate) const fn new(inner: R) -> Self {
        Self {
            inner,
            total_bytes: 0,
            record_bytes: 0,
            fields: 1,
            in_quotes: false,
            quote_pending: false,
            at_field_start: true,
        }
    }

    fn invalid_limit() -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, "frame CSV limit exceeded")
    }

    fn inspect(&mut self, bytes: &[u8]) -> io::Result<()> {
        for byte in bytes {
            self.total_bytes = self.total_bytes.saturating_add(1);
            self.record_bytes = self.record_bytes.saturating_add(1);
            if self.total_bytes > MAX_FRAME_INPUT_BYTES || self.record_bytes > MAX_CSV_RECORD_BYTES
            {
                return Err(Self::invalid_limit());
            }

            if self.quote_pending {
                self.quote_pending = false;
                if *byte == b'"' {
                    continue;
                }
                self.in_quotes = false;
            }

            if self.in_quotes {
                if *byte == b'"' {
                    self.quote_pending = true;
                }
                continue;
            }

            match *byte {
                b'"' if self.at_field_start => {
                    self.in_quotes = true;
                    self.at_field_start = false;
                }
                b',' => {
                    self.fields = self.fields.saturating_add(1);
                    if self.fields > MAX_CSV_FIELDS {
                        return Err(Self::invalid_limit());
                    }
                    self.at_field_start = true;
                }
                b'\n' => {
                    self.record_bytes = 0;
                    self.fields = 1;
                    self.at_field_start = true;
                }
                _ => self.at_field_start = false,
            }
        }
        Ok(())
    }
}

impl<R: Read> Read for BoundedCsvRead<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.inspect(&buffer[..count])?;
        Ok(count)
    }
}

pub(crate) fn csv_reader<R: Read>(input: R) -> Reader<R> {
    let mut builder = ReaderBuilder::new();
    builder.has_headers(false).flexible(true);
    builder.from_reader(input)
}

pub(crate) fn read_record<R: Read>(
    reader: &mut Reader<R>,
    record: &mut ByteRecord,
) -> Result<bool, FrameInputError> {
    reader.read_byte_record(record).map_err(map_csv_error)
}

pub(crate) fn validate_utf8(record: &ByteRecord) -> Result<(), FrameInputError> {
    for field in record.iter() {
        std::str::from_utf8(field).map_err(|_| FrameInputError::malformed())?;
    }
    Ok(())
}

fn map_csv_error(error: csv::Error) -> FrameInputError {
    match error.kind() {
        csv::ErrorKind::Io(io_error) if io_error.kind() != io::ErrorKind::InvalidData => {
            FrameInputError::new(FrameInputErrorKind::Io)
        }
        _ => FrameInputError::malformed(),
    }
}
