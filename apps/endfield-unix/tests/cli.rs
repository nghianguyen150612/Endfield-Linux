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
    assert!(stdout.contains("native GTK4 desktop application"));
    assert!(stdout.contains(
        "Game, runtime, optimization, benchmarking, and daemon services are under development."
    ));
    assert!(!stdout.contains("  game "));
    assert!(output.stderr.is_empty());
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

    assert_eq!(gui.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&gui.stdout).contains("same startup path"));
    assert!(String::from_utf8_lossy(&gui.stdout).contains("graphical session"));
    assert_eq!(about.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&about.stdout).contains("static purpose, version"));
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
fn startup_attempts_real_graphics_initialization_without_game_or_network() {
    let output = invoke(&[]);

    assert_eq!(output.status.code(), Some(69));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("could not initialize GTK display"));
    assert!(!stderr.contains("GUI mode is unavailable"));
}
