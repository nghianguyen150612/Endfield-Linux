use std::error::Error;
use std::fmt;

use crate::capability::CapabilityRecord;
use crate::display::DisplayInfo;
use crate::hardware::{CpuInfo, GpuInfo, MemoryInfo};
use crate::platform::PlatformIdentity;
use crate::provenance::{Diagnostic, DiagnosticCategory, Provenance};
use crate::snapshot::SystemSnapshot;
use crate::validation::{ValidationErrors, validate_provider_name, validate_text};

/// A total failure to obtain a platform report. Partial reports are successful
/// snapshots and carry subsystem diagnostics instead.
pub trait PlatformProbe {
    fn probe(&self) -> Result<SystemSnapshot, ProbeError>;
}

/// A synchronous boundary for one independent capability source.
pub trait CapabilityProvider {
    fn name(&self) -> &str;
    fn collect(&self) -> Result<ProbeReport, ProbeError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeErrorKind {
    Unsupported,
    MissingProvider,
    PermissionDenied,
    TemporaryFailure,
    MalformedData,
    StaleObservation,
    NotYetProbed,
    NoProviders,
    AllProvidersFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeError {
    provider: String,
    kind: ProbeErrorKind,
    detail: Option<String>,
}

impl ProbeError {
    pub fn new(
        provider: impl Into<String>,
        kind: ProbeErrorKind,
        detail: Option<String>,
    ) -> Result<Self, crate::validation::ValidationError> {
        let provider = provider.into();
        validate_provider_name(&provider)?;
        if let Some(detail) = &detail {
            validate_text(detail, "probe error detail", 1024)?;
        }
        Ok(Self {
            provider,
            kind,
            detail,
        })
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub const fn kind(&self) -> ProbeErrorKind {
        self.kind
    }

    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} probe failed ({:?})", self.provider, self.kind)?;
        if let Some(detail) = &self.detail {
            write!(f, ": {detail}")?;
        }
        Ok(())
    }
}

impl Error for ProbeError {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProbeReport {
    pub cpu: Option<CpuInfo>,
    pub gpus: Vec<GpuInfo>,
    pub memory: Option<MemoryInfo>,
    pub displays: Vec<DisplayInfo>,
    pub capabilities: Vec<CapabilityRecord>,
    pub provenance: Vec<Provenance>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Runs independent providers in sequence and keeps successful subsystem
/// results when another provider fails. It performs no operating-system I/O.
pub struct CompositePlatformProbe {
    platform: PlatformIdentity,
    providers: Vec<Box<dyn CapabilityProvider>>,
}

impl CompositePlatformProbe {
    pub fn new(platform: PlatformIdentity) -> Self {
        Self {
            platform,
            providers: Vec::new(),
        }
    }

    pub fn add_provider<P>(&mut self, provider: P)
    where
        P: CapabilityProvider + 'static,
    {
        self.providers.push(Box::new(provider));
    }
}

impl PlatformProbe for CompositePlatformProbe {
    fn probe(&self) -> Result<SystemSnapshot, ProbeError> {
        if self.providers.is_empty() {
            return Err(ProbeError {
                provider: "composite".to_owned(),
                kind: ProbeErrorKind::NoProviders,
                detail: Some("no capability providers were configured".to_owned()),
            });
        }

        let mut snapshot = SystemSnapshot::new(self.platform.clone());
        let mut valid_reports = 0usize;
        for provider in &self.providers {
            match provider.collect() {
                Ok(report) => {
                    if let Some(reason) = report_conflict(&snapshot, &report) {
                        snapshot.diagnostics.push(Diagnostic {
                            category: DiagnosticCategory::MalformedSourceData,
                            provider: Some(safe_provider_name(provider.name())),
                            message: Some(reason.to_owned()),
                        });
                        continue;
                    }
                    let mut candidate = snapshot.clone();
                    candidate.append_report(report);
                    match candidate.validate() {
                        Ok(()) => {
                            snapshot = candidate;
                            valid_reports += 1;
                        }
                        Err(errors) => {
                            snapshot.diagnostics.push(Diagnostic {
                                category: DiagnosticCategory::MalformedSourceData,
                                provider: Some(safe_provider_name(provider.name())),
                                message: Some(format!(
                                    "provider returned an invalid report: {}",
                                    concise_validation_message(&errors)
                                )),
                            });
                        }
                    }
                }
                Err(error) => snapshot.diagnostics.push(Diagnostic {
                    category: diagnostic_category(error.kind),
                    provider: Some(error.provider),
                    message: error.detail,
                }),
            }
        }

        if valid_reports == 0 {
            return Err(ProbeError {
                provider: "composite".to_owned(),
                kind: ProbeErrorKind::AllProvidersFailed,
                detail: Some("no provider produced a valid partial report".to_owned()),
            });
        }

        snapshot.normalize().map_err(|errors| ProbeError {
            provider: "composite".to_owned(),
            kind: ProbeErrorKind::MalformedData,
            detail: Some(concise_validation_message(&errors)),
        })
    }
}

fn report_conflict(snapshot: &SystemSnapshot, report: &ProbeReport) -> Option<&'static str> {
    if snapshot.cpu.is_some() && report.cpu.is_some() {
        return Some("multiple providers supplied CPU inventory");
    }
    if snapshot.memory.is_some() && report.memory.is_some() {
        return Some("multiple providers supplied memory inventory");
    }
    None
}

fn diagnostic_category(kind: ProbeErrorKind) -> DiagnosticCategory {
    match kind {
        ProbeErrorKind::Unsupported => DiagnosticCategory::UnsupportedCapability,
        ProbeErrorKind::MissingProvider => DiagnosticCategory::MissingProvider,
        ProbeErrorKind::PermissionDenied => DiagnosticCategory::PermissionDenied,
        ProbeErrorKind::TemporaryFailure | ProbeErrorKind::AllProvidersFailed => {
            DiagnosticCategory::TemporaryFailure
        }
        ProbeErrorKind::MalformedData => DiagnosticCategory::MalformedSourceData,
        ProbeErrorKind::StaleObservation => DiagnosticCategory::StaleObservation,
        ProbeErrorKind::NotYetProbed | ProbeErrorKind::NoProviders => {
            DiagnosticCategory::NotYetProbed
        }
    }
}

fn concise_validation_message(errors: &ValidationErrors) -> String {
    errors
        .0
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "validation failed".to_owned())
}

fn safe_provider_name(name: &str) -> String {
    if validate_provider_name(name).is_ok() {
        name.to_owned()
    } else {
        "invalid-provider".to_owned()
    }
}
