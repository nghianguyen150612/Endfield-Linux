mod common;

use std::fs;
use std::path::PathBuf;

use endfield_unix_platform_linux::{
    DataSourceErrorKind, EnvironmentClassification, EnvironmentConfidence, EnvironmentStatus,
    LinuxDataSource, LinuxPlatformProbe, RawKernelIdentity, RuntimeDirectoryStatus,
    RuntimeEnvironmentFacts, SystemLinuxDataSource,
};
use tempfile::tempdir;

fn default_facts() -> RuntimeEnvironmentFacts {
    LinuxPlatformProbe::with_source(common::FixtureSource::default())
        .inspect()
        .expect("report")
        .runtime_environment
}

#[test]
fn recognized_marker_identifies_a_container_without_exposing_ids() {
    let source = common::FixtureSource {
        markers: [Ok(true), Ok(false), Ok(false)],
        ..Default::default()
    };
    let facts = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("report")
        .runtime_environment;
    assert_eq!(facts.classification, EnvironmentClassification::Container);
    assert_eq!(facts.confidence, EnvironmentConfidence::High);
    assert_eq!(facts.status, EnvironmentStatus::Identified);
    assert!(facts.source_ids.contains(&"linux-dockerenv"));
}

#[test]
fn systemd_container_metadata_can_identify_a_container() {
    let source = common::FixtureSource {
        markers: [Ok(false), Ok(false), Ok(true)],
        ..Default::default()
    };
    let facts = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("report")
        .runtime_environment;
    assert_eq!(facts.classification, EnvironmentClassification::Container);
    assert!(facts.source_ids.contains(&"linux-systemd-container"));
}

#[test]
fn podman_marker_is_recognized() {
    let source = common::FixtureSource {
        markers: [Ok(false), Ok(true), Ok(false)],
        ..Default::default()
    };
    let facts = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("report")
        .runtime_environment;
    assert_eq!(facts.classification, EnvironmentClassification::Container);
    assert!(facts.source_ids.contains(&"linux-containerenv"));
}

#[test]
fn systemd_container_source_accepts_only_fixed_recognized_values() {
    let root = tempdir().expect("temporary root");
    let path = root.path().join("run/systemd");
    fs::create_dir_all(&path).expect("systemd directory");
    let marker = path.join("container");
    fs::write(&marker, b"podman\n").expect("container marker");
    let source = SystemLinuxDataSource::rooted_at(root.path());
    assert_eq!(
        source.container_marker_present(endfield_unix_platform_linux::ContainerMarker::Systemd),
        Ok(true)
    );
    fs::write(&marker, b"private-container-id\n").expect("unknown value");
    assert_eq!(
        source.container_marker_present(endfield_unix_platform_linux::ContainerMarker::Systemd),
        Ok(false)
    );
}

#[test]
fn wsl_kernel_marker_is_detected_without_claiming_a_generation() {
    let source = common::FixtureSource {
        kernel: RawKernelIdentity {
            system_name: "Linux".to_owned(),
            release: "5.15.146.1-microsoft-standard-WSL2".to_owned(),
            machine: "x86_64".to_owned(),
        },
        ..Default::default()
    };
    let facts = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("report")
        .runtime_environment;
    assert_eq!(facts.classification, EnvironmentClassification::Wsl);
    assert_eq!(facts.confidence, EnvironmentConfidence::High);
    assert_eq!(
        facts.uncertainty,
        Some("WSL generation and nested-container state are not inferred")
    );
}

#[test]
fn no_markers_are_low_confidence_and_do_not_prove_bare_metal() {
    let facts = default_facts();
    assert_eq!(facts.classification, EnvironmentClassification::NativeLinux);
    assert_eq!(facts.confidence, EnvironmentConfidence::Low);
    assert_eq!(facts.status, EnvironmentStatus::Uncertain);
    assert!(
        facts
            .uncertainty
            .unwrap()
            .contains("bare-metal status is not established")
    );
}

#[test]
fn conflicting_wsl_and_container_indicators_return_unknown() {
    let source = common::FixtureSource {
        kernel: RawKernelIdentity {
            system_name: "Linux".to_owned(),
            release: "6.1.0-microsoft".to_owned(),
            machine: "x86_64".to_owned(),
        },
        markers: [Ok(false), Ok(true), Ok(false)],
        ..Default::default()
    };
    let facts = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("report")
        .runtime_environment;
    assert_eq!(facts.classification, EnvironmentClassification::Unknown);
    assert_eq!(facts.status, EnvironmentStatus::Uncertain);
}

