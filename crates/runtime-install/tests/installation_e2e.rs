use ed25519_dalek::{Signer, SigningKey};
use endfield_unix_runtime_install::*;
use endfield_unix_runtime_verification as v;
use endfield_unix_storage::AppPaths;
use serde_json::json;
fn archive(family: &str, unsafe_link: bool) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);
    for (path, data, mode) in [
        (
            if family == "proton" {
                "proton"
            } else {
                "bin/wine"
            },
            b"never executed".as_slice(),
            0o6755,
        ),
        ("version", b"GE-Proton10-26".as_slice(), 0o644),
        (
            if family == "proton" {
                "files/nested/file"
            } else {
                "lib/file"
            },
            b"fixture".as_slice(),
            0o644,
        ),
    ] {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(mode);
        h.set_cksum();
        tar.append_data(&mut h, path, data).unwrap();
    }
    if unsafe_link {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_size(0);
        h.set_mode(0o777);
        h.set_link_name("../../escape").unwrap();
        h.set_cksum();
        tar.append_data(&mut h, "link", std::io::empty()).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}
fn empty_archive() -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    tar::Builder::new(encoder)
        .into_inner()
        .unwrap()
        .finish()
        .unwrap()
}
fn evidence(
    data: &[u8],
    family: &str,
    provenance: v::Provenance,
    checkpoint: bool,
) -> v::VerifiedManifest {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    evidence_at(data, family, provenance, checkpoint, now)
}
fn evidence_at(
    data: &[u8],
    family: &str,
    provenance: v::Provenance,
    checkpoint: bool,
    now: i64,
) -> v::VerifiedManifest {
    let key = SigningKey::from_bytes(&[42; 32]);
    let bytes=serde_json::to_vec(&json!({"schema_version":1,"manifest_id":"fixture","manifest_version":1,"provider_id":"fixture","project_id":"fixture","channel":"test","runtime_family":family,"runtime_lineage":"custom","runtime_build":"GE-Proton10-26","key_id":"test","signature_algorithm":"ed25519","issued_at":"2026-01-01T00:00:00Z","expires_at":"2036-01-01T00:00:00Z","artifacts":[{"artifact_id":"test","filename":"runtime.tar.gz","size_bytes":data.len(),"sha256":v::sha256(data)}]})).unwrap();
    let anchor = v::TrustAnchor {
        key_id: "test".into(),
        public_key: key.verifying_key().to_bytes(),
        scope: v::Scope {
            provider: "fixture".into(),
            project: "fixture".into(),
            channel: "test".into(),
            family: family.into(),
            lineage: "custom".into(),
        },
        provenance,
        revoked: false,
        valid_from: None,
        valid_until: None,
    };
    let cp = v::TrustedCheckpoint {
        scope: anchor.scope.clone(),
        manifest_id: "fixture".into(),
        version: 1,
        content_sha256: v::sha256(&bytes),
    };
    v::verify_manifest(
        &bytes,
        &key.sign(&v::signing_input(&bytes)).to_bytes(),
        &[anchor],
        Some(now),
        checkpoint.then_some(&cp),
    )
    .unwrap()
}
#[test]
fn installation_side_by_side_collision_and_integrity() {
    for family in ["wine", "proton"] {
        let d = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_base_dirs(d.path(), d.path(), d.path()).unwrap();
        let data = archive(family, false);
        let source = d.path().join("runtime.tar.gz");
        std::fs::write(&source, &data).unwrap();
        let verified = evidence(
            &data,
            family,
            v::Provenance::IndependentlyAuthenticatedPublisher,
            true,
        );
        let plan = RuntimeInstallationPlan::new(&paths, &verified, "test", 1).unwrap();
        assert!(plan.approve("wrong").is_err());
        install(
            &paths,
            &plan,
            plan.approve(&plan.identity()).unwrap(),
            &verified,
            &source,
        )
        .unwrap();
        assert!(
            plan.destination()
                .join(".endfield-installation.json")
                .is_file()
        );
        assert_eq!(std::fs::read(&source).unwrap(), data);
        assert!(
            install(
                &paths,
                &plan,
                plan.approve(&plan.identity()).unwrap(),
                &verified,
                &source
            )
            .is_err()
        );
        let second = RuntimeInstallationPlan::new(&paths, &verified, "test", 2).unwrap();
        install(
            &paths,
            &second,
            second.approve(&second.identity()).unwrap(),
            &verified,
            &source,
        )
        .unwrap();
        std::fs::write(&source, b"corrupted").unwrap();
        let third = RuntimeInstallationPlan::new(&paths, &verified, "test", 3).unwrap();
        assert!(
            install(
                &paths,
                &third,
                third.approve(&third.identity()).unwrap(),
                &verified,
                &source
            )
            .is_err()
        );
        assert!(!third.destination().exists());
        assert!(plan.destination().exists());
    }
}

