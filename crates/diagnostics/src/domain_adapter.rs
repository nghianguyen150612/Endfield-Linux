use endfield_unix_domain::{Diagnostic, ProbeError, ProbeErrorKind};

use crate::event::{map_domain_category, outcome_for_category};
use crate::{
    Component, DiagnosticEvent, DiagnosticsError, EventCode, OperationOutcome,
    SafeDiagnosticCategory, SafeProvider, Severity,
};

/// Converts a domain diagnostic into a typed event. Its free-form message is
/// intentionally discarded; only stable category, capability, and allowlisted
/// provider identifiers cross this boundary.
pub fn event_from_domain_diagnostic(
    diagnostic: &Diagnostic,
    capability_id: Option<u32>,
    timestamp_unix_nanos: u64,
) -> Result<DiagnosticEvent, DiagnosticsError> {
    event_from_diagnostic(
        diagnostic,
        Component::PlatformProbe,
        capability_id,
        timestamp_unix_nanos,
    )
}

pub fn event_from_diagnostic(
    diagnostic: &Diagnostic,
    component: Component,
    capability_id: Option<u32>,
    timestamp_unix_nanos: u64,
) -> Result<DiagnosticEvent, DiagnosticsError> {
    diagnostic
        .validate()
        .map_err(|_| DiagnosticsError::InvalidEvent("invalid domain diagnostic"))?;
    let (category, outcome) = map_domain_category(diagnostic.category);
    let severity = severity_for_category(category);
    let provider = diagnostic
        .provider
        .as_deref()
        .and_then(SafeProvider::from_domain_name);
    let mut event = DiagnosticEvent::new_at(
        severity,
        component,
        EventCode::DomainDiagnostic,
        timestamp_unix_nanos,
    );
    event.set_domain_fields(outcome, category, capability_id, provider)?;
    Ok(event)
}

/// Converts a typed probe failure without exporting its arbitrary detail text.
pub fn event_from_probe_error(
    error: &ProbeError,
    timestamp_unix_nanos: u64,
) -> Result<DiagnosticEvent, DiagnosticsError> {
    let category = match error.kind() {
        ProbeErrorKind::Unsupported => SafeDiagnosticCategory::UnsupportedCapability,
        ProbeErrorKind::MissingProvider => SafeDiagnosticCategory::MissingProvider,
        ProbeErrorKind::PermissionDenied => SafeDiagnosticCategory::PermissionDenied,
        ProbeErrorKind::TemporaryFailure
        | ProbeErrorKind::NoProviders
        | ProbeErrorKind::AllProvidersFailed => SafeDiagnosticCategory::TemporaryFailure,
        ProbeErrorKind::MalformedData => SafeDiagnosticCategory::MalformedSourceData,
        ProbeErrorKind::StaleObservation => SafeDiagnosticCategory::StaleObservation,
        ProbeErrorKind::NotYetProbed => SafeDiagnosticCategory::NotYetProbed,
    };
    let outcome = outcome_for_category(category);
    let severity = severity_for_category(category);
    let provider = SafeProvider::from_domain_name(error.provider());
    let mut event = DiagnosticEvent::new_at(
        severity,
        Component::PlatformProbe,
        EventCode::ProbeFailure,
        timestamp_unix_nanos,
    );
    // The probe error has no single capability identifier.
    event.set_domain_fields(outcome, category, None, provider)?;
    Ok(event)
}

pub fn event_for_probe_success(timestamp_unix_nanos: u64) -> DiagnosticEvent {
    let mut event = DiagnosticEvent::new_at(
        Severity::Info,
        Component::PlatformProbe,
        EventCode::ProbeSucceeded,
        timestamp_unix_nanos,
    );
    event.set_outcome(OperationOutcome::Succeeded);
    event
}

pub fn event_for_unimplemented_operation(
    component: Component,
    timestamp_unix_nanos: u64,
) -> DiagnosticEvent {
    let mut event = DiagnosticEvent::new_at(
        Severity::Info,
        component,
        EventCode::OperationNotImplemented,
        timestamp_unix_nanos,
    );
    event.set_outcome(OperationOutcome::NotImplemented);
    event
}

fn severity_for_category(category: SafeDiagnosticCategory) -> Severity {
    match category {
        SafeDiagnosticCategory::UnsupportedCapability
        | SafeDiagnosticCategory::MissingProvider
        | SafeDiagnosticCategory::NotYetProbed => Severity::Info,
        SafeDiagnosticCategory::StaleObservation | SafeDiagnosticCategory::TemporaryFailure => {
            Severity::Warn
        }
        SafeDiagnosticCategory::PermissionDenied | SafeDiagnosticCategory::MalformedSourceData => {
            Severity::Error
        }
    }
}
