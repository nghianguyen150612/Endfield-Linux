use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::adapter::{RuntimeAdapterKind, RuntimeAdapterSelection};
use crate::compatibility::{CompatibilityAssessment, CompatibilityState};
use crate::environment::{EnvironmentChange, ValueSensitivity};
use crate::errors::ContractError;
use crate::identity::ProviderId;
use crate::identity::{Identifier, RuntimeIdentity};
use crate::source::SourceVerificationState;
use crate::validation::{
    MAX_ARGUMENT_BYTES, MAX_ARGUMENTS, MAX_ENVIRONMENT_CHANGES, MAX_ENVIRONMENT_VALUE_BYTES,
    MAX_MOUNT_REQUIREMENTS, MAX_RUNTIME_METADATA_BYTES, contains_parent_component, validate_os_str,
    validate_path,
};

pub const CURRENT_LAUNCH_PLAN_SCHEMA_VERSION: u32 = 1;
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;
const MAX_DRY_RUN_JSON_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathClassification {
    Public,
    Private,
}

impl PathClassification {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutablePathPolicy {
    Absolute,
    RelativeToWorkingDirectory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Executable {
    path: PathBuf,
    path_policy: ExecutablePathPolicy,
    classification: PathClassification,
}

impl Executable {
    pub fn new(
        path: impl Into<PathBuf>,
        path_policy: ExecutablePathPolicy,
        classification: PathClassification,
    ) -> Result<Self, ContractError> {
        let path = path.into();
        validate_path(
            &path,
            "executable",
            path_policy == ExecutablePathPolicy::Absolute,
        )?;
        if contains_parent_component(&path) {
            return Err(ContractError::InvalidValue {
                field: "executable",
                reason: "parent-directory components are not allowed",
            });
        }
        Ok(Self {
            path,
            path_policy,
            classification,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn path_policy(&self) -> ExecutablePathPolicy {
        self.path_policy
    }

    pub const fn classification(&self) -> PathClassification {
        self.classification
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkingDirectory {
    path: PathBuf,
    classification: PathClassification,
}

impl WorkingDirectory {
    pub fn new(
        path: impl Into<PathBuf>,
        classification: PathClassification,
    ) -> Result<Self, ContractError> {
        let path = path.into();
        validate_path(&path, "working directory", true)?;
        if contains_parent_component(&path) {
            return Err(ContractError::InvalidValue {
                field: "working directory",
                reason: "parent-directory components are not allowed",
            });
        }
        Ok(Self {
            path,
            classification,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn classification(&self) -> PathClassification {
        self.classification
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchArgument {
    value: OsString,
    sensitivity: ValueSensitivity,
}

impl LaunchArgument {
    pub fn new(
        value: impl Into<OsString>,
        sensitivity: ValueSensitivity,
    ) -> Result<Self, ContractError> {
        let value = value.into();
        validate_os_str(&value, "argument", MAX_ARGUMENT_BYTES, true)?;
        Ok(Self { value, sensitivity })
    }

    pub fn value(&self) -> &OsString {
        &self.value
    }

    pub const fn sensitivity(&self) -> ValueSensitivity {
        self.sensitivity
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefixOwnership {
    EndfieldManaged,
    ExternalUserManaged,
    OtherLauncherManaged,
    Unknown,
}

impl PrefixOwnership {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EndfieldManaged => "endfield-managed",
            Self::ExternalUserManaged => "external-user-managed",
            Self::OtherLauncherManaged => "other-launcher-managed",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixReference {
    id: Identifier,
    path: PathBuf,
    classification: PathClassification,
    ownership: PrefixOwnership,
}

impl PrefixReference {
    pub fn new(
        id: Identifier,
        path: impl Into<PathBuf>,
        classification: PathClassification,
        ownership: PrefixOwnership,
    ) -> Result<Self, ContractError> {
        let path = path.into();
        validate_path(&path, "prefix reference", true)?;
        if contains_parent_component(&path) {
            return Err(ContractError::InvalidValue {
                field: "prefix reference",
                reason: "parent-directory components are not allowed",
            });
        }
        Ok(Self {
            id,
            path,
            classification,
            ownership,
        })
    }

    pub const fn id(&self) -> &Identifier {
        &self.id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub const fn classification(&self) -> PathClassification {
        self.classification
    }

    pub const fn ownership(&self) -> PrefixOwnership {
        self.ownership
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathOwnership {
    EndfieldManaged,
    External,
    Unknown,
}

impl PathOwnership {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EndfieldManaged => "endfield-managed",
            Self::External => "external",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountAccess {
    ReadOnly,
    ReadWrite,
}

impl MountAccess {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::ReadWrite => "read-write",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountRequirement {
    source: PathBuf,
    source_classification: PathClassification,
    source_ownership: PathOwnership,
    target: PathBuf,
    target_classification: PathClassification,
    access: MountAccess,
}

impl MountRequirement {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source: impl Into<PathBuf>,
        source_classification: PathClassification,
        source_ownership: PathOwnership,
        target: impl Into<PathBuf>,
        target_classification: PathClassification,
        access: MountAccess,
    ) -> Result<Self, ContractError> {
        let source = source.into();
        let target = target.into();
        validate_path(&source, "mount source", true)?;
        validate_path(&target, "mount target", true)?;
        if contains_parent_component(&source) || contains_parent_component(&target) {
            return Err(ContractError::InvalidValue {
                field: "mount requirement",
                reason: "parent-directory components are not allowed",
            });
        }
        if access == MountAccess::ReadWrite && source_ownership == PathOwnership::Unknown {
            return Err(ContractError::InvalidValue {
                field: "mount source ownership",
                reason: "writable mount ownership must be explicit",
            });
        }
        Ok(Self {
            source,
            source_classification,
            source_ownership,
            target,
            target_classification,
            access,
        })
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn target(&self) -> &Path {
        &self.target
    }

    pub const fn access(&self) -> MountAccess {
        self.access
    }

    pub const fn source_classification(&self) -> PathClassification {
        self.source_classification
    }

    pub const fn target_classification(&self) -> PathClassification {
        self.target_classification
    }

    pub const fn source_ownership(&self) -> PathOwnership {
        self.source_ownership
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputCapturePolicy {
    Inherit,
    Discard,
    CaptureBounded { maximum_bytes: usize },
}

impl OutputCapturePolicy {
    pub fn validate(self) -> Result<(), ContractError> {
        match self {
            Self::CaptureBounded { maximum_bytes }
                if maximum_bytes == 0 || maximum_bytes > MAX_CAPTURE_BYTES =>
            {
                Err(ContractError::InvalidValue {
                    field: "output capture",
                    reason: "capture bound must be between 1 byte and 1 MiB",
                })
            }
            _ => Ok(()),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::Discard => "discard",
            Self::CaptureBounded { .. } => "capture-bounded",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessOwnershipPolicy {
    NoOwnershipTransfer,
    ProviderOwnedChild,
    DaemonOwnedSession,
}

impl ProcessOwnershipPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoOwnershipTransfer => "no-ownership-transfer",
            Self::ProviderOwnedChild => "provider-owned-child",
            Self::DaemonOwnedSession => "daemon-owned-session",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanRisk {
    ObservationOnly,
    UserScopedLaunch,
    FilesystemChangingSetup,
    PrivilegedOrUnsupported,
}

impl PlanRisk {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ObservationOnly => "observation-only",
            Self::UserScopedLaunch => "user-scoped-launch",
            Self::FilesystemChangingSetup => "filesystem-changing-setup",
            Self::PrivilegedOrUnsupported => "privileged-or-unsupported",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanAuthorization {
    NotRequested,
    Pending,
    ExplicitlyGranted {
        grant_id: Identifier,
        plan_id: Identifier,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructuralValidationState {
    StructurallyValid,
}

impl StructuralValidationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StructurallyValid => "structurally-valid",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchReadiness {
    ReviewDoesNotMatchPlan,
    ProviderUnavailable,
    ProviderCapabilityUnknown,
    ProviderOperationNotImplemented,
    UnsupportedOperation,
    SourceVerificationRequired,
    CompatibilityEvidenceRequired,
    UserApprovalRequired,
    UnsupportedRisk,
    EligibleForFurtherReview,
}

impl LaunchReadiness {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReviewDoesNotMatchPlan => "review-does-not-match-plan",
            Self::ProviderUnavailable => "provider-unavailable",
            Self::ProviderCapabilityUnknown => "provider-capability-unknown",
            Self::ProviderOperationNotImplemented => "provider-operation-not-implemented",
            Self::UnsupportedOperation => "unsupported-operation",
            Self::SourceVerificationRequired => "source-verification-required",
            Self::CompatibilityEvidenceRequired => "compatibility-evidence-required",
            Self::UserApprovalRequired => "user-approval-required",
            Self::UnsupportedRisk => "unsupported-risk",
            Self::EligibleForFurtherReview => "eligible-for-further-review",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchPlanReview {
    provider_id: ProviderId,
    adapter: RuntimeAdapterKind,
    provider_availability: crate::capabilities::ProviderAvailability,
    launch_capability: crate::capabilities::CapabilityStatus,
    authorization: PlanAuthorization,
}

impl LaunchPlanReview {
    pub fn new(
        provider_id: ProviderId,
        adapter: RuntimeAdapterKind,
        provider_availability: crate::capabilities::ProviderAvailability,
        launch_capability: crate::capabilities::CapabilityStatus,
        authorization: PlanAuthorization,
    ) -> Self {
        Self {
            provider_id,
            adapter,
            provider_availability,
            launch_capability,
            authorization,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchPlan {
    plan_id: Identifier,
    schema_version: u32,
    runtime: RuntimeIdentity,
    adapter: RuntimeAdapterSelection,
    executable: Executable,
    arguments: Vec<LaunchArgument>,
    working_directory: WorkingDirectory,
    environment: Vec<EnvironmentChange>,
    prefix: Option<PrefixReference>,
    mounts: Vec<MountRequirement>,
    output_capture: OutputCapturePolicy,
    process_ownership: ProcessOwnershipPolicy,
    risk: PlanRisk,
    source_verification: SourceVerificationState,
    compatibility: CompatibilityAssessment,
    validation_state: StructuralValidationState,
}

impl LaunchPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        plan_id: Identifier,
        schema_version: u32,
        runtime: RuntimeIdentity,
        adapter: RuntimeAdapterSelection,
        executable: Executable,
        arguments: Vec<LaunchArgument>,
        working_directory: WorkingDirectory,
        mut environment: Vec<EnvironmentChange>,
        prefix: Option<PrefixReference>,
        mounts: Vec<MountRequirement>,
        output_capture: OutputCapturePolicy,
        process_ownership: ProcessOwnershipPolicy,
        risk: PlanRisk,
        compatibility: CompatibilityAssessment,
    ) -> Result<Self, ContractError> {
        if schema_version != CURRENT_LAUNCH_PLAN_SCHEMA_VERSION {
            return Err(ContractError::InvalidValue {
                field: "launch-plan schema",
                reason: "schema version is unsupported",
            });
        }
        if !adapter.applies_to(&runtime) {
            return Err(ContractError::InvalidAdapterCombination);
        }
        if compatibility.scope().runtime_build() != &runtime.build_identity()
            || compatibility.scope().adapter() != adapter.kind()
        {
            return Err(ContractError::InvalidCompatibilityEvidence);
        }
        if arguments.len() > MAX_ARGUMENTS {
            return Err(ContractError::TooManyValues {
                field: "launch arguments",
                maximum: MAX_ARGUMENTS,
            });
        }
        if environment.len() > MAX_ENVIRONMENT_CHANGES {
            return Err(ContractError::TooManyValues {
                field: "environment changes",
                maximum: MAX_ENVIRONMENT_CHANGES,
            });
        }
        if mounts.len() > MAX_MOUNT_REQUIREMENTS {
            return Err(ContractError::TooManyValues {
                field: "mount requirements",
                maximum: MAX_MOUNT_REQUIREMENTS,
            });
        }
        let mut total_bytes = executable.path().as_os_str().as_encoded_bytes().len()
            + working_directory
                .path()
                .as_os_str()
                .as_encoded_bytes()
                .len();
        if let Some(prefix) = &prefix {
            total_bytes = total_bytes
                .saturating_add(prefix.id().as_str().len())
                .saturating_add(prefix.path().as_os_str().as_encoded_bytes().len());
        }
        for argument in &arguments {
            total_bytes = total_bytes.saturating_add(argument.value().as_encoded_bytes().len());
        }
        for change in &environment {
            total_bytes = total_bytes.saturating_add(change.name().as_str().len());
            if let Some(value) = change.value() {
                if value.as_encoded_bytes().len() > MAX_ENVIRONMENT_VALUE_BYTES {
                    return Err(ContractError::MetadataTooLarge);
                }
                total_bytes = total_bytes.saturating_add(value.as_encoded_bytes().len());
            }
        }
        for mount in &mounts {
            total_bytes = total_bytes
                .saturating_add(mount.source().as_os_str().as_encoded_bytes().len())
                .saturating_add(mount.target().as_os_str().as_encoded_bytes().len());
        }
        if total_bytes > MAX_RUNTIME_METADATA_BYTES {
            return Err(ContractError::MetadataTooLarge);
        }
        environment.sort_by(|left, right| left.name().cmp(right.name()));
        if environment
            .windows(2)
            .any(|pair| pair[0].name() == pair[1].name())
        {
            return Err(ContractError::DuplicateValue {
                field: "environment modification",
            });
        }
        for index in 0..mounts.len() {
            if mounts[index + 1..]
                .iter()
                .any(|other| other.target() == mounts[index].target())
            {
                return Err(ContractError::DuplicateValue {
                    field: "mount target",
                });
            }
        }
        if mounts
            .iter()
            .any(|mount| mount.access() == MountAccess::ReadWrite)
            && risk != PlanRisk::PrivilegedOrUnsupported
        {
            return Err(ContractError::InvalidValue {
                field: "launch risk",
                reason: "writable mount requirements need privileged-or-unsupported review",
            });
        }
        output_capture.validate()?;
        let source_verification = runtime
            .source()
            .map_or(SourceVerificationState::Unverified, |source| {
                source.verification().state()
            });
        Ok(Self {
            plan_id,
            schema_version,
            runtime,
            adapter,
            executable,
            arguments,
            working_directory,
            environment,
            prefix,
            mounts,
            output_capture,
            process_ownership,
            risk,
            source_verification,
            compatibility,
            validation_state: StructuralValidationState::StructurallyValid,
        })
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub const fn plan_id(&self) -> &Identifier {
        &self.plan_id
    }

    pub const fn runtime(&self) -> &RuntimeIdentity {
        &self.runtime
    }

    pub const fn adapter(&self) -> &RuntimeAdapterSelection {
        &self.adapter
    }

    pub const fn executable(&self) -> &Executable {
        &self.executable
    }

    pub fn arguments(&self) -> &[LaunchArgument] {
        &self.arguments
    }

    pub const fn working_directory(&self) -> &WorkingDirectory {
        &self.working_directory
    }

    pub fn environment(&self) -> &[EnvironmentChange] {
        &self.environment
    }

    pub const fn prefix(&self) -> Option<&PrefixReference> {
        self.prefix.as_ref()
    }

    pub fn mounts(&self) -> &[MountRequirement] {
        &self.mounts
    }

    pub const fn output_capture(&self) -> OutputCapturePolicy {
        self.output_capture
    }

    pub const fn process_ownership(&self) -> ProcessOwnershipPolicy {
        self.process_ownership
    }

    pub const fn risk(&self) -> PlanRisk {
        self.risk
    }

    pub const fn source_verification(&self) -> SourceVerificationState {
        self.source_verification
    }

    pub const fn compatibility(&self) -> &CompatibilityAssessment {
        &self.compatibility
    }

    pub const fn validation_state(&self) -> StructuralValidationState {
        self.validation_state
    }

    pub fn readiness(&self, review: &LaunchPlanReview) -> LaunchReadiness {
        use crate::capabilities::{CapabilityStatus, ProviderAvailability};
        if review.provider_id != *self.runtime.provider_id()
            || review.adapter != self.adapter.kind()
        {
            return LaunchReadiness::ReviewDoesNotMatchPlan;
        }
        if self.risk == PlanRisk::PrivilegedOrUnsupported {
            return LaunchReadiness::UnsupportedRisk;
        }
        match review.provider_availability {
            ProviderAvailability::Unavailable => return LaunchReadiness::ProviderUnavailable,
            ProviderAvailability::Unknown => return LaunchReadiness::ProviderCapabilityUnknown,
            ProviderAvailability::Available => {}
        }
        match review.launch_capability {
            CapabilityStatus::Unsupported => return LaunchReadiness::UnsupportedOperation,
            CapabilityStatus::Unavailable => return LaunchReadiness::ProviderUnavailable,
            CapabilityStatus::Unknown => return LaunchReadiness::ProviderCapabilityUnknown,
            CapabilityStatus::SupportedByContract => {
                return LaunchReadiness::ProviderOperationNotImplemented;
            }
            CapabilityStatus::ImplementedByProvider | CapabilityStatus::VerifiedUsable => {}
        }
        if self.source_verification != SourceVerificationState::ArtifactIntegrityVerified {
            return LaunchReadiness::SourceVerificationRequired;
        }
        if self.compatibility.state() != CompatibilityState::SupportedByVerifiedEvidence {
            return LaunchReadiness::CompatibilityEvidenceRequired;
        }
        if matches!(
            self.risk,
            PlanRisk::UserScopedLaunch | PlanRisk::FilesystemChangingSetup
        ) {
            match &review.authorization {
                PlanAuthorization::ExplicitlyGranted { plan_id, .. }
                    if plan_id == &self.plan_id => {}
                PlanAuthorization::ExplicitlyGranted { .. }
                | PlanAuthorization::NotRequested
                | PlanAuthorization::Pending => return LaunchReadiness::UserApprovalRequired,
            }
        }
        LaunchReadiness::EligibleForFurtherReview
    }

    /// Returns a deterministic JSON preview. It never creates an executable shell command.
    pub fn dry_run_json(&self) -> Result<String, ContractError> {
        self.dry_run_json_inner(None)
    }

    /// Adds the provider's declared state to a dry-run preview without changing its
    /// non-executable semantics.
    pub fn dry_run_json_with_review(
        &self,
        review: &LaunchPlanReview,
    ) -> Result<String, ContractError> {
        self.dry_run_json_inner(Some(review))
    }

    fn dry_run_json_inner(
        &self,
        review: Option<&LaunchPlanReview>,
    ) -> Result<String, ContractError> {
        let arguments: Vec<Value> = self
            .arguments
            .iter()
            .map(|argument| safe_os_value(argument.value(), argument.sensitivity()))
            .collect();
        let environment: Vec<Value> = self
            .environment
            .iter()
            .map(|change| {
                let value = change
                    .value()
                    .map(|value| safe_os_value(value, change.sensitivity()));
                json!({
                    "name": change.name().as_str(),
                    "operation": change.operation(),
                    "value": value,
                    "sensitive": change.sensitivity().is_sensitive(),
                })
            })
            .collect();
        let prefix = self.prefix.as_ref().map(|prefix| {
            json!({
                "id": prefix.id().as_str(),
                "path": safe_path(prefix.path(), prefix.classification()),
                "ownership": prefix.ownership().as_str(),
            })
        });
        let mounts: Vec<Value> = self
            .mounts
            .iter()
            .map(|mount| {
                json!({
                    "source": safe_path(mount.source(), mount.source_classification()),
                    "source_ownership": mount.source_ownership().as_str(),
                    "target": safe_path(mount.target(), mount.target_classification()),
                    "access": mount.access().as_str(),
                })
            })
            .collect();
        let build = self.runtime.build_identity();
        let serialized = json!({
            "schema_version": self.schema_version,
            "plan_id": self.plan_id.as_str(),
            "adapter": self.adapter.kind().as_str(),
            "runtime": {
                "family": self.runtime.family().as_str(),
                "lineage": self.runtime.lineage().as_str(),
                "provider": self.runtime.provider_id().as_str(),
                "version": self.runtime.version().as_str(),
                "build_channel": self.runtime.build_channel(),
                "integrity_identity": build.integrity_identity().map(|digest| {
                    json!({
                        "algorithm": digest.algorithm().as_str(),
                        "digest": digest.hexadecimal(),
                    })
                }),
            },
            "executable": safe_path(self.executable.path(), self.executable.classification()),
            "executable_path_policy": match self.executable.path_policy() {
                ExecutablePathPolicy::Absolute => "absolute",
                ExecutablePathPolicy::RelativeToWorkingDirectory => "relative-to-working-directory",
            },
            "argv": arguments,
            "working_directory": safe_path(
                self.working_directory.path(),
                self.working_directory.classification(),
            ),
            "environment": environment,
            "prefix": prefix,
            "mounts": mounts,
            "output_capture": {
                "policy": self.output_capture.as_str(),
                "maximum_bytes": match self.output_capture {
                    OutputCapturePolicy::CaptureBounded { maximum_bytes } => Some(maximum_bytes),
                    _ => None,
                },
            },
            "process_ownership": self.process_ownership.as_str(),
            "risk": self.risk.as_str(),
            "source_verification": self.source_verification.as_str(),
            "compatibility": self.compatibility.state().as_str(),
            "compatibility_evidence_count": self.compatibility.evidence().len(),
            "provider_availability": review
                .map(|item| item.provider_availability.as_str())
                .unwrap_or("unknown"),
            "launch_capability": review
                .map(|item| item.launch_capability.as_str())
                .unwrap_or("unknown"),
            "readiness": review
                .map(|item| self.readiness(item).as_str())
                .unwrap_or("not-evaluated"),
            "validation_state": self.validation_state.as_str(),
            "executable_now": false,
        });
        let output =
            serde_json::to_string(&serialized).map_err(|_| ContractError::SerializationFailed)?;
        if output.len() > MAX_DRY_RUN_JSON_BYTES {
            return Err(ContractError::MetadataTooLarge);
        }
        Ok(output)
    }
}

fn safe_path(path: &Path, classification: PathClassification) -> Value {
    match classification {
        PathClassification::Private => json!("[redacted]"),
        PathClassification::Public => match path.to_str() {
            Some(path) => json!(path),
            None => json!("[non-UTF-8 path omitted]"),
        },
    }
}

fn safe_os_value(value: &std::ffi::OsStr, sensitivity: ValueSensitivity) -> Value {
    if sensitivity.is_sensitive() {
        return json!("[redacted]");
    }
    match value.to_str() {
        Some(value) => json!(value),
        None => json!("[non-UTF-8 value omitted]"),
    }
}
