use endfield_unix_platform_linux::{
    FrameInputErrorKind, FrameMeasurementSemantics, FrameObservationGranularity, FrameRecording,
    FrameSourceFormat, FrameSourceVersion, parse_frame_recording,
};

const VERSIONED: &[u8] = include_bytes!("fixtures/mangohud-versioned-v1.csv");
const LEGACY: &[u8] = include_bytes!("fixtures/mangohud-legacy.csv");

#[test]
fn parses_the_upstream_versioned_mangohud_schema_without_retaining_private_headers() {
    let recording = parse_frame_recording(VERSIONED, FrameSourceFormat::MangoHudCsv)
        .expect("source-backed MangoHud v1 schema fixture");
    let FrameRecording::MangoHud(recording) = recording else {
        panic!("expected MangoHud recording");
    };
    assert_eq!(
        recording.metadata().version(),
        FrameSourceVersion::MangoHudVersionedV1
    );
    assert_eq!(recording.metadata().record_count(), 3);
    assert_eq!(recording.metadata().source_fps_sample_count(), Some(3));
    assert_eq!(
        recording.metadata().semantics(),
        FrameMeasurementSemantics::SampledLastFrameIntervalAndInstantaneousFps
    );
    assert_eq!(
        recording.metadata().granularity(),
        FrameObservationGranularity::SampledIndividualFrameIntervals
    );
    assert_eq!(recording.metadata().source_sampling_interval_ns(), None);
    assert_eq!(recording.samples()[0].frametime_ns(), 16_666_700);
    assert_eq!(recording.samples()[0].source_fps_milli(), 60_000);
    assert_eq!(recording.samples()[2].elapsed_ns(), 40_000_000);
}

#[test]
fn parses_the_upstream_unversioned_header_variant() {
    let recording = parse_frame_recording(LEGACY, FrameSourceFormat::MangoHudCsv)
        .expect("source-backed legacy MangoHud schema fixture");
    let FrameRecording::MangoHud(recording) = recording else {
        panic!("expected MangoHud recording");
    };
    assert_eq!(
        recording.metadata().version(),
        FrameSourceVersion::MangoHudLegacyHeader
    );
    assert_eq!(recording.samples().len(), 2);
}

#[test]
fn quoted_system_metadata_is_parsed_but_never_retained() {
    let input = String::from_utf8(VERSIONED.to_vec())
        .expect("fixture utf8")
        .replace("fixture-cpu", "\"private, cpu label\"");
    let recording = parse_frame_recording(input.as_bytes(), FrameSourceFormat::MangoHudCsv)
        .expect("quoted comma in metadata is valid CSV");
    assert!(matches!(recording, FrameRecording::MangoHud(_)));
    let rendered = format!("{recording:?}");
    assert!(!rendered.contains("private, cpu label"));
    assert!(!rendered.contains("fixture-build"));
}

#[test]
fn unsupported_summary_future_version_and_wrong_header_are_not_guessed() {
    let summary = b"0.1% Min FPS,1% Min FPS,Average FPS\n0,0,60\n";
    assert_eq!(
        parse_frame_recording(summary.as_slice(), FrameSourceFormat::MangoHudCsv)
            .expect_err("aggregated summary is not a frame log")
            .kind(),
        FrameInputErrorKind::UnsupportedFormat
    );
    let future = VERSIONED.to_vec();
    let future = String::from_utf8(future)
        .unwrap()
        .replacen("v1\n", "v2\n", 1);
    assert_eq!(
        parse_frame_recording(future.as_bytes(), FrameSourceFormat::MangoHudCsv)
            .expect_err("future version not supported")
            .kind(),
        FrameInputErrorKind::UnsupportedFormat
    );
    let header = String::from_utf8(VERSIONED.to_vec())
        .unwrap()
        .replace("fps,frametime", "framerate,frametime");
    assert_eq!(
        parse_frame_recording(header.as_bytes(), FrameSourceFormat::MangoHudCsv)
            .expect_err("missing required fps field")
            .kind(),
        FrameInputErrorKind::Malformed
    );
}

#[test]
fn malformed_numeric_units_timestamps_and_row_width_are_rejected() {
    for (from, to) in [
        ("60,16.6667", "60,-16.6667"),
        ("60,16.6667", "60,0"),
        ("60,16.6667", "nan,16.6667"),
        ("60,16.6667", "0,16.6667"),
    ] {
        let input = String::from_utf8(VERSIONED.to_vec())
            .unwrap()
            .replacen(from, to, 1);
        assert_eq!(
            parse_frame_recording(input.as_bytes(), FrameSourceFormat::MangoHudCsv)
                .expect_err("invalid metric row")
                .kind(),
            FrameInputErrorKind::Malformed
        );
    }
    let duplicate = String::from_utf8(VERSIONED.to_vec()).unwrap().replace(
        "30,33.3333,0,0,0,0,0,0,0,0,0,0,0,0,0,30000000",
        "30,33.3333,0,0,0,0,0,0,0,0,0,0,0,0,0,10000000",
    );
    assert_eq!(
        parse_frame_recording(duplicate.as_bytes(), FrameSourceFormat::MangoHudCsv)
            .expect_err("duplicate elapsed timestamp")
            .kind(),
        FrameInputErrorKind::Malformed
    );
}

#[test]
fn invalid_utf8_and_unexpected_columns_are_rejected() {
    let mut invalid = VERSIONED.to_vec();
    invalid.push(0xff);
    assert_eq!(
        parse_frame_recording(invalid.as_slice(), FrameSourceFormat::MangoHudCsv)
            .expect_err("invalid utf8")
            .kind(),
        FrameInputErrorKind::Malformed
    );
    let extra = String::from_utf8(VERSIONED.to_vec()).unwrap().replace(
        "60,16.6667,0,0,0,0,0,0,0,0,0,0,0,0,0,10000000",
        "60,16.6667,0,0,0,0,0,0,0,0,0,0,0,0,0,10000000,extra",
    );
    assert_eq!(
        parse_frame_recording(extra.as_bytes(), FrameSourceFormat::MangoHudCsv)
            .expect_err("extra row field")
            .kind(),
        FrameInputErrorKind::Malformed
    );
}
