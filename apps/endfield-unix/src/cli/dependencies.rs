//! P033 adapter: only existing read-only providers supply observations.
use super::compatibility::{self, CompatibilityError};
use endfield_unix_dependency_diagnostics as core;
use endfield_unix_game_import::{
    GameInstallationId, GameInstallationRegistry, GameInstallationState,
};
use endfield_unix_platform_linux::{CgroupLimit, LinuxHardwareProbe};
use endfield_unix_storage::AppPaths;
use serde::Serialize;
use std::{
    ffi::CString,
    fs::File,
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    },
    path::{Component, Path, PathBuf},
};

#[derive(Serialize)]
struct CpuObservation {
    model: Option<String>,
    logical_online: Option<usize>,
    physical_cores: Option<u32>,
    process_allowed: Option<usize>,
    limitation: &'static str,
}
#[derive(Serialize)]
struct GameObservation {
    selected_id: Option<String>,
    state: String,
    identity: &'static str,
    explanation: &'static str,
    action: &'static str,
}
#[derive(Serialize)]
struct AntiCheatEvidence {
    claim: &'static str,
    source_url: &'static str,
    observed_on: &'static str,
    linux_compatibility: &'static str,
    limitation: &'static str,
}
#[derive(Serialize)]
struct StorageObservation {
    selected: bool,
    status: &'static str,
    available_bytes: Option<u64>,
    source: &'static str,
    limitation: &'static str,
}

