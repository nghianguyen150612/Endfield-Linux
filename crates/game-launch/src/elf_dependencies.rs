//! Bounded, non-executing ELF64 little-endian inspection. No loader simulation.
//! Static imports cannot establish dlopen, NSS, driver or helper completeness.
use serde::Serialize;

pub const MAX_ELF_BYTES: usize = 64 * 1024 * 1024;
const MAX_HEADERS: usize = 256;
const MAX_DYNAMIC: usize = 4096;
const MAX_NEEDED: usize = 128;
const MAX_STRING: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ElfError {
    Invalid,
    Truncated,
    Limit,
    UnsupportedArchitecture,
    UnsupportedResolution,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ElfDependencies {
    pub class: u8,
    pub machine: u16,
    pub executable_type: u16,
    pub interpreter: Option<String>,
    pub needed: Vec<String>,
    pub rpath: Option<String>,
    pub runpath: Option<String>,
    pub dynamic_loading_not_established: bool,
}

fn slice(bytes: &[u8], offset: u64, len: u64) -> Result<&[u8], ElfError> {
    let start = usize::try_from(offset).map_err(|_| ElfError::Limit)?;
    let end = usize::try_from(offset.checked_add(len).ok_or(ElfError::Limit)?)
        .map_err(|_| ElfError::Limit)?;
    bytes.get(start..end).ok_or(ElfError::Truncated)
}
fn number(bytes: &[u8], offset: u64, width: u64) -> Result<u64, ElfError> {
    let mut word = [0; 8];
    let value = slice(bytes, offset, width)?;
    word[..value.len()].copy_from_slice(value);
    Ok(u64::from_le_bytes(word))
}
fn string(bytes: &[u8], offset: u64) -> Result<String, ElfError> {
    let rest = slice(
        bytes,
        offset,
        (bytes.len() as u64)
            .checked_sub(offset)
            .ok_or(ElfError::Invalid)?,
    )?;
    let end = rest
        .iter()
        .take(MAX_STRING + 1)
        .position(|b| *b == 0)
        .ok_or(ElfError::Invalid)?;
    if end == 0 || end > MAX_STRING {
        return Err(ElfError::Invalid);
    }
    let value = std::str::from_utf8(&rest[..end]).map_err(|_| ElfError::Invalid)?;
    if value.chars().any(char::is_control) {
        return Err(ElfError::Invalid);
    }
    Ok(value.to_owned())
}

/// Supports the explicit initial platform policy: ELF64 LE x86-64 ET_EXEC/ET_DYN.
/// Other classes/architectures are reported unsupported, never guessed.
pub fn inspect(bytes: &[u8]) -> Result<ElfDependencies, ElfError> {
    if bytes.len() > MAX_ELF_BYTES {
        return Err(ElfError::Limit);
    }
    if slice(bytes, 0, 4)? != b"\x7fELF" {
        return Err(ElfError::Invalid);
    }
    if number(bytes, 4, 1)? != 2 || number(bytes, 5, 1)? != 1 {
        return Err(ElfError::UnsupportedArchitecture);
    }
    if number(bytes, 6, 1)? != 1 || number(bytes, 20, 4)? != 1 || number(bytes, 52, 2)? != 64 {
        return Err(ElfError::Invalid);
    }
    let machine = number(bytes, 18, 2)? as u16;
    if machine != 62 {
        return Err(ElfError::UnsupportedArchitecture);
    }
    let kind = number(bytes, 16, 2)? as u16;
    if !matches!(kind, 2 | 3) {
        return Err(ElfError::Invalid);
    }
    let phoff = number(bytes, 32, 8)?;
    let count = number(bytes, 56, 2)? as usize;
    if count == 0 || count > MAX_HEADERS || number(bytes, 54, 2)? != 56 {
        return Err(ElfError::Limit);
    }
    slice(bytes, phoff, count as u64 * 56)?;
    let mut loads = Vec::new();
    let mut dynamic = None;
    let mut interpreter = None;
    for index in 0..count {
        let base = phoff + index as u64 * 56;
        let tag = number(bytes, base, 4)?;
        let offset = number(bytes, base + 8, 8)?;
        let address = number(bytes, base + 16, 8)?;
        let size = number(bytes, base + 32, 8)?;
        slice(bytes, offset, size)?;
        match tag {
            1 => {
                if size > number(bytes, base + 40, 8)? {
                    return Err(ElfError::Invalid);
                }
                address.checked_add(size).ok_or(ElfError::Limit)?;
                loads.push((address, size, offset));
            }
            2 => {
                if dynamic.replace((offset, size, address)).is_some() {
                    return Err(ElfError::Invalid);
                }
            }
            3 => {
                if interpreter.is_some() || size > MAX_STRING as u64 || size < 2 {
                    return Err(ElfError::Invalid);
                }
                let segment = slice(bytes, offset, size)?;
                let value = string(segment, 0)?;
                if value.len() + 1 != segment.len() || !value.starts_with('/') {
                    return Err(ElfError::Invalid);
                }
                interpreter = Some(value);
            }
            _ => {}
        }
    }
    let mut result = ElfDependencies {
        class: 64,
        machine,
        executable_type: kind,
        interpreter,
        needed: Vec::new(),
        rpath: None,
        runpath: None,
        dynamic_loading_not_established: true,
    };
    let Some((offset, size, address)) = dynamic else {
        return Ok(result);
    };
    // The runtime loader consumes PT_DYNAMIC at its virtual address. Require
    // that address to map uniquely to the same file bytes we inspect.
    let dynamic_mappings = loads
        .iter()
        .filter(|(start, len, file_offset)| {
            address >= *start
                && address
                    .checked_add(size)
                    .is_some_and(|end| end <= start + len)
                && file_offset.checked_add(address - start) == Some(offset)
        })
        .count();
    if dynamic_mappings != 1 {
        return Err(ElfError::Invalid);
    }
    if size % 16 != 0 || size / 16 > MAX_DYNAMIC as u64 {
        return Err(ElfError::Limit);
    }
    let mut imports = Vec::new();
    let mut strtab = None;
    let mut strsize = None;
    let mut terminated = false;
    for index in 0..size / 16 {
        let tag = number(bytes, offset + index * 16, 8)?;
        let value = number(bytes, offset + index * 16 + 8, 8)?;
        match tag {
            0 => {
                terminated = true;
                break;
            }
            5 => {
                if strtab.replace(value).is_some() {
                    return Err(ElfError::Invalid);
                }
            }
            10 => {
                if strsize.replace(value).is_some() {
                    return Err(ElfError::Invalid);
                }
            }
            0x6ffffffb if value & 0x800 != 0 => return Err(ElfError::UnsupportedResolution),
            1 | 15 | 29 => {
                if imports.len() >= MAX_NEEDED + 2 {
                    return Err(ElfError::Limit);
                }
                imports.push((tag, value));
            }
            // Audit/filter/auxiliary objects alter ordinary loader resolution.
            0x6ffffefb | 0x6ffffefc | 0x7fffffff | 0x7ffffffd => {
                return Err(ElfError::UnsupportedResolution);
            }
            _ => {}
        }
    }
    if !terminated {
        return Err(ElfError::Invalid);
    }
    if imports.is_empty() {
        return Ok(result);
    }
    let address = strtab.ok_or(ElfError::Invalid)?;
    let size = strsize.ok_or(ElfError::Invalid)?;
    if size > MAX_ELF_BYTES as u64 {
        return Err(ElfError::Limit);
    }
    let mappings: Vec<_> = loads
        .iter()
        .filter(|(start, len, _)| {
            address >= *start
                && address
                    .checked_add(size)
                    .is_some_and(|end| end <= start + len)
        })
        .collect();
    if mappings.len() != 1 {
        return Err(ElfError::Invalid);
    }
    let (start, _, offset) = *mappings[0];
    let table = slice(
        bytes,
        offset.checked_add(address - start).ok_or(ElfError::Limit)?,
        size,
    )?;
    for (tag, index) in imports {
        let value = string(table, index)?;
        match tag {
            1 => {
                if value.contains('/') || value.contains('$') || result.needed.len() >= MAX_NEEDED {
                    return Err(ElfError::UnsupportedResolution);
                }
                result.needed.push(value);
            }
            15 => {
                if result.rpath.replace(value).is_some() {
                    return Err(ElfError::Invalid);
                }
            }
            29 => {
                if result.runpath.replace(value).is_some() {
                    return Err(ElfError::Invalid);
                }
            }
            _ => unreachable!(),
        }
    }
    Ok(result)
}

/// Only exact $ORIGIN/${ORIGIN} plus normal relative components is modelled.
/// Empty/current-directory entries, traversal, $LIB/$PLATFORM and absolute
/// search overrides are blocked. Expansion must remain inside the managed root.
pub fn origin_search_paths(value: &str) -> Result<Vec<String>, ElfError> {
    if value.len() > MAX_STRING {
        return Err(ElfError::Limit);
    }
    let mut paths = Vec::new();
    for entry in value.split(':') {
        let suffix = entry
            .strip_prefix("$ORIGIN")
            .or_else(|| entry.strip_prefix("${ORIGIN}"))
            .ok_or(ElfError::UnsupportedResolution)?;
        if !suffix.is_empty() && !suffix.starts_with('/') {
            return Err(ElfError::UnsupportedResolution);
        }
        if suffix.contains('$')
            || suffix
                .split('/')
                .skip(1)
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(ElfError::UnsupportedResolution);
        }
        paths.push(suffix.trim_start_matches('/').to_owned());
        if paths.len() > 32 {
            return Err(ElfError::Limit);
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn put(b: &mut [u8], at: usize, n: u64, size: usize) {
        b[at..at + size].copy_from_slice(&n.to_le_bytes()[..size]);
    }
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 1024];
        b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        for (at, n, size) in [
            (16, 3, 2),
            (18, 62, 2),
            (20, 1, 4),
            (32, 64, 8),
            (52, 64, 2),
            (54, 56, 2),
            (56, 2, 2),
            (64, 1, 4),
            (72, 0, 8),
            (80, 0x1000, 8),
            (96, 1024, 8),
            (104, 1024, 8),
            (120, 2, 4),
            (128, 256, 8),
            (136, 0x1100, 8),
            (152, 80, 8),
            (256, 5, 8),
            (264, 0x1200, 8),
            (272, 10, 8),
            (280, 64, 8),
            (288, 1, 8),
            (296, 1, 8),
        ] {
            put(&mut b, at, n, size);
        }
        b[512..522].copy_from_slice(b"\0libc.so\0\0");
        b
    }
    #[test]
    fn parses_dynamic_import_without_executing() {
        let p = inspect(&fixture()).unwrap();
        assert_eq!(p.needed, ["libc.so"]);
        assert!(p.dynamic_loading_not_established);
        assert_eq!(p.class, 64);
    }
    #[test]
    fn rejects_invalid_and_every_truncated_header() {
        let b = fixture();
        for n in 0..256 {
            assert!(inspect(&b[..n]).is_err());
        }
        assert!(inspect(b"not ELF").is_err());
    }
    #[test]
    fn rejects_unsupported_architecture_and_class() {
        for (at, value) in [(4, 1), (5, 2), (18, 183)] {
            let mut b = fixture();
            b[at] = value;
            assert_eq!(inspect(&b), Err(ElfError::UnsupportedArchitecture));
        }
    }
    #[test]
    fn rejects_dynamic_address_that_does_not_map_to_inspected_bytes() {
        let mut b = fixture();
        put(&mut b, 136, 0x1300, 8);
        assert_eq!(inspect(&b), Err(ElfError::Invalid));
        let mut b = fixture();
        put(&mut b, 104, 10, 8);
        assert_eq!(inspect(&b), Err(ElfError::Invalid));
    }
    #[test]
    fn rejects_missing_string_table_and_invalid_needed() {
        let mut b = fixture();
        put(&mut b, 256, 21, 8);
        assert!(inspect(&b).is_err());
        let mut b = fixture();
        put(&mut b, 296, 64, 8);
        assert!(inspect(&b).is_err());
    }
    #[test]
    fn rejects_unterminated_dynamic_section() {
        let mut b = fixture();
        put(&mut b, 152, 48, 8);
        assert!(inspect(&b).is_err());
    }
    #[test]
    fn rejects_overflowing_program_headers_and_string_addresses() {
        for at in [32, 72, 80, 96, 128, 152, 264, 280, 296] {
            let mut b = fixture();
            put(&mut b, at, u64::MAX, 8);
            assert!(inspect(&b).is_err());
        }
    }
    #[test]
    fn rejects_absolute_needed_and_loader_tokens() {
        for name in [b"/tmp/x\0".as_slice(), b"$LIB/x\0"] {
            let mut b = fixture();
            b[513..513 + name.len()].copy_from_slice(name);
            assert!(inspect(&b).is_err());
        }
    }
    #[test]
    fn rejects_audit_filter_and_auxiliary_resolution() {
        for tag in [0x6ffffefb, 0x6ffffefc, 0x7fffffff, 0x7ffffffd] {
            let mut b = fixture();
            put(&mut b, 288, tag, 8);
            assert_eq!(inspect(&b), Err(ElfError::UnsupportedResolution));
        }
    }
    #[test]
    fn rejects_unsafe_search_paths() {
        for p in [
            "",
            ".",
            ":$ORIGIN",
            "$ORIGIN:",
            "/tmp/lib",
            "$ORIGIN/../lib",
            "$LIB",
            "$PLATFORM",
            "$ORIGIN//lib",
            "$ORIGIN/x/./y",
            "$ORIGIN_suffix",
        ] {
            assert!(origin_search_paths(p).is_err(), "{p}");
        }
    }
    #[test]
    fn accepts_only_contained_origin_expansion() {
        assert_eq!(
            origin_search_paths("$ORIGIN:${ORIGIN}/lib").unwrap(),
            ["", "lib"]
        );
    }
    #[test]
    fn rejects_conflicting_search_tags_and_malformed_interpreter() {
        let mut b = fixture();
        put(&mut b, 288, 29, 8);
        put(&mut b, 304, 29, 8);
        put(&mut b, 312, 1, 8);
        assert!(inspect(&b).is_err());
        let mut b = fixture();
        put(&mut b, 120, 3, 4);
        b[256] = b'x';
        assert!(inspect(&b).is_err());
    }
    #[test]
    fn inspects_real_native_elf() {
        let b = std::fs::read("/proc/self/exe").unwrap();
        let parsed = inspect(&b).unwrap();
        assert_eq!(parsed.machine, 62);
        assert!(parsed.interpreter.is_some());
        assert!(!parsed.needed.is_empty());
    }
    #[test]
    fn mutated_inputs_never_panic() {
        let original = fixture();
        for i in 0..original.len() {
            let mut b = original.clone();
            b[i] = 255;
            let _ = inspect(&b);
        }
    }
}
