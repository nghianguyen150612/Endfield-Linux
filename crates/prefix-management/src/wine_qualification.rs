//! Conservative Wine qualification. Reports are observations, never capabilities.
use crate::authentic_qualification::RecordedQualificationStatus;
use crate::wine_artifact_profile::VerifiedProfile;
pub use crate::wine_artifact_profile::{ArtifactProfileBlocker, ComponentUse};
pub use crate::wine_host_abi::{HostAbiEvidence, HostAbiState};
use endfield_unix_game_launch::{
    elf_dependencies as elf,
    linux_executor::{DirectoryIdentity, ExpectedExecutableIdentity, TrustedRoot},
};
use endfield_unix_runtime_audit::{Baseline, EntryKind};
use endfield_unix_runtime_selection as pins;
use endfield_unix_storage::AppPaths;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, VecDeque},
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
};

const MAX_OBJECTS: usize = 128;
const MAX_TOTAL_BYTES: usize = 256 * 1024 * 1024;
const SYSTEM_DIRS: &[&str] = &["/usr/lib/x86_64-linux-gnu", "/usr/lib64", "/usr/lib"];
pub const POLICY_ID: &str = "wine10-linux-x86_64-conservative-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyState {
    VerifiedManaged,
    TrustedSystem,
    Missing,
    Untrusted,
    Unverifiable,
    DynamicNotEstablished,
    Unsupported,
}
#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyObservation {
    /// Basename or fixed role, never an absolute private pathname.
    pub object: String,
    pub state: DependencyState,
    pub sha256: Option<String>,
    pub elf: Option<elf::ElfDependencies>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationState {
    Verified,
    Missing,
    Unsupported,
    NotRun,
    Unknown,
    Blocked,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestAuthorityScope {
    IndependentlyAuthenticatedPublisher,
    UserPinned,
    DevelopmentOnly,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestSignatureState {
    PublisherProvenanceRecorded,
    DevelopmentOnlyProvenanceRecorded,
    UserPinned,
    Unavailable,
}
/// Source-defined roles are inventory candidates, not an approved package layout.
#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WineHelperEvidence {
    pub role: String,
    pub candidate_count: usize,
    pub usage: Option<ComponentUse>,
    pub integrity: QualificationState,
}
#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Wine10InvocationContract {
    pub source_ref: &'static str,
    pub source_semantics: QualificationState,
    pub artifact_mapping: QualificationState,
    pub arguments: [&'static str; 2],
    pub environment: [&'static str; 1],
    pub working_directory: &'static str,
    pub direct_exit_is_completion: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WineRuntimeQualification {
    pub schema_version: u32,
    pub policy_id: &'static str,
    pub manifest_authority_scope: ManifestAuthorityScope,
    pub manifest_signature_state: ManifestSignatureState,
    pub runtime_installation_id: Option<String>,
    pub pin_id: Option<String>,
    pub pin_revision: u64,
    pub artifact_sha256: Option<String>,
    pub ownership: QualificationState,
    pub integrity: QualificationState,
    pub executable_sha256: Option<String>,
    pub executable_identity: Option<[u64; 3]>,
    pub dependencies: Vec<DependencyObservation>,
    pub helper_inventory: QualificationState,
    pub helpers: Vec<WineHelperEvidence>,
    pub invocation: Wine10InvocationContract,
    pub invocation_contract: QualificationState,
    pub completion_verifier: QualificationState,
    pub descendant_completion: QualificationState,
    pub authentic_qualification: QualificationState,
    pub recorded_qualification: Option<RecordedQualificationStatus>,
    pub artifact_profile_id: Option<String>,
    pub artifact_profile_blocker: Option<ArtifactProfileBlocker>,
    pub host_abi: HostAbiEvidence,
    pub host_i386_abi: QualificationState,
    pub runtime_image_provider: &'static str,
    pub runtime_image_sha256: Option<String>,
    pub mounted_view_eligibility: QualificationState,
    pub isolated_qualification_readiness: QualificationState,
    pub host_compatible: bool,
    pub host_architecture: &'static str,
    pub host_os: &'static str,
    pub production_initialization: QualificationState,
    pub approval_required: bool,
    pub blockers: Vec<String>,
}
impl WineRuntimeQualification {
    pub fn unavailable(reason: &str) -> Self {
        Self {
            schema_version: 2,
            policy_id: POLICY_ID,
            manifest_authority_scope: ManifestAuthorityScope::Unknown,
            manifest_signature_state: ManifestSignatureState::Unavailable,
            runtime_installation_id: None,
            pin_id: None,
            pin_revision: 0,
            artifact_sha256: None,
            ownership: QualificationState::Unknown,
            integrity: QualificationState::Unknown,
            executable_sha256: None,
            executable_identity: None,
            dependencies: Vec::new(),
            helper_inventory: QualificationState::Unknown,
            helpers: Vec::new(),
            invocation: Wine10InvocationContract {
                source_ref: "wine-mirror/wine:wine-10.0",
                source_semantics: QualificationState::Verified,
                artifact_mapping: QualificationState::Unsupported,
                arguments: ["wineboot.exe", "--init"],
                environment: ["WINEPREFIX"],
                working_directory: "exact-selected-managed-prefix",
                direct_exit_is_completion: false,
            },
            invocation_contract: QualificationState::Unsupported,
            completion_verifier: QualificationState::Verified,
            descendant_completion: QualificationState::Unknown,
            authentic_qualification: QualificationState::NotRun,
            recorded_qualification: None,
            artifact_profile_id: None,
            artifact_profile_blocker: Some(ArtifactProfileBlocker::Missing),
            host_abi: HostAbiEvidence::issue(HostAbiState::Unverifiable, "artifact_profile"),
            host_i386_abi: if Path::new("/lib/ld-linux.so.2").is_file() {
                QualificationState::Unknown
            } else {
                QualificationState::Missing
            },
            runtime_image_provider: "squashfuse",
            runtime_image_sha256: None,
            mounted_view_eligibility: QualificationState::Blocked,
            isolated_qualification_readiness: QualificationState::Blocked,
            host_compatible: false,
            host_architecture: std::env::consts::ARCH,
            host_os: std::env::consts::OS,
            production_initialization: QualificationState::Blocked,
            approval_required: true,
            blockers: vec![
                reason.into(),
                "artifact_layout_contract_unavailable".into(),
                "dynamic_modules_not_established".into(),
                "descendant_completion_not_established".into(),
                "authentic_qualification_not_run".into(),
                "application_runtime_image_provider_not_integrated".into(),
                "descriptor_runtime_loading_observation_required".into(),
                "disposable_root_and_exact_approval_required".into(),
            ],
        }
    }
}

/// Read-only exact selection inspection. No storage creation, subprocess or network.
pub fn qualify_managed_wine(
    paths: &AppPaths,
    pin_id: &str,
    revision: u64,
    reference: &pins::PinnedRuntimeReference,
) -> WineRuntimeQualification {
    let mut report = WineRuntimeQualification::unavailable("runtime_evidence_unavailable");
    report.pin_id = Some(pin_id.into());
    report.pin_revision = revision;
    let pins::PinnedRuntimeReference::Managed(runtime) = reference else {
        return report;
    };
    match runtime.trust {
        pins::Provenance::IndependentlyAuthenticatedPublisher => {
            report.manifest_authority_scope =
                ManifestAuthorityScope::IndependentlyAuthenticatedPublisher;
            report.manifest_signature_state = ManifestSignatureState::PublisherProvenanceRecorded;
        }
        pins::Provenance::UserPinnedLimitedTrust => {
            report.manifest_authority_scope = ManifestAuthorityScope::UserPinned;
            report.manifest_signature_state = ManifestSignatureState::UserPinned;
        }
        pins::Provenance::DevelopmentOnly => {
            report.manifest_authority_scope = ManifestAuthorityScope::DevelopmentOnly;
            report.manifest_signature_state =
                ManifestSignatureState::DevelopmentOnlyProvenanceRecorded;
        }
        pins::Provenance::UnknownExternalSource => {
            report.manifest_authority_scope = ManifestAuthorityScope::Unknown;
            report.manifest_signature_state = ManifestSignatureState::Unavailable;
        }
    }
    report.runtime_installation_id = Some(runtime.installation_id.clone());
    report.artifact_sha256 = Some(runtime.artifact_sha256.clone());
    if reference.validate().is_err() {
        return report;
    }
    let path = paths
        .data_dir()
        .join("runtimes/wine")
        .join(&runtime.installation_id);
    let result = (|| {
        let metadata = fs::symlink_metadata(&path).ok()?;
        let uid = crate::current_uid();
        let root = TrustedRoot::open(
            &path,
            DirectoryIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
                owner_uid: metadata.uid(),
            },
            uid,
        )
        .ok()?;
        let file = root.open_regular(Path::new("bin/wine")).ok()?;
        let meta = file.metadata().ok()?;
        let expected = ExpectedExecutableIdentity {
            device: meta.dev(),
            inode: meta.ino(),
            size: meta.len(),
        };
        let verified =
            crate::verify_managed_wine_executable(paths, pin_id, revision, reference, expected)
                .ok()?;
        let baseline = endfield_unix_runtime_audit::load_baseline(&path).ok()?;
        report.executable_sha256 = Some(verified.sha256().into());
        report.executable_identity = Some([expected.device, expected.inode, expected.size]);
        report.ownership = QualificationState::Verified;
        report.integrity = QualificationState::Verified;
        report
            .blockers
            .retain(|b| b != "runtime_evidence_unavailable");
        report.dependencies = inspect_graph(&root, &baseline);
        if report.dependencies.iter().any(|d| {
            !matches!(
                d.state,
                DependencyState::VerifiedManaged
                    | DependencyState::TrustedSystem
                    | DependencyState::DynamicNotEstablished
            )
        }) {
            report
                .blockers
                .push("static_dependency_inspection_incomplete".into());
        }
        match VerifiedProfile::load(paths, runtime, &root, &baseline) {
            Ok(profile) => {
                report.artifact_profile_id = Some(profile.id);
                report.artifact_profile_blocker = None;
                report.host_abi = crate::wine_host_abi::inspect(&root, &baseline, &profile.profile);
                report.host_compatible =
                    report.host_abi.state == HostAbiState::SupportedUnderHostAssumptions;
                report.helpers = profile
                    .profile
                    .components
                    .into_iter()
                    .map(|component| WineHelperEvidence {
                        role: component.role,
                        candidate_count: 1,
                        usage: Some(component.usage),
                        integrity: if matches!(
                            component.usage,
                            ComponentUse::Initialization | ComponentUse::HostDependent
                        ) {
                            QualificationState::Verified
                        } else {
                            QualificationState::Unknown
                        },
                    })
                    .collect();
                report.helper_inventory = QualificationState::Verified;
                report.invocation.artifact_mapping = QualificationState::Verified;
                report.invocation_contract = QualificationState::Verified;
                report
                    .blockers
                    .retain(|blocker| blocker != "artifact_layout_contract_unavailable");
                if report.host_compatible {
                    report.isolated_qualification_readiness = QualificationState::Unknown;
                }
            }
            Err(error) => {
                report.artifact_profile_blocker = Some(error);
                report.helpers = inspect_helpers(&root, &baseline);
                report.helper_inventory = QualificationState::Unsupported;
            }
        }
        // Repeat P025/P026/P027 after all helper/dependency reads.
        crate::verify_managed_wine_executable(paths, pin_id, revision, reference, expected).ok()?;
        if endfield_unix_runtime_audit::load_baseline(&path).ok()? != baseline {
            return None;
        }
        Some(())
    })();
    if result.is_none() {
        report.ownership = QualificationState::Unknown;
        report.integrity = QualificationState::Unknown;
        report.isolated_qualification_readiness = QualificationState::Blocked;
        report.host_compatible = false;
        if report.artifact_profile_id.take().is_some() {
            report.artifact_profile_blocker = Some(ArtifactProfileBlocker::BaselineMismatch);
            report.helper_inventory = QualificationState::Unknown;
            report.invocation.artifact_mapping = QualificationState::Unknown;
            report.invocation_contract = QualificationState::Unknown;
        }
        report.host_abi = HostAbiEvidence::issue(HostAbiState::Unverifiable, "volatile_provenance");
        report
            .blockers
            .push("runtime_changed_or_unverifiable".into());
    }
    if !report.host_compatible {
        report
            .blockers
            .push("host_platform_abi_not_qualified".into());
    }
    report
}

pub(crate) fn trusted_directory(path: &Path) -> std::io::Result<TrustedRoot> {
    let metadata = fs::symlink_metadata(path)?;
    TrustedRoot::open(
        path,
        DirectoryIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            owner_uid: metadata.uid(),
        },
        crate::current_uid(),
    )
    .map_err(std::io::Error::other)
}

pub(crate) fn bounded_bytes(
    mut file: File,
    budget: &mut usize,
) -> Result<Vec<u8>, DependencyState> {
    let before = file.metadata().map_err(|_| DependencyState::Unverifiable)?;
    let size = usize::try_from(before.len()).map_err(|_| DependencyState::Unverifiable)?;
    if size > elf::MAX_ELF_BYTES || size > *budget {
        return Err(DependencyState::Unverifiable);
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(size as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| DependencyState::Unverifiable)?;
    let after = file.metadata().map_err(|_| DependencyState::Unverifiable)?;
    if bytes.len() != size
        || (
            before.dev(),
            before.ino(),
            before.mode(),
            before.mtime(),
            before.mtime_nsec(),
            before.ctime(),
            before.ctime_nsec(),
        ) != (
            after.dev(),
            after.ino(),
            after.mode(),
            after.mtime(),
            after.mtime_nsec(),
            after.ctime(),
            after.ctime_nsec(),
        )
    {
        return Err(DependencyState::Unverifiable);
    }
    *budget -= bytes.len();
    Ok(bytes)
}
pub(crate) fn managed_bytes(
    root: &TrustedRoot,
    baseline: &Baseline,
    path: &Path,
    budget: &mut usize,
) -> Result<Vec<u8>, DependencyState> {
    let entry = baseline
        .entries
        .iter()
        .find(|e| Path::new(&e.path) == path)
        .ok_or(DependencyState::Untrusted)?;
    let EntryKind::File {
        size,
        sha256,
        setid: false,
        ..
    } = &entry.kind
    else {
        return Err(DependencyState::Unsupported);
    };
    let file = root
        .open_regular(path)
        .map_err(|_| DependencyState::Untrusted)?;
    let bytes = bounded_bytes(file, budget)?;
    if bytes.len() as u64 != *size || format!("{:x}", Sha256::digest(&bytes)) != *sha256 {
        return Err(DependencyState::Untrusted);
    }
    Ok(bytes)
}
/// Explicit host-distribution assumption: canonical /usr/lib files and every
/// canonical ancestor are root-owned and not group/world writable. Canonical
/// symlink aliases are accepted only within this system trust domain. No cache
/// interpretation; any report remains blocked by unmodelled loader resolution.
// Inspect every component in the actual symlink resolution chain. Checking
// only the final canonical target would accept a user-writable alias to ld.so.
pub(crate) fn trusted_system_path(path: &Path) -> Result<PathBuf, DependencyState> {
    if !path.is_absolute() {
        return Err(DependencyState::Untrusted);
    }
    let mut pending: VecDeque<PathBuf> = path
        .components()
        .map(|c| PathBuf::from(c.as_os_str()))
        .collect();
    let mut resolved = PathBuf::from("/");
    let mut links = 0;
    while let Some(part) = pending.pop_front() {
        match part.components().next() {
            Some(Component::RootDir) => {
                resolved = PathBuf::from("/");
                continue;
            }
            Some(Component::CurDir) => continue,
            Some(Component::ParentDir) => {
                resolved.pop();
                continue;
            }
            Some(Component::Normal(_)) => {}
            _ => return Err(DependencyState::Untrusted),
        }
        let candidate = resolved.join(&part);
        let metadata = fs::symlink_metadata(&candidate).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                DependencyState::Missing
            } else {
                DependencyState::Unverifiable
            }
        })?;
        if metadata.uid() != 0 {
            return Err(DependencyState::Untrusted);
        }
        if metadata.file_type().is_symlink() {
            links += 1;
            if links > 40 {
                return Err(DependencyState::Unverifiable);
            }
            let target = fs::read_link(&candidate).map_err(|_| DependencyState::Unverifiable)?;
            if target.as_os_str().len() > 4096 {
                return Err(DependencyState::Unverifiable);
            }
            for component in target.components().rev() {
                pending.push_front(PathBuf::from(component.as_os_str()));
            }
            if pending.len() > 256 {
                return Err(DependencyState::Unverifiable);
            }
        } else {
            if metadata.mode() & 0o6022 != 0 || (!pending.is_empty() && !metadata.is_dir()) {
                return Err(DependencyState::Untrusted);
            }
            resolved = candidate;
        }
    }
    Ok(resolved)
}

