use crate::adapter::RuntimeAdapterKind;
use crate::errors::ContractError;
use crate::identity::{
    EvidenceId, GameBuildId, Identifier, ProcessArchitecture, ProfileId, ProviderId,
    RuntimeBuildIdentity, RuntimeFamily, RuntimeIdentity, RuntimePlatform,
};
use crate::validation::{MAX_PROVIDER_CAPABILITIES, validate_text};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GraphicsApi {
    Vulkan,
    OpenGl,
    DirectX,
    Unknown,
}

impl GraphicsApi {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Vulkan => "vulkan",
            Self::OpenGl => "opengl",
            Self::DirectX => "directx",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatibilityScope {
    runtime_build: RuntimeBuildIdentity,
    adapter: RuntimeAdapterKind,
    host_platform: RuntimePlatform,
    process_architecture: ProcessArchitecture,
    wine_prefix_architecture: Option<ProcessArchitecture>,
    graphics_api: Option<GraphicsApi>,
    driver_requirements: Vec<Identifier>,
    game_build: Option<GameBuildId>,
    prefix_configuration: Option<Identifier>,
    environment_restrictions: Vec<Identifier>,
}

impl CompatibilityScope {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime_build: RuntimeBuildIdentity,
        adapter: RuntimeAdapterKind,
        host_platform: RuntimePlatform,
        process_architecture: ProcessArchitecture,
        wine_prefix_architecture: Option<ProcessArchitecture>,
        graphics_api: Option<GraphicsApi>,
        mut driver_requirements: Vec<Identifier>,
        game_build: Option<GameBuildId>,
        prefix_configuration: Option<Identifier>,
        mut environment_restrictions: Vec<Identifier>,
    ) -> Result<Self, ContractError> {
        if adapter.family() != runtime_build.family() {
            return Err(ContractError::InvalidCompatibilityEvidence);
        }
        if driver_requirements.len() > MAX_PROVIDER_CAPABILITIES
            || environment_restrictions.len() > MAX_PROVIDER_CAPABILITIES
        {
            return Err(ContractError::MetadataTooLarge);
        }
        driver_requirements.sort();
        environment_restrictions.sort();
        if driver_requirements
            .windows(2)
            .any(|pair| pair[0] == pair[1])
            || environment_restrictions
                .windows(2)
                .any(|pair| pair[0] == pair[1])
        {
            return Err(ContractError::DuplicateValue {
                field: "compatibility requirement",
            });
        }
        Ok(Self {
            runtime_build,
            adapter,
            host_platform,
            process_architecture,
            wine_prefix_architecture,
            graphics_api,
            driver_requirements,
            game_build,
            prefix_configuration,
            environment_restrictions,
        })
    }

    pub const fn runtime_build(&self) -> &RuntimeBuildIdentity {
        &self.runtime_build
    }

    pub const fn adapter(&self) -> RuntimeAdapterKind {
        self.adapter
    }

    pub const fn host_platform(&self) -> RuntimePlatform {
        self.host_platform
    }

    pub const fn process_architecture(&self) -> ProcessArchitecture {
        self.process_architecture
    }

    pub const fn wine_prefix_architecture(&self) -> Option<ProcessArchitecture> {
        self.wine_prefix_architecture
    }

    pub const fn graphics_api(&self) -> Option<GraphicsApi> {
        self.graphics_api
    }

    pub fn driver_requirements(&self) -> &[Identifier] {
        &self.driver_requirements
    }

    pub const fn game_build(&self) -> Option<&GameBuildId> {
        self.game_build.as_ref()
    }

    pub const fn prefix_configuration(&self) -> Option<&Identifier> {
        self.prefix_configuration.as_ref()
    }

