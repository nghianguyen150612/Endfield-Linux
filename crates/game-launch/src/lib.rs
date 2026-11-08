//! Portable launch planning and approval contracts.
//! This crate performs no filesystem access and starts no processes.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const PLAN_SCHEMA_VERSION: u32 = 1;
const MAX_ID: usize = 96;
const MAX_PATH: usize = 4096;
const MAX_TEXT: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAdapter {
    ManagedWine,
    ExternalWine,
    UmuProton,
    DirectProton,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    Verified,
    Unknown,
    Missing,
    Stale,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameLaunchRequest {
    pub installation_id: String,
    pub prefix_id: String,
    pub profile: Option<String>,
    pub executable_relative: String,
    pub adapter: RuntimeAdapter,
}

impl GameLaunchRequest {
    pub fn new(
        installation_id: impl Into<String>,
        prefix_id: impl Into<String>,
        profile: Option<String>,
        executable_relative: impl Into<String>,
        adapter: RuntimeAdapter,
    ) -> Result<Self, LaunchError> {
        let value = Self {
            installation_id: installation_id.into(),
            prefix_id: prefix_id.into(),
            profile,
            executable_relative: executable_relative.into(),
            adapter,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), LaunchError> {
        validate_id(&self.installation_id, "installation_id")?;
        validate_id(&self.prefix_id, "prefix_id")?;
        if let Some(p) = &self.profile {
            validate_profile(p)?;
        }
        validate_relative_executable(&self.executable_relative)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSelectionEvidence {
    pub state: EvidenceState,
    pub selection_source: Option<String>,
    pub runtime_id: Option<String>,
    pub family: Option<String>,
    pub adapter: RuntimeAdapter,
    pub pin_id: Option<String>,
    pub pin_revision: Option<u64>,
    pub managed_ownership_verified: bool,
    pub baseline_integrity_verified: bool,
    pub external_structure_revalidated: bool,
    pub external_trust_policy_documented: bool,
    pub invocation_contract_verified: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefixLifecycle {
    Planned,
    Created,
    Initialized,
    Incomplete,
    RecoveryRequired,
    Missing,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefixEvidence {
    pub state: EvidenceState,
    pub prefix_id: Option<String>,
    pub lifecycle: PrefixLifecycle,
    pub managed_ownership_verified: bool,
    pub directory_identity_verified: bool,
    pub runtime_association_matches: bool,
    pub registry_revision: Option<u64>,
    pub initialization_verified: bool,
    pub recovery_pending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutableFormat {
    PeCoff,
    Other,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameExecutableEvidence {
    pub relative_path: String,
    pub exists: bool,
    pub regular_file: bool,
    pub symlink_free: bool,
    pub within_registered_root: bool,
    pub root_identity_matches: bool,
    pub file_device: Option<u64>,
    pub file_inode: Option<u64>,
    pub file_size: Option<u64>,
    pub format: ExecutableFormat,
    pub publisher_authenticity_verified: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityEvidence {
    Unknown,
    Unsupported,
    EvidenceObserved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchConfiguration {
    pub game_registered: bool,
    pub game_registry_revision: Option<u64>,
    pub game_root_identity_matches: bool,
    pub executable: GameExecutableEvidence,
    pub runtime: RuntimeSelectionEvidence,
    pub prefix: PrefixEvidence,
    pub compatibility: CompatibilityEvidence,
    pub critical_dependencies: EvidenceState,
    pub policy: LaunchPolicy,
}

/// Explicit policy evidence for whether a production executor may create a
/// process. This contract is portable; the crate does not implement execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchPolicy {
    pub state: EvidenceState,
    pub process_creation_allowed: bool,
    pub approval_required: bool,
}

impl LaunchPolicy {
    pub const fn fail_closed() -> Self {
        Self {
            state: EvidenceState::Unknown,
            process_creation_allowed: false,
            approval_required: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchState {
    Prepared,
    Blocked,
    RequiresApproval,
    Approved,
    Starting,
    Started,
    Exited,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedProcessBehavior {
    NoProcessCreated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessOwnership {
    None,
}

impl LaunchState {
    pub fn transition(self, next: Self) -> Result<Self, LaunchError> {
        let allowed = matches!(
            (self, next),
            (
                Self::Prepared,
                Self::Blocked | Self::RequiresApproval | Self::Failed | Self::Cancelled
            ) | (
                Self::RequiresApproval,
                Self::Approved | Self::Failed | Self::Cancelled
            ) | (
                Self::Approved,
                Self::Starting | Self::Failed | Self::Cancelled
            ) | (
                Self::Starting,
                Self::Started | Self::Failed | Self::Cancelled
            ) | (Self::Started, Self::Exited | Self::Failed | Self::Cancelled)
        );
        if allowed {
            Ok(next)
        } else {
            Err(LaunchError::InvalidTransition)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchBlocker {
    GameNotRegistered,
    GameRootChanged,
    ExecutableMissing,
    ExecutableNotRegular,
    ExecutableSymlink,
    ExecutableOutsideRoot,
    ExecutableIdentityUnknown,
    ExecutableFormatUnsupported,
    RuntimePinMissing,
    RuntimeStale,
    RuntimeUnknown,
    RuntimeIntegrityUnverified,
    ExternalTrustPolicyMissing,
    AdapterMismatch,
    AdapterUnsupported,
    PrefixMissing,
    PrefixUninitialized,
    PrefixStale,
    PrefixRecoveryPending,
    PrefixInitializationUnsupported,
    CriticalDependencyFinding,
    CriticalDependencyReadinessUnknown,
    CompatibilityUnsupported,
    ExecutionPolicyUnknown,
}
impl LaunchBlocker {
    pub const fn code(self) -> &'static str {
        match self {
            Self::GameNotRegistered => "game_not_registered",
            Self::GameRootChanged => "game_root_changed",
            Self::ExecutableMissing => "executable_missing",
            Self::ExecutableNotRegular => "executable_not_regular",
            Self::ExecutableSymlink => "executable_symlink",
            Self::ExecutableOutsideRoot => "executable_outside_root",
            Self::ExecutableIdentityUnknown => "executable_identity_unknown",
            Self::ExecutableFormatUnsupported => "executable_format_unsupported",
            Self::RuntimePinMissing => "runtime_pin_missing",
            Self::RuntimeStale => "runtime_stale",
            Self::RuntimeUnknown => "runtime_unknown",
            Self::RuntimeIntegrityUnverified => "runtime_integrity_unverified",
            Self::ExternalTrustPolicyMissing => "external_trust_policy_missing",
            Self::AdapterMismatch => "adapter_mismatch",
            Self::AdapterUnsupported => "adapter_unsupported",
            Self::PrefixMissing => "prefix_missing",
            Self::PrefixUninitialized => "prefix_uninitialized",
            Self::PrefixStale => "prefix_stale",
            Self::PrefixRecoveryPending => "prefix_recovery_pending",
            Self::PrefixInitializationUnsupported => "prefix_initialization_unsupported",
            Self::CriticalDependencyFinding => "critical_dependency_finding",
            Self::CriticalDependencyReadinessUnknown => "critical_dependency_readiness_unknown",
            Self::CompatibilityUnsupported => "compatibility_unsupported",
            Self::ExecutionPolicyUnknown => "execution_policy_unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchWarning {
    PublisherAuthenticityUnknown,
    CompatibilityUnknown,
    AntiCheatCompatibilityUnknown,
    RuntimeNetworkEffectsUnknown,
    ProcessSupervisionNotPersistent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchPlan {
    pub schema_version: u32,
    pub plan_id: String,
    pub installation_id: String,
    pub prefix_id: String,
    pub profile: Option<String>,
    pub executable_relative: String,
    pub game_registry_revision: Option<u64>,
    pub runtime: RuntimeSelectionEvidence,
    pub prefix: PrefixEvidence,
    pub executable: GameExecutableEvidence,
    pub policy: LaunchPolicy,
    pub blockers: Vec<LaunchBlocker>,
    pub warnings: Vec<LaunchWarning>,
    pub state: LaunchState,
    pub approval_required: bool,
    pub process_execution_supported: bool,
    pub expected_process_behavior: ExpectedProcessBehavior,
    pub process_ownership: ProcessOwnership,
    pub side_effects: Vec<String>,
}

#[derive(Serialize)]
struct PlanIdentity<'a> {
    schema_version: u32,
    installation_id: &'a str,
    prefix_id: &'a str,
    profile: &'a Option<String>,
    executable_relative: &'a str,
    game_registry_revision: Option<u64>,
    runtime: &'a RuntimeSelectionEvidence,
    prefix: &'a PrefixEvidence,
    executable: &'a GameExecutableEvidence,
    policy: &'a LaunchPolicy,
    blockers: &'a [LaunchBlocker],
    warnings: &'a [LaunchWarning],
    state: LaunchState,
    approval_required: bool,
    process_execution_supported: bool,
    expected_process_behavior: ExpectedProcessBehavior,
    process_ownership: ProcessOwnership,
    side_effects: &'a [String],
}

impl LaunchPlan {
    pub fn build(
        request: &GameLaunchRequest,
        config: &LaunchConfiguration,
    ) -> Result<Self, LaunchError> {
        request.validate()?;
        validate_runtime_evidence(&config.runtime)?;
        validate_prefix_evidence(&config.prefix)?;
        if !config.policy.approval_required {
            return Err(LaunchError::InvalidValue("approval_required"));
        }
        let mut blockers = Vec::new();
        let mut warnings = Vec::new();
        if !config.game_registered {
            blockers.push(LaunchBlocker::GameNotRegistered);
        }
        if !config.game_root_identity_matches {
            blockers.push(LaunchBlocker::GameRootChanged);
        }
        let exe = &config.executable;
        validate_relative_executable(&exe.relative_path)?;
        if exe.relative_path != request.executable_relative {
            return Err(LaunchError::InvalidValue("executable_evidence_path"));
        }
        if exe.relative_path != request.executable_relative {
            blockers.push(LaunchBlocker::ExecutableIdentityUnknown);
        }
        if !exe.exists {
            blockers.push(LaunchBlocker::ExecutableMissing);
        }
        if exe.exists && !exe.regular_file {
            blockers.push(LaunchBlocker::ExecutableNotRegular);
        }
        if !exe.symlink_free {
            blockers.push(LaunchBlocker::ExecutableSymlink);
        }
        if !exe.within_registered_root {
            blockers.push(LaunchBlocker::ExecutableOutsideRoot);
        }
        if !exe.root_identity_matches {
            blockers.push(LaunchBlocker::GameRootChanged);
        }
        if exe.file_device.is_none() || exe.file_inode.is_none() {
            blockers.push(LaunchBlocker::ExecutableIdentityUnknown);
        }
        if exe.format != ExecutableFormat::PeCoff {
            blockers.push(LaunchBlocker::ExecutableFormatUnsupported);
        }
        if !exe.publisher_authenticity_verified {
            warnings.push(LaunchWarning::PublisherAuthenticityUnknown);
        }

        let runtime = &config.runtime;
        if runtime.adapter != request.adapter {
            blockers.push(LaunchBlocker::AdapterMismatch);
        }
        match runtime.state {
            EvidenceState::Missing => blockers.push(LaunchBlocker::RuntimePinMissing),
            EvidenceState::Stale => blockers.push(LaunchBlocker::RuntimeStale),
            EvidenceState::Unknown => blockers.push(LaunchBlocker::RuntimeUnknown),
            EvidenceState::Unsupported => blockers.push(LaunchBlocker::AdapterUnsupported),
            EvidenceState::Verified => {}
        }
        if runtime.pin_id.is_none() || runtime.pin_revision.is_none() {
            blockers.push(LaunchBlocker::RuntimePinMissing);
        }
        if runtime.state != EvidenceState::Missing
            && (runtime.runtime_id.is_none() || runtime.family.is_none())
        {
            blockers.push(LaunchBlocker::RuntimeUnknown);
        }
        if runtime.adapter == RuntimeAdapter::ManagedWine
            && (!runtime.managed_ownership_verified || !runtime.baseline_integrity_verified)
        {
            blockers.push(LaunchBlocker::RuntimeIntegrityUnverified);
        }
        if runtime.adapter == RuntimeAdapter::ExternalWine
            && (!runtime.external_structure_revalidated
                || !runtime.external_trust_policy_documented)
        {
            blockers.push(LaunchBlocker::ExternalTrustPolicyMissing);
        }
        if !runtime.invocation_contract_verified {
            blockers.push(LaunchBlocker::AdapterUnsupported);
        }

        let prefix = &config.prefix;
        match prefix.state {
            EvidenceState::Missing => blockers.push(LaunchBlocker::PrefixMissing),
            EvidenceState::Stale | EvidenceState::Unknown | EvidenceState::Unsupported => {
                blockers.push(LaunchBlocker::PrefixStale)
            }
            EvidenceState::Verified => {}
        }
        if prefix.prefix_id.as_deref() != Some(request.prefix_id.as_str())
            || !prefix.managed_ownership_verified
            || !prefix.directory_identity_verified
            || !prefix.runtime_association_matches
        {
            blockers.push(LaunchBlocker::PrefixStale);
        }
        if prefix.lifecycle == PrefixLifecycle::Created || !prefix.initialization_verified {
            blockers.push(LaunchBlocker::PrefixUninitialized);
            blockers.push(LaunchBlocker::PrefixInitializationUnsupported);
        }
        if prefix.lifecycle != PrefixLifecycle::Initialized {
            blockers.push(LaunchBlocker::PrefixUninitialized);
        }
        if prefix.recovery_pending || prefix.lifecycle == PrefixLifecycle::RecoveryRequired {
            blockers.push(LaunchBlocker::PrefixRecoveryPending);
        }
        match config.compatibility {
            CompatibilityEvidence::Unsupported => {
                blockers.push(LaunchBlocker::CompatibilityUnsupported)
            }
            CompatibilityEvidence::Unknown => {
                warnings.push(LaunchWarning::CompatibilityUnknown);
                warnings.push(LaunchWarning::AntiCheatCompatibilityUnknown);
            }
            CompatibilityEvidence::EvidenceObserved => {}
        }
        match config.critical_dependencies {
            EvidenceState::Verified => {}
            EvidenceState::Unsupported => blockers.push(LaunchBlocker::CriticalDependencyFinding),
            EvidenceState::Unknown | EvidenceState::Missing | EvidenceState::Stale => {
                blockers.push(LaunchBlocker::CriticalDependencyReadinessUnknown)
            }
        }
        if config.policy.state != EvidenceState::Verified || !config.policy.process_creation_allowed
        {
            blockers.push(LaunchBlocker::ExecutionPolicyUnknown);
        }
        blockers.sort_by_key(|b| b.code());
        blockers.dedup();
        warnings.sort_by_key(|w| *w as u8);
        warnings.dedup();

        // The portable planner never authorizes process creation by itself.
        let process_execution_supported = false;
        let state = if blockers.is_empty() {
            LaunchState::RequiresApproval
        } else {
            LaunchState::Blocked
        };
        let side_effects = vec![
            "No writes are performed while planning.".into(),
            "Runtime execution may modify prefix files and runtime caches.".into(),
            "Network access by the selected runtime is not fully controlled.".into(),
        ];
        let identity = PlanIdentity {
            schema_version: PLAN_SCHEMA_VERSION,
            installation_id: &request.installation_id,
            prefix_id: &request.prefix_id,
            profile: &request.profile,
            executable_relative: &request.executable_relative,
            game_registry_revision: config.game_registry_revision,
            runtime,
            prefix,
            executable: exe,
            policy: &config.policy,
            blockers: &blockers,
            warnings: &warnings,
            state,
            approval_required: config.policy.approval_required,
            process_execution_supported,
            expected_process_behavior: ExpectedProcessBehavior::NoProcessCreated,
            process_ownership: ProcessOwnership::None,
            side_effects: &side_effects,
        };
        let plan_id = identity_id(&identity)?;
        Ok(Self {
            schema_version: PLAN_SCHEMA_VERSION,
            plan_id,
            installation_id: request.installation_id.clone(),
            prefix_id: request.prefix_id.clone(),
            profile: request.profile.clone(),
            executable_relative: request.executable_relative.clone(),
            game_registry_revision: config.game_registry_revision,
            runtime: runtime.clone(),
            prefix: prefix.clone(),
            executable: exe.clone(),
            policy: config.policy.clone(),
            blockers,
            warnings,
            state,
            approval_required: config.policy.approval_required,
            process_execution_supported,
            expected_process_behavior: ExpectedProcessBehavior::NoProcessCreated,
            process_ownership: ProcessOwnership::None,
            side_effects,
        })
    }

    pub fn is_blocked(&self) -> bool {
        self.state == LaunchState::Blocked || !self.blockers.is_empty()
    }

    pub fn validate_integrity(&self) -> Result<(), LaunchError> {
        if self.schema_version != PLAN_SCHEMA_VERSION {
            return Err(LaunchError::PlanIntegrityMismatch);
        }
        validate_id(&self.installation_id, "installation_id")?;
        validate_id(&self.prefix_id, "prefix_id")?;
        if let Some(profile) = &self.profile {
            validate_profile(profile)?;
        }
        validate_relative_executable(&self.executable_relative)?;
        validate_runtime_evidence(&self.runtime)?;
        validate_prefix_evidence(&self.prefix)?;
        validate_relative_executable(&self.executable.relative_path)?;
        if self.executable.relative_path != self.executable_relative
            || !self.approval_required
            || self.policy.approval_required != self.approval_required
            || self.process_execution_supported
            || self.expected_process_behavior != ExpectedProcessBehavior::NoProcessCreated
            || self.process_ownership != ProcessOwnership::None
            || self.blockers.len() > 64
            || self.warnings.len() > 16
            || self.side_effects.len() > 16
            || self.side_effects.iter().any(|text| text.len() > MAX_TEXT)
        {
            return Err(LaunchError::PlanIntegrityMismatch);
        }
        let identity = PlanIdentity {
            schema_version: self.schema_version,
            installation_id: &self.installation_id,
            prefix_id: &self.prefix_id,
            profile: &self.profile,
            executable_relative: &self.executable_relative,
            game_registry_revision: self.game_registry_revision,
            runtime: &self.runtime,
            prefix: &self.prefix,
            executable: &self.executable,
            policy: &self.policy,
            blockers: &self.blockers,
            warnings: &self.warnings,
            state: self.state,
            approval_required: self.approval_required,
            process_execution_supported: self.process_execution_supported,
            expected_process_behavior: self.expected_process_behavior,
            process_ownership: self.process_ownership,
            side_effects: &self.side_effects,
        };
        if identity_id(&identity)? != self.plan_id {
            return Err(LaunchError::PlanIntegrityMismatch);
        }
        Ok(())
    }

    /// Verifies approval and the independently established production process boundary.
    pub fn validate_execution_approval(
        &self,
        approval: &LaunchApproval,
    ) -> Result<(), LaunchError> {
        approval.validate(self)?;
        if !self.process_execution_supported {
            return Err(LaunchError::ExecutionUnsupported);
        }
        Ok(())
    }
    /// Plan serialization exposes only the installation-relative executable path.
    pub fn redacted_json(&self) -> Result<String, LaunchError> {
        self.validate_integrity()?;
        serde_json::to_string_pretty(self).map_err(|_| LaunchError::Serialization)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchApproval {
    pub plan_id: String,
    pub approved: bool,
}
impl LaunchApproval {
    pub fn approve(plan: &LaunchPlan) -> Result<Self, LaunchError> {
        plan.validate_integrity()?;
        if plan.is_blocked()
            || plan.state != LaunchState::RequiresApproval
            || !plan.approval_required
        {
            return Err(LaunchError::Blocked);
        }
        Ok(Self {
            plan_id: plan.plan_id.clone(),
            approved: true,
        })
    }
    pub fn validate(&self, plan: &LaunchPlan) -> Result<(), LaunchError> {
        plan.validate_integrity()?;
        if !self.approved || self.plan_id != plan.plan_id {
            return Err(LaunchError::ApprovalMismatch);
        }
        if plan.is_blocked() {
            return Err(LaunchError::Blocked);
        }
        if plan.state != LaunchState::RequiresApproval || !plan.approval_required {
            return Err(LaunchError::Blocked);
        }
        Ok(())
    }
}

/// Injection point for platform process adapters. Implementations must call
/// LaunchPlan::validate_execution_approval and revalidate identities before spawn.
pub trait ProcessExecutor {
    type Error;
    fn execute(
        &self,
        plan: &LaunchPlan,
        approval: &LaunchApproval,
    ) -> Result<LaunchResult, Self::Error>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchResult {
    pub plan_id: String,
    pub state: LaunchState,
    pub error_code: Option<String>,
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum LaunchError {
    #[error("invalid {0}")]
    InvalidValue(&'static str),
    #[error("selected executable path must be relative and contain no traversal")]
    UnsafeExecutablePath,
    #[error("launch plan is blocked")]
    Blocked,
    #[error("launch approval does not match the exact plan")]
    ApprovalMismatch,
    #[error("production process execution is not implemented and verified")]
    ExecutionUnsupported,
    #[error("launch plan fields do not match their plan ID")]
    PlanIntegrityMismatch,
    #[error("launch state transition is invalid")]
    InvalidTransition,
    #[error("launch plan serialization failed")]
    Serialization,
}
impl LaunchError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidValue(_) => "invalid_value",
            Self::UnsafeExecutablePath => "unsafe_executable_path",
            Self::Blocked => "launch_blocked",
            Self::ApprovalMismatch => "approval_mismatch",
            Self::ExecutionUnsupported => "execution_unsupported",
            Self::PlanIntegrityMismatch => "plan_integrity_mismatch",
            Self::InvalidTransition => "invalid_transition",
            Self::Serialization => "serialization_error",
        }
    }
}

pub fn validate_relative_executable(path: &str) -> Result<(), LaunchError> {
    if path.is_empty()
        || path.len() > MAX_PATH
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(LaunchError::UnsafeExecutablePath);
    }
    Ok(())
}
fn validate_id(value: &str, field: &'static str) -> Result<(), LaunchError> {
    if value.is_empty()
        || value.len() > MAX_ID
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err(LaunchError::InvalidValue(field));
    }
    Ok(())
}

fn validate_profile(value: &str) -> Result<(), LaunchError> {
    if value.is_empty()
        || value.len() > MAX_ID
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_ .".contains(&byte))
        || value.starts_with('.')
        || value.contains("..")
    {
        return Err(LaunchError::InvalidValue("profile"));
    }
    Ok(())
}

fn validate_runtime_evidence(runtime: &RuntimeSelectionEvidence) -> Result<(), LaunchError> {
    for (value, field, limit) in [
        (runtime.selection_source.as_deref(), "selection_source", 64),
        (runtime.runtime_id.as_deref(), "runtime_id", 256),
        (runtime.family.as_deref(), "runtime_family", 32),
        (runtime.pin_id.as_deref(), "pin_id", MAX_ID),
    ] {
        if value.is_some_and(|text| {
            text.is_empty() || text.len() > limit || text.chars().any(char::is_control)
        }) {
            return Err(LaunchError::InvalidValue(field));
        }
    }
    if runtime.selection_source.as_deref().is_some_and(|source| {
        !matches!(
            source,
            "profile_pin" | "machine_pin" | "explicit_override" | "none"
        )
    }) {
        return Err(LaunchError::InvalidValue("selection_source"));
    }
    if runtime
        .family
        .as_deref()
        .is_some_and(|family| !matches!(family, "wine" | "proton"))
    {
        return Err(LaunchError::InvalidValue("runtime_family"));
    }
    if runtime.runtime_id.as_deref().is_some_and(|value| {
        !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_:".contains(&byte))
    }) {
        return Err(LaunchError::InvalidValue("runtime_id"));
    }
    if runtime
        .pin_id
        .as_deref()
        .is_some_and(|pin_id| validate_id(pin_id, "pin_id").is_err())
    {
        return Err(LaunchError::InvalidValue("pin_id"));
    }
    Ok(())
}

fn validate_prefix_evidence(prefix: &PrefixEvidence) -> Result<(), LaunchError> {
    if prefix
        .prefix_id
        .as_deref()
        .is_some_and(|id| validate_id(id, "prefix_id").is_err())
    {
        return Err(LaunchError::InvalidValue("prefix_id"));
    }
    Ok(())
}

fn identity_id(identity: &PlanIdentity<'_>) -> Result<String, LaunchError> {
    let bytes = serde_json::to_vec(identity).map_err(|_| LaunchError::Serialization)?;
    Ok(format!("sha256:{}", hex(&Sha256::digest(bytes))))
}
fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 15) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> GameLaunchRequest {
        GameLaunchRequest::new(
            "endfield-main",
            "prefix-main",
            Some("desktop".into()),
            "Game/Selected.exe",
            RuntimeAdapter::UmuProton,
        )
        .unwrap()
    }
    fn config() -> LaunchConfiguration {
        LaunchConfiguration {
            game_registered: true,
            game_registry_revision: Some(7),
            game_root_identity_matches: true,
            executable: GameExecutableEvidence {
                relative_path: "Game/Selected.exe".into(),
                exists: true,
                regular_file: true,
                symlink_free: true,
                within_registered_root: true,
                root_identity_matches: true,
                file_device: Some(1),
                file_inode: Some(2),
                file_size: Some(4096),
                format: ExecutableFormat::PeCoff,
                publisher_authenticity_verified: false,
            },
            runtime: RuntimeSelectionEvidence {
                state: EvidenceState::Verified,
                selection_source: Some("machine_pin".into()),
                runtime_id: Some("selected-runtime".into()),
                family: Some("proton".into()),
                adapter: RuntimeAdapter::UmuProton,
                pin_id: Some("runtime-pin".into()),
                pin_revision: Some(3),
                managed_ownership_verified: false,
                baseline_integrity_verified: false,
                external_structure_revalidated: true,
                external_trust_policy_documented: true,
                invocation_contract_verified: true,
            },
            prefix: PrefixEvidence {
                state: EvidenceState::Verified,
                prefix_id: Some("prefix-main".into()),
                lifecycle: PrefixLifecycle::Initialized,
                managed_ownership_verified: true,
                directory_identity_verified: true,
                runtime_association_matches: true,
                registry_revision: Some(4),
                initialization_verified: true,
                recovery_pending: false,
            },
            compatibility: CompatibilityEvidence::Unknown,
            critical_dependencies: EvidenceState::Verified,
            policy: LaunchPolicy {
                state: EvidenceState::Verified,
                process_creation_allowed: true,
                approval_required: true,
            },
        }
    }
    #[test]
    fn path_rejects_traversal_absolute_and_windows_separators() {
        for path in [
            "../game.exe",
            "Game/../../game.exe",
            "/tmp/game.exe",
            "Game\\game.exe",
            "Game//game.exe",
        ] {
            assert_eq!(
                validate_relative_executable(path),
                Err(LaunchError::UnsafeExecutablePath)
            );
        }
    }
    #[test]
    fn invalid_game_identifiers_are_rejected_before_planning() {
        assert!(matches!(
            GameLaunchRequest::new(
                "../game",
                "prefix-main",
                None,
                "Game.exe",
                RuntimeAdapter::ManagedWine
            ),
            Err(LaunchError::InvalidValue("installation_id"))
        ));
    }
    #[test]
    fn plan_id_is_stable_and_approval_is_bound_to_observed_state() {
        let request = request();
        let config = config();
        let plan = LaunchPlan::build(&request, &config).unwrap();
        assert_eq!(
            plan.plan_id,
            LaunchPlan::build(&request, &config).unwrap().plan_id
        );
        assert!(
            plan.warnings
                .contains(&LaunchWarning::AntiCheatCompatibilityUnknown)
        );
        let approval = LaunchApproval::approve(&plan).unwrap();
        approval.validate(&plan).unwrap();
        let mut changed = config.clone();
        changed.runtime.pin_revision = Some(5);
        let changed_plan = LaunchPlan::build(&request, &changed).unwrap();
        assert_ne!(plan.plan_id, changed_plan.plan_id);
        assert_eq!(
            approval.validate(&changed_plan),
            Err(LaunchError::ApprovalMismatch)
        );
        let mut executable_replaced = config.clone();
        executable_replaced.executable.file_inode = Some(99);
        let replaced_plan = LaunchPlan::build(&request, &executable_replaced).unwrap();
        assert_ne!(plan.plan_id, replaced_plan.plan_id);
        assert_eq!(
            approval.validate(&replaced_plan),
            Err(LaunchError::ApprovalMismatch)
        );
        let mut tampered = plan.clone();
        tampered.process_execution_supported = true;
        assert_eq!(
            approval.validate(&tampered),
            Err(LaunchError::PlanIntegrityMismatch)
        );
    }
    #[test]
    fn created_prefix_is_blocked_and_cannot_be_approved() {
        let mut config = config();
        config.prefix.lifecycle = PrefixLifecycle::Created;
        config.prefix.initialization_verified = false;
        let plan = LaunchPlan::build(&request(), &config).unwrap();
        assert!(
            plan.blockers
                .contains(&LaunchBlocker::PrefixInitializationUnsupported)
        );
        assert_eq!(plan.state, LaunchState::Blocked);
        assert_eq!(LaunchApproval::approve(&plan), Err(LaunchError::Blocked));
    }
    #[test]
    fn unknown_execution_policy_blocks_and_cannot_be_approved() {
        let mut config = config();
        config.policy = LaunchPolicy::fail_closed();
        let plan = LaunchPlan::build(&request(), &config).unwrap();
        assert!(
            plan.blockers
                .contains(&LaunchBlocker::ExecutionPolicyUnknown)
        );
        assert_eq!(plan.state, LaunchState::Blocked);
        assert_eq!(LaunchApproval::approve(&plan), Err(LaunchError::Blocked));
    }
    #[test]
    fn symlink_recovery_and_unknown_critical_dependency_evidence_block() {
        let mut config = config();
        config.executable.symlink_free = false;
        config.prefix.recovery_pending = true;
        config.prefix.runtime_association_matches = false;
        config.critical_dependencies = EvidenceState::Unknown;
        let plan = LaunchPlan::build(&request(), &config).unwrap();
        for blocker in [
            LaunchBlocker::ExecutableSymlink,
            LaunchBlocker::PrefixRecoveryPending,
            LaunchBlocker::PrefixStale,
            LaunchBlocker::CriticalDependencyReadinessUnknown,
        ] {
            assert!(plan.blockers.contains(&blocker), "missing {blocker:?}");
        }
        assert_eq!(LaunchApproval::approve(&plan), Err(LaunchError::Blocked));
    }
    #[test]
    fn redacted_plan_has_no_absolute_installation_path() {
        let plan = LaunchPlan::build(&request(), &config()).unwrap();
        let json = plan.redacted_json().unwrap();
        assert!(json.contains("Game/Selected.exe"));
        assert!(!json.contains("/home/"));
    }

    #[test]
    fn production_execution_gate_stays_closed_even_after_plan_approval() {
        let plan = LaunchPlan::build(&request(), &config()).unwrap();
        let approval = LaunchApproval::approve(&plan).unwrap();
        assert_eq!(
            plan.validate_execution_approval(&approval),
            Err(LaunchError::ExecutionUnsupported)
        );
    }

    #[test]
    fn lifecycle_rejects_skipped_and_terminal_transitions() {
        assert_eq!(
            LaunchState::Prepared.transition(LaunchState::Started),
            Err(LaunchError::InvalidTransition)
        );
        assert_eq!(
            LaunchState::Started.transition(LaunchState::Exited),
            Ok(LaunchState::Exited)
        );
        assert_eq!(
            LaunchState::Exited.transition(LaunchState::Failed),
            Err(LaunchError::InvalidTransition)
        );
    }
}
