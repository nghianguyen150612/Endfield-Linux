use crate::adapter::RuntimeAdapterKind;
use crate::compatibility::CompatibilityState;
use crate::errors::ContractError;
use crate::identity::{
    Identifier, ProcessArchitecture, RuntimeFamily, RuntimeOwnership, RuntimePlatform,
};
use crate::policy::{UserAction, UserIntent};
use crate::provider::ProviderDeclaration;
use crate::source::SourceVerificationState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderAvailability {
    Available,
    Unavailable,
    Unknown,
}

impl ProviderAvailability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityStatus {
    SupportedByContract,
    ImplementedByProvider,
    VerifiedUsable,
    Unsupported,
    Unknown,
    Unavailable,
}

impl CapabilityStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SupportedByContract => "supported-by-contract",
            Self::ImplementedByProvider => "implemented-by-provider",
            Self::VerifiedUsable => "verified-usable",
            Self::Unsupported => "unsupported",
            Self::Unknown => "unknown",
            Self::Unavailable => "unavailable",
        }
    }

    pub const fn is_declared_supported(self) -> bool {
        matches!(
            self,
            Self::SupportedByContract | Self::ImplementedByProvider | Self::VerifiedUsable
        )
    }

    pub const fn is_implemented(self) -> bool {
        matches!(self, Self::ImplementedByProvider | Self::VerifiedUsable)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProviderCapabilityKind {
    DiscoverBuilds,
    InspectIdentity,
    VerifySource,
    VerifyIntegrity,
    AssessCompatibility,
    PlanInstallation,
    PrepareLaunchPlan,
    ManageInstallations,
    ManageUpdates,
    UseIsolatedPrefix,
    IntegrateWithUmu,
    SupportsArchitecture(ProcessArchitecture),
    SupportsPlatform(RuntimePlatform),
    LaunchWithAdapter(RuntimeAdapterKind),
}

impl ProviderCapabilityKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DiscoverBuilds => "discover-builds",
            Self::InspectIdentity => "inspect-identity",
            Self::VerifySource => "verify-source",
            Self::VerifyIntegrity => "verify-integrity",
            Self::AssessCompatibility => "assess-compatibility",
            Self::PlanInstallation => "plan-installation",
            Self::PrepareLaunchPlan => "prepare-launch-plan",
            Self::ManageInstallations => "manage-installations",
            Self::ManageUpdates => "manage-updates",
            Self::UseIsolatedPrefix => "use-isolated-prefix",
            Self::IntegrateWithUmu => "integrate-with-umu",
            Self::SupportsArchitecture(_) => "supports-architecture",
            Self::SupportsPlatform(_) => "supports-platform",
            Self::LaunchWithAdapter(_) => "launch-with-adapter",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderCapability {
    kind: ProviderCapabilityKind,
    status: CapabilityStatus,
}

impl ProviderCapability {
    pub const fn new(kind: ProviderCapabilityKind, status: CapabilityStatus) -> Self {
        Self { kind, status }
    }

    pub const fn kind(&self) -> ProviderCapabilityKind {
        self.kind
    }

    pub const fn status(&self) -> CapabilityStatus {
        self.status
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestedOperation {
    Discover,
    InspectIdentity,
    VerifySource,
    VerifyIntegrity,
    AssessCompatibility,
    PlanInstallation,
    PrepareLaunch,
    Install,
    Update,
}

impl RequestedOperation {
    pub const fn capability(self) -> ProviderCapabilityKind {
        match self {
            Self::Discover => ProviderCapabilityKind::DiscoverBuilds,
            Self::InspectIdentity => ProviderCapabilityKind::InspectIdentity,
            Self::VerifySource => ProviderCapabilityKind::VerifySource,
            Self::VerifyIntegrity => ProviderCapabilityKind::VerifyIntegrity,
            Self::AssessCompatibility => ProviderCapabilityKind::AssessCompatibility,
            Self::PlanInstallation => ProviderCapabilityKind::PlanInstallation,
            Self::PrepareLaunch => ProviderCapabilityKind::PrepareLaunchPlan,
            Self::Install => ProviderCapabilityKind::ManageInstallations,
            Self::Update => ProviderCapabilityKind::ManageUpdates,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePresence {
    Installed,
    NotInstalled,
    NotDiscovered,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NegotiationResult {
    AllowedByDeclaredContract,
    EligibleForFurtherVerification,
    Unsupported,
    Unknown,
    NotImplemented,
    ProviderUnavailable,
    ProviderMissing,
    RequiresVerification,
    RequiresUserApproval,
    InsufficientEvidence,
    InvalidMetadata,
    IncompatibleRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NegotiationContext {
    pub operation: RequestedOperation,
    pub runtime_family: RuntimeFamily,
    pub adapter: Option<RuntimeAdapterKind>,
    pub runtime_presence: RuntimePresence,
    pub source_verification: SourceVerificationState,
    pub compatibility: CompatibilityState,
    pub process_architecture: Option<ProcessArchitecture>,
    pub platform: Option<RuntimePlatform>,
    pub ownership: RuntimeOwnership,
    pub resource_id: Option<Identifier>,
    pub user_intent: UserIntent,
}

impl NegotiationContext {
    pub const fn new(operation: RequestedOperation, runtime_family: RuntimeFamily) -> Self {
        Self {
            operation,
            runtime_family,
            adapter: None,
            runtime_presence: RuntimePresence::Unknown,
            source_verification: SourceVerificationState::Unverified,
            compatibility: CompatibilityState::NotEvaluated,
            process_architecture: None,
            platform: None,
            ownership: RuntimeOwnership::Unknown,
            resource_id: None,
            user_intent: UserIntent::NotProvided,
        }
    }

    pub const fn with_adapter(mut self, adapter: RuntimeAdapterKind) -> Self {
        self.adapter = Some(adapter);
        self
    }

    pub const fn with_presence(mut self, presence: RuntimePresence) -> Self {
        self.runtime_presence = presence;
        self
    }

    pub const fn with_verification(
        mut self,
        source_verification: SourceVerificationState,
        compatibility: CompatibilityState,
    ) -> Self {
        self.source_verification = source_verification;
        self.compatibility = compatibility;
        self
    }

    pub const fn with_target(
        mut self,
        architecture: ProcessArchitecture,
        platform: RuntimePlatform,
    ) -> Self {
        self.process_architecture = Some(architecture);
        self.platform = Some(platform);
        self
    }

    pub fn with_management_context(
        mut self,
        ownership: RuntimeOwnership,
        resource_id: Identifier,
        user_intent: UserIntent,
    ) -> Self {
        self.ownership = ownership;
        self.resource_id = Some(resource_id);
        self.user_intent = user_intent;
        self
    }
}

pub fn negotiate_capability(
    provider: Option<&ProviderDeclaration>,
    context: NegotiationContext,
) -> NegotiationResult {
    let Some(provider) = provider else {
        return NegotiationResult::ProviderMissing;
    };
    if provider.availability() == ProviderAvailability::Unavailable {
        return NegotiationResult::ProviderUnavailable;
    }
    if provider.availability() == ProviderAvailability::Unknown {
        return NegotiationResult::Unknown;
    }
    if !provider.supports_family(context.runtime_family) {
        return NegotiationResult::IncompatibleRequest;
    }

    let capability = context.operation.capability();
    let Some(status) = provider.capability_status(capability) else {
        return NegotiationResult::Unknown;
    };
    match status {
        CapabilityStatus::Unsupported => return NegotiationResult::Unsupported,
        CapabilityStatus::Unknown => return NegotiationResult::Unknown,
        CapabilityStatus::Unavailable => return NegotiationResult::ProviderUnavailable,
        CapabilityStatus::SupportedByContract => {
            return NegotiationResult::EligibleForFurtherVerification;
        }
        CapabilityStatus::ImplementedByProvider | CapabilityStatus::VerifiedUsable => {}
    }

    if context.operation == RequestedOperation::PrepareLaunch {
        match context.runtime_presence {
            RuntimePresence::Installed => {}
            RuntimePresence::NotDiscovered => return NegotiationResult::RequiresVerification,
            RuntimePresence::NotInstalled => return NegotiationResult::IncompatibleRequest,
            RuntimePresence::Unknown => return NegotiationResult::InsufficientEvidence,
        }
        if context.source_verification != SourceVerificationState::ArtifactIntegrityVerified
            || context.compatibility != CompatibilityState::SupportedByVerifiedEvidence
        {
            return NegotiationResult::InsufficientEvidence;
        }
        let Some(adapter) = context.adapter else {
            return NegotiationResult::InvalidMetadata;
        };
        if adapter.family() != context.runtime_family {
            return NegotiationResult::IncompatibleRequest;
        }
        if let Some(architecture) = context.process_architecture {
            if architecture == ProcessArchitecture::Unknown {
                return NegotiationResult::InsufficientEvidence;
            }
            match provider
                .capability_status(ProviderCapabilityKind::SupportsArchitecture(architecture))
            {
                Some(CapabilityStatus::Unsupported) => {
                    return NegotiationResult::IncompatibleRequest;
                }
                Some(CapabilityStatus::Unknown) | None => {
                    return NegotiationResult::InsufficientEvidence;
                }
                Some(CapabilityStatus::Unavailable) => {
                    return NegotiationResult::ProviderUnavailable;
                }
                Some(CapabilityStatus::SupportedByContract) => {
                    return NegotiationResult::EligibleForFurtherVerification;
                }
                Some(
                    CapabilityStatus::ImplementedByProvider | CapabilityStatus::VerifiedUsable,
                ) => {}
            }
        }
        if let Some(platform) = context.platform {
            if platform == RuntimePlatform::Unknown {
                return NegotiationResult::InsufficientEvidence;
            }
            match provider.capability_status(ProviderCapabilityKind::SupportsPlatform(platform)) {
                Some(CapabilityStatus::Unsupported) => {
                    return NegotiationResult::IncompatibleRequest;
                }
                Some(CapabilityStatus::Unknown) | None => {
                    return NegotiationResult::InsufficientEvidence;
                }
                Some(CapabilityStatus::Unavailable) => {
                    return NegotiationResult::ProviderUnavailable;
                }
                Some(CapabilityStatus::SupportedByContract) => {
                    return NegotiationResult::EligibleForFurtherVerification;
                }
                Some(
                    CapabilityStatus::ImplementedByProvider | CapabilityStatus::VerifiedUsable,
                ) => {}
            }
        }
        let Some(adapter_status) =
            provider.capability_status(ProviderCapabilityKind::LaunchWithAdapter(adapter))
        else {
            return NegotiationResult::Unsupported;
        };
        match adapter_status {
            CapabilityStatus::Unsupported => return NegotiationResult::Unsupported,
            CapabilityStatus::Unknown => return NegotiationResult::Unknown,
            CapabilityStatus::Unavailable => return NegotiationResult::ProviderUnavailable,
            CapabilityStatus::SupportedByContract => {
                return NegotiationResult::EligibleForFurtherVerification;
            }
            CapabilityStatus::ImplementedByProvider | CapabilityStatus::VerifiedUsable => {}
        }
        if adapter == RuntimeAdapterKind::UmuProton {
            match provider.capability_status(ProviderCapabilityKind::IntegrateWithUmu) {
                Some(
                    CapabilityStatus::ImplementedByProvider | CapabilityStatus::VerifiedUsable,
                ) => {}
                Some(CapabilityStatus::Unsupported) => return NegotiationResult::Unsupported,
                Some(CapabilityStatus::Unavailable) => {
                    return NegotiationResult::ProviderUnavailable;
                }
                Some(CapabilityStatus::Unknown) | None => return NegotiationResult::Unknown,
                Some(CapabilityStatus::SupportedByContract) => {
                    return NegotiationResult::EligibleForFurtherVerification;
                }
            }
        }
        if adapter == RuntimeAdapterKind::VerifiedDirectProton
            && context.compatibility != CompatibilityState::SupportedByVerifiedEvidence
        {
            return NegotiationResult::RequiresVerification;
        }
        let Some(resource_id) = context.resource_id.as_ref() else {
            return NegotiationResult::RequiresUserApproval;
        };
        if !context
            .user_intent
            .approves(UserAction::PrepareLaunch, resource_id)
        {
            return NegotiationResult::RequiresUserApproval;
        }
        return NegotiationResult::AllowedByDeclaredContract;
    }

    if matches!(
        context.operation,
        RequestedOperation::Install | RequestedOperation::Update
    ) {
        if !context.ownership.is_endfield_managed() {
            return NegotiationResult::IncompatibleRequest;
        }
        let Some(resource_id) = context.resource_id.as_ref() else {
            return NegotiationResult::RequiresUserApproval;
        };
        let action = match context.operation {
            RequestedOperation::Install => UserAction::InstallRuntime,
            RequestedOperation::Update => UserAction::UpdateRuntime,
            _ => return NegotiationResult::IncompatibleRequest,
        };
        if !context.user_intent.approves(action, resource_id) {
            return NegotiationResult::RequiresUserApproval;
        }
    }
    NegotiationResult::AllowedByDeclaredContract
}

impl From<ContractError> for NegotiationResult {
    fn from(_: ContractError) -> Self {
        Self::InvalidMetadata
    }
}
