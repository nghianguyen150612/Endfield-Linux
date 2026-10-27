use crate::adapter::{RuntimeAdapterKind, RuntimeAdapterSelection};
use crate::capabilities::{
    CapabilityStatus, ProviderAvailability, ProviderCapability, ProviderCapabilityKind,
};
use crate::compatibility::{CompatibilityAssessment, CompatibilityScope};
use crate::errors::{ContractError, ProviderError, ProviderErrorKind, ProviderOperation};
use crate::identity::{
    ArtifactId, Identifier, ProcessArchitecture, ProviderId, RuntimeFamily, RuntimeId,
    RuntimeIdentity, RuntimeLineage, RuntimeOwnership, RuntimePlatform, RuntimeVersion,
};
use crate::integrity::{ExpectedDigest, IntegrityAssessment};
use crate::launch_plan::LaunchPlan;
use crate::source::{SourceMetadata, SourceVerification};
use crate::validation::{MAX_PROVIDER_CAPABILITIES, MAX_RUNTIME_CANDIDATES};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderDeclaration {
    provider_id: ProviderId,
    availability: ProviderAvailability,
    supported_families: Vec<RuntimeFamily>,
    capabilities: Vec<ProviderCapability>,
}

impl ProviderDeclaration {
    pub fn new(
        provider_id: ProviderId,
        availability: ProviderAvailability,
        mut supported_families: Vec<RuntimeFamily>,
        mut capabilities: Vec<ProviderCapability>,
    ) -> Result<Self, ContractError> {
        if supported_families.len() > 8 {
            return Err(ContractError::TooManyValues {
                field: "runtime families",
                maximum: 8,
            });
        }
        if capabilities.len() > MAX_PROVIDER_CAPABILITIES {
            return Err(ContractError::TooManyValues {
                field: "provider capabilities",
                maximum: MAX_PROVIDER_CAPABILITIES,
            });
        }
        supported_families.sort_unstable();
        supported_families.dedup();
        capabilities.sort_by_key(|capability| capability.kind());
        if capabilities
            .windows(2)
            .any(|pair| pair[0].kind() == pair[1].kind())
        {
            return Err(ContractError::DuplicateValue {
                field: "provider capability",
            });
        }
        for capability in &capabilities {
            if capability.status() == CapabilityStatus::VerifiedUsable
                && availability != ProviderAvailability::Available
            {
                return Err(ContractError::InvalidValue {
                    field: "provider capability",
                    reason: "verified usable capability requires an available provider",
                });
            }
            match capability.kind() {
                ProviderCapabilityKind::SupportsArchitecture(ProcessArchitecture::Unknown)
                | ProviderCapabilityKind::SupportsPlatform(RuntimePlatform::Unknown) => {
                    return Err(ContractError::InvalidValue {
                        field: "provider capability",
                        reason: "unknown architecture or platform cannot be declared supported",
                    });
                }
                ProviderCapabilityKind::IntegrateWithUmu
                    if !supported_families.contains(&RuntimeFamily::Proton) =>
                {
                    return Err(ContractError::InvalidValue {
                        field: "UMU capability",
                        reason: "UMU integration applies to a Proton provider",
                    });
                }
                ProviderCapabilityKind::LaunchWithAdapter(adapter) => {
                    if !supported_families.contains(&adapter.family()) {
                        return Err(ContractError::InvalidValue {
                            field: "adapter capability",
                            reason: "adapter family is not declared by the provider",
                        });
                    }
                    if capability.status().is_declared_supported()
                        && !capabilities.iter().any(|item| {
                            item.kind() == ProviderCapabilityKind::PrepareLaunchPlan
                                && item.status().is_declared_supported()
                        })
                    {
                        return Err(ContractError::InvalidValue {
                            field: "adapter capability",
                            reason: "launch adapter requires launch-plan preparation capability",
                        });
                    }
                    if capability.status().is_declared_supported()
                        && adapter == RuntimeAdapterKind::UmuProton
                        && !capabilities.iter().any(|item| {
                            item.kind() == ProviderCapabilityKind::IntegrateWithUmu
                                && item.status().is_declared_supported()
                        })
                    {
                        return Err(ContractError::InvalidValue {
                            field: "UMU adapter capability",
                            reason: "UMU launch requires UMU integration capability",
                        });
                    }
                }
                _ => {}
            }
        }
        if capabilities.iter().any(|item| {
            item.kind() == ProviderCapabilityKind::ManageUpdates
                && item.status().is_declared_supported()
        }) && !capabilities.iter().any(|item| {
            item.kind() == ProviderCapabilityKind::ManageInstallations
                && item.status().is_declared_supported()
        }) {
            return Err(ContractError::InvalidValue {
                field: "update capability",
                reason: "runtime updates require runtime installation management",
            });
        }
        Ok(Self {
            provider_id,
            availability,
            supported_families,
            capabilities,
        })
    }

    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub const fn availability(&self) -> ProviderAvailability {
        self.availability
    }

