use crate::errors::ContractError;
use crate::identity::{EvidenceId, Identifier, ProjectId, ProviderId};
use crate::integrity::{
    ArtifactIntegrityVerification, DigestAuthority, IntegrityOutcome, validate_manifest_reference,
};
use crate::validation::validate_text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceCategory {
    OfficialUpstreamRelease,
    VerifiedProjectRelease,
    VerifiedDistributionPackage,
    UserSuppliedArtifact,
    ExternalInstallation,
    UnknownOrigin,
}

impl SourceCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OfficialUpstreamRelease => "official-upstream-release",
            Self::VerifiedProjectRelease => "verified-project-release",
            Self::VerifiedDistributionPackage => "verified-distribution-package",
            Self::UserSuppliedArtifact => "user-supplied-artifact",
            Self::ExternalInstallation => "external-installation",
            Self::UnknownOrigin => "unknown-origin",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceIdentity {
    provider_id: ProviderId,
    category: SourceCategory,
    project_id: Option<ProjectId>,
    release_or_build_id: Option<Identifier>,
    claimed_address: Option<String>,
}

impl SourceIdentity {
    pub fn new(
        provider_id: ProviderId,
        category: SourceCategory,
        project_id: Option<ProjectId>,
        release_or_build_id: Option<Identifier>,
        claimed_address: Option<String>,
    ) -> Result<Self, ContractError> {
        if let Some(address) = &claimed_address {
            validate_text(address, "source address", 2048)?;
        }
        if category == SourceCategory::OfficialUpstreamRelease && project_id.is_none() {
            return Err(ContractError::InvalidValue {
                field: "source identity",
                reason: "an official source claim requires a project identity",
            });
        }
        Ok(Self {
            provider_id,
            category,
            project_id,
            release_or_build_id,
            claimed_address,
        })
    }

    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub const fn category(&self) -> SourceCategory {
        self.category
    }

    pub const fn project_id(&self) -> Option<&ProjectId> {
        self.project_id.as_ref()
    }

    pub const fn release_or_build_id(&self) -> Option<&Identifier> {
        self.release_or_build_id.as_ref()
    }

