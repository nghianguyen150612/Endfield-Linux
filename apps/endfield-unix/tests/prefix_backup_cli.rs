//! Exercise the canonical executable and parser, never a substitute CLI harness.
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::{Command, Output},
};
fn run(root: &Path, args: &[&str]) -> Output {
    let result = Command::new(env!("CARGO_BIN_EXE_endfield-unix"))
        .args(args)
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .output()
        .unwrap();
    println!(
        "{args:?}: exit {:?}\nstdout: {}\nstderr: {}",
        result.status.code(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    result
}
fn expect(root: &Path, args: &[&str], code: i32) -> Output {
    let r = run(root, args);
    assert_eq!(r.status.code(), Some(code), "{args:?}");
    // Preserve the existing canonical dispatch contract: operation diagnostics
    // are on stdout and nonzero operation status adds this stderr summary.
    let stderr = if code == 0 {
        ""
    } else {
        "endfield-unix: runtime verification completed; see report.\n"
    };
    assert_eq!(String::from_utf8_lossy(&r.stderr), stderr, "{args:?}");
    r
}
fn json(root: &Path, args: &[&str]) -> Value {
    serde_json::from_slice(&expect(root, args, 0).stdout).unwrap()
}
#[test]
fn every_documented_p029_command_is_recognized_by_the_actual_parser() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    for (group, leaves) in [
        (
            "backup",
            &[
                "list",
                "pending",
                "deletions",
                "deletion-plan",
                "deletion-resume",
                "deletion-journals",
                "plan",
                "create",
                "show",
                "verify",
                "recover",
                "delete",
            ][..],
        ),
        ("restore", &["plan", "apply"][..]),
        (
            "recovery",
            &[
                "status",
                "run",
                "release",
                "pin",
                "rollback-plan",
                "rollback",
                "history",
                "pins",
            ][..],
        ),
    ] {
        expect(root, &["prefix", group, "--help"], 0);
        for leaf in leaves {
            let r = expect(root, &["prefix", group, leaf, "--help"], 0);
            assert!(
                String::from_utf8_lossy(&r.stdout)
                    .contains(&format!("Usage: endfield-unix prefix {group} {leaf}"))
            );
        }
    }
    for leaf in ["list", "pending", "deletions", "deletion-journals"] {
        assert_eq!(
            json(root, &["prefix", "backup", leaf]),
            serde_json::json!([])
        );
    }
    expect(root, &["prefix", "recover", "--help"], 0);
    assert_eq!(
        json(root, &["prefix", "recovery", "history", "--id", "missing"]),
        serde_json::json!([])
    );
    assert_eq!(
        json(root, &["prefix", "recovery", "pins"]),
        serde_json::json!([])
    );
    expect(
        root,
        &["prefix", "recovery", "rollback-plan", "--id", "missing"],
        3,
    );
    expect(
        root,
        &[
            "prefix",
            "recovery",
            "rollback",
            "--id",
            "missing",
            "--expected-revision",
            "0",
            "--confirm",
            "wrong",
        ],
        3,
    );
    for args in [
        &[
            "prefix",
            "restore",
            "apply",
            "--id",
            "game",
            "--backup-id",
            "b",
            "--expected-revision",
            "2",
        ][..],
        &[
            "prefix",
            "recovery",
            "pin",
            "--backup-id",
            "b",
            "--confirm",
            "pin:b",
        ][..],
    ] {
        let r = run(root, args);
        assert_eq!(r.status.code(), Some(2));
        assert!(!r.stderr.is_empty());
    }
    assert!(!root.join("data").exists());
    assert!(!root.join("config").exists());
    assert!(!root.join("cache").exists());
}
#[test]
fn actual_backup_restore_recovery_and_pin_cas_or_documented_use_refusal() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let runtimes = root.join("external-runtimes");
    let runtime = runtimes.join("GE-Proton-P029A-fixture");
    fs::create_dir_all(&runtime).unwrap();
    fs::write(
        runtime.join("proton"),
        b"#!/bin/sh\nprintf called > \"$0.executed\"\nexit 99\n",
    )
    .unwrap();
    fs::set_permissions(runtime.join("proton"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(runtime.join("version"), b"P029A inert fixture\n").unwrap();
    let scan = expect(
        root,
        &["diagnose", "runtimes", "--root", runtimes.to_str().unwrap()],
        0,
    );
    let text = String::from_utf8(scan.stdout).unwrap();
    let block = text
        .split("\nRuntime ")
        .find(|b| b.contains("Local label: GE-Proton-P029A-fixture"))
        .unwrap();
    let candidate = block
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("Candidate ID: ")
                .and_then(|v| v.split_whitespace().next())
        })
        .unwrap();
    expect(
        root,
        &[
            "runtime",
            "pin",
            "set-external",
            "--candidate-id",
            candidate,
            "--scope",
            "machine",
            "--root",
            runtimes.to_str().unwrap(),
            "--expected-revision",
            "0",
        ],
        0,
    );
    expect(
        root,
        &[
            "prefix",
            "create",
            "--id",
            "game",
            "--scope",
            "machine",
            "--expected-revision",
            "0",
        ],
        0,
    );
    let prefix = root.join("data/endfield-unix/prefixes/data/game");
    fs::write(prefix.join("snapshot.bin"), b"\0\xffroundtrip").unwrap();
    let pin_path = root.join("config/endfield-unix/runtime-pins.json");
    let pin_bytes = fs::read(&pin_path).unwrap();
    expect(
        root,
        &[
            "prefix",
            "backup",
            "create",
            "--id",
            "game",
            "--expected-revision",
            "2",
        ],
        4,
    );
    assert!(!root.join("data/endfield-unix/backups").exists());
    let result = run(
        root,
        &[
            "prefix",
            "backup",
            "create",
            "--id",
            "game",
            "--expected-revision",
            "2",
            "--confirm",
        ],
    );
    if result.status.code() == Some(4) {
        assert_eq!(
            String::from_utf8_lossy(&result.stderr),
            "endfield-unix: runtime verification completed; see report.\n"
        );
        let msg = String::from_utf8_lossy(&result.stdout);
        assert!(
            msg.contains("prefix use could not be checked safely")
                || msg.contains("actively referenced"),
            "{msg}"
        );
        assert!(!root.join("data/endfield-unix/backups").exists());
        assert_eq!(fs::read(&pin_path).unwrap(), pin_bytes);
        assert!(!runtime.join("proton.executed").exists());
        return;
    }
    assert_eq!(result.status.code(), Some(0));
    assert!(result.stderr.is_empty());
    let m: Value = serde_json::from_slice(&result.stdout).unwrap();
    let id = m["id"].as_str().unwrap();
    json(root, &["prefix", "backup", "show", "--backup-id", id]);
    json(root, &["prefix", "backup", "verify", "--backup-id", id]);
    let plan = json(
        root,
        &[
            "prefix",
            "restore",
            "plan",
            "--id",
            "game",
            "--backup-id",
            id,
        ],
    );
    assert_eq!(plan["current_selection"]["state"], "Selected");
    assert_eq!(plan["current_selection"]["matches_saved_association"], true);
    assert_eq!(plan["current_selection"]["saved_runtime_valid"], true);
    assert_eq!(fs::read(&pin_path).unwrap(), pin_bytes);
    fs::write(prefix.join("preserve"), b"original stays").unwrap();
    expect(
        root,
        &[
            "runtime",
            "pin",
            "remove",
            "--scope",
            "machine",
            "--expected-revision",
            "1",
        ],
        0,
    );
    let pin_removed = fs::read(&pin_path).unwrap();
    let plan = json(
        root,
        &[
            "prefix",
            "restore",
            "plan",
            "--id",
            "game",
            "--backup-id",
            id,
        ],
    );
    assert_eq!(plan["current_selection"]["state"], "NoPin");
    assert_eq!(plan["current_selection"]["pin_registry_revision"], 2);
    assert_eq!(plan["backup_runtime"], m["source"]["runtime"]);
    let confirmation = plan["confirmation"].as_str().unwrap();
    expect(
        root,
        &[
            "prefix",
            "restore",
            "apply",
            "--id",
            "game",
            "--backup-id",
            id,
            "--expected-revision",
            "2",
            "--confirm",
            "wrong",
        ],
        4,
    );
    expect(
        root,
        &[
            "prefix",
            "restore",
            "apply",
            "--id",
            "game",
            "--backup-id",
            id,
            "--expected-revision",
            "3",
            "--confirm",
            confirmation,
        ],
        4,
    );
    assert!(!root.join("data/endfield-unix/backups/restores").exists());
    json(root, &["prefix", "recovery", "status", "--id", "game"]);
    expect(
        root,
        &[
            "prefix",
            "recovery",
            "run",
            "--id",
            "game",
            "--expected-revision",
            "2",
            "--confirm",
        ],
        4,
    );
    json(
        root,
        &[
            "prefix",
            "restore",
            "apply",
            "--id",
            "game",
            "--backup-id",
            id,
            "--expected-revision",
            "2",
            "--confirm",
            confirmation,
        ],
    );
    assert_eq!(
        fs::read(prefix.join("snapshot.bin")).unwrap(),
        b"\0\xffroundtrip"
    );
    assert!(!prefix.join("preserve").exists());
    let tx = root.join("data/endfield-unix/backups/restores/game");
    assert_eq!(
        fs::read(tx.join("payload/preserve")).unwrap(),
        b"original stays"
    );
    let status = json(root, &["prefix", "recovery", "status", "--id", "game"]);
    assert_eq!(status["action"], "None");
    assert_eq!(status["original_preserved"], true);
    expect(
        root,
        &[
            "prefix",
            "recovery",
            "run",
            "--id",
            "game",
            "--expected-revision",
            "3",
        ],
        4,
    );
    expect(
        root,
        &[
            "prefix",
            "recovery",
            "run",
            "--id",
            "game",
            "--expected-revision",
            "2",
            "--confirm",
        ],
        4,
    );
    json(
        root,
        &[
            "prefix",
            "recovery",
            "run",
            "--id",
            "game",
            "--expected-revision",
            "3",
            "--confirm",
        ],
    );
    let registry = root.join("data/endfield-unix/prefixes/registry.json");
    let registry_bytes = fs::read(&registry).unwrap();
    let marker = fs::read(tx.join("completed.json")).unwrap();
    fs::write(tx.join("completed.json"), b"{}").unwrap();
    expect(
        root,
        &[
            "prefix",
            "recovery",
            "run",
            "--id",
            "game",
            "--expected-revision",
            "3",
            "--confirm",
        ],
        4,
    );
    assert_eq!(registry_bytes, fs::read(&registry).unwrap());
    fs::write(tx.join("completed.json"), marker).unwrap();
    expect(
        root,
        &[
            "prefix",
            "recovery",
            "release",
            "--id",
            "game",
            "--expected-revision",
            "3",
            "--confirm",
            "wrong",
        ],
        4,
    );
    expect(
        root,
        &[
            "prefix",
            "recovery",
            "release",
            "--id",
            "game",
            "--expected-revision",
            "3",
            "--confirm",
            "release:game",
        ],
        0,
    );
    assert_eq!(pin_removed, fs::read(&pin_path).unwrap());
    let observed = json(root, &["prefix", "recovery", "history", "--id", "game"]);
    assert_eq!(observed.as_array().unwrap().len(), 1);
    assert_eq!(observed[0]["outcome"], "Restore");
    let history = root.join("data/endfield-unix/backups/history");
    let originals = fs::read_dir(&history)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(originals.len(), 1);
    assert_eq!(
        fs::read(originals[0].path().join("payload/preserve")).unwrap(),
        b"original stays"
    );
    let registry_state: Value = serde_json::from_slice(&fs::read(&registry).unwrap()).unwrap();
    assert_eq!(
        registry_state["records"][0]["lifecycle"]["state"],
        "created"
    );
    assert!(registry_state["records"][0]["initialization_evidence"].is_null());
    let second = json(
        root,
        &[
            "prefix",
            "backup",
            "create",
            "--id",
            "game",
            "--expected-revision",
            "3",
            "--confirm",
        ],
    );
    let second_id = second["id"].as_str().unwrap();
    json(
        root,
        &["prefix", "backup", "verify", "--backup-id", second_id],
    );
    expect(
        root,
        &[
            "prefix",
            "backup",
            "delete",
            "--backup-id",
            second_id,
            "--confirm",
            "wrong",
        ],
        4,
    );
    expect(
        root,
        &[
            "prefix",
            "backup",
            "delete",
            "--backup-id",
            second_id,
            "--confirm",
            &format!("delete:{second_id}"),
        ],
        0,
    );
    let pin_confirm = format!("pin:{id}");
    expect(
        root,
        &[
            "prefix",
            "recovery",
            "pin",
            "--backup-id",
            id,
            "--expected-pin-revision",
            "1",
            "--confirm",
            &pin_confirm,
        ],
        4,
    );
    assert_eq!(pin_removed, fs::read(&pin_path).unwrap());
    assert_eq!(
        json(
            root,
            &[
                "prefix",
                "recovery",
                "pin",
                "--backup-id",
                id,
                "--expected-pin-revision",
                "2",
                "--confirm",
                &pin_confirm
            ]
        ),
        3
    );
    assert_eq!(
        json(
            root,
            &[
                "prefix",
                "recovery",
                "pin",
                "--backup-id",
                id,
                "--expected-pin-revision",
                "2",
                "--confirm",
                &pin_confirm
            ]
        ),
        3
    );
    let pin_journals = json(root, &["prefix", "recovery", "pins"]);
    assert_eq!(pin_journals.as_array().unwrap().len(), 2);
    assert_eq!(pin_journals[0]["state"], "Pending");
    assert_eq!(pin_journals[1]["state"], "Completed");
    assert_eq!(
        pin_journals[1]["validation"],
        "journal marker only; current pin state not assessed"
    );
    expect(
        root,
        &[
            "runtime",
            "pin",
            "remove",
            "--scope",
            "machine",
            "--expected-revision",
            "3",
        ],
        0,
    );
    let after_remove = fs::read(&pin_path).unwrap();
    expect(
        root,
        &[
            "prefix",
            "recovery",
            "pin",
            "--backup-id",
            id,
            "--expected-pin-revision",
            "2",
            "--confirm",
            &pin_confirm,
        ],
        4,
    );
    assert_eq!(after_remove, fs::read(&pin_path).unwrap());
    fs::write(runtime.join("proton"), b"modified inert runtime").unwrap();
    expect(
        root,
        &[
            "prefix",
            "recovery",
            "pin",
            "--backup-id",
            id,
            "--expected-pin-revision",
            "4",
            "--confirm",
            &pin_confirm,
        ],
        4,
    );
    assert_eq!(after_remove, fs::read(&pin_path).unwrap());
    // Simulate an interrupted deletion with exact app-owned fixture paths.
    let backups = root.join("data/endfield-unix/backups");
    let deleting = backups.join("deleting");
    fs::rename(backups.join("snapshots").join(id), deleting.join(id)).unwrap();
    let retired = deleting.join(id);
    fs::remove_file(retired.join("payload/snapshot.bin")).unwrap();
    let inspection = json(root, &["prefix", "backup", "deletions"]);
    assert_eq!(inspection[0]["state"], "Incomplete");
    symlink("/", retired.join("payload/escape")).unwrap();
    let inspection = json(root, &["prefix", "backup", "deletions"]);
    assert_eq!(inspection[0]["state"], "Unsafe");
    assert_eq!(
        fs::read(prefix.join("snapshot.bin")).unwrap(),
        b"\0\xffroundtrip"
    );
    assert!(!runtime.join("proton.executed").exists());
}

