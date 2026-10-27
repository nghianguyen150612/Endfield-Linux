#![allow(dead_code)]

use endfield_unix_runtime::{
    ArtifactId, ArtifactIntegrityVerification, AuthenticityProof, AuthenticityState,
    CompatibilityAssessment, CompatibilityEvidence, CompatibilityEvidenceResult,
    CompatibilityEvidenceStrength, CompatibilityScope, CompatibilityTestProvenance, Digest,
    DigestAlgorithm, DigestAuthority, EvidenceId, GameBuildId, Identifier, IntegrityRecord,
    ManifestId, MetadataVerification, ProcessArchitecture, ProfileId, ProjectId, ProviderId,
    RuntimeAdapterKind, RuntimeAdapterSelection, RuntimeFamily, RuntimeIdentity, RuntimeLineage,
    RuntimeOwnership, RuntimePlatform, RuntimeRequirements, RuntimeVersion, SourceCategory,
    SourceIdentity, SourceMetadata, SourceVerification, SourceVerificationState,
    VerificationMethod, VerificationTimestamp,
};

pub fn sha256(fill: char) -> Digest {
    Digest::new(DigestAlgorithm::Sha256, fill.to_string().repeat(64)).expect("valid fixture digest")
}

pub fn verified_source(digest: &Digest) -> SourceMetadata {
    let manifest = ManifestId::new("fixture-manifest").expect("fixture id");
    let expected = endfield_unix_runtime::ExpectedDigest::new(
        digest.clone(),
        DigestAuthority::AuthenticatedManifest(manifest),
    );
    let record = IntegrityRecord::new(
        ArtifactId::new("fixture-artifact").expect("fixture id"),
        Some(expected),
        Some(digest.clone()),
    );
    let integrity = ArtifactIntegrityVerification::new(
        EvidenceId::new("fixture-integrity-evidence").expect("fixture id"),
        record.assess(),
        Some(1_780_000_000),
    )
    .expect("fixture digest matches");
    let authenticity = AuthenticityProof::new(
        VerificationMethod::AuthenticatedManifest,
        EvidenceId::new("fixture-source-evidence").expect("fixture id"),
        Some(VerificationTimestamp::new(1_780_000_000)),
    )
    .expect("fixture authenticity proof");
    let verification = SourceVerification::new(
        SourceVerificationState::ArtifactIntegrityVerified,
        AuthenticityState::Established(authenticity),
        MetadataVerification::Verified {
            evidence_id: EvidenceId::new("fixture-metadata-evidence").expect("fixture id"),
            verified_at: Some(VerificationTimestamp::new(1_780_000_000)),
        },
        Some(integrity),
        None,
        None,
    )
    .expect("valid source verification lifecycle");
    let identity = SourceIdentity::new(
        ProviderId::new("fixture-provider").expect("fixture provider"),
        SourceCategory::VerifiedProjectRelease,
        Some(ProjectId::new("fixture-project").expect("fixture project")),
        Some(Identifier::new("fixture-release").expect("fixture release")),
        Some("https://example.invalid/fixture".to_owned()),
    )
    .expect("source claim");
    SourceMetadata::new(identity, verification).expect("source metadata")
}

pub fn runtime_identity(
    family: RuntimeFamily,
    lineage: RuntimeLineage,
    provider: &str,
    version: &str,
    ownership: RuntimeOwnership,
    integrity: Option<Digest>,
    source: Option<SourceMetadata>,
) -> RuntimeIdentity {
    RuntimeIdentity::new(
        family,
        lineage,
        ProviderId::new(provider).expect("fixture provider"),
        RuntimeVersion::new(version).expect("fixture version"),
        Some("synthetic-test-channel".to_owned()),
        RuntimeRequirements::new(
            [RuntimePlatform::Linux],
            [ProcessArchitecture::X86_64, ProcessArchitecture::X86],
        )
        .expect("fixture requirements"),
        source,
        integrity,
        ownership,
    )
    .expect("fixture runtime identity")
}

pub fn compatibility_scope(
    runtime: &RuntimeIdentity,
    adapter: RuntimeAdapterKind,
    game_build: Option<GameBuildId>,
) -> CompatibilityScope {
    CompatibilityScope::new(
        runtime.build_identity(),
        adapter,
        RuntimePlatform::Linux,
        ProcessArchitecture::X86_64,
        Some(ProcessArchitecture::X86_64),
        Some(endfield_unix_runtime::GraphicsApi::Vulkan),
        vec![Identifier::new("driver-family-fixture").expect("fixture id")],
        game_build,
        Some(Identifier::new("prefix-config-fixture").expect("fixture id")),
        vec![],
    )
    .expect("fixture compatibility scope")
}

pub fn verified_compatibility_evidence(
    runtime: &RuntimeIdentity,
    adapter: RuntimeAdapterKind,
    evidence_id: &str,
    game_build: Option<GameBuildId>,
) -> CompatibilityEvidence {
    let scope = compatibility_scope(runtime, adapter, game_build);
    let provenance = CompatibilityTestProvenance::new(
        Identifier::new(format!("test-{evidence_id}")).expect("test id"),
        RuntimePlatform::Linux,
        ProcessArchitecture::X86_64,
        1_780_000_000,
        Identifier::new("synthetic-method").expect("method id"),
        "Synthetic contract fixture; no game or runtime was executed.",
    )
    .expect("fixture provenance");
    CompatibilityEvidence::new(
        EvidenceId::new(evidence_id).expect("evidence id"),
        scope,
        CompatibilityEvidenceStrength::VerifiedCompatibilityTest,
        CompatibilityEvidenceResult::Pass,
        None,
        Some(provenance),
    )
    .expect("verified test evidence model")
}

pub fn compatibility_assessment(evidence: Vec<CompatibilityEvidence>) -> CompatibilityAssessment {
    let scope = evidence
        .first()
        .map(|item| item.scope().clone())
        .expect("assessment fixture needs evidence");
    CompatibilityAssessment::from_evidence(scope, evidence).expect("fixture assessment")
}

pub fn wine_identity() -> RuntimeIdentity {
    let digest = sha256('a');
    runtime_identity(
        RuntimeFamily::Wine,
        RuntimeLineage::UpstreamWine,
        "fixture-wine-provider",
        "wine-10.0",
        RuntimeOwnership::EndfieldManaged,
        Some(digest.clone()),
        Some(verified_source(&digest)),
    )
}

pub fn managed_wine_adapter(runtime: &RuntimeIdentity) -> RuntimeAdapterSelection {
    RuntimeAdapterSelection::new(RuntimeAdapterKind::ManagedWine, runtime, None)
        .expect("managed wine adapter")
}

pub fn fixture_game_build() -> GameBuildId {
    GameBuildId::new("synthetic-game-build").expect("fixture game build")
}

pub fn fixture_profile() -> ProfileId {
    ProfileId::new("fixture-profile").expect("fixture profile")
}
