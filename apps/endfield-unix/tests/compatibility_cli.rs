//! Exercise compatibility diagnostics through the canonical application binary.
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_endfield-unix"))
        .args(args)
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .output()
        .expect("run actual endfield-unix binary")
}

fn json(root: &Path, args: &[&str]) -> Value {
    let output = run(root, args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(output.stdout.len() <= 1024 * 1024);
    serde_json::from_slice(&output.stdout).expect("versioned compatibility JSON")
}

fn candidate_id(root: &Path, runtime_root: &Path, label: &str) -> String {
    let runtime_root = runtime_root.to_str().unwrap();
    let output = run(root, &["diagnose", "runtimes", "--root", runtime_root]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let block = text
        .split("\nRuntime ")
        .find(|block| block.contains(&format!("Local label: {label}")))
        .expect("synthetic P022 runtime candidate was discovered");
    block
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("Candidate ID: ")
                .and_then(|value| value.split_whitespace().next())
        })
        .expect("candidate ID in actual discovery output")
        .to_owned()
}

fn make_external_proton(root: &Path, label: &str, version: &str) -> PathBuf {
    let runtime = root.join("runtimes").join(label);
    fs::create_dir_all(&runtime).unwrap();
    fs::write(runtime.join("proton"), b"synthetic inert Proton launcher").unwrap();
    fs::set_permissions(runtime.join("proton"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(runtime.join("version"), version).unwrap();
    runtime
}

fn set_external_pin(
    root: &Path,
    runtime_root: &Path,
    label: &str,
    scope: &str,
    profile: Option<&str>,
    revision: &str,
) {
    let candidate = candidate_id(root, runtime_root, label);
    let mut args = vec![
        "runtime",
        "pin",
        "set-external",
        "--candidate-id",
        &candidate,
        "--scope",
        scope,
    ];
    if let Some(profile) = profile {
        args.extend(["--profile", profile]);
    }
    args.extend([
        "--root",
        runtime_root.to_str().unwrap(),
        "--expected-revision",
        revision,
    ]);
    let output = run(root, &args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn empty_host_json_and_help_are_read_only_and_headless() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let report = json(root, &["diagnose", "compatibility", "--json"]);
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["game_compatibility"], "not_tested");
    assert_eq!(
        report["translation_layer_readiness"],
        "unknown_or_unverifiable"
    );
    assert!(report["runner"]["runtime"].is_null());
    assert_eq!(report["runner"]["selection"]["automatic_fallback"], false);
    assert!(
        report["compatibility_limitations"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(!String::from_utf8_lossy(&serde_json::to_vec(&report).unwrap()).contains("machine-id"));

    let human = run(root, &["diagnose", "compatibility"]);
    assert_eq!(human.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&human.stdout).contains("Game compatibility: NOT TESTED"));
    assert!(
        String::from_utf8_lossy(&human.stdout)
            .contains("Translation-layer readiness: UnknownOrUnverifiable")
    );
    assert!(human.stdout.len() <= 64 * 1024);

    let nested_help = run(root, &["diagnose", "compatibility", "--help"]);
    assert_eq!(nested_help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&nested_help.stdout).contains("--prefix-id"));
    let invalid = run(root, &["diagnose", "compatibility", "--unsupported"]);
    assert_eq!(invalid.status.code(), Some(2));

    for directory in ["home", "config", "data", "cache", "state"] {
        assert!(
            !root.join(directory).exists(),
            "read-only CLI created {directory}"
        );
    }
}

#[test]
fn actual_binary_observes_profile_precedence_without_fallback_and_created_prefix_state() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let machine_root = root.join("machine-runtimes");
    let profile_root = root.join("profile-runtimes");
    make_external_proton(
        &machine_root,
        "GE-Proton-machine-fixture",
        "P030 machine fixture\n",
    );
    let profile_runtime = make_external_proton(
        &profile_root,
        "GE-Proton-profile-fixture",
        "P030 profile fixture\n",
    );

    set_external_pin(
        root,
        &machine_root,
        "GE-Proton-machine-fixture",
        "machine",
        None,
        "0",
    );
    set_external_pin(
        root,
        &profile_root,
        "GE-Proton-profile-fixture",
        "profile",
        Some("fixture-profile"),
        "1",
    );
    fs::write(
        profile_runtime.join("version"),
        b"P030 changed profile fixture\n",
    )
    .unwrap();

    let stale_profile = json(
        root,
        &[
            "diagnose",
            "compatibility",
            "--profile",
            "fixture-profile",
            "--json",
        ],
    );
    assert_eq!(stale_profile["runner"]["selection"]["state"], "stale");
    assert_eq!(
        stale_profile["runner"]["selection"]["source"],
        "profile_pin"
    );
    assert_eq!(
        stale_profile["runner"]["selection"]["automatic_fallback"],
        false
    );
    let stale_identity = stale_profile["runner"]["runtime"]["exact_identity"]
        .as_str()
        .unwrap();
    assert!(stale_identity.starts_with("external:p022-structural:"));

    let machine_selection = json(root, &["diagnose", "compatibility", "--json"]);
    assert_eq!(
        machine_selection["runner"]["selection"]["state"],
        "selected"
    );
    assert_eq!(
        machine_selection["runner"]["selection"]["source"],
        "machine_pin"
    );
    assert_ne!(
        stale_identity,
        machine_selection["runner"]["runtime"]["exact_identity"]
            .as_str()
            .unwrap(),
        "the broken profile pin must not silently select the valid machine pin"
    );
    let serialized = serde_json::to_string(&stale_profile).unwrap();
    assert!(
        !serialized.contains(root.to_str().unwrap()),
        "report exposed a runtime path"
    );

    let missing_prefix = json(
        root,
        &[
            "diagnose",
            "compatibility",
            "--prefix-id",
            "not-registered",
            "--json",
        ],
    );
    assert_eq!(missing_prefix["prefix"]["lifecycle"], "missing");
    assert_eq!(
        missing_prefix["prefix"]["filesystem_state"],
        "missing_requirement"
    );

    let create = run(
        root,
        &[
            "prefix",
            "create",
            "--id",
            "p030-created",
            "--scope",
            "machine",
            "--expected-revision",
            "0",
        ],
    );
    assert_eq!(
        create.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&create.stdout)
    );
    assert!(String::from_utf8_lossy(&create.stdout).contains("created (uninitialized)"));
    let created = json(
        root,
        &[
            "diagnose",
            "compatibility",
            "--prefix-id",
            "p030-created",
            "--json",
        ],
    );
    assert_eq!(created["prefix"]["lifecycle"], "created");
    assert_eq!(
        created["prefix"]["initialization_state"],
        "unknown_or_unverifiable"
    );
    assert_eq!(
        created["prefix"]["initialization_evidence_available"],
        false
    );
    assert_ne!(created["verdict"], "no_known_static_blocker");
}
