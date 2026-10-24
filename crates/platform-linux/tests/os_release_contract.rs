mod common;

use std::fs;

use endfield_unix_platform_linux::{
    DataSourceErrorKind, LinuxDataSource, LinuxPlatformProbe, MAX_OS_RELEASE_BYTES, OsReleaseFile,
    SystemLinuxDataSource, parse_os_release,
};
use tempfile::tempdir;

#[test]
fn parses_arch_ubuntu_and_fedora_documents() {
    for (document, expected_id, expected_name) in [
        (
            "ID=arch\nNAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\n",
            "arch",
            "Arch Linux",
        ),
        (
            "NAME=Ubuntu\nVERSION_ID=24.04\nID=ubuntu\nPRETTY_NAME=\"Ubuntu 24.04.3 LTS\"\n",
            "ubuntu",
            "Ubuntu 24.04.3 LTS",
        ),
        (
            "NAME=\"Fedora Linux\"\nVERSION_ID=42\nID=fedora\nPRETTY_NAME=\"Fedora Linux 42 (Workstation Edition)\"\n",
            "fedora",
            "Fedora Linux 42 (Workstation Edition)",
        ),
    ] {
        let parsed = parse_os_release(document.as_bytes()).expect("valid os-release");
        assert_eq!(parsed.id.as_deref(), Some(expected_id));
        assert_eq!(parsed.pretty_name.as_deref(), Some(expected_name));
    }
}

#[test]
fn rolling_release_without_version_id_keeps_the_version_absent() {
    let parsed = parse_os_release(b"ID=arch\nNAME=\"Arch Linux\"\n").expect("valid release");
    assert_eq!(parsed.version_id, None);
    assert_eq!(parsed.version, None);
}

#[test]
fn version_falls_back_to_the_actual_version_field_when_version_id_is_absent() {
    let parsed =
        parse_os_release(b"ID=example\nVERSION=\"2026 rolling\"\n").expect("valid document");
    assert_eq!(parsed.version_id, None);
    assert_eq!(parsed.version.as_deref(), Some("2026 rolling"));
}

#[test]
fn quoted_and_supported_escaped_values_are_decoded_without_evaluation() {
    let parsed = parse_os_release(
        br#"ID=Example
NAME="Example \"Linux\""
PRETTY_NAME='Example Linux'
VERSION="1\$HOME"
VERSION_ID=1.0
ID_LIKE="debian ubuntu"
VARIANT_ID=server
"#,
    )
    .expect("valid quoted fields");
    assert_eq!(parsed.name.as_deref(), Some("Example \"Linux\""));
    assert_eq!(parsed.version.as_deref(), Some("1$HOME"));
    assert_eq!(parsed.id_like, vec!["debian", "ubuntu"]);
    assert_eq!(parsed.variant_id.as_deref(), Some("server"));
}

#[test]
fn parses_crlf_and_ordinary_line_whitespace() {
    let parsed = parse_os_release(b"  # comment\r\nID=arch\r\nNAME=\"Arch Linux\"\r\n")
        .expect("valid CRLF document");
    assert_eq!(parsed.id.as_deref(), Some("arch"));
}

#[test]
fn malformed_assignments_and_escapes_are_rejected() {
    for document in [
        b"ID".as_slice(),
        b"id=lowercase-key",
        b"ID=arch extra",
        b"ID=arch\\q",
        b"ID=\"unterminated",
        b"ID=arch\nBROKEN LINE\n",
    ] {
        assert!(parse_os_release(document).is_err(), "{document:?}");
    }
}

#[test]
fn duplicate_keys_use_the_last_valid_assignment() {
    let parsed = parse_os_release(b"ID=first\nID=second\n").expect("valid document");
    assert_eq!(parsed.id.as_deref(), Some("second"));
}

#[test]
fn unrelated_valid_keys_are_ignored() {
    let parsed = parse_os_release(b"ID=arch\nVENDOR_URL=https://example.invalid\n")
        .expect("unknown valid field is ignored");
    assert_eq!(parsed.id.as_deref(), Some("arch"));
}

#[test]
fn selected_fields_values_and_file_size_are_bounded() {
    let long_field = format!("ID=arch\nPRETTY_NAME=\"{}\"\n", "x".repeat(129));
    assert!(parse_os_release(long_field.as_bytes()).is_err());
    assert!(parse_os_release(&vec![b'x'; MAX_OS_RELEASE_BYTES + 1]).is_err());
    assert!(parse_os_release(format!("ID=arch\n#{}", "x".repeat(4097)).as_bytes()).is_err());
}

