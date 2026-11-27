use ed25519_dalek::VerifyingKey;
use endfield_unix_runtime_verification::*;
use serde_json::Value;
use std::{fs, path::PathBuf};

fn required_path(name: &str) -> PathBuf {
    PathBuf::from(
        std::env::var_os(name).unwrap_or_else(|| panic!("set {name} for this opt-in test")),
    )
}

#[test]
#[ignore = "requires the recovered Wine candidate and a temporary development manifest/key"]
fn p024_verifies_the_real_candidate_only_in_development_scope() {
    let artifact = required_path("P034_WINE_ARTIFACT");
    let profile = required_path("P034_WINE_PROFILE");
    let manifest_path = required_path("P034_WINE_MANIFEST");
    let signature_path = required_path("P034_WINE_SIGNATURE");
    let public_key_path = required_path("P034_WINE_PUBLIC_KEY");
    let manifest_bytes = fs::read(&manifest_path).unwrap();
    let signature = fs::read(&signature_path).unwrap();
    let public_key: [u8; 32] = fs::read(public_key_path).unwrap().try_into().unwrap();
    let manifest_value: Value = serde_json::from_slice(&manifest_bytes).unwrap();
    let manifest = Manifest::parse(&manifest_bytes).unwrap();
    assert_eq!(manifest.provider_id, "endfield-unix-development");
    assert_eq!(manifest.project_id, "p034-disposable");
    assert_eq!(manifest.channel, "development");
    assert_eq!(manifest.runtime_build, "wine-10.0");
    assert!(manifest.key_id.starts_with("dev-"));
    let artifact_entry = &manifest.artifacts[0];
    assert_eq!(
        artifact_entry.filename,
        "winehq-noble-10.0-dualarch-candidate.tar.gz"
    );
    assert_eq!(artifact_entry.size_bytes, 437_830_035);
    assert_eq!(
        artifact_entry.sha256,
        "391fe86b400d2cf7a8a58c72866c7a8752d4cc613f5536003a7f45f40f7851ec"
    );
    let profile_bytes = fs::read(profile).unwrap();
    assert_eq!(
        artifact_entry.wine_qualification_profile_sha256.as_deref(),
        Some(sha256(&profile_bytes).as_str())
    );

    let key = VerifyingKey::from_bytes(&public_key).unwrap();
    let scope = Scope {
        provider: manifest.provider_id.clone(),
        project: manifest.project_id.clone(),
        channel: manifest.channel.clone(),
        family: manifest.runtime_family.clone(),
        lineage: manifest.runtime_lineage.clone(),
    };
    let anchor = TrustAnchor {
        key_id: manifest.key_id.clone(),
        public_key: key.to_bytes(),
        scope,
        provenance: Provenance::DevelopmentOnly,
        revoked: false,
        valid_from: None,
        valid_until: None,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let verified = verify_manifest(
        &manifest_bytes,
        &signature,
        &[anchor.clone()],
        Some(now),
        None,
    )
    .unwrap();
    assert_eq!(verified.provenance(), Provenance::DevelopmentOnly);
    assert!(verified.checkpoint().is_err());
    let report = verify_artifact(ArtifactInput {
        artifact_id: &artifact_entry.artifact_id,
        filename: &artifact_entry.filename,
        build: &manifest.runtime_build,
        file: open_artifact(&artifact).unwrap(),
        metadata: &verified,
    })
    .unwrap();
    assert_eq!(report.size, 437_830_035);
    assert!(!report.assessment.matched_trusted_metadata());
    assert!(!report.source.verification().state().has_verified_artifact());

    let mut promoted = anchor.clone();
    promoted.provenance = Provenance::IndependentlyAuthenticatedPublisher;
    assert!(matches!(
        verify_manifest(&manifest_bytes, &signature, &[promoted], Some(now), None),
        Err(Error::Scope)
    ));
    let mut revoked = anchor.clone();
    revoked.revoked = true;
    assert!(matches!(
        verify_manifest(&manifest_bytes, &signature, &[revoked], Some(now), None),
        Err(Error::RevokedKey)
    ));
    let mut wrong_key = anchor.clone();
    wrong_key.public_key = [0; 32];
    assert!(verify_manifest(&manifest_bytes, &signature, &[wrong_key], Some(now), None).is_err());
    let mut wrong_scope = anchor;
    wrong_scope.scope.channel = "stable".into();
    assert!(matches!(
        verify_manifest(&manifest_bytes, &signature, &[wrong_scope], Some(now), None),
        Err(Error::Scope)
    ));

    let mut modified_manifest = manifest_bytes.clone();
    modified_manifest.push(b' ');
    assert!(
        verify_manifest(
            &modified_manifest,
            &signature,
            &[TrustAnchor {
                key_id: manifest.key_id,
                public_key: key.to_bytes(),
                scope: Scope {
                    provider: "endfield-unix-development".into(),
                    project: "p034-disposable".into(),
                    channel: "development".into(),
                    family: "wine".into(),
                    lineage: "upstream-wine".into()
                },
                provenance: Provenance::DevelopmentOnly,
                revoked: false,
                valid_from: None,
                valid_until: None,
            }],
            Some(now),
            None
        )
        .is_err()
    );
    assert_eq!(manifest_value["channel"], "development");
}
