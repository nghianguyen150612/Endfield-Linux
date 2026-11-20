//! Non-authorizing real-layout evidence. A validated observation cannot create a
//! P025 installation, P027 pin, or an authentic execution capability.
use crate::wine_artifact_profile::ArtifactProfileBlocker as Error;
use endfield_unix_runtime_audit::{Entry, EntryKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateProvenance {
    RepositoryIntegrityPublisherIdentityUnverified,
    SourceAuthenticatedLocalBuild,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateRole {
    pub role: String,
    pub path: String,
    pub sha256: String,
    pub kind: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateProfile {
    pub schema_version: u32,
    pub source_version: String,
    pub source_commit: String,
    pub source_relationship: String,
    pub artifact_build: String,
    pub architecture: String,
    pub build_configuration: String,
    pub installed_prefix: String,
    pub launcher_role: String,
    pub launcher_path: String,
    pub preloader_policy: String,
    pub wineserver_path: String,
    pub unix_module_directories: Vec<String>,
    pub windows_module_directories: Vec<String>,
    pub required_initialization_roles: Vec<String>,
    pub required_resource_roles: Vec<String>,
    pub native_helper_roles: Vec<String>,
    pub invocation_semantics: String,
    pub compiler_provenance: String,
    pub required_host_abi: String,
    pub package_sha256: String,
    pub inventory_sha256: String,
    pub provenance: CandidateProvenance,
    pub roles: Vec<CandidateRole>,
    pub missing_companion_packages: Vec<String>,
}

impl CandidateProfile {
    /// Structural and byte-role validation only; never grants execution trust.
    pub fn validate(&self, entries: &[Entry]) -> Result<(), Error> {
        use endfield_unix_runtime_verification::sha256;
        if self.schema_version != 2 || self.roles.len() > 256 || entries.len() > 30_000 {
            return Err(Error::InvalidSchema);
        }
        if self.architecture != "x86_64" {
            return Err(Error::WrongArchitecture);
        }
        if self.source_version != "10.0"
            || self.source_commit != "b073859675060c9211fcbccfd90e4e87520dc2c2"
            || self.source_relationship != "upstream_reference_not_binary_attestation"
            || self.preloader_policy != "required_pathname_handoff_unverified"
            || self.invocation_semantics != "descriptor_then_upstream_pathname_reexec"
        {
            return Err(Error::UnsupportedBuild);
        }
        if !digest(&self.package_sha256)
            || sha256(&serde_json::to_vec(entries).map_err(|_| Error::InvalidSchema)?)
                != self.inventory_sha256
            || entries.windows(2).any(|e| e[0].path >= e[1].path)
        {
            return Err(Error::BaselineMismatch);
        }
        let mut paths = BTreeSet::new();
        for entry in entries {
            if !safe(&entry.path) || !paths.insert(&entry.path) {
                return Err(Error::UnsafePath);
            }
            if let EntryKind::Symlink { target } = &entry.kind {
                if !safe(target) {
                    return Err(Error::UnsafePath);
                }
            }
        }
        if !self.installed_prefix.strip_prefix('/').is_some_and(safe)
            || self.unix_module_directories.is_empty()
            || self.windows_module_directories.is_empty()
            || self
                .unix_module_directories
                .iter()
                .chain(&self.windows_module_directories)
                .any(|p| !safe(p) || !p.starts_with(&format!("{}/", &self.installed_prefix[1..])))
        {
            return Err(Error::UnsafePath);
        }
        for required in [
            "launcher",
            "ntdll_unix",
            "ntdll",
            "wineboot",
            "kernel32",
            "kernelbase",
            "services",
            "rundll32",
            "setupapi",
            "advapi32",
            "shell32",
            "ws2_32",
            "newdev",
        ] {
            if !self
                .required_initialization_roles
                .iter()
                .any(|r| r == required)
            {
                return Err(Error::MissingRole);
            }
        }
        if !self.required_resource_roles.iter().any(|r| r == "wine_inf")
            || !["preloader", "wineserver"]
                .iter()
                .all(|name| self.native_helper_roles.iter().any(|r| r == name))
        {
            return Err(Error::MissingRole);
        }
        let mut roles = BTreeSet::new();
        let mut mapped_paths = BTreeSet::new();
        for role in &self.roles {
            if !endfield_unix_runtime_verification::identifier(&role.role)
                || !safe(&role.path)
                || !digest(&role.sha256)
            {
                return Err(Error::UnsafePath);
            }
            if !roles.insert(&role.role) || !mapped_paths.insert(&role.path) {
                return Err(Error::AmbiguousRole);
            }
            let entry = entries
                .iter()
                .find(|e| e.path == role.path)
                .ok_or(Error::MissingRole)?;
            // Exact regular-file entries only; symlink ancestors cannot authenticate a role.
            if entries.iter().any(|e| {
                role.path.starts_with(&format!("{}/", e.path))
                    && !matches!(e.kind, EntryKind::Directory { .. })
            }) {
                return Err(Error::UnsafePath);
            }
            if !matches!(&entry.kind, EntryKind::File { sha256, executable, setid: false, .. }
                if *sha256 == role.sha256 && (role.kind != "native_executable" || *executable))
                || !matches!(
                    role.kind.as_str(),
                    "native_executable" | "unix_module" | "windows_module" | "resource"
                )
            {
                return Err(Error::ComponentIntegrity(role.role.clone()));
            }
        }
        for required in self
            .required_initialization_roles
            .iter()
            .chain(&self.required_resource_roles)
            .chain(&self.native_helper_roles)
        {
            if !roles.contains(required) {
                return Err(Error::MissingRole);
            }
        }
        for (name, path) in [
            (&self.launcher_role, &self.launcher_path),
            (&"wineserver".to_owned(), &self.wineserver_path),
        ] {
            if !self
                .roles
                .iter()
                .any(|r| &r.role == name && &r.path == path && r.kind == "native_executable")
            {
                return Err(Error::MissingRole);
            }
        }
        if !self.native_helper_roles.iter().any(|r| r == "preloader")
            || !self
                .roles
                .iter()
                .any(|r| r.role == "preloader" && r.kind == "native_executable")
        {
            return Err(Error::MissingRole);
        }
        Ok(())
    }

    /// Checks exact mapped native bytes without opening a pathname or running code.
    pub fn validate_native_bytes(&self, role: &str, bytes: &[u8]) -> Result<(), Error> {
        let mapped = self
            .roles
            .iter()
            .find(|r| r.role == role)
            .ok_or(Error::MissingRole)?;
        if !matches!(mapped.kind.as_str(), "native_executable" | "unix_module")
            || endfield_unix_runtime_verification::sha256(bytes) != mapped.sha256
        {
            return Err(Error::ComponentIntegrity(role.into()));
        }
        let elf = endfield_unix_game_launch::elf_dependencies::inspect(bytes)
            .map_err(|_| Error::WrongArchitecture)?;
        if elf.machine != 62 || elf.class != 64 || !matches!(elf.executable_type, 2 | 3) {
            return Err(Error::WrongArchitecture);
        }
        Ok(())
    }

    pub fn execution_blocker(&self) -> Error {
        Error::PreloaderHandoffUnverified
    }
}

fn safe(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
