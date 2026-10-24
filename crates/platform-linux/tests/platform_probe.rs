mod common;

use endfield_unix_domain::{Architecture, DiagnosticCategory, PlatformFamily, PlatformProbe};
use endfield_unix_platform_linux::{
    DataSourceErrorKind, LinuxPlatformProbe, LinuxReportCompleteness, RawKernelIdentity,
};

#[test]
fn builds_a_valid_linux_snapshot_from_kernel_and_distribution_facts() {
    let report = LinuxPlatformProbe::with_source(common::FixtureSource::default())
        .inspect()
        .expect("platform report");
    assert_eq!(report.snapshot.platform.family, PlatformFamily::Linux);
    assert_eq!(report.snapshot.platform.os.name, "Linux");
    assert_eq!(report.snapshot.platform.architecture, Architecture::X86_64);
    assert_eq!(
        report
            .snapshot
            .platform
            .kernel
            .as_ref()
            .map(|kernel| kernel.version.as_deref()),
        Some(Some("6.12.0-41-generic"))
    );
    assert_eq!(
        report
            .snapshot
            .platform
            .distribution
            .as_ref()
            .map(|distro| distro.id.as_str()),
        Some("arch")
    );
    assert!(report.snapshot.validate().is_ok());
}

#[test]
fn kernel_only_result_is_valid_and_explicitly_partial() {
    let report = LinuxPlatformProbe::with_source(common::FixtureSource::missing_os_release())
        .inspect()
        .expect("kernel identity is sufficient");
    assert_eq!(report.completeness, LinuxReportCompleteness::Partial);
    assert_eq!(
        report.snapshot.platform.kernel.as_ref().unwrap().name,
        "Linux"
    );
    assert!(report.snapshot.platform.distribution.is_none());
    assert!(report.snapshot.validate().is_ok());
}

#[test]
fn missing_distribution_version_remains_none_in_domain_identity() {
    let source = common::FixtureSource {
        etc_os_release: Some(b"ID=arch\nNAME=\"Arch Linux\"\n".to_vec()),
        ..Default::default()
    };
    let report = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("probe report");
    let distribution = report
        .snapshot
        .platform
        .distribution
        .expect("distribution is present");
    assert_eq!(distribution.version, None);
}

#[test]
fn os_release_without_required_id_remains_a_valid_partial_kernel_report() {
    let source = common::FixtureSource {
        etc_os_release: Some(b"NAME=Unknown\nPRETTY_NAME=Unknown Linux\n".to_vec()),
        ..Default::default()
    };
    let report = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("kernel identity remains independently valid");
    assert!(report.snapshot.platform.kernel.is_some());
    assert!(report.snapshot.platform.distribution.is_none());
    assert_eq!(report.completeness, LinuxReportCompleteness::Partial);
}

#[test]
fn uname_kernel_machine_is_used_instead_of_process_build_architecture() {
    let source = common::FixtureSource {
        kernel: RawKernelIdentity {
            system_name: "Linux".to_owned(),
            release: "6.18.3-arch1-1".to_owned(),
            machine: "aarch64".to_owned(),
        },
        ..Default::default()
    };
    let report = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("report uses kernel observation");
    assert_eq!(report.snapshot.platform.architecture, Architecture::Aarch64);
    assert_eq!(report.snapshot.platform.architecture.as_str(), "aarch64");
}

#[test]
fn actual_kernel_release_identifiers_are_preserved() {
    for release in ["6.12.0-41-generic", "6.18.3-arch1-1", "7.2.9-zen1-1-zen"] {
        let source = common::FixtureSource {
            kernel: RawKernelIdentity {
                system_name: "Linux".to_owned(),
                release: release.to_owned(),
                machine: "x86_64".to_owned(),
            },
            ..Default::default()
        };
        let report = LinuxPlatformProbe::with_source(source)
            .inspect()
            .expect("valid release");
        assert_eq!(
            report
                .snapshot
                .platform
                .kernel
                .as_ref()
                .unwrap()
                .version
                .as_deref(),
            Some(release)
        );
    }
}

#[test]
fn unknown_kernel_architecture_is_preserved_as_other() {
    let source = common::FixtureSource {
        kernel: RawKernelIdentity {
            system_name: "Linux".to_owned(),
            release: "6.1.0".to_owned(),
            machine: "LoongArch64".to_owned(),
        },
        ..Default::default()
    };
    let report = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("unknown machine string is retained");
    assert_eq!(
        report.snapshot.platform.architecture,
        Architecture::Other("loongarch64".to_owned())
    );
}

#[test]
fn x86_32_bit_kernel_architecture_maps_to_the_portable_x86_variant() {
    let source = common::FixtureSource {
        kernel: RawKernelIdentity {
            system_name: "Linux".to_owned(),
            release: "6.1.0".to_owned(),
            machine: "i686".to_owned(),
        },
        ..Default::default()
    };
    let report = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("valid 32-bit machine name");
    assert_eq!(report.snapshot.platform.architecture, Architecture::X86);
}