#[test]
fn actual_cli_empty_snapshot_deletion_plan_and_idempotent_resume() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let runtimes = root.join("runtimes");
    let runtime = runtimes.join("GE-Proton-P029B-fixture");
    fs::create_dir_all(&runtime).unwrap();
    fs::write(
        runtime.join("proton"),
        b"#!/bin/sh\nprintf called > \"$0.executed\"\nexit 99\n",
    )
    .unwrap();
    fs::set_permissions(runtime.join("proton"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(runtime.join("version"), b"P029B inert fixture\n").unwrap();
    let scan = expect(
        root,
        &["diagnose", "runtimes", "--root", runtimes.to_str().unwrap()],
        0,
    );
    let output = String::from_utf8(scan.stdout).unwrap();
    let block = output
        .split("\nRuntime ")
        .find(|b| b.contains("Local label: GE-Proton-P029B-fixture"))
        .unwrap();
    let candidate = block
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("Candidate ID: ")
                .and_then(|v| v.split_whitespace().next())
        })
        .unwrap();
    expect(
        root,
        &[
            "runtime",
            "pin",
            "set-external",
            "--candidate-id",
            candidate,
            "--scope",
            "machine",
            "--root",
            runtimes.to_str().unwrap(),
            "--expected-revision",
            "0",
        ],
        0,
    );
    expect(
        root,
        &[
            "prefix",
            "create",
            "--id",
            "game",
            "--scope",
            "machine",
            "--expected-revision",
            "0",
        ],
        0,
    );
    let pin_path = root.join("config/endfield-unix/runtime-pins.json");
    let pin_bytes = fs::read(&pin_path).unwrap();
    let backup = run(
        root,
        &[
            "prefix",
            "backup",
            "create",
            "--id",
            "game",
            "--expected-revision",
            "2",
            "--confirm",
        ],
    );
    if backup.status.code() == Some(4) {
        assert_eq!(
            String::from_utf8_lossy(&backup.stderr),
            "endfield-unix: runtime verification completed; see report.\n"
        );
        let msg = String::from_utf8_lossy(&backup.stdout);
        assert!(
            msg.contains("prefix use could not be checked safely")
                || msg.contains("actively referenced"),
            "{msg}"
        );
        assert!(!root.join("data/endfield-unix/backups").exists());
        assert_eq!(fs::read(pin_path).unwrap(), pin_bytes);
        return;
    }
    assert_eq!(backup.status.code(), Some(0));
    assert!(backup.stderr.is_empty());
    let manifest: Value = serde_json::from_slice(&backup.stdout).unwrap();
    let id = manifest["id"].as_str().unwrap();
    expect(
        root,
        &[
            "prefix",
            "backup",
            "delete",
            "--backup-id",
            id,
            "--confirm",
            &format!("delete:{id}"),
        ],
        0,
    );
    let plan = json(
        root,
        &["prefix", "backup", "deletion-plan", "--backup-id", id],
    );
    assert_eq!(plan["state"], "Completed");
    let journals = json(root, &["prefix", "backup", "deletion-journals"]);
    assert_eq!(journals[0]["state"], "Completed");
    let token = plan["confirmation"].as_str().unwrap();
    expect(
        root,
        &[
            "prefix",
            "backup",
            "deletion-resume",
            "--backup-id",
            id,
            "--expected-revision",
            "2",
            "--confirm",
            "wrong",
        ],
        4,
    );
    let resumed = json(
        root,
        &[
            "prefix",
            "backup",
            "deletion-resume",
            "--backup-id",
            id,
            "--expected-revision",
            "2",
            "--confirm",
            token,
        ],
    );
    assert_eq!(resumed["state"], "Completed");
    let resumed = json(
        root,
        &[
            "prefix",
            "backup",
            "deletion-resume",
            "--backup-id",
            id,
            "--expected-revision",
            "2",
            "--confirm",
            token,
        ],
    );
    assert_eq!(resumed["state"], "Completed");
    assert_eq!(fs::read(pin_path).unwrap(), pin_bytes);
    assert!(root.join("data/endfield-unix/prefixes/data/game").is_dir());
    assert!(!runtime.join("proton.executed").exists());
}

