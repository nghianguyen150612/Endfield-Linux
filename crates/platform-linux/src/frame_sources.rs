//! Explicitly selected, bounded readers for imported frame-performance logs.

use std::fs::OpenOptions;
use std::io::{self, Read};
use std::path::Path;

use crate::frame_csv::{BoundedCsvRead, MAX_FRAME_INPUT_BYTES};
use crate::frame_events::{FrameEvent, FrameEventsV1Source, FrameTimeline};
use crate::frame_mangohud::{MangoHudCsvSource, MangoHudFrameSample, MangoHudRecording};

/// Supported user-selected file formats. File extensions are never used to
/// choose a parser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameSourceFormat {
    FrameEventsV1,
    MangoHudCsv,
}

impl FrameSourceFormat {
    pub const fn stable_name(self) -> &'static str {
        match self {
            Self::FrameEventsV1 => "frame-events-v1",
            Self::MangoHudCsv => "mangohud-csv",
        }
    }
}

/// The timing values present in a source, without assuming that unlike values
/// describe the same event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameMeasurementSemantics {
    PresentedFrameEventTimeline,
    SampledLastFrameIntervalAndInstantaneousFps,
    SampledFpsOnly,
    AveragedFrametime,
    AggregatedBenchmarkStatistics,
    Unknown,
}

/// How much of a frame timeline one source record represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameObservationGranularity {
    IndividualPresentedFrameEvents,
    SampledIndividualFrameIntervals,
    SampledRateValues,
    AggregatedStatistics,
}

/// Version metadata is an allowlisted enum. Raw headers and application
/// identity fields are never retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameSourceVersion {
    CanonicalV1,
    MangoHudLegacyHeader,
    MangoHudVersionedV1,
}

/// Fixed, privacy-safe metadata describing one parsed recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSourceMetadata {
    format: FrameSourceFormat,
    version: FrameSourceVersion,
    semantics: FrameMeasurementSemantics,
    granularity: FrameObservationGranularity,
    record_count: u32,
    timestamp_unit: FrameTimestampUnit,
    source_sampling_interval_ns: Option<u64>,
    missing_record_count: Option<u32>,
    discontinuity_count: Option<u32>,
    invalid_record_count: u32,
    source_fps_sample_count: Option<u32>,
}

pub(crate) struct FrameSourceMetadataConfig {
    pub(crate) format: FrameSourceFormat,
    pub(crate) version: FrameSourceVersion,
    pub(crate) semantics: FrameMeasurementSemantics,
    pub(crate) granularity: FrameObservationGranularity,
    pub(crate) record_count: u32,
    pub(crate) timestamp_unit: FrameTimestampUnit,
    pub(crate) source_sampling_interval_ns: Option<u64>,
    pub(crate) missing_record_count: Option<u32>,
    pub(crate) discontinuity_count: Option<u32>,
    pub(crate) invalid_record_count: u32,
    pub(crate) source_fps_sample_count: Option<u32>,
}

impl FrameSourceMetadata {
    pub(crate) const fn new(config: FrameSourceMetadataConfig) -> Self {
        Self {
            format: config.format,
            version: config.version,
            semantics: config.semantics,
            granularity: config.granularity,
            record_count: config.record_count,
            timestamp_unit: config.timestamp_unit,
            source_sampling_interval_ns: config.source_sampling_interval_ns,
            missing_record_count: config.missing_record_count,
            discontinuity_count: config.discontinuity_count,
            invalid_record_count: config.invalid_record_count,
            source_fps_sample_count: config.source_fps_sample_count,
        }
    }

    pub const fn format(self) -> FrameSourceFormat {
        self.format
    }

    pub const fn version(self) -> FrameSourceVersion {
        self.version
    }

    pub const fn semantics(self) -> FrameMeasurementSemantics {
        self.semantics
    }

    pub const fn granularity(self) -> FrameObservationGranularity {
        self.granularity
    }

    pub const fn record_count(self) -> u32 {
        self.record_count
    }