#[test]
fn arm_and_risc_v_kernel_architectures_use_portable_domain_variants() {
    for (machine, expected) in [
        ("armv7l", Architecture::Arm),
        ("riscv64", Architecture::RiscV64),
    ] {
        let source = common::FixtureSource {
            kernel: RawKernelIdentity {
                system_name: "Linux".to_owned(),
                release: "6.1.0".to_owned(),
                machine: machine.to_owned(),
            },
            ..Default::default()
        };
        let report = LinuxPlatformProbe::with_source(source)
            .inspect()
            .expect("recognized kernel machine name");
        assert_eq!(report.snapshot.platform.architecture, expected);
    }
}

#[test]
fn missing_version_fields_are_not_fabricated_from_distribution_name() {
    let source = common::FixtureSource {
        etc_os_release: Some(b"ID=arch\nNAME=\"Arch Linux 2026.10\"\n".to_vec()),
        ..Default::default()
    };
    let report = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("probe report");
    let distro = report.snapshot.platform.distribution.unwrap();
    assert_eq!(distro.version, None);
    assert_eq!(distro.display_name.as_deref(), Some("Arch Linux 2026.10"));
}

#[test]
fn snapshot_has_no_fabricated_hardware_and_uses_safe_provenance() {
    let source = common::FixtureSource {
        runtime_path: Some(std::path::PathBuf::from("/private/user/runtime")),
        ..Default::default()
    };
    let report = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("probe report");
    assert!(report.snapshot.cpu.is_none());
    assert!(report.snapshot.gpus.is_empty());
    assert!(report.snapshot.memory.is_none());
    assert!(report.snapshot.displays.is_empty());
    assert_eq!(
        report
            .snapshot
            .provenance
            .iter()
            .map(|item| item.provider.as_str())
            .collect::<Vec<_>>(),
        vec!["linux-uname", "linux-os-release"]
    );
    let combined = format!("{:?}", report.snapshot.diagnostics);
    assert!(!combined.contains("/private/user/runtime"));
    assert!(!combined.contains("username"));
    assert!(report.snapshot.diagnostics.iter().any(|diagnostic| {
        diagnostic.provider.as_deref() == Some("linux-environment")
            && diagnostic.category == DiagnosticCategory::NotYetProbed
    }));
    assert!(report.snapshot.validate().is_ok());
}

#[test]
fn malformed_distribution_keeps_verified_kernel_and_does_not_echo_source_text() {
    let source = common::FixtureSource {
        etc_os_release: Some(b"ID=arch\nPRETTY_NAME=\"/home/private/secret\" trailing\n".to_vec()),
        ..Default::default()
    };
    let report = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("malformed distro is partial, not fatal");
    assert!(report.snapshot.platform.kernel.is_some());
    assert!(report.snapshot.platform.distribution.is_none());
    assert_eq!(report.completeness, LinuxReportCompleteness::Partial);
    let diagnostics = format!("{:?}", report.snapshot.diagnostics);
    assert!(!diagnostics.contains("/home/private/secret"));
}

#[test]
fn kernel_source_failure_is_a_typed_total_error() {
    let source = common::FixtureSource {
        kernel_error: Some(DataSourceErrorKind::PermissionDenied),
        ..Default::default()
    };
    let error = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect_err("kernel identity is required");
    assert_eq!(error.provider(), "linux-uname");
    assert_eq!(
        error.kind(),
        endfield_unix_domain::ProbeErrorKind::PermissionDenied
    );
}

#[test]
fn empty_and_overlong_kernel_fields_are_typed_malformed_errors() {
    for kernel in [
        RawKernelIdentity {
            system_name: "Linux".to_owned(),
            release: String::new(),
            machine: "x86_64".to_owned(),
        },
        RawKernelIdentity {
            system_name: "Linux".to_owned(),
            release: "x".repeat(129),
            machine: "x86_64".to_owned(),
        },
        RawKernelIdentity {
            system_name: "Linux".to_owned(),
            release: "6.1.0".to_owned(),
            machine: String::new(),
        },
    ] {
        let error = LinuxPlatformProbe::with_source(common::FixtureSource {
            kernel,
            ..Default::default()
        })
        .inspect()
        .expect_err("invalid kernel field");
        assert_eq!(
            error.kind(),
            endfield_unix_domain::ProbeErrorKind::MalformedData
        );
    }
}

#[test]
fn platform_trait_returns_the_same_validated_domain_snapshot() {
    let probe = LinuxPlatformProbe::with_source(common::FixtureSource::default());
    let snapshot = PlatformProbe::probe(&probe).expect("P007 platform probe");
    assert!(snapshot.validate().is_ok());
}