#[test]
fn status_requires_a_consistent_owned_record_and_never_adopts_directory_names() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_base_dirs(d.path(), d.path(), d.path()).unwrap();
    let data = archive("proton", false);
    let source = d.path().join("runtime.tar.gz");
    std::fs::write(&source, &data).unwrap();
    let verified = evidence(&data, "proton", v::Provenance::UserPinned, false);
    let plan = RuntimeInstallationPlan::new(&paths, &verified, "test", 1).unwrap();
    install(
        &paths,
        &plan,
        plan.approve_user_pinned(&plan.identity(), plan.artifact_sha256())
            .unwrap(),
        &verified,
        &source,
    )
    .unwrap();
    let record_path = plan.destination().join(".endfield-installation.json");
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    record["plan_sha256"] = serde_json::json!("0".repeat(64));
    std::fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
    let report = status(&paths).unwrap();
    assert_eq!(report.recorded_installations, 0);
    assert!(report.ambiguous_entries > 0);
    assert!(plan.destination().join("proton").is_file());

    let arbitrary = paths.data_dir().join("runtimes/wine/unowned");
    std::fs::create_dir_all(&arbitrary).unwrap();
    for directory in [
        paths.data_dir().to_path_buf(),
        paths.data_dir().join("runtimes"),
        paths.data_dir().join("runtimes/wine"),
        arbitrary,
    ] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let report = status(&paths).unwrap();
    assert_eq!(report.recorded_installations, 0);
    assert!(report.ambiguous_entries > 0);
}

