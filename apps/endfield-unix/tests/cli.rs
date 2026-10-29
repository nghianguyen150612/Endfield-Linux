use std::fs;
use std::process::{Command, Output};

use tempfile::tempdir;

fn invoke(arguments: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_endfield-unix"));
    command.env_clear().args(arguments);
    match command.output() {
        Ok(output) => output,
        Err(error) => panic!("failed to start endfield-unix test executable: {error}"),
    }
}

#[test]
fn compiled_executable_help_succeeds_without_initializing_the_display() {
    let output = invoke(&["--help"]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("endfield-unix"));
    assert!(stdout.contains("native GTK4 desktop application"));
    assert!(stdout.contains("diagnose report --output PATH"));
    assert!(stdout.contains("diagnose platform"));
    assert!(stdout.contains("diagnose hardware"));
    assert!(stdout.contains("diagnose display"));
    assert!(stdout.contains("diagnose runtimes"));
    assert!(stdout.contains("diagnose umu"));
    assert!(stdout.contains("diagnose telemetry frames --input PATH"));
    assert!(stdout.contains("service ping"));
    assert!(stdout.contains("service status"));
    assert!(
        stdout.contains(
            "service systemd <install|uninstall|enable|disable|start|stop|restart|status>"
        )
    );
    assert!(!stdout.contains("  game "));
    assert!(output.stderr.is_empty());
}

