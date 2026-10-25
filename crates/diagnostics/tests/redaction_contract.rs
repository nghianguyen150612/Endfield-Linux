use endfield_unix_diagnostics::{
    Component, DiagnosticEvent, EventCode, OperationOutcome, ReportField, SafeDiagnosticCategory,
    SensitivityClass, Severity, SupportReport, event_for_probe_success,
    event_for_unimplemented_operation, event_from_domain_diagnostic, event_from_probe_error,
};
use endfield_unix_domain::{Diagnostic, DiagnosticCategory, ProbeError, ProbeErrorKind};

#[test]
fn adversarial_domain_details_and_correlation_ids_never_enter_json_reports() {
    let long_untrusted = "L".repeat(300);
    let fixtures = [
        "/home/alice/.config/endfield-unix/private.toml",
        "username=alice",
        "ghp_FAKE_TOKEN_123456",
        "password=hunter2",
        "Authorization: Bearer fake-bearer-789",
        "https://alice:uriSecret@example.invalid/?token=query-secret",
        "aPi_KeY=FAKE_API_KEY",
        "Cookie: session=FAKE_SESSION; session_id=FAKE_ID",
        "endfield-unix --password cmdSecret",
        "END_FIELD_PRIVATE=env-secret",
        "192.0.2.99 aa:bb:cc:dd:ee:ff",
        "用户名称=秘密",
        long_untrusted.as_str(),
    ];
    let mut events = Vec::new();
    for (index, fixture) in fixtures.iter().enumerate() {
        let diagnostic = Diagnostic::new(
            DiagnosticCategory::TemporaryFailure,
            Some("future-provider".to_owned()),
            Some((*fixture).to_owned()),
        )
        .expect("synthetic diagnostic within the domain's text bound");
        let mut event = event_from_domain_diagnostic(&diagnostic, Some(13), 10 + index as u64)
            .expect("safe structured event");
        if index == 0 {
            event = event
                .with_correlation_id("session-id-TEST-123")
                .expect("test correlation ID");
        }
        events.push(event);
    }
    let report = SupportReport::from_events_at(
        events,
        20,
        endfield_unix_diagnostics::LogHistoryStatus::Available,
        0,
    );
    let json = String::from_utf8(report.to_json().expect("report JSON")).expect("UTF-8 JSON");

    for fixture in fixtures {
        assert!(!json.contains(fixture), "report leaked {fixture:?}");
    }
    assert!(
        Diagnostic::new(
            DiagnosticCategory::TemporaryFailure,
            None,
            Some("Authorization:\n  Bearer multi-line-secret".to_owned()),
        )
        .is_err()
    );
    assert!(!json.contains("multi-line-secret"));
    assert!(!json.contains("session-id-TEST-123"));
    assert!(!json.contains("future-provider"));
    assert!(json.contains("temporary_failure"));
    assert!(json.contains("A structured domain diagnostic was recorded."));
}

#[test]
fn unknown_and_unsupported_categories_remain_distinct_and_missing_provider_is_not_incompatibility()
{
    let missing = Diagnostic::new(
        DiagnosticCategory::MissingProvider,
        Some("future-provider".to_owned()),
        Some("a provider was not configured".to_owned()),
    )
    .expect("missing-provider diagnostic");
    let missing_event = event_from_domain_diagnostic(&missing, None, 1).expect("mapped event");
    assert_eq!(
        missing_event.category(),
        Some(SafeDiagnosticCategory::MissingProvider)
    );
    assert_eq!(missing_event.outcome(), Some(OperationOutcome::Unavailable));

    let unsupported = Diagnostic::new(
        DiagnosticCategory::UnsupportedCapability,
        None,
        Some("unsupported by provider".to_owned()),
    )
    .expect("unsupported diagnostic");
    let unsupported_event =
        event_from_domain_diagnostic(&unsupported, Some(14), 2).expect("mapped event");
    assert_eq!(
        unsupported_event.category(),
        Some(SafeDiagnosticCategory::UnsupportedCapability)
    );
    assert_eq!(
        unsupported_event.outcome(),
        Some(OperationOutcome::Unavailable)
    );
    assert_ne!(missing_event.category(), unsupported_event.category());
}

#[test]
fn all_domain_categories_map_to_explicit_operation_outcomes() {
    let cases = [
        (
            DiagnosticCategory::UnsupportedCapability,
            SafeDiagnosticCategory::UnsupportedCapability,
            OperationOutcome::Unavailable,
        ),
        (
            DiagnosticCategory::MissingProvider,
            SafeDiagnosticCategory::MissingProvider,
            OperationOutcome::Unavailable,
        ),
        (
            DiagnosticCategory::PermissionDenied,
            SafeDiagnosticCategory::PermissionDenied,
            OperationOutcome::Failed,
        ),
        (
            DiagnosticCategory::TemporaryFailure,
            SafeDiagnosticCategory::TemporaryFailure,
            OperationOutcome::Failed,
        ),
        (
            DiagnosticCategory::MalformedSourceData,
            SafeDiagnosticCategory::MalformedSourceData,
            OperationOutcome::Failed,
        ),
        (
            DiagnosticCategory::StaleObservation,
            SafeDiagnosticCategory::StaleObservation,
            OperationOutcome::Incomplete,
        ),
        (
            DiagnosticCategory::NotYetProbed,
            SafeDiagnosticCategory::NotYetProbed,
            OperationOutcome::NotAttempted,
        ),
    ];
    for (domain_category, expected_category, expected_outcome) in cases {
        let diagnostic = Diagnostic::new(domain_category, None, None).expect("diagnostic");
        let event = event_from_domain_diagnostic(&diagnostic, None, 5).expect("mapped event");
        assert_eq!(event.category(), Some(expected_category));
        assert_eq!(event.outcome(), Some(expected_outcome));
    }
}

