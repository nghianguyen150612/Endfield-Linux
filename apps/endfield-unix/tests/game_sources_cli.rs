//! P032 qualification through the real executable, with network/child creation denied.
use serde_json::Value;
use std::{
    fs,
    os::unix::process::CommandExt,
    path::Path,
    process::{Command, Output},
};
fn run(root: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_endfield-unix"));
    command
        .args(args)
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", root.join("no-executables"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DBUS_SESSION_BUS_ADDRESS");
    // SAFETY: child-only pre_exec performs raw syscalls without allocation or locks.
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
            }; 15];
            filter[0] = libc::sock_filter {
                code: 0x20,
                jt: 0,
                jf: 0,
                k: 0,
            }; // load syscall nr
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
    command
        .output()
        .expect("run canonical binary under seccomp")
}
fn json(output: Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.len() <= 64 * 1024);
    serde_json::from_slice(&output.stdout).unwrap()
}
fn absent(root: &Path) {
    for dir in ["home", "config", "data", "cache", "state"] {
        assert!(!root.join(dir).exists(), "read-only command created {dir}");
    }
}
fn fixture(root: &Path) -> std::path::PathBuf {
    let library = root.join("Library");
    fs::create_dir_all(library.join("steamapps/common/Chosen Δ")).unwrap();
    fs::write(
        library.join("steamapps/common/Chosen Δ/payload"),
        b"unchanged game bytes",
    )
    .unwrap();
    fs::write(
        library.join("steamapps/appmanifest_4732690.acf"),
        r#""AppState" { "appid" "4732690" "installdir" "Chosen Δ" }"#,
    )
    .unwrap();
    library
}
#[test]
fn catalog_details_errors_and_nested_help_are_headless_and_storage_free() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let catalog = json(run(root, &["game", "sources", "--json"]), 0);
    assert_eq!(catalog["schema_version"], 1);
    assert_eq!(catalog["sources"].as_array().unwrap().len(), 3);
    for id in ["steam", "epic", "gryphlink"] {
        let value = json(
            run(root, &["game", "source", "show", "--id", id, "--json"]),
            0,
        );
        assert_eq!(value["source"]["id"], id);
        assert!(
            value["source"]["official_page"]
                .as_str()
                .unwrap()
                .starts_with("https://")
        );
        assert!(
            !value["source"]["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "verified_artifact_acquisition")
        );
        let human = run(root, &["game", "source", "show", "--id", id]);
        assert!(human.status.success());
        assert!(String::from_utf8_lossy(&human.stdout).contains("No download or execution"));
    }
    assert_eq!(
        json(
            run(
                root,
                &["game", "source", "show", "--id", "mirror", "--json"]
            ),
            2
        )["error"]["code"],
        "unknown_source"
    );
    assert_eq!(
        json(run(root, &["game", "source", "check-catalog", "--json"]), 0)["live_availability"],
        "unknown"
    );
    for args in [
        vec!["game", "--help"],
        vec!["game", "source", "--help"],
        vec!["game", "source", "inspect", "--help"],
        vec!["game", "source", "plan-association", "--help"],
        vec!["game", "sources", "--help"],
    ] {
        assert!(run(root, &args).status.success());
    }
    for id in ["epic", "gryphlink"] {
        assert_eq!(
            json(
                run(
                    root,
                    &[
                        "game",
                        "source",
                        "inspect",
                        "--id",
                        id,
                        "--path",
                        "/never-read-credentials",
                        "--json"
                    ]
                ),
                78
            )["error"]["code"],
            "unsupported_source_operation"
        );
    }
    assert_eq!(
        run(root, &["game", "source", "download", "--id", "steam"])
            .status
            .code(),
        Some(2)
    );
    absent(root);
}
#[test]
fn steam_inspect_and_exact_revalidation_preserve_files_and_report_changes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let library = fixture(root);
    let path = library.to_str().unwrap();
    let value = json(
        run(
            root,
            &[
                "game", "source", "inspect", "--id", "steam", "--path", path, "--json",
            ],
        ),
        0,
    );
    assert_eq!(value["app_id"], "4732690");
    assert_eq!(value["publisher_authenticity"], "unverifiable");
    assert_eq!(value["game_compatibility"], "not_tested");
    let token = value["observation_token"].as_str().unwrap();
    assert_eq!(
        json(
            run(
                root,
                &[
                    "game",
                    "source",
                    "revalidate",
                    "--id",
                    "steam",
                    "--path",
                    path,
                    "--expected-observation",
                    token,
                    "--json"
                ]
            ),
            0
        )["status"],
        "observed_local_metadata"
    );
    fs::write(
        library.join("steamapps/appmanifest_4732690.acf"),
        r#""AppState" { "appid" "4732690" "installdir" "Chosen Δ" "buildid" "2" }"#,
    )
    .unwrap();
    assert_eq!(
        json(
            run(
                root,
                &[
                    "game",
                    "source",
                    "revalidate",
                    "--id",
                    "steam",
                    "--path",
                    path,
                    "--expected-observation",
                    token,
                    "--json"
                ]
            ),
            4
        )["status"],
        "changed"
    );
    assert_eq!(
        fs::read(library.join("steamapps/common/Chosen Δ/payload")).unwrap(),
        b"unchanged game bytes"
    );
    absent(root);
}
#[test]
fn p031_v1_import_association_preview_and_removal_keep_source_state_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let library = fixture(root);
    let game = library.join("steamapps/common/Chosen Δ");
    let game_path = game.to_str().unwrap();
    let plan = json(
        run(
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
        ),
        0,
    );
    let token = plan["required_confirmation"].as_str().unwrap();
    absent(root);
    let record = json(
        run(
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
                token,
                "--json",
            ],
        ),
        0,
    );
    assert_eq!(record["schema_version"], 1);
    let registry = root.join("data/endfield-unix/game-installations/registry.json");
    let before = fs::read(&registry).unwrap();
    let no_source = json(
        run(
            root,
            &[
                "game",
                "source",
                "validate",
                "--game-id",
                "endfield-main",
                "--json",
            ],
        ),
        0,
    );
    assert!(no_source["source_association"].is_null());
    let label = json(
        run(
            root,
            &[
                "game",
                "source",
                "plan-association",
                "--game-id",
                "endfield-main",
                "--source-id",
                "steam",
                "--json",
            ],
        ),
        0,
    );
    assert_eq!(label["source_evidence_type"], "user_declared");
    assert_eq!(label["association_stored"], false);
    let observation = json(
        run(
            root,
            &[
                "game",
                "source",
                "plan-association",
                "--game-id",
                "endfield-main",
                "--source-id",
                "steam",
                "--path",
                library.to_str().unwrap(),
                "--json",
            ],
        ),
        0,
    );
    assert_eq!(
        observation["source_evidence_type"],
        "observed_local_metadata"
    );
    assert_eq!(
        observation["local_observation_matches_registered_location"],
        true
    );
    assert_eq!(observation["registry_revision"], 1);
    for id in ["epic", "gryphlink"] {
        let v = json(
            run(
                root,
                &[
                    "game",
                    "source",
                    "plan-association",
                    "--game-id",
                    "endfield-main",
                    "--source-id",
                    id,
                    "--json",
                ],
            ),
            0,
        );
        assert_eq!(v["source_evidence_type"], "user_declared");
    }
    assert_eq!(fs::read(&registry).unwrap(), before);
    let other = fixture(&root.join("other"));
    let mismatch = json(
        run(
            root,
            &[
                "game",
                "source",
                "plan-association",
                "--game-id",
                "endfield-main",
                "--source-id",
                "steam",
                "--path",
                other.to_str().unwrap(),
                "--json",
            ],
        ),
        0,
    );
    assert_eq!(
        mismatch["local_observation_matches_registered_location"],
        false
    );
    let remove = json(
        run(
            root,
            &["game", "remove-plan", "--id", "endfield-main", "--json"],
        ),
        0,
    );
    let confirmation = remove["required_confirmation"].as_str().unwrap();
    json(
        run(
            root,
            &[
                "game",
                "remove",
                "--id",
                "endfield-main",
                "--expected-revision",
                "0",
                "--confirm",
                confirmation,
                "--json",
            ],
        ),
        4,
    );
    json(
        run(
            root,
            &[
                "game",
                "remove",
                "--id",
                "endfield-main",
                "--expected-revision",
                "1",
                "--confirm",
                confirmation,
                "--json",
            ],
        ),
        0,
    );
    assert_eq!(
        fs::read(game.join("payload")).unwrap(),
        b"unchanged game bytes"
    );
    assert!(!root.join("config").exists());
    assert!(!root.join("data/endfield-unix/prefixes").exists());
}
#[test]
fn corrupt_and_future_p031_registries_are_never_reset_by_source_commands() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let dir = root.join("data/endfield-unix/game-installations");
    fs::create_dir_all(&dir).unwrap();
    use std::os::unix::fs::PermissionsExt;
    for p in [
        root.join("data"),
        root.join("data/endfield-unix"),
        dir.clone(),
    ] {
        fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let path = dir.join("registry.json");
    for bytes in [
        b"corrupt".as_slice(),
        br#"{"schema_version":99,"revision":1,"installations":[],"checksum":"invalid"}"#,
    ] {
        fs::write(&path, bytes).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        json(
            run(
                root,
                &[
                    "game",
                    "source",
                    "plan-association",
                    "--game-id",
                    "endfield-main",
                    "--source-id",
                    "steam",
                    "--json",
                ],
            ),
            70,
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
