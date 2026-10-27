use crate::errors::ContractError;
use crate::identity::{ArtifactId, EvidenceId, Identifier, ManifestId};
use crate::validation::validate_text;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DigestAlgorithm {
    Sha256,
    Unsupported(Identifier),
}

impl DigestAlgorithm {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Sha256 => "sha256",
            Self::Unsupported(identifier) => identifier.as_str(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest {
    algorithm: DigestAlgorithm,
    hexadecimal: String,
}

impl Digest {
    pub fn new(
        algorithm: DigestAlgorithm,
        hexadecimal: impl Into<String>,
    ) -> Result<Self, ContractError> {
        let hexadecimal = hexadecimal.into();
        if !hexadecimal.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ContractError::InvalidDigest);
        }
        match algorithm {
            DigestAlgorithm::Sha256 if hexadecimal.len() != 64 => {
                return Err(ContractError::InvalidDigest);
            }
            DigestAlgorithm::Unsupported(_)
                if hexadecimal.is_empty() || hexadecimal.len() > 128 =>
            {
                return Err(ContractError::InvalidDigest);
            }
            _ => {}
        }
        Ok(Self {
            algorithm,
            hexadecimal: hexadecimal.to_ascii_lowercase(),
        })
    }

    pub const fn algorithm(&self) -> &DigestAlgorithm {
        &self.algorithm
    }

    pub fn hexadecimal(&self) -> &str {
        &self.hexadecimal
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DigestAuthority {
    AuthenticatedManifest(ManifestId),
    VerifiedDistributionMetadata(EvidenceId),
    UserSupplied,
    UntrustedSource,
}

impl DigestAuthority {
    pub const fn has_authenticated_origin(&self) -> bool {
        matches!(
            self,
            Self::AuthenticatedManifest(_) | Self::VerifiedDistributionMetadata(_)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedDigest {
    digest: Digest,
    authority: DigestAuthority,
}

impl ExpectedDigest {
    pub const fn new(digest: Digest, authority: DigestAuthority) -> Self {
        Self { digest, authority }
    }

    pub const fn digest(&self) -> &Digest {
        &self.digest
    }

    pub const fn authority(&self) -> &DigestAuthority {
        &self.authority
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegrityOutcome {
    DigestMatches,
    DigestMismatch,
    MissingExpectedDigest,
    MissingActualDigest,
    AlgorithmMismatch,
    UnsupportedAlgorithm,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrityAssessment {
    artifact_id: ArtifactId,
    outcome: IntegrityOutcome,
    expected: Option<ExpectedDigest>,
    actual: Option<Digest>,
}

impl IntegrityAssessment {
    pub const fn artifact_id(&self) -> &ArtifactId {
        &self.artifact_id
    }

    pub const fn outcome(&self) -> IntegrityOutcome {
        self.outcome
    }

    pub const fn expected(&self) -> Option<&ExpectedDigest> {
        self.expected.as_ref()
    }

    pub const fn actual(&self) -> Option<&Digest> {
        self.actual.as_ref()
    }

    pub const fn matched_trusted_metadata(&self) -> bool {
        matches!(self.outcome, IntegrityOutcome::DigestMatches)
            && matches!(
                self.expected,
                Some(ExpectedDigest {
                    authority: DigestAuthority::AuthenticatedManifest(_)
                        | DigestAuthority::VerifiedDistributionMetadata(_),
                    ..
                })
            )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrityRecord {
    artifact_id: ArtifactId,
    expected: Option<ExpectedDigest>,
    actual: Option<Digest>,
}

impl IntegrityRecord {
    pub fn new(
        artifact_id: ArtifactId,
        expected: Option<ExpectedDigest>,
        actual: Option<Digest>,
    ) -> Self {
        Self {
            artifact_id,
            expected,
            actual,
        }
    }

    /// Compares already supplied synthetic or provider-produced metadata.
    /// This method does not hash files or verify the authority of a manifest.
    pub fn assess(&self) -> IntegrityAssessment {
        let outcome = match (&self.expected, &self.actual) {
            (None, _) => IntegrityOutcome::MissingExpectedDigest,
            (Some(_), None) => IntegrityOutcome::MissingActualDigest,
            (Some(expected), Some(actual)) if expected.digest.algorithm != actual.algorithm => {
                IntegrityOutcome::AlgorithmMismatch
            }
            (
                Some(ExpectedDigest {
                    digest:
                        Digest {
                            algorithm: DigestAlgorithm::Unsupported(_),
                            ..
                        },
                    ..
                }),
                Some(_),
            ) => IntegrityOutcome::UnsupportedAlgorithm,
            (Some(expected), Some(actual)) if expected.digest.hexadecimal == actual.hexadecimal => {
                IntegrityOutcome::DigestMatches
            }
            (Some(_), Some(_)) => IntegrityOutcome::DigestMismatch,
        };
        IntegrityAssessment {
            artifact_id: self.artifact_id.clone(),
            outcome,
            expected: self.expected.clone(),
            actual: self.actual.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactIntegrityVerification {
    evidence_id: EvidenceId,
    assessment: IntegrityAssessment,
    verified_at_unix_seconds: Option<u64>,
}

impl ArtifactIntegrityVerification {
    pub fn new(
        evidence_id: EvidenceId,
        assessment: IntegrityAssessment,
        verified_at_unix_seconds: Option<u64>,
    ) -> Result<Self, ContractError> {
        if let Some(timestamp) = verified_at_unix_seconds {
            if timestamp == u64::MAX {
                return Err(ContractError::InvalidValue {
                    field: "verification timestamp",
                    reason: "timestamp exceeds the supported range",
                });
            }
        }
        if assessment.outcome() != IntegrityOutcome::DigestMatches {
            return Err(ContractError::InvalidVerificationTransition);
        }
        Ok(Self {
            evidence_id,
            assessment,
            verified_at_unix_seconds,
        })
    }

    pub const fn evidence_id(&self) -> &EvidenceId {
        &self.evidence_id
    }

    pub const fn assessment(&self) -> &IntegrityAssessment {
        &self.assessment
    }

    pub const fn verified_at_unix_seconds(&self) -> Option<u64> {
        self.verified_at_unix_seconds
    }

    pub const fn is_from_authenticated_metadata(&self) -> bool {
        self.assessment.matched_trusted_metadata()
    }
}

pub(crate) fn validate_manifest_reference(value: &str) -> Result<(), ContractError> {
    validate_text(value, "manifest reference", 256)
}