#[test]
fn success_and_not_implemented_remain_distinct() {
    assert_eq!(
        event_for_probe_success(1).outcome(),
        Some(OperationOutcome::Succeeded)
    );
    assert_eq!(
        event_for_unimplemented_operation(Component::PlatformProbe, 2).outcome(),
        Some(OperationOutcome::NotImplemented)
    );
}

#[test]
fn probe_error_detail_is_omitted_and_not_yet_probed_is_not_a_failure() {
    let error = ProbeError::new(
        "linux-procfs",
        ProbeErrorKind::NotYetProbed,
        Some("/home/alice/private command output".to_owned()),
    )
    .expect("synthetic probe error");
    let event = event_from_probe_error(&error, 123).expect("event adapter");
    assert_eq!(event.outcome(), Some(OperationOutcome::NotAttempted));
    assert_eq!(event.category(), Some(SafeDiagnosticCategory::NotYetProbed));
    assert_eq!(
        event.provider().map(|provider| provider.as_str()),
        Some("linux-procfs")
    );

    let mut inconsistent = serde_json::to_value(&event).expect("serialize diagnostic event");
    inconsistent["outcome"] = serde_json::Value::String("failed".to_owned());
    assert!(serde_json::from_value::<DiagnosticEvent>(inconsistent).is_err());

    let report = SupportReport::from_events_at(
        vec![event],
        456,
        endfield_unix_diagnostics::LogHistoryStatus::Available,
        0,
    );
    let json = String::from_utf8(report.to_json().expect("report JSON")).expect("UTF-8");
    assert!(!json.contains("/home/alice"));
    assert!(json.contains("not_yet_probed"));
    assert!(json.contains("not_attempted"));
}

#[test]
fn p011_provider_names_are_allowlisted_while_details_remain_discarded() {
    for provider_name in [
        "linux-uname",
        "linux-os-release",
        "linux-environment",
        "linux-xdg-runtime-dir",
    ] {
        let diagnostic = Diagnostic::new(
            DiagnosticCategory::PermissionDenied,
            Some(provider_name.to_owned()),
            Some("private path and source text".to_owned()),
        )
        .expect("bounded synthetic diagnostic");
        let event = event_from_domain_diagnostic(&diagnostic, None, 123)
            .expect("allowlisted provider event");
        assert_eq!(
            event.provider().map(|provider| provider.as_str()),
            Some(provider_name)
        );
        let serialized = serde_json::to_string(&event).expect("serialized event");
        assert!(!serialized.contains("private path"));
    }
}

#[test]
fn p012_provider_names_are_allowlisted_while_details_remain_discarded() {
    for provider_name in [
        "linux-cpu-sysfs",
        "linux-cpuinfo",
        "linux-meminfo",
        "linux-cgroup",
        "linux-cpufreq",
        "linux-scheduler",
        "linux-power",
    ] {
        let diagnostic = Diagnostic::new(
            DiagnosticCategory::PermissionDenied,
            Some(provider_name.to_owned()),
            Some("private path and source text".to_owned()),
        )
        .expect("bounded synthetic diagnostic");
        let event = event_from_domain_diagnostic(&diagnostic, None, 123)
            .expect("allowlisted provider event");
        assert_eq!(
            event.provider().map(|provider| provider.as_str()),
            Some(provider_name)
        );
        let serialized = serde_json::to_string(&event).expect("serialized event");
        assert!(!serialized.contains("private path"));
    }
}

#[test]
fn sensitivity_classes_mark_only_static_and_structured_values_as_reportable() {
    assert_eq!(
        ReportField::ApplicationName.sensitivity(),
        SensitivityClass::SafeStaticMetadata
    );
    assert_eq!(
        ReportField::DiagnosticCategory.sensitivity(),
        SensitivityClass::SafeStructuredDiagnostic
    );
    assert_eq!(
        ReportField::CorrelationIdentifier.sensitivity(),
        SensitivityClass::PotentiallyIdentifying
    );
    assert_eq!(
        ReportField::Credential.sensitivity(),
        SensitivityClass::SecretForbidden
    );
    let static_event = DiagnosticEvent::new_at(
        Severity::Error,
        Component::Gui,
        EventCode::GuiStartupFailed,
        1,
    );
    let report = SupportReport::from_events_at(
        vec![static_event],
        2,
        endfield_unix_diagnostics::LogHistoryStatus::Missing,
        0,
    );
    assert!(report.to_json().is_ok());
}

#[test]
fn overlong_untrusted_domain_text_is_rejected_at_the_domain_boundary() {
    let message = "secret".repeat(2000);
    assert!(Diagnostic::new(DiagnosticCategory::TemporaryFailure, None, Some(message),).is_err());
}
