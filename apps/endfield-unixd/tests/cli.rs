use std::process::{Command, Output};

use endfield_unixd::cli::{
    self, Command as DaemonCommand, DAEMON_FAILURE, INVALID_ARGUMENTS, OUTPUT_IO_FAILURE, SUCCESS,
};
use tempfile::tempdir;

fn invoke(arguments: &[&str], state_home: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_endfield-unixd"))
        .env_clear()
        .env("HOME", state_home.join("home"))
        .env("XDG_STATE_HOME", state_home.join("state"))
        .args(arguments)
        .output()
        .expect("run endfield-unixd")
}

#[test]
fn help_is_headless_deterministic_and_describes_the_current_boundary() {
    let directory = tempdir().expect("temporary home");
    let first = invoke(&["--help"], directory.path());
    let second = invoke(&["--help"], directory.path());
    assert_eq!(first.status.code(), Some(SUCCESS));
    assert_eq!(first.stdout, second.stdout);
    assert!(first.stderr.is_empty());
    let help = String::from_utf8_lossy(&first.stdout);
    for phrase in [
        "foreground lifecycle daemon",
        "--foreground",
        "SIGTERM",
        "Ctrl+C",
        "does not provide IPC",
        "No systemd unit is installed or enabled",
    ] {
        assert!(help.contains(phrase), "missing help text: {phrase}");
    }
    assert!(!directory.path().join("state").exists());
}

#[test]
fn version_comes_from_package_metadata_without_creating_state() {
    let directory = tempdir().expect("temporary home");
    let output = invoke(&["--version"], directory.path());
    assert_eq!(output.status.code(), Some(SUCCESS));
    assert_eq!(
        String::from_utf8(output.stdout),
        Ok(format!("endfield-unixd {}\n", env!("CARGO_PKG_VERSION")))
    );
    assert!(output.stderr.is_empty());
    assert!(!directory.path().join("state").exists());
}

#[test]
fn foreground_and_implicit_foreground_are_parsed_without_starting_a_service() {
    assert_eq!(
        cli::parse(Vec::<String>::new()),
        Ok(DaemonCommand::Foreground)
    );
    assert_eq!(cli::parse(["--foreground"]), Ok(DaemonCommand::Foreground));
    assert_eq!(cli::parse(["--help"]), Ok(DaemonCommand::Help));
    assert_eq!(cli::parse(["--version"]), Ok(DaemonCommand::Version));
}

#[test]
fn unsupported_or_combined_arguments_return_status_two_without_side_effects() {
    let directory = tempdir().expect("temporary home");
    for arguments in [
        &["--unknown"][..],
        &["--foreground", "--help"][..],
        &["start"][..],
    ] {
        let output = invoke(arguments, directory.path());
        assert_eq!(output.status.code(), Some(INVALID_ARGUMENTS));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("invalid arguments"));
    }
    assert!(!directory.path().join("state").exists());
}

#[test]
fn documented_status_mapping_is_stable() {
    assert_eq!(SUCCESS, 0);
    assert_eq!(INVALID_ARGUMENTS, 2);
    assert_eq!(DAEMON_FAILURE, 70);
    assert_eq!(OUTPUT_IO_FAILURE, 74);
}
