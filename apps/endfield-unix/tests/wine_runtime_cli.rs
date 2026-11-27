use std::{fs, process::Command};
#[test]
fn wine_qualification_is_read_only_and_never_claims_authentic_success() {
    let root = tempfile::tempdir().unwrap();
    let preserved = root.path().join("unrelated-game.bin");
    fs::write(&preserved, b"preserved").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_endfield-unix"))
        .args(["diagnose", "wine-runtime", "--profile", "desktop", "--json"])
        .env("HOME", root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_DATA_HOME", root.path().join("data"))
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.len() < 1024 * 1024);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 2);
    assert_eq!(report["authentic_qualification"], "not_run");
    assert_eq!(report["production_initialization"], "blocked");
    assert_eq!(report["manifest_authority_scope"], "unknown");
    assert_eq!(report["manifest_signature_state"], "unavailable");
    assert_eq!(report["runtime_image_provider"], "squashfuse");
    assert_eq!(report["runtime_image_sha256"], serde_json::Value::Null);
    assert_eq!(report["mounted_view_eligibility"], "blocked");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(root.path().to_str().unwrap()));
    for path in ["config", "data", "cache", ".wine"] {
        assert!(!root.path().join(path).exists());
    }
    assert_eq!(fs::read(preserved).unwrap(), b"preserved");
}
