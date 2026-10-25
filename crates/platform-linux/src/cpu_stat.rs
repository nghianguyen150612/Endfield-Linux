//! Bounded parser for Linux aggregate and per-logical-CPU `/proc/stat` records.

use std::collections::{BTreeMap, btree_map::Entry};

pub const MAX_PROC_STAT_BYTES: usize = 256 * 1024;
pub const MAX_PROC_STAT_LINE_BYTES: usize = 512;
pub const MAX_CPU_STAT_RECORDS: usize = 4096;
const MIN_COUNTERS: usize = 4;
const MAX_COUNTERS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuStatParseError {
    TooLarge,
    InvalidUtf8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuRecordError {
    InvalidIdentity,
    MissingMandatoryCounters,
    LineTooLong,
    TooManyCounters,
    InvalidCounter,
    DuplicateRecord,
}

/// A single CPU accounting record. Only counters present in the source line
/// are retained; absent optional fields are never synthesized as zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuCounters {
    counters: [u64; MAX_COUNTERS],
    field_count: u8,
}

impl CpuCounters {
    pub fn fields(&self) -> &[u64] {
        &self.counters[..usize::from(self.field_count)]
    }

    pub const fn field_count(&self) -> usize {
        self.field_count as usize
    }
}

/// Parsed records are keyed by the kernel CPU ID, never by line/vector order.
/// Record-local errors do not discard valid aggregate or other CPU records.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CpuStatSnapshot {
    pub aggregate: Option<Result<CpuCounters, CpuRecordError>>,
    pub logical_cpus: BTreeMap<u32, Result<CpuCounters, CpuRecordError>>,
    /// True when the bounded parser saw more logical CPU records than it can
    /// retain. Consumers must drop per-core output as a whole in this case.
    pub records_exceeded: bool,
    /// CPU-like labels whose numeric identity could not be safely resolved.
    pub invalid_identities: usize,
}

/// Parses the CPU records from `/proc/stat`. Other procfs records are ignored.
pub fn parse_proc_stat(bytes: &[u8]) -> Result<CpuStatSnapshot, CpuStatParseError> {
    if bytes.len() > MAX_PROC_STAT_BYTES {
        return Err(CpuStatParseError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| CpuStatParseError::InvalidUtf8)?;
    let mut snapshot = CpuStatSnapshot::default();
    let mut logical_records_seen = 0usize;

    for line in text.lines() {
        let Some(label) = line.split_ascii_whitespace().next() else {
            continue;
        };
        if label == "cpu" {
            let parsed = if line.len() > MAX_PROC_STAT_LINE_BYTES {
                Err(CpuRecordError::LineTooLong)
            } else {
                parse_counters(line)
            };
            snapshot.aggregate = Some(match snapshot.aggregate.take() {
                Some(_) => Err(CpuRecordError::DuplicateRecord),
                None => parsed,
            });
            continue;
        }
        let Some(suffix) = label.strip_prefix("cpu") else {
            continue;
        };
        if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
            if !suffix.is_empty() {
                snapshot.invalid_identities = snapshot.invalid_identities.saturating_add(1);
            }
            continue;
        }
        let cpu_id = match suffix.parse::<u32>() {
            Ok(cpu_id) if cpu_id.to_string() == suffix => cpu_id,
            _ => {
                snapshot.invalid_identities = snapshot.invalid_identities.saturating_add(1);
                continue;
            }
        };
        logical_records_seen = logical_records_seen.saturating_add(1);
        if logical_records_seen > MAX_CPU_STAT_RECORDS {
            snapshot.records_exceeded = true;
            continue;
        }
        let parsed = if line.len() > MAX_PROC_STAT_LINE_BYTES {
            Err(CpuRecordError::LineTooLong)
        } else {
            parse_counters(line)
        };
        match snapshot.logical_cpus.entry(cpu_id) {
            Entry::Occupied(mut entry) => {
                let _ = entry.insert(Err(CpuRecordError::DuplicateRecord));
            }
            Entry::Vacant(entry) => {
                entry.insert(parsed);
            }
        }
    }
    Ok(snapshot)
}

fn parse_counters(line: &str) -> Result<CpuCounters, CpuRecordError> {
    let mut fields = line.split_ascii_whitespace();
    let _label = fields
        .next()
        .ok_or(CpuRecordError::MissingMandatoryCounters)?;
    let mut counters = [0_u64; MAX_COUNTERS];
    let mut field_count = 0usize;
    for field in fields {
        if field_count == MAX_COUNTERS {
            return Err(CpuRecordError::TooManyCounters);
        }
        if field.is_empty() || !field.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(CpuRecordError::InvalidCounter);
        }
        counters[field_count] = field.parse().map_err(|_| CpuRecordError::InvalidCounter)?;
        field_count += 1;
    }
    if field_count < MIN_COUNTERS {
        return Err(CpuRecordError::MissingMandatoryCounters);
    }
    Ok(CpuCounters {
        counters,
        field_count: field_count as u8,
    })
}

