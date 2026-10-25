//! P016 read-only memory gauge derivation from P012's bounded meminfo parser.

use crate::memory::{MemInfoFieldError, MemInfoParseError, parse_meminfo};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemoryValue {
    Missing,
    Valid(u64),
    Invalid(MemInfoFieldError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LinuxMemoryTelemetryFacts {
    pub ram_total_bytes: MemoryValue,
    pub ram_available_bytes: MemoryValue,
    pub estimated_ram_used_bytes: MemoryValue,
    pub swap_used_bytes: MemoryValue,
}

/// Parses and derives memory gauges without turning missing or malformed
/// values into zero. `MemAvailable` remains distinct from `MemFree`.
pub(crate) fn collect_memory_telemetry(
    bytes: &[u8],
) -> Result<LinuxMemoryTelemetryFacts, MemInfoParseError> {
    let parsed = parse_meminfo(bytes)?;
    let ram_total_bytes = match field_value(parsed.mem_total_bytes) {
        MemoryValue::Valid(0) => MemoryValue::Invalid(MemInfoFieldError::InconsistentValues),
        value => value,
    };
    let ram_available_bytes = field_value(parsed.mem_available_bytes);
    let (ram_available_bytes, estimated_ram_used_bytes) =
        match (ram_total_bytes, ram_available_bytes) {
            (MemoryValue::Valid(total), MemoryValue::Valid(available)) => {
                if available > total {
                    let invalid = MemoryValue::Invalid(MemInfoFieldError::InconsistentValues);
                    (invalid, invalid)
                } else {
                    (
                        MemoryValue::Valid(available),
                        MemoryValue::Valid(total - available),
                    )
                }
            }
            (MemoryValue::Invalid(error), available) => (available, MemoryValue::Invalid(error)),
            (_, MemoryValue::Invalid(error)) => {
                let invalid = MemoryValue::Invalid(error);
                (invalid, invalid)
            }
            (_, available) => (available, MemoryValue::Missing),
        };

    let swap_total_bytes = field_value(parsed.swap_total_bytes);
    let swap_free_bytes = field_value(parsed.swap_free_bytes);
    let swap_used_bytes = match (swap_total_bytes, swap_free_bytes) {
        (MemoryValue::Valid(total), MemoryValue::Valid(free)) if free <= total => {
            MemoryValue::Valid(total - free)
        }
        (MemoryValue::Valid(_), MemoryValue::Valid(_)) => {
            MemoryValue::Invalid(MemInfoFieldError::InconsistentValues)
        }
        (MemoryValue::Invalid(error), _) | (_, MemoryValue::Invalid(error)) => {
            MemoryValue::Invalid(error)
        }
        _ => MemoryValue::Missing,
    };

    Ok(LinuxMemoryTelemetryFacts {
        ram_total_bytes,
        ram_available_bytes,
        estimated_ram_used_bytes,
        swap_used_bytes,
    })
}

fn field_value(field: Option<Result<u64, MemInfoFieldError>>) -> MemoryValue {
    match field {
        Some(Ok(value)) => MemoryValue::Valid(value),
        Some(Err(error)) => MemoryValue::Invalid(error),
        None => MemoryValue::Missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_available_estimated_used_and_swap_used_in_bytes() {
        let facts = collect_memory_telemetry(
            b"MemTotal: 1024 kB\nMemAvailable: 256 kB\nSwapTotal: 512 kB\nSwapFree: 128 kB\n",
        )
        .unwrap();
        assert_eq!(facts.ram_total_bytes, MemoryValue::Valid(1024 * 1024));
        assert_eq!(facts.ram_available_bytes, MemoryValue::Valid(256 * 1024));
        assert_eq!(
            facts.estimated_ram_used_bytes,
            MemoryValue::Valid(768 * 1024)
        );
        assert_eq!(facts.swap_used_bytes, MemoryValue::Valid(384 * 1024));
    }

    #[test]
    fn valid_zero_swap_is_distinct_from_missing_swap_fields() {
        let zero = collect_memory_telemetry(
            b"MemTotal: 10 kB\nMemAvailable: 8 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n",
        )
        .unwrap();
        assert_eq!(zero.swap_used_bytes, MemoryValue::Valid(0));
        let missing = collect_memory_telemetry(b"MemTotal: 10 kB\nMemAvailable: 8 kB\n").unwrap();
        assert_eq!(missing.swap_used_bytes, MemoryValue::Missing);
    }

    #[test]
    fn inconsistent_free_available_and_total_relations_are_rejected() {
        let available = collect_memory_telemetry(
            b"MemTotal: 10 kB\nMemAvailable: 11 kB\nSwapTotal: 1 kB\nSwapFree: 1 kB\n",
        )
        .unwrap();
        assert_eq!(
            available.ram_available_bytes,
            MemoryValue::Invalid(MemInfoFieldError::InconsistentValues)
        );
        assert_eq!(
            available.estimated_ram_used_bytes,
            MemoryValue::Invalid(MemInfoFieldError::InconsistentValues)
        );

        let swap = collect_memory_telemetry(
            b"MemTotal: 10 kB\nMemAvailable: 8 kB\nSwapTotal: 1 kB\nSwapFree: 2 kB\n",
        )
        .unwrap();
        assert_eq!(
            swap.swap_used_bytes,
            MemoryValue::Invalid(MemInfoFieldError::InconsistentValues)
        );
    }

    #[test]
    fn malformed_units_and_missing_values_are_not_zero() {
        let facts =
            collect_memory_telemetry(b"MemTotal: 10 MB\nMemAvailable: 8 kB\nSwapTotal: 1 kB\n")
                .unwrap();
        assert_eq!(
            facts.ram_total_bytes,
            MemoryValue::Invalid(MemInfoFieldError::InvalidUnit)
        );
        assert_eq!(
            facts.estimated_ram_used_bytes,
            MemoryValue::Invalid(MemInfoFieldError::InvalidUnit)
        );
        assert_eq!(facts.swap_used_bytes, MemoryValue::Missing);

        let missing_available =
            collect_memory_telemetry(b"MemTotal: 10 kB\nSwapTotal: 1 kB\nSwapFree: 0 kB\n")
                .unwrap();
        assert_eq!(missing_available.ram_available_bytes, MemoryValue::Missing);
        assert_eq!(
            missing_available.estimated_ram_used_bytes,
            MemoryValue::Missing
        );
    }

    #[test]
    fn ram_used_underflow_and_meminfo_byte_overflow_are_rejected() {
        let underflow = collect_memory_telemetry(
            b"MemTotal: 10 kB\nMemAvailable: 11 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n",
        )
        .unwrap();
        assert_eq!(
            underflow.estimated_ram_used_bytes,
            MemoryValue::Invalid(MemInfoFieldError::InconsistentValues)
        );

        let overflow = collect_memory_telemetry(
            b"MemTotal: 18446744073709551615 kB\nMemAvailable: 1 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n",
        )
        .unwrap();
        assert_eq!(
            overflow.ram_total_bytes,
            MemoryValue::Invalid(MemInfoFieldError::Overflow)
        );

        let zero_total = collect_memory_telemetry(
            b"MemTotal: 0 kB\nMemAvailable: 0 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n",
        )
        .unwrap();
        assert_eq!(
            zero_total.ram_total_bytes,
            MemoryValue::Invalid(MemInfoFieldError::InconsistentValues)
        );
        assert_eq!(
            zero_total.estimated_ram_used_bytes,
            MemoryValue::Invalid(MemInfoFieldError::InconsistentValues)
        );
    }
}