#[test]
fn actual_cli_rolls_back_a_synthetic_published_uncommitted_transaction() {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let runtimes = root.join("runtimes");
    let runtime = runtimes.join("GE-Proton-P029B-rollback-fixture");
    fs::create_dir_all(&runtime).unwrap();
    fs::write(
        runtime.join("proton"),
        b"#!/bin/sh\nprintf called > \"$0.executed\"\nexit 99\n",
    )
    .unwrap();
    fs::set_permissions(runtime.join("proton"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(runtime.join("version"), b"rollback fixture\n").unwrap();
    let scan = expect(
        root,
        &["diagnose", "runtimes", "--root", runtimes.to_str().unwrap()],
        0,
    );
    let text = String::from_utf8(scan.stdout).unwrap();
    let block = text
        .split("\nRuntime ")
        .find(|b| b.contains("Local label: GE-Proton-P029B-rollback-fixture"))
        .unwrap();
    let candidate = block
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("Candidate ID: ")
                .and_then(|v| v.split_whitespace().next())
        })
        .unwrap();
    expect(
        root,
        &[
            "runtime",
            "pin",
            "set-external",
            "--candidate-id",
            candidate,
            "--scope",
            "machine",
            "--root",
            runtimes.to_str().unwrap(),
            "--expected-revision",
            "0",
        ],
        0,
    );
    expect(
        root,
        &[
            "prefix",
            "create",
            "--id",
            "game",
            "--scope",
            "machine",
            "--expected-revision",
            "0",
        ],
        0,
    );
    let app_data = root.join("data/endfield-unix");
    let live = app_data.join("prefixes/data/game");
    fs::write(live.join("save"), b"snapshot").unwrap();
    let backup = run(
        root,
        &[
            "prefix",
            "backup",
            "create",
            "--id",
            "game",
            "--expected-revision",
            "2",
            "--confirm",
        ],
    );
    if backup.status.code() == Some(4) {
        assert_eq!(
            String::from_utf8_lossy(&backup.stderr),
            "endfield-unix: runtime verification completed; see report.\n"
        );
        let msg = String::from_utf8_lossy(&backup.stdout);
        assert!(
            msg.contains("prefix use could not be checked safely")
                || msg.contains("actively referenced"),
            "{msg}"
        );
        assert!(!app_data.join("backups").exists());
        return;
    }
    assert_eq!(backup.status.code(), Some(0));
    assert!(backup.stderr.is_empty());
    let backup: Value = serde_json::from_slice(&backup.stdout).unwrap();
    let backup_id = backup["id"].as_str().unwrap();
    fs::write(live.join("save"), b"original").unwrap();
    let original = json(
        root,
        &[
            "prefix",
            "backup",
            "create",
            "--id",
            "game",
            "--expected-revision",
            "2",
            "--confirm",
        ],
    );
    let registry = app_data.join("prefixes/registry.json");
    let registry_bytes = fs::read(&registry).unwrap();
    let registry_json: Value = serde_json::from_slice(&registry_bytes).unwrap();
    let record = registry_json["records"][0].clone();
    let pin = root.join("config/endfield-unix/runtime-pins.json");
    let pin_bytes = fs::read(&pin).unwrap();
    let restores = app_data.join("backups/restores");
    fs::create_dir(&restores).unwrap();
    fs::set_permissions(&restores, fs::Permissions::from_mode(0o700)).unwrap();
    let tx = restores.join("game");
    fs::create_dir(&tx).unwrap();
    fs::set_permissions(&tx, fs::Permissions::from_mode(0o700)).unwrap();
    let payload = tx.join("payload");
    fs::create_dir(&payload).unwrap();
    fs::set_permissions(&payload, fs::Permissions::from_mode(0o700)).unwrap();
    let source = app_data
        .join("backups/snapshots")
        .join(backup_id)
        .join("payload/save");
    let dest = payload.join("save");
    fs::copy(&source, &dest).unwrap();
    let source_metadata = fs::metadata(&source).unwrap();
    let dest_file = fs::File::open(&dest).unwrap();
    let times = [
        libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_OMIT,
        },
        libc::timespec {
            tv_sec: source_metadata.mtime(),
            tv_nsec: source_metadata.mtime_nsec(),
        },
    ];
    assert_eq!(
        unsafe { libc::futimens(dest_file.as_raw_fd(), times.as_ptr()) },
        0
    );
    let staged = fs::metadata(&payload).unwrap();
    let intent_path = tx.join("intent.json");
    fs::write(&intent_path, serde_json::to_vec(&record).unwrap()).unwrap();
    fs::set_permissions(&intent_path, fs::Permissions::from_mode(0o600)).unwrap();
    let journal = serde_json::json!({"schema_version":2,"backup":backup_id,"original":record,"expected_revision":2,"staged":{"device":staged.dev(),"inode":staged.ino()},"original_inventory":original["inventory"]});
    let journal_path = tx.join("staged.json");
    fs::write(&journal_path, serde_json::to_vec(&journal).unwrap()).unwrap();
    fs::set_permissions(&journal_path, fs::Permissions::from_mode(0o600)).unwrap();
    // Construct the exact post-exchange/pre-registry-commit state in this isolated fixture.
    let a = CString::new(payload.as_os_str().as_bytes()).unwrap();
    let b = CString::new(live.as_os_str().as_bytes()).unwrap();
    assert_eq!(
        unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                libc::RENAME_EXCHANGE,
            )
        },
        0
    );
    let plan = json(
        root,
        &["prefix", "recovery", "rollback-plan", "--id", "game"],
    );
    let token = plan["confirmation"].as_str().unwrap();
    expect(
        root,
        &[
            "prefix",
            "recovery",
            "rollback",
            "--id",
            "game",
            "--expected-revision",
            "2",
            "--confirm",
            "wrong",
        ],
        4,
    );
    json(
        root,
        &[
            "prefix",
            "recovery",
            "rollback",
            "--id",
            "game",
            "--expected-revision",
            "2",
            "--confirm",
            token,
        ],
    );
    json(
        root,
        &[
            "prefix",
            "recovery",
            "rollback",
            "--id",
            "game",
            "--expected-revision",
            "2",
            "--confirm",
            token,
        ],
    );
    assert_eq!(
        json(root, &["prefix", "recovery", "status", "--id", "game"])["action"],
        "RollbackCompleted"
    );
    assert_eq!(fs::read(live.join("save")).unwrap(), b"original");
    assert_eq!(fs::read(tx.join("payload/save")).unwrap(), b"snapshot");
    assert_eq!(fs::read(&registry).unwrap(), registry_bytes);
    assert_eq!(fs::read(&pin).unwrap(), pin_bytes);
    expect(
        root,
        &[
            "prefix",
            "recovery",
            "release",
            "--id",
            "game",
            "--expected-revision",
            "2",
            "--confirm",
            "release:game",
        ],
        0,
    );
    let history = json(root, &["prefix", "recovery", "history", "--id", "game"]);
    assert_eq!(history[0]["outcome"], "Rollback");
    assert!(!runtime.join("proton.executed").exists());
}
