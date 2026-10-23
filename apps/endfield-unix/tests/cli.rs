use std::process::{Command, Output};

fn invoke(arguments: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_endfield-unix"));
    command.env_clear().args(arguments);
    match command.output() {
        Ok(output) => output,
        Err(error) => panic!("failed to start endfield-unix test executable: {error}"),
    }
}

#[test]
fn compiled_executable_help_succeeds_and_describes_development_status() {
    let output = invoke(&["--help"]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("endfield-unix"));
    assert!(stdout.contains("GUI and CLI functionality are under active development."));
}

#[test]
fn compiled_executable_version_uses_package_metadata() {
    let output = invoke(&["--version"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout),
        Ok(format!("endfield-unix {}\n", env!("CARGO_PKG_VERSION")))
    );
}

#[test]
fn compiled_executable_default_uses_unavailable_gui_boundary_without_environment() {
    let output = invoke(&[]);

    assert_eq!(output.status.code(), Some(69));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("GUI mode is unavailable"));
    assert!(stderr.contains("P004"));
}

#[test]
fn compiled_executable_invalid_argument_fails_clearly() {
    let output = invoke(&["--unknown"]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unrecognized argument \"--unknown\""));
    assert!(stderr.contains("endfield-unix"));
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
fn startup_with_no_external_runtime_environment_needs_no_game_or_network() {
    let output = invoke(&[]);

    assert_eq!(output.status.code(), Some(69));
    assert!(String::from_utf8_lossy(&output.stderr).contains("GUI mode is unavailable"));
}
