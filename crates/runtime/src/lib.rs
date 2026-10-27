//! Portable runtime metadata and provider contracts for endfield-unix.
//!
//! This crate does not discover, download, install, verify files, benchmark,
//! execute a launch plan, or change the filesystem. Provider implementations
//! and runtime lifecycle operations belong to later milestones.

#![forbid(unsafe_code)]

pub mod adapter;
pub mod capabilities;
pub mod compatibility;
pub mod environment;
pub mod errors;
pub mod identity;
pub mod integrity;
pub mod launch_plan;
pub mod policy;
pub mod provider;
pub mod source;
pub mod validation;

pub use adapter::{RuntimeAdapterKind, RuntimeAdapterSelection};
pub use capabilities::{
    CapabilityStatus, NegotiationContext, NegotiationResult, ProviderAvailability,
    ProviderCapability, ProviderCapabilityKind, RequestedOperation, RuntimePresence,
    negotiate_capability,
};
pub use compatibility::{
    CompatibilityAssessment, CompatibilityEvidence, CompatibilityEvidenceResult,
    CompatibilityEvidenceStrength, CompatibilityScope, CompatibilityState,
    CompatibilityTestProvenance, GraphicsApi, KnownGoodRuntimeReference,
    StaticCompatibilityRequest, assess_static_compatibility,
};
pub use environment::{EnvironmentChange, EnvironmentName, SensitiveValueKind, ValueSensitivity};
pub use errors::{ContractError, ProviderError, ProviderErrorKind, ProviderOperation};
pub use identity::{
    ArtifactId, EvidenceId, GameBuildId, Identifier, ManifestId, ProcessArchitecture, ProfileId,
    ProjectId, ProviderId, RuntimeBuildIdentity, RuntimeFamily, RuntimeId, RuntimeIdentity,
    RuntimeLineage, RuntimeOwnership, RuntimePlatform, RuntimeRequirements, RuntimeVersion,
};
pub use integrity::{
    ArtifactIntegrityVerification, Digest, DigestAlgorithm, DigestAuthority, ExpectedDigest,
    IntegrityAssessment, IntegrityOutcome, IntegrityRecord,
};
pub use launch_plan::{
    CURRENT_LAUNCH_PLAN_SCHEMA_VERSION, Executable, ExecutablePathPolicy, LaunchArgument,
    LaunchPlan, LaunchPlanReview, LaunchReadiness, MountAccess, MountRequirement,
    OutputCapturePolicy, PathClassification, PathOwnership, PlanAuthorization, PlanRisk,
    PrefixOwnership, PrefixReference, ProcessOwnershipPolicy, StructuralValidationState,
    WorkingDirectory,
};
pub use policy::{
    ManagementAction, ManagementDecision, ManagementTarget, RuntimeManagementPolicy, UserAction,
    UserIntent,
};
pub use provider::{
    DiscoveryCompleteness, DiscoveryQuery, DiscoveryReport, InstallationPlan,
    InstallationPlanRequest, IntegrityVerificationRequest, LaunchPlanRequest, ProviderDeclaration,
    RuntimeCandidate, RuntimeProvider, SourceVerificationRequest,
};
pub use source::{
    AuthenticityProof, AuthenticityState, MetadataVerification, SourceCategory, SourceIdentity,
    SourceMetadata, SourceVerification, SourceVerificationState, VerificationMethod,
    VerificationTimestamp,
};
