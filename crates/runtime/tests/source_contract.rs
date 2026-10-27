mod common;

use endfield_unix_runtime::{
    ArtifactId, ArtifactIntegrityVerification, AuthenticityProof, AuthenticityState, ContractError,
    DigestAuthority, EvidenceId, IntegrityRecord, ManifestId, MetadataVerification, ProjectId,
    ProviderId, SourceCategory, SourceIdentity, SourceMetadata, SourceVerification,
    SourceVerificationState, VerificationMethod, VerificationTimestamp,
};

#[test]
fn unknown_origin_stays_unverified() {
    let source = SourceMetadata::new(
        SourceIdentity::new(
            ProviderId::new("unknown-provider").expect("provider"),
            SourceCategory::UnknownOrigin,
            None,
            None,
            None,
        )
        .expect("unknown origin"),
        SourceVerification::unverified(),
    )
    .expect("unverified unknown source");
    assert_eq!(
        source.verification().state(),
        SourceVerificationState::Unverified
    );
    assert!(!source.verification().has_authenticated_source_identity());
}

#[test]
fn candidate_and_claimed_url_do_not_establish_authenticity() {
    let verification = SourceVerification::new(
        SourceVerificationState::CandidateIdentified,
        AuthenticityState::Claimed(VerificationMethod::UserAssertion),
        MetadataVerification::NotVerified,
        None,
        None,
        None,
    )
    .expect("candidate state");
    let source = SourceMetadata::new(
        SourceIdentity::new(
            ProviderId::new("claimed-provider").expect("provider"),
            SourceCategory::OfficialUpstreamRelease,
            Some(ProjectId::new("claimed-project").expect("project")),
            Some(endfield_unix_runtime::Identifier::new("v10").expect("release")),
            Some("https://github.com/example/wine/releases/tag/v10".to_owned()),
        )
        .expect("source claim"),
        verification,
    )
    .expect("candidate metadata");
    assert_eq!(
        source.verification().state(),
        SourceVerificationState::CandidateIdentified
    );
    assert!(!source.verification().has_authenticated_source_identity());
}

#[test]
fn release_tag_and_valid_hash_from_untrusted_manifest_are_not_trusted_origin() {
    let digest = common::sha256('d');
    let expected = endfield_unix_runtime::ExpectedDigest::new(
        digest.clone(),
        DigestAuthority::UntrustedSource,
    );
    let assessment = IntegrityRecord::new(
        ArtifactId::new("artifact").expect("artifact"),
        Some(expected),
        Some(digest),
    )
    .assess();
    assert_eq!(
        assessment.outcome(),
        endfield_unix_runtime::IntegrityOutcome::DigestMatches
    );
    assert!(!assessment.matched_trusted_metadata());

    let integrity = ArtifactIntegrityVerification::new(
        EvidenceId::new("integrity-evidence").expect("evidence"),
        assessment,
        None,
    )
    .expect("a matching value can be represented");
    let authenticity = AuthenticityProof::new(
        VerificationMethod::AuthenticatedManifest,
        EvidenceId::new("claimed-manifest-proof").expect("evidence"),
        None,
    )
    .expect("synthetic authentication evidence");
    let attempted = SourceVerification::new(
        SourceVerificationState::ArtifactIntegrityVerified,
        AuthenticityState::Established(authenticity),
        MetadataVerification::Verified {
            evidence_id: EvidenceId::new("metadata-proof").expect("evidence"),
            verified_at: None,
        },
        Some(integrity),
        None,
        None,
    );
    assert!(matches!(
        attempted,
        Err(ContractError::InvalidVerificationTransition)
    ));
}

#[test]
fn authenticated_synthetic_manifest_can_represent_each_verified_stage() {
    let source = common::verified_source(&common::sha256('e'));
    assert_eq!(
        source.verification().state(),
        SourceVerificationState::ArtifactIntegrityVerified
    );
    assert!(source.verification().has_authenticated_source_identity());
    assert!(
        source
            .verification()
            .artifact_integrity()
            .expect("integrity evidence")
            .is_from_authenticated_metadata()
    );
}

#[test]
fn malformed_and_oversized_source_metadata_are_rejected() {
    assert!(matches!(
        ProviderId::new("bad/provider"),
        Err(ContractError::InvalidIdentifier { .. })
    ));
    assert!(matches!(
        SourceIdentity::new(
            ProviderId::new("provider").expect("provider"),
            SourceCategory::ExternalInstallation,
            None,
            None,
            Some("x".repeat(2049)),
        ),
        Err(ContractError::InvalidText { .. })
    ));
}

#[test]
fn conflicting_provenance_is_recorded_as_verification_failure() {
    let failed = SourceVerification::new(
        SourceVerificationState::VerificationFailed,
        AuthenticityState::Claimed(VerificationMethod::UserAssertion),
        MetadataVerification::NotVerified,
        None,
        Some("conflicting source claims".to_owned()),
        None,
    )
    .expect("failure state");
    assert_eq!(failed.state(), SourceVerificationState::VerificationFailed);
    assert_eq!(failed.failure_reason(), Some("conflicting source claims"));
}

#[test]
fn unsupported_verification_is_distinct_from_failed_verification() {
    let unsupported = SourceVerification::new(
        SourceVerificationState::VerificationUnsupported,
        AuthenticityState::NotAssessed,
        MetadataVerification::NotVerified,
        None,
        None,
        Some(endfield_unix_runtime::Identifier::new("pinned-signature-v9").expect("method")),
    )
    .expect("unsupported verifier");
    assert_eq!(
        unsupported.state(),
        SourceVerificationState::VerificationUnsupported
    );
    assert_eq!(
        VerificationMethod::Unsupported(
            endfield_unix_runtime::Identifier::new("future-method").expect("method")
        )
        .as_str(),
        "unsupported"
    );
}

#[test]
fn authenticity_proof_rejects_a_user_assertion() {
    assert!(matches!(
        AuthenticityProof::new(
            VerificationMethod::UserAssertion,
            EvidenceId::new("assertion").expect("id"),
            Some(VerificationTimestamp::new(1_780_000_000)),
        ),
        Err(ContractError::MissingVerificationEvidence)
    ));
}

#[test]
fn manifest_authority_keeps_a_typed_manifest_reference() {
    let expected = endfield_unix_runtime::ExpectedDigest::new(
        common::sha256('f'),
        DigestAuthority::AuthenticatedManifest(
            ManifestId::new("manifest-fixture").expect("manifest"),
        ),
    );
    assert!(matches!(
        expected.authority(),
        DigestAuthority::AuthenticatedManifest(_)
    ));
}