#[test]
fn inaccessible_markers_keep_classification_uncertain() {
    let source = common::FixtureSource {
        markers: [
            Err(DataSourceErrorKind::PermissionDenied),
            Ok(false),
            Ok(false),
        ],
        ..Default::default()
    };
    let facts = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("report is partial, not fatal");
    assert_eq!(
        facts.runtime_environment.classification,
        EnvironmentClassification::Unknown
    );
    assert_eq!(
        facts.runtime_environment.confidence,
        EnvironmentConfidence::Low
    );
    assert_eq!(
        facts.completeness,
        endfield_unix_platform_linux::LinuxReportCompleteness::Partial
    );
}

#[test]
fn missing_runtime_directory_is_reported() {
    let source = common::FixtureSource {
        runtime_path: None,
        ..Default::default()
    };
    let assessment = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("report")
        .user_runtime_directory;
    assert_eq!(assessment.status, RuntimeDirectoryStatus::Missing);
}

#[test]
fn runtime_directory_must_match_the_p010_owner_mode_and_symlink_policy() {
    let secure = LinuxPlatformProbe::with_source(common::FixtureSource::default())
        .inspect()
        .expect("report");
    assert_eq!(
        secure.user_runtime_directory.status,
        RuntimeDirectoryStatus::Available
    );

    for metadata in [
        endfield_unix_platform_linux::RuntimeDirectoryMetadata {
            is_directory: true,
            is_symlink: false,
            owner_uid: 1001,
            mode: 0o700,
        },
        endfield_unix_platform_linux::RuntimeDirectoryMetadata {
            is_directory: true,
            is_symlink: false,
            owner_uid: 1000,
            mode: 0o750,
        },
        endfield_unix_platform_linux::RuntimeDirectoryMetadata {
            is_directory: true,
            is_symlink: true,
            owner_uid: 1000,
            mode: 0o700,
        },
        endfield_unix_platform_linux::RuntimeDirectoryMetadata {
            is_directory: false,
            is_symlink: false,
            owner_uid: 1000,
            mode: 0o700,
        },
    ] {
        let source = common::FixtureSource {
            runtime_metadata: Some(metadata),
            ..Default::default()
        };
        let assessment = LinuxPlatformProbe::with_source(source)
            .inspect()
            .expect("platform remains available")
            .user_runtime_directory;
        assert_eq!(assessment.status, RuntimeDirectoryStatus::InvalidOrUnsafe);
    }
}

#[test]
fn permission_denied_and_unknown_runtime_metadata_are_distinct() {
    let denied = common::FixtureSource {
        runtime_metadata_error: Some(DataSourceErrorKind::PermissionDenied),
        ..Default::default()
    };
    assert_eq!(
        LinuxPlatformProbe::with_source(denied)
            .inspect()
            .expect("partial report")
            .user_runtime_directory
            .status,
        RuntimeDirectoryStatus::PermissionDenied
    );

    let unknown = common::FixtureSource {
        runtime_metadata_error: Some(DataSourceErrorKind::Other),
        ..Default::default()
    };
    assert_eq!(
        LinuxPlatformProbe::with_source(unknown)
            .inspect()
            .expect("partial report")
            .user_runtime_directory
            .status,
        RuntimeDirectoryStatus::Unknown
    );
}

#[test]
fn relative_runtime_directory_is_unsafe_without_metadata_access() {
    let source = common::FixtureSource {
        runtime_path: Some(PathBuf::from("relative/path")),
        ..Default::default()
    };
    let assessment = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("report")
        .user_runtime_directory;
    assert_eq!(assessment.status, RuntimeDirectoryStatus::InvalidOrUnsafe);
}

#[test]
fn runtime_directory_is_rejected_when_the_derived_socket_path_exceeds_the_linux_limit() {
    let source = common::FixtureSource {
        runtime_path: Some(PathBuf::from(format!("/{}", "x".repeat(100)))),
        ..Default::default()
    };
    let assessment = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("kernel report remains valid")
        .user_runtime_directory;
    assert_eq!(assessment.status, RuntimeDirectoryStatus::InvalidOrUnsafe);
}
