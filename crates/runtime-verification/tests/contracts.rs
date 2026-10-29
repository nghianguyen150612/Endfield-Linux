use ed25519_dalek::{Signer, SigningKey};
use endfield_unix_runtime::*;
use endfield_unix_runtime_verification::*;
use serde_json::{Value, json};
fn manifest(data: &[u8]) -> Value {
    json!({"schema_version":1,"manifest_id":"synthetic-manifest","manifest_version":1,"provider_id":"synthetic-provider","project_id":"synthetic-project","channel":"test","runtime_family":"proton","runtime_lineage":"ge-proton","runtime_build":"synthetic-build","key_id":"synthetic-key","signature_algorithm":"ed25519","issued_at":"2026-01-01T00:00:00Z","expires_at":"2036-01-01T00:00:00Z","artifacts":[{"artifact_id":"synthetic-artifact","filename":"runtime.tar.gz","size_bytes":data.len(),"sha256":sha256(data)}]})
}
fn anchor(p: Provenance) -> TrustAnchor {
    TrustAnchor {
        key_id: "synthetic-key".into(),
        public_key: SigningKey::from_bytes(&[42; 32]).verifying_key().to_bytes(),
        scope: Scope {
            provider: "synthetic-provider".into(),
            project: "synthetic-project".into(),
            channel: "test".into(),
            family: "proton".into(),
            lineage: "ge-proton".into(),
        },
        provenance: p,
        revoked: false,
        valid_from: None,
        valid_until: None,
    }
}
fn signed(v: &Value) -> (Vec<u8>, Vec<u8>) {
    let b = serde_json::to_vec(v).unwrap();
    let s = SigningKey::from_bytes(&[42; 32])
        .sign(&signing_input(&b))
        .to_bytes()
        .to_vec();
    (b, s)
}
fn verify(v: &Value) -> Result<VerifiedManifest, Error> {
    let (b, s) = signed(v);
    verify_manifest(
        &b,
        &s,
        &[anchor(Provenance::IndependentlyAuthenticatedPublisher)],
        Some(1800000000),
        None,
    )
}
#[test]
fn schema_negative_matrix() {
    let base = manifest(b"test");
    assert!(verify(&base).is_ok());
    for (field, value) in [
        ("schema_version", json!(2)),
        ("manifest_id", json!("")),
        ("project_id", json!("bad/project")),
        ("provider_id", json!("bad provider")),
        ("manifest_version", json!(0)),
        ("runtime_family", json!("unknown")),
        ("issued_at", json!("yesterday")),
        ("expires_at", json!("2025-01-01T00:00:00Z")),
        ("signature_algorithm", json!("rsa")),
        ("critical", json!(true)),
        ("artifacts", json!([])),
    ] {
        let mut v = base.clone();
        v[field] = value;
        assert!(verify(&v).is_err(), "{field}");
    }
    for field in ["manifest_id", "runtime_build", "key_id"] {
        let mut v = base.clone();
        v.as_object_mut().unwrap().remove(field);
        assert!(verify(&v).is_err());
    }
    for (field, value) in [
        ("filename", json!("../bad")),
        ("filename", json!("C:\\bad")),
        ("filename", json!("NUL.tar")),
        ("size_bytes", json!(-1)),
        ("size_bytes", json!(u64::MAX)),
        ("sha256", json!("bad")),
        ("sha256", json!("")),
        ("digest_algorithm", json!("md5")),
    ] {
        let mut v = base.clone();
        v["artifacts"][0][field] = value;
        assert!(verify(&v).is_err(), "{field}");
    }
    let mut v = base.clone();
    v["artifacts"] = json!([base["artifacts"][0], base["artifacts"][0]]);
    assert!(verify(&v).is_err());
    let b = serde_json::to_string(&base)
        .unwrap()
        .replacen('{', "{\"schema_version\":1,", 1);
    assert!(Manifest::parse(b.as_bytes()).is_err());
    assert!(Manifest::parse(&vec![b' '; MAX_MANIFEST_BYTES + 1]).is_err());
    assert!(
        Manifest::parse(format!("{}0{}", "[".repeat(200), "]".repeat(200)).as_bytes()).is_err()
    );
    let mut v = base.clone();
    v["artifacts"] = Value::Array(
        (0..129)
            .map(|i| {
                let mut a = base["artifacts"][0].clone();
                a["artifact_id"] = json!(format!("id-{i}"));
                a["filename"] = json!(format!("file-{i}"));
                a
            })
            .collect(),
    );
    assert!(verify(&v).is_err());
}
#[test]
fn signature_and_scope_negative_matrix() {
    let (b, s) = signed(&manifest(b"a"));
    let a = anchor(Provenance::IndependentlyAuthenticatedPublisher);
    assert!(verify_manifest(&b, &s, &[a.clone()], Some(1800000000), None).is_ok());
    for bytes in [
        [b.clone(), vec![b' ']].concat(),
        b.iter()
            .map(|x| if *x == b'g' { b'h' } else { *x })
            .collect(),
    ] {
        assert!(verify_manifest(&bytes, &s, &[a.clone()], None, None).is_err());
    }
    for sig in [
        s[..63].to_vec(),
        vec![0; 64],
        SigningKey::from_bytes(&[42; 32])
            .sign(&b)
            .to_bytes()
            .to_vec(),
    ] {
        assert!(verify_manifest(&b, &sig, &[a.clone()], None, None).is_err());
    }
    for i in 0..10 {
        let mut wrong = a.clone();
        match i {
            0 => wrong.public_key = SigningKey::from_bytes(&[43; 32]).verifying_key().to_bytes(),
            1 => wrong.key_id = "wrong".into(),
            2 => wrong.scope.project = "valve".into(),
            3 => wrong.scope.provider = "other".into(),
            4 => wrong.scope.channel = "stable".into(),
            5 => wrong.public_key = [0; 32],
            6 => wrong.revoked = true,
            7 => wrong.provenance = Provenance::Unverified,
            8 => wrong.scope.family = "wine".into(),
            _ => wrong.scope.lineage = "valve-proton".into(),
        };
        assert!(
            verify_manifest(&b, &s, &[wrong], None, None).is_err(),
            "{i}"
        );
    }
    assert!(verify_manifest(&b, &s, &[], None, None).is_err());
    assert!(verify_manifest(&b, &s, &[a.clone(), a], None, None).is_err());
}
#[test]
fn rollback_and_expiry() {
    let v = manifest(b"a");
    let m = verify(&v).unwrap();
    assert_eq!(m.rollback(), RollbackStatus::NoCheckpoint);
    let checkpoint = m.checkpoint().unwrap();
    let (b, s) = signed(&v);
    let a = anchor(Provenance::IndependentlyAuthenticatedPublisher);
    assert_eq!(
        verify_manifest(&b, &s, &[a.clone()], Some(1800000000), Some(&checkpoint))
            .unwrap()
            .rollback(),
        RollbackStatus::Checked
    );
    let mut newer = v.clone();
    newer["manifest_version"] = json!(2);
    let n = verify(&newer).unwrap().checkpoint().unwrap();
    assert!(matches!(
        verify_manifest(&b, &s, &[a.clone()], Some(1800000000), Some(&n)),
        Err(Error::Rollback)
    ));
    let mut changed = v;
    changed["runtime_build"] = json!("different");
    let (cb, cs) = signed(&changed);
    assert!(matches!(
        verify_manifest(&cb, &cs, &[a.clone()], Some(1800000000), Some(&checkpoint)),
        Err(Error::CheckpointConflict)
    ));
    assert!(matches!(
        verify_manifest(&b, &s, &[a.clone()], Some(1), None),
        Err(Error::FutureIssued)
    ));
    assert!(matches!(
        verify_manifest(&b, &s, &[a.clone()], Some(2200000000), None),
        Err(Error::Expired)
    ));
    let unknown = verify_manifest(&b, &s, &[a], None, None).unwrap();
    assert_eq!(
        unknown.expiration(),
        ExpirationStatus::TrustedTimeUnavailable
    );
    assert!(unknown.checkpoint().is_err());
}
#[test]
fn artifact_e2e_and_limited_trust() {
    for data in [
        vec![],
        b"synthetic archive bytes".to_vec(),
        vec![73; 200000],
    ] {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("runtime.tar.gz");
        std::fs::write(&p, &data).unwrap();
        let v = manifest(&data);
        let (b, s) = signed(&v);
        for provenance in [
            Provenance::IndependentlyAuthenticatedPublisher,
            Provenance::UserPinned,
        ] {
            let m = verify_manifest(&b, &s, &[anchor(provenance)], Some(1800000000), None).unwrap();
            let r = verify_artifact(ArtifactInput {
                artifact_id: "synthetic-artifact",
                filename: "runtime.tar.gz",
                build: "synthetic-build",
                file: open_artifact(&p).unwrap(),
                metadata: &m,
            })
            .unwrap();
            assert_eq!(
                r.assessment.matched_trusted_metadata(),
                provenance == Provenance::IndependentlyAuthenticatedPublisher
            );
            assert_eq!(
                r.source.verification().state().has_verified_artifact(),
                provenance == Provenance::IndependentlyAuthenticatedPublisher
            );
            assert_eq!(std::fs::read(&p).unwrap(), data);
            assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
            let req = IntegrityVerificationRequest::new(
                r.assessment.artifact_id().clone(),
                r.assessment.expected().cloned(),
            );
            assert!(r.assess_request(&req).is_ok());
            for (id, build, name) in [
                ("wrong", "synthetic-build", "runtime.tar.gz"),
                ("synthetic-artifact", "wrong", "runtime.tar.gz"),
                ("synthetic-artifact", "synthetic-build", "other"),
            ] {
                assert!(matches!(
                    verify_artifact(ArtifactInput {
                        artifact_id: id,
                        filename: name,
                        build,
                        file: open_artifact(&p).unwrap(),
                        metadata: &m
                    }),
                    Err(Error::Identity)
                ));
            }
        }
        let m = verify(&v).unwrap();
        std::fs::write(&p, vec![0; data.len()]).unwrap();
        if !data.is_empty() {
            assert!(matches!(
                verify_artifact(ArtifactInput {
                    artifact_id: "synthetic-artifact",
                    filename: "runtime.tar.gz",
                    build: "synthetic-build",
                    file: open_artifact(&p).unwrap(),
                    metadata: &m
                }),
                Err(Error::DigestMismatch)
            ));
        }
        std::fs::write(&p, b"different size").unwrap();
        assert!(matches!(
            verify_artifact(ArtifactInput {
                artifact_id: "synthetic-artifact",
                filename: "runtime.tar.gz",
                build: "synthetic-build",
                file: open_artifact(&p).unwrap(),
                metadata: &m
            }),
            Err(Error::SizeMismatch)
        ));
    }
}
#[cfg(target_os = "linux")]
#[test]
fn filesystem_policy() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("file");
    std::fs::write(&p, b"x").unwrap();
    let link = d.path().join("link");
    symlink(&p, &link).unwrap();
    assert!(matches!(open_artifact(&link), Err(Error::FileType)));
    assert!(matches!(open_artifact(d.path()), Err(Error::FileType)));
    assert!(matches!(
        open_artifact(&d.path().join("missing")),
        Err(Error::FileUnavailable)
    ));
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o0)).unwrap();
    if std::env::var("USER").unwrap_or_default() != "root" {
        assert!(matches!(open_artifact(&p), Err(Error::PermissionDenied)));
    }
    let fifo = d.path().join("fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    assert!(matches!(open_artifact(&fifo), Err(Error::FileType)));
    assert!(matches!(
        open_artifact(std::path::Path::new("/dev/null")),
        Err(Error::FileType)
    ));
}

