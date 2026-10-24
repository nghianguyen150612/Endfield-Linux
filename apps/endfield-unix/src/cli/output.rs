use std::io::{self, Write};

use endfield_unix_platform_linux::{
    EnvironmentClassification, LinuxPlatformReport, LinuxReportCompleteness, RuntimeDirectoryStatus,
};

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
        Some(HelpTopic::Service) => write_service_help(output),
        Some(HelpTopic::Diagnose) => write_diagnose_help(output),
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

pub fn write_platform_report<W: Write>(
    report: &LinuxPlatformReport,
    output: &mut W,
) -> io::Result<()> {
    writeln!(output, "Platform: Linux")?;
    if let Some(distribution) = &report.distribution {
        let name = distribution
            .pretty_name
            .as_deref()
            .or(distribution.name.as_deref())
            .unwrap_or(&distribution.id);
        let version = distribution
            .version_id
            .as_deref()
            .or(distribution.version.as_deref());
        match version {
            Some(version) => writeln!(
                output,
                "Distribution: {name} (id: {}; version: {version})",
                distribution.id
            )?,
            None => writeln!(output, "Distribution: {name} (id: {})", distribution.id)?,
        }
    } else {
        writeln!(
            output,
            "Distribution: unknown (metadata unavailable or invalid)"
        )?;
    }

    let kernel = report
        .snapshot
        .platform
        .kernel
        .as_ref()
        .and_then(|kernel| kernel.version.as_deref())
        .unwrap_or("unknown");
    writeln!(output, "Kernel: Linux {kernel}")?;
    writeln!(
        output,
        "Architecture: {} (kernel-reported)",
        report.snapshot.platform.architecture.as_str()
    )?;

    let (environment_name, confidence) = match report.runtime_environment.classification {
        EnvironmentClassification::NativeLinux => (
            "native Linux process",
            confidence_name(report.runtime_environment.confidence),
        ),
        EnvironmentClassification::Container => (
            "container",
            confidence_name(report.runtime_environment.confidence),
        ),
        EnvironmentClassification::Wsl => (
            "WSL",
            confidence_name(report.runtime_environment.confidence),
        ),
        EnvironmentClassification::Unknown => (
            "unknown",
            confidence_name(report.runtime_environment.confidence),
        ),
    };
    write!(
        output,
        "Environment: {environment_name} ({confidence} confidence"
    )?;
    if !report.runtime_environment.source_ids.is_empty() {
        write!(
            output,
            "; sources: {}",
            report.runtime_environment.source_ids.join(", ")
        )?;
    }
    writeln!(output, ")")?;
    if let Some(uncertainty) = report.runtime_environment.uncertainty {
        writeln!(output, "Environment uncertainty: {uncertainty}")?;
    }
    if let Some(diagnostic) = report.runtime_environment.diagnostic {
        writeln!(output, "Environment note: {diagnostic}")?;
    }

    let runtime_directory = match report.user_runtime_directory.status {
        RuntimeDirectoryStatus::Available => "available and valid",
        RuntimeDirectoryStatus::Missing => "missing",
        RuntimeDirectoryStatus::InvalidOrUnsafe => "unsafe",
        RuntimeDirectoryStatus::PermissionDenied => "permission denied",
        RuntimeDirectoryStatus::Unknown => "unknown",
    };
    writeln!(
        output,
        "User runtime directory: {runtime_directory} ({})",
        report.user_runtime_directory.source_id
    )?;
    if let Some(diagnostic) = report.user_runtime_directory.diagnostic {
        writeln!(output, "Runtime-directory note: {diagnostic}")?;
    }
    let completeness = match report.completeness {
        LinuxReportCompleteness::Complete => "complete",
        LinuxReportCompleteness::Partial => "partial",
    };
    writeln!(output, "Probe result: {completeness}")
}

