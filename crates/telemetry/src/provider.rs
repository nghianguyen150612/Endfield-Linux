use endfield_unix_domain::DiagnosticCategory;

use crate::batch::TelemetryBatch;
use crate::errors::TelemetryError;
use crate::metric::MetricId;
use crate::provenance::{ProviderId, SamplingContextId};
use crate::timestamp::MonotonicTimestamp;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionRequest {
    context: SamplingContextId,
    metrics: Vec<MetricId>,
    requested_at: Option<MonotonicTimestamp>,
}

impl CollectionRequest {
    pub fn new(
        context: SamplingContextId,
        mut metrics: Vec<MetricId>,
        requested_at: Option<MonotonicTimestamp>,
    ) -> Result<Self, TelemetryError> {
        metrics.sort_unstable();
        metrics.dedup();
        if metrics.is_empty() {
            return Err(TelemetryError::InvalidValue {
                field: "collection request",
                reason: "at least one metric must be requested",
            });
        }
        Ok(Self {
            context,
            metrics,
            requested_at,
        })
    }

    pub const fn context(&self) -> SamplingContextId {
        self.context
    }

    pub fn metrics(&self) -> &[MetricId] {
        &self.metrics
    }

    pub const fn requested_at(&self) -> Option<MonotonicTimestamp> {
        self.requested_at
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderIssueKind {
    NotYetSampled,
    MissingProvider,
    Unsupported,
    PermissionDenied,
    TemporaryFailure,
    MalformedSourceData,
    Degraded,
}

impl ProviderIssueKind {
    pub const fn diagnostic_category(self) -> DiagnosticCategory {
        match self {
            Self::NotYetSampled => DiagnosticCategory::NotYetProbed,
            Self::MissingProvider => DiagnosticCategory::MissingProvider,
            Self::Unsupported => DiagnosticCategory::UnsupportedCapability,
            Self::PermissionDenied => DiagnosticCategory::PermissionDenied,
            Self::TemporaryFailure => DiagnosticCategory::TemporaryFailure,
            Self::MalformedSourceData => DiagnosticCategory::MalformedSourceData,
            Self::Degraded => DiagnosticCategory::StaleObservation,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderIssue {
    metric: MetricId,
    kind: ProviderIssueKind,
}

impl ProviderIssue {
    pub const fn new(metric: MetricId, kind: ProviderIssueKind) -> Self {
        Self { metric, kind }
    }

    pub const fn metric(self) -> MetricId {
        self.metric
    }

    pub const fn kind(self) -> ProviderIssueKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionCompleteness {
    Complete,
    Partial,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderCollection {
    batch: TelemetryBatch,
    completeness: CollectionCompleteness,
    issues: Vec<ProviderIssue>,
}

impl ProviderCollection {
    pub fn complete(batch: TelemetryBatch) -> Result<Self, TelemetryError> {
        if batch
            .observations()
            .iter()
            .any(|observation| !observation.state().is_measured())
        {
            return Err(TelemetryError::InvalidObservationState);
        }
        Ok(Self {
            batch,
            completeness: CollectionCompleteness::Complete,
            issues: Vec::new(),
        })
    }

    pub fn partial(
        batch: TelemetryBatch,
        issues: Vec<ProviderIssue>,
    ) -> Result<Self, TelemetryError> {
        if issues.is_empty() {
            return Err(TelemetryError::InvalidObservationState);
        }
        Ok(Self {
            batch,
            completeness: CollectionCompleteness::Partial,
            issues,
        })
    }

    pub const fn batch(&self) -> &TelemetryBatch {
        &self.batch
    }

    pub const fn completeness(&self) -> CollectionCompleteness {
        self.completeness
    }

    pub fn issues(&self) -> &[ProviderIssue] {
        &self.issues
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderErrorKind {
    Unsupported,
    PermissionDenied,
    TemporaryFailure,
    MalformedSourceData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderError {
    provider: ProviderId,
    kind: ProviderErrorKind,
}

impl ProviderError {
    pub const fn new(provider: ProviderId, kind: ProviderErrorKind) -> Self {
        Self { provider, kind }
    }

    pub const fn provider(&self) -> &ProviderId {
        &self.provider
    }

    pub const fn kind(&self) -> ProviderErrorKind {
        self.kind
    }
}

/// Future providers receive explicit caller context and time. They declare
/// supported metrics and return typed partial data or a typed fatal failure.
pub trait TelemetryProvider {
    fn provider_id(&self) -> &ProviderId;
    fn declared_metrics(&self) -> &[MetricId];
    fn collect(&self, request: &CollectionRequest) -> Result<ProviderCollection, ProviderError>;
}