#[test]
fn runtime_diagnosis_is_headless_and_reports_no_fake_candidates() {
    let output = invoke(&["diagnose", "runtimes", "--family", "proton"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Runtime discovery: Linux"));
    assert!(stdout.contains("Discovered runtimes:"));
    assert!(stdout.contains("Discovery result:"));
    assert!(!stdout.contains("/home/"));
    assert!(!stdout.contains("Proton 10-26"));
}

#[test]
fn umu_diagnosis_is_headless_truthful_and_redacts_runtime_paths() {
    let output = invoke(&["diagnose", "umu"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("UMU integration: Linux"));
    assert!(stdout.contains("UMU version: unknown"));
    assert!(stdout.contains("Proton discovery:"));
    assert!(stdout.contains("Selected runtime: none (identity not evaluated)"));
    assert!(stdout.contains("Source verification: not performed"));
    assert!(stdout.contains("Compatibility: not evaluated"));
    assert!(stdout.contains("Network access: not performed"));
    assert!(stdout.contains("Runtime execution: not performed"));
    assert!(stdout.contains("Game execution: not performed"));
    assert!(!stdout.contains("/home/"));
}

#[test]
fn runtime_diagnosis_preserves_partial_results_and_prints_safe_issue_categories() {
    let directory = tempdir().unwrap();
    let malformed_root = directory.path().join("not-a-directory");
    fs::write(&malformed_root, b"regular file").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_endfield-unix"))
        .env_clear()
        .args(["diagnose", "runtimes", "--root"])
        .arg(&malformed_root)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Discovery result: partial"));
    assert!(stdout.contains("explicit runtime directory: root-not-directory"));
    assert!(!stdout.contains(&directory.path().display().to_string()));
}

#[test]
fn help_aliases_and_command_are_equivalent() {
    let long = invoke(&["--help"]);
    let short = invoke(&["-h"]);
    let command = invoke(&["help"]);

    assert_eq!(long.status.code(), Some(0));
    assert_eq!(short.stdout, long.stdout);
    assert_eq!(command.stdout, long.stdout);
}

#[test]
fn command_specific_help_describes_gui_and_about() {
    let gui = invoke(&["help", "gui"]);
    let about = invoke(&["help", "about"]);
    let service = invoke(&["help", "service"]);
    let diagnose = invoke(&["help", "diagnose"]);

    assert_eq!(gui.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&gui.stdout).contains("same startup path"));
    assert!(String::from_utf8_lossy(&gui.stdout).contains("graphical session"));
    assert_eq!(about.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&about.stdout).contains("static purpose, version"));
    assert_eq!(service.status.code(), Some(0));
    let service_help = String::from_utf8_lossy(&service.stdout);
    assert!(service_help.contains("authenticated local Ping/Pong"));
    assert!(service_help.contains("does not enable or start the unit"));
    assert!(service_help.contains("never uses root privileges"));
    assert_eq!(diagnose.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&diagnose.stdout).contains(
        "diagnose <report|platform|hardware|gpu|display|runtimes|umu|runtime-integrity|telemetry>\nUsage: endfield-unix runtime <install|list-managed>"
    ));
    for args in [
        &["runtime", "install", "--help"][..],
        &["runtime", "install", "plan", "--help"][..],
        &["runtime", "install", "apply", "--help"][..],
    ] {
        let output = invoke(args);
        assert_eq!(
            output.status.code(),
            Some(0),
            "help args {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
    }
}

#[test]
fn compiled_executable_version_uses_package_metadata() {
    let output = invoke(&["--version"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout),
        Ok(format!("endfield-unix {}\n", env!("CARGO_PKG_VERSION")))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn diagnose_platform_runs_headlessly_without_a_daemon_or_runtime_directory() {
    let output = invoke(&["diagnose", "platform"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Platform: Linux"));
    assert!(stdout.contains("Distribution:"));
    assert!(stdout.contains("Kernel: Linux "));
    assert!(stdout.contains("Architecture:") && stdout.contains("kernel-reported"));
    assert!(stdout.contains("Environment:"));
    assert!(stdout.contains("User runtime directory: missing"));
    assert!(stdout.contains("Probe result: partial"));
    assert!(!stdout.contains("Daemon:"));
}

#[test]
fn diagnose_hardware_runs_headlessly_without_a_daemon_or_runtime_directory() {
    let output = invoke(&["diagnose", "hardware"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Hardware probe: Linux"));
    assert!(stdout.contains("Logical CPUs online:"));
    assert!(stdout.contains("CPUs allowed to process:"));
    assert!(stdout.contains("Host-visible total RAM:"));
    assert!(stdout.contains("Cgroup memory limit:"));
    assert!(stdout.contains("Probe result:"));
    assert!(!stdout.contains("Daemon:"));
    assert!(!stdout.contains("/sys/"));
    assert!(!stdout.contains("/proc/"));
}

#[test]
fn diagnose_gpu_runs_headlessly_without_a_daemon() {
    let output = invoke(&["diagnose", "gpu"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("GPU discovery: Linux"));
    assert!(stdout.contains("Vulkan:"));
    assert!(stdout.contains("Probe result: "));
}

#[test]
fn diagnose_display_runs_headlessly_without_a_daemon_or_graphical_session() {
    let output = invoke(&["diagnose", "display"]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Display discovery: Linux"));
    assert!(stdout.contains("Type hint: headless"));
    assert!(stdout.contains("Verified display protocol: none connected"));
    assert!(stdout.contains("Displays: none verified"));
    assert!(stdout.contains("Probe result: partial"));
    assert!(!stdout.contains("Daemon:"));
    assert!(!stdout.contains("/sys/"));
    assert!(!stdout.contains("/run/user/"));
}

#[test]
fn compiled_executable_about_uses_package_metadata() {
    let output = invoke(&["about"]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Application: endfield-unix"));
    assert!(stdout.contains(&format!("Version: {}", env!("CARGO_PKG_VERSION"))));
    assert!(stdout.contains(env!("CARGO_PKG_DESCRIPTION")));
    assert!(stdout.contains(&format!("License: {}", env!("CARGO_PKG_LICENSE"))));
    assert!(stdout.contains(env!("CARGO_PKG_REPOSITORY")));
    assert!(stdout.contains("under development"));
    assert!(output.stderr.is_empty());
}

#[test]
fn compiled_executable_default_reports_missing_graphical_session() {
    let output = invoke(&[]);

    assert_eq!(output.status.code(), Some(69));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("could not initialize GTK display"));
    assert!(stderr.contains("graphical session"));
    assert!(!stderr.contains("panicked at"));
}

#[test]
fn default_and_explicit_gui_use_the_same_startup_behavior() {
    let default = invoke(&[]);
    let explicit = invoke(&["gui"]);

    assert_eq!(default.status.code(), Some(69));
    assert_eq!(explicit.status.code(), default.status.code());
    assert!(default.stdout.is_empty());
    assert!(explicit.stdout.is_empty());
    let default_stderr = String::from_utf8_lossy(&default.stderr);
    let explicit_stderr = String::from_utf8_lossy(&explicit.stderr);
    assert!(default_stderr.contains("could not initialize GTK display"));
    assert_eq!(explicit_stderr, default_stderr);
}

#[test]
fn compiled_executable_invalid_argument_fails_clearly() {
    let output = invoke(&["--unknown"]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unrecognized argument \"--unknown\""));
    assert!(stderr.contains("endfield-unix"));
    assert!(output.stdout.is_empty());
}

#[test]
fn compiled_executable_rejects_multiple_options_deterministically() {
    let first = invoke(&["--help", "--version"]);
    let second = invoke(&["--help", "--version"]);

    assert_eq!(first.status.code(), Some(2));
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(first.stderr, second.stderr);
}

#[test]
fn unknown_commands_and_extra_arguments_return_status_two() {
    for arguments in [&["unknown"][..], &["about", "extra"][..]] {
        let output = invoke(arguments);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn frame_event_import_is_a_read_only_headless_file_based_end_to_end() {
    let directory = tempdir().expect("isolated import directory");
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/platform-linux/tests/fixtures/frame-events-v1.csv");
    let input = directory.path().join("selected-recording.csv");
    fs::copy(source, &input).expect("copy validated deterministic fixture");
    let original = fs::read(&input).expect("read fixture bytes");

    let output = Command::new(env!("CARGO_BIN_EXE_endfield-unix"))
        .env_clear()
        .env("HOME", directory.path())
        .env("XDG_CONFIG_HOME", directory.path())
        .env("XDG_STATE_HOME", directory.path())
        .current_dir(directory.path())
        .args([
            "diagnose",
            "telemetry",
            "frames",
            "--format",
            "frame-events-v1",
            "--input",
        ])
        .arg(&input)
        .output()
        .expect("execute real endfield-unix binary");

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 CLI output");
    assert!(stdout.contains("Frame telemetry: Linux"));
    assert!(stdout.contains("Source: frame-events-v1"));
    assert!(stdout.contains("Session association: import-local"));
    assert!(stdout.contains("Valid frame interval observations: 2"));
    assert!(stdout.contains("FPS: 100.000 FPS"));
    assert!(stdout.contains("Mean frametime: 10.000000 ms"));
    assert!(stdout.contains("P99 frametime: unknown (unknown: insufficient observations)"));
    assert!(stdout.contains("FrameCount: not published"));
    assert!(stdout.contains("Result: partial"));
    assert!(!stdout.contains("selected-recording.csv"));
    assert_eq!(fs::read(&input).expect("input remains readable"), original);
    assert_eq!(
        fs::read_dir(directory.path())
            .expect("isolated output directory")
            .count(),
        1,
        "the import must not create state or telemetry files"
    );
}

#[test]
fn frame_import_distinguishes_invalid_arguments_unsupported_input_and_io_failure() {
    let missing_input = invoke(&["diagnose", "telemetry", "frames"]);
    assert_eq!(missing_input.status.code(), Some(2));

    let directory = tempdir().expect("isolated import directory");
    let malformed = directory.path().join("malformed.csv");
    fs::write(
        &malformed,
        b"# endfield-frame-events-v1\nframe_index,time\n1,1\n",
    )
    .expect("write malformed frame source");
    let malformed_result = Command::new(env!("CARGO_BIN_EXE_endfield-unix"))
        .env_clear()
        .arg("diagnose")
        .arg("telemetry")
        .arg("frames")
        .arg("--input")
        .arg(&malformed)
        .output()
        .expect("execute malformed import");
    assert_eq!(malformed_result.status.code(), Some(70));
    assert!(String::from_utf8_lossy(&malformed_result.stderr).contains("recording is malformed"));

    let unsupported_result = Command::new(env!("CARGO_BIN_EXE_endfield-unix"))
        .env_clear()
        .arg("diagnose")
        .arg("telemetry")
        .arg("frames")
        .arg("--format")
        .arg("mangohud-csv")
        .arg("--input")
        .arg(&malformed)
        .output()
        .expect("execute unsupported import");
    assert_eq!(unsupported_result.status.code(), Some(78));
    assert!(String::from_utf8_lossy(&unsupported_result.stderr).contains("format is unsupported"));

    let inaccessible = directory.path().join("missing.csv");
    let io_result = Command::new(env!("CARGO_BIN_EXE_endfield-unix"))
        .env_clear()
        .arg("diagnose")
        .arg("telemetry")
        .arg("frames")
        .arg("--input")
        .arg(inaccessible)
        .output()
        .expect("execute inaccessible import");
    assert_eq!(io_result.status.code(), Some(74));
}

#[test]
fn mangohud_cli_keeps_source_sampled_values_distinct_from_measured_fps() {
    let directory = tempdir().expect("isolated import directory");
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/platform-linux/tests/fixtures/mangohud-versioned-v1.csv");
    let input = directory.path().join("selected-mangohud.csv");
    fs::copy(source, &input).expect("copy source-backed MangoHud schema fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_endfield-unix"))
        .env_clear()
        .arg("diagnose")
        .arg("telemetry")
        .arg("frames")
        .arg("--format")
        .arg("mangohud-csv")
        .arg("--input")
        .arg(&input)
        .output()
        .expect("execute MangoHud frame import");

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("MangoHud CSV (versioned v1 header)"));
    assert!(stdout.contains("sampled rows; not every frame"));
    assert!(stdout.contains("FPS: unknown (unknown: no verified frame-event window)"));
    assert!(stdout.contains("MangoHud FPS values: 3 sampled instantaneous last-frame reciprocals"));
    assert!(stdout.contains("Missing records: unknown"));
}

#[test]
fn startup_attempts_real_graphics_initialization_without_game_or_network() {
    let output = invoke(&[]);

    assert_eq!(output.status.code(), Some(69));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("could not initialize GTK display"));
    assert!(!stderr.contains("GUI mode is unavailable"));
}

#[test]
fn static_commands_do_not_create_application_directories_or_logs() {
    let directory = tempdir().expect("temporary root");
    let home = directory.path().join("home");
    let state = directory.path().join("state");
    let config = directory.path().join("config");
    let data = directory.path().join("data");
    for arguments in [
        &["--help"][..],
        &["--version"][..],
        &["about"][..],
        &["help", "service"][..],
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_endfield-unix"));
        command
            .env_clear()
            .env("HOME", &home)
            .env("XDG_STATE_HOME", &state)
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_DATA_HOME", &data)
            .args(arguments);
        let output = command.output().expect("run static command");
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
    }
    assert!(!home.exists());
    assert!(!state.exists());
    assert!(!config.exists());
    assert!(!data.exists());
}

#[test]
fn diagnose_report_writes_only_to_its_explicit_path_and_is_headless() {
    let directory = tempdir().expect("temporary root");
    let report_path = directory.path().join("support-report.json");
    let state_path = directory.path().join("state");
    let mut command = Command::new(env!("CARGO_BIN_EXE_endfield-unix"));
    command
        .env_clear()
        .env("HOME", directory.path().join("home"))
        .env("XDG_STATE_HOME", &state_path)
        .args(["diagnose", "report", "--output"])
        .arg(&report_path);
    let output = command.output().expect("run report command");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Support report written"));
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(&report_path).expect("report output"))
            .expect("valid report JSON");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["application"]["name"], "endfield-unix");
    assert_eq!(report["log_history"]["status"], "missing");
    assert!(!state_path.exists());
}

#[test]
fn diagnose_report_requires_explicit_overwrite() {
    let directory = tempdir().expect("temporary root");
    let report_path = directory.path().join("support-report.json");
    fs::write(&report_path, b"existing report").expect("existing destination");

    let no_overwrite = Command::new(env!("CARGO_BIN_EXE_endfield-unix"))
        .env_clear()
        .args(["diagnose", "report", "--output"])
        .arg(&report_path)
        .output()
        .expect("run report without overwrite");
    assert_eq!(no_overwrite.status.code(), Some(74));
    assert_eq!(
        fs::read(&report_path).expect("preserved destination"),
        b"existing report"
    );

    let overwrite = Command::new(env!("CARGO_BIN_EXE_endfield-unix"))
        .env_clear()
        .args(["diagnose", "report", "--output"])
        .arg(&report_path)
        .arg("--overwrite")
        .output()
        .expect("run report with overwrite");
    assert_eq!(overwrite.status.code(), Some(0));
    assert_ne!(
        fs::read(&report_path).expect("replaced destination"),
        b"existing report"
    );
}

#[cfg(unix)]
#[test]
fn diagnose_report_refuses_symlink_destination() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary root");
    let target = directory.path().join("target.json");
    let report_path = directory.path().join("report.json");
    fs::write(&target, b"preserve").expect("target file");
    symlink(&target, &report_path).expect("destination symlink");
    let mut command = Command::new(env!("CARGO_BIN_EXE_endfield-unix"));
    command
        .env_clear()
        .args(["diagnose", "report", "--output"])
        .arg(&report_path)
        .arg("--overwrite");
    let output = command.output().expect("run report command");
    assert_eq!(output.status.code(), Some(74));
    assert_eq!(fs::read(target).expect("target unchanged"), b"preserve");
}

#[test]
fn gpu_telemetry_is_headless_on_demand_and_does_not_create_configuration() {
    let root = tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_endfield-unix"))
        .env_clear()
        .env("HOME", root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_DATA_HOME", root.path().join("data"))
        .args(["diagnose", "telemetry", "gpu"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("GPU telemetry: Linux"));
    assert!(text.contains("Probe result:"));
    assert!(text.contains("NVIDIA NVML:"));
    assert!(text.contains("Utilization: unsupported normalized window"));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    let invalid = invoke(&["diagnose", "telemetry", "gpu", "--interval-ms", "99"]);
    assert_eq!(invalid.status.code(), Some(2));
}