pub(crate) fn system_bytes(
    path: &Path,
    budget: &mut usize,
) -> Result<(PathBuf, Vec<u8>), DependencyState> {
    let canonical = trusted_system_path(path)?;
    if !canonical.starts_with("/usr/lib") && !canonical.starts_with("/usr/lib64") {
        return Err(DependencyState::Untrusted);
    }
    let mut parent = canonical.as_path();
    loop {
        let meta = fs::symlink_metadata(parent).map_err(|_| DependencyState::Unverifiable)?;
        if meta.uid() != 0 || meta.mode() & 0o6022 != 0 || meta.file_type().is_symlink() {
            return Err(DependencyState::Untrusted);
        }
        let Some(next) = parent.parent() else {
            break;
        };
        parent = next;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(&canonical)
        .map_err(|_| DependencyState::Unverifiable)?;
    if !file
        .metadata()
        .map_err(|_| DependencyState::Unverifiable)?
        .is_file()
    {
        return Err(DependencyState::Untrusted);
    }
    Ok((canonical, bounded_bytes(file, budget)?))
}

fn inspect_helpers(root: &TrustedRoot, baseline: &Baseline) -> Vec<WineHelperEvidence> {
    let mut budget = MAX_TOTAL_BYTES;
    [
        "wineserver",
        "wineboot.exe",
        "ntdll.so",
        "ntdll.dll",
        "kernel32.dll",
        "kernelbase.dll",
        "services.exe",
        "rundll32.exe",
        "wine.inf",
    ]
    .into_iter()
    .map(|role| {
        let candidates: Vec<_> = baseline
            .entries
            .iter()
            .filter(|entry| {
                Path::new(&entry.path)
                    .file_name()
                    .is_some_and(|n| n == role)
            })
            .collect();
        let integrity = if candidates.is_empty() {
            QualificationState::Missing
        } else if candidates.len() != 1 {
            QualificationState::Unknown
        } else if managed_bytes(root, baseline, Path::new(&candidates[0].path), &mut budget).is_ok()
        {
            QualificationState::Verified
        } else {
            QualificationState::Blocked
        };
        WineHelperEvidence {
            role: role.into(),
            candidate_count: candidates.len(),
            usage: None,
            integrity,
        }
    })
    .collect()
}

fn inspect_graph(root: &TrustedRoot, baseline: &Baseline) -> Vec<DependencyObservation> {
    let mut queue = VecDeque::from([(PathBuf::from("bin/wine"), true, "launcher".to_owned())]);
    // Inventory candidates are read/hash checked, but do not establish a layout.
    for entry in &baseline.entries {
        if matches!(
            Path::new(&entry.path).file_name().and_then(|n| n.to_str()),
            Some(
                "wineserver"
                    | "wine64"
                    | "wine-preloader"
                    | "wine64-preloader"
                    | "wineboot.exe"
                    | "ntdll.so"
            )
        ) {
            queue.push_back((PathBuf::from(&entry.path), true, "helper_candidate".into()));
        }
        if queue.len() >= MAX_OBJECTS {
            break;
        }
    }
    let mut visited = BTreeSet::new();
    let mut output = Vec::new();
    let mut budget = MAX_TOTAL_BYTES;
    while let Some((path, managed, role)) = queue.pop_front() {
        if !visited.insert((path.clone(), managed)) {
            continue;
        }
        if visited.len() > MAX_OBJECTS {
            output.push(observation(
                "graph_limit",
                DependencyState::Unverifiable,
                None,
                None,
            ));
            break;
        }
        let read = if managed {
            managed_bytes(root, baseline, &path, &mut budget).map(|b| (path.clone(), b))
        } else {
            system_bytes(&path, &mut budget)
        };
        let (resolved, bytes) = match read {
            Ok(value) => value,
            Err(state) => {
                output.push(observation(&role, state, None, None));
                continue;
            }
        };
        let digest = format!("{:x}", Sha256::digest(&bytes));
        // Windows modules are integrity observations only, not native ELF graphs.
        if path.extension().is_some_and(|x| x == "exe") {
            output.push(observation(
                &role,
                DependencyState::VerifiedManaged,
                Some(digest),
                None,
            ));
            continue;
        }
        let parsed = match elf::inspect(&bytes) {
            Ok(value) => value,
            Err(_) => {
                output.push(observation(
                    &role,
                    DependencyState::Unsupported,
                    Some(digest),
                    None,
                ));
                continue;
            }
        };
        if role == "launcher" && parsed.interpreter.is_none() {
            output.push(observation(
                "interpreter_not_present",
                DependencyState::Unverifiable,
                None,
                None,
            ));
        }
        let mut dirs = Vec::new();
        if parsed.rpath.is_some() && parsed.runpath.is_some() {
            output.push(observation(
                "rpath_runpath_combination",
                DependencyState::Unsupported,
                None,
                None,
            ));
        }
        for search in [&parsed.rpath, &parsed.runpath].into_iter().flatten() {
            match elf::origin_search_paths(search) {
                Ok(entries) if managed => {
                    for entry in entries {
                        dirs.push(resolved.parent().unwrap_or(Path::new("")).join(entry));
                    }
                }
                _ => output.push(observation(
                    "search_path",
                    DependencyState::Unsupported,
                    None,
                    None,
                )),
            }
        }
        if let Some(interpreter) = &parsed.interpreter {
            queue.push_back((PathBuf::from(interpreter), false, "interpreter".into()));
        }
        for needed in &parsed.needed {
            let candidate = dirs
                .iter()
                .map(|d| d.join(needed))
                .find(|p| baseline.entries.iter().any(|e| Path::new(&e.path) == p));
            if let Some(candidate) = candidate {
                queue.push_back((candidate, true, needed.clone()));
            } else {
                // Managed RPATH/RUNPATH misses and default system candidates are
                // observations only. ld.so.cache/hwcaps/inherited RPATH are not modelled.
                let candidate = SYSTEM_DIRS
                    .iter()
                    .map(|d| Path::new(d).join(needed))
                    .find(|p| p.symlink_metadata().is_ok());
                if let Some(candidate) = candidate {
                    queue.push_back((candidate, false, needed.clone()));
                } else {
                    output.push(observation(needed, DependencyState::Missing, None, None));
                }
            }
            if queue.len() + output.len() > MAX_OBJECTS * 4 {
                output.push(observation(
                    "edge_limit",
                    DependencyState::Unverifiable,
                    None,
                    None,
                ));
                queue.clear();
                break;
            }
        }
        output.push(observation(
            &role,
            if managed {
                DependencyState::VerifiedManaged
            } else {
                DependencyState::TrustedSystem
            },
            Some(digest),
            Some(parsed),
        ));
    }
    output.push(observation(
        "dlopen_and_helper_closure",
        DependencyState::DynamicNotEstablished,
        None,
        None,
    ));
    output.push(observation(
        "loader_cache_hwcaps_and_inherited_rpath",
        DependencyState::Unsupported,
        None,
        None,
    ));
    output
}
fn observation(
    role: &str,
    state: DependencyState,
    sha256: Option<String>,
    mut parsed: Option<elf::ElfDependencies>,
) -> DependencyObservation {
    // ELF strings can contain private paths. Public reports expose only whether
    // search/interpreter fields were present; detailed raw ELF parsing is internal.
    if let Some(elf) = &mut parsed {
        if elf.interpreter.is_some() {
            elf.interpreter = Some("<system-interpreter>".into());
        }
        if elf.rpath.is_some() {
            elf.rpath = Some("<redacted>".into());
        }
        if elf.runpath.is_some() {
            elf.runpath = Some("<redacted>".into());
        }
    }
    DependencyObservation {
        object: role.chars().take(128).collect(),
        state,
        sha256,
        elf: parsed,
    }
}

/// No report or CLI string can supply authentic qualification. This producer
/// evaluates current evidence and fails closed until a build-specific profile,
/// quiescence implementation and authenticated qualification evidence exist.
pub fn authorize_production_initialization(
    paths: &AppPaths,
    plan: &crate::PrefixInitializationPlan,
    approval: &crate::PrefixInitializationApproval,
    store: &crate::PrefixStore,
) -> Result<crate::InitializationExecutionAuthorization, crate::PrefixError> {
    plan.validate_integrity()?;
    if approval.plan_id() != plan.plan_id {
        return Err(crate::PrefixError::ApprovalMismatch);
    }
    let id = crate::PrefixId::new(plan.prefix_id.clone())?;
    let inspection = store.show(&id, |_| Ok(()))?;
    let report = qualify_managed_wine(
        paths,
        &plan.runtime_pin_id,
        plan.runtime_pin_revision,
        &inspection.record.runtime.runtime,
    );
    // No eligible profile is shipped. Do not add a bypass for an externally
    // serialized report, matching filenames, or a synthetic test result.
    if report.production_initialization != QualificationState::Verified {
        return Err(crate::PrefixError::InitializationUnsupported);
    }
    Err(crate::PrefixError::InitializationUnsupported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use endfield_unix_runtime_audit::{BaselineOrigin, Entry, TrustClassification};
    use std::os::unix::fs::PermissionsExt;
    fn root(path: &Path) -> TrustedRoot {
        let m = fs::metadata(path).unwrap();
        TrustedRoot::open(
            path,
            DirectoryIdentity {
                device: m.dev(),
                inode: m.ino(),
                owner_uid: m.uid(),
            },
            crate::current_uid(),
        )
        .unwrap()
    }
    fn baseline() -> Baseline {
        Baseline {
            schema_version: 1,
            installation_id: "wine-synthetic".into(),
            generation: 1,
            family: "wine".into(),
            build: "wine-10.0".into(),
            artifact_sha256: "a".repeat(64),
            manifest_sha256: "b".repeat(64),
            trust: TrustClassification::IndependentlyAuthenticatedPublisher,
            origin: BaselineOrigin::VerifiedInstallation,
            inventory_sha256: "c".repeat(64),
            entries: Vec::new(),
        }
    }
    fn add(base: &mut Baseline, directory: &Path, path: &str, bytes: &[u8]) {
        let full = directory.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, bytes).unwrap();
        fs::set_permissions(&full, fs::Permissions::from_mode(0o700)).unwrap();
        base.entries.push(Entry {
            path: path.into(),
            kind: EntryKind::File {
                size: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(bytes)),
                executable: true,
                setid: false,
            },
        });
    }
    fn elf_fixture(needed: &str) -> Vec<u8> {
        let mut b = vec![0; 1024];
        b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        for (at, n, size) in [
            (16, 3u64, 2),
            (18, 62, 2),
            (20, 1, 4),
            (32, 64, 8),
            (52, 64, 2),
            (54, 56, 2),
            (56, 2, 2),
            (64, 1, 4),
            (80, 0x1000, 8),
            (96, 1024, 8),
            (104, 1024, 8),
            (120, 2, 4),
            (128, 256, 8),
            (136, 0x1100, 8),
            (152, 96, 8),
            (256, 5, 8),
            (264, 0x1200, 8),
            (272, 10, 8),
            (280, 128, 8),
            (288, 1, 8),
            (296, 1, 8),
            (304, 29, 8),
            (312, 64, 8),
        ] {
            b[at..at + size].copy_from_slice(&n.to_le_bytes()[..size]);
        }
        b[513..513 + needed.len()].copy_from_slice(needed.as_bytes());
        b[576..583].copy_from_slice(b"$ORIGIN");
        b
    }
    #[test]
    fn managed_integrity_requires_exact_bytes_and_rejects_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let mut base = baseline();
        add(&mut base, temp.path(), "lib/a.so", b"test");
        let root = root(temp.path());
        let mut budget = 100;
        assert_eq!(
            managed_bytes(&root, &base, Path::new("lib/a.so"), &mut budget).unwrap(),
            b"test"
        );
        fs::write(temp.path().join("lib/a.so"), b"evil").unwrap();
        assert_eq!(
            managed_bytes(&root, &base, Path::new("lib/a.so"), &mut budget),
            Err(DependencyState::Untrusted)
        );
    }
    #[test]
    fn managed_symlinks_writable_dependencies_and_untracked_objects_are_blocked() {
        let temp = tempfile::tempdir().unwrap();
        let mut base = baseline();
        add(&mut base, temp.path(), "a.so", b"test");
        let root = root(temp.path());
        fs::set_permissions(temp.path().join("a.so"), fs::Permissions::from_mode(0o777)).unwrap();
        assert!(managed_bytes(&root, &base, Path::new("a.so"), &mut 100).is_err());
        assert!(managed_bytes(&root, &base, Path::new("untracked"), &mut 100).is_err());
        fs::remove_file(temp.path().join("a.so")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", temp.path().join("a.so")).unwrap();
        assert!(managed_bytes(&root, &base, Path::new("a.so"), &mut 100).is_err());
    }
    #[test]
    fn dependency_cycles_terminate_and_dynamic_unknown_stays_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let mut base = baseline();
        add(&mut base, temp.path(), "bin/wine", &elf_fixture("cycle.so"));
        add(
            &mut base,
            temp.path(),
            "bin/cycle.so",
            &elf_fixture("cycle.so"),
        );
        let observations = inspect_graph(&root(temp.path()), &base);
        assert_eq!(
            observations
                .iter()
                .filter(|o| o.state == DependencyState::VerifiedManaged)
                .count(),
            2
        );
        assert!(
            observations
                .iter()
                .any(|o| o.state == DependencyState::DynamicNotEstablished)
        );
        assert!(observations.len() < 10);
        let json = serde_json::to_string(&observations).unwrap();
        assert!(!json.contains(temp.path().to_str().unwrap()));
        assert!(!json.contains("$ORIGIN"));
    }
    #[test]
    fn absent_interpreter_and_unsafe_runpath_remain_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let mut base = baseline();
        let mut bytes = elf_fixture("nonexistent-p034.so");
        bytes[576..585].copy_from_slice(b"/tmp/lib\0");
        add(&mut base, temp.path(), "bin/wine", &bytes);
        let observations = inspect_graph(&root(temp.path()), &base);
        assert!(
            observations
                .iter()
                .any(|o| o.object == "interpreter_not_present"
                    && o.state == DependencyState::Unverifiable)
        );
        assert!(
            observations
                .iter()
                .any(|o| o.object == "search_path" && o.state == DependencyState::Unsupported)
        );
    }

    #[test]
    fn missing_library_and_invalid_elf_are_separate_blockers() {
        let temp = tempfile::tempdir().unwrap();
        let mut base = baseline();
        add(
            &mut base,
            temp.path(),
            "bin/wine",
            &elf_fixture("lib-nonexistent-p034.so"),
        );
        assert!(
            inspect_graph(&root(temp.path()), &base)
                .iter()
                .any(|o| o.state == DependencyState::Missing)
        );
        let mut base = baseline();
        add(&mut base, temp.path(), "bin/wine", b"invalid");
        assert!(
            inspect_graph(&root(temp.path()), &base)
                .iter()
                .any(|o| o.state == DependencyState::Unsupported)
        );
    }
    #[test]
    fn missing_replaced_and_ambiguous_helpers_cannot_qualify_a_layout() {
        let temp = tempfile::tempdir().unwrap();
        let mut base = baseline();
        add(&mut base, temp.path(), "bin/wineserver", b"synthetic bytes");
        let root = root(temp.path());
        let helpers = inspect_helpers(&root, &base);
        assert_eq!(helpers[0].integrity, QualificationState::Verified);
        assert_eq!(helpers[1].integrity, QualificationState::Missing);
        fs::write(temp.path().join("bin/wineserver"), b"changed").unwrap();
        assert_eq!(
            inspect_helpers(&root, &base)[0].integrity,
            QualificationState::Blocked
        );
        add(&mut base, temp.path(), "other/wineserver", b"another");
        assert_eq!(
            inspect_helpers(&root, &base)[0].integrity,
            QualificationState::Unknown
        );
    }
    #[test]
    fn byte_budgets_are_enforced() {
        let temp = tempfile::tempfile().unwrap();
        temp.set_len(elf::MAX_ELF_BYTES as u64 + 1).unwrap();
        assert_eq!(
            bounded_bytes(temp, &mut MAX_TOTAL_BYTES.clone()),
            Err(DependencyState::Unverifiable)
        );
    }
    #[test]
    fn untrusted_system_candidate_is_not_accepted_as_host_library() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        assert_eq!(
            system_bytes(temp.path(), &mut 100),
            Err(DependencyState::Untrusted)
        );
    }
    #[test]
    fn writable_alias_to_a_trusted_system_library_is_rejected() {
        let system = Path::new("/usr/lib/x86_64-linux-gnu/libc.so.6");
        if system.exists() {
            let temp = tempfile::tempdir().unwrap();
            let alias = temp.path().join("loader");
            std::os::unix::fs::symlink(system, &alias).unwrap();
            assert_eq!(
                system_bytes(&alias, &mut 1024),
                Err(DependencyState::Untrusted)
            );
        }
    }

    #[test]
    fn host_library_classification_is_conditional_on_root_owned_system_policy() {
        let path = Path::new("/usr/lib/x86_64-linux-gnu/libc.so.6");
        if path.exists() {
            let (_, bytes) = system_bytes(path, &mut MAX_TOTAL_BYTES.clone()).unwrap();
            assert!(elf::inspect(&bytes).is_ok());
        }
    }
    #[test]
    fn diagnostics_do_not_create_state_or_promote_unverified_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_base_dirs(
            temp.path().join("config"),
            temp.path().join("data"),
            temp.path().join("cache"),
        )
        .unwrap();
        let reference = pins::PinnedRuntimeReference::Managed(pins::ManagedRuntimeReference {
            installation_id: "wine-synthetic".into(),
            family: "wine".into(),
            lineage: "upstream-wine".into(),
            provider: "synthetic".into(),
            project: "synthetic".into(),
            build: "wine-10.0".into(),
            artifact_sha256: "a".repeat(64),
            manifest_sha256: "b".repeat(64),
            generation: 1,
            ownership_record_sha256: "c".repeat(64),
            trust: pins::Provenance::IndependentlyAuthenticatedPublisher,
        });
        fs::write(temp.path().join("unrelated"), b"preserved").unwrap();
        let report = qualify_managed_wine(&paths, "missing-pin", 1, &reference);
        assert_eq!(report.schema_version, 2);
        assert_eq!(
            report.manifest_authority_scope,
            ManifestAuthorityScope::IndependentlyAuthenticatedPublisher
        );
        assert_eq!(
            report.manifest_signature_state,
            ManifestSignatureState::PublisherProvenanceRecorded
        );
        assert_eq!(report.runtime_image_provider, "squashfuse");
        assert!(report.runtime_image_sha256.is_none());
        assert_eq!(report.mounted_view_eligibility, QualificationState::Blocked);
        assert_eq!(
            report.host_i386_abi,
            if Path::new("/lib/ld-linux.so.2").is_file() {
                QualificationState::Unknown
            } else {
                QualificationState::Missing
            }
        );
        assert_eq!(report.authentic_qualification, QualificationState::NotRun);
        assert_eq!(
            report.production_initialization,
            QualificationState::Blocked
        );
        assert_eq!(
            report.invocation.artifact_mapping,
            QualificationState::Unsupported
        );
        assert!(!paths.data_dir().exists());
        assert!(!paths.config_file().exists());
        assert!(!paths.cache_dir().exists());
        assert_eq!(
            fs::read(temp.path().join("unrelated")).unwrap(),
            b"preserved"
        );
        let development = pins::PinnedRuntimeReference::Managed(pins::ManagedRuntimeReference {
            installation_id: "wine-development".into(),
            family: "wine".into(),
            lineage: "upstream-wine".into(),
            provider: "endfield-unix-development".into(),
            project: "p034-disposable".into(),
            build: "wine-10.0".into(),
            artifact_sha256: "a".repeat(64),
            manifest_sha256: "b".repeat(64),
            generation: 1,
            ownership_record_sha256: "c".repeat(64),
            trust: pins::Provenance::DevelopmentOnly,
        });
        let dev_report = qualify_managed_wine(&paths, "development-pin", 1, &development);
        assert_eq!(
            dev_report.manifest_authority_scope,
            ManifestAuthorityScope::DevelopmentOnly
        );
        assert_eq!(
            dev_report.manifest_signature_state,
            ManifestSignatureState::DevelopmentOnlyProvenanceRecorded
        );
        assert_eq!(
            dev_report.mounted_view_eligibility,
            QualificationState::Blocked
        );
        assert_eq!(
            dev_report.production_initialization,
            QualificationState::Blocked
        );
        assert!(!paths.data_dir().exists());
    }
}
