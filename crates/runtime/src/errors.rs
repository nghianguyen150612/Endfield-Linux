use std::error::Error;
use std::fmt;

use crate::identity::Identifier;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContractError {
    InvalidText {
        field: &'static str,
    },
    InvalidValue {
        field: &'static str,
        reason: &'static str,
    },
    InvalidIdentifier {
        field: &'static str,
    },
    DuplicateValue {
        field: &'static str,
    },
    TooManyValues {
        field: &'static str,
        maximum: usize,
    },
    InvalidDigest,
    DigestAlgorithmMismatch,
    MissingExpectedDigest,
    MissingActualDigest,
    InvalidAdapterCombination,
    MissingVerificationEvidence,
    InvalidVerificationTransition,
    InvalidCompatibilityEvidence,
    InvalidApprovalScope,
    MetadataTooLarge,
    SerializationFailed,
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidText { field } => write!(f, "invalid text for {field}"),
            Self::InvalidValue { field, reason } => write!(f, "invalid {field}: {reason}"),
            Self::InvalidIdentifier { field } => write!(f, "invalid {field} identifier"),
            Self::DuplicateValue { field } => write!(f, "duplicate {field}"),
            Self::TooManyValues { field, maximum } => {
                write!(f, "too many {field}; maximum is {maximum}")
            }
            Self::InvalidDigest => f.write_str("invalid digest encoding"),
            Self::DigestAlgorithmMismatch => f.write_str("digest algorithms do not match"),
            Self::MissingExpectedDigest => f.write_str("expected digest is absent"),
            Self::MissingActualDigest => f.write_str("actual digest is absent"),
            Self::InvalidAdapterCombination => {
                f.write_str("runtime family, ownership, or adapter combination is invalid")
            }
            Self::MissingVerificationEvidence => {
                f.write_str("required verification evidence is absent")
            }
            Self::InvalidVerificationTransition => {
                f.write_str("verification state prerequisites are not satisfied")
            }
            Self::InvalidCompatibilityEvidence => {
                f.write_str("compatibility evidence is incomplete or out of scope")
            }
            Self::InvalidApprovalScope => {
                f.write_str("user approval does not match the requested action and resource")
            }
            Self::MetadataTooLarge => f.write_str("runtime metadata exceeds its size bound"),
            Self::SerializationFailed => f.write_str("dry-run serialization failed"),
        }
    }
}

impl Error for ContractError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderErrorKind {
    Unsupported,
    Unknown,
    NotImplemented,
    ProviderUnavailable,
    PermissionDenied,
    MissingMetadata,
    InvalidMetadata,
    VerificationFailed,
    IncompatibleRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderOperation {
    Discover,
    InspectIdentity,
    VerifySource,
    VerifyIntegrity,
    AssessCompatibility,
    PlanInstallation,
    PrepareLaunchPlan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderError {
    provider_id: Identifier,
    operation: ProviderOperation,
    kind: ProviderErrorKind,
}

impl ProviderError {
    pub const fn new(
        provider_id: Identifier,
        operation: ProviderOperation,
        kind: ProviderErrorKind,
    ) -> Self {
        Self {
            provider_id,
            operation,
            kind,
        }
    }

    pub const fn provider_id(&self) -> &Identifier {
        &self.provider_id
    }

    pub const fn operation(&self) -> ProviderOperation {
        self.operation
    }

    pub const fn kind(&self) -> ProviderErrorKind {
        self.kind
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "provider {} operation {:?} failed ({:?})",
            self.provider_id, self.operation, self.kind
        )
    }
}

impl Error for ProviderError {}
