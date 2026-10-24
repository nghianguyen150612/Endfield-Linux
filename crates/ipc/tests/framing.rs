use endfield_unix_ipc::{FrameError, MAX_FRAME_SIZE, read_frame, write_frame};

#[test]
fn rejects_zero_declared_length_and_oversized_declared_length_before_allocation() {
    assert_eq!(
        read_frame(&mut &[0, 0, 0, 0][..]),
        Err(FrameError::ZeroLength)
    );
    let oversized = (MAX_FRAME_SIZE as u32 + 1).to_be_bytes();
    assert_eq!(
        read_frame(&mut oversized.as_slice()),
        Err(FrameError::Oversized)
    );
}

#[test]
fn rejects_truncated_payload_invalid_utf8_and_oversized_actual_payload() {
    assert_eq!(
        read_frame(&mut &[0, 0, 0, 4, b'{'][..]),
        Err(FrameError::Truncated)
    );
    assert_eq!(
        read_frame(&mut &[0, 0, 0, 1, 0xff][..]),
        Err(FrameError::InvalidUtf8)
    );
    assert_eq!(
        write_frame(&mut Vec::new(), &vec![b'x'; MAX_FRAME_SIZE + 1]),
        Err(FrameError::Oversized)
    );
}

#[test]
fn accepts_the_inclusive_maximum_payload_size() {
    let payload = vec![b'x'; MAX_FRAME_SIZE];
    let mut wire = Vec::new();
    write_frame(&mut wire, &payload).expect("maximum-sized frame");
    assert_eq!(read_frame(&mut wire.as_slice()), Ok(payload));
}
