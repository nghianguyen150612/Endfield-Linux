use endfield_unix_platform_linux::{
    MIN_P99_FRAME_INTERVALS, arithmetic_mean_nanos, measured_frame_rate_milli,
    nearest_rank_p99_nanos,
};

#[test]
fn exact_rates_and_fractional_rates_use_milli_fps() {
    assert_eq!(measured_frame_rate_milli(30, 1_000_000_000), Ok(30_000));
    assert_eq!(measured_frame_rate_milli(60, 1_000_000_000), Ok(60_000));
    assert_eq!(measured_frame_rate_milli(100, 1_000_000_000), Ok(100_000));
    assert_eq!(measured_frame_rate_milli(3, 46_000_000), Ok(65_217));
}

#[test]
fn rate_rounds_to_nearest_milli_fps_with_half_ties_upward() {
    // 1e12 / 8192 is exactly 122070312.5 milli-FPS.
    assert_eq!(measured_frame_rate_milli(1, 8_192), Ok(122_070_313));
}

#[test]
fn zero_windows_and_values_that_do_not_fit_are_rejected() {
    assert!(measured_frame_rate_milli(0, 1).is_err());
    assert!(measured_frame_rate_milli(1, 0).is_err());
    assert!(measured_frame_rate_milli(u64::MAX, 1).is_err());
}

#[test]
fn arithmetic_mean_requires_two_samples_and_rounds_ties_upward() {
    assert_eq!(arithmetic_mean_nanos(&[12]), Ok(None));
    assert_eq!(arithmetic_mean_nanos(&[10, 12]), Ok(Some(11)));
    assert_eq!(arithmetic_mean_nanos(&[10, 11]), Ok(Some(11)));
    assert_eq!(arithmetic_mean_nanos(&[0, 10]), Ok(None));
}

#[test]
fn nearest_rank_p99_is_deterministic_and_requires_one_hundred_samples() {
    assert_eq!(MIN_P99_FRAME_INTERVALS, 100);
    assert_eq!(nearest_rank_p99_nanos(&vec![10; 99]), Ok(None));

    let mut values = vec![10_000_000; 98];
    values.extend([100_000_000, 100_000_000]);
    assert_eq!(nearest_rank_p99_nanos(&values), Ok(Some(100_000_000)));
}

#[test]
fn slow_frame_outliers_remain_in_mean_and_percentile_values() {
    let mut values = vec![10_000_000; 98];
    values.extend([100_000_000, 100_000_000]);
    assert_eq!(arithmetic_mean_nanos(&values), Ok(Some(11_800_000)));
    assert_eq!(nearest_rank_p99_nanos(&values), Ok(Some(100_000_000)));
}