#[test]
fn explicit_user_pin_remains_limited_and_requires_two_bound_acknowledgements() {
    let d = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_base_dirs(d.path(), d.path(), d.path()).unwrap();
    let data = archive("proton", false);
    let pinned = evidence(&data, "proton", v::Provenance::UserPinned, true);
    let source = d.path().join("pinned.tar.gz");
    std::fs::write(&source, &data).unwrap();
    let p021_report = v::verify_artifact(v::ArtifactInput {
        artifact_id: "test",
        filename: "runtime.tar.gz",
        build: "GE-Proton10-26",
        file: v::open_artifact(&source).unwrap(),
        metadata: &pinned,
    })
    .unwrap();
    assert_eq!(p021_report.provenance, v::Provenance::UserPinned);
    assert_eq!(
        p021_report.source.verification().state(),
        endfield_unix_runtime::SourceVerificationState::CandidateIdentified
    );
    let plan = RuntimeInstallationPlan::new(&paths, &pinned, "test", 1).unwrap();
    assert_eq!(
        plan.trust_classification(),
        TrustClassification::UserPinnedLimitedTrust
    );
    assert!(plan.approve(&plan.identity()).is_err());
    assert!(
        plan.approve_user_pinned(&plan.identity(), "0".repeat(64).as_str())
            .is_err()
    );
    let record = install(
        &paths,
        &plan,
        plan.approve_user_pinned(&plan.identity(), plan.artifact_sha256())
            .unwrap(),
        &pinned,
        &source,
    )
    .unwrap();
    assert_eq!(
        record.trust_classification(),
        TrustClassification::UserPinnedLimitedTrust
    );
    let serialized =
        std::fs::read_to_string(plan.destination().join(".endfield-installation.json")).unwrap();
    assert!(serialized.contains("user_pinned_limited_trust"));
    let baseline = endfield_unix_runtime_audit::load_baseline(plan.destination()).unwrap();
    assert_eq!(baseline.installation_id, plan.installation_id());
    let clean =
        endfield_unix_runtime_install::audit_installation(&paths, plan.installation_id()).unwrap();
    assert_eq!(
        clean.completeness,
        endfield_unix_runtime_audit::Completeness::Complete
    );
    assert!(
        clean
            .findings
            .iter()
            .all(|f| f.kind == endfield_unix_runtime_audit::FindingKind::Match)
    );
    let source_match = endfield_unix_runtime_install::audit_against_verified_artifact(
        &paths,
        plan.installation_id(),
        &pinned,
        &source,
    )
    .unwrap();
    assert!(
        source_match
            .findings
            .iter()
            .all(|f| f.kind == endfield_unix_runtime_audit::FindingKind::Match)
    );
    assert_eq!(
        source_match.trust,
        Some(endfield_unix_runtime_audit::TrustClassification::UserPinnedLimitedTrust)
    );
    assert_eq!(
        baseline.origin,
        endfield_unix_runtime_audit::BaselineOrigin::VerifiedInstallation
    );
    let bad_source = d.path().join("wrong-source.tar.gz");
    let mut wrong = data.clone();
    let last = wrong.len() - 1;
    wrong[last] ^= 0x01;
    std::fs::write(&bad_source, wrong).unwrap();
    assert!(
        endfield_unix_runtime_install::audit_against_verified_artifact(
            &paths,
            plan.installation_id(),
            &pinned,
            &bad_source
        )
        .is_err()
    );
    std::fs::write(plan.destination().join("version"), b"tampered").unwrap();
    let changed =
        endfield_unix_runtime_install::audit_installation(&paths, plan.installation_id()).unwrap();
    assert!(
        changed
            .findings
            .iter()
            .any(|f| f.kind == endfield_unix_runtime_audit::FindingKind::Modified)
    );
    let source_changed = endfield_unix_runtime_install::audit_against_verified_artifact(
        &paths,
        plan.installation_id(),
        &pinned,
        &source,
    )
    .unwrap();
    assert!(
        source_changed
            .findings
            .iter()
            .any(|f| f.kind == endfield_unix_runtime_audit::FindingKind::Modified)
    );
    let unchecked = evidence(
        &data,
        "proton",
        v::Provenance::IndependentlyAuthenticatedPublisher,
        false,
    );
    assert!(matches!(
        RuntimeInstallationPlan::new(&paths, &unchecked, "test", 1),
        Err(Error::CheckpointRequired)
    ));
    assert!(plan.destination().exists());
    std::fs::remove_file(
        plan.destination()
            .join(endfield_unix_runtime_audit::BASELINE_FILE),
    )
    .unwrap();
    let legacy =
        endfield_unix_runtime_install::audit_installation(&paths, plan.installation_id()).unwrap();
    assert_eq!(
        legacy.completeness,
        endfield_unix_runtime_audit::Completeness::Unverifiable
    );
    assert_eq!(
        legacy.findings[0].kind,
        endfield_unix_runtime_audit::FindingKind::BaselineMissing
    );
    assert_eq!(
        endfield_unix_runtime_install::status(&paths)
            .unwrap()
            .recorded_installations,
        1
    );
    let legacy_source = endfield_unix_runtime_install::audit_against_verified_artifact(
        &paths,
        plan.installation_id(),
        &pinned,
        &source,
    )
    .unwrap();
    assert!(
        legacy_source
            .findings
            .iter()
            .any(|f| f.kind == endfield_unix_runtime_audit::FindingKind::Modified)
    );
}

#[test]
fn stale_verification_evidence_cannot_authorize_library_apply() {
    let d = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_base_dirs(d.path(), d.path(), d.path()).unwrap();
    let data = archive("proton", false);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let stale = evidence_at(&data, "proton", v::Provenance::UserPinned, false, now - 301);
    assert!(matches!(
        RuntimeInstallationPlan::new(&paths, &stale, "test", 1),
        Err(Error::StaleEvidence)
    ));
    assert!(!paths.data_dir().exists());
}

#[test]
fn edited_plan_destination_invalidates_the_original_approval() {
    let d = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_base_dirs(d.path(), d.path(), d.path()).unwrap();
    let data = archive("proton", false);
    let source = d.path().join("runtime.tar.gz");
    std::fs::write(&source, &data).unwrap();
    let verified = evidence(&data, "proton", v::Provenance::UserPinned, false);
    let original = RuntimeInstallationPlan::new(&paths, &verified, "test", 1).unwrap();
    let approval = original
        .approve_user_pinned(&original.identity(), original.artifact_sha256())
        .unwrap();
    let mut json = serde_json::to_value(&original).unwrap();
    json["destination"] = serde_json::json!(d.path().join("outside"));
    let edited: RuntimeInstallationPlan = serde_json::from_value(json).unwrap();
    assert!(matches!(
        install(&paths, &edited, approval, &verified, &source),
        Err(Error::Approval)
    ));
    assert!(!d.path().join("outside").exists());
    assert!(!original.destination().exists());
}
#[test]
fn unsafe_archive_never_publishes() {
    let d = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_base_dirs(d.path(), d.path(), d.path()).unwrap();
    let data = archive("proton", true);
    let source = d.path().join("runtime.tar.gz");
    std::fs::write(&source, &data).unwrap();
    let verified = evidence(
        &data,
        "proton",
        v::Provenance::IndependentlyAuthenticatedPublisher,
        true,
    );
    let plan = RuntimeInstallationPlan::new(&paths, &verified, "test", 1).unwrap();
    assert!(
        install(
            &paths,
            &plan,
            plan.approve(&plan.identity()).unwrap(),
            &verified,
            &source
        )
        .is_err()
    );
    assert!(!plan.destination().exists());
    assert!(!d.path().join("escape").exists());
}

