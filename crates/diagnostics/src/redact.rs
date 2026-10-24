use serde::Serialize;

use crate::{
    Component, DiagnosticEvent, EventCode, OperationOutcome, SafeDiagnosticCategory, SafeProvider,
    Severity,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensitivityClass {
    SafeStaticMetadata,
    SafeStructuredDiagnostic,
    PotentiallyIdentifying,
    SecretForbidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportField {
    ApplicationName,
    PackageVersion,
    BuildStatus,
    EventTimestamp,
    Severity,
    Component,
    EventCode,
    SafeMessage,
    OperationOutcome,
    DiagnosticCategory,
    CapabilityIdentifier,
    ProviderIdentifier,
    CorrelationIdentifier,
    FreeFormText,
    HomePath,
    EnvironmentValue,
    CommandLine,
    Credential,
    DeviceIdentifier,
}

impl ReportField {
    pub const fn sensitivity(self) -> SensitivityClass {
        match self {
            Self::ApplicationName | Self::PackageVersion | Self::BuildStatus => {
                SensitivityClass::SafeStaticMetadata
            }
            Self::EventTimestamp
            | Self::Severity
            | Self::Component
            | Self::EventCode
            | Self::SafeMessage
            | Self::OperationOutcome
            | Self::DiagnosticCategory
            | Self::CapabilityIdentifier
            | Self::ProviderIdentifier => SensitivityClass::SafeStructuredDiagnostic,
            Self::CorrelationIdentifier | Self::DeviceIdentifier => {
                SensitivityClass::PotentiallyIdentifying
            }
            Self::FreeFormText
            | Self::HomePath
            | Self::EnvironmentValue
            | Self::CommandLine
            | Self::Credential => SensitivityClass::SecretForbidden,
        }
    }
}

/// Allowlist-first serializer for support-report diagnostics. The output type
/// has no field for correlation IDs, free-form text, paths, or credentials.
#[derive(Clone, Copy, Debug, Default)]
pub struct RedactionPolicy;

impl RedactionPolicy {
    pub fn redact_event(self, event: &DiagnosticEvent) -> ReportDiagnostic {
        ReportDiagnostic {
            timestamp_unix_nanos: event.timestamp_unix_nanos(),
            severity: event.severity(),
            component: event.component(),
            code: event.code(),
            message: event.message(),
            outcome: event.outcome(),
            category: event.category(),
            capability_id: event.capability_id(),
            provider: event.provider(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReportDiagnostic {
    timestamp_unix_nanos: u64,
    severity: Severity,
    component: Component,
    code: EventCode,
    message: &'static str,
    outcome: Option<OperationOutcome>,
    category: Option<SafeDiagnosticCategory>,
    capability_id: Option<u32>,
    provider: Option<SafeProvider>,
}

impl ReportDiagnostic {
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    pub const fn timestamp_unix_nanos(&self) -> u64 {
        self.timestamp_unix_nanos
    }
}
