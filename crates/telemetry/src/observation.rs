use endfield_unix_domain::{DiagnosticCategory, MeasurementValue};

use crate::errors::TelemetryError;
use crate::metric::{MetricId, MetricKind, MetricValueKind};
use crate::provenance::{SafeDiagnostic, TelemetryProvenance};
use crate::quality::{Completeness, QualityMetadata};
use crate::scope::{ObservationTarget, SnapshotContext};
use crate::timestamp::{
    ClockDomainId, MonotonicObservation, MonotonicTimestamp, ObservationWindow, WallClockTimestamp,
};
use crate::units::{TelemetryMeasurement, TelemetryUnit};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObservationTime {
    monotonic: Option<MonotonicObservation>,
    wall_clock: Option<WallClockTimestamp>,
}

impl ObservationTime {
    pub fn new(
        monotonic: Option<MonotonicObservation>,
        wall_clock: Option<WallClockTimestamp>,
    ) -> Result<Self, TelemetryError> {
        if monotonic.is_none() && wall_clock.is_none() {
            return Err(TelemetryError::MissingTimestamp);
        }
        Ok(Self {
            monotonic,
            wall_clock,
        })
    }

    pub fn monotonic(timestamp: MonotonicTimestamp) -> Self {
        Self {
            monotonic: Some(MonotonicObservation::Instant(timestamp)),
            wall_clock: None,
        }
    }

    pub fn window(window: ObservationWindow) -> Self {
        Self {
            monotonic: Some(MonotonicObservation::Window(window)),
            wall_clock: None,
        }
    }

    pub fn from_wall_clock(timestamp: WallClockTimestamp) -> Self {
        Self {
            monotonic: None,
            wall_clock: Some(timestamp),
        }
    }

    pub fn with_wall_clock(mut self, timestamp: WallClockTimestamp) -> Self {
        self.wall_clock = Some(timestamp);
        self
    }

    pub const fn monotonic_observation(self) -> Option<MonotonicObservation> {
        self.monotonic
    }

    pub const fn wall_clock(self) -> Option<WallClockTimestamp> {
        self.wall_clock
    }

