//! Structured, local-first diagnostics for `endfield-unix`.
//!
//! Persistent events use typed, allowlisted fields. Arbitrary `tracing` fields
//! are ignored, domain free-form details are omitted, and support reports have
//! no fields for paths, credentials, environment values, command lines, or
//! correlation identifiers.

#![forbid(unsafe_code)]

mod domain_adapter;
mod errors;
mod event;
mod logging;
mod paths;
mod redact;
mod report;
mod retention;

pub use domain_adapter::{
    event_for_probe_success, event_for_unimplemented_operation, event_from_diagnostic,
    event_from_domain_diagnostic, event_from_probe_error,
};
pub use errors::{DiagnosticsError, IoFailureKind};
pub use event::{
    Component, CorrelationId, DiagnosticEvent, EVENT_SCHEMA_VERSION, EventCode,
    MAX_CORRELATION_ID_BYTES, OperationOutcome, SafeDiagnosticCategory, SafeProvider, Severity,
};
pub use logging::{
    LifecycleEvent, LogHandle, LogWorker, RecordDisposition, flush_global_logging,
    global_dropped_event_count, global_logging_initialized, initialize_global_logging,
    record_lifecycle,
};
pub use paths::{DiagnosticsPaths, LinuxDiagnosticsEnvironment};
pub use redact::{RedactionPolicy, ReportDiagnostic, ReportField, SensitivityClass};
pub use report::{
    ApplicationMetadata, BuildStatus, ComponentAvailability, ComponentId, ComponentStatus,
    EventCounts, LogHistoryStatus, LogHistorySummary, MAX_REPORT_BYTES, MAX_REPORT_EVENTS,
    REPORT_SCHEMA_VERSION, SupportReport, export_support_report, generate_support_report,
    write_report_to_path,
};
pub use retention::{
    LogReadResult, LogStore, MAX_EVENT_BYTES, MAX_LOG_AGE, MAX_RETAINED_SEGMENTS,
    MAX_SEGMENT_BYTES, RetentionPolicy,
};