    pub const fn timestamp_unit(self) -> FrameTimestampUnit {
        self.timestamp_unit
    }

    pub const fn source_sampling_interval_ns(self) -> Option<u64> {
        self.source_sampling_interval_ns
    }

    pub const fn missing_record_count(self) -> Option<u32> {
        self.missing_record_count
    }

    pub const fn discontinuity_count(self) -> Option<u32> {
        self.discontinuity_count
    }

    pub const fn invalid_record_count(self) -> u32 {
        self.invalid_record_count
    }

    pub const fn source_fps_sample_count(self) -> Option<u32> {
        self.source_fps_sample_count
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameTimestampUnit {
    Nanoseconds,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameRecording {
    EventTimeline(FrameTimeline),
    MangoHud(MangoHudRecording),
}

impl FrameRecording {
    pub const fn metadata(&self) -> FrameSourceMetadata {
        match self {
            Self::EventTimeline(timeline) => timeline.metadata(),
            Self::MangoHud(recording) => recording.metadata(),
        }
    }

    pub fn frame_events(&self) -> Option<&[FrameEvent]> {
        match self {
            Self::EventTimeline(timeline) => Some(timeline.events()),
            Self::MangoHud(_) => None,
        }
    }

    pub fn mangohud_samples(&self) -> Option<&[MangoHudFrameSample]> {
        match self {
            Self::EventTimeline(_) => None,
            Self::MangoHud(recording) => Some(recording.samples()),
        }
    }
}

/// Typed parser failures contain no input bytes, source path, or raw upstream
/// metadata and are safe to map to fixed CLI diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameInputErrorKind {
    Io,
    UnsupportedFormat,
    Malformed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameInputError {
    kind: FrameInputErrorKind,
}

impl FrameInputError {
    pub(crate) const fn new(kind: FrameInputErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(&self) -> FrameInputErrorKind {
        self.kind
    }

    pub(crate) fn io(_error: io::Error) -> Self {
        Self::new(FrameInputErrorKind::Io)
    }

    pub(crate) const fn malformed() -> Self {
        Self::new(FrameInputErrorKind::Malformed)
    }

    pub(crate) const fn unsupported() -> Self {
        Self::new(FrameInputErrorKind::UnsupportedFormat)
    }
}

/// A format implementation reads only the supplied stream and returns typed,
/// normalized frame observations.
pub trait FrameDataSource {
    fn format(&self) -> FrameSourceFormat;
    fn parse(&self, input: &mut dyn Read) -> Result<FrameRecording, FrameInputError>;
}

pub fn parse_frame_recording<R: Read>(
    input: R,
    format: FrameSourceFormat,
) -> Result<FrameRecording, FrameInputError> {
    let mut bounded = BoundedCsvRead::new(input);
    match format {
        FrameSourceFormat::FrameEventsV1 => FrameEventsV1Source.parse(&mut bounded),
        FrameSourceFormat::MangoHudCsv => MangoHudCsvSource.parse(&mut bounded),
    }
}

/// Opens exactly the explicitly named file. `O_NOFOLLOW` rejects a symlink in
/// the final component, `O_NONBLOCK` prevents FIFOs from hanging the CLI, and
/// metadata is checked on the opened descriptor to avoid a path check/open
/// race. Parsing remains streaming and each CSV record is independently
/// bounded.
pub fn read_frame_recording(
    path: &Path,
    format: FrameSourceFormat,
) -> Result<FrameRecording, FrameInputError> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(FrameInputError::io)?;
    let metadata = file.metadata().map_err(FrameInputError::io)?;
    if !metadata.is_file() || metadata.len() > MAX_FRAME_INPUT_BYTES {
        return Err(FrameInputError::malformed());
    }
    parse_frame_recording(file, format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_ids_are_stable_and_explicit() {
        assert_eq!(
            FrameSourceFormat::FrameEventsV1.stable_name(),
            "frame-events-v1"
        );
        assert_eq!(FrameSourceFormat::MangoHudCsv.stable_name(), "mangohud-csv");
    }
}