/// Computes busy percentage from one pair of counter records, normalized to
/// basis points. iowait counts as idle. Linux guest counters are excluded from
/// the sum because they are already included in user/nice accounting.
pub fn calculate_cpu_utilization_basis_points(
    earlier: CpuCounters,
    later: CpuCounters,
) -> Result<u64, crate::cpu_telemetry::CpuUtilizationError> {
    crate::cpu_telemetry::calculate_cpu_delta_basis_points(earlier, later)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(values: &[u64]) -> CpuCounters {
        let mut raw = String::from("cpu");
        for value in values {
            raw.push_str(&format!(" {value}"));
        }
        parse_counters(&raw).unwrap()
    }

    #[test]
    fn parses_aggregate_sparse_cpu_ids_guest_fields_and_ignores_other_records() {
        let snapshot = parse_proc_stat(
            b"cpu 100 2 30 400 10 3 4 5 8 1\ncpu0 10 0 2 80 2 0 1 0 2 0\ncpu7 20 1 4 90 3 1 0 0 3 1\nintr 9 1 2\n",
        )
        .unwrap();
        assert_eq!(snapshot.aggregate.unwrap().unwrap().field_count(), 10);
        assert_eq!(
            snapshot.logical_cpus.keys().copied().collect::<Vec<_>>(),
            vec![0, 7]
        );
        assert_eq!(snapshot.logical_cpus[&7].as_ref().unwrap().fields()[8], 3);
        assert!(!snapshot.records_exceeded);
    }

    #[test]
    fn duplicate_and_malformed_cpu_records_do_not_erase_other_records() {
        let snapshot = parse_proc_stat(
            b"cpu 1 0 0 0\ncpu0 1 0 0 0\ncpu0 2 0 0 0\ncpu4 1 x 0 0\ncpu9 1 0 0 0\n",
        )
        .unwrap();
        assert_eq!(snapshot.aggregate.unwrap().unwrap().fields(), &[1, 0, 0, 0]);
        assert_eq!(
            snapshot.logical_cpus[&0],
            Err(CpuRecordError::DuplicateRecord)
        );
        assert_eq!(
            snapshot.logical_cpus[&4],
            Err(CpuRecordError::InvalidCounter)
        );
        assert!(snapshot.logical_cpus[&9].is_ok());
    }

    #[test]
    fn malformed_aggregate_and_duplicate_aggregate_are_explicit() {
        let malformed = parse_proc_stat(b"cpu 1 2 3\ncpu0 1 2 3 4\n").unwrap();
        assert_eq!(
            malformed.aggregate,
            Some(Err(CpuRecordError::MissingMandatoryCounters))
        );
        let duplicate = parse_proc_stat(b"cpu 1 2 3 4\ncpu 1 2 3 4\n").unwrap();
        assert_eq!(
            duplicate.aggregate,
            Some(Err(CpuRecordError::DuplicateRecord))
        );
        assert_eq!(parse_proc_stat(b"cpu0 1 2 3 4\n").unwrap().aggregate, None);
    }

    #[test]
    fn strict_integer_and_identity_parsing_rejects_overflow_and_noncanonical_ids() {
        let snapshot = parse_proc_stat(
            b"cpu 18446744073709551616 0 0 0\ncpu01 1 0 0 0\ncpu4294967296 1 0 0 0\ncpux 1 0 0 0\n",
        )
        .unwrap();
        assert_eq!(
            snapshot.aggregate,
            Some(Err(CpuRecordError::InvalidCounter))
        );
        assert!(snapshot.logical_cpus.is_empty());
        assert_eq!(snapshot.invalid_identities, 3);
    }

    #[test]
    fn parser_enforces_byte_line_and_utf8_bounds() {
        assert_eq!(
            parse_proc_stat(&vec![b' '; MAX_PROC_STAT_BYTES + 1]),
            Err(CpuStatParseError::TooLarge)
        );
        let mut line = b"cpu 1 0 0 0 ".to_vec();
        line.extend(std::iter::repeat_n(b'1', MAX_PROC_STAT_LINE_BYTES));
        let parsed = parse_proc_stat(&line).unwrap();
        assert_eq!(parsed.aggregate, Some(Err(CpuRecordError::LineTooLong)));
        assert_eq!(
            parse_proc_stat(&[0xff]),
            Err(CpuStatParseError::InvalidUtf8)
        );
    }

    #[test]
    fn long_unrelated_proc_stat_lines_are_ignored_within_the_file_bound() {
        let mut input = String::from("cpu 1 0 0 0\nintr ");
        input.push_str(&"1 ".repeat(MAX_PROC_STAT_LINE_BYTES));
        input.push('\n');
        let parsed = parse_proc_stat(input.as_bytes()).unwrap();
        assert_eq!(parsed.aggregate.unwrap().unwrap().fields(), &[1, 0, 0, 0]);
    }

    #[test]
    fn cpu_record_overflow_is_explicit_and_retained_records_are_not_a_partial_sample() {
        let mut input = String::from("cpu 1 0 0 0\n");
        for cpu in 0..=MAX_CPU_STAT_RECORDS {
            input.push_str(&format!("cpu{cpu} 1 0 0 0\n"));
        }
        let snapshot = parse_proc_stat(input.as_bytes()).unwrap();
        assert!(snapshot.records_exceeded);
        assert_eq!(snapshot.logical_cpus.len(), MAX_CPU_STAT_RECORDS);
    }

    #[test]
    fn optional_fields_are_not_filled_and_guest_time_is_not_double_counted() {
        let earlier = record(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let later = record(&[100, 0, 0, 0, 0, 0, 0, 0, 80, 0]);
        assert_eq!(later.field_count(), 10);
        assert_eq!(
            calculate_cpu_utilization_basis_points(earlier, later),
            Ok(10_000)
        );

        let short = record(&[1, 2, 3, 4]);
        assert_eq!(short.fields(), &[1, 2, 3, 4]);

        let too_many = parse_proc_stat(b"cpu 1 2 3 4 5 6 7 8 9 10 11\n").unwrap();
        assert_eq!(
            too_many.aggregate,
            Some(Err(CpuRecordError::TooManyCounters))
        );
    }
}