    pub fn claimed_address(&self) -> Option<&str> {
        self.claimed_address.as_deref()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerificationTimestamp(u64);

impl VerificationTimestamp {
    pub const fn new(unix_seconds: u64) -> Self {
        Self(unix_seconds)
    }

    pub const fn unix_seconds(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerificationMethod {
    DetachedSignature,
    AuthenticatedManifest,
    DistributionSignature,
    LocalMetadata,
    UserAssertion,
    Unsupported(Identifier),
}

impl VerificationMethod {
    pub const fn establishes_authenticity(&self) -> bool {
        matches!(
            self,
            Self::DetachedSignature | Self::AuthenticatedManifest | Self::DistributionSignature
        )
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::DetachedSignature => "detached-signature",
            Self::AuthenticatedManifest => "authenticated-manifest",
            Self::DistributionSignature => "distribution-signature",
            Self::LocalMetadata => "local-metadata",
            Self::UserAssertion => "user-assertion",
            Self::Unsupported(_) => "unsupported",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticityProof {
    method: VerificationMethod,
    evidence_id: EvidenceId,
    verified_at: Option<VerificationTimestamp>,
}

impl AuthenticityProof {
    pub fn new(
        method: VerificationMethod,
        evidence_id: EvidenceId,
        verified_at: Option<VerificationTimestamp>,
    ) -> Result<Self, ContractError> {
        if !method.establishes_authenticity() {
            return Err(ContractError::MissingVerificationEvidence);
        }
        Ok(Self {
            method,
            evidence_id,
            verified_at,
        })
    }

    pub const fn method(&self) -> &VerificationMethod {
        &self.method
    }

    pub const fn evidence_id(&self) -> &EvidenceId {
        &self.evidence_id
    }

    pub const fn verified_at(&self) -> Option<VerificationTimestamp> {
        self.verified_at
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthenticityState {
    NotAssessed,
    Claimed(VerificationMethod),
    Established(AuthenticityProof),
}

impl AuthenticityState {
    pub const fn is_established(&self) -> bool {
        matches!(self, Self::Established(_))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataVerification {
    NotVerified,
    Verified {
        evidence_id: EvidenceId,
        verified_at: Option<VerificationTimestamp>,
    },
}

impl MetadataVerification {
    pub const fn is_verified(&self) -> bool {
        matches!(self, Self::Verified { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceVerificationState {
    Unverified,
    CandidateIdentified,
    SourceIdentityEstablished,
    MetadataVerified,
    ArtifactIntegrityVerified,
    VerificationFailed,
    VerificationUnsupported,
}

impl SourceVerificationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unverified => "unverified",
            Self::CandidateIdentified => "candidate-identified",
            Self::SourceIdentityEstablished => "source-identity-established",
            Self::MetadataVerified => "metadata-verified",
            Self::ArtifactIntegrityVerified => "artifact-integrity-verified",
            Self::VerificationFailed => "verification-failed",
            Self::VerificationUnsupported => "verification-unsupported",
        }
    }

    pub const fn has_verified_artifact(self) -> bool {
        matches!(self, Self::ArtifactIntegrityVerified)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceVerification {
    state: SourceVerificationState,
    authenticity: AuthenticityState,
    metadata: MetadataVerification,
    artifact_integrity: Option<ArtifactIntegrityVerification>,
    failure_reason: Option<String>,
    unsupported_method: Option<Identifier>,
}

impl SourceVerification {
    pub fn new(
        state: SourceVerificationState,
        authenticity: AuthenticityState,
        metadata: MetadataVerification,
        artifact_integrity: Option<ArtifactIntegrityVerification>,
        failure_reason: Option<String>,
        unsupported_method: Option<Identifier>,
    ) -> Result<Self, ContractError> {
        if let Some(reason) = &failure_reason {
            validate_text(reason, "verification failure", 512)?;
        }
        if failure_reason.is_some() != (state == SourceVerificationState::VerificationFailed)
            || unsupported_method.is_some()
                != (state == SourceVerificationState::VerificationUnsupported)
        {
            return Err(ContractError::InvalidVerificationTransition);
        }
        let prerequisites_hold = match state {
            SourceVerificationState::Unverified => {
                !authenticity.is_established()
                    && !metadata.is_verified()
                    && artifact_integrity.is_none()
            }
            SourceVerificationState::CandidateIdentified => {
                !authenticity.is_established()
                    && !metadata.is_verified()
                    && artifact_integrity.is_none()
            }
            SourceVerificationState::SourceIdentityEstablished => {
                authenticity.is_established()
                    && !metadata.is_verified()
                    && artifact_integrity.is_none()
            }
            SourceVerificationState::MetadataVerified => {
                authenticity.is_established()
                    && metadata.is_verified()
                    && artifact_integrity.is_none()
            }
            SourceVerificationState::ArtifactIntegrityVerified => {
                authenticity.is_established()
                    && metadata.is_verified()
                    && artifact_integrity.as_ref().is_some_and(|integrity| {
                        integrity.assessment().outcome() == IntegrityOutcome::DigestMatches
                            && integrity.is_from_authenticated_metadata()
                    })
            }
            SourceVerificationState::VerificationFailed => true,
            SourceVerificationState::VerificationUnsupported => true,
        };
        if !prerequisites_hold {
            return Err(ContractError::InvalidVerificationTransition);
        }
        Ok(Self {
            state,
            authenticity,
            metadata,
            artifact_integrity,
            failure_reason,
            unsupported_method,
        })
    }

    pub fn unverified() -> Self {
        Self {
            state: SourceVerificationState::Unverified,
            authenticity: AuthenticityState::NotAssessed,
            metadata: MetadataVerification::NotVerified,
            artifact_integrity: None,
            failure_reason: None,
            unsupported_method: None,
        }
    }

    pub const fn state(&self) -> SourceVerificationState {
        self.state
    }

    pub const fn authenticity(&self) -> &AuthenticityState {
        &self.authenticity
    }

    pub const fn metadata(&self) -> &MetadataVerification {
        &self.metadata
    }

    pub const fn artifact_integrity(&self) -> Option<&ArtifactIntegrityVerification> {
        self.artifact_integrity.as_ref()
    }

    pub fn failure_reason(&self) -> Option<&str> {
        self.failure_reason.as_deref()
    }

    pub const fn unsupported_method(&self) -> Option<&Identifier> {
        self.unsupported_method.as_ref()
    }

    pub const fn has_authenticated_source_identity(&self) -> bool {
        self.authenticity.is_established()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceMetadata {
    identity: SourceIdentity,
    verification: SourceVerification,
}

impl SourceMetadata {
    pub fn new(
        identity: SourceIdentity,
        verification: SourceVerification,
    ) -> Result<Self, ContractError> {
        if identity.category() == SourceCategory::UnknownOrigin
            && (verification.authenticity().is_established()
                || matches!(
                    verification.state(),
                    SourceVerificationState::SourceIdentityEstablished
                        | SourceVerificationState::MetadataVerified
                        | SourceVerificationState::ArtifactIntegrityVerified
                ))
        {
            return Err(ContractError::InvalidVerificationTransition);
        }
        if let Some(integrity) = verification.artifact_integrity() {
            if let Some(expected) = integrity.assessment().expected() {
                if let DigestAuthority::AuthenticatedManifest(manifest) = expected.authority() {
                    validate_manifest_reference(manifest.as_str())?;
                }
            }
        }
        Ok(Self {
            identity,
            verification,
        })
    }

    pub const fn identity(&self) -> &SourceIdentity {
        &self.identity
    }

    pub const fn verification(&self) -> &SourceVerification {
        &self.verification
    }

    pub const fn category(&self) -> SourceCategory {
        self.identity.category()
    }
}
