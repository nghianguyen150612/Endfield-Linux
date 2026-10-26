//! Verified MangoHud CSV variants supported by P018.

use std::io::Read;

use csv::{ByteRecord, Reader};

use crate::frame_csv::{MAX_FRAME_INTERVALS, csv_reader, validate_utf8};
use crate::frame_sources::{
    FrameDataSource, FrameInputError, FrameMeasurementSemantics, FrameObservationGranularity,
    FrameSourceFormat, FrameSourceMetadata, FrameSourceMetadataConfig, FrameSourceVersion,
    FrameTimestampUnit,
};

const SYSTEM_INFO_HEADER: [&[u8]; 7] = [
    b"os",
    b"cpu",
    b"gpu",
    b"ram",
    b"kernel",
    b"driver",
    b"cpuscheduler",
];
const FRAME_METRICS_HEADER: [&[u8]; 16] = [
    b"fps",
    b"frametime",
    b"cpu_load",
    b"cpu_power",
    b"gpu_load",
    b"cpu_temp",
    b"gpu_temp",
    b"gpu_core_clock",
    b"gpu_mem_clock",
    b"gpu_vram_used",
    b"gpu_power",
    b"ram_used",
    b"swap_used",
    b"process_rss",
    b"cpu_mhz",
    b"elapsed",
];
const SYSTEM_INFO_SEPARATOR: &[u8] = b"---------------------SYSTEM INFO---------------------";
const FRAME_METRICS_SEPARATOR: &[u8] = b"--------------------FRAME METRICS--------------------";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MangoHudFrameSample {
    record_index: u32,
    elapsed_ns: u64,
    frametime_ns: u64,
    source_fps_milli: u64,
}

impl MangoHudFrameSample {
    pub const fn record_index(self) -> u32 {
        self.record_index
    }

    pub const fn elapsed_ns(self) -> u64 {
        self.elapsed_ns
    }

    /// Duration in nanoseconds of the last frame-to-frame interval observed
    /// when MangoHud wrote this sampled row.
    pub const fn frametime_ns(self) -> u64 {
        self.frametime_ns
    }

