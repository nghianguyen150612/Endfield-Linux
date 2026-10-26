use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use endfield_unix_platform_linux::{
    FrameInputErrorKind, FrameSourceFormat, MAX_FRAME_INPUT_BYTES, read_frame_recording,
};
use nix::sys::stat::Mode;
use nix::unistd::mkfifo;
use tempfile::tempdir;

const FIXTURE: &[u8] = include_bytes!("fixtures/frame-events-v1.csv");

#[test]
fn opens_only_regular_files_and_rejects_symlinks() {
    let directory = tempdir().expect("temporary directory");
    let valid = directory.path().join("valid.csv");
    fs::write(&valid, FIXTURE).expect("write fixture");
    let recording = read_frame_recording(&valid, FrameSourceFormat::FrameEventsV1)
        .expect("regular file is accepted");
    assert_eq!(recording.metadata().record_count(), 3);

    let link = directory.path().join("link.csv");
    symlink(&valid, &link).expect("create final-component symlink");
    let error = read_frame_recording(&link, FrameSourceFormat::FrameEventsV1)
        .expect_err("final symlink is refused");
    assert_eq!(error.kind(), FrameInputErrorKind::Io);

    let folder = read_frame_recording(directory.path(), FrameSourceFormat::FrameEventsV1)
        .expect_err("directory is not a regular input file");
    assert_eq!(folder.kind(), FrameInputErrorKind::Malformed);
}

#[test]
fn fifo_is_rejected_without_waiting_for_a_writer() {
    let directory = tempdir().expect("temporary directory");
    let fifo = directory.path().join("frames.pipe");
    mkfifo(&fifo, Mode::S_IRUSR | Mode::S_IWUSR).expect("create test FIFO");
    let error = read_frame_recording(&fifo, FrameSourceFormat::FrameEventsV1)
        .expect_err("non-regular input is refused");
    assert_eq!(error.kind(), FrameInputErrorKind::Malformed);
}

#[test]
fn oversized_sparse_file_is_rejected_from_descriptor_metadata() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("oversized.csv");
    let file = fs::File::create(&path).expect("create sparse file");
    file.set_len(MAX_FRAME_INPUT_BYTES + 1)
        .expect("set sparse size without materializing file bytes");
    let error = read_frame_recording(&path, FrameSourceFormat::FrameEventsV1)
        .expect_err("files over 64 MiB are rejected");
    assert_eq!(error.kind(), FrameInputErrorKind::Malformed);
}

#[test]
fn oversized_csv_records_and_field_counts_are_bounded_during_streaming_parse() {
    let too_long = format!(
        "# endfield-frame-events-v1\nframe_index,elapsed_ns\n{}\n",
        "1".repeat(65 * 1024)
    );
    let error = endfield_unix_platform_linux::parse_frame_recording(
        too_long.as_bytes(),
        FrameSourceFormat::FrameEventsV1,
    )
    .expect_err("oversized logical CSV row");
    assert_eq!(error.kind(), FrameInputErrorKind::Malformed);

    let too_many_fields = format!(
        "# endfield-frame-events-v1\nframe_index,elapsed_ns\n{}\n",
        vec!["1"; 65].join(",")
    );
    let error = endfield_unix_platform_linux::parse_frame_recording(
        too_many_fields.as_bytes(),
        FrameSourceFormat::FrameEventsV1,
    )
    .expect_err("too many CSV fields");
    assert_eq!(error.kind(), FrameInputErrorKind::Malformed);
}

#[test]
fn directory_contents_are_never_discovered() {
    let directory = tempdir().expect("temporary directory");
    fs::write(directory.path().join("valid.csv"), FIXTURE).expect("write fixture");
    assert!(Path::new(directory.path()).is_dir());
    assert_eq!(
        read_frame_recording(directory.path(), FrameSourceFormat::FrameEventsV1)
            .expect_err("directories are not scanned")
            .kind(),
        FrameInputErrorKind::Malformed
    );
}
