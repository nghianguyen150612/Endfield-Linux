use std::io::{self, Write};

use super::commands::HelpTopic;
use crate::{
    APPLICATION_LICENSE, APPLICATION_NAME, APPLICATION_PURPOSE, APPLICATION_REPOSITORY,
    APPLICATION_STAGE, APPLICATION_VERSION,
};

/// Writes the general help text, which lists only implemented commands.
pub fn write_help<W: Write>(topic: Option<HelpTopic>, output: &mut W) -> io::Result<()> {
    match topic {
        None => write_general_help(output),
        Some(HelpTopic::Gui) => write_gui_help(output),
        Some(HelpTopic::About) => write_about_help(output),
    }
}

pub fn write_version<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(output, "{APPLICATION_NAME} {APPLICATION_VERSION}")
}

pub fn write_about<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(output, "Application: {APPLICATION_NAME}")?;
    writeln!(output, "Version: {APPLICATION_VERSION}")?;
    writeln!(output, "Purpose: {APPLICATION_PURPOSE}")?;
    writeln!(output, "Development stage: {APPLICATION_STAGE}")?;
    writeln!(output, "License: {APPLICATION_LICENSE}")?;
    writeln!(output, "Repository: {APPLICATION_REPOSITORY}")
}

fn write_general_help<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(output, "{APPLICATION_NAME} — {APPLICATION_PURPOSE}.")?;
    writeln!(output, "{APPLICATION_STAGE}\n")?;
    writeln!(output, "Usage: {APPLICATION_NAME} [OPTIONS] [COMMAND]\n")?;
    writeln!(output, "Commands:")?;
    writeln!(
        output,
        "  gui           Launch the native GTK4 desktop application"
    )?;
    writeln!(
        output,
        "  help [TOPIC]  Show help for the application or a command"
    )?;
    writeln!(
        output,
        "  about         Show application and repository information\n"
    )?;
    writeln!(output, "Options:")?;
    writeln!(output, "  -h, --help    Show this help message")?;
    writeln!(output, "  --version     Show the application version\n")?;
    writeln!(
        output,
        "Running without arguments or with `gui` starts the same GTK4 application."
    )?;
    writeln!(
        output,
        "Static help, version, and about commands work without a graphical session."
    )?;
    writeln!(
        output,
        "Game, runtime, optimization, benchmarking, and daemon services are under development."
    )
}

fn write_gui_help<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(output, "Usage: {APPLICATION_NAME} gui\n")?;
    writeln!(output, "Launch the native GTK4 desktop application.")?;
    writeln!(
        output,
        "This uses the same startup path as running {APPLICATION_NAME} without arguments."
    )?;
    writeln!(output, "A graphical session is required.")
}

fn write_about_help<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(output, "Usage: {APPLICATION_NAME} about\n")?;
    writeln!(
        output,
        "Display the application's static purpose, version, development stage, license, and repository metadata."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comes_from_package_metadata() {
        let mut output = Vec::new();

        assert!(write_version(&mut output).is_ok());
        assert_eq!(
            String::from_utf8(output),
            Ok(format!("{APPLICATION_NAME} {APPLICATION_VERSION}\n"))
        );
    }

    #[test]
    fn about_contains_static_package_metadata() {
        let mut output = Vec::new();

        assert!(write_about(&mut output).is_ok());
        let output = String::from_utf8_lossy(&output);
        assert!(output.contains(APPLICATION_NAME));
        assert!(output.contains(APPLICATION_VERSION));
        assert!(output.contains(APPLICATION_PURPOSE));
        assert!(output.contains(APPLICATION_LICENSE));
        assert!(output.contains(APPLICATION_REPOSITORY));
        assert!(output.contains("under development"));
    }
}