#[test]
fn invalid_utf8_is_rejected() {
    assert!(parse_os_release(b"ID=arch\nNAME=\xff").is_err());
}

#[test]
fn preferred_etc_source_wins_and_fallback_is_used_only_when_missing() {
    let source = common::FixtureSource {
        etc_os_release: Some(b"ID=etc\nNAME=Etc\n".to_vec()),
        usr_lib_os_release: Some(b"ID=usr\nNAME=Usr\n".to_vec()),
        ..Default::default()
    };
    let report = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("probe succeeds");
    assert_eq!(
        report
            .distribution
            .as_ref()
            .map(|distro| distro.id.as_str()),
        Some("etc")
    );
    assert_eq!(
        report.distribution.as_ref().map(|distro| distro.source),
        Some("etc-os-release")
    );

    let source = common::FixtureSource {
        etc_os_release: None,
        usr_lib_os_release: Some(b"ID=usr\nNAME=Usr\n".to_vec()),
        ..Default::default()
    };
    let report = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("fallback probe succeeds");
    assert_eq!(
        report
            .distribution
            .as_ref()
            .map(|distro| distro.id.as_str()),
        Some("usr")
    );
    assert_eq!(
        report.distribution.as_ref().map(|distro| distro.source),
        Some("usr-lib-os-release")
    );
}

#[test]
fn etc_permission_error_does_not_merge_or_fall_back_to_usr_lib() {
    let source = common::FixtureSource {
        etc_os_release: None,
        usr_lib_os_release: Some(b"ID=usr\nNAME=Usr\n".to_vec()),
        etc_error: Some(DataSourceErrorKind::PermissionDenied),
        ..Default::default()
    };
    let report = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("kernel report remains valid");
    assert!(report.distribution.is_none());
    assert_eq!(
        report.completeness,
        endfield_unix_platform_linux::LinuxReportCompleteness::Partial
    );
}

#[test]
fn malformed_etc_metadata_does_not_fall_back_to_usr_lib() {
    let source = common::FixtureSource {
        etc_os_release: Some(b"ID=arch\nPRETTY_NAME=\"broken\" trailing\n".to_vec()),
        usr_lib_os_release: Some(b"ID=ubuntu\nNAME=Ubuntu\n".to_vec()),
        ..Default::default()
    };
    let report = LinuxPlatformProbe::with_source(source)
        .inspect()
        .expect("kernel report remains valid");
    assert!(report.distribution.is_none());
    assert_eq!(
        report.completeness,
        endfield_unix_platform_linux::LinuxReportCompleteness::Partial
    );
}

#[test]
fn valid_etc_symlink_to_usr_lib_file_is_supported() {
    let root = tempdir().expect("temp root");
    fs::create_dir_all(root.path().join("etc")).expect("etc directory");
    fs::create_dir_all(root.path().join("usr/lib")).expect("usr lib directory");
    fs::write(root.path().join("usr/lib/os-release"), b"ID=arch\n").expect("release file");
    std::os::unix::fs::symlink("../usr/lib/os-release", root.path().join("etc/os-release"))
        .expect("valid symlink");
    let source = SystemLinuxDataSource::rooted_at(root.path());
    assert_eq!(
        source.read_os_release(OsReleaseFile::Etc),
        Ok(b"ID=arch\n".to_vec())
    );
}

#[test]
fn unexpected_symlink_and_directory_sources_are_rejected() {
    let root = tempdir().expect("temp root");
    fs::create_dir_all(root.path().join("etc")).expect("etc directory");
    fs::create_dir_all(root.path().join("usr/lib")).expect("usr lib directory");
    fs::write(root.path().join("elsewhere"), b"ID=other\n").expect("outside file");
    std::os::unix::fs::symlink("../elsewhere", root.path().join("etc/os-release"))
        .expect("unsafe symlink");
    let source = SystemLinuxDataSource::rooted_at(root.path());
    assert_eq!(
        source
            .read_os_release(OsReleaseFile::Etc)
            .map_err(|error| error.kind),
        Err(DataSourceErrorKind::UnexpectedType)
    );

    fs::remove_file(root.path().join("etc/os-release")).expect("remove link");
    fs::create_dir(root.path().join("etc/os-release")).expect("unexpected directory");
    assert_eq!(
        source
            .read_os_release(OsReleaseFile::Etc)
            .map_err(|error| error.kind),
        Err(DataSourceErrorKind::UnexpectedType)
    );
}
