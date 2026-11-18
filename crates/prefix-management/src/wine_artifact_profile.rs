//! P024-authenticated package roles, bound to the current P025/P026/P027 evidence.
use crate::wine_qualification::{bounded_bytes, managed_bytes, trusted_directory};
use endfield_unix_game_launch::linux_executor::TrustedRoot;
use endfield_unix_runtime_audit::{Baseline, BaselineOrigin, EntryKind};
use endfield_unix_runtime_selection::ManagedRuntimeReference;
use endfield_unix_runtime_verification as verification;
use endfield_unix_storage::AppPaths;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, os::unix::fs::MetadataExt, path::Path};

const MAX_PROFILE: u64 = 128 * 1024;
const MAX_ROLES: usize = 256;
pub(crate) const CORE: &[(&str, &str, ComponentKind)] = &[
    ("launcher", "bin/wine", ComponentKind::NativeExecutable),
    (
        "wineserver",
        "bin/wineserver",
        ComponentKind::NativeExecutable,
    ),
    (
        "ntdll_unix",
        "x86_64-unix/ntdll.so",
        ComponentKind::UnixModule,
    ),
    (
        "ntdll",
        "x86_64-windows/ntdll.dll",
        ComponentKind::WindowsModule,
    ),
    (
        "wineboot",
        "x86_64-windows/wineboot.exe",
        ComponentKind::WindowsModule,
    ),
    (
        "kernel32",
        "x86_64-windows/kernel32.dll",
        ComponentKind::WindowsModule,
    ),
    (
        "kernelbase",
        "x86_64-windows/kernelbase.dll",
        ComponentKind::WindowsModule,
    ),
    (
        "services",
        "x86_64-windows/services.exe",
        ComponentKind::WindowsModule,
    ),
    (
        "rundll32",
        "x86_64-windows/rundll32.exe",
        ComponentKind::WindowsModule,
    ),
    (
        "wow64_rundll32",
        "i386-windows/rundll32.exe",
        ComponentKind::WindowsModule,
    ),
    (
        "wow64_setupapi",
        "i386-windows/setupapi.dll",
        ComponentKind::WindowsModule,
    ),
    (
        "setupapi",
        "x86_64-windows/setupapi.dll",
        ComponentKind::WindowsModule,
    ),
    (
        "advapi32",
        "x86_64-windows/advapi32.dll",
        ComponentKind::WindowsModule,
    ),
    (
        "shell32",
        "x86_64-windows/shell32.dll",
        ComponentKind::WindowsModule,
    ),
    (
        "ws2_32",
        "x86_64-windows/ws2_32.dll",
        ComponentKind::WindowsModule,
    ),
    (
        "newdev",
        "x86_64-windows/newdev.dll",
        ComponentKind::WindowsModule,
    ),
    ("wine_inf", "share/wine/wine.inf", ComponentKind::Resource),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ComponentKind {
    NativeExecutable,
    UnixModule,
    WindowsModule,
    Resource,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentUse {
    Initialization,
    Optional,
    HostDependent,
    Gameplay,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Component {
    pub role: String,
    pub path: String,
    pub kind: ComponentKind,
    pub usage: ComponentUse,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Profile {
    schema_version: u32,
    build: String,
    source_ref: String,
    source_commit: String,
    architecture: String,
    build_configuration: String,
    invocation: String,
    /// Configured prefix with bin, share and the supplied relative libdir.
    configure_prefix: String,
    libdir: String,
    pub interpreter: String,
    pub host_libraries: Vec<String>,
    inventory_sha256: String,
    pub components: Vec<Component>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactProfileBlocker {
    #[error("authenticated artifact profile or manifest is missing")]
    Missing,
    #[error("artifact profile or manifest could not be read safely")]
    Unreadable,
    #[error("profile is not authenticated by the installed P024 manifest")]
    ProvenanceMismatch,
    #[error("artifact profile schema is invalid")]
    InvalidSchema,
    #[error("Wine build, configuration, or invocation is unsupported")]
    UnsupportedBuild,
    #[error("artifact profile architecture is unsupported")]
    WrongArchitecture,
    #[error("P025 generation or P026 inventory differs from the authenticated scope")]
    BaselineMismatch,
    #[error("a required component role is missing")]
    MissingRole,
    #[error("a component role or path is ambiguous")]
    AmbiguousRole,
    #[error("a component path is unsafe or incompatible with loader configuration")]
    UnsafePath,
    #[error("required component {0} could not be verified against the authenticated inventory")]
    ComponentIntegrity(String),
}

pub(crate) struct VerifiedProfile {
    pub id: String,
    pub profile: Profile,
    pub baseline_id: String,
    pub wine_inf_mtime: u32,
}

impl VerifiedProfile {
    pub fn load(
        paths: &AppPaths,
        runtime: &ManagedRuntimeReference,
        root: &TrustedRoot,
        baseline: &Baseline,
    ) -> Result<Self, ArtifactProfileBlocker> {
        use ArtifactProfileBlocker as Error;
        let sidecar = paths
            .data_dir()
            .join("wine-qualification")
            .join(&runtime.installation_id);
        let sidecar = trusted_directory(&sidecar).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::Missing
            } else {
                Error::Unreadable
            }
        })?;
        let read = |name, mut max| {
            let file = sidecar
                .open_regular(Path::new(name))
                .map_err(|error| match error {
                    endfield_unix_game_launch::linux_executor::ExecutionError::Open(e)
                        if e.kind() == std::io::ErrorKind::NotFound =>
                    {
                        Error::Missing
                    }
                    _ => Error::Unreadable,
                })?;
            bounded_bytes(file, &mut max).map_err(|_| Error::Unreadable)
        };
        let manifest = read("manifest.json", verification::MAX_MANIFEST_BYTES)?;
        let profile = read("profile.json", MAX_PROFILE as usize)?;
        let mut verified = Self::validate(&manifest, &profile, runtime, baseline)?;
        let mut budget = 256 * 1024 * 1024;
        for component in &verified.profile.components {
            if !matches!(
                component.usage,
                ComponentUse::Initialization | ComponentUse::HostDependent
            ) {
                continue;
            }
            managed_bytes(root, baseline, Path::new(&component.path), &mut budget)
                .map_err(|_| Error::ComponentIntegrity(component.role.clone()))?;
        }
        verified.wine_inf_mtime = root
            .open_regular(Path::new("share/wine/wine.inf"))
            .ok()
            .and_then(|file| file.metadata().ok())
            .and_then(|metadata| u32::try_from(metadata.mtime()).ok())
            .ok_or_else(|| Error::ComponentIntegrity("wine_inf".into()))?;
        // Stable output and plan IDs regardless of input record order.
        verified
            .profile
            .components
            .sort_by(|a, b| a.role.cmp(&b.role));
        Ok(verified)
    }

    pub(crate) fn validate(
        manifest: &[u8],
        raw_profile: &[u8],
        runtime: &ManagedRuntimeReference,
        baseline: &Baseline,
    ) -> Result<Self, ArtifactProfileBlocker> {
        use ArtifactProfileBlocker as Error;
        if verification::sha256(manifest) != runtime.manifest_sha256
            || runtime.trust
                != endfield_unix_runtime_selection::Provenance::IndependentlyAuthenticatedPublisher
        {
            return Err(Error::ProvenanceMismatch);
        }
        let manifest =
            verification::Manifest::parse(manifest).map_err(|_| Error::ProvenanceMismatch)?;
        if manifest.provider_id != runtime.provider
            || manifest.project_id != runtime.project
            || manifest.runtime_family != runtime.family
            || manifest.runtime_lineage != runtime.lineage
            || manifest.runtime_build != runtime.build
        {
            return Err(Error::ProvenanceMismatch);
        }
        let id = verification::sha256(raw_profile);
        let mut matches = manifest
            .artifacts
            .iter()
            .filter(|a| a.sha256 == runtime.artifact_sha256);
        let artifact = matches.next().ok_or(Error::ProvenanceMismatch)?;
        if matches.next().is_some()
            || artifact.wine_qualification_profile_sha256.as_deref() != Some(&id)
        {
            return Err(Error::ProvenanceMismatch);
        }
        let profile: Profile =
            serde_json::from_slice(raw_profile).map_err(|_| Error::InvalidSchema)?;
        profile.validate_layout()?;
        if baseline.validate().is_err()
            || baseline.installation_id != runtime.installation_id
            || baseline.generation != runtime.generation
            || baseline.build != runtime.build
            || baseline.family != runtime.family
            || baseline.manifest_sha256 != runtime.manifest_sha256
            || baseline.artifact_sha256 != runtime.artifact_sha256
            || baseline.trust != endfield_unix_runtime_audit::TrustClassification::IndependentlyAuthenticatedPublisher
            || baseline.origin != BaselineOrigin::VerifiedInstallation
            || baseline.inventory_sha256 != profile.inventory_sha256
        {
            return Err(Error::BaselineMismatch);
        }
        for component in &profile.components {
            let entry = baseline
                .entries
                .binary_search_by_key(&component.path.as_str(), |entry| entry.path.as_str())
                .ok()
                .map(|position| &baseline.entries[position]);
            if !matches!(entry.map(|e| &e.kind), Some(EntryKind::File { setid: false, executable, .. })
                if component.kind != ComponentKind::NativeExecutable || *executable)
            {
                return Err(Error::ComponentIntegrity(component.role.clone()));
            }
        }
        if baseline.entries.iter().any(|e| {
            matches!(
                e.path.as_str(),
                "bin/wine-preloader" | "bin/wine64-preloader" | "bin/wine64"
            )
        }) {
            return Err(Error::UnsupportedBuild);
        }
        let baseline_id = verification::sha256(
            &serde_json::to_vec(baseline).map_err(|_| Error::BaselineMismatch)?,
        );
        Ok(Self {
            id,
            profile,
            baseline_id,
            wine_inf_mtime: 0,
        })
    }
}

impl Profile {
    pub fn fallback_server(&self) -> std::path::PathBuf {
        Path::new(&self.configure_prefix).join("bin/wineserver")
    }

    fn validate_layout(&self) -> Result<(), ArtifactProfileBlocker> {
        use ArtifactProfileBlocker as Error;
        if self.schema_version != 1
            || self.components.len() > MAX_ROLES
            || self.host_libraries.len() > 64
        {
            return Err(Error::InvalidSchema);
        }
        if self.architecture != "x86_64" {
            return Err(Error::WrongArchitecture);
        }
        if self.build != "wine-10.0"
            || self.source_ref != "wine-10.0"
            || self.source_commit != "b073859675060c9211fcbccfd90e4e87520dc2c2"
            || self.build_configuration != "linux-x86_64-wow64-no-preloader-headless"
            || self.invocation != "wineboot.exe --init"
        {
            return Err(Error::UnsupportedBuild);
        }
        if !relative(&self.libdir)
            || !self
                .configure_prefix
                .strip_prefix('/')
                .is_some_and(relative)
            || !matches!(
                self.interpreter.as_str(),
                "/lib64/ld-linux-x86-64.so.2" | "/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2"
            )
            || self
                .host_libraries
                .iter()
                .any(|name| !relative(name) || name.contains('/'))
        {
            return Err(Error::UnsafePath);
        }
        let mut paths = BTreeSet::new();
        let mut roles = BTreeSet::new();
        for component in &self.components {
            if !verification::identifier(&component.role) || !relative(&component.path) {
                return Err(Error::UnsafePath);
            }
            if !paths.insert(&component.path) || !roles.insert(&component.role) {
                return Err(Error::AmbiguousRole);
            }
        }
        for &(role, path, kind) in CORE {
            let component = self
                .components
                .iter()
                .find(|c| c.role == role)
                .ok_or(Error::MissingRole)?;
            let correct_path = if path.starts_with("x86_64-") || path.starts_with("i386-") {
                component.path == format!("{}/wine/{path}", self.libdir)
            } else {
                component.path == path
            };
            if !correct_path
                || component.kind != kind
                || component.usage != ComponentUse::Initialization
            {
                return Err(Error::UnsafePath);
            }
        }
        Ok(())
    }
}

fn relative(value: &str) -> bool {
    value.len() <= 512
        && value
            .split('/')
            .all(|part| verification::identifier(part) && !matches!(part, "." | ".."))
}