fn confidence_name(
    confidence: endfield_unix_platform_linux::EnvironmentConfidence,
) -> &'static str {
    match confidence {
        endfield_unix_platform_linux::EnvironmentConfidence::High => "high",
        endfield_unix_platform_linux::EnvironmentConfidence::Moderate => "moderate",
        endfield_unix_platform_linux::EnvironmentConfidence::Low => "low",
    }
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
    writeln!(
        output,
        "  diagnose report --output PATH  Write a privacy-safe support report"
    )?;
    writeln!(
        output,
        "  diagnose platform  Inspect Linux platform facts without the daemon"
    )?;
    writeln!(output, "  service ping  Verify authenticated daemon IPC")?;
    writeln!(
        output,
        "  service status  Show validated daemon lifecycle status"
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
        "The service commands require an independently running endfield-unixd daemon."
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

fn write_service_help<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(output, "Usage: {APPLICATION_NAME} service <ping|status>\n")?;
    writeln!(
        output,
        "Ping verifies a real authenticated local Ping/Pong exchange."
    )?;
    writeln!(
        output,
        "Status prints lifecycle and implemented component capabilities."
    )?;
    writeln!(
        output,
        "The commands never start the daemon or initialize GTK."
    )
}

fn write_diagnose_help<W: Write>(output: &mut W) -> io::Result<()> {
    writeln!(
        output,
        "Usage: {APPLICATION_NAME} diagnose <report|platform>\n"
    )?;
    writeln!(
        output,
        "Platform performs bounded, read-only Linux discovery and works without GTK or the daemon."
    )?;
    writeln!(
        output,
        "Report writes only to its explicit output path and does not inspect the host platform."
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
    fn platform_output_labels_unknown_facts_and_partial_results() {
        use endfield_unix_domain::{
            Architecture, KernelIdentity, OsIdentity, PlatformFamily, PlatformIdentity,
            SystemSnapshot,
        };
        use endfield_unix_platform_linux::{
            EnvironmentClassification, EnvironmentConfidence, EnvironmentStatus,
            LINUX_PLATFORM_REPORT_SCHEMA_VERSION, LinuxPlatformReport, LinuxReportCompleteness,
            RuntimeDirectoryAssessment, RuntimeDirectoryStatus, RuntimeEnvironmentFacts,
        };

        let platform = PlatformIdentity::new(
            PlatformFamily::Linux,
            OsIdentity::new("Linux", None).expect("OS identity"),
            Architecture::Other("loongarch64".to_owned()),
            Some(KernelIdentity::new("Linux", Some("6.1.0".to_owned())).expect("kernel")),
            None,
        )
        .expect("platform");
        let report = LinuxPlatformReport {
            schema_version: LINUX_PLATFORM_REPORT_SCHEMA_VERSION,
            snapshot: SystemSnapshot::new(platform),
            distribution: None,
            runtime_environment: RuntimeEnvironmentFacts {
                classification: EnvironmentClassification::Unknown,
                status: EnvironmentStatus::Uncertain,
                confidence: EnvironmentConfidence::Low,
                source_ids: vec!["linux-environment"],
                diagnostic: Some("environment indicators could not all be checked"),
                uncertainty: Some("bare-metal status is not established"),
            },
            user_runtime_directory: RuntimeDirectoryAssessment {
                status: RuntimeDirectoryStatus::Unknown,
                source_id: "linux-xdg-runtime-dir",
                diagnostic: None,
            },
            completeness: LinuxReportCompleteness::Partial,
        };
        let mut output = Vec::new();
        write_platform_report(&report, &mut output).expect("report output");
        let output = String::from_utf8(output).expect("UTF-8");
        assert!(output.contains("Distribution: unknown"));
        assert!(output.contains("Architecture: loongarch64 (kernel-reported)"));
        assert!(output.contains("Environment: unknown (low confidence"));
        assert!(output.contains("User runtime directory: unknown"));
        assert!(output.contains("Probe result: partial"));
    }

    #[test]
    fn general_help_lists_platform_diagnosis() {
        let mut output = Vec::new();
        assert!(write_help(None, &mut output).is_ok());
        assert!(String::from_utf8_lossy(&output).contains("diagnose platform"));
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