#[test]
fn empty_signed_archive_fails_structural_validation_without_publication() {
    let d = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_base_dirs(d.path(), d.path(), d.path()).unwrap();
    let data = empty_archive();
    let source = d.path().join("empty.tar.gz");
    std::fs::write(&source, &data).unwrap();
    let verified = evidence(&data, "proton", v::Provenance::UserPinned, false);
    let plan = RuntimeInstallationPlan::new(&paths, &verified, "test", 1).unwrap();
    let approval = plan
        .approve_user_pinned(&plan.identity(), plan.artifact_sha256())
        .unwrap();
    assert!(install(&paths, &plan, approval, &verified, &source).is_err());
    assert!(!plan.destination().exists());
}

struct StopAt(InstallStage);
impl InstallObserver for StopAt {
    fn reached(&mut self, stage: InstallStage) -> Result<()> {
        if stage == self.0 {
            Err(Error::Record)
        } else {
            Ok(())
        }
    }
}

#[test]
fn failure_injection_distinguishes_pre_and_post_publication() {
    let d = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_base_dirs(d.path(), d.path(), d.path()).unwrap();
    let data = archive("proton", false);
    let source = d.path().join("runtime.tar.gz");
    std::fs::write(&source, &data).unwrap();
    let verified = evidence(&data, "proton", v::Provenance::UserPinned, false);
    for (generation, stage) in [
        InstallStage::LockAcquired,
        InstallStage::BeforeStageCreation,
        InstallStage::StageCreated,
        InstallStage::BeforeArtifactCopy,
        InstallStage::StagedArtifactCopied,
        InstallStage::ArtifactVerified,
        InstallStage::BeforeExtraction,
        InstallStage::Extracted,
        InstallStage::BeforeValidation,
        InstallStage::Validated,
        InstallStage::RecordWritten,
        InstallStage::BeforePublish,
    ]
    .into_iter()
    .enumerate()
    {
        let generation = generation as u64 + 1;
        let before = RuntimeInstallationPlan::new(&paths, &verified, "test", generation).unwrap();
        let error = install_observed(
            &paths,
            &before,
            before
                .approve_user_pinned(&before.identity(), before.artifact_sha256())
                .unwrap(),
            &verified,
            &source,
            &mut StopAt(stage),
        )
        .unwrap_err();
        assert!(matches!(error, Error::Record), "{stage:?}: {error}");
        assert!(!before.destination().exists());
    }
    assert_eq!(status(&paths).unwrap().recorded_installations, 0);

    let after = RuntimeInstallationPlan::new(&paths, &verified, "test", 10).unwrap();
    let error = install_observed(
        &paths,
        &after,
        after
            .approve_user_pinned(&after.identity(), after.artifact_sha256())
            .unwrap(),
        &verified,
        &source,
        &mut StopAt(InstallStage::Published),
    )
    .unwrap_err();
    assert!(matches!(error, Error::ReconciliationRequired));
    let report = status(&paths).unwrap();
    assert_eq!(report.recorded_installations, 1);
    assert_eq!(report.incomplete_staging, 0);
}

struct ChangeSourceAt {
    path: std::path::PathBuf,
    bytes: Vec<u8>,
    changed: bool,
}
impl InstallObserver for ChangeSourceAt {
    fn reached(&mut self, stage: InstallStage) -> Result<()> {
        if stage == InstallStage::BeforeArtifactCopy {
            std::fs::write(&self.path, &self.bytes).unwrap();
            self.changed = true;
        }
        Ok(())
    }
}

