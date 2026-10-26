//! Canonical versioned timestamped frame-event input.

use std::io::Read;

use csv::{ByteRecord, Reader};

use crate::frame_csv::{MAX_FRAME_EVENT_RECORDS, csv_reader, read_record, validate_utf8};
use crate::frame_sources::{
    FrameDataSource, FrameInputError, FrameMeasurementSemantics, FrameObservationGranularity,
    FrameSourceFormat, FrameSourceMetadata, FrameSourceMetadataConfig, FrameSourceVersion,
    FrameTimestampUnit,
};

const MAGIC: &[u8] = b"# endfield-frame-events-v1";
const HEADER: [&[u8]; 2] = [b"frame_index", b"elapsed_ns"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameEvent {
    frame_index: u64,
    elapsed_ns: u64,
}

impl FrameEvent {
    pub const fn frame_index(self) -> u64 {
        self.frame_index
    }

    pub const fn elapsed_ns(self) -> u64 {
        self.elapsed_ns
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameTimeline {
    events: Vec<FrameEvent>,
    metadata: FrameSourceMetadata,
}

impl FrameTimeline {
    pub fn events(&self) -> &[FrameEvent] {
        &self.events
    }

    pub const fn metadata(&self) -> FrameSourceMetadata {
        self.metadata
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameEventsV1Source;

impl FrameDataSource for FrameEventsV1Source {
    fn format(&self) -> FrameSourceFormat {
        FrameSourceFormat::FrameEventsV1
    }

    fn parse(&self, input: &mut dyn Read) -> Result<super::FrameRecording, FrameInputError> {
        let mut reader = csv_reader(input);
        parse_event_timeline(&mut reader).map(super::FrameRecording::EventTimeline)
    }
}

fn parse_event_timeline<R: Read>(reader: &mut Reader<R>) -> Result<FrameTimeline, FrameInputError> {
    let mut record = ByteRecord::new();
    if !read_record(reader, &mut record)? {
        return Err(FrameInputError::unsupported());
    }
    validate_utf8(&record)?;
    if record.len() != 1 || record.get(0) != Some(MAGIC) {
        return Err(FrameInputError::unsupported());
    }

    if !read_record(reader, &mut record)? {
        return Err(FrameInputError::malformed());
    }
    validate_utf8(&record)?;
    if record.len() != HEADER.len()
        || record
            .iter()
            .zip(HEADER)
            .any(|(actual, expected)| actual != expected)
    {
        return Err(FrameInputError::malformed());
    }

    let mut events: Vec<FrameEvent> = Vec::with_capacity(MAX_FRAME_EVENT_RECORDS.min(1024));
    let mut missing_records = 0u32;
    let mut discontinuities = 0u32;
    while read_record(reader, &mut record)? {
        validate_utf8(&record)?;
        if record.len() != 2 || events.len() >= MAX_FRAME_EVENT_RECORDS {
            return Err(FrameInputError::malformed());
        }
        let frame_index = parse_u64(record.get(0).ok_or_else(FrameInputError::malformed)?)?;
        let elapsed_ns = parse_u64(record.get(1).ok_or_else(FrameInputError::malformed)?)?;
        if frame_index == 0 {
            return Err(FrameInputError::malformed());
        }
        if let Some(previous) = events.last().copied() {
            if elapsed_ns <= previous.elapsed_ns || frame_index <= previous.frame_index {
                return Err(FrameInputError::malformed());
            }
            let index_delta = frame_index
                .checked_sub(previous.frame_index)
                .ok_or_else(FrameInputError::malformed)?;
            if index_delta > 1 {
                missing_records = missing_records
                    .checked_add(
                        u32::try_from(index_delta - 1).map_err(|_| FrameInputError::malformed())?,
                    )
                    .ok_or_else(FrameInputError::malformed)?;
                discontinuities = discontinuities
                    .checked_add(1)
                    .ok_or_else(FrameInputError::malformed)?;
            }
        }
        events.push(FrameEvent {
            frame_index,
            elapsed_ns,
        });
    }

    if events.is_empty() {
        return Err(FrameInputError::malformed());
    }

    let metadata = FrameSourceMetadata::new(FrameSourceMetadataConfig {
        format: FrameSourceFormat::FrameEventsV1,
        version: FrameSourceVersion::CanonicalV1,
        semantics: FrameMeasurementSemantics::PresentedFrameEventTimeline,
        granularity: FrameObservationGranularity::IndividualPresentedFrameEvents,
        record_count: u32::try_from(events.len()).map_err(|_| FrameInputError::malformed())?,
        timestamp_unit: FrameTimestampUnit::Nanoseconds,
        source_sampling_interval_ns: None,
        missing_record_count: Some(missing_records),
        discontinuity_count: Some(discontinuities),
        invalid_record_count: 0,
        source_fps_sample_count: None,
    });
    Ok(FrameTimeline { events, metadata })
}

fn parse_u64(bytes: &[u8]) -> Result<u64, FrameInputError> {
    if bytes.is_empty() || bytes.len() > 20 || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(FrameInputError::malformed());
    }
    let value = std::str::from_utf8(bytes)
        .map_err(|_| FrameInputError::malformed())?
        .parse::<u64>()
        .map_err(|_| FrameInputError::malformed())?;
    Ok(value)
}