/// Observe only an explicitly selected directory. Each component is opened
/// relative to a held parent descriptor with O_NOFOLLOW; no path is created,
/// scanned, persisted or returned in the diagnostic report.
fn inspect_storage(path: Option<&Path>) -> StorageObservation {
    let observation = |selected, status, available_bytes| StorageObservation {
        selected,
        status,
        available_bytes,
        source: "explicit destination directory fstatvfs",
        limitation: "Snapshot of available blocks only; filesystem permissions, allocation, future updates, unpack location and GB units remain separate uncertainties.",
    };
    let Some(path) = path else {
        return observation(false, "not_selected", None);
    };
    if !path.is_absolute() || path.as_os_str().as_bytes().len() > 4096 {
        return observation(true, "invalid_path", None);
    }
    let Ok(mut current) = File::open("/") else {
        return observation(true, "unavailable", None);
    };
    let mut parts = 0_usize;
    for component in path.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(name) => {
                parts += 1;
                if parts > 256 || name.as_bytes().len() > 255 {
                    return observation(true, "invalid_path", None);
                }
                let Ok(name) = CString::new(name.as_bytes()) else {
                    return observation(true, "invalid_path", None);
                };
                // SAFETY: name is a bounded, NUL-free single path component.
                // The parent fd stays alive, and the returned fd is owned by File.
                let fd = unsafe {
                    libc::openat(
                        current.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                if fd < 0 {
                    return observation(true, "blocked_or_unavailable", None);
                }
                // SAFETY: openat returned a new owned file descriptor.
                current = unsafe { File::from_raw_fd(fd) };
            }
            _ => return observation(true, "invalid_path", None),
        }
    }
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: current is an open directory fd and stat has enough space.
    if unsafe { libc::fstatvfs(current.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return observation(true, "unavailable", None);
    }
    // SAFETY: fstatvfs returned success and initialized the structure.
    let stat = unsafe { stat.assume_init() };
    match (stat.f_bavail as u64).checked_mul(stat.f_frsize as u64) {
        Some(bytes) => observation(true, "observed", Some(bytes)),
        None => observation(true, "overflow", None),
    }
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    observed_at_unix_seconds: u64,
    requirement_catalog_revision: &'static str,
    selected_source: Option<core::Channel>,
    requirements: Vec<core::Requirement>,
    cpu: CpuObservation,
    memory: core::MemoryObservation,
    storage: StorageObservation,
    game: GameObservation,
    source_guidance: Option<&'static str>,
    source_evidence: Option<endfield_unix_game_sources::OfficialGameSource>,
    compatibility: endfield_unix_compatibility_diagnostics::CompatibilityDiagnosticReport,
    findings: Vec<core::Finding>,
    anti_cheat: AntiCheatEvidence,
    overall: &'static str,
    game_compatibility: &'static str,
    limitations: Vec<&'static str>,
}

pub fn run(
    profile: Option<String>,
    prefix_id: Option<String>,
    game_id: Option<String>,
    source_id: Option<String>,
    storage_path: Option<PathBuf>,
    json: bool,
    output: &mut impl Write,
) -> Result<(), CompatibilityError> {
    let channel = match source_id.as_deref() {
        Some("steam") => Some(core::Channel::Steam),
        Some("epic") => Some(core::Channel::Epic),
        Some("gryphlink") => Some(core::Channel::Gryphlink),
        _ => None,
    };
    let game_key = game_id
        .as_ref()
        .map(|id| GameInstallationId::new(id.clone()))
        .transpose()
        .map_err(|_| CompatibilityError::InvalidGameId)?;
    let compatibility = compatibility::collect(profile, prefix_id)?;
    let (cpu, memory) = match LinuxHardwareProbe::system().inspect() {
        Ok(hardware) => {
            let topology = hardware.cpu_topology.as_ref();
            let memory = core::MemoryObservation {
                physical_bytes: hardware.memory.host_total_bytes,
                available_bytes: hardware.memory.host_available_bytes,
                cgroup_limit_bytes: match &hardware.cgroup.memory_limit_bytes {
                    CgroupLimit::Limited(value) => Some(*value),
                    _ => None,
                },
                cgroup_limit_state: match hardware.cgroup.memory_limit_bytes {
                    CgroupLimit::Limited(_) => core::CgroupLimitState::Limited,
                    CgroupLimit::Unlimited => core::CgroupLimitState::Unlimited,
                    CgroupLimit::Unknown => core::CgroupLimitState::Unknown,
                },
                cgroup_ancestor_limits_assessed: hardware.cgroup.ancestor_limits_assessed,
                swap_bytes: hardware.memory.host_swap_total_bytes,
            };
            (
                CpuObservation {
                    model: hardware
                        .cpu_model
                        .map(|s| s.chars().filter(|c| !c.is_control()).take(256).collect()),
                    logical_online: topology.map(|t| t.online_cpu_ids.len()),
                    physical_cores: topology.and_then(|t| t.physical_core_count),
                    process_allowed: topology
                        .and_then(|t| t.process_allowed_cpu_ids.as_ref().map(Vec::len)),
                    limitation: "CPU model identity does not establish performance equivalence. Cgroup ancestor limits may be unobserved.",
                },
                memory,
            )
        }
        Err(_) => (
            CpuObservation {
                model: None,
                logical_online: None,
                physical_cores: None,
                process_allowed: None,
                limitation: "P012 hardware observation unavailable.",
            },
            core::MemoryObservation::default(),
        ),
    };
    let game = collect_game(game_key.as_ref(), game_id);
    let source_evidence = source_id
        .as_deref()
        .and_then(|id| endfield_unix_game_sources::source(id).ok());
    let guidance = source_evidence.as_ref().map(|source| source.guidance);
    let storage = inspect_storage(storage_path.as_deref());
    let mut findings = core::assess_memory(&memory, channel);
    findings.extend(core::assess_storage(channel, storage.available_bytes));
    findings.extend(core::assess_noncomparable(channel));
    let requirements = core::catalog()
        .into_iter()
        .filter(|r| Some(r.channel) == channel)
        .collect();
    let report = Report {
        schema_version: 1,
        observed_at_unix_seconds: compatibility.observed_at_unix_seconds,
        requirement_catalog_revision: core::CATALOG_REVISION,
        selected_source: channel,
        requirements,
        cpu,
        memory,
        storage,
        game,
        source_guidance: guidance,
        source_evidence,
        compatibility,
        findings,
        anti_cheat: AntiCheatEvidence {
            claim: "Steam listing reports kernel-level ACE anti-cheat.",
            source_url: "https://store.steampowered.com/app/4732690/",
            observed_on: endfield_unix_game_sources::OBSERVED_ON,
            linux_compatibility: "unknown",
            limitation: "No Linux support, publisher authorization, or Windows kernel-component compatibility is established. Do not bypass protections.",
        },
        overall: "incomplete: static observations do not establish launch readiness",
        game_compatibility: "not_tested",
        limitations: vec![
            "Windows requirements do not establish Linux compatibility.",
            "Storage observations are only a point-in-time snapshot of a selected destination and do not establish successful installation.",
            "DXVK and VKD3D-Proton availability and version-specific requirements are unknown.",
            "No runtime, game, or anti-cheat component was executed.",
        ],
    };
    let bytes = if json {
        serde_json::to_vec_pretty(&report).map_err(|_| CompatibilityError::Serialization)?
    } else {
        let mut text = format!(
            "Dependency diagnostics (static, incomplete)\nSource: {:?}\nCPU: {}\nPhysical RAM: {:?} bytes; available: {:?}; cgroup limit: {:?}; swap: {:?}\nStorage: {}\nGame: {} — {}\nRuntime selection: {:?}\nVulkan loader: {:?}\n",
            report.selected_source,
            report.cpu.model.as_deref().unwrap_or("unknown"),
            report.memory.physical_bytes,
            report.memory.available_bytes,
            report.memory.cgroup_limit_bytes,
            report.memory.swap_bytes,
            format!(
                "{} (available bytes: {:?})",
                report.storage.status, report.storage.available_bytes
            ),
            report.game.state,
            report.game.action,
            report.compatibility.runner.selection.state,
            report.compatibility.vulkan.loader_state
        );
        for requirement in &report.requirements {
            text.push_str(&format!(
                "{}: {} {} ({})\n",
                requirement.id, requirement.value, requirement.unit, requirement.source_url
            ));
        }
        for finding in &report.findings {
            text.push_str(&format!(
                "{}: {:?} — {} Action: {}\n",
                finding.id, finding.state, finding.explanation, finding.suggested_action
            ));
        }
        for finding in report.compatibility.findings.iter().take(48) {
            text.push_str(&format!(
                "{}: {:?} — {}\n",
                finding.id, finding.state, finding.reason
            ));
            if let Some(action) = &finding.suggested_remediation {
                text.push_str(&format!(
                    "  Action: {} (supported: {})\n",
                    action.action, action.supported_by_endfield_unix
                ));
            }
        }
        if let Some(guidance) = report.source_guidance {
            text.push_str(guidance);
            text.push('\n');
        }
        text.push_str("Game compatibility: NOT TESTED\n");
        text.into_bytes()
    };
    if bytes.len() > if json { 1024 * 1024 } else { 64 * 1024 } {
        return Err(CompatibilityError::OutputLimit);
    }
    output.write_all(&bytes).map_err(CompatibilityError::Output)
}

fn collect_game(id: Option<&GameInstallationId>, selected_id: Option<String>) -> GameObservation {
    let base = |state: String, explanation, action| GameObservation {
        selected_id: selected_id.clone(),
        state,
        identity: "not_established_or_user_declared_unverified",
        explanation,
        action,
    };
    let Some(paths) = AppPaths::discover().ok() else {
        return base(
            "unknown".into(),
            "Application paths unavailable.",
            "Review application storage configuration.",
        );
    };
    let registry = GameInstallationRegistry::for_app(&paths);
    match id {
        Some(id) => match registry.validate(id) {
            Ok(validation) => {
                let state = match validation.state {
                    GameInstallationState::Registered => "registered",
                    GameInstallationState::StructureObserved => "structure_observed",
                    GameInstallationState::IdentityUnverified => "identity_unverified",
                    GameInstallationState::IdentityVerified => "identity_verified",
                    GameInstallationState::Changed => "changed",
                    GameInstallationState::Missing => "missing",
                    GameInstallationState::Blocked => "blocked",
                    GameInstallationState::Unsupported => "unsupported",
                };
                base(
                    state.into(),
                    "The exact registered location was revalidated; payload integrity and identity are unverified.",
                    "Review the registered location with game validate.",
                )
            }
            Err(error) => base(
                error.code().into(),
                "The selected registration could not be revalidated.",
                "Review the selected registration with game list or the existing import plan.",
            ),
        },
        None => match registry.load_read_only() {
            Ok(snapshot) if snapshot.installations.is_empty() => base(
                "missing".into(),
                "No game is registered.",
                "Use the existing game import plan for a selected directory.",
            ),
            Ok(_) => base(
                "not_selected".into(),
                "Registered games exist, but none was explicitly selected.",
                "Choose an existing game ID for revalidation.",
            ),
            Err(_) => base(
                "unknown".into(),
                "The game registry is unavailable or corrupt.",
                "Review the registry using the existing game commands.",
            ),
        },
    }
}