    pub fn environment_restrictions(&self) -> &[Identifier] {
        &self.environment_restrictions
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatibilityEvidenceStrength {
    StaticCapabilityAssessment,
    ProviderDocumentation,
    UserReportedObservation,
    ReproducibleLocalTest,
    VerifiedCompatibilityTest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatibilityEvidenceResult {
    Pass,
    Fail,
    Degraded,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatibilityTestProvenance {
    test_id: Identifier,
    host_platform: RuntimePlatform,
    process_architecture: ProcessArchitecture,
    observed_at_unix_seconds: u64,
    methodology: Identifier,
    safe_result_summary: String,
}

impl CompatibilityTestProvenance {
    pub fn new(
        test_id: Identifier,
        host_platform: RuntimePlatform,
        process_architecture: ProcessArchitecture,
        observed_at_unix_seconds: u64,
        methodology: Identifier,
        safe_result_summary: impl Into<String>,
    ) -> Result<Self, ContractError> {
        let safe_result_summary = safe_result_summary.into();
        validate_text(&safe_result_summary, "compatibility result summary", 512)?;
        Ok(Self {
            test_id,
            host_platform,
            process_architecture,
            observed_at_unix_seconds,
            methodology,
            safe_result_summary,
        })
    }

    pub const fn test_id(&self) -> &Identifier {
        &self.test_id
    }

    pub const fn host_platform(&self) -> RuntimePlatform {
        self.host_platform
    }

    pub const fn process_architecture(&self) -> ProcessArchitecture {
        self.process_architecture
    }

    pub const fn observed_at_unix_seconds(&self) -> u64 {
        self.observed_at_unix_seconds
    }

    pub const fn methodology(&self) -> &Identifier {
        &self.methodology
    }

    pub fn safe_result_summary(&self) -> &str {
        &self.safe_result_summary
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatibilityEvidence {
    id: EvidenceId,
    scope: CompatibilityScope,
    strength: CompatibilityEvidenceStrength,
    result: CompatibilityEvidenceResult,
    source_reference: Option<Identifier>,
    provenance: Option<CompatibilityTestProvenance>,
}

impl CompatibilityEvidence {
    pub fn new(
        id: EvidenceId,
        scope: CompatibilityScope,
        strength: CompatibilityEvidenceStrength,
        result: CompatibilityEvidenceResult,
        source_reference: Option<Identifier>,
        provenance: Option<CompatibilityTestProvenance>,
    ) -> Result<Self, ContractError> {
        match strength {
            CompatibilityEvidenceStrength::StaticCapabilityAssessment => {
                if provenance.is_some() {
                    return Err(ContractError::InvalidCompatibilityEvidence);
                }
            }
            CompatibilityEvidenceStrength::ProviderDocumentation
            | CompatibilityEvidenceStrength::UserReportedObservation => {
                if source_reference.is_none() || provenance.is_some() {
                    return Err(ContractError::InvalidCompatibilityEvidence);
                }
            }
            CompatibilityEvidenceStrength::ReproducibleLocalTest => {
                if provenance.is_none() {
                    return Err(ContractError::InvalidCompatibilityEvidence);
                }
            }
            CompatibilityEvidenceStrength::VerifiedCompatibilityTest => {
                if provenance.is_none()
                    || result == CompatibilityEvidenceResult::Unknown
                    || scope.runtime_build.integrity_identity().is_none()
                    || scope.host_platform == RuntimePlatform::Unknown
                    || scope.process_architecture == ProcessArchitecture::Unknown
                {
                    return Err(ContractError::InvalidCompatibilityEvidence);
                }
            }
        }
        if result == CompatibilityEvidenceResult::Pass
            && (scope
                .runtime_build
                .requirements()
                .supports_platform(scope.host_platform)
                == Some(false)
                || scope
                    .runtime_build
                    .requirements()
                    .supports_architecture(scope.process_architecture)
                    == Some(false))
        {
            return Err(ContractError::InvalidCompatibilityEvidence);
        }
        if let Some(test) = &provenance {
            if test.host_platform() != scope.host_platform
                || test.process_architecture() != scope.process_architecture
            {
                return Err(ContractError::InvalidCompatibilityEvidence);
            }
        }
        Ok(Self {
            id,
            scope,
            strength,
            result,
            source_reference,
            provenance,
        })
    }

    pub const fn id(&self) -> &EvidenceId {
        &self.id
    }

    pub const fn scope(&self) -> &CompatibilityScope {
        &self.scope
    }

    pub const fn strength(&self) -> CompatibilityEvidenceStrength {
        self.strength
    }

    pub const fn result(&self) -> CompatibilityEvidenceResult {
        self.result
    }

    pub const fn source_reference(&self) -> Option<&Identifier> {
        self.source_reference.as_ref()
    }

    pub const fn provenance(&self) -> Option<&CompatibilityTestProvenance> {
        self.provenance.as_ref()
    }

    pub fn verifies_direct_proton(&self, identity: &RuntimeIdentity) -> bool {
        self.strength == CompatibilityEvidenceStrength::VerifiedCompatibilityTest
            && self.result == CompatibilityEvidenceResult::Pass
            && self.scope.adapter == RuntimeAdapterKind::VerifiedDirectProton
            && self.scope.runtime_build == identity.build_identity()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatibilityState {
    Unknown,
    NotEvaluated,
    IneligibleByStaticConstraints,
    InsufficientEvidence,
    TechnicallyEligibleForTesting,
    SupportedByVerifiedEvidence,
    UnsupportedByVerifiedEvidence,
    Degraded,
    ConflictingEvidence,
}

impl CompatibilityState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::NotEvaluated => "not-evaluated",
            Self::IneligibleByStaticConstraints => "ineligible-by-static-constraints",
            Self::InsufficientEvidence => "insufficient-evidence",
            Self::TechnicallyEligibleForTesting => "technically-eligible-for-testing",
            Self::SupportedByVerifiedEvidence => "supported-by-verified-evidence",
            Self::UnsupportedByVerifiedEvidence => "unsupported-by-verified-evidence",
            Self::Degraded => "degraded",
            Self::ConflictingEvidence => "conflicting-evidence",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatibilityAssessment {
    scope: CompatibilityScope,
    state: CompatibilityState,
    evidence: Vec<CompatibilityEvidence>,
}

impl CompatibilityAssessment {
    pub fn from_evidence(
        scope: CompatibilityScope,
        evidence: Vec<CompatibilityEvidence>,
    ) -> Result<Self, ContractError> {
        if evidence.len() > MAX_PROVIDER_CAPABILITIES {
            return Err(ContractError::TooManyValues {
                field: "compatibility evidence",
                maximum: MAX_PROVIDER_CAPABILITIES,
            });
        }
        for (index, item) in evidence.iter().enumerate() {
            if item.scope() != &scope {
                return Err(ContractError::InvalidCompatibilityEvidence);
            }
            if evidence[..index]
                .iter()
                .any(|prior| prior.id() == item.id())
            {
                return Err(ContractError::DuplicateValue {
                    field: "compatibility evidence identifier",
                });
            }
        }
        let state = state_for_evidence(&evidence);
        Ok(Self {
            scope,
            state,
            evidence,
        })
    }

    pub const fn scope(&self) -> &CompatibilityScope {
        &self.scope
    }

    pub const fn state(&self) -> CompatibilityState {
        self.state
    }

    pub fn evidence(&self) -> &[CompatibilityEvidence] {
        &self.evidence
    }
}

fn state_for_evidence(evidence: &[CompatibilityEvidence]) -> CompatibilityState {
    if evidence.is_empty() {
        return CompatibilityState::NotEvaluated;
    }
    let has_pass = evidence
        .iter()
        .any(|item| item.result == CompatibilityEvidenceResult::Pass);
    let has_fail = evidence
        .iter()
        .any(|item| item.result == CompatibilityEvidenceResult::Fail);
    let has_degraded = evidence
        .iter()
        .any(|item| item.result == CompatibilityEvidenceResult::Degraded);
    if (has_pass && has_fail) || (has_degraded && (has_pass || has_fail)) {
        return CompatibilityState::ConflictingEvidence;
    }
    let verified: Vec<_> = evidence
        .iter()
        .filter(|item| item.strength == CompatibilityEvidenceStrength::VerifiedCompatibilityTest)
        .collect();
    if !verified.is_empty() {
        return if has_degraded {
            CompatibilityState::Degraded
        } else if has_fail {
            CompatibilityState::UnsupportedByVerifiedEvidence
        } else if has_pass {
            CompatibilityState::SupportedByVerifiedEvidence
        } else {
            CompatibilityState::Unknown
        };
    }
    if evidence.iter().all(|item| {
        item.strength == CompatibilityEvidenceStrength::StaticCapabilityAssessment
            && item.result == CompatibilityEvidenceResult::Pass
    }) {
        return CompatibilityState::TechnicallyEligibleForTesting;
    }
    CompatibilityState::Unknown
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticCompatibilityRequest {
    family: RuntimeFamily,
    adapter: RuntimeAdapterKind,
    platform: RuntimePlatform,
    process_architecture: ProcessArchitecture,
    required_graphics_api: Option<GraphicsApi>,
    observed_graphics_api: Option<GraphicsApi>,
    supported_platforms: Vec<RuntimePlatform>,
    supported_architectures: Vec<ProcessArchitecture>,
}

impl StaticCompatibilityRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        family: RuntimeFamily,
        adapter: RuntimeAdapterKind,
        platform: RuntimePlatform,
        process_architecture: ProcessArchitecture,
        required_graphics_api: Option<GraphicsApi>,
        observed_graphics_api: Option<GraphicsApi>,
        supported_platforms: Vec<RuntimePlatform>,
        supported_architectures: Vec<ProcessArchitecture>,
    ) -> Result<Self, ContractError> {
        if supported_platforms.len() > 16 || supported_architectures.len() > 16 {
            return Err(ContractError::MetadataTooLarge);
        }
        Ok(Self {
            family,
            adapter,
            platform,
            process_architecture,
            required_graphics_api,
            observed_graphics_api,
            supported_platforms,
            supported_architectures,
        })
    }
}

pub fn assess_static_compatibility(request: &StaticCompatibilityRequest) -> CompatibilityState {
    if request.family == RuntimeFamily::Unknown
        || request.family != request.adapter.family()
        || request.platform == RuntimePlatform::Unknown
        || request.process_architecture == ProcessArchitecture::Unknown
    {
        return CompatibilityState::InsufficientEvidence;
    }
    if !request.supported_platforms.is_empty()
        && !request.supported_platforms.contains(&request.platform)
    {
        return CompatibilityState::IneligibleByStaticConstraints;
    }
    if !request.supported_architectures.is_empty()
        && !request
            .supported_architectures
            .contains(&request.process_architecture)
    {
        return CompatibilityState::IneligibleByStaticConstraints;
    }
    if let Some(required) = request.required_graphics_api {
        if request.observed_graphics_api != Some(required) {
            return CompatibilityState::InsufficientEvidence;
        }
    }
    CompatibilityState::TechnicallyEligibleForTesting
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownGoodRuntimeReference {
    runtime: RuntimeBuildIdentity,
    provider_id: ProviderId,
    target_game_build: GameBuildId,
    profile_id: ProfileId,
    evidence_id: EvidenceId,
    evidence_timestamp_unix_seconds: u64,
    scope: CompatibilityScope,
}

impl KnownGoodRuntimeReference {
    pub fn new(
        identity: &RuntimeIdentity,
        target_game_build: GameBuildId,
        profile_id: ProfileId,
        evidence: &CompatibilityEvidence,
        assessment: &CompatibilityAssessment,
    ) -> Result<Self, ContractError> {
        let timestamp = evidence
            .provenance()
            .map(CompatibilityTestProvenance::observed_at_unix_seconds)
            .ok_or(ContractError::MissingVerificationEvidence)?;
        if identity.integrity_identity().is_none()
            || !identity.source_integrity_matches_identity()
            || evidence.strength() != CompatibilityEvidenceStrength::VerifiedCompatibilityTest
            || evidence.result() != CompatibilityEvidenceResult::Pass
            || !assessment.evidence().contains(evidence)
            || assessment.state() != CompatibilityState::SupportedByVerifiedEvidence
            || evidence.scope().runtime_build() != &identity.build_identity()
            || assessment.scope() != evidence.scope()
            || evidence.scope().game_build() != Some(&target_game_build)
        {
            return Err(ContractError::InvalidCompatibilityEvidence);
        }
        Ok(Self {
            runtime: identity.build_identity(),
            provider_id: identity.provider_id().clone(),
            target_game_build,
            profile_id,
            evidence_id: evidence.id().clone(),
            evidence_timestamp_unix_seconds: timestamp,
            scope: evidence.scope().clone(),
        })
    }

    pub const fn runtime(&self) -> &RuntimeBuildIdentity {
        &self.runtime
    }

    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub const fn target_game_build(&self) -> &GameBuildId {
        &self.target_game_build
    }

    pub const fn profile_id(&self) -> &ProfileId {
        &self.profile_id
    }

    pub const fn evidence_id(&self) -> &EvidenceId {
        &self.evidence_id
    }

    pub const fn evidence_timestamp_unix_seconds(&self) -> u64 {
        self.evidence_timestamp_unix_seconds
    }

    pub const fn scope(&self) -> &CompatibilityScope {
        &self.scope
    }
}
