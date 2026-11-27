use ed25519_dalek::VerifyingKey;
use endfield_unix_runtime_install::*;
use endfield_unix_runtime_selection as selection;
use endfield_unix_runtime_verification as verification;
use endfield_unix_storage::AppPaths;
use std::{fs, path::PathBuf};

fn path(name: &str) -> PathBuf {
    PathBuf::from(
        std::env::var_os(name).unwrap_or_else(|| panic!("set {name} for this opt-in test")),
    )
}

#[test]
#[ignore = "requires recovered candidate and a temporary P024 development manifest/key"]
fn disposable_p024_p025_p026_p027_lifecycle_keeps_development_scope() {
    let artifact = path("P034_WINE_ARTIFACT");
    let profile = path("P034_WINE_PROFILE");
    let manifest_path = path("P034_WINE_MANIFEST");
    let signature_path = path("P034_WINE_SIGNATURE");
    let public_key_path = path("P034_WINE_PUBLIC_KEY");
    let manifest_bytes = fs::read(&manifest_path).unwrap();
    let manifest = verification::Manifest::parse(&manifest_bytes).unwrap();
    let key: [u8; 32] = fs::read(public_key_path).unwrap().try_into().unwrap();
    let key = VerifyingKey::from_bytes(&key).unwrap();
    assert_eq!(
        manifest.artifacts[0]
            .wine_qualification_profile_sha256
            .as_deref(),
        Some(verification::sha256(&fs::read(profile).unwrap()).as_str())
    );
    let anchor = verification::TrustAnchor {
        key_id: manifest.key_id.clone(),
        public_key: key.to_bytes(),
        scope: verification::Scope {
            provider: manifest.provider_id.clone(),
            project: manifest.project_id.clone(),
            channel: manifest.channel.clone(),
            family: manifest.runtime_family.clone(),
            lineage: manifest.runtime_lineage.clone(),
        },
        provenance: verification::Provenance::DevelopmentOnly,
        revoked: false,
        valid_from: None,
        valid_until: None,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let verified = verification::verify_manifest(
        &manifest_bytes,
        &fs::read(signature_path).unwrap(),
        &[anchor],
        Some(now),
        None,
    )
    .unwrap();
    assert_eq!(
        verified.provenance(),
        verification::Provenance::DevelopmentOnly
    );
    assert!(verified.checkpoint().is_err());

    let temporary = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_base_dirs(
        temporary.path().join("config"),
        temporary.path().join("data"),
        temporary.path().join("cache"),
    )
    .unwrap();
    let plan =
        RuntimeInstallationPlan::new(&paths, &verified, "winehq-dualarch-candidate", 1).unwrap();
    assert_eq!(
        plan.trust_classification(),
        TrustClassification::DevelopmentOnly
    );
    assert!(plan.approve(&plan.identity()).is_err());
    let authorization = plan.approve_development(&plan.identity()).unwrap();
    let installed = install(&paths, &plan, authorization, &verified, &artifact).unwrap();
    assert_eq!(
        installed.trust_classification(),
        TrustClassification::DevelopmentOnly
    );

    let identity = pin_identity(&paths, plan.installation_id()).unwrap();
    assert_eq!(identity.trust, TrustClassification::DevelopmentOnly);
    assert_eq!(
        identity.audit.completeness,
        endfield_unix_runtime_audit::Completeness::Complete
    );
    assert_eq!(
        identity.audit.trust,
        Some(endfield_unix_runtime_audit::TrustClassification::DevelopmentOnly)
    );
    let machine = selection::current_machine_id().unwrap();
    let scope = selection::RuntimePinScope::Machine { machine };
    let runtime = selection::ManagedRuntimeReference {
        installation_id: identity.installation_id.clone(),
        family: identity.family.clone(),
        lineage: identity.lineage.clone(),
        provider: identity.provider.clone(),
        project: identity.project.clone(),
        build: identity.build.clone(),
        artifact_sha256: identity.artifact_sha256.clone(),
        manifest_sha256: identity.manifest_sha256.clone(),
        generation: identity.generation,
        ownership_record_sha256: identity.ownership_record_sha256.clone(),
        trust: selection::Provenance::DevelopmentOnly,
    };
    let pin_id = selection::stable_pin_id(&scope, &runtime);
    let pin = selection::RuntimePin::new(&pin_id, scope, runtime, None, now as u64).unwrap();
    let pin_store = selection::PinStore::for_app(&paths);
    let initial = pin_store.load().unwrap();
    let published = pin_store.set(pin, initial.revision, false).unwrap();
    let reloaded = selection::PinStore::for_app(&paths)
        .load_read_only()
        .unwrap();
    assert_eq!(reloaded.revision, published.revision);
    assert_eq!(reloaded.pins, published.pins);
    assert_eq!(
        reloaded.pins[0].runtime.provenance(),
        selection::Provenance::DevelopmentOnly
    );
    let after_pin = pin_identity(&paths, plan.installation_id()).unwrap();
    assert_eq!(after_pin.installation_id, identity.installation_id);
    assert_eq!(after_pin.generation, 1);
    assert_eq!(
        after_pin.audit.completeness,
        endfield_unix_runtime_audit::Completeness::Complete
    );
    if let Some(record) = std::env::var_os("P034_PRESERVE_STATE_RECORD") {
        let preserved = temporary.into_path();
        fs::write(record, preserved.as_os_str().as_encoded_bytes()).unwrap();
    }
}
