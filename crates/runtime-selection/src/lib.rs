//! Persistent runtime preference records and deterministic, non-executable selection.
//!
//! This crate never discovers, installs, audits, or executes a runtime. Callers supply
//! identity revalidation when resolving a stored reference.
use endfield_unix_runtime::{ProjectId, ProviderId, RuntimeVersion};
pub mod evidence;
pub use evidence::{CompatibilityEvidenceEvaluation, evaluate_pinned_compatibility};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use thiserror::Error;

pub const REGISTRY_SCHEMA_VERSION: u32 = 2;
const LEGACY_REGISTRY_SCHEMA_VERSION: u32 = 1;
pub const MAX_PINS: usize = 256;
const MAX_REGISTRY_BYTES: u64 = 256 * 1024;
const MACHINE_DOMAIN: &[u8] = b"endfield-unix:machine-identity:v1\0";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error)]
pub enum SelectionError {
    #[error("stable machine identity is unavailable")]
    MachineIdentityUnavailable,
    #[error("machine identity is invalid")]
    InvalidMachineIdentity,
    #[error("profile identifier is invalid")]
    InvalidProfileId,
    #[error("runtime identity is invalid")]
    InvalidRuntimeIdentity,
    #[error("pin already exists; explicit replacement and current revision are required")]
    PinExists,
    #[error("pin does not exist")]
    PinMissing,
    #[error("registry revision conflict")]
    RevisionConflict,
    #[error("registry is corrupt or unsupported")]
    RegistryCorrupt,
    #[error("registry exceeds configured limits")]
    RegistryLimit,
    #[error("registry path is unsafe")]
    UnsafePath,
    #[error("I/O operation failed")]
    Io(#[from] std::io::Error),
}

/// Derive an application-scoped identifier from a validated Linux machine-id.
/// It is pseudonymous, not anonymous; raw source bytes are never returned or logged.
pub fn machine_id_from_bytes(raw: &[u8]) -> Result<MachineId, SelectionError> {
    let text = std::str::from_utf8(raw).map_err(|_| SelectionError::InvalidMachineIdentity)?;
    let value = text.trim();
    if value.len() != 32 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(SelectionError::InvalidMachineIdentity);
    }
    let mut hash = Sha256::new();
    hash.update(MACHINE_DOMAIN);
    hash.update(value.to_ascii_lowercase().as_bytes());
    Ok(MachineId(format!("{:x}", hash.finalize())))
}

