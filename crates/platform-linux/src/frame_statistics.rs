//! Checked integer statistics over compatible frame-duration observations.

use crate::frame_sources::FrameInputError;

pub const MIN_P99_FRAME_INTERVALS: usize = 100;

pub fn measured_frame_rate_milli(
    observed_intervals: u64,
    elapsed_ns: u64,
) -> Result<u64, FrameInputError> {
    if observed_intervals == 0 || elapsed_ns == 0 {
        return Err(FrameInputError::malformed());
    }
    let numerator = u128::from(observed_intervals)
        .checked_mul(1_000_000_000_000)
        .ok_or_else(FrameInputError::malformed)?;
    let denominator = u128::from(elapsed_ns);
    round_nearest_ties_up(numerator, denominator)
}

pub fn arithmetic_mean_nanos(durations: &[u64]) -> Result<Option<u64>, FrameInputError> {
    if durations.len() < 2 || durations.contains(&0) {
        return Ok(None);
    }
    let sum = durations.iter().try_fold(0u128, |sum, duration| {
        sum.checked_add(u128::from(*duration))
            .ok_or_else(FrameInputError::malformed)
    })?;
    let count = u128::try_from(durations.len()).map_err(|_| FrameInputError::malformed())?;
    Ok(Some(round_nearest_ties_up(sum, count)?))
}

/// Nearest-rank P99: sorted[ceil(99*N/100)-1]. At least 100 valid values are
/// required so the result is not presented as a useful percentile for a tiny
/// sample. This is P99 frametime, not 1% low FPS.
pub fn nearest_rank_p99_nanos(durations: &[u64]) -> Result<Option<u64>, FrameInputError> {
    if durations.len() < MIN_P99_FRAME_INTERVALS || durations.contains(&0) {
        return Ok(None);
    }
    let count = u64::try_from(durations.len()).map_err(|_| FrameInputError::malformed())?;
    let scaled_rank = count
        .checked_mul(99)
        .ok_or_else(FrameInputError::malformed)?;
    let rank = scaled_rank
        .checked_add(99)
        .ok_or_else(FrameInputError::malformed)?
        / 100;
    let index = usize::try_from(rank - 1).map_err(|_| FrameInputError::malformed())?;
    let mut sorted = durations.to_vec();
    sorted.sort_unstable();
    Ok(sorted.get(index).copied())
}

fn round_nearest_ties_up(numerator: u128, denominator: u128) -> Result<u64, FrameInputError> {
    if denominator == 0 {
        return Err(FrameInputError::malformed());
    }
    let mut quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if remainder
        .checked_mul(2)
        .ok_or_else(FrameInputError::malformed)?
        >= denominator
    {
        quotient = quotient
            .checked_add(1)
            .ok_or_else(FrameInputError::malformed)?;
    }
    u64::try_from(quotient).map_err(|_| FrameInputError::malformed())
}