    pub const fn clock_domain(self) -> Option<ClockDomainId> {
        match self.monotonic {
            Some(observation) => Some(observation.domain()),
            None => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemporalMetadata {
    observation: ObservationTime,
    collection_completed_at: Option<MonotonicTimestamp>,
    ingested_at: Option<MonotonicTimestamp>,
}

impl TemporalMetadata {
    pub fn new(
        observation: ObservationTime,
        collection_completed_at: Option<MonotonicTimestamp>,
        ingested_at: Option<MonotonicTimestamp>,
    ) -> Result<Self, TelemetryError> {
        let metadata = Self {
            observation,
            collection_completed_at,
            ingested_at,
        };
        metadata.validate()?;
        Ok(metadata)
    }

    pub const fn observation(self) -> ObservationTime {
        self.observation
    }

    pub const fn collection_completed_at(self) -> Option<MonotonicTimestamp> {
        self.collection_completed_at
    }

    pub const fn ingested_at(self) -> Option<MonotonicTimestamp> {
        self.ingested_at
    }

    pub fn clock_domain(self) -> Option<ClockDomainId> {
        self.observation
            .clock_domain()
            .or_else(|| self.collection_completed_at.map(MonotonicTimestamp::domain))
            .or_else(|| self.ingested_at.map(MonotonicTimestamp::domain))
    }

    pub fn validate(self) -> Result<(), TelemetryError> {
        let domain = self.clock_domain();
        let timestamps = [self.collection_completed_at, self.ingested_at];
        if timestamps
            .into_iter()
            .flatten()
            .any(|timestamp| domain.is_some_and(|expected| timestamp.domain() != expected))
        {
            return Err(TelemetryError::ClockDomainMismatch);
        }

        let observed = self
            .observation
            .monotonic_observation()
            .map(MonotonicObservation::freshness_anchor);
        if let (Some(observed), Some(collected)) = (observed, self.collection_completed_at) {
            collected.duration_since(observed)?;
        }
        if let Some(ingested) = self.ingested_at {
            let prior = self.collection_completed_at.or(observed);
            if let Some(prior) = prior {
                ingested.duration_since(prior)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationState {
    Measured(TelemetryMeasurement),
    NotYetSampled,
    Unknown,
    ProviderMissing,
    Unsupported,
    PermissionDenied,
    TemporaryFailure,
    MalformedSourceData,
    Degraded(Option<TelemetryMeasurement>),
}

impl ObservationState {
    pub const fn value(self) -> Option<TelemetryMeasurement> {
        match self {
            Self::Measured(value) => Some(value),
            Self::Degraded(value) => value,
            Self::NotYetSampled
            | Self::Unknown
            | Self::ProviderMissing
            | Self::Unsupported
            | Self::PermissionDenied
            | Self::TemporaryFailure
            | Self::MalformedSourceData => None,
        }
    }

    pub const fn is_measured(self) -> bool {
        matches!(self, Self::Measured(_))
    }

    pub const fn diagnostic_category(self) -> Option<DiagnosticCategory> {
        match self {
            Self::NotYetSampled => Some(DiagnosticCategory::NotYetProbed),
            Self::ProviderMissing => Some(DiagnosticCategory::MissingProvider),
            Self::Unsupported => Some(DiagnosticCategory::UnsupportedCapability),
            Self::PermissionDenied => Some(DiagnosticCategory::PermissionDenied),
            Self::TemporaryFailure => Some(DiagnosticCategory::TemporaryFailure),
            Self::MalformedSourceData => Some(DiagnosticCategory::MalformedSourceData),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelemetryObservation {
    metric: MetricId,
    target: ObservationTarget,
    state: ObservationState,
    temporal: TemporalMetadata,
    provenance: TelemetryProvenance,
    quality: QualityMetadata,
    diagnostic: Option<SafeDiagnostic>,
}

impl TelemetryObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        metric: MetricId,
        target: ObservationTarget,
        state: ObservationState,
        temporal: TemporalMetadata,
        provenance: TelemetryProvenance,
        quality: QualityMetadata,
        diagnostic: Option<SafeDiagnostic>,
    ) -> Result<Self, TelemetryError> {
        let observation = Self {
            metric,
            target,
            state,
            temporal,
            provenance,
            quality,
            diagnostic,
        };
        observation.validate(None)?;
        Ok(observation)
    }

    pub fn measured(
        metric: MetricId,
        target: ObservationTarget,
        value: TelemetryMeasurement,
        temporal: TemporalMetadata,
        provenance: TelemetryProvenance,
        quality: QualityMetadata,
    ) -> Result<Self, TelemetryError> {
        Self::new(
            metric,
            target,
            ObservationState::Measured(value),
            temporal,
            provenance,
            quality,
            None,
        )
    }

    pub fn unavailable(
        metric: MetricId,
        target: ObservationTarget,
        state: ObservationState,
        temporal: TemporalMetadata,
        provenance: TelemetryProvenance,
        diagnostic: Option<SafeDiagnostic>,
    ) -> Result<Self, TelemetryError> {
        if matches!(
            state,
            ObservationState::Measured(_) | ObservationState::Degraded(_)
        ) {
            return Err(TelemetryError::InvalidObservationState);
        }
        Self::new(
            metric,
            target,
            state,
            temporal,
            provenance,
            QualityMetadata::partial(),
            diagnostic,
        )
    }

    pub fn degraded(
        metric: MetricId,
        target: ObservationTarget,
        value: Option<TelemetryMeasurement>,
        temporal: TemporalMetadata,
        provenance: TelemetryProvenance,
        diagnostic: Option<SafeDiagnostic>,
        quality: QualityMetadata,
    ) -> Result<Self, TelemetryError> {
        Self::new(
            metric,
            target,
            ObservationState::Degraded(value),
            temporal,
            provenance,
            quality,
            diagnostic,
        )
    }

    pub const fn metric(&self) -> MetricId {
        self.metric
    }

    pub const fn target(&self) -> ObservationTarget {
        self.target
    }

    pub const fn state(&self) -> ObservationState {
        self.state
    }

    pub const fn value(&self) -> Option<TelemetryMeasurement> {
        self.state.value()
    }

    pub const fn temporal(&self) -> TemporalMetadata {
        self.temporal
    }

    pub const fn provenance(&self) -> &TelemetryProvenance {
        &self.provenance
    }

    pub const fn quality(&self) -> &QualityMetadata {
        &self.quality
    }

    pub const fn diagnostic(&self) -> Option<SafeDiagnostic> {
        self.diagnostic
    }

    pub fn validate(&self, snapshot: Option<&SnapshotContext>) -> Result<(), TelemetryError> {
        let definition = self.metric.definition();
        if self.target.scope() != definition.scope {
            return Err(TelemetryError::InvalidTargetForMetric);
        }
        if let Some(snapshot) = snapshot {
            snapshot.validate_target(self.metric, self.target)?;
        } else if matches!(
            self.target,
            ObservationTarget::Cpu(_)
                | ObservationTarget::LogicalCpu { .. }
                | ObservationTarget::Gpu(_)
        ) {
            // Reference resolution is completed when the enclosing batch
            // validates against its explicit snapshot context.
        }

        self.temporal.validate()?;
        self.quality.validate()?;
        self.validate_state_semantics()?;
        let Some(value) = self.state.value() else {
            return Ok(());
        };

        if value.unit() != definition.unit {
            return Err(TelemetryError::MetricUnitMismatch);
        }
        if value_kind(value.value()) != definition.value_kind {
            return Err(TelemetryError::MetricValueKindMismatch);
        }
        if let Some(range) = definition.permitted_range {
            if !range.accepts(value.value()) {
                return Err(TelemetryError::ValueOutOfRange);
            }
        }
        if definition.unit == TelemetryUnit::BasisPoints
            && definition.permitted_range.is_some()
            && matches!(value.value(), MeasurementValue::Unsigned(value) if value > 10_000)
        {
            return Err(TelemetryError::ValueOutOfRange);
        }
        if definition.unit == TelemetryUnit::MilliCelsius
            && !matches!(value.value(), MeasurementValue::Signed(_))
        {
            return Err(TelemetryError::MetricValueKindMismatch);
        }
        if definition.kind == MetricKind::Rate {
            let valid_window = self
                .temporal
                .observation()
                .monotonic_observation()
                .is_some_and(|observation| {
                    matches!(observation, MonotonicObservation::Window(window) if !window.is_zero_duration())
                });
            if !valid_window {
                return Err(TelemetryError::ZeroDuration);
            }
        }
        if definition.kind == MetricKind::DerivedStatistic {
            let sampling = self.quality.sampling();
            if sampling.is_none_or(|sampling| {
                sampling.observation_count().is_none_or(|count| count < 2)
                    || sampling.derivation_method().is_none()
            }) {
                return Err(TelemetryError::InvalidSamplingMetadata);
            }
            if self.provenance.method() != crate::provenance::SamplingMethod::Derived {
                return Err(TelemetryError::InvalidObservationState);
            }
        }
        Ok(())
    }

    fn validate_state_semantics(&self) -> Result<(), TelemetryError> {
        if self.state.is_measured() && self.quality.completeness() != Completeness::Complete {
            return Err(TelemetryError::InvalidObservationState);
        }
        if self.state.is_measured() && self.diagnostic.is_some() {
            return Err(TelemetryError::InvalidObservationState);
        }
        if !self.state.is_measured() && self.quality.completeness() != Completeness::Partial {
            return Err(TelemetryError::InvalidObservationState);
        }
        if matches!(self.state, ObservationState::Degraded(_))
            && self.quality.completeness() != Completeness::Partial
        {
            return Err(TelemetryError::InvalidObservationState);
        }
        if let Some(diagnostic) = self.diagnostic {
            let category = diagnostic.category();
            let compatible = match self.state {
                ObservationState::Measured(_) => false,
                ObservationState::NotYetSampled | ObservationState::Unknown => {
                    category == DiagnosticCategory::NotYetProbed
                }
                ObservationState::ProviderMissing => {
                    category == DiagnosticCategory::MissingProvider
                }
                ObservationState::Unsupported => {
                    category == DiagnosticCategory::UnsupportedCapability
                }
                ObservationState::PermissionDenied => {
                    category == DiagnosticCategory::PermissionDenied
                }
                ObservationState::TemporaryFailure => {
                    category == DiagnosticCategory::TemporaryFailure
                }
                ObservationState::MalformedSourceData => {
                    category == DiagnosticCategory::MalformedSourceData
                }
                ObservationState::Degraded(_) => matches!(
                    category,
                    DiagnosticCategory::PermissionDenied
                        | DiagnosticCategory::TemporaryFailure
                        | DiagnosticCategory::MalformedSourceData
                        | DiagnosticCategory::StaleObservation
                ),
            };
            if !compatible {
                return Err(TelemetryError::InvalidObservationState);
            }
        }
        Ok(())
    }

    pub(crate) fn observation_key(&self) -> ObservationIdentityTime {
        match self.temporal.observation().monotonic_observation() {
            Some(MonotonicObservation::Instant(timestamp)) => {
                ObservationIdentityTime::Instant(timestamp.domain(), timestamp.elapsed_nanos())
            }
            Some(MonotonicObservation::Window(window)) => ObservationIdentityTime::Window(
                window.start().domain(),
                window.start().elapsed_nanos(),
                window.end().elapsed_nanos(),
            ),
            None => ObservationIdentityTime::WallClock(
                self.temporal
                    .observation()
                    .wall_clock()
                    .expect("observation timestamps are validated")
                    .unix_nanos(),
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ObservationIdentityTime {
    Instant(ClockDomainId, u64),
    Window(ClockDomainId, u64, u64),
    WallClock(u64),
}

fn value_kind(value: MeasurementValue) -> MetricValueKind {
    match value {
        MeasurementValue::Unsigned(_) => MetricValueKind::Unsigned,
        MeasurementValue::Signed(_) => MetricValueKind::Signed,
    }
}
