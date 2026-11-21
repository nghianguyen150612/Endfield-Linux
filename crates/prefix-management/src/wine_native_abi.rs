//! Preloader-aware, non-authorizing candidate ABI assessment.
use crate::wine_candidate::CandidateProfile;
use endfield_unix_game_launch::elf_dependencies as elf;
use endfield_unix_runtime_audit::Entry;
use serde::Serialize;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeExecutableClass {
    DynamicallyLinked,
    StaticElf,
    InterpreterFreeEtDyn,
    WinePreloader,
    Unsupported,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreloaderAbiState {
    PreloaderAbiSupported,
    PreloaderAbiUnsupported,
    PreloaderAbiUnverified,
}
#[derive(Debug, Serialize)]
pub struct PreloaderAbiAssessment {
    pub state: PreloaderAbiState,
    pub classification: NativeExecutableClass,
    pub structurally_supported: bool,
    pub role_inventory_bound: bool,
    pub execution_authorized: bool,
    pub reason: String,
}

pub fn classify(bytes: &[u8]) -> NativeExecutableClass {
    match elf::inspect_candidate(bytes) {
        Ok(e) if e.interpreter.is_some() => NativeExecutableClass::DynamicallyLinked,
        Ok(e) if e.executable_type == 2 && e.needed.is_empty() => NativeExecutableClass::StaticElf,
        Ok(e) if e.executable_type == 3 && e.interpreter.is_none() => {
            NativeExecutableClass::InterpreterFreeEtDyn
        }
        _ => NativeExecutableClass::Unsupported,
    }
}

/// Candidate evidence binds bytes and role; it cannot authenticate a publisher.
/// Supported execution requires an authenticated profile plus a qualified view.
pub fn assess_candidate_preloader(
    profile: &CandidateProfile,
    inventory: &[Entry],
    role: &str,
    bytes: &[u8],
) -> PreloaderAbiAssessment {
    let mut result = PreloaderAbiAssessment {
        state: PreloaderAbiState::PreloaderAbiUnsupported,
        classification: classify(bytes),
        structurally_supported: false,
        role_inventory_bound: false,
        execution_authorized: false,
        reason: "invalid exact preloader role or bytes".into(),
    };
    if profile.validate(inventory).is_err()
        || !matches!(role, "preloader" | "preloader_i386")
        || !profile.native_helper_roles.iter().any(|r| r == role)
        || profile.validate_native_bytes(role, bytes).is_err()
    {
        return result;
    }
    result.role_inventory_bound = true;
    if !preloader_shape(bytes) {
        result.reason = "unsupported preloader program headers or entry point".into();
        return result;
    }
    result.structurally_supported = true;
    // A candidate role is explicitly not an authenticated execution role.
    result.state = PreloaderAbiState::PreloaderAbiUnverified;
    result.reason = "structure supported; candidate role lacks production publisher authentication, kernel qualification and sealed mounted runtime view".into();
    result
}

fn read(bytes: &[u8], offset: usize, size: usize) -> Option<u64> {
    let mut value = [0; 8];
    value[..size].copy_from_slice(bytes.get(offset..offset.checked_add(size)?)?);
    Some(u64::from_le_bytes(value))
}
fn shape(bytes: &[u8]) -> Option<()> {
    let object = elf::inspect_candidate(bytes).ok()?;
    if !((object.executable_type == 3) || (object.executable_type == 2 && object.class == 32))
        || object.interpreter.is_some()
        || !object.needed.is_empty()
        || object.rpath.is_some()
        || object.runpath.is_some()
    {
        return None;
    }
    let wide = object.class == 64;
    let word = if wide { 8 } else { 4 };
    let entry = read(bytes, 24, word)?;
    let phoff = usize::try_from(read(bytes, if wide { 32 } else { 28 }, word)?).ok()?;
    let count = read(bytes, if wide { 56 } else { 44 }, 2)?;
    let stride = if wide { 56 } else { 32 };
    let mut matches = 0;
    for index in 0..count as usize {
        let base = phoff.checked_add(index.checked_mul(stride)?)?;
        if read(bytes, base, 4)? != 1 {
            continue;
        }
        let flags = read(bytes, base + if wide { 4 } else { 24 }, 4)?;
        let offset = read(bytes, base + if wide { 8 } else { 4 }, word)?;
        let address = read(bytes, base + if wide { 16 } else { 8 }, word)?;
        let size = read(bytes, base + if wide { 32 } else { 16 }, word)?;
        let align = read(bytes, base + if wide { 48 } else { 28 }, word)?;
        if flags & 3 == 3
            || (align > 1 && (!align.is_power_of_two() || address % align != offset % align))
        {
            return None;
        }
        if flags & 1 != 0 && entry >= address && entry < address.checked_add(size)? {
            matches += 1;
        }
    }
    (entry != 0 && matches == 1).then_some(())
}

pub fn preloader_shape(bytes: &[u8]) -> bool {
    shape(bytes).is_some()
}
