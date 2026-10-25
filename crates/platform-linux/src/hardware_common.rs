use endfield_unix_domain::{Diagnostic, DiagnosticCategory, ProbeError, ProbeErrorKind};

use crate::errors::DataSourceErrorKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HardwareIssue {
    pub provider: &'static str,
    pub category: DiagnosticCategory,
    pub message: &'static str,
}

impl HardwareIssue {
    pub fn from_source(
        provider: &'static str,
        kind: DataSourceErrorKind,
        subject: &'static str,
    ) -> Self {
        let category = match kind {
            DataSourceErrorKind::NotFound => DiagnosticCategory::MissingProvider,
            DataSourceErrorKind::PermissionDenied => DiagnosticCategory::PermissionDenied,
            DataSourceErrorKind::Unsupported => DiagnosticCategory::UnsupportedCapability,
            DataSourceErrorKind::InvalidUtf8
            | DataSourceErrorKind::MalformedData
            | DataSourceErrorKind::TooLarge
            | DataSourceErrorKind::UnexpectedType
            | DataSourceErrorKind::UnsafeSource => DiagnosticCategory::MalformedSourceData,
            DataSourceErrorKind::Other => DiagnosticCategory::TemporaryFailure,
        };
        Self {
            provider,
            category,
            message: subject,
        }
    }

    pub fn malformed(provider: &'static str, message: &'static str) -> Self {
        Self {
            provider,
            category: DiagnosticCategory::MalformedSourceData,
            message,
        }
    }

    pub fn not_yet_probed(provider: &'static str, message: &'static str) -> Self {
        Self {
            provider,
            category: DiagnosticCategory::NotYetProbed,
            message,
        }
    }

    pub fn to_domain(self) -> Result<Diagnostic, ProbeError> {
        Diagnostic::new(
            self.category,
            Some(self.provider.to_owned()),
            Some(self.message.to_owned()),
        )
        .map_err(|_| {
            ProbeError::new(
                self.provider,
                ProbeErrorKind::MalformedData,
                Some("hardware diagnostic failed domain validation".to_owned()),
            )
            .expect("fixed Linux hardware provider is valid")
        })
    }
}
