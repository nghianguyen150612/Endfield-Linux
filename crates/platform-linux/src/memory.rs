//! Typed, bounded parser for Linux `/proc/meminfo` values.

pub const MAX_MEMINFO_BYTES: usize = 64 * 1024;
const MAX_MEMINFO_LINE_BYTES: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemInfoParseError {
    TooLarge,
    InvalidUtf8,
    LineTooLong,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemInfoFieldError {
    InvalidInteger,
    InvalidUnit,
    Overflow,
    DuplicateField,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemInfoData {
    /// Values are normalized to bytes from the kernel's 1024-byte `kB` unit.
    pub mem_total_bytes: Option<Result<u64, MemInfoFieldError>>,
    pub mem_available_bytes: Option<Result<u64, MemInfoFieldError>>,
    pub swap_total_bytes: Option<Result<u64, MemInfoFieldError>>,
    pub swap_free_bytes: Option<Result<u64, MemInfoFieldError>>,
}

/// Parses only the fields used by P012. The first duplicate makes that field
/// invalid while unrelated fields remain usable; input ordering is otherwise
/// irrelevant.
pub fn parse_meminfo(bytes: &[u8]) -> Result<MemInfoData, MemInfoParseError> {
    if bytes.len() > MAX_MEMINFO_BYTES {
        return Err(MemInfoParseError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| MemInfoParseError::InvalidUtf8)?;
    let mut parsed = MemInfoData::default();
    for line in text.lines() {
        if line.len() > MAX_MEMINFO_LINE_BYTES {
            return Err(MemInfoParseError::LineTooLong);
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let target = match key {
            "MemTotal" => &mut parsed.mem_total_bytes,
            "MemAvailable" => &mut parsed.mem_available_bytes,
            "SwapTotal" => &mut parsed.swap_total_bytes,
            "SwapFree" => &mut parsed.swap_free_bytes,
            _ => continue,
        };
        if target.is_some() {
            *target = Some(Err(MemInfoFieldError::DuplicateField));
        } else {
            *target = Some(parse_meminfo_value(value));
        }
    }
    Ok(parsed)
}

fn parse_meminfo_value(value: &str) -> Result<u64, MemInfoFieldError> {
    let fields = value.split_ascii_whitespace().collect::<Vec<_>>();
    if fields.len() != 2 || fields[1] != "kB" {
        return Err(MemInfoFieldError::InvalidUnit);
    }
    if fields[0].starts_with('-') || fields[0].starts_with('+') || fields[0].is_empty() {
        return Err(MemInfoFieldError::InvalidInteger);
    }
    let kib = fields[0]
        .parse::<u64>()
        .map_err(|_| MemInfoFieldError::InvalidInteger)?;
    kib.checked_mul(1024).ok_or(MemInfoFieldError::Overflow)
}
