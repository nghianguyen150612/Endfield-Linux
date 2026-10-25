//! Validated Linux CPU counter-delta calculations.

use crate::cpu_stat::CpuCounters;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuUtilizationError {
    AccountingLayoutChanged,
    CounterReset,
    ZeroTotalDelta,
    IdleDeltaExceedsTotal,
    ArithmeticOverflow,
}

/// Computes busy CPU time over one counter interval. Linux guest and
/// guest_nice counters are checked for monotonicity but excluded from total
/// time because Linux already includes them in user and nice respectively.
/// iowait is counted as idle, matching the common Linux reporting convention.
pub fn calculate_cpu_delta_basis_points(
    earlier: CpuCounters,
    later: CpuCounters,
) -> Result<u64, CpuUtilizationError> {
    if earlier.field_count() != later.field_count() {
        return Err(CpuUtilizationError::AccountingLayoutChanged);
    }
    let mut deltas = [0_u128; 10];
    for (index, (start, end)) in earlier.fields().iter().zip(later.fields()).enumerate() {
        deltas[index] = u128::from(
            end.checked_sub(*start)
                .ok_or(CpuUtilizationError::CounterReset)?,
        );
    }

    let accounting_count = usize::min(earlier.field_count(), 8);
    let total_delta = deltas[..accounting_count]
        .iter()
        .try_fold(0_u128, |sum, delta| sum.checked_add(*delta))
        .ok_or(CpuUtilizationError::ArithmeticOverflow)?;
    if total_delta == 0 {
        return Err(CpuUtilizationError::ZeroTotalDelta);
    }

    let mut idle_delta = deltas[3];
    if earlier.field_count() >= 5 {
        idle_delta = idle_delta
            .checked_add(deltas[4])
            .ok_or(CpuUtilizationError::ArithmeticOverflow)?;
    }
    if idle_delta > total_delta {
        return Err(CpuUtilizationError::IdleDeltaExceedsTotal);
    }
    let busy_delta = total_delta - idle_delta;
    let scaled = busy_delta
        .checked_mul(10_000)
        .ok_or(CpuUtilizationError::ArithmeticOverflow)?;
    let quotient = scaled / total_delta;
    let remainder = scaled % total_delta;
    // Explicit round-to-nearest, exact halves upward.
    let rounded = if remainder >= total_delta.div_ceil(2) {
        quotient
            .checked_add(1)
            .ok_or(CpuUtilizationError::ArithmeticOverflow)?
    } else {
        quotient
    };
    u64::try_from(rounded).map_err(|_| CpuUtilizationError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu_stat::parse_proc_stat;

    fn record(line: &str) -> CpuCounters {
        parse_proc_stat(line.as_bytes())
            .unwrap()
            .aggregate
            .unwrap()
            .unwrap()
    }

    fn utilization(start: &str, end: &str) -> Result<u64, CpuUtilizationError> {
        calculate_cpu_delta_basis_points(record(start), record(end))
    }

    #[test]
    fn computes_idle_quarter_half_and_full_utilization() {
        assert_eq!(utilization("cpu 0 0 0 0", "cpu 0 0 0 100"), Ok(0));
        assert_eq!(utilization("cpu 0 0 0 0", "cpu 25 0 0 75"), Ok(2_500));
        assert_eq!(utilization("cpu 0 0 0 0", "cpu 50 0 0 50"), Ok(5_000));
        assert_eq!(utilization("cpu 0 0 0 0", "cpu 100 0 0 0"), Ok(10_000));
    }

    #[test]
    fn counts_iowait_as_idle_and_uses_explicit_nearest_rounding() {
        assert_eq!(utilization("cpu 0 0 0 0 0", "cpu 25 0 0 50 25"), Ok(2_500));
        assert_eq!(utilization("cpu 0 0 0 0", "cpu 1 0 0 19"), Ok(500));
        assert_eq!(utilization("cpu 0 0 0 0", "cpu 1 0 0 19999"), Ok(1));
    }

    #[test]
    fn zero_deltas_resets_and_accounting_layout_changes_are_invalid() {
        assert_eq!(
            utilization("cpu 5 0 0 5", "cpu 5 0 0 5"),
            Err(CpuUtilizationError::ZeroTotalDelta)
        );
        assert_eq!(
            utilization("cpu 5 0 0 5", "cpu 4 0 0 6"),
            Err(CpuUtilizationError::CounterReset)
        );
        assert_eq!(
            utilization("cpu 0 0 0 0", "cpu 1 0 0 0 0"),
            Err(CpuUtilizationError::AccountingLayoutChanged)
        );
    }

    #[test]
    fn guest_fields_are_not_added_twice_and_all_fields_are_checked() {
        assert_eq!(
            utilization("cpu 0 0 0 0 0 0 0 0 0 0", "cpu 100 0 0 0 0 0 0 0 80 0"),
            Ok(10_000)
        );
        assert_eq!(
            utilization("cpu 0 0 0 0 0 0 0 0 4 0", "cpu 10 0 0 0 0 0 0 0 3 0"),
            Err(CpuUtilizationError::CounterReset)
        );
    }

    #[test]
    fn very_large_valid_counters_use_wide_checked_intermediates() {
        assert_eq!(
            utilization(
                "cpu 0 0 0 0 0 0 0 0",
                "cpu 18446744073709551615 0 0 18446744073709551615 0 0 0 0"
            ),
            Ok(5_000)
        );
    }
}
