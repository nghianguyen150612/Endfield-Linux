//! Exercise the canonical executable using a nonexistent isolated XDG root.
use serde_json::Value;
use std::os::unix::process::CommandExt;
use std::{
    path::Path,
    process::{Command, Output},
};

fn invoke(root: &Path, args: &[&str]) -> Output {
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
        .unwrap()
}
#[test]
fn empty_host_and_source_selection_are_read_only() {
    let temp = tempfile::tempdir().unwrap();
    for source in ["steam", "epic", "gryphlink"] {
        let output = invoke(
            temp.path(),
            &["diagnose", "dependencies", "--source-id", source, "--json"],
        );
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.len() <= 1024 * 1024);
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["selected_source"], source);
        assert_eq!(report["game_compatibility"], "not_tested");
        assert_eq!(
            report["compatibility"]["runner"]["selection"]["automatic_fallback"],
            false
        );
        assert_eq!(report["game"]["state"], "missing");
        if source == "gryphlink" {
            assert_eq!(report["requirements"].as_array().unwrap().len(), 0);
        }
        assert!(!String::from_utf8_lossy(&output.stdout).contains(temp.path().to_str().unwrap()));
    }
    let human = invoke(temp.path(), &["diagnose", "dependencies"]);
    assert_eq!(human.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&human.stdout).contains("Game compatibility: NOT TESTED"));
    assert!(human.stdout.len() <= 64 * 1024);
    let help = invoke(temp.path(), &["diagnose", "dependencies", "--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help.stdout).contains("--game-id"));
    assert_eq!(
        invoke(
            temp.path(),
            &["diagnose", "dependencies", "--source-id", "bad"]
        )
        .status
        .code(),
        Some(2)
    );
    for dir in ["home", "config", "data", "cache", "state"] {
        assert!(!temp.path().join(dir).exists());
    }
}

#[test]
fn explicit_storage_observation_is_read_only_and_does_not_leak_path() {
    use std::{fs, os::unix::fs::symlink};
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("selected-storage");
    fs::create_dir(&destination).unwrap();
    let selected = destination.to_str().unwrap();
    let response = invoke(
        temp.path(),
        &[
            "diagnose",
            "dependencies",
            "--source-id",
            "steam",
            "--storage-path",
            selected,
            "--json",
        ],
    );
    assert_eq!(
        response.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&response.stderr)
    );
    let report: Value = serde_json::from_slice(&response.stdout).unwrap();
    assert_eq!(report["storage"]["status"], "observed");
    assert_eq!(report["storage"]["selected"], true);
    assert!(report["storage"]["available_bytes"].as_u64().is_some());
    assert!(!String::from_utf8_lossy(&response.stdout).contains(selected));
    assert_eq!(report["game_compatibility"], "not_tested");

    let alias = temp.path().join("directory-symlink");
    symlink(&destination, &alias).unwrap();
    let aliased = invoke(
        temp.path(),
        &[
            "diagnose",
            "dependencies",
            "--source-id",
            "steam",
            "--storage-path",
            alias.to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(aliased.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&aliased.stdout).unwrap();
    assert_eq!(report["storage"]["status"], "blocked_or_unavailable");
    assert!(report["storage"]["available_bytes"].is_null());
    assert!(report["findings"].as_array().unwrap().iter().any(
        |finding| finding["id"] == "storage.installation" && finding["state"] == "unknown"
    ));
    assert_eq!(report["game_compatibility"], "not_tested");

    assert_eq!(
        invoke(
            temp.path(),
            &["diagnose", "dependencies", "--storage-path", "relative/path"]
        )
        .status
        .code(),
        Some(2)
    );
    for dir in ["home", "config", "data", "cache", "state"] {
        assert!(!temp.path().join(dir).exists());
    }
}

#[test]
fn empty_host_completes_with_network_and_child_creation_denied() {
    let temp = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_endfield-unix"));
    command
        .args(["diagnose", "dependencies", "--source-id", "steam", "--json"])
        .env("HOME", temp.path().join("home"))
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .env("XDG_DATA_HOME", temp.path().join("data"))
        .env("XDG_CACHE_HOME", temp.path().join("cache"))
        .env("XDG_STATE_HOME", temp.path().join("state"))
        .env("PATH", temp.path().join("no-executables"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DBUS_SESSION_BUS_ADDRESS");
    // SAFETY: the pre-exec closure uses only stack storage and raw syscalls.
    unsafe {
        command.pre_exec(|| {
            let denied = [
                libc::SYS_socket,
                libc::SYS_connect,
                libc::SYS_clone,
                libc::SYS_clone3,
                libc::SYS_fork,
                libc::SYS_vfork,
            ];
            let mut filter = [libc::sock_filter {
                code: 0,
                jt: 0,
                jf: 0,
                k: 0,
            }; 14];
            filter[0] = libc::sock_filter {
                code: 0x20,
                jt: 0,
                jf: 0,
                k: 0,
            };
            for (i, nr) in denied.iter().enumerate() {
                filter[1 + 2 * i] = libc::sock_filter {
                    code: 0x15,
                    jt: 0,
                    jf: 1,
                    k: *nr as u32,
                };
                filter[2 + 2 * i] = libc::sock_filter {
                    code: 0x06,
                    jt: 0,
                    jf: 0,
                    k: 0x00050000 | libc::EPERM as u32,
                };
            }
            filter[13] = libc::sock_filter {
                code: 0x06,
                jt: 0,
                jf: 0,
                k: 0x7fff0000,
            };
            let program = libc::sock_fprog {
                len: 14,
                filter: filter.as_mut_ptr(),
            };
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                || libc::prctl(libc::PR_SET_SECCOMP, 2, &program) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let output = command.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["game_compatibility"], "not_tested");
    for dir in ["home", "config", "data", "cache", "state"] {
        assert!(!temp.path().join(dir).exists());
    }
}

#[test]
fn explicit_missing_game_and_prefix_remain_partial() {
    let temp = tempfile::tempdir().unwrap();
    let output = invoke(
        temp.path(),
        &[
            "diagnose",
            "dependencies",
            "--profile",
            "desktop",
            "--game-id",
            "endfield-main",
            "--prefix-id",
            "prefix-main",
            "--json",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["game"]["state"], "not_registered");
    assert_eq!(
        report["compatibility"]["target"]["profile_scope"],
        "desktop"
    );
    assert_eq!(
        report["compatibility"]["target"]["prefix_id"],
        "prefix-main"
    );
    assert_eq!(report["game_compatibility"], "not_tested");
    for dir in ["home", "config", "data", "cache", "state"] {
        assert!(!temp.path().join(dir).exists());
    }
}

#[test]
fn registered_game_revalidation_does_not_modify_registry_or_payload() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for dir in ["config/endfield-unix", "data/endfield-unix"] {
        let path = root.join(dir);
        fs::create_dir_all(&path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let game = root.join("selected-game");
    fs::create_dir(&game).unwrap();
    fs::write(game.join("payload"), b"inert game data").unwrap();
    let path = game.to_str().unwrap();
    let plan = invoke(
        root,
        &[
            "game",
            "import",
            "plan",
            "--path",
            path,
            "--id",
            "endfield-main",
            "--declare-endfield",
            "--json",
        ],
    );
    assert_eq!(
        plan.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let plan: Value = serde_json::from_slice(&plan.stdout).unwrap();
    let token = plan["required_confirmation"].as_str().unwrap();
    let applied = invoke(
        root,
        &[
            "game",
            "import",
            "apply",
            "--path",
            path,
            "--id",
            "endfield-main",
            "--declare-endfield",
            "--expected-revision",
            "0",
            "--confirm",
            token,
            "--json",
        ],
    );
    assert_eq!(
        applied.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let registry = root.join("data/endfield-unix/game-installations/registry.json");
    let before = fs::read(&registry).unwrap();
    let report = invoke(
        root,
        &[
            "diagnose",
            "dependencies",
            "--game-id",
            "endfield-main",
            "--source-id",
            "steam",
            "--json",
        ],
    );
    assert_eq!(
        report.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&report.stderr)
    );
    let report: Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report["game"]["selected_id"], "endfield-main");
    assert_eq!(report["game_compatibility"], "not_tested");
    assert_eq!(fs::read(&registry).unwrap(), before);
    assert_eq!(fs::read(game.join("payload")).unwrap(), b"inert game data");
}

#[test]
fn corrupt_runtime_prefix_and_recovery_metadata_are_untouched() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let config = root.join("config/endfield-unix");
    let data = root.join("data/endfield-unix");
    for path in [
        &config,
        &data,
        &data.join("prefixes"),
        &data.join("backups"),
    ] {
        fs::create_dir_all(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let files = [
        config.join("runtime-pins.json"),
        data.join("prefixes/registry.json"),
        data.join("backups/journal.json"),
    ];
    for file in &files {
        fs::write(file, b"inert corrupt fixture").unwrap();
        fs::set_permissions(file, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let output = invoke(
        root,
        &[
            "diagnose",
            "dependencies",
            "--profile",
            "desktop",
            "--prefix-id",
            "prefix-main",
            "--json",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["game_compatibility"], "not_tested");
    for file in &files {
        assert_eq!(fs::read(file).unwrap(), b"inert corrupt fixture");
    }
}