pub fn current_machine_id() -> Result<MachineId, SelectionError> {
    #[cfg(target_os = "linux")]
    {
        let metadata = fs::symlink_metadata("/etc/machine-id")
            .map_err(|_| SelectionError::MachineIdentityUnavailable)?;
        if !metadata.file_type().is_file() {
            return Err(SelectionError::MachineIdentityUnavailable);
        }
        let bytes =
            fs::read("/etc/machine-id").map_err(|_| SelectionError::MachineIdentityUnavailable)?;
        machine_id_from_bytes(&bytes)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(SelectionError::MachineIdentityUnavailable)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MachineId(String);
impl MachineId {
    pub fn from_digest(value: impl Into<String>) -> Result<Self, SelectionError> {
        let value = value.into();
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(SelectionError::InvalidMachineIdentity);
        }
        Ok(Self(value.to_ascii_lowercase()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProfileKey(String);
impl ProfileKey {
    pub fn new(value: impl Into<String>) -> Result<Self, SelectionError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 96
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_ .".contains(&b))
            || value.starts_with('.')
            || value.contains("..")
        {
            return Err(SelectionError::InvalidProfileId);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimePinScope {
    Machine {
        machine: MachineId,
    },
    Profile {
        machine: MachineId,
        profile: ProfileKey,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedRuntimeReference {
    pub installation_id: String,
    pub family: String,
    pub lineage: String,
    pub provider: String,
    pub project: String,
    pub build: String,
    pub artifact_sha256: String,
    pub manifest_sha256: String,
    pub generation: u64,
    /// Digest of the exact validated P025 ownership record at pin time.
    pub ownership_record_sha256: String,
    pub trust: Provenance,
}
impl ManagedRuntimeReference {
    pub fn validate(&self) -> Result<(), SelectionError> {
        let safe_text = |s: &str, max: usize| {
            !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control)
        };
        let digest = |s: &str| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        };
        if !safe_text(&self.installation_id, 256)
            || self.installation_id.contains('/')
            || self.installation_id.contains("..")
            || !matches!(self.family.as_str(), "wine" | "proton")
            || !matches!(
                (self.family.as_str(), self.lineage.as_str()),
                ("wine", "upstream-wine" | "wine-staging" | "custom")
                    | (
                        "proton",
                        "valve-proton" | "ge-proton" | "dw-proton" | "custom"
                    )
            )
            || !safe_text(&self.provider, 128)
            || !safe_text(&self.project, 128)
            || !safe_text(&self.build, 160)
            || ProviderId::new(self.provider.clone()).is_err()
            || ProjectId::new(self.project.clone()).is_err()
            || RuntimeVersion::new(self.build.clone()).is_err()
            || !self
                .installation_id
                .starts_with(&format!("{}-", self.family))
            || self.generation == 0
            || !digest(&self.artifact_sha256)
            || !digest(&self.manifest_sha256)
            || !digest(&self.ownership_record_sha256)
            || self.trust == Provenance::UnknownExternalSource
        {
            return Err(SelectionError::InvalidRuntimeIdentity);
        }
        Ok(())
    }
}

/// Conservative binding to a P022-discovered runtime outside P025 ownership.
/// `location` is a canonical selected location, while `identity_sha256` binds
/// P022 structural metadata and observed filesystem identities. It is not an
/// artifact digest or an integrity assessment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalRuntimeReference {
    pub location: String,
    pub family: String,
    pub lineage: String,
    pub lineage_basis: String,
    pub provider: String,
    pub sources: Vec<String>,
    pub build: Option<String>,
    pub architectures: Vec<String>,
    pub structure: String,
    pub ownership: String,
    pub identity_sha256: String,
    pub trust: Provenance,
}
impl ExternalRuntimeReference {
    pub fn validate(&self) -> Result<(), SelectionError> {
        use std::path::{Component, Path};
        let path = Path::new(&self.location);
        let valid_lineage = match (self.family.as_str(), self.lineage.as_str()) {
            ("wine", "upstream-wine" | "wine-staging" | "unknown")
            | ("proton", "valve-proton" | "ge-proton" | "dw-proton" | "unknown") => true,
            ("wine" | "proton", lineage) => lineage
                .strip_prefix("other-verified-derivative:")
                .is_some_and(|id| endfield_unix_runtime::Identifier::new(id).is_ok()),
            _ => false,
        };
        let safe_text = |value: &str, max: usize| {
            !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
        };
        let digest = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        };
        let source_ids = [
            "system-wine",
            "path-wine",
            "native-steam",
            "flatpak-steam",
            "heroic",
            "lutris",
            "explicit-root",
            "unknown-external-manager",
        ];
        if !path.is_absolute()
            || self.location.len() > 4096
            || path.components().any(|part| part == Component::ParentDir)
            || !safe_text(&self.location, 4096)
            || !matches!(self.family.as_str(), "wine" | "proton")
            || !valid_lineage
            || !matches!(
                self.lineage_basis.as_str(),
                "unknown"
                    | "local metadata"
                    | "directory label hint"
                    | "local version metadata"
                    | "conflicting local metadata"
                    | "conflicting directory label"
                    | "conflicting local metadata and directory label"
            )
            || !safe_text(&self.provider, 128)
            || self.provider != "linux-runtime-discovery"
            || ProviderId::new(self.provider.clone()).is_err()
            || self.sources.is_empty()
            || self.sources.len() > 8
            || self
                .sources
                .iter()
                .any(|source| !source_ids.contains(&source.as_str()))
            || self.sources.windows(2).any(|pair| pair[0] >= pair[1])
            || self.build.as_ref().is_some_and(|build| {
                !safe_text(build, 160) || RuntimeVersion::new(build.clone()).is_err()
            })
            || self.architectures.len() > 16
            || self.architectures.iter().any(|architecture| {
                !matches!(
                    architecture.as_str(),
                    "x86" | "x86_64" | "aarch64" | "arm" | "riscv64" | "unknown"
                )
            })
            || self.architectures.windows(2).any(|pair| pair[0] >= pair[1])
            || !matches!(
                self.structure.as_str(),
                "recognized" | "partially recognized"
            )
            || !safe_text(&self.ownership, 128)
            || !valid_external_ownership(&self.ownership)
            || !digest(&self.identity_sha256)
            || self.trust != Provenance::UnknownExternalSource
        {
            return Err(SelectionError::InvalidRuntimeIdentity);
        }
        Ok(())
    }
}

fn valid_external_ownership(ownership: &str) -> bool {
    matches!(
        ownership,
        "external-user-managed" | "distribution-managed" | "unknown"
    ) || ownership
        .strip_prefix("other-launcher-managed:")
        .is_some_and(|provider| ProviderId::new(provider.to_owned()).is_ok())
}
fn explicitly_external(ownership: &str) -> bool {
    ownership != "unknown" && valid_external_ownership(ownership)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "identity",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PinnedRuntimeReference {
    Managed(ManagedRuntimeReference),
    External(ExternalRuntimeReference),
}
impl PinnedRuntimeReference {
    pub fn validate(&self) -> Result<(), SelectionError> {
        match self {
            Self::Managed(reference) => reference.validate(),
            Self::External(reference) => reference.validate(),
        }
    }
    pub fn family(&self) -> &str {
        match self {
            Self::Managed(reference) => &reference.family,
            Self::External(reference) => &reference.family,
        }
    }
    pub fn build(&self) -> Option<&str> {
        match self {
            Self::Managed(reference) => Some(&reference.build),
            Self::External(reference) => reference.build.as_deref(),
        }
    }
    pub fn provenance(&self) -> Provenance {
        match self {
            Self::Managed(reference) => reference.trust,
            Self::External(reference) => reference.trust,
        }
    }
}
impl From<ManagedRuntimeReference> for PinnedRuntimeReference {
    fn from(value: ManagedRuntimeReference) -> Self {
        Self::Managed(value)
    }
}
impl From<ExternalRuntimeReference> for PinnedRuntimeReference {
    fn from(value: ExternalRuntimeReference) -> Self {
        Self::External(value)
    }
}

/// Create a deterministic registry ID bound to exact scope and runtime
/// identity. This unkeyed token is not a privacy or integrity mechanism.
pub fn stable_pin_id(scope: &RuntimePinScope, runtime: &impl Serialize) -> String {
    let encoded = serde_json::to_vec(&(scope, runtime)).expect("validated pin values serialize");
    let digest = Sha256::digest(encoded);
    format!("pin-{:x}", digest)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    IndependentlyAuthenticatedPublisher,
    UserPinnedLimitedTrust,
    UnknownExternalSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimePin {
    pub pin_id: String,
    pub scope: RuntimePinScope,
    pub runtime: PinnedRuntimeReference,
    pub adapter_preference: Option<AdapterPreference>,
    pub compatibility: CompatibilityStatus,
    pub created_at_unix_seconds: u64,
    pub updated_at_unix_seconds: u64,
}
impl RuntimePin {
    pub fn new(
        pin_id: impl Into<String>,
        scope: RuntimePinScope,
        runtime: impl Into<PinnedRuntimeReference>,
        adapter_preference: Option<AdapterPreference>,
        now: u64,
    ) -> Result<Self, SelectionError> {
        let pin_id = pin_id.into();
        if pin_id.is_empty()
            || pin_id.len() > 96
            || !pin_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err(SelectionError::InvalidRuntimeIdentity);
        }
        let runtime = runtime.into();
        runtime.validate()?;
        let adapter_valid = match (adapter_preference, &runtime) {
            (None, _) => true,
            (Some(AdapterPreference::UmuProton), runtime) => runtime.family() == "proton",
            (Some(AdapterPreference::ManagedWine), PinnedRuntimeReference::Managed(runtime)) => {
                runtime.family == "wine"
            }
            (Some(AdapterPreference::ExternalWine), PinnedRuntimeReference::External(runtime)) => {
                runtime.family == "wine" && explicitly_external(&runtime.ownership)
            }
            _ => false,
        };
        if !adapter_valid {
            return Err(SelectionError::InvalidRuntimeIdentity);
        }
        Ok(Self {
            pin_id,
            scope,
            runtime,
            adapter_preference,
            compatibility: CompatibilityStatus::NotVerified,
            created_at_unix_seconds: now,
            updated_at_unix_seconds: now,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterPreference {
    UmuProton,
    ManagedWine,
    ExternalWine,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityStatus {
    NotVerified,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinRegistry {
    pub schema_version: u32,
    pub revision: u64,
    pub pins: Vec<RuntimePin>,
    checksum_sha256: String,
}
#[derive(Serialize)]
struct RegistryPayload<'a> {
    schema_version: u32,
    revision: u64,
    pins: &'a [RuntimePin],
}
#[derive(Serialize)]
struct LegacyRegistryPayloadV1<'a> {
    schema_version: u32,
    revision: u64,
    pins: &'a [LegacyRuntimePinV1],
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyRuntimePinV1 {
    pin_id: String,
    scope: RuntimePinScope,
    runtime: ManagedRuntimeReference,
    adapter_preference: Option<AdapterPreference>,
    compatibility: CompatibilityStatus,
    created_at_unix_seconds: u64,
    updated_at_unix_seconds: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyPinRegistryV1 {
    schema_version: u32,
    revision: u64,
    pins: Vec<LegacyRuntimePinV1>,
    checksum_sha256: String,
}
fn legacy_registry_checksum_v1(revision: u64, pins: &[LegacyRuntimePinV1]) -> String {
    let payload = LegacyRegistryPayloadV1 {
        schema_version: LEGACY_REGISTRY_SCHEMA_VERSION,
        revision,
        pins,
    };
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&payload).expect("legacy payload serializes"))
    )
}
fn registry_checksum(schema_version: u32, revision: u64, pins: &[RuntimePin]) -> String {
    let payload = RegistryPayload {
        schema_version,
        revision,
        pins,
    };
    let bytes = serde_json::to_vec(&payload).expect("registry payload serializes");
    format!("{:x}", Sha256::digest(bytes))
}
impl Default for PinRegistry {
    fn default() -> Self {
        let schema_version = REGISTRY_SCHEMA_VERSION;
        let revision = 0;
        let pins = Vec::new();
        let checksum_sha256 = registry_checksum(schema_version, revision, &pins);
        Self {
            schema_version,
            revision,
            pins,
            checksum_sha256,
        }
    }
}
impl PinRegistry {
    fn validate(&self) -> Result<(), SelectionError> {
        if self.schema_version != REGISTRY_SCHEMA_VERSION || self.pins.len() > MAX_PINS {
            return Err(SelectionError::RegistryCorrupt);
        }
        for pin in &self.pins {
            let valid_machine = match &pin.scope {
                RuntimePinScope::Machine { machine } | RuntimePinScope::Profile { machine, .. } => {
                    MachineId::from_digest(machine.as_str().to_owned()).is_ok()
                }
            };
            let valid_profile = match &pin.scope {
                RuntimePinScope::Profile { profile, .. } => {
                    ProfileKey::new(profile.as_str()).is_ok()
                }
                RuntimePinScope::Machine { .. } => true,
            };
            if !valid_machine
                || !valid_profile
                || pin.pin_id.is_empty()
                || pin.pin_id.len() > 96
                || !pin
                    .pin_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
                || pin.updated_at_unix_seconds < pin.created_at_unix_seconds
                || pin
                    .adapter_preference
                    .is_some_and(|a| match (a, &pin.runtime) {
                        (AdapterPreference::UmuProton, runtime) => runtime.family() != "proton",
                        (
                            AdapterPreference::ManagedWine,
                            PinnedRuntimeReference::Managed(runtime),
                        ) => runtime.family != "wine",
                        (
                            AdapterPreference::ExternalWine,
                            PinnedRuntimeReference::External(runtime),
                        ) => runtime.family != "wine" || !explicitly_external(&runtime.ownership),
                        _ => true,
                    })
            {
                return Err(SelectionError::RegistryCorrupt);
            }
            pin.runtime.validate()?;
        }
        for (i, pin) in self.pins.iter().enumerate() {
            if self.pins[..i]
                .iter()
                .any(|p| p.scope == pin.scope || p.pin_id == pin.pin_id)
            {
                return Err(SelectionError::RegistryCorrupt);
            }
        }
        Ok(())
    }
    pub fn pin(&self, scope: &RuntimePinScope) -> Option<&RuntimePin> {
        self.pins.iter().find(|p| &p.scope == scope)
    }
}

/// Registry path in the app's XDG configuration directory. A separate registry
/// avoids migration or interpretation changes to P006's preferences database.
pub fn registry_path(paths: &endfield_unix_storage::AppPaths) -> PathBuf {
    paths
        .config_file()
        .parent()
        .expect("config path has parent")
        .join("runtime-pins.json")
}

pub struct PinStore {
    path: PathBuf,
}
impl PinStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn for_app(paths: &endfield_unix_storage::AppPaths) -> Self {
        Self::new(registry_path(paths))
    }
    pub fn load(&self) -> Result<PinRegistry, SelectionError> {
        self.with_lock(false, None)
    }
    pub fn set(
        &self,
        pin: RuntimePin,
        expected_revision: u64,
        replace: bool,
    ) -> Result<PinRegistry, SelectionError> {
        pin.runtime.validate()?;
        let registry = self.with_lock(true, Some((expected_revision, Some(pin), replace)))?;
        registry.validate()?;
        Ok(registry)
    }
    pub fn remove(
        &self,
        scope: &RuntimePinScope,
        expected_revision: u64,
    ) -> Result<PinRegistry, SelectionError> {
        let parent = self.path.parent().ok_or(SelectionError::UnsafePath)?;
        fs::create_dir_all(parent)?;
        let parent_metadata = fs::symlink_metadata(parent)?;
        if !parent_metadata.is_dir() || parent_metadata.file_type().is_symlink() {
            return Err(SelectionError::UnsafePath);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
        let lock = self.open_lock()?;
        lock.lock_exclusive()?;
        let mut registry = self.read()?;
        if registry.revision != expected_revision {
            return Err(SelectionError::RevisionConflict);
        }
        if let Some(index) = registry.pins.iter().position(|p| &p.scope == scope) {
            registry.pins.remove(index);
            registry.revision = registry
                .revision
                .checked_add(1)
                .ok_or(SelectionError::RegistryLimit)?;
            self.write(&registry)?;
            Ok(registry)
        } else {
            Err(SelectionError::PinMissing)
        }
    }
    fn with_lock(
        &self,
        write: bool,
        mutation: Option<(u64, Option<RuntimePin>, bool)>,
    ) -> Result<PinRegistry, SelectionError> {
        let parent = self.path.parent().ok_or(SelectionError::UnsafePath)?;
        fs::create_dir_all(parent)?;
        let meta = fs::symlink_metadata(parent)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(SelectionError::UnsafePath);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
        let lock = self.open_lock()?;
        if write {
            lock.lock_exclusive()?;
        } else {
            FileExt::lock_shared(&lock)?;
        }
        let mut registry = self.read()?;
        if let Some((expected, pin, replace)) = mutation {
            if registry.revision != expected {
                return Err(SelectionError::RevisionConflict);
            }
            let pin = pin.ok_or(SelectionError::PinMissing)?;
            match registry.pins.iter().position(|p| p.scope == pin.scope) {
                Some(i) if !replace => {
                    if registry.pins[i].runtime == pin.runtime
                        && registry.pins[i].adapter_preference == pin.adapter_preference
                    {
                        return Ok(registry);
                    }
                    return Err(SelectionError::PinExists);
                }
                Some(i) => {
                    let mut replacement = pin;
                    replacement.created_at_unix_seconds = registry.pins[i].created_at_unix_seconds;
                    registry.pins[i] = replacement;
                }
                None => {
                    if registry.pins.len() == MAX_PINS {
                        return Err(SelectionError::RegistryLimit);
                    }
                    registry.pins.push(pin);
                }
            }
            registry.pins.sort_by(|a, b| a.scope.cmp(&b.scope));
            registry.revision = registry
                .revision
                .checked_add(1)
                .ok_or(SelectionError::RegistryLimit)?;
            registry.validate()?;
            self.write(&registry)?;
        }
        Ok(registry)
    }
    fn open_lock(&self) -> Result<File, SelectionError> {
        let lock_path = self.path.with_extension("lock");
        if let Ok(meta) = fs::symlink_metadata(&lock_path) {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err(SelectionError::UnsafePath);
            }
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let file = options.open(lock_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        Ok(file)
    }
    fn read(&self) -> Result<PinRegistry, SelectionError> {
        match fs::symlink_metadata(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(PinRegistry::default()),
            Err(e) => return Err(e.into()),
            Ok(m) if !m.is_file() || m.file_type().is_symlink() || m.len() > MAX_REGISTRY_BYTES => {
                return Err(SelectionError::RegistryCorrupt);
            }
            Ok(_) => {}
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let mut file = options.open(&self.path)?;
        let opened = file.metadata()?;
        if !opened.is_file() || opened.len() > MAX_REGISTRY_BYTES {
            return Err(SelectionError::RegistryCorrupt);
        }
        let mut bytes = Vec::with_capacity(opened.len() as usize);
        Read::by_ref(&mut file)
            .take(MAX_REGISTRY_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_REGISTRY_BYTES {
            return Err(SelectionError::RegistryLimit);
        }
        let json: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| SelectionError::RegistryCorrupt)?;
        match json
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
        {
            Some(schema_version) if schema_version == REGISTRY_SCHEMA_VERSION as u64 => {
                let registry: PinRegistry =
                    serde_json::from_value(json).map_err(|_| SelectionError::RegistryCorrupt)?;
                registry.validate()?;
                if registry.checksum_sha256
                    != registry_checksum(registry.schema_version, registry.revision, &registry.pins)
                {
                    return Err(SelectionError::RegistryCorrupt);
                }
                Ok(registry)
            }
            Some(schema_version) if schema_version == LEGACY_REGISTRY_SCHEMA_VERSION as u64 => {
                let legacy: LegacyPinRegistryV1 =
                    serde_json::from_value(json).map_err(|_| SelectionError::RegistryCorrupt)?;
                if legacy.schema_version != LEGACY_REGISTRY_SCHEMA_VERSION
                    || legacy.pins.len() > MAX_PINS
                    || legacy.checksum_sha256
                        != legacy_registry_checksum_v1(legacy.revision, &legacy.pins)
                {
                    return Err(SelectionError::RegistryCorrupt);
                }
                let pins = legacy
                    .pins
                    .into_iter()
                    .map(|pin| RuntimePin {
                        pin_id: pin.pin_id,
                        scope: pin.scope,
                        runtime: PinnedRuntimeReference::Managed(pin.runtime),
                        adapter_preference: pin.adapter_preference,
                        compatibility: pin.compatibility,
                        created_at_unix_seconds: pin.created_at_unix_seconds,
                        updated_at_unix_seconds: pin.updated_at_unix_seconds,
                    })
                    .collect();
                let mut registry = PinRegistry {
                    schema_version: REGISTRY_SCHEMA_VERSION,
                    revision: legacy.revision,
                    pins,
                    checksum_sha256: String::new(),
                };
                registry.checksum_sha256 =
                    registry_checksum(registry.schema_version, registry.revision, &registry.pins);
                registry.validate()?;
                Ok(registry)
            }
            _ => Err(SelectionError::RegistryCorrupt),
        }
    }
    fn write(&self, registry: &PinRegistry) -> Result<(), SelectionError> {
        let mut registry = registry.clone();
        registry.checksum_sha256 =
            registry_checksum(registry.schema_version, registry.revision, &registry.pins);
        let bytes = serde_json::to_vec(&registry).map_err(|_| SelectionError::RegistryCorrupt)?;
        if bytes.len() as u64 > MAX_REGISTRY_BYTES {
            return Err(SelectionError::RegistryLimit);
        }
        let parent = self.path.parent().ok_or(SelectionError::UnsafePath)?;
        let seq = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let tmp = parent.join(format!(".runtime-pins-{}-{seq}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        let result = (|| -> Result<(), SelectionError> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&tmp, &self.path)?;
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectionSource {
    ExplicitOverride,
    ProfilePin,
    MachinePin,
    None,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolutionState {
    Selected,
    NoPin,
    Stale(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionReport {
    pub source: SelectionSource,
    pub state: ResolutionState,
    pub pin_id: Option<String>,
    pub runtime: Option<PinnedRuntimeReference>,
    pub registry_revision: u64,
    pub automatic_fallback: bool,
    pub compatibility: CompatibilityStatus,
}

/// Selects in the documented order. A present invalid most-specific pin blocks fallback.
pub fn resolve(
    registry: &PinRegistry,
    machine: &MachineId,
    profile: Option<&ProfileKey>,
    explicit: Option<PinnedRuntimeReference>,
    mut validate: impl FnMut(&PinnedRuntimeReference) -> Result<(), String>,
) -> SelectionReport {
    let candidate = if let Some(runtime) = explicit {
        Some((SelectionSource::ExplicitOverride, None, runtime))
    } else {
        let profile_pin = profile.and_then(|id| {
            registry.pin(&RuntimePinScope::Profile {
                machine: machine.clone(),
                profile: id.clone(),
            })
        });
        profile_pin
            .map(|p| (SelectionSource::ProfilePin, Some(p), p.runtime.clone()))
            .or_else(|| {
                registry
                    .pin(&RuntimePinScope::Machine {
                        machine: machine.clone(),
                    })
                    .map(|p| (SelectionSource::MachinePin, Some(p), p.runtime.clone()))
            })
    };
    let Some((source, pin, runtime)) = candidate else {
        return SelectionReport {
            source: SelectionSource::None,
            state: ResolutionState::NoPin,
            pin_id: None,
            runtime: None,
            registry_revision: registry.revision,
            automatic_fallback: false,
            compatibility: CompatibilityStatus::NotVerified,
        };
    };
    let state = match validate(&runtime) {
        Ok(()) => ResolutionState::Selected,
        Err(reason) => ResolutionState::Stale(reason),
    };
    SelectionReport {
        source,
        state,
        pin_id: pin.map(|p| p.pin_id.clone()),
        runtime: Some(runtime),
        registry_revision: registry.revision,
        automatic_fallback: false,
        compatibility: CompatibilityStatus::NotVerified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn runtime(id: &str) -> ManagedRuntimeReference {
        ManagedRuntimeReference {
            installation_id: id.into(),
            family: "proton".into(),
            lineage: "ge-proton".into(),
            provider: "test".into(),
            project: "test".into(),
            build: id.into(),
            artifact_sha256: "a".repeat(64),
            manifest_sha256: "b".repeat(64),
            generation: 1,
            ownership_record_sha256: "c".repeat(64),
            trust: Provenance::UserPinnedLimitedTrust,
        }
    }
    fn external(location: &str) -> ExternalRuntimeReference {
        ExternalRuntimeReference {
            location: location.into(),
            family: "proton".into(),
            lineage: "ge-proton".into(),
            lineage_basis: "directory label hint".into(),
            provider: "linux-runtime-discovery".into(),
            sources: vec!["native-steam".into()],
            build: Some("GE-Proton-10-26".into()),
            architectures: vec!["x86_64".into()],
            structure: "recognized".into(),
            ownership: "external-user-managed".into(),
            identity_sha256: "d".repeat(64),
            trust: Provenance::UnknownExternalSource,
        }
    }
    fn machine() -> MachineId {
        machine_id_from_bytes(b"00112233445566778899aabbccddeeff\n").unwrap()
    }
    #[test]
    fn machine_id_is_domain_separated_and_validated() {
        assert_eq!(
            machine(),
            machine_id_from_bytes(b"00112233445566778899AABBCCDDEEFF").unwrap()
        );
        assert!(machine_id_from_bytes(b"bad").is_err());
    }
    #[test]
    fn profile_scope_is_machine_bound() {
        assert_ne!(
            RuntimePinScope::Profile {
                machine: machine(),
                profile: ProfileKey::new("game-one").unwrap()
            },
            RuntimePinScope::Profile {
                machine: machine_id_from_bytes(b"ffeeddccbbaa99887766554433221100").unwrap(),
                profile: ProfileKey::new("game-one").unwrap()
            }
        );
        assert!(ProfileKey::new("../x").is_err());
        let runtime = runtime("proton-a");
        assert_ne!(
            stable_pin_id(&RuntimePinScope::Machine { machine: machine() }, &runtime),
            stable_pin_id(
                &RuntimePinScope::Profile {
                    machine: machine(),
                    profile: ProfileKey::new("game-one").unwrap()
                },
                &runtime
            )
        );
    }
    #[test]
    fn pin_contract_keeps_provenance_and_rejects_invalid_runtime_or_adapter() {
        let scope = RuntimePinScope::Machine { machine: machine() };
        let pin = RuntimePin::new(
            "selected",
            scope.clone(),
            runtime("proton-a"),
            Some(AdapterPreference::UmuProton),
            10,
        )
        .unwrap();
        assert_eq!(pin.runtime.provenance(), Provenance::UserPinnedLimitedTrust);
        assert_eq!(pin.compatibility, CompatibilityStatus::NotVerified);
        let encoded = serde_json::to_vec(&pin).unwrap();
        let decoded: RuntimePin = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, pin);
        let mut invalid_family = runtime("proton-a");
        invalid_family.family = "unknown".into();
        assert!(matches!(
            RuntimePin::new("bad", scope.clone(), invalid_family, None, 1),
            Err(SelectionError::InvalidRuntimeIdentity)
        ));
        let mut invalid_digest = runtime("proton-a");
        invalid_digest.artifact_sha256 = "not-a-digest".into();
        assert!(invalid_digest.validate().is_err());
        assert!(
            RuntimePin::new(
                "bad",
                scope,
                runtime("proton-a"),
                Some(AdapterPreference::ManagedWine),
                1
            )
            .is_err()
        );
    }
    #[test]
    fn external_pin_keeps_unknown_provenance_and_validates_exact_reference() {
        let external_ref = external("/home/user/.steam/steam/compatibilitytools.d/GE-Proton");
        let pin = RuntimePin::new(
            "external",
            RuntimePinScope::Machine { machine: machine() },
            external_ref.clone(),
            Some(AdapterPreference::UmuProton),
            1,
        )
        .unwrap();
        assert_eq!(pin.runtime.provenance(), Provenance::UnknownExternalSource);
        assert!(external_ref.validate().is_ok());
        let mut invalid = external_ref;
        invalid.location = "/tmp/../replacement".into();
        assert!(invalid.validate().is_err());
        let mut unknown_wine = external("/opt/wine-custom");
        unknown_wine.family = "wine".into();
        unknown_wine.lineage = "unknown".into();
        unknown_wine.ownership = "unknown".into();
        assert!(
            RuntimePin::new(
                "external-wine",
                RuntimePinScope::Machine { machine: machine() },
                unknown_wine,
                Some(AdapterPreference::ExternalWine),
                1,
            )
            .is_err()
        );
    }
    #[test]
    fn precedence_and_stale_profile_block_fallback() {
        let m = machine();
        let profile = ProfileKey::new("fixture").unwrap();
        let mut r = PinRegistry::default();
        r.pins.push(
            RuntimePin::new(
                "machine",
                RuntimePinScope::Machine { machine: m.clone() },
                runtime("proton-a"),
                None,
                1,
            )
            .unwrap(),
        );
        r.pins.push(
            RuntimePin::new(
                "profile",
                RuntimePinScope::Profile {
                    machine: m.clone(),
                    profile: profile.clone(),
                },
                runtime("proton-b"),
                None,
                1,
            )
            .unwrap(),
        );
        let resolved = resolve(&r, &m, Some(&profile), None, |_| Err("missing".into()));
        assert_eq!(resolved.source, SelectionSource::ProfilePin);
        assert_eq!(resolved.state, ResolutionState::Stale("missing".into()));
        assert!(!resolved.automatic_fallback);
        let resolved = resolve(&r, &m, None, None, |_| Ok(()));
        assert!(matches!(
            resolved.runtime.unwrap(),
            PinnedRuntimeReference::Managed(runtime) if runtime.installation_id == "proton-a"
        ));
    }
    #[test]
    fn stale_machine_pin_reports_no_automatic_version_switch() {
        let m = machine();
        let mut registry = PinRegistry::default();
        registry.pins.push(
            RuntimePin::new(
                "p",
                RuntimePinScope::Machine { machine: m.clone() },
                runtime("proton-a"),
                None,
                1,
            )
            .unwrap(),
        );
        let report = resolve(&registry, &m, None, None, |identity| {
            assert!(
                matches!(identity, PinnedRuntimeReference::Managed(runtime) if runtime.installation_id == "proton-a")
            );
            Err("selected installation missing".into())
        });
        assert_eq!(
            report.state,
            ResolutionState::Stale("selected installation missing".into())
        );
        assert!(!report.automatic_fallback);
        assert!(matches!(
            &registry.pins[0].runtime,
            PinnedRuntimeReference::Managed(runtime) if runtime.installation_id == "proton-a"
        ));
    }
    #[test]
    fn persistence_revision_replace_remove_and_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.json");
        let store = PinStore::new(&path);
        let scope = RuntimePinScope::Machine { machine: machine() };
        let first = store
            .set(
                RuntimePin::new("p1", scope.clone(), runtime("proton-a"), None, 1).unwrap(),
                0,
                false,
            )
            .unwrap();
        assert_eq!(first.revision, 1);
        assert_eq!(
            store
                .set(
                    RuntimePin::new("p1", scope.clone(), runtime("proton-a"), None, 2).unwrap(),
                    1,
                    false
                )
                .unwrap()
                .revision,
            1
        );
        assert!(matches!(
            store.set(
                RuntimePin::new("p2", scope.clone(), runtime("proton-b"), None, 2).unwrap(),
                1,
                false
            ),
            Err(SelectionError::PinExists)
        ));
        let changed = store
            .set(
                RuntimePin::new("p2", scope.clone(), runtime("proton-b"), None, 1).unwrap(),
                1,
                true,
            )
            .unwrap();
        assert_eq!(changed.revision, 2);
        assert_eq!(store.remove(&scope, 2).unwrap().pins.len(), 0);
        fs::write(&path, b"{").unwrap();
        assert!(matches!(store.load(), Err(SelectionError::RegistryCorrupt)));
    }
    #[test]
    fn concurrent_writers_detect_revision_conflict_and_symlinks_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.json");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut writers = Vec::new();
        for name in ["one", "two"] {
            let path = path.clone();
            let barrier = barrier.clone();
            writers.push(std::thread::spawn(move || {
                let store = PinStore::new(path);
                let pin = RuntimePin::new(
                    name,
                    RuntimePinScope::Machine { machine: machine() },
                    runtime(&format!("proton-{name}")),
                    None,
                    1,
                )
                .unwrap();
                barrier.wait();
                store.set(pin, 0, false)
            }));
        }
        barrier.wait();
        let results: Vec<_> = writers
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(
                    result,
                    Err(SelectionError::RevisionConflict | SelectionError::PinExists)
                ))
                .count(),
            1
        );

        let target = dir.path().join("protected");
        fs::write(&target, b"do not replace").unwrap();
        fs::remove_file(&path).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(matches!(
            PinStore::new(&path).load(),
            Err(SelectionError::RegistryCorrupt)
        ));
        assert_eq!(fs::read(&target).unwrap(), b"do not replace");
    }

    #[test]
    fn unknown_fields_and_duplicate_scopes_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.json");
        fs::write(
            &path,
            br#"{"schema_version":1,"revision":0,"pins":[],"extra":true}"#,
        )
        .unwrap();
        assert!(matches!(
            PinStore::new(&path).load(),
            Err(SelectionError::RegistryCorrupt)
        ));
        let pin = RuntimePin::new(
            "duplicate",
            RuntimePinScope::Machine { machine: machine() },
            runtime("proton-a"),
            None,
            1,
        )
        .unwrap();
        let mut registry = PinRegistry {
            schema_version: 1,
            revision: 2,
            pins: vec![pin.clone(), pin],
            checksum_sha256: String::new(),
        };
        registry.checksum_sha256 =
            registry_checksum(registry.schema_version, registry.revision, &registry.pins);
        fs::write(&path, serde_json::to_vec(&registry).unwrap()).unwrap();
        assert!(matches!(
            PinStore::new(&path).load(),
            Err(SelectionError::RegistryCorrupt)
        ));
    }
    #[test]
    fn checksum_detects_valid_json_edits_and_future_schema_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.json");
        let store = PinStore::new(&path);
        store
            .set(
                RuntimePin::new(
                    "pin",
                    RuntimePinScope::Machine { machine: machine() },
                    runtime("proton-a"),
                    None,
                    1,
                )
                .unwrap(),
                0,
                false,
            )
            .unwrap();
        let mut edited: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        edited["revision"] = serde_json::json!(15);
        fs::write(&path, serde_json::to_vec(&edited).unwrap()).unwrap();
        assert!(matches!(store.load(), Err(SelectionError::RegistryCorrupt)));

        let mut future = serde_json::to_value(PinRegistry::default()).unwrap();
        future["schema_version"] = serde_json::json!(99);
        fs::write(&path, serde_json::to_vec(&future).unwrap()).unwrap();
        assert!(matches!(store.load(), Err(SelectionError::RegistryCorrupt)));
    }
    #[test]
    fn explicit_override_does_not_require_or_mutate_pin() {
        let r = PinRegistry::default();
        let selected = resolve(
            &r,
            &machine(),
            None,
            Some(PinnedRuntimeReference::Managed(runtime("proton-x"))),
            |_| Ok(()),
        );
        assert_eq!(selected.source, SelectionSource::ExplicitOverride);
        assert_eq!(r.revision, 0);
    }

    #[test]
    fn legacy_managed_registry_migrates_on_read_without_mutating_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.json");
        let scope = RuntimePinScope::Machine { machine: machine() };
        let pin = LegacyRuntimePinV1 {
            pin_id: "legacy-pin".into(),
            scope,
            runtime: runtime("proton-old"),
            adapter_preference: Some(AdapterPreference::UmuProton),
            compatibility: CompatibilityStatus::NotVerified,
            created_at_unix_seconds: 10,
            updated_at_unix_seconds: 10,
        };
        let legacy = LegacyPinRegistryV1 {
            schema_version: LEGACY_REGISTRY_SCHEMA_VERSION,
            revision: 7,
            checksum_sha256: legacy_registry_checksum_v1(7, std::slice::from_ref(&pin)),
            pins: vec![pin],
        };
        let original = serde_json::to_vec(&legacy).unwrap();
        fs::write(&path, &original).unwrap();
        let migrated = PinStore::new(&path).load().unwrap();
        assert_eq!(migrated.schema_version, REGISTRY_SCHEMA_VERSION);
        assert_eq!(migrated.revision, 7);
        assert!(matches!(
            &migrated.pins[0].runtime,
            PinnedRuntimeReference::Managed(reference) if reference.installation_id == "proton-old"
        ));
        assert_eq!(fs::read(path).unwrap(), original);
    }
}
