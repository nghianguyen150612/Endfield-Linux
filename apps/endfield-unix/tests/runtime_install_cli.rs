use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};
fn archive(unsafe_link: bool) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    for (path, data, mode) in [
        ("proton", b"synthetic inert launcher".as_slice(), 0o755),
        ("version", b"GE-Proton10-26".as_slice(), 0o644),
        ("files/lib/nested", b"fixture".as_slice(), 0o644),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(mode);
        header.set_cksum();
        builder.append_data(&mut header, path, data).unwrap();
    }
    if unsafe_link {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        header.set_link_name("../../escape").unwrap();
        header.set_cksum();
        builder
            .append_data(&mut header, "escape-link", std::io::empty())
            .unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_endfield-unix"))
        .args(args)
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .output()
        .unwrap()
}
fn common(manifest: &Path, signature: &Path, archive: &Path, key: &Path) -> Vec<String> {
    let pairs = [
        ("--manifest", manifest),
        ("--signature", signature),
        ("--artifact", archive),
        ("--trust-key", key),
    ];
    let mut args = Vec::new();
    for (name, value) in pairs {
        args.push(name.to_owned());
        args.push(value.display().to_string());
    }
    for (name, value) in [
        ("--provider", "fixture"),
        ("--project", "fixture"),
        ("--channel", "test"),
        ("--family", "proton"),
        ("--lineage", "custom"),
        ("--build", "GE-Proton10-26"),
        ("--artifact-id", "runtime"),
        ("--key-id", "test-key"),
    ] {
        args.push(name.into());
        args.push(value.into());
    }
    args
}
#[test]
fn actual_cli_plan_apply_and_preserve_existing_install() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let signing_key = SigningKey::from_bytes(&[23; 32]);
    let bytes = archive(false);
    let manifest = serde_json::to_vec(&json!({
        "schema_version":1,"manifest_id":"synthetic","manifest_version":1,
        "provider_id":"fixture","project_id":"fixture","channel":"test",
        "runtime_family":"proton","runtime_lineage":"custom","runtime_build":"GE-Proton10-26",
        "key_id":"test-key","signature_algorithm":"ed25519",
        "issued_at":"2026-01-01T00:00:00Z","expires_at":"2036-01-01T00:00:00Z",
        "artifacts":[{"artifact_id":"runtime","filename":"runtime.tar.gz","size_bytes":bytes.len(),"sha256":format!("{:x}",sha2::Sha256::digest(&bytes))}]
    })).unwrap();
    use sha2::Digest;
    let signature = signing_key
        .sign(&endfield_unix_runtime_verification::signing_input(
            &manifest,
        ))
        .to_bytes();
    let manifest_path = root.join("manifest.json");
    let signature_path = root.join("manifest.sig");
    let archive_path = root.join("runtime.tar.gz");
    let key_path = root.join("pinned.pub");
    fs::write(&manifest_path, &manifest).unwrap();
    fs::write(&signature_path, signature).unwrap();
    fs::write(&archive_path, &bytes).unwrap();
    fs::write(&key_path, signing_key.verifying_key().to_bytes()).unwrap();

    let bad_signature_path = root.join("bad-signature.bin");
    let mut bad_signature = signature;
    bad_signature[0] ^= 1;
    fs::write(&bad_signature_path, bad_signature).unwrap();
    let bad_plan_path = root.join("bad-plan.json");
    let mut bad_plan_args = vec!["runtime".into(), "install".into(), "plan".into()];
    bad_plan_args.extend(common(
        &manifest_path,
        &bad_signature_path,
        &archive_path,
        &key_path,
    ));
    bad_plan_args.extend(["--plan-out".into(), bad_plan_path.display().to_string()]);
    let bad_plan = run(
        root,
        &bad_plan_args.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    assert_ne!(bad_plan.status.code(), Some(0));
    assert!(!bad_plan_path.exists());
    assert!(!root.join("data/endfield-unix/runtimes").exists());

    let plan_path = root.join("plan.json");
    let mut plan_args = vec!["runtime".into(), "install".into(), "plan".into()];
    plan_args.extend(common(
        &manifest_path,
        &signature_path,
        &archive_path,
        &key_path,
    ));
    plan_args.extend(["--plan-out".into(), plan_path.display().to_string()]);
    let refs: Vec<_> = plan_args.iter().map(String::as_str).collect();
    let plan = run(root, &refs);
    println!(
        "P025 synthetic plan output:\n{}",
        String::from_utf8_lossy(&plan.stdout)
    );
    assert_eq!(
        plan.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&plan.stdout)
    );
    assert!(String::from_utf8_lossy(&plan.stdout).contains("user-pinned (limited"));
    assert_eq!(
        fs::metadata(&plan_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let plan_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&plan_path).unwrap()).unwrap();
    let identity = String::from_utf8_lossy(&plan.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Plan: "))
        .unwrap()
        .to_owned();
    assert!(String::from_utf8_lossy(&plan.stdout).contains(&format!(
        "Destination: runtimes/proton/{} (under the P006-managed XDG data directory)",
        plan_json["installation_id"].as_str().unwrap()
    )));
    let artifact_digest = plan_json["artifact_sha256"].as_str().unwrap();
    let mut apply_base = vec!["runtime".into(), "install".into(), "apply".into()];
    apply_base.extend(common(
        &manifest_path,
        &signature_path,
        &archive_path,
        &key_path,
    ));
    apply_base.extend(["--plan".into(), plan_path.display().to_string()]);
    let mut bad = apply_base.clone();
    bad.extend([
        "--approve-plan".into(),
        "0".repeat(64),
        "--accept-user-pinned-sha256".into(),
        artifact_digest.into(),
    ]);
    let rejected = run(root, &bad.iter().map(String::as_str).collect::<Vec<_>>());
    println!(
        "P025 rejected apply output:\n{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    assert_ne!(rejected.status.code(), Some(0));
    assert!(!root.join("data/endfield-unix/runtimes").exists());
    let mut good = apply_base.clone();
    good.extend([
        "--approve-plan".into(),
        identity.clone(),
        "--accept-user-pinned-sha256".into(),
        artifact_digest.into(),
    ]);
    let applied = run(root, &good.iter().map(String::as_str).collect::<Vec<_>>());
    println!(
        "P025 successful apply output:\n{}",
        String::from_utf8_lossy(&applied.stdout)
    );
    assert_eq!(
        applied.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&applied.stdout)
    );
    assert!(String::from_utf8_lossy(&applied.stdout).contains("Trust: user-pinned (limited"));
    assert!(
        root.join("data/endfield-unix/runtimes/proton")
            .join(plan_json["installation_id"].as_str().unwrap())
            .join(".endfield-installation.json")
            .is_file()
    );
    assert!(
        root.join("data/endfield-unix/runtimes/proton")
            .join(plan_json["installation_id"].as_str().unwrap())
            .join("files/lib/nested")
            .is_file()
    );
    assert_eq!(fs::read(&archive_path).unwrap(), bytes);
    let inventory = run(root, &["runtime", "list-managed"]);
    println!(
        "P025 managed inventory output:\n{}",
        String::from_utf8_lossy(&inventory.stdout)
    );
    assert_eq!(inventory.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&inventory.stdout).contains("Recorded installations: 1"));
    let installation_id = plan_json["installation_id"].as_str().unwrap();
    let managed = root
        .join("data/endfield-unix/runtimes/proton")
        .join(installation_id);
    let clean_audit = run(
        root,
        &["runtime", "audit", "--installation-id", installation_id],
    );
    assert_eq!(
        clean_audit.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&clean_audit.stdout)
    );
    assert!(String::from_utf8_lossy(&clean_audit.stdout).contains("Local baseline: matches"));
    let mut source_audit_args = vec![
        "runtime".to_owned(),
        "audit-source".to_owned(),
        "--installation-id".to_owned(),
        installation_id.to_owned(),
    ];
    source_audit_args.extend(common(
        &manifest_path,
        &signature_path,
        &archive_path,
        &key_path,
    ));
    let source_audit = run(
        root,
        &source_audit_args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        source_audit.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&source_audit.stdout)
    );
    assert!(
        String::from_utf8_lossy(&source_audit.stdout)
            .contains("Source provenance: user-pinned (limited trust)")
    );
    fs::write(managed.join("version"), b"changed locally").unwrap();
    let changed_audit = run(
        root,
        &["runtime", "audit", "--installation-id", installation_id],
    );
    assert_eq!(changed_audit.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&changed_audit.stdout).contains("Modified"));
    assert_eq!(
        fs::read(managed.join("version")).unwrap(),
        b"changed locally"
    );
    fs::remove_file(managed.join("files/lib/nested")).unwrap();
    let missing_audit = run(
        root,
        &["runtime", "audit", "--installation-id", installation_id],
    );
    assert_eq!(missing_audit.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&missing_audit.stdout).contains("Missing"));
    fs::remove_file(managed.join(endfield_unix_runtime_audit::BASELINE_FILE)).unwrap();
    let legacy_audit = run(
        root,
        &["runtime", "audit", "--installation-id", installation_id],
    );
    assert_eq!(legacy_audit.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&legacy_audit.stdout).contains("UNVERIFIABLE"));
    let legacy_inventory = run(root, &["runtime", "list-managed"]);
    assert_eq!(legacy_inventory.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&legacy_inventory.stdout).contains("Recorded installations: 1")
    );
    let mut duplicate = apply_base.clone();
    duplicate.extend([
        "--approve-plan".into(),
        identity.clone(),
        "--accept-user-pinned-sha256".into(),
        artifact_digest.into(),
    ]);
    assert_ne!(
        run(
            root,
            &duplicate.iter().map(String::as_str).collect::<Vec<_>>()
        )
        .status
        .code(),
        Some(0)
    );
    fs::write(&archive_path, b"changed").unwrap();
    assert_ne!(
        run(root, &good.iter().map(String::as_str).collect::<Vec<_>>())
            .status
            .code(),
        Some(0)
    );
    assert!(
        !root
            .join("data/endfield-unix/runtimes/proton")
            .join(plan_json["installation_id"].as_str().unwrap())
            .join("files/lib/nested")
            .exists(),
        "audit must not restore a deleted runtime file"
    );

    let evil_bytes = archive(true);
    let evil_manifest = serde_json::to_vec(&json!({
        "schema_version":1,"manifest_id":"synthetic-evil","manifest_version":1,
        "provider_id":"fixture","project_id":"fixture","channel":"test",
        "runtime_family":"proton","runtime_lineage":"custom","runtime_build":"GE-Proton10-26",
        "key_id":"test-key","signature_algorithm":"ed25519",
        "issued_at":"2026-01-01T00:00:00Z","expires_at":"2036-01-01T00:00:00Z",
        "artifacts":[{"artifact_id":"runtime","filename":"evil.tar.gz","size_bytes":evil_bytes.len(),"sha256":format!("{:x}",sha2::Sha256::digest(&evil_bytes))}]
    })).unwrap();
    let evil_signature = signing_key
        .sign(&endfield_unix_runtime_verification::signing_input(
            &evil_manifest,
        ))
        .to_bytes();
    let evil_manifest_path = root.join("evil-manifest.json");
    let evil_signature_path = root.join("evil-manifest.sig");
    let evil_archive_path = root.join("evil.tar.gz");
    fs::write(&evil_manifest_path, &evil_manifest).unwrap();
    fs::write(&evil_signature_path, evil_signature).unwrap();
    fs::write(&evil_archive_path, &evil_bytes).unwrap();
    let evil_plan_path = root.join("evil-plan.json");
    let mut evil_plan_args = vec!["runtime".into(), "install".into(), "plan".into()];
    evil_plan_args.extend(common(
        &evil_manifest_path,
        &evil_signature_path,
        &evil_archive_path,
        &key_path,
    ));
    evil_plan_args.extend(["--plan-out".into(), evil_plan_path.display().to_string()]);
    let evil_plan = run(
        root,
        &evil_plan_args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        evil_plan.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&evil_plan.stdout)
    );
    let evil_plan_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&evil_plan_path).unwrap()).unwrap();
    let evil_identity = String::from_utf8_lossy(&evil_plan.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Plan: "))
        .unwrap()
        .to_owned();
    let mut evil_apply = vec!["runtime".into(), "install".into(), "apply".into()];
    evil_apply.extend(common(
        &evil_manifest_path,
        &evil_signature_path,
        &evil_archive_path,
        &key_path,
    ));
    evil_apply.extend([
        "--plan".into(),
        evil_plan_path.display().to_string(),
        "--approve-plan".into(),
        evil_identity,
        "--accept-user-pinned-sha256".into(),
        evil_plan_json["artifact_sha256"].as_str().unwrap().into(),
    ]);
    let evil_result = run(
        root,
        &evil_apply.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    assert_ne!(evil_result.status.code(), Some(0));
    let evil_destination = root
        .join("data/endfield-unix/runtimes/proton")
        .join(evil_plan_json["installation_id"].as_str().unwrap());
    assert!(!evil_destination.exists());
    assert!(!root.join("escape").exists());
    assert!(
        root.join("data/endfield-unix/runtimes/proton")
            .join(plan_json["installation_id"].as_str().unwrap())
            .exists()
    );
}
