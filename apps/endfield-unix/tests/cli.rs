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
fn compiled_executable_help_succeeds_without_initializing_the_display() {
    let output = invoke(&["--help"]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("endfield-unix"));
    assert!(stdout.contains("native desktop shell is available"));
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
fn startup_attempts_real_graphics_initialization_without_game_or_network() {
    let output = invoke(&[]);

    assert_eq!(output.status.code(), Some(69));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("could not initialize GTK display"));
    assert!(!stderr.contains("GUI mode is unavailable"));
}
