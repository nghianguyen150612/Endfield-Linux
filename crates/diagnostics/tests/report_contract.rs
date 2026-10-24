use endfield_unix_diagnostics::{
    BuildStatus, DiagnosticEvent, DiagnosticsError, EventCode, LogHistoryStatus,
    REPORT_SCHEMA_VERSION, Severity, SupportReport, write_report_to_path,
};
use std::fs;
use tempfile::tempdir;

fn fixture_report(timestamp: u64) -> SupportReport {
    let event = DiagnosticEvent::new_at(
        Severity::Warn,
        endfield_unix_diagnostics::Component::Diagnostics,
        EventCode::DiagnosticsUnavailable,
        100,
    );
    SupportReport::from_events_at(vec![event], timestamp, LogHistoryStatus::Missing, 0)
}

#[test]
fn report_json_is_versioned_valid_and_deterministic_for_fixed_timestamps() {
    let first = fixture_report(900);
    let second = fixture_report(900);
    let first_json = first.to_json().expect("serialize report");
    let second_json = second.to_json().expect("serialize report");
    assert_eq!(first_json, second_json);
    let parsed: serde_json::Value = serde_json::from_slice(&first_json).expect("valid JSON");
    assert_eq!(parsed["schema_version"], REPORT_SCHEMA_VERSION);
    assert_eq!(parsed["generated_at_unix_nanos"], 900);
    assert_eq!(parsed["application"]["name"], "endfield-unix");
    assert_eq!(parsed["application"]["build_status"], "development");
    assert_eq!(parsed["log_history"]["status"], "missing");
    assert_eq!(parsed["log_history"]["event_counts"]["warn"], 1);
    assert_eq!(parsed["diagnostics"][0]["timestamp_unix_nanos"], 100);
    assert_eq!(
        parsed["diagnostics"][0]["correlation_id"],
        serde_json::Value::Null
    );
}

#[test]
fn support_report_is_bounded_and_has_no_measured_hardware_claims() {
    let report = fixture_report(1);
    let json = report.to_json().expect("bounded report");
    assert!(json.len() <= endfield_unix_diagnostics::MAX_REPORT_BYTES);
    let parsed: serde_json::Value = serde_json::from_slice(&json).expect("valid JSON");
    assert_eq!(parsed["components"][0]["component"], "application");
    let components = parsed["components"].as_array().expect("component array");
    assert!(components.iter().any(|component| {
        component["component"] == "hardware_probes" && component["status"] == "not_implemented"
    }));
    assert!(
        !json
            .windows(b"temperature".len())
            .any(|window| window == b"temperature")
    );
}

#[test]
fn report_export_uses_explicit_private_destination_without_silent_overwrite() {
    let directory = tempdir().expect("temporary directory");
    let destination = directory.path().join("support-report.json");
    let report = fixture_report(55);

    write_report_to_path(&report, &destination, false).expect("write report");
    let original = fs::read(&destination).expect("report file");
    assert_eq!(
        write_report_to_path(&fixture_report(66), &destination, false),
        Err(DiagnosticsError::OutputExists)
    );
    assert_eq!(fs::read(&destination).expect("report unchanged"), original);
    write_report_to_path(&fixture_report(66), &destination, true).expect("explicit overwrite");
    assert_ne!(fs::read(&destination).expect("replaced report"), original);
}

#[cfg(unix)]
#[test]
fn report_export_refuses_symlink_destinations_and_keeps_target_unchanged() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory");
    let target = directory.path().join("target.json");
    let destination = directory.path().join("support-report.json");
    fs::write(&target, b"keep this content").expect("target file");
    symlink(&target, &destination).expect("destination symlink");

    assert!(matches!(
        write_report_to_path(&fixture_report(77), &destination, true),
        Err(DiagnosticsError::UnsafePath(_))
    ));
    assert_eq!(
        fs::read(target).expect("target unchanged"),
        b"keep this content"
    );
}

#[cfg(unix)]
#[test]
fn report_file_permissions_are_restricted_to_the_current_user() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory");
    let destination = directory.path().join("support-report.json");
    write_report_to_path(&fixture_report(88), &destination, false).expect("write report");
    let mode = fs::metadata(destination)
        .expect("report metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn report_generation_does_not_use_an_unbounded_event_list() {
    let events = (0..endfield_unix_diagnostics::MAX_REPORT_EVENTS + 10)
        .map(|timestamp| {
            DiagnosticEvent::new_at(
                Severity::Info,
                endfield_unix_diagnostics::Component::Application,
                EventCode::AppStarting,
                timestamp as u64,
            )
        })
        .collect();
    let report = SupportReport::from_events_at(events, 999, LogHistoryStatus::Available, 0);
    let json = report.to_json().expect("bounded report");
    let parsed: serde_json::Value = serde_json::from_slice(&json).expect("valid JSON");
    assert_eq!(
        parsed["diagnostics"].as_array().expect("diagnostics").len(),
        endfield_unix_diagnostics::MAX_REPORT_EVENTS
    );
    assert_eq!(
        parsed["log_history"]["event_counts"]["info"],
        endfield_unix_diagnostics::MAX_REPORT_EVENTS + 10
    );
}

#[test]
fn a_missing_output_parent_fails_without_creating_directories() {
    let directory = tempdir().expect("temporary directory");
    let destination = directory.path().join("missing").join("support-report.json");
    assert!(write_report_to_path(&fixture_report(101), &destination, false).is_err());
    let parent = directory.path().join("missing");
    assert!(!parent.exists());
}

#[test]
fn report_application_metadata_is_static() {
    let report = fixture_report(1);
    assert_eq!(report.application.name, "endfield-unix");
    assert_eq!(report.application.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(report.application.build_status, BuildStatus::Development);
}