    pub fn supported_families(&self) -> &[RuntimeFamily] {
        &self.supported_families
    }

    pub fn capabilities(&self) -> &[ProviderCapability] {
        &self.capabilities
    }

    pub fn supports_family(&self, family: RuntimeFamily) -> bool {
        self.supported_families.contains(&family)
    }

    pub fn capability_status(&self, kind: ProviderCapabilityKind) -> Option<CapabilityStatus> {
        self.capabilities
            .iter()
            .find(|capability| capability.kind() == kind)
            .map(ProviderCapability::status)
    }

    fn operation_capability(operation: ProviderOperation) -> ProviderCapabilityKind {
        match operation {
            ProviderOperation::Discover => ProviderCapabilityKind::DiscoverBuilds,
            ProviderOperation::InspectIdentity => ProviderCapabilityKind::InspectIdentity,
            ProviderOperation::VerifySource => ProviderCapabilityKind::VerifySource,
            ProviderOperation::VerifyIntegrity => ProviderCapabilityKind::VerifyIntegrity,
            ProviderOperation::AssessCompatibility => ProviderCapabilityKind::AssessCompatibility,
            ProviderOperation::PlanInstallation => ProviderCapabilityKind::PlanInstallation,
            ProviderOperation::PrepareLaunchPlan => ProviderCapabilityKind::PrepareLaunchPlan,
        }
    }

    fn operation_error(&self, operation: ProviderOperation) -> ProviderError {
        let kind = if self.availability == ProviderAvailability::Unavailable {
            ProviderErrorKind::ProviderUnavailable
        } else if self.availability == ProviderAvailability::Unknown {
            ProviderErrorKind::Unknown
        } else {
            match self.capability_status(Self::operation_capability(operation)) {
                Some(CapabilityStatus::Unsupported) => ProviderErrorKind::Unsupported,
                Some(CapabilityStatus::Unavailable) => ProviderErrorKind::ProviderUnavailable,
                Some(CapabilityStatus::Unknown) | None => ProviderErrorKind::Unknown,
                Some(
                    CapabilityStatus::SupportedByContract
                    | CapabilityStatus::ImplementedByProvider
                    | CapabilityStatus::VerifiedUsable,
                ) => ProviderErrorKind::NotImplemented,
            }
        };
        ProviderError::new(self.provider_id.identifier().clone(), operation, kind)
    }