    /// MangoHud's displayed instantaneous reciprocal of the last frametime,
    /// represented in milli-FPS. It is not a rate over the recording window.
    pub const fn source_fps_milli(self) -> u64 {
        self.source_fps_milli
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MangoHudRecording {
    samples: Vec<MangoHudFrameSample>,
    metadata: FrameSourceMetadata,
}

impl MangoHudRecording {
    pub fn samples(&self) -> &[MangoHudFrameSample] {
        &self.samples
    }

    pub const fn metadata(&self) -> FrameSourceMetadata {
        self.metadata
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MangoHudCsvSource;

impl FrameDataSource for MangoHudCsvSource {
    fn format(&self) -> FrameSourceFormat {
        FrameSourceFormat::MangoHudCsv
    }

    fn parse(&self, input: &mut dyn Read) -> Result<super::FrameRecording, FrameInputError> {
        let mut reader = csv_reader(input);
        parse_mangohud(&mut reader).map(super::FrameRecording::MangoHud)
    }
}

fn parse_mangohud<R: Read>(reader: &mut Reader<R>) -> Result<MangoHudRecording, FrameInputError> {
    let mut record = ByteRecord::new();
    if !next_record(reader, &mut record)? {
        return Err(FrameInputError::unsupported());
    }
    let version = match record.get(0) {
        Some(value) if value == b"v1" && record.len() == 1 => {
            if !next_record(reader, &mut record)? || record.len() != 1 {
                return Err(FrameInputError::malformed());
            }
            // The package version is checked as UTF-8 above and then discarded
            // to avoid retaining arbitrary header metadata.
            if record.get(0).is_none_or(<[u8]>::is_empty) {
                return Err(FrameInputError::malformed());
            }
            if !next_record(reader, &mut record)?
                || record.len() != 1
                || record.get(0) != Some(SYSTEM_INFO_SEPARATOR)
            {
                return Err(FrameInputError::malformed());
            }
            FrameSourceVersion::MangoHudVersionedV1
        }
        Some(value) if value.starts_with(b"v") => return Err(FrameInputError::unsupported()),
        _ => FrameSourceVersion::MangoHudLegacyHeader,
    };

    if version == FrameSourceVersion::MangoHudLegacyHeader {
        validate_system_header(&record)?;
    } else {
        if !next_record(reader, &mut record)? {
            return Err(FrameInputError::malformed());
        }
        validate_system_header(&record)?;
    }

    if !next_record(reader, &mut record)? {
        return Err(FrameInputError::malformed());
    }
    // System values contain private machine and application metadata. Check
    // syntax/UTF-8, but never copy or expose those values.

    if version == FrameSourceVersion::MangoHudVersionedV1
        && (!next_record(reader, &mut record)?
            || record.len() != 1
            || record.get(0) != Some(FRAME_METRICS_SEPARATOR))
    {
        return Err(FrameInputError::malformed());
    }

    if !next_record(reader, &mut record)? {
        return Err(FrameInputError::malformed());
    }
    if !header_matches(&record, &FRAME_METRICS_HEADER) {
        return Err(FrameInputError::malformed());
    }

    let mut samples = Vec::with_capacity(MAX_FRAME_INTERVALS.min(1024));
    let mut previous_elapsed = None;
    while next_record(reader, &mut record)? {
        if record.len() != FRAME_METRICS_HEADER.len() || samples.len() >= MAX_FRAME_INTERVALS {
            return Err(FrameInputError::malformed());
        }
        validate_utf8(&record)?;
        let source_fps_milli =
            parse_decimal_scaled(record.get(0).ok_or_else(FrameInputError::malformed)?, 1_000)?;
        let frametime_ns = parse_decimal_scaled(
            record.get(1).ok_or_else(FrameInputError::malformed)?,
            1_000_000,
        )?;
        let elapsed_ns = parse_u64(record.get(15).ok_or_else(FrameInputError::malformed)?)?;
        if source_fps_milli == 0 || frametime_ns == 0 {
            return Err(FrameInputError::malformed());
        }
        if previous_elapsed.is_some_and(|previous| elapsed_ns <= previous) {
            return Err(FrameInputError::malformed());
        }
        let record_index =
            u32::try_from(samples.len() + 1).map_err(|_| FrameInputError::malformed())?;
        samples.push(MangoHudFrameSample {
            record_index,
            elapsed_ns,
            frametime_ns,
            source_fps_milli,
        });
        previous_elapsed = Some(elapsed_ns);
    }
    if samples.is_empty() {
        return Err(FrameInputError::malformed());
    }

    let count = u32::try_from(samples.len()).map_err(|_| FrameInputError::malformed())?;
    let metadata = FrameSourceMetadata::new(FrameSourceMetadataConfig {
        format: FrameSourceFormat::MangoHudCsv,
        version,
        semantics: FrameMeasurementSemantics::SampledLastFrameIntervalAndInstantaneousFps,
        granularity: FrameObservationGranularity::SampledIndividualFrameIntervals,
        record_count: count,
        timestamp_unit: FrameTimestampUnit::Nanoseconds,
        source_sampling_interval_ns: None,
        missing_record_count: None,
        discontinuity_count: None,
        invalid_record_count: 0,
        source_fps_sample_count: Some(count),
    });
    Ok(MangoHudRecording { samples, metadata })
}

fn next_record<R: Read>(
    reader: &mut Reader<R>,
    record: &mut ByteRecord,
) -> Result<bool, FrameInputError> {
    let has_record = crate::frame_csv::read_record(reader, record)?;
    if has_record {
        crate::frame_csv::validate_utf8(record)?;
        if record.len() > crate::frame_csv::MAX_CSV_FIELDS {
            return Err(FrameInputError::malformed());
        }
    }
    Ok(has_record)
}

fn validate_system_header(record: &ByteRecord) -> Result<(), FrameInputError> {
    if header_matches(record, &SYSTEM_INFO_HEADER) {
        Ok(())
    } else {
        Err(FrameInputError::unsupported())
    }
}

fn header_matches(record: &ByteRecord, expected: &[&[u8]]) -> bool {
    record.len() == expected.len()
        && record
            .iter()
            .zip(expected.iter().copied())
            .all(|(actual, expected)| actual == expected)
}

fn parse_u64(bytes: &[u8]) -> Result<u64, FrameInputError> {
    if bytes.is_empty() || bytes.len() > 20 || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(FrameInputError::malformed());
    }
    std::str::from_utf8(bytes)
        .map_err(|_| FrameInputError::malformed())?
        .parse::<u64>()
        .map_err(|_| FrameInputError::malformed())
}

/// Parses a non-negative decimal and scales it using round-to-nearest, ties
/// upward. Exponents and locale-specific separators are intentionally outside
/// the supported source schema.
fn parse_decimal_scaled(bytes: &[u8], scale: u64) -> Result<u64, FrameInputError> {
    if bytes.is_empty() || bytes.len() > 32 || scale == 0 {
        return Err(FrameInputError::malformed());
    }
    let mut split = bytes.split(|byte| *byte == b'.');
    let whole = split.next().ok_or_else(FrameInputError::malformed)?;
    let fraction = split.next();
    if split.next().is_some()
        || whole.is_empty()
        || !whole.iter().all(u8::is_ascii_digit)
        || fraction.is_some_and(|part| part.is_empty() || !part.iter().all(u8::is_ascii_digit))
    {
        return Err(FrameInputError::malformed());
    }
    let whole = parse_decimal_digits(whole)?;
    let mut scaled = whole
        .checked_mul(u128::from(scale))
        .ok_or_else(FrameInputError::malformed)?;
    if let Some(fraction) = fraction {
        if fraction.len() > 18 {
            return Err(FrameInputError::malformed());
        }
        let fractional_value = parse_decimal_digits(fraction)?;
        let divisor = 10u128
            .checked_pow(u32::try_from(fraction.len()).map_err(|_| FrameInputError::malformed())?)
            .ok_or_else(FrameInputError::malformed)?;
        let numerator = fractional_value
            .checked_mul(u128::from(scale))
            .ok_or_else(FrameInputError::malformed)?;
        let mut fraction_scaled = numerator / divisor;
        if (numerator % divisor).saturating_mul(2) >= divisor {
            fraction_scaled = fraction_scaled
                .checked_add(1)
                .ok_or_else(FrameInputError::malformed)?;
        }
        scaled = scaled
            .checked_add(fraction_scaled)
            .ok_or_else(FrameInputError::malformed)?;
    }
    u64::try_from(scaled).map_err(|_| FrameInputError::malformed())
}

fn parse_decimal_digits(bytes: &[u8]) -> Result<u128, FrameInputError> {
    let mut value = 0u128;
    for digit in bytes {
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u128::from(digit - b'0')))
            .ok_or_else(FrameInputError::malformed)?;
    }
    Ok(value)
}
