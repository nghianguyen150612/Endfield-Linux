use endfield_unix_platform_linux::{
    FrameInputErrorKind, FrameRecording, FrameSourceFormat, FrameSourceVersion,
    parse_frame_recording,
};

const FIXTURE: &[u8] = include_bytes!("fixtures/frame-events-v1.csv");

#[test]
fn parses_versioned_event_csv_with_nonzero_initial_time() {
    let recording = parse_frame_recording(FIXTURE, FrameSourceFormat::FrameEventsV1)
        .expect("valid canonical event file");
    let FrameRecording::EventTimeline(timeline) = recording else {
        panic!("expected event timeline");
    };
    assert_eq!(
        timeline.metadata().version(),
        FrameSourceVersion::CanonicalV1
    );
    assert_eq!(timeline.events().len(), 3);
    assert_eq!(timeline.events()[0].frame_index(), 7);
    assert_eq!(timeline.events()[0].elapsed_ns(), 123_456_789);
    assert_eq!(timeline.metadata().missing_record_count(), Some(0));
    assert_eq!(timeline.metadata().discontinuity_count(), Some(0));
}

#[test]
fn accepts_csv_quoted_numeric_fields_and_preserves_frame_event_values() {
    let input = b"# endfield-frame-events-v1\nframe_index,elapsed_ns\n\"12\",\"450000000\"\n\"13\",\"460000000\"\n";
    let recording = parse_frame_recording(input.as_slice(), FrameSourceFormat::FrameEventsV1)
        .expect("RFC CSV quoting is supported");
    let FrameRecording::EventTimeline(timeline) = recording else {
        panic!("expected event timeline");
    };
    assert_eq!(timeline.events()[1].frame_index(), 13);
    assert_eq!(timeline.events()[1].elapsed_ns(), 460_000_000);
}

#[test]
fn gaps_are_counted_without_reconstructing_missing_intervals() {
    let input = b"# endfield-frame-events-v1\nframe_index,elapsed_ns\n4,10000000\n5,20000000\n8,70000000\n9,80000000\n";
    let recording = parse_frame_recording(input.as_slice(), FrameSourceFormat::FrameEventsV1)
        .expect("a forward gap is retained as incomplete data");
    let FrameRecording::EventTimeline(timeline) = recording else {
        panic!("expected event timeline");
    };
    assert_eq!(timeline.metadata().missing_record_count(), Some(2));
    assert_eq!(timeline.metadata().discontinuity_count(), Some(1));
    assert_eq!(timeline.events().len(), 4);
}

#[test]
fn duplicate_indexes_and_nonmonotonic_values_are_rejected() {
    for rows in [
        "1,100\n1,200\n",
        "2,100\n1,200\n",
        "1,100\n2,100\n",
        "1,200\n2,100\n",
        "0,0\n1,1\n",
    ] {
        let input = format!("# endfield-frame-events-v1\nframe_index,elapsed_ns\n{rows}");
        let error = parse_frame_recording(input.as_bytes(), FrameSourceFormat::FrameEventsV1)
            .expect_err("invalid indexes and timestamps must not be repaired");
        assert_eq!(error.kind(), FrameInputErrorKind::Malformed);
    }
}

#[test]
fn unsupported_magic_and_mismatched_headers_are_distinguished() {
    let unknown = b"# endfield-frame-events-v2\nframe_index,elapsed_ns\n1,1\n";
    assert_eq!(
        parse_frame_recording(unknown.as_slice(), FrameSourceFormat::FrameEventsV1)
            .expect_err("unknown version")
            .kind(),
        FrameInputErrorKind::UnsupportedFormat
    );
    let bad_header = b"# endfield-frame-events-v1\nframe_index,time\n1,1\n";
    assert_eq!(
        parse_frame_recording(bad_header.as_slice(), FrameSourceFormat::FrameEventsV1)
            .expect_err("bad schema")
            .kind(),
        FrameInputErrorKind::Malformed
    );
}

#[test]
fn blank_and_truncated_event_files_are_rejected() {
    assert_eq!(
        parse_frame_recording(&b""[..], FrameSourceFormat::FrameEventsV1)
            .expect_err("empty file has no selected schema")
            .kind(),
        FrameInputErrorKind::UnsupportedFormat
    );
    for input in [
        &b"# endfield-frame-events-v1\n"[..],
        &b"# endfield-frame-events-v1\nframe_index,elapsed_ns\n"[..],
        &b"# endfield-frame-events-v1\nframe_index,elapsed_ns\n1,100\n2\n"[..],
    ] {
        assert_eq!(
            parse_frame_recording(input, FrameSourceFormat::FrameEventsV1)
                .expect_err("incomplete event file")
                .kind(),
            FrameInputErrorKind::Malformed
        );
    }
}
