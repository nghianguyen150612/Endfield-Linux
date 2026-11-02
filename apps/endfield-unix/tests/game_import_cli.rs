//! Exercise P031 through the canonical `endfield-unix` executable.
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
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

fn json(output: &Output, expected_status: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(expected_status),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.len() <= 1024 * 1024);
    serde_json::from_slice(&output.stdout).expect("versioned game-import JSON")
}

fn selected_game(root: &Path) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let game = root.join("Mounted Games/Arknights Endfield Δ");
    fs::create_dir_all(&game).unwrap();
    let binary = game.join("opaque-game-launcher.sh");
    fs::write(
        &binary,
        format!(
            "#!/bin/sh\nprintf executed > '{}'\n",
            root.join("executed").display()
        ),
    )
    .unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(game.join("payload.bin"), b"synthetic game bytes\0\x01").unwrap();
    (game, binary)
}

#[test]
fn list_plan_and_nested_help_leave_empty_xdg_storage_absent() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (game, _) = selected_game(root);
    let game_path = game.to_str().unwrap();

    let list = run(root, &["game", "list", "--json"]);
    let report = json(&list, 0);
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["registry_revision"], 0);
    assert_eq!(report["installations"].as_array().unwrap().len(), 0);

    let plan = run(
        root,
        &[
            "game",
            "import",
            "plan",
            "--path",
            game_path,
            "--id",
            "endfield-main",
            "--json",
        ],
    );
    let plan = json(&plan, 2);
    assert_eq!(plan["schema_version"], 1);
    assert_eq!(plan["can_register"], false);
    assert_eq!(plan["evidence"]["identity_confidence"], "not_established");
    assert_eq!(plan["evidence"]["publisher_authenticity_verified"], false);

    let help = run(root, &["game", "import", "plan", "--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help.stdout).contains("--declare-endfield"));

    for directory in ["home", "config", "data", "cache", "state"] {
        assert!(
            !root.join(directory).exists(),
            "read-only P031 operation created {directory}"
        );
    }
}

#[test]
fn actual_binary_import_validate_list_and_unregister_preserve_other_state_and_game_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let app_config = root.join("config/endfield-unix");
    fs::create_dir_all(&app_config).unwrap();
    fs::set_permissions(&app_config, fs::Permissions::from_mode(0o700)).unwrap();
    let app_data = root.join("data/endfield-unix");
    fs::create_dir_all(&app_data).unwrap();
    fs::set_permissions(&app_data, fs::Permissions::from_mode(0o700)).unwrap();
    let prefixes = app_data.join("prefixes");
    fs::create_dir_all(&prefixes).unwrap();
    fs::set_permissions(&prefixes, fs::Permissions::from_mode(0o700)).unwrap();
    let backups = app_data.join("backups");
    fs::create_dir_all(&backups).unwrap();
    fs::set_permissions(&backups, fs::Permissions::from_mode(0o700)).unwrap();
    let preserved_app_state = [
        (
            app_config.join("runtime-pins.json"),
            b"synthetic P027 pin bytes".as_slice(),
        ),
        (
            prefixes.join("registry.json"),
            b"synthetic P028 registry bytes".as_slice(),
        ),
        (
            backups.join("journal.json"),
            b"synthetic P029 journal bytes".as_slice(),
        ),
    ];
    for (path, bytes) in &preserved_app_state {
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    let (game, inert_launcher) = selected_game(root);
    let game_path = game.to_str().unwrap();
    let payload = game.join("payload.bin");
    let payload_before = fs::read(&payload).unwrap();
    let payload_metadata = fs::metadata(&payload).unwrap();
    let launcher_before = fs::read(&inert_launcher).unwrap();
    let launcher_metadata = fs::metadata(&inert_launcher).unwrap();

    let plan_output = run(
        root,
        &[
            "game",
            "import",
            "plan",
            "--path",
            game_path,
            "--id",
            "endfield-main",
            "--declare-endfield",
            "--json",
        ],
    );
    let plan = json(&plan_output, 0);
    assert_eq!(plan["can_register"], true);
    assert_eq!(plan["registry_revision"], 0);
    assert_eq!(
        plan["evidence"]["identity_confidence"],
        "user_declared_unverified"
    );
    assert_eq!(plan["evidence"]["publisher_authenticity_verified"], false);
    assert!(plan["evidence"]["game_build"].is_null());
    assert_eq!(
        plan["evidence"]["executable_candidates"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let confirmation = plan["required_confirmation"].as_str().unwrap();

    let rejected = run(
        root,
        &[
            "game",
            "import",
            "apply",
            "--path",
            game_path,
            "--id",
            "endfield-main",
            "--declare-endfield",
            "--expected-revision",
            "0",
            "--confirm",
            "incorrect",
            "--json",
        ],
    );
    let rejection = json(&rejected, 2);
    assert_eq!(rejection["error"]["code"], "confirmation_mismatch");
    assert!(!app_data.join("game-installations").exists());

    let applied = run(
        root,
        &[
            "game",
            "import",
            "apply",
            "--path",
            game_path,
            "--id",
            "endfield-main",
            "--declare-endfield",
            "--expected-revision",
            "0",
            "--confirm",
            confirmation,
            "--json",
        ],
    );
    let applied = json(&applied, 0);
    assert_eq!(applied["registry_revision"], 1);
    assert_eq!(applied["installation"]["state"], "registered");
    assert_eq!(applied["compatibility"], "not_tested");
    assert_eq!(applied["game_execution"], "not_performed");

    let listed = run(root, &["game", "list", "--json"]);
    let listed_json = json(&listed, 0);
    assert_eq!(listed_json["installations"].as_array().unwrap().len(), 1);
    assert!(!String::from_utf8_lossy(&listed.stdout).contains(game_path));

    let shown = run(root, &["game", "show", "--id", "endfield-main", "--json"]);
    let shown = json(&shown, 0);
    assert_eq!(shown["selected_path"], game_path);
    assert_eq!(
        shown["evidence"]["identity_confidence"],
        "user_declared_unverified"
    );

    let validation_output = run(
        root,
        &["game", "validate", "--id", "endfield-main", "--json"],
    );
    let validation = json(&validation_output, 0);
    assert_eq!(validation["state"], "registered");
    assert!(String::from_utf8_lossy(&validation_output.stdout).contains("unverified"));

    let stale = run(
        root,
        &[
            "game",
            "import",
            "apply",
            "--path",
            game_path,
            "--id",
            "endfield-main",
            "--declare-endfield",
            "--expected-revision",
            "0",
            "--confirm",
            confirmation,
            "--json",
        ],
    );
    let stale = json(&stale, 4);
    assert_eq!(stale["error"]["code"], "revision_conflict");

    let removal_plan = run(
        root,
        &["game", "remove-plan", "--id", "endfield-main", "--json"],
    );
    let removal_plan = json(&removal_plan, 0);
    assert_eq!(removal_plan["registry_revision"], 1);
    assert!(
        removal_plan["effects"]
            .as_str()
            .unwrap()
            .contains("remain untouched")
    );
    let remove_confirmation = removal_plan["required_confirmation"].as_str().unwrap();
    let removed = run(
        root,
        &[
            "game",
            "remove",
            "--id",
            "endfield-main",
            "--expected-revision",
            "1",
            "--confirm",
            remove_confirmation,
            "--json",
        ],
    );
    let removed = json(&removed, 0);
    assert_eq!(removed["registry_revision"], 2);
    assert_eq!(removed["game_directory"], "preserved");

    for (path, bytes) in &preserved_app_state {
        assert_eq!(fs::read(path).unwrap(), *bytes);
    }
    assert_eq!(fs::read(&payload).unwrap(), payload_before);
    assert_eq!(fs::read(&inert_launcher).unwrap(), launcher_before);
    let payload_after = fs::metadata(&payload).unwrap();
    let launcher_after = fs::metadata(&inert_launcher).unwrap();
    assert_eq!(payload_after.dev(), payload_metadata.dev());
    assert_eq!(payload_after.ino(), payload_metadata.ino());
    assert_eq!(payload_after.mode(), payload_metadata.mode());
    assert_eq!(payload_after.mtime(), payload_metadata.mtime());
    assert_eq!(launcher_after.dev(), launcher_metadata.dev());
    assert_eq!(launcher_after.ino(), launcher_metadata.ino());
    assert_eq!(launcher_after.mode(), launcher_metadata.mode());
    assert!(!root.join("executed").exists());
    assert!(!game.join("registry.json").exists());
}
