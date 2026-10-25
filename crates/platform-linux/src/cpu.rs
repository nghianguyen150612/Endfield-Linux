//! Bounded model identification from the architecture-dependent procfs fields.

pub const MAX_CPUINFO_BYTES: usize = 256 * 1024;
const MAX_CPUINFO_LINE_BYTES: usize = 4096;
const MAX_CPU_MODEL_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuInfoParseError {
    TooLarge,
    InvalidUtf8,
    LineTooLong,
}

/// Returns one unambiguous model string reported by the kernel. Identifiers,
/// clock rates, cache sizes, and marketing-name lookups are intentionally
/// ignored.
pub fn parse_cpu_model(bytes: &[u8]) -> Result<Option<String>, CpuInfoParseError> {
    if bytes.len() > MAX_CPUINFO_BYTES {
        return Err(CpuInfoParseError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| CpuInfoParseError::InvalidUtf8)?;
    let mut candidates = [Vec::<String>::new(), Vec::new(), Vec::new(), Vec::new()];
    for line in text.lines() {
        if line.len() > MAX_CPUINFO_LINE_BYTES {
            return Err(CpuInfoParseError::LineTooLong);
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let priority = match key.trim() {
            "model name" => 0,
            "Hardware" => 1,
            "Model" => 2,
            "Processor" => 3,
            _ => continue,
        };
        let value = value.trim();
        if value.is_empty()
            || value.len() > MAX_CPU_MODEL_BYTES
            || value.chars().any(char::is_control)
            || (priority == 3 && value.bytes().all(|byte| byte.is_ascii_digit()))
        {
            continue;
        }
        candidates[priority].push(value.to_owned());
    }
    for mut values in candidates {
        if values.is_empty() {
            continue;
        }
        values.sort();
        values.dedup();
        return Ok((values.len() == 1).then(|| values[0].clone()));
    }
    Ok(None)
}
