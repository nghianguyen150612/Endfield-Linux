use endfield_unix_runtime::{
    ArtifactId, ContractError, Digest, DigestAlgorithm, DigestAuthority, EvidenceId,
    ExpectedDigest, IntegrityOutcome, IntegrityRecord, ManifestId,
};

fn digest(value: char) -> Digest {
    Digest::new(DigestAlgorithm::Sha256, value.to_string().repeat(64)).expect("valid sha256")
}

#[test]
fn accepts_sha256_and_normalizes_hexadecimal_case() {
    let parsed = Digest::new(DigestAlgorithm::Sha256, "A".repeat(64)).expect("valid digest");
    assert_eq!(parsed.hexadecimal(), "a".repeat(64));
}

#[test]
fn rejects_invalid_and_truncated_sha256_encodings() {
    assert!(matches!(
        Digest::new(DigestAlgorithm::Sha256, "z".repeat(64)),
        Err(ContractError::InvalidDigest)
    ));
    assert!(matches!(
        Digest::new(DigestAlgorithm::Sha256, "a".repeat(63)),
        Err(ContractError::InvalidDigest)
    ));
    assert!(matches!(
        Digest::new(DigestAlgorithm::Sha256, "a".repeat(65)),
        Err(ContractError::InvalidDigest)
    ));
}

#[test]
fn compares_matching_and_mismatched_digest_values() {
    let artifact = ArtifactId::new("runtime-archive").expect("artifact id");
    let expected = ExpectedDigest::new(
        digest('a'),
        DigestAuthority::AuthenticatedManifest(
            ManifestId::new("fixture-manifest").expect("manifest id"),
        ),
    );
    let match_result =
        IntegrityRecord::new(artifact.clone(), Some(expected.clone()), Some(digest('a'))).assess();
    let mismatch = IntegrityRecord::new(artifact, Some(expected), Some(digest('b'))).assess();

    assert_eq!(match_result.outcome(), IntegrityOutcome::DigestMatches);
    assert!(match_result.matched_trusted_metadata());
    assert_eq!(mismatch.outcome(), IntegrityOutcome::DigestMismatch);
}

#[test]
fn distinguishes_absent_expected_and_actual_digests() {
    let artifact = ArtifactId::new("runtime").expect("artifact id");
    let missing_expected = IntegrityRecord::new(artifact.clone(), None, Some(digest('a'))).assess();
    let missing_actual = IntegrityRecord::new(
        artifact,
        Some(ExpectedDigest::new(
            digest('a'),
            DigestAuthority::UserSupplied,
        )),
        None,
    )
    .assess();
    assert_eq!(
        missing_expected.outcome(),
        IntegrityOutcome::MissingExpectedDigest
    );
    assert_eq!(
        missing_actual.outcome(),
        IntegrityOutcome::MissingActualDigest
    );
}

#[test]
fn rejects_mismatched_digest_algorithm_identifiers_during_comparison() {
    let unsupported = Digest::new(
        DigestAlgorithm::Unsupported(
            endfield_unix_runtime::Identifier::new("blake3").expect("algorithm id"),
        ),
        "a".repeat(32),
    )
    .expect("bounded unsupported digest representation");
    let result = IntegrityRecord::new(
        ArtifactId::new("runtime").expect("artifact"),
        Some(ExpectedDigest::new(
            digest('a'),
            DigestAuthority::UntrustedSource,
        )),
        Some(unsupported),
    )
    .assess();
    assert_eq!(result.outcome(), IntegrityOutcome::AlgorithmMismatch);
}

#[test]
fn unsupported_digest_algorithm_cannot_be_marked_as_a_match() {
    let algorithm = DigestAlgorithm::Unsupported(
        endfield_unix_runtime::Identifier::new("sha3-256").expect("algorithm id"),
    );
    let expected_digest = Digest::new(algorithm.clone(), "a".repeat(64)).expect("digest");
    let actual_digest = Digest::new(algorithm, "a".repeat(64)).expect("digest");
    let record = IntegrityRecord::new(
        ArtifactId::new("artifact").expect("artifact id"),
        Some(ExpectedDigest::new(
            expected_digest,
            DigestAuthority::AuthenticatedManifest(
                ManifestId::new("manifest").expect("manifest id"),
            ),
        )),
        Some(actual_digest),
    );

    let assessment = record.assess();
    assert_eq!(assessment.outcome(), IntegrityOutcome::UnsupportedAlgorithm);
    assert!(matches!(
        endfield_unix_runtime::ArtifactIntegrityVerification::new(
            EvidenceId::new("integrity-evidence").expect("evidence id"),
            assessment,
            None,
        ),
        Err(ContractError::InvalidVerificationTransition)
    ));
}

#[test]
fn a_matching_user_hash_does_not_acquire_authority() {
    let result = IntegrityRecord::new(
        ArtifactId::new("runtime").expect("artifact"),
        Some(ExpectedDigest::new(
            digest('a'),
            DigestAuthority::UserSupplied,
        )),
        Some(digest('a')),
    )
    .assess();
    assert_eq!(result.outcome(), IntegrityOutcome::DigestMatches);
    assert!(!result.matched_trusted_metadata());
}

#[test]
fn unknown_artifacts_remain_identified_only_by_their_untrusted_id() {
    let artifact = ArtifactId::new("unknown-runtime-bytes").expect("artifact");
    let result = IntegrityRecord::new(artifact.clone(), None, None).assess();
    assert_eq!(result.artifact_id(), &artifact);
    assert_eq!(result.outcome(), IntegrityOutcome::MissingExpectedDigest);
}

#[test]
fn evidence_record_requires_a_matching_digest() {
    let result = IntegrityRecord::new(
        ArtifactId::new("runtime").expect("artifact"),
        Some(ExpectedDigest::new(
            digest('a'),
            DigestAuthority::AuthenticatedManifest(ManifestId::new("manifest").expect("manifest")),
        )),
        Some(digest('b')),
    )
    .assess();
    assert!(
        endfield_unix_runtime::ArtifactIntegrityVerification::new(
            EvidenceId::new("evidence").expect("evidence"),
            result,
            None,
        )
        .is_err()
    );
}

#[test]
fn validation_only_metadata_does_not_hash_any_file() {
    let digest = digest('c');
    assert_eq!(digest.hexadecimal().len(), 64);
    assert_eq!(digest.algorithm(), &DigestAlgorithm::Sha256);
}