#[test]
fn source_change_after_initial_verification_is_rejected_before_extraction() {
    let d = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_base_dirs(d.path(), d.path(), d.path()).unwrap();
    let data = archive("proton", false);
    let source = d.path().join("runtime.tar.gz");
    std::fs::write(&source, &data).unwrap();
    let verified = evidence(&data, "proton", v::Provenance::UserPinned, false);
    let plan = RuntimeInstallationPlan::new(&paths, &verified, "test", 1).unwrap();
    let approval = plan
        .approve_user_pinned(&plan.identity(), plan.artifact_sha256())
        .unwrap();
    let mut observer = ChangeSourceAt {
        path: source.clone(),
        bytes: vec![b'x'; data.len()],
        changed: false,
    };
    assert!(matches!(
        install_observed(&paths, &plan, approval, &verified, &source, &mut observer),
        Err(Error::Verification(v::Error::DigestMismatch))
    ));
    assert!(observer.changed);
    assert!(!plan.destination().exists());
    assert_eq!(status(&paths).unwrap().recorded_installations, 0);
}

#[test]
fn recovery_reconciles_matching_validated_staging_after_fresh_approval() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_base_dirs(d.path(), d.path(), d.path()).unwrap();
    let data = archive("proton", false);
    let source = d.path().join("runtime.tar.gz");
    std::fs::write(&source, &data).unwrap();
    let verified = evidence(&data, "proton", v::Provenance::UserPinned, false);
    let plan = RuntimeInstallationPlan::new(&paths, &verified, "test", 7).unwrap();
    let stage = paths.data_dir().join("runtimes/.staging/transaction-crash");
    std::fs::create_dir_all(&stage).unwrap();
    for directory in [
        paths.data_dir().to_path_buf(),
        paths.data_dir().join("runtimes"),
        paths.data_dir().join("runtimes/.staging"),
        stage.clone(),
    ] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let journal = serde_json::json!({
        "schema_version":1,
        "owner":"endfield-unix-runtime-install-v1",
        "plan":plan,
        "state":"Validated"
    });
    let journal_path = stage.join("transaction.json");
    std::fs::write(&journal_path, serde_json::to_vec(&journal).unwrap()).unwrap();
    std::fs::set_permissions(&journal_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let report = status(&paths).unwrap();
    assert_eq!(report.incomplete_staging, 1);
    assert_eq!(report.validated_staging_requiring_approval, 1);
    assert_eq!(report.recorded_installations, 0);
    assert!(journal_path.exists());
    let approval = plan
        .approve_user_pinned(&plan.identity(), plan.artifact_sha256())
        .unwrap();
    install(&paths, &plan, approval, &verified, &source).unwrap();
    assert!(!stage.exists());
    let reconciled = status(&paths).unwrap();
    assert_eq!(reconciled.incomplete_staging, 0);
    assert_eq!(reconciled.recorded_installations, 1);
}

#[test]
fn recovery_inspection_has_a_hard_bound_on_stale_transactions() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_base_dirs(d.path(), d.path(), d.path()).unwrap();
    let staging = paths.data_dir().join("runtimes/.staging");
    std::fs::create_dir_all(&staging).unwrap();
    for directory in [
        paths.data_dir().to_path_buf(),
        paths.data_dir().join("runtimes"),
        staging.clone(),
    ] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    for index in 0..1025 {
        let transaction = staging.join(format!("unknown-{index}"));
        std::fs::create_dir(&transaction).unwrap();
        std::fs::set_permissions(transaction, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    assert!(matches!(status(&paths), Err(Error::Limit)));
}

#[test]
fn simultaneous_same_destination_attempts_publish_at_most_once() {
    use std::sync::{Arc, Barrier};
    let d = tempfile::tempdir().unwrap();
    let data = archive("proton", false);
    let source = d.path().join("runtime.tar.gz");
    std::fs::write(&source, &data).unwrap();
    let source = Arc::new(source);
    let root = Arc::new(d.path().to_path_buf());
    let barrier = Arc::new(Barrier::new(2));
    let mut threads = Vec::new();
    for _ in 0..2 {
        let root = Arc::clone(&root);
        let source = Arc::clone(&source);
        let barrier = Arc::clone(&barrier);
        let data = data.clone();
        threads.push(std::thread::spawn(move || {
            let paths = AppPaths::from_base_dirs(&*root, &*root, &*root).unwrap();
            let verified = evidence(&data, "proton", v::Provenance::UserPinned, false);
            let plan = RuntimeInstallationPlan::new(&paths, &verified, "test", 1).unwrap();
            let approval = plan
                .approve_user_pinned(&plan.identity(), plan.artifact_sha256())
                .unwrap();
            barrier.wait();
            install(&paths, &plan, approval, &verified, &source).is_ok()
        }));
    }
    let succeeded = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(succeeded, 1);
    let paths = AppPaths::from_base_dirs(&*root, &*root, &*root).unwrap();
    assert_eq!(status(&paths).unwrap().recorded_installations, 1);
}