    fn operation_gate(&self, operation: ProviderOperation) -> Option<ProviderError> {
        let status = self.capability_status(Self::operation_capability(operation));
        match (self.availability, status) {
            (ProviderAvailability::Unavailable, _) => Some(self.operation_error(operation)),
            (ProviderAvailability::Unknown, _) => Some(self.operation_error(operation)),
            (_, Some(CapabilityStatus::Unsupported | CapabilityStatus::Unavailable)) => {
                Some(self.operation_error(operation))
            }
            (_, Some(CapabilityStatus::Unknown)) | (_, None) => {
                Some(self.operation_error(operation))
            }
            (_, Some(CapabilityStatus::SupportedByContract)) => Some(ProviderError::new(
                self.provider_id.identifier().clone(),
                operation,
                ProviderErrorKind::NotImplemented,
            )),
            (
                ProviderAvailability::Available,
                Some(CapabilityStatus::ImplementedByProvider | CapabilityStatus::VerifiedUsable),
            ) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryQuery {
    families: Vec<RuntimeFamily>,
    include_unknown_families: bool,
}

impl DiscoveryQuery {
    pub fn new(
        mut families: Vec<RuntimeFamily>,
        include_unknown_families: bool,
    ) -> Result<Self, ContractError> {
        if families.len() > 8 {
            return Err(ContractError::TooManyValues {
                field: "requested runtime families",
                maximum: 8,
            });
        }
        families.sort_unstable();
        families.dedup();
        Ok(Self {
            families,
            include_unknown_families,
        })
    }

    pub fn families(&self) -> &[RuntimeFamily] {
        &self.families
    }

    pub const fn includes_unknown_families(&self) -> bool {
        self.include_unknown_families
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCandidate {
    runtime_id: RuntimeId,
    provider_id: ProviderId,
    family: RuntimeFamily,
    version_hint: Option<RuntimeVersion>,
    ownership: RuntimeOwnership,
}

impl RuntimeCandidate {
    pub const fn new(
        runtime_id: RuntimeId,
        provider_id: ProviderId,
        family: RuntimeFamily,
        version_hint: Option<RuntimeVersion>,
        ownership: RuntimeOwnership,
    ) -> Self {
        Self {
            runtime_id,
            provider_id,
            family,
            version_hint,
            ownership,
        }
    }

    pub const fn runtime_id(&self) -> &RuntimeId {
        &self.runtime_id
    }

    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub const fn family(&self) -> RuntimeFamily {
        self.family
    }

    pub const fn version_hint(&self) -> Option<&RuntimeVersion> {
        self.version_hint.as_ref()
    }

    pub const fn ownership(&self) -> &RuntimeOwnership {
        &self.ownership
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryCompleteness {
    Complete,
    Partial { reason: Identifier },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryReport {
    provider_id: ProviderId,
    candidates: Vec<RuntimeCandidate>,
    completeness: DiscoveryCompleteness,
}

impl DiscoveryReport {
    pub fn new(
        provider_id: ProviderId,
        candidates: Vec<RuntimeCandidate>,
        completeness: DiscoveryCompleteness,
    ) -> Result<Self, ContractError> {
        if candidates.len() > MAX_RUNTIME_CANDIDATES {
            return Err(ContractError::TooManyValues {
                field: "runtime candidates",
                maximum: MAX_RUNTIME_CANDIDATES,
            });
        }
        for (index, candidate) in candidates.iter().enumerate() {
            if candidate.provider_id() != &provider_id {
                return Err(ContractError::InvalidValue {
                    field: "discovery result",
                    reason: "candidate provider does not match the report provider",
                });
            }
            if candidates[..index]
                .iter()
                .any(|prior| prior.runtime_id() == candidate.runtime_id())
            {
                return Err(ContractError::DuplicateValue {
                    field: "runtime candidate",
                });
            }
        }
        Ok(Self {
            provider_id,
            candidates,
            completeness,
        })
    }

    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub fn candidates(&self) -> &[RuntimeCandidate] {
        &self.candidates
    }

    pub const fn completeness(&self) -> &DiscoveryCompleteness {
        &self.completeness
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceVerificationRequest {
    source: SourceMetadata,
}

impl SourceVerificationRequest {
    pub const fn new(source: SourceMetadata) -> Self {
        Self { source }
    }

    pub const fn source(&self) -> &SourceMetadata {
        &self.source
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrityVerificationRequest {
    artifact_id: ArtifactId,
    expected: Option<ExpectedDigest>,
}

impl IntegrityVerificationRequest {
    pub const fn new(artifact_id: ArtifactId, expected: Option<ExpectedDigest>) -> Self {
        Self {
            artifact_id,
            expected,
        }
    }

    pub const fn artifact_id(&self) -> &ArtifactId {
        &self.artifact_id
    }

    pub const fn expected(&self) -> Option<&ExpectedDigest> {
        self.expected.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallationPlanRequest {
    family: RuntimeFamily,
    lineage: RuntimeLineage,
    provider_id: ProviderId,
    version: RuntimeVersion,
    target_id: Identifier,
}

impl InstallationPlanRequest {
    pub fn new(
        family: RuntimeFamily,
        lineage: RuntimeLineage,
        provider_id: ProviderId,
        version: RuntimeVersion,
        target_id: Identifier,
    ) -> Result<Self, ContractError> {
        if !lineage.is_valid_for(family) {
            return Err(ContractError::InvalidValue {
                field: "installation plan",
                reason: "runtime lineage does not match family",
            });
        }
        Ok(Self {
            family,
            lineage,
            provider_id,
            version,
            target_id,
        })
    }

    pub const fn family(&self) -> RuntimeFamily {
        self.family
    }

    pub const fn lineage(&self) -> &RuntimeLineage {
        &self.lineage
    }

    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub const fn version(&self) -> &RuntimeVersion {
        &self.version
    }

    pub const fn target_id(&self) -> &Identifier {
        &self.target_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallationPlan {
    family: RuntimeFamily,
    lineage: RuntimeLineage,
    provider_id: ProviderId,
    version: RuntimeVersion,
    target_id: Identifier,
    ownership_after_install: RuntimeOwnership,
}

impl InstallationPlan {
    pub fn from_request(request: &InstallationPlanRequest) -> Self {
        Self {
            family: request.family,
            lineage: request.lineage.clone(),
            provider_id: request.provider_id.clone(),
            version: request.version.clone(),
            target_id: request.target_id.clone(),
            ownership_after_install: RuntimeOwnership::EndfieldManaged,
        }
    }

    pub const fn family(&self) -> RuntimeFamily {
        self.family
    }

    pub const fn lineage(&self) -> &RuntimeLineage {
        &self.lineage
    }

    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub const fn version(&self) -> &RuntimeVersion {
        &self.version
    }

    pub const fn target_id(&self) -> &Identifier {
        &self.target_id
    }

    pub const fn ownership_after_install(&self) -> &RuntimeOwnership {
        &self.ownership_after_install
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchPlanRequest {
    request_id: Identifier,
    runtime: RuntimeIdentity,
    adapter: RuntimeAdapterSelection,
    target_platform: RuntimePlatform,
    process_architecture: ProcessArchitecture,
}

impl LaunchPlanRequest {
    pub fn new(
        request_id: Identifier,
        runtime: RuntimeIdentity,
        adapter: RuntimeAdapterSelection,
        target_platform: RuntimePlatform,
        process_architecture: ProcessArchitecture,
    ) -> Result<Self, ContractError> {
        if !adapter.applies_to(&runtime) {
            return Err(ContractError::InvalidAdapterCombination);
        }
        Ok(Self {
            request_id,
            runtime,
            adapter,
            target_platform,
            process_architecture,
        })
    }

    pub const fn request_id(&self) -> &Identifier {
        &self.request_id
    }

    pub const fn runtime(&self) -> &RuntimeIdentity {
        &self.runtime
    }

    pub const fn adapter(&self) -> &RuntimeAdapterSelection {
        &self.adapter
    }

    pub const fn target_platform(&self) -> RuntimePlatform {
        self.target_platform
    }

    pub const fn process_architecture(&self) -> ProcessArchitecture {
        self.process_architecture
    }
}

/// Portable provider contract. Each public operation is gated by the declaration
/// before its injectable implementation hook is called. An empty discovery report
/// means the implementation ran and found no candidates; it is not the default.
/// Implementations must not treat declaration as proof that a runtime is installed.
pub trait RuntimeProvider {
    fn declaration(&self) -> &ProviderDeclaration;

    fn discover(&self, query: &DiscoveryQuery) -> Result<DiscoveryReport, ProviderError> {
        let operation = ProviderOperation::Discover;
        if let Some(error) = self.declaration().operation_gate(operation) {
            return Err(error);
        }
        self.discover_impl(query)
    }

    fn discover_impl(&self, _query: &DiscoveryQuery) -> Result<DiscoveryReport, ProviderError> {
        Err(ProviderError::new(
            self.declaration().provider_id.identifier().clone(),
            ProviderOperation::Discover,
            ProviderErrorKind::NotImplemented,
        ))
    }

    fn inspect_identity(
        &self,
        candidate: &RuntimeCandidate,
    ) -> Result<RuntimeIdentity, ProviderError> {
        let operation = ProviderOperation::InspectIdentity;
        if let Some(error) = self.declaration().operation_gate(operation) {
            return Err(error);
        }
        self.inspect_identity_impl(candidate)
    }

    fn inspect_identity_impl(
        &self,
        _candidate: &RuntimeCandidate,
    ) -> Result<RuntimeIdentity, ProviderError> {
        Err(ProviderError::new(
            self.declaration().provider_id.identifier().clone(),
            ProviderOperation::InspectIdentity,
            ProviderErrorKind::NotImplemented,
        ))
    }

    fn verify_source(
        &self,
        request: &SourceVerificationRequest,
    ) -> Result<SourceVerification, ProviderError> {
        let operation = ProviderOperation::VerifySource;
        if let Some(error) = self.declaration().operation_gate(operation) {
            return Err(error);
        }
        self.verify_source_impl(request)
    }

    fn verify_source_impl(
        &self,
        _request: &SourceVerificationRequest,
    ) -> Result<SourceVerification, ProviderError> {
        Err(ProviderError::new(
            self.declaration().provider_id.identifier().clone(),
            ProviderOperation::VerifySource,
            ProviderErrorKind::NotImplemented,
        ))
    }

    fn verify_integrity(
        &self,
        request: &IntegrityVerificationRequest,
    ) -> Result<IntegrityAssessment, ProviderError> {
        let operation = ProviderOperation::VerifyIntegrity;
        if let Some(error) = self.declaration().operation_gate(operation) {
            return Err(error);
        }
        self.verify_integrity_impl(request)
    }

    fn verify_integrity_impl(
        &self,
        _request: &IntegrityVerificationRequest,
    ) -> Result<IntegrityAssessment, ProviderError> {
        Err(ProviderError::new(
            self.declaration().provider_id.identifier().clone(),
            ProviderOperation::VerifyIntegrity,
            ProviderErrorKind::NotImplemented,
        ))
    }

    fn assess_compatibility(
        &self,
        scope: &CompatibilityScope,
    ) -> Result<CompatibilityAssessment, ProviderError> {
        let operation = ProviderOperation::AssessCompatibility;
        if let Some(error) = self.declaration().operation_gate(operation) {
            return Err(error);
        }
        self.assess_compatibility_impl(scope)
    }

    fn assess_compatibility_impl(
        &self,
        _scope: &CompatibilityScope,
    ) -> Result<CompatibilityAssessment, ProviderError> {
        Err(ProviderError::new(
            self.declaration().provider_id.identifier().clone(),
            ProviderOperation::AssessCompatibility,
            ProviderErrorKind::NotImplemented,
        ))
    }

    fn plan_installation(
        &self,
        request: &InstallationPlanRequest,
    ) -> Result<InstallationPlan, ProviderError> {
        let operation = ProviderOperation::PlanInstallation;
        if let Some(error) = self.declaration().operation_gate(operation) {
            return Err(error);
        }
        self.plan_installation_impl(request)
    }

    fn plan_installation_impl(
        &self,
        _request: &InstallationPlanRequest,
    ) -> Result<InstallationPlan, ProviderError> {
        Err(ProviderError::new(
            self.declaration().provider_id.identifier().clone(),
            ProviderOperation::PlanInstallation,
            ProviderErrorKind::NotImplemented,
        ))
    }

    fn prepare_launch_plan(
        &self,
        request: &LaunchPlanRequest,
    ) -> Result<LaunchPlan, ProviderError> {
        let operation = ProviderOperation::PrepareLaunchPlan;
        if let Some(error) = self.declaration().operation_gate(operation) {
            return Err(error);
        }
        self.prepare_launch_plan_impl(request)
    }

    fn prepare_launch_plan_impl(
        &self,
        _request: &LaunchPlanRequest,
    ) -> Result<LaunchPlan, ProviderError> {
        Err(ProviderError::new(
            self.declaration().provider_id.identifier().clone(),
            ProviderOperation::PrepareLaunchPlan,
            ProviderErrorKind::NotImplemented,
        ))
    }
}
