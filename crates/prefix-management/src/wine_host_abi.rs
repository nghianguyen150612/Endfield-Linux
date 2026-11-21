//! Minimum native ABI policy for the authenticated headless initialization closure.
use crate::{
    wine_artifact_profile::{ComponentKind, ComponentUse, Profile},
    wine_qualification::{bounded_bytes, managed_bytes, system_bytes, trusted_system_path},
};
use endfield_unix_game_launch::{elf_dependencies as elf, linux_executor::TrustedRoot};
use endfield_unix_runtime_audit::Baseline;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{self, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

const SEARCH: &[&str] = &["/usr/lib/x86_64-linux-gnu", "/usr/lib64", "/usr/lib"];
const MAX_OBJECTS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostAbiState {
    SupportedUnderHostAssumptions,
    MissingRequiredDependency,
    AbiIncompatible,
    UnsupportedLoaderConfiguration,
    Unverifiable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostAbiEvidence {
    pub state: HostAbiState,
    pub object: Option<String>,
    pub required_version: Option<String>,
    pub observed_fingerprint: Option<String>,
    pub authentic_loading_observation_required: bool,
}
impl HostAbiEvidence {
    pub(crate) fn issue(state: HostAbiState, object: impl Into<String>) -> Self {
        Self {
            state,
            object: Some(object.into()),
            required_version: None,
            observed_fingerprint: None,
            authentic_loading_observation_required: true,
        }
    }
}

type HostResult<T> = Result<T, HostAbiEvidence>;

pub(crate) fn inspect(
    root: &TrustedRoot,
    baseline: &Baseline,
    profile: &Profile,
) -> HostAbiEvidence {
    match inspect_inner(root, baseline, profile) {
        Ok(observed_fingerprint) => HostAbiEvidence {
            state: HostAbiState::SupportedUnderHostAssumptions,
            object: None,
            required_version: None,
            observed_fingerprint: Some(observed_fingerprint),
            authentic_loading_observation_required: true,
        },
        Err(evidence) => evidence,
    }
}

fn inspect_inner(root: &TrustedRoot, baseline: &Baseline, profile: &Profile) -> HostResult<String> {
    use HostAbiState as State;
    let issue = |state, object: &str| HostAbiEvidence::issue(state, object);
    if !cfg!(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_env = "gnu"
    )) {
        return Err(issue(State::AbiIncompatible, "native_platform"));
    }
    if !fs::read_link("/proc/self/exe").is_ok_and(|path| path.is_absolute()) {
        return Err(issue(State::Unverifiable, "proc_self_exe"));
    }
    if !matches!(profile.fallback_server().symlink_metadata(), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
    {
        return Err(issue(
            State::UnsupportedLoaderConfiguration,
            "compiled_in_wineserver_fallback",
        ));
    }
    let mut budget = 256 * 1024 * 1024;
    let mut fingerprint = BTreeMap::new();
    for name in ["/etc/ld.so.preload", "/etc/ld.so.cache"] {
        let path = Path::new(name);
        match trusted_system_path(path) {
            Err(crate::wine_qualification::DependencyState::Missing) => continue,
            Ok(resolved) if resolved == path => {}
            _ => return Err(issue(State::Unverifiable, "host_loader_metadata")),
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| issue(State::Unverifiable, "host_loader_metadata"))?;
        if !file.metadata().is_ok_and(|m| m.is_file()) {
            return Err(issue(State::Unverifiable, "host_loader_metadata"));
        }
        let bytes = bounded_bytes(file, &mut budget)
            .map_err(|_| issue(State::Unverifiable, "host_loader_metadata"))?;
        if name.ends_with("preload") && bytes.iter().any(|byte| !byte.is_ascii_whitespace()) {
            return Err(issue(
                State::UnsupportedLoaderConfiguration,
                "ld.so.preload",
            ));
        }
        fingerprint.insert(name.to_owned(), format!("{:x}", Sha256::digest(bytes)));
    }

    let interpreter = Path::new(&profile.interpreter);
    let mut objects = BTreeMap::new();
    let mut pending: VecDeque<_> = profile.host_libraries.iter().cloned().collect();
    let (resolved_interpreter, bytes) = system_bytes(interpreter, &mut budget)
        .map_err(|state| dependency_issue(state, "interpreter"))?;
    if !resolved_interpreter
        .metadata()
        .is_ok_and(|m| m.mode() & 0o111 != 0)
    {
        return Err(issue(
            State::AbiIncompatible,
            "glibc_interpreter_permissions",
        ));
    }
    let linker = parse(&bytes, "interpreter")?;
    if linker.interpreter.is_some()
        || !linker
            .version_definitions
            .iter()
            .any(|v| v.starts_with("GLIBC_"))
    {
        return Err(issue(State::AbiIncompatible, "glibc_interpreter"));
    }
    fingerprint.insert("interpreter".into(), format!("{:x}", Sha256::digest(bytes)));
    pending.extend(linker.needed.iter().cloned());
    objects.insert("interpreter".into(), linker);
    for component in &profile.components {
        if !matches!(
            component.usage,
            ComponentUse::Initialization | ComponentUse::HostDependent
        ) || !matches!(
            component.kind,
            ComponentKind::NativeExecutable | ComponentKind::UnixModule
        ) {
            continue;
        }
        let bytes = managed_bytes(root, baseline, Path::new(&component.path), &mut budget)
            .map_err(|state| dependency_issue(state, &component.role))?;
        let object = inspect_managed(
            &bytes,
            &component.role,
            component.kind,
            &profile.interpreter,
        )?;
        pending.extend(object.needed.iter().cloned());
        objects.insert(format!("role:{}", component.role), object);
    }

    let mut seen = BTreeSet::new();
    while let Some(name) = pending.pop_front() {
        if !seen.insert(name.clone()) {
            continue;
        }
        if objects.len() >= MAX_OBJECTS {
            return Err(issue(State::Unverifiable, "native_dependency_limit"));
        }
        let mut selected: Option<PathBuf> = None;
        let mut observed = Vec::new();
        for dir in SEARCH {
            let path = Path::new(dir).join(&name);
            match trusted_system_path(&path) {
                Ok(resolved) => {
                    if selected
                        .as_ref()
                        .is_some_and(|previous| previous != &resolved)
                    {
                        return Err(issue(State::UnsupportedLoaderConfiguration, &name));
                    }
                    if selected.is_none() {
                        let (_, bytes) = system_bytes(&path, &mut budget)
                            .map_err(|state| dependency_issue(state, &name))?;
                        observed = bytes;
                        selected = Some(resolved);
                    }
                }
                Err(crate::wine_qualification::DependencyState::Missing) => {}
                Err(state) => return Err(dependency_issue(state, &name)),
            }
        }
        let resolved =
            selected.ok_or_else(|| issue(State::MissingRequiredDependency, name.as_str()))?;
        let object = parse(&observed, &name)?;
        if object
            .interpreter
            .as_deref()
            .is_some_and(|path| path != profile.interpreter)
        {
            return Err(issue(State::AbiIncompatible, &name));
        }
        let digest = format!("{:x}", Sha256::digest(observed));
        fingerprint.insert(name.clone(), format!("{}:{digest}", resolved.display()));
        pending.extend(object.needed.iter().cloned());
        objects.insert(name, object);
    }

    verify_versions(&objects)?;
    let bytes = serde_json::to_vec(&fingerprint)
        .map_err(|_| issue(State::Unverifiable, "host_fingerprint"))?;
    // Include observed kernel version and machine (bounded Linux procfs).
    let mut hash = Sha256::new();
    hash.update(bytes);
    for path in [
        "/proc/sys/kernel/osrelease",
        "/proc/sys/kernel/random/boot_id",
    ] {
        let content =
            fs::read_to_string(path).map_err(|_| issue(State::Unverifiable, "host_kernel"))?;
        hash.update(content.as_bytes());
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn verify_versions(objects: &BTreeMap<String, elf::ElfDependencies>) -> HostResult<()> {
    use HostAbiState as State;
    let issue = HostAbiEvidence::issue;
    for object in objects.values() {
        for requirement in &object.version_requirements {
            let provider = objects.get(&requirement.library).ok_or_else(|| {
                issue(
                    State::MissingRequiredDependency,
                    requirement.library.as_str(),
                )
            })?;
            for version in &requirement.versions {
                if !provider.version_definitions.contains(version) {
                    let mut error = issue(State::AbiIncompatible, requirement.library.as_str());
                    error.required_version = Some(version.clone());
                    return Err(error);
                }
            }
        }
    }
    Ok(())
}

fn inspect_managed(
    bytes: &[u8],
    role: &str,
    kind: ComponentKind,
    interpreter: &str,
) -> HostResult<elf::ElfDependencies> {
    let object = parse(bytes, role)?;
    let expected = (kind == ComponentKind::NativeExecutable).then_some(interpreter);
    if object.interpreter.as_deref() != expected
        || (kind == ComponentKind::UnixModule && object.executable_type != 3)
    {
        return Err(HostAbiEvidence::issue(HostAbiState::AbiIncompatible, role));
    }
    Ok(object)
}

fn dependency_issue(
    state: crate::wine_qualification::DependencyState,
    object: &str,
) -> HostAbiEvidence {
    use crate::wine_qualification::DependencyState;
    HostAbiEvidence::issue(
        match state {
            DependencyState::Missing => HostAbiState::MissingRequiredDependency,
            DependencyState::Unsupported => HostAbiState::UnsupportedLoaderConfiguration,
            _ => HostAbiState::Unverifiable,
        },
        object,
    )
}

fn parse(bytes: &[u8], object: &str) -> HostResult<elf::ElfDependencies> {
    let elf = elf::inspect(bytes).map_err(|e| {
        HostAbiEvidence::issue(
            match e {
                elf::ElfError::UnsupportedArchitecture => HostAbiState::AbiIncompatible,
                elf::ElfError::UnsupportedResolution => {
                    HostAbiState::UnsupportedLoaderConfiguration
                }
                _ => HostAbiState::Unverifiable,
            },
            object,
        )
    })?;
    if elf.rpath.is_some() || elf.runpath.is_some() {
        return Err(HostAbiEvidence::issue(
            HostAbiState::UnsupportedLoaderConfiguration,
            object,
        ));
    }
    Ok(elf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_fixture() -> Vec<u8> {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("abi.c");
        let executable = directory.path().join("abi");
        fs::write(
            &source,
            b"#include <stdio.h>\nint main(void) { return puts(\"fixture\") < 0; }\n",
        )
        .unwrap();
        assert!(
            std::process::Command::new("cc")
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .status()
                .unwrap()
                .success()
        );
        fs::read(executable).unwrap()
    }

    #[test]
    fn observed_import_versions_must_exist_in_the_selected_provider() {
        let current = native_fixture();
        let consumer = parse(&current, "fixture").unwrap();
        assert_eq!((consumer.class, consumer.machine), (64, 62));
        let (library, version) = consumer
            .version_requirements
            .iter()
            .find(|entry| entry.library == "libc.so.6")
            .map(|entry| (&entry.library, &entry.versions[0]))
            .unwrap();
        assert!(version.starts_with("GLIBC_"));
        let key = library.clone();
        let mut provider = consumer.clone();
        provider.version_requirements.clear();
        let mut isolated_consumer = consumer.clone();
        isolated_consumer
            .version_requirements
            .retain(|entry| &entry.library == library);
        assert_eq!(
            verify_versions(&BTreeMap::from([(
                "fixture".into(),
                isolated_consumer.clone()
            )]))
            .unwrap_err()
            .state,
            HostAbiState::MissingRequiredDependency
        );
        let mut objects = BTreeMap::from([
            ("fixture".into(), isolated_consumer.clone()),
            (key.clone(), provider.clone()),
        ]);
        let mismatch = verify_versions(&objects).unwrap_err();
        assert_eq!(mismatch.state, HostAbiState::AbiIncompatible);
        assert_eq!(mismatch.required_version.as_ref(), Some(version));
        provider.version_definitions = isolated_consumer.version_requirements[0].versions.clone();
        objects.insert(key, provider);
        assert!(verify_versions(&objects).is_ok());
    }

    #[test]
    fn native_machine_and_exact_interpreter_are_required() {
        let mut bytes = native_fixture();
        let interpreter = parse(&bytes, "fixture").unwrap().interpreter.unwrap();
        assert!(
            inspect_managed(
                &bytes,
                "fixture",
                ComponentKind::NativeExecutable,
                &interpreter
            )
            .is_ok()
        );
        assert_eq!(
            inspect_managed(
                &bytes,
                "fixture",
                ComponentKind::NativeExecutable,
                "/different/linker"
            )
            .unwrap_err()
            .state,
            HostAbiState::AbiIncompatible
        );
        bytes[18] = 183;
        assert_eq!(
            parse(&bytes, "fixture").unwrap_err().state,
            HostAbiState::AbiIncompatible
        );
    }
}

#[cfg(test)]
mod candidate_observation {
    use super::*;

    /// Read-only developer observation. Never invokes Wine or creates managed state.
    #[test]
    #[ignore = "requires explicitly acquired non-executable candidate bytes"]
    fn inspect_real_candidate_without_execution() {
        let root = std::path::PathBuf::from(std::env::var("ENDFIELD_CANDIDATE_STAGING").unwrap());
        let output =
            std::path::PathBuf::from(std::env::var("ENDFIELD_CANDIDATE_ABI_REPORT").unwrap());
        let candidate: crate::wine_candidate::CandidateProfile =
            serde_json::from_str(include_str!(
                "../../../docs/artifact-evidence/winehq-noble-10.0-candidate-profile.json"
            ))
            .unwrap();
        for role in &candidate.roles {
            if matches!(role.kind.as_str(), "native_executable" | "unix_module") {
                candidate
                    .validate_native_bytes(
                        &role.role,
                        &std::fs::read(root.join(&role.path)).unwrap(),
                    )
                    .unwrap();
            }
        }
        let baseline = endfield_unix_runtime_audit::create_baseline(
            &root,
            "candidate-observation",
            1,
            "wine",
            "wine-10.0",
            &"0".repeat(64),
            &"0".repeat(64),
            endfield_unix_runtime_audit::TrustClassification::UserPinnedLimitedTrust,
        )
        .unwrap();
        let trusted = crate::wine_qualification::trusted_directory(&root).unwrap();
        let mut observations = Vec::new();
        let mut components = Vec::new();
        for entry in &baseline.entries {
            if !matches!(
                entry.kind,
                endfield_unix_runtime_audit::EntryKind::File { .. }
            ) {
                continue;
            }
            let bytes = std::fs::read(root.join(&entry.path)).unwrap();
            if !bytes.starts_with(b"\x7fELF") {
                continue;
            }
            let kind = if entry.path.contains("/bin/") {
                ComponentKind::NativeExecutable
            } else {
                ComponentKind::UnixModule
            };
            let assessment =
                inspect_managed(&bytes, &entry.path, kind, "/lib64/ld-linux-x86-64.so.2");
            observations.push(serde_json::json!({"path":entry.path,
                "sha256":endfield_unix_runtime_verification::sha256(&bytes),
                "elf":elf::inspect(&bytes), "policy":assessment}));
            components.push(
                serde_json::json!({"role":format!("native{}",components.len()),
                "path":entry.path, "kind":kind, "usage":"initialization"}),
            );
        }
        let mut profile: Profile = serde_json::from_value(serde_json::json!({
            "schema_version":1,"build":"wine-10.0","source_ref":"wine-10.0",
            "source_commit":"b073859675060c9211fcbccfd90e4e87520dc2c2", "architecture":"x86_64",
            "build_configuration":"observation-only", "invocation":"NOT_RUN",
            "configure_prefix":"/opt/wine-stable", "libdir":"lib64",
            "interpreter":"/lib64/ld-linux-x86-64.so.2", "host_libraries":[],
            "inventory_sha256":baseline.inventory_sha256,"components":components
        }))
        .unwrap();
        let full = inspect(&trusted, &baseline, &profile);
        profile
            .components
            .retain(|c| !c.path.ends_with("-preloader"));
        let without_preloader = inspect(&trusted, &baseline, &profile);
        let report = serde_json::json!({"schema_version":1,"host_os":std::fs::read_to_string("/etc/os-release").unwrap(),
            "staging_inventory_sha256":baseline.inventory_sha256,
            "note":"Staging modes are non-executable. This is byte inspection, not installed P026 evidence. Excluding preloader is diagnostic only.",
            "native_objects":observations,"full_policy":full,"diagnostic_without_preloader":without_preloader,
            "authentic_execution":"NOT_RUN","production_initialization":"BLOCKED"});
        std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}

#[cfg(test)]
mod dualarch_observation {
    use super::*;
    #[test]
    #[ignore = "read-only acquired dualarch bytes; never runs Wine"]
    fn inspect_actual_dualarch_candidate() {
        use crate::{wine_candidate::CandidateProfile, wine_native_abi};
        let profile: CandidateProfile = serde_json::from_str(include_str!(
            "../../../docs/artifact-evidence/winehq-dualarch-profile.json"
        ))
        .unwrap();
        let inventory: Vec<endfield_unix_runtime_audit::Entry> =
            serde_json::from_str(include_str!(
                "../../../docs/artifact-evidence/winehq-dualarch-transformed-inventory.json"
            ))
            .unwrap();
        profile.validate(&inventory).unwrap();
        let sources: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/artifact-evidence/winehq-dualarch-source-inventory.json"
        ))
        .unwrap();
        let roots: BTreeMap<String, PathBuf> =
            serde_json::from_str(&std::env::var("ENDFIELD_WINE_SOURCE_ROOTS").unwrap()).unwrap();
        let mut objects = Vec::new();
        let mut preloaders = Vec::new();
        let mut dependencies = BTreeMap::new();
        let mut budget = 256 * 1024 * 1024;
        for owned in sources["entry_ownership"].as_array().unwrap() {
            let path = owned["path"].as_str().unwrap();
            let entry = inventory.iter().find(|e| e.path == path).unwrap();
            if !matches!(
                entry.kind,
                endfield_unix_runtime_audit::EntryKind::File { .. }
            ) {
                continue;
            }
            let owner = owned["packages"][0].as_str().unwrap();
            let mut file = std::fs::File::open(roots[owner].join(path)).unwrap();
            let mut magic = [0; 4];
            use std::io::Read;
            if file.read_exact(&mut magic).is_err() || magic != *b"\x7fELF" {
                continue;
            }
            let bytes = std::fs::read(roots[owner].join(path)).unwrap();
            if let endfield_unix_runtime_audit::EntryKind::File { sha256, .. } = &entry.kind {
                assert_eq!(*sha256, endfield_unix_runtime_verification::sha256(&bytes));
            }
            let parsed = elf::inspect_candidate(&bytes);
            for role in profile.roles.iter().filter(|r| r.path == path) {
                profile.validate_native_bytes(&role.role, &bytes).unwrap();
                if matches!(role.role.as_str(), "preloader" | "preloader_i386") {
                    preloaders.push(serde_json::json!({"role":role.role,"assessment":wine_native_abi::assess_candidate_preloader(&profile,&inventory,&role.role,&bytes)}));
                }
            }
            if let Ok(object) = &parsed {
                let dirs = if object.class == 64 {
                    vec!["/usr/lib/x86_64-linux-gnu", "/usr/lib64"]
                } else {
                    vec!["/usr/lib/i386-linux-gnu", "/usr/lib32"]
                };
                for name in &object.needed {
                    let key = format!("{}:{name}", object.class);
                    if dependencies.contains_key(&key) {
                        continue;
                    }
                    let managed: Vec<_> = inventory
                        .iter()
                        .filter(|e| {
                            profile
                                .unix_module_directories
                                .iter()
                                .any(|d| e.path == format!("{d}/{name}"))
                        })
                        .map(|e| e.path.clone())
                        .collect();
                    let mut providers = Vec::new();
                    for dir in &dirs {
                        let candidate = Path::new(dir).join(name);
                        if let Ok((resolved, bytes)) = system_bytes(&candidate, &mut budget) {
                            let observed = elf::inspect_candidate(&bytes);
                            providers.push(serde_json::json!({"path":resolved,"sha256":endfield_unix_runtime_verification::sha256(&bytes),"elf":observed}));
                        }
                    }
                    dependencies.insert(key,serde_json::json!({"managed_candidates":managed,"observed_system_providers":providers,
                        "resolution":"STATIC_CANDIDATES_ONLY_ACTUAL_LOADER_RESOLUTION_UNVERIFIED"}));
                }
            }
            objects.push(serde_json::json!({"path":path,"sha256":endfield_unix_runtime_verification::sha256(&bytes),"classification":wine_native_abi::classify(&bytes),"elf":parsed}));
        }
        let mut version_checks = Vec::new();
        for consumer in &objects {
            if let Some(object) = consumer["elf"]["Ok"].as_object() {
                let class = object["class"].as_u64().unwrap();
                for requirement in object["version_requirements"].as_array().unwrap() {
                    let name = requirement["library"].as_str().unwrap();
                    let providers =
                        &dependencies[&format!("{class}:{name}")]["observed_system_providers"];
                    let satisfied =
                        providers.as_array().unwrap().iter().any(|p| {
                            p["elf"]["Ok"]["class"].as_u64() == Some(class)
                                && requirement["versions"].as_array().unwrap().iter().all(
                                    |version| {
                                        p["elf"]["Ok"]["version_definitions"]
                                            .as_array()
                                            .is_some_and(|defs| defs.contains(version))
                                    },
                                )
                        });
                    version_checks.push(serde_json::json!({"consumer":consumer["path"],"library":name,"required":requirement["versions"],"satisfied_in_observed_provider":satisfied,"actual_loader_selection_verified":false}));
                }
            }
        }
        let probe = tempfile::tempdir().unwrap();
        let capabilities = crate::runtime_view::inspect_capabilities(probe.path()).unwrap();
        let report = serde_json::json!({"schema_version":1,"host_os":std::fs::read_to_string("/etc/os-release").unwrap(),"host_kernel":std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap(),
            "artifact_sha256":profile.derived_artifact_sha256,"profile_sha256":endfield_unix_runtime_verification::sha256(include_bytes!("../../../docs/artifact-evidence/winehq-dualarch-profile.json")),"inventory_sha256":profile.inventory_sha256,"native_objects":objects,"preloaders":preloaders,"dependencies":dependencies,"gnu_symbol_checks":version_checks,
            "i386_interpreter_present":Path::new("/lib/ld-linux.so.2").exists(),"kernel_ia32_execution":"NOT_TESTED","runtime_view":capabilities,
            "host_abi":"UNQUALIFIED_STATIC_OBSERVATIONS_ONLY","gnu_version_definitions":"Recorded per provider; full multiarch symbol/loader closure remains unverified",
            "authentic_execution":"NOT_RUN","production_initialization":"BLOCKED"});
        std::fs::write(
            std::env::var("ENDFIELD_DUALARCH_ABI_REPORT").unwrap(),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
}
