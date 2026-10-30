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
    assert!(String::from_utf8_lossy(&clean_audit.stdout).contains("Audit completeness: complete"));

    // P027 actual process-level persistence and precedence smoke. Each command
    // starts a fresh CLI process against this isolated XDG root.
    let set_machine = vec![
        "runtime",
        "pin",
        "set",
        "--scope",
        "machine",
        "--installation-id",
        installation_id,
        "--expected-revision",
        "0",
    ];
    let machine_pin = run(root, &set_machine);
    assert_eq!(
        machine_pin.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&machine_pin.stdout)
    );
    let shown_machine = run(root, &["runtime", "pin", "show", "--scope", "machine"]);
    assert_eq!(
        shown_machine.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&shown_machine.stdout)
    );
    assert!(String::from_utf8_lossy(&shown_machine.stdout).contains(installation_id));
    let resolved_machine = run(root, &["runtime", "pin", "resolve"]);
    assert_eq!(
        resolved_machine.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&resolved_machine.stdout)
    );
    assert!(
        String::from_utf8_lossy(&resolved_machine.stdout).contains("P026 local baseline matches")
    );

    let profile_set_args = [
        "runtime",
        "pin",
        "set",
        "--scope",
        "profile",
        "--profile",
        "fixture-profile",
        "--installation-id",
        installation_id,
        "--expected-revision",
        "1",
        "--adapter",
        "umu-proton",
    ];
    let profile_set = run(root, &profile_set_args);
    assert_eq!(
        profile_set.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&profile_set.stdout)
    );
    let resolved_profile = run(
        root,
        &["runtime", "pin", "resolve", "--profile", "fixture-profile"],
    );
    assert_eq!(
        resolved_profile.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&resolved_profile.stdout)
    );
    assert!(String::from_utf8_lossy(&resolved_profile.stdout).contains("ProfilePin"));
    let profile_pin_before_prefix = run(
        root,
        &[
            "runtime",
            "pin",
            "show",
            "--scope",
            "profile",
            "--profile",
            "fixture-profile",
        ],
    );
    assert_eq!(profile_pin_before_prefix.status.code(), Some(0));

    // P028 binds a directory-only prefix to the exact P027 profile selection.
    // Planning is read-only, creation does not execute Wine, and init fails closed.
    let prefix_path = root.join("data/endfield-unix/prefixes/data/p028-managed");
    let prefix_plan = run(
        root,
        &[
            "prefix",
            "plan",
            "--id",
            "p028-managed",
            "--scope",
            "profile",
            "--profile",
            "fixture-profile",
            "--expected-revision",
            "0",
        ],
    );
    println!(
        "Prefix managed plan output:\n{}",
        String::from_utf8_lossy(&prefix_plan.stdout)
    );
    assert_eq!(
        prefix_plan.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&prefix_plan.stdout)
    );
    let prefix_plan_output = String::from_utf8_lossy(&prefix_plan.stdout);
    assert!(prefix_plan_output.contains("Prefix creation plan"));
    assert!(prefix_plan_output.contains("ProfilePin"));
    assert!(prefix_plan_output.contains("Adapter preference: UmuProton"));
    assert!(prefix_plan_output.contains("Filesystem mutation: no"));
    assert!(!prefix_path.exists());
    assert!(
        !root
            .join("data/endfield-unix/prefixes/registry.json")
            .exists()
    );

    let prefix_create = run(
        root,
        &[
            "prefix",
            "create",
            "--id",
            "p028-managed",
            "--scope",
            "profile",
            "--profile",
            "fixture-profile",
            "--expected-revision",
            "0",
        ],
    );
    println!(
        "Prefix managed create output:\n{}",
        String::from_utf8_lossy(&prefix_create.stdout)
    );
    assert_eq!(
        prefix_create.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&prefix_create.stdout)
    );
    assert!(
        String::from_utf8_lossy(&prefix_create.stdout)
            .contains("Lifecycle: created (uninitialized)")
    );
    assert!(
        String::from_utf8_lossy(&prefix_create.stdout).contains("Adapter preference: UmuProton")
    );
    assert!(String::from_utf8_lossy(&prefix_create.stdout).contains(installation_id));
    assert!(prefix_path.is_dir());
    assert!(!prefix_path.join("drive_c").exists());
    assert!(managed.join("proton").is_file());

    let prefix_show = run(root, &["prefix", "show", "--id", "p028-managed"]);
    assert_eq!(prefix_show.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&prefix_show.stdout).contains("Lifecycle: created (uninitialized)")
    );
    assert!(String::from_utf8_lossy(&prefix_show.stdout).contains("Initialization evidence: none"));
    let prefix_list = run(root, &["prefix", "list"]);
    assert_eq!(prefix_list.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&prefix_list.stdout).contains("Prefix p028-managed"));

    fs::write(managed.join("proton"), b"modified synthetic inert launcher").unwrap();
    let stale_managed_prefix = run(root, &["prefix", "validate", "--id", "p028-managed"]);
    assert_eq!(stale_managed_prefix.status.code(), Some(4));
    let stale_managed_output = String::from_utf8_lossy(&stale_managed_prefix.stdout);
    assert!(stale_managed_output.contains("Runtime association: stale or unavailable"));
    assert!(
        stale_managed_output
            .contains("Runtime validation: stale or unavailable; P025/P026 validation failed")
    );
    assert!(stale_managed_output.contains("P026 detected a local baseline mismatch"));
    assert!(prefix_path.is_dir());
    fs::write(managed.join("proton"), b"synthetic inert launcher").unwrap();

    fs::remove_file(managed.join("proton")).unwrap();
    let removed_managed_runtime = run(root, &["prefix", "validate", "--id", "p028-managed"]);
    assert_eq!(removed_managed_runtime.status.code(), Some(4));
    assert!(
        String::from_utf8_lossy(&removed_managed_runtime.stdout)
            .contains("Runtime association: stale or unavailable")
    );
    assert!(
        String::from_utf8_lossy(&removed_managed_runtime.stdout)
            .contains("Runtime validation: stale or unavailable; P025/P026 validation failed")
    );
    assert!(prefix_path.is_dir());
    fs::write(managed.join("proton"), b"synthetic inert launcher").unwrap();

    let prefix_validate = run(root, &["prefix", "validate", "--id", "p028-managed"]);
    println!(
        "Prefix managed validation output:\n{}",
        String::from_utf8_lossy(&prefix_validate.stdout)
    );
    assert_eq!(
        prefix_validate.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&prefix_validate.stdout)
    );
    assert!(
        String::from_utf8_lossy(&prefix_validate.stdout)
            .contains("Wine initialization: not initialized")
    );
    let prefix_init = run(root, &["prefix", "init", "--id", "p028-managed"]);
    println!(
        "Prefix blocked initialization output:\n{}",
        String::from_utf8_lossy(&prefix_init.stdout)
    );
    assert_eq!(prefix_init.status.code(), Some(5));
    let prefix_init_output = String::from_utf8_lossy(&prefix_init.stdout);
    assert!(prefix_init_output.contains("Initialization: BLOCKED"));
    assert!(prefix_init_output.contains("Error category: initialization_unsupported"));
    assert!(prefix_init_output.contains("Process execution: not performed"));
    assert!(prefix_path.is_dir());
    assert!(!prefix_path.join("drive_c").exists());
    assert!(managed.join("proton").is_file());

    let prefix_delete = run(
        root,
        &[
            "prefix",
            "delete",
            "--id",
            "p028-managed",
            "--expected-revision",
            "2",
            "--confirm-path",
            prefix_path.to_str().unwrap(),
        ],
    );
    println!(
        "Prefix managed deletion output:\n{}",
        String::from_utf8_lossy(&prefix_delete.stdout)
    );
    assert_eq!(
        prefix_delete.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&prefix_delete.stdout)
    );
    assert!(!prefix_path.exists());
    assert!(managed.join("proton").is_file());
    let profile_pin_after_prefix = run(
        root,
        &[
            "runtime",
            "pin",
            "show",
            "--scope",
            "profile",
            "--profile",
            "fixture-profile",
        ],
    );
    assert_eq!(profile_pin_after_prefix.status.code(), Some(0));
    assert_eq!(
        profile_pin_after_prefix.stdout,
        profile_pin_before_prefix.stdout
    );

    let unregistered_path = root.join("data/endfield-unix/prefixes/data/p028-unregistered");
    let unregistered_create = run(
        root,
        &[
            "prefix",
            "create",
            "--id",
            "p028-unregistered",
            "--scope",
            "profile",
            "--profile",
            "fixture-profile",
            "--expected-revision",
            "4",
        ],
    );
    assert_eq!(unregistered_create.status.code(), Some(0));
    let unregistered_remove = run(
        root,
        &[
            "prefix",
            "remove",
            "--id",
            "p028-unregistered",
            "--expected-revision",
            "6",
        ],
    );
    assert_eq!(unregistered_remove.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&unregistered_remove.stdout).contains("Prefix bytes: preserved")
    );
    assert!(
        String::from_utf8_lossy(&unregistered_remove.stdout)
            .contains(unregistered_path.to_str().unwrap())
    );
    assert!(unregistered_path.is_dir());
    let refused_adoption = run(
        root,
        &[
            "prefix",
            "create",
            "--id",
            "p028-unregistered",
            "--scope",
            "profile",
            "--profile",
            "fixture-profile",
            "--expected-revision",
            "7",
        ],
    );
    assert_eq!(refused_adoption.status.code(), Some(4));
    assert!(unregistered_path.is_dir());
    assert_eq!(
        run(
            root,
            &["runtime", "pin", "resolve", "--profile", "fixture-profile"]
        )
        .status
        .code(),
        Some(0)
    );

    let removed_profile = run(
        root,
        &[
            "runtime",
            "pin",
            "remove",
            "--scope",
            "profile",
            "--profile",
            "fixture-profile",
            "--expected-revision",
            "2",
        ],
    );
    assert_eq!(
        removed_profile.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&removed_profile.stdout)
    );
    assert!(
        managed.exists(),
        "removing a pin must preserve runtime files"
    );
    assert_eq!(
        run(root, &["runtime", "pin", "resolve"]).status.code(),
        Some(0)
    );

    // External candidates are selected from the actual P022 discovery output.
    // The candidate ID is used only to make this explicit choice; the pin
    // stores the canonical location and a separately revalidated structure.
    let external_root = root.join("external-runtimes");
    let external_build = external_root.join("GE-Proton-P027-fixture");
    fs::create_dir_all(&external_build).unwrap();
    fs::write(external_build.join("proton"), b"synthetic inert launcher").unwrap();
    fs::set_permissions(
        external_build.join("proton"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    fs::write(external_build.join("version"), b"P027 external fixture\n").unwrap();
    let external_scan = run(
        root,
        &[
            "diagnose",
            "runtimes",
            "--root",
            external_root.to_str().unwrap(),
        ],
    );
    assert_eq!(
        external_scan.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&external_scan.stdout)
    );
    let scan_text = String::from_utf8_lossy(&external_scan.stdout);
    let external_block = scan_text
        .split("\nRuntime ")
        .find(|block| block.contains("Local label: GE-Proton-P027-fixture"))
        .expect("synthetic external candidate appears in P022 discovery");
    let candidate_id = external_block
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("Candidate ID: ")
                .and_then(|value| value.split_whitespace().next())
        })
        .expect("discovery prints its transient candidate ID");
    let external_set_args = vec![
        "runtime",
        "pin",
        "set-external",
        "--candidate-id",
        candidate_id,
        "--scope",
        "profile",
        "--profile",
        "external-fixture",
        "--root",
        external_root.to_str().unwrap(),
        "--expected-revision",
        "3",
    ];
    let external_set = run(root, &external_set_args);
    assert_eq!(
        external_set.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&external_set.stdout)
    );
    let external_resolve = run(
        root,
        &["runtime", "pin", "resolve", "--profile", "external-fixture"],
    );
    assert_eq!(
        external_resolve.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&external_resolve.stdout)
    );
    let external_resolution = String::from_utf8_lossy(&external_resolve.stdout);
    assert!(external_resolution.contains("P022 structural identity revalidated"));
    assert!(external_resolution.contains("external source authenticity unverified"));
    assert!(external_resolution.contains("Compatibility: not verified"));
    assert!(!external_resolution.contains(external_root.to_str().unwrap()));

    let external_prefix_path = root.join("data/endfield-unix/prefixes/data/p028-external");
    let external_prefix_plan = run(
        root,
        &[
            "prefix",
            "plan",
            "--id",
            "p028-external",
            "--scope",
            "profile",
            "--profile",
            "external-fixture",
            "--expected-revision",
            "7",
        ],
    );
    println!(
        "Prefix external-runtime plan output:\n{}",
        String::from_utf8_lossy(&external_prefix_plan.stdout)
    );
    assert_eq!(external_prefix_plan.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&external_prefix_plan.stdout).contains("ProfilePin"));
    assert!(
        !String::from_utf8_lossy(&external_prefix_plan.stdout)
            .contains(external_root.to_str().unwrap())
    );
    let external_prefix_create = run(
        root,
        &[
            "prefix",
            "create",
            "--id",
            "p028-external",
            "--scope",
            "profile",
            "--profile",
            "external-fixture",
            "--expected-revision",
            "7",
        ],
    );
    println!(
        "Prefix external-runtime create output:\n{}",
        String::from_utf8_lossy(&external_prefix_create.stdout)
    );
    assert_eq!(
        external_prefix_create.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&external_prefix_create.stdout)
    );
    assert!(external_prefix_path.is_dir());
    let changed_pin = run(
        root,
        &[
            "runtime",
            "pin",
            "set",
            "--scope",
            "profile",
            "--profile",
            "external-fixture",
            "--installation-id",
            installation_id,
            "--expected-revision",
            "4",
            "--replace",
        ],
    );
    assert_eq!(changed_pin.status.code(), Some(0));
    let retained_prefix = run(root, &["prefix", "validate", "--id", "p028-external"]);
    println!(
        "Prefix after pin change output:\n{}",
        String::from_utf8_lossy(&retained_prefix.stdout)
    );
    assert_eq!(
        retained_prefix.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&retained_prefix.stdout)
    );
    let retained_prefix_output = String::from_utf8_lossy(&retained_prefix.stdout);
    assert!(
        retained_prefix_output
            .contains("Current P027 pin: changed; existing prefix association is preserved")
    );
    assert!(
        retained_prefix_output.contains("Runtime association: available; identity revalidated")
    );
    assert!(!retained_prefix_output.contains(external_root.to_str().unwrap()));
    let restore_external_pin = run(
        root,
        &[
            "runtime",
            "pin",
            "set-external",
            "--candidate-id",
            candidate_id,
            "--scope",
            "profile",
            "--profile",
            "external-fixture",
            "--root",
            external_root.to_str().unwrap(),
            "--expected-revision",
            "5",
            "--replace",
        ],
    );
    assert_eq!(
        restore_external_pin.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&restore_external_pin.stdout)
    );
    fs::write(
        external_build.join("version"),
        b"P027 external replacement\n",
    )
    .unwrap();
    let stale_external = run(
        root,
        &["runtime", "pin", "resolve", "--profile", "external-fixture"],
    );
    assert_eq!(stale_external.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&stale_external.stdout).contains("identity changed"));
    assert!(
        String::from_utf8_lossy(&stale_external.stdout)
            .contains("Automatic fallback: not performed")
    );
    assert!(String::from_utf8_lossy(&stale_external.stdout).contains("Stored pin: preserved"));
    let stale_association = run(root, &["prefix", "validate", "--id", "p028-external"]);
    println!(
        "Prefix after external runtime change output:\n{}",
        String::from_utf8_lossy(&stale_association.stdout)
    );
    assert_eq!(stale_association.status.code(), Some(4));
    let stale_association_output = String::from_utf8_lossy(&stale_association.stdout);
    assert!(stale_association_output.contains("Runtime association: stale or unavailable"));
    assert!(
        stale_association_output
            .contains("Runtime validation: stale or unavailable; P022 structural identity changed")
    );
    assert!(stale_association_output.contains("identity changed"));
    assert!(!stale_association_output.contains(external_root.to_str().unwrap()));
    assert!(external_prefix_path.is_dir());
    let delete_stale_association = run(
        root,
        &[
            "prefix",
            "delete",
            "--id",
            "p028-external",
            "--expected-revision",
            "9",
            "--confirm-path",
            external_prefix_path.to_str().unwrap(),
        ],
    );
    println!(
        "Prefix deletion with stale runtime output:\n{}",
        String::from_utf8_lossy(&delete_stale_association.stdout)
    );
    assert_eq!(
        delete_stale_association.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&delete_stale_association.stdout)
    );
    assert!(!external_prefix_path.exists());
    assert!(external_build.join("proton").is_file());
    let removed_external = run(
        root,
        &[
            "runtime",
            "pin",
            "remove",
            "--scope",
            "profile",
            "--profile",
            "external-fixture",
            "--expected-revision",
            "6",
        ],
    );
    assert_eq!(removed_external.status.code(), Some(0));
    assert!(external_build.join("proton").exists());
    assert_eq!(
        run(root, &["runtime", "pin", "resolve"]).status.code(),
        Some(0)
    );

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
    assert!(String::from_utf8_lossy(&changed_audit.stdout).contains("Local baseline: mismatch"));
    assert!(
        String::from_utf8_lossy(&changed_audit.stdout).contains("Audit completeness: complete")
    );
    let stale_pin = run(root, &["runtime", "pin", "resolve"]);
    assert_eq!(stale_pin.status.code(), Some(4));
    assert!(
        String::from_utf8_lossy(&stale_pin.stdout)
            .contains("P026 detected a local baseline mismatch")
    );
    assert!(
        String::from_utf8_lossy(&stale_pin.stdout).contains("Automatic fallback: not performed")
    );
    assert!(String::from_utf8_lossy(&stale_pin.stdout).contains("Stored pin: preserved"));
    assert_eq!(
        fs::read(managed.join("version")).unwrap(),
        b"changed locally"
    );
    let source_changed = run(
        root,
        &source_audit_args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    assert_eq!(source_changed.status.code(), Some(4));
    assert!(
        String::from_utf8_lossy(&source_changed.stdout).contains("Source comparison: MISMATCH")
    );
    assert!(
        String::from_utf8_lossy(&source_changed.stdout)
            .contains("Source provenance: user-pinned (limited trust)")
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
