use std::{path::PathBuf, process::Command};
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/runtime-verification/tests/fixtures")
}
fn run(manifest: &std::path::Path, artifact: &std::path::Path) -> std::process::Output {
    let f = fixture();
    Command::new(env!("CARGO_BIN_EXE_endfield-unix"))
        .args(["diagnose", "runtime-integrity", "--manifest"])
        .arg(manifest)
        .arg("--artifact")
        .arg(artifact)
        .arg("--signature")
        .arg(f.join("manifest.sig"))
        .arg("--trust-key")
        .arg(f.join("public-key.bin"))
        .args([
            "--provider",
            "synthetic-provider",
            "--project",
            "synthetic-project",
            "--channel",
            "test",
            "--family",
            "proton",
            "--lineage",
            "ge-proton",
            "--build",
            "synthetic-build",
            "--artifact-id",
            "synthetic-artifact",
            "--key-id",
            "synthetic-key",
        ])
        .output()
        .unwrap()
}
#[test]
fn actual_headless_cli_reports_limited_trust_and_rejects_tampering() {
    let f = fixture();
    let out = run(&f.join("manifest.json"), &f.join("runtime.tar.gz"));
    assert_eq!(out.status.code(), Some(3));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("SHA-256: matches"));
    assert!(text.contains("Official publisher authenticity: UNVERIFIED"));
    let d = tempfile::tempdir().unwrap();
    let m = d.path().join("tampered.json");
    let mut b = std::fs::read(f.join("manifest.json")).unwrap();
    b.push(b' ');
    std::fs::write(&m, b).unwrap();
    assert_eq!(run(&m, &f.join("runtime.tar.gz")).status.code(), Some(4));
    let a = d.path().join("runtime.tar.gz");
    let mut b = std::fs::read(f.join("runtime.tar.gz")).unwrap();
    b[0] ^= 1;
    std::fs::write(&a, b).unwrap();
    assert_eq!(run(&f.join("manifest.json"), &a).status.code(), Some(5));
}