#[test]
fn committed_fixture_and_time_policy() {
    let bytes = include_bytes!("fixtures/manifest.json");
    let sig = include_bytes!("fixtures/manifest.sig");
    let key = include_bytes!("fixtures/public-key.bin");
    let mut a = anchor(Provenance::UserPinned);
    assert_eq!(key, &a.public_key);
    assert!(verify_manifest(bytes, sig, &[a.clone()], Some(1800000000), None).is_ok());
    a.valid_from = Some(1700000000);
    a.valid_until = Some(1900000000);
    assert!(matches!(
        verify_manifest(bytes, sig, &[a.clone()], None, None),
        Err(Error::TimeUnknown)
    ));
    assert!(verify_manifest(bytes, sig, &[a.clone()], Some(1800000000), None).is_ok());
    a.valid_until = Some(1750000000);
    assert!(matches!(
        verify_manifest(bytes, sig, &[a], Some(1800000000), None),
        Err(Error::Expired)
    ));
    let m = verify_manifest(
        bytes,
        sig,
        &[anchor(Provenance::IndependentlyAuthenticatedPublisher)],
        None,
        None,
    )
    .unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime.tar.gz");
    let r = verify_artifact(ArtifactInput {
        artifact_id: "synthetic-artifact",
        filename: "runtime.tar.gz",
        build: "synthetic-build",
        file: open_artifact(&p).unwrap(),
        metadata: &m,
    })
    .unwrap();
    assert!(!r.assessment.matched_trusted_metadata());
    assert!(!r.source.verification().authenticity().is_established());
}

#[test]
fn p021_provider_is_bound_to_verified_input() {
    let m = verify(&manifest(b"test")).unwrap();
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("runtime.tar.gz");
    std::fs::write(&p, b"test").unwrap();
    let provider = VerifiedArtifactProvider::verify(ArtifactInput {
        artifact_id: "synthetic-artifact",
        filename: "runtime.tar.gz",
        build: "synthetic-build",
        file: open_artifact(&p).unwrap(),
        metadata: &m,
    })
    .unwrap();
    let report = provider.report();
    assert!(
        provider
            .verify_source(&SourceVerificationRequest::new(report.source.clone()))
            .unwrap()
            .state()
            .has_verified_artifact()
    );
    assert!(
        provider
            .verify_integrity(&IntegrityVerificationRequest::new(
                report.assessment.artifact_id().clone(),
                report.assessment.expected().cloned()
            ))
            .unwrap()
            .matched_trusted_metadata()
    );
    assert!(
        provider
            .verify_integrity(&IntegrityVerificationRequest::new(
                ArtifactId::new("different").unwrap(),
                None
            ))
            .is_err()
    );
}
