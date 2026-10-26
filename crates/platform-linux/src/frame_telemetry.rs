//! P015 provider for one explicitly imported frame recording.

use std::collections::BTreeSet;

use endfield_unix_domain::{MeasurementOrigin, MeasurementValue};
use endfield_unix_telemetry::{
    ClockDomainId, CollectionRequest, DerivationMethod, DurationNanos, MetricId, ObservationTarget,
    ObservationTime, ObservationWindow, ProviderCollection, ProviderError, ProviderErrorKind,
    ProviderId, ProviderIssue, ProviderIssueKind, QualityFlag, QualityMetadata, SafeDiagnostic,
    SamplingContextId, SamplingMetadata, SamplingMethod, TelemetryBatch, TelemetryError,
    TelemetryMeasurement, TelemetryObservation, TelemetryProvenance, TelemetryProvider,
    TelemetryUnit, TemporalMetadata,
};

use crate::frame_csv::MAX_FRAME_INTERVALS;
use crate::frame_events::FrameEvent;
use crate::frame_mangohud::MangoHudFrameSample;
use crate::frame_sources::{
    FrameMeasurementSemantics, FrameRecording, FrameSourceFormat, FrameSourceMetadata,
};
use crate::frame_statistics::{
    arithmetic_mean_nanos, measured_frame_rate_milli, nearest_rank_p99_nanos,
};

const FRAME_PROVIDER_NAME: &str = "frame-log-import";
const FRAME_METRICS: [MetricId; 5] = [
    MetricId::FramesPerSecond,
    MetricId::FrameTime,
    MetricId::MeanFrameTime,
    MetricId::FrameCount,
    MetricId::P99FrameTime,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameMetricStatus {
    Measured,
    InsufficientSamples,
    NoVerifiedObservationWindow,
    DiscontinuousTimeline,
    UnsupportedBySource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameTelemetryResult {
    source_metadata: FrameSourceMetadata,
    valid_interval_count: u32,
    recording_duration_ns: Option<u64>,
    fps_status: FrameMetricStatus,
    mean_status: FrameMetricStatus,
    p99_status: FrameMetricStatus,
    frame_count_status: FrameMetricStatus,
    collection: ProviderCollection,
}

impl FrameTelemetryResult {
    pub const fn source_metadata(&self) -> FrameSourceMetadata {
        self.source_metadata
    }

    pub const fn valid_interval_count(&self) -> u32 {
        self.valid_interval_count
    }

    pub const fn recording_duration_ns(&self) -> Option<u64> {
        self.recording_duration_ns
    }

    pub const fn fps_status(&self) -> FrameMetricStatus {
        self.fps_status
    }

    pub const fn mean_status(&self) -> FrameMetricStatus {
        self.mean_status
    }

    pub const fn p99_status(&self) -> FrameMetricStatus {
        self.p99_status
    }

    pub const fn frame_count_status(&self) -> FrameMetricStatus {
        self.frame_count_status
    }

    pub const fn collection(&self) -> &ProviderCollection {
        &self.collection
    }
}

#[derive(Clone, Copy, Debug)]
struct FrameInterval {
    timestamp_ns: u64,
    duration_ns: u64,
}

struct DerivedMetricRequest<'a> {
    requested: &'a BTreeSet<MetricId>,
    observations: &'a mut Vec<TelemetryObservation>,
    issues: &'a mut Vec<ProviderIssue>,
    metric: MetricId,
    context: SamplingContextId,
    value: Option<u64>,
    intervals: &'a [FrameInterval],
    discontinuous: bool,
    derivation: DerivationMethod,
}

/// Collects one already-parsed file. The caller owns both import-local IDs;
/// neither ID establishes a live game process or system monotonic clock.
pub struct FrameLogTelemetryProvider {
    recording: FrameRecording,
    clock_domain: ClockDomainId,
    provider_id: ProviderId,
}

impl FrameLogTelemetryProvider {
    pub fn new(recording: FrameRecording, clock_domain: ClockDomainId) -> Self {
        Self {
            recording,
            clock_domain,
            provider_id: ProviderId::new(FRAME_PROVIDER_NAME)
                .expect("static frame provider identifier is valid"),
        }
    }

    pub fn recording(&self) -> &FrameRecording {
        &self.recording
    }

    pub const fn clock_domain(&self) -> ClockDomainId {
        self.clock_domain
    }

    pub fn collect_report(
        &self,
        request: &CollectionRequest,
    ) -> Result<FrameTelemetryResult, ProviderError> {
        let requested = request
            .metrics()
            .iter()
            .copied()
            .filter(|metric| FRAME_METRICS.contains(metric))
            .collect::<BTreeSet<_>>();
        let intervals = self.frame_intervals();
        if intervals.len() > MAX_FRAME_INTERVALS {
            return Err(self.provider_error(ProviderErrorKind::MalformedSourceData));
        }
        let durations = intervals
            .iter()
            .map(|interval| interval.duration_ns)
            .collect::<Vec<_>>();
        let source_metadata = self.recording.metadata();
        let timeline_discontinuous = source_metadata.discontinuity_count().is_some_and(|n| n > 0);
        let recording_duration_ns = self.recording_span_ns();
        let interval_count = u32::try_from(intervals.len())
            .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
        let mut observations = Vec::new();
        let mut issues = Vec::new();

        if requested.contains(&MetricId::FrameTime) {
            for interval in &intervals {
                observations.push(
                    self.frame_time_observation(request.context(), *interval)
                        .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?,
                );
            }
            if intervals.is_empty() {
                issues.push(ProviderIssue::new(
                    MetricId::FrameTime,
                    ProviderIssueKind::Degraded,
                ));
            }
        }

        let (fps_status, mean_status, p99_status) = match self.recording.metadata().semantics() {
            FrameMeasurementSemantics::PresentedFrameEventTimeline => {
                let events = self.recording.frame_events().unwrap_or_default();
                let fps_value = if timeline_discontinuous {
                    None
                } else if events.len() >= 2 {
                    let first = events[0].elapsed_ns();
                    let last = events[events.len() - 1].elapsed_ns();
                    let elapsed = last.checked_sub(first).ok_or_else(|| {
                        self.provider_error(ProviderErrorKind::MalformedSourceData)
                    })?;
                    Some(
                        measured_frame_rate_milli(
                            u64::try_from(events.len() - 1).map_err(|_| {
                                self.provider_error(ProviderErrorKind::MalformedSourceData)
                            })?,
                            elapsed,
                        )
                        .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?,
                    )
                } else {
                    None
                };
                if requested.contains(&MetricId::FramesPerSecond) {
                    if let Some(value) = fps_value {
                        let window = ObservationWindow::new(
                            self.timestamp(events[0].elapsed_ns()),
                            self.timestamp(events[events.len() - 1].elapsed_ns()),
                        )
                        .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
                        observations.push(
                            self.derived_observation(
                                MetricId::FramesPerSecond,
                                request.context(),
                                MeasurementValue::Unsigned(value),
                                TelemetryUnit::MilliFramesPerSecond,
                                ObservationTime::window(window),
                                None,
                                None,
                                None,
                                timeline_discontinuous,
                            )
                            .map_err(|_| {
                                self.provider_error(ProviderErrorKind::MalformedSourceData)
                            })?,
                        );
                    } else {
                        issues.push(ProviderIssue::new(
                            MetricId::FramesPerSecond,
                            ProviderIssueKind::Degraded,
                        ));
                    }
                }

                let mean_value = arithmetic_mean_nanos(&durations)
                    .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
                let p99_value = nearest_rank_p99_nanos(&durations)
                    .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
                let mean_status = self.add_derived_metric(DerivedMetricRequest {
                    requested: &requested,
                    observations: &mut observations,
                    issues: &mut issues,
                    metric: MetricId::MeanFrameTime,
                    context: request.context(),
                    value: mean_value,
                    intervals: &intervals,
                    discontinuous: timeline_discontinuous,
                    derivation: DerivationMethod::ArithmeticMean,
                })?;
                let p99_status = self.add_derived_metric(DerivedMetricRequest {
                    requested: &requested,
                    observations: &mut observations,
                    issues: &mut issues,
                    metric: MetricId::P99FrameTime,
                    context: request.context(),
                    value: p99_value,
                    intervals: &intervals,
                    discontinuous: timeline_discontinuous,
                    derivation: DerivationMethod::Percentile99,
                })?;
                let fps_status = if fps_value.is_some() {
                    FrameMetricStatus::Measured
                } else if timeline_discontinuous {
                    FrameMetricStatus::DiscontinuousTimeline
                } else {
                    FrameMetricStatus::InsufficientSamples
                };
                (fps_status, mean_status, p99_status)
            }
            FrameMeasurementSemantics::SampledLastFrameIntervalAndInstantaneousFps => {
                if requested.contains(&MetricId::FramesPerSecond) {
                    issues.push(ProviderIssue::new(
                        MetricId::FramesPerSecond,
                        ProviderIssueKind::Unsupported,
                    ));
                }
                let mean_value = arithmetic_mean_nanos(&durations)
                    .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
                let p99_value = nearest_rank_p99_nanos(&durations)
                    .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
                let mean_status = self.add_derived_metric(DerivedMetricRequest {
                    requested: &requested,
                    observations: &mut observations,
                    issues: &mut issues,
                    metric: MetricId::MeanFrameTime,
                    context: request.context(),
                    value: mean_value,
                    intervals: &intervals,
                    discontinuous: false,
                    derivation: DerivationMethod::ArithmeticMean,
                })?;
                let p99_status = self.add_derived_metric(DerivedMetricRequest {
                    requested: &requested,
                    observations: &mut observations,
                    issues: &mut issues,
                    metric: MetricId::P99FrameTime,
                    context: request.context(),
                    value: p99_value,
                    intervals: &intervals,
                    discontinuous: false,
                    derivation: DerivationMethod::Percentile99,
                })?;
                (
                    FrameMetricStatus::NoVerifiedObservationWindow,
                    mean_status,
                    p99_status,
                )
            }
            _ => {
                return Err(self.provider_error(ProviderErrorKind::Unsupported));
            }
        };

        if requested.contains(&MetricId::FrameCount) {
            issues.push(ProviderIssue::new(
                MetricId::FrameCount,
                ProviderIssueKind::Unsupported,
            ));
        }

        let batch = TelemetryBatch::new(
            request.context(),
            Some(self.clock_domain),
            None,
            observations,
        )
        .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
        batch
            .validate()
            .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
        let collection = if issues.is_empty() {
            ProviderCollection::complete(batch)
        } else {
            ProviderCollection::partial(batch, issues)
        }
        .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;

        let frame_count_status = FrameMetricStatus::UnsupportedBySource;
        Ok(FrameTelemetryResult {
            source_metadata,
            valid_interval_count: interval_count,
            recording_duration_ns,
            fps_status,
            mean_status,
            p99_status,
            frame_count_status,
            collection,
        })
    }

    fn add_derived_metric(
        &self,
        input: DerivedMetricRequest<'_>,
    ) -> Result<FrameMetricStatus, ProviderError> {
        let DerivedMetricRequest {
            requested,
            observations,
            issues,
            metric,
            context,
            value,
            intervals,
            discontinuous,
            derivation,
        } = input;
        if !requested.contains(&metric) {
            return Ok(if value.is_some() {
                FrameMetricStatus::Measured
            } else {
                FrameMetricStatus::InsufficientSamples
            });
        }
        let Some(value) = value else {
            issues.push(ProviderIssue::new(metric, ProviderIssueKind::Degraded));
            return Ok(FrameMetricStatus::InsufficientSamples);
        };
        let window = self
            .recording_window()
            .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
        let count = u32::try_from(intervals.len())
            .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
        let missing_count = self
            .recording
            .metadata()
            .missing_record_count()
            .unwrap_or(0);
        let observation = self
            .derived_observation(
                metric,
                context,
                MeasurementValue::Unsigned(value),
                TelemetryUnit::Nanoseconds,
                ObservationTime::window(window),
                Some(count),
                Some(DurationNanos(
                    window
                        .duration()
                        .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?
                        .get(),
                )),
                Some((derivation, missing_count)),
                discontinuous,
            )
            .map_err(|_| self.provider_error(ProviderErrorKind::MalformedSourceData))?;
        observations.push(observation);
        if discontinuous {
            issues.push(ProviderIssue::new(metric, ProviderIssueKind::Degraded));
            Ok(FrameMetricStatus::DiscontinuousTimeline)
        } else {
            Ok(FrameMetricStatus::Measured)
        }
    }

    fn frame_time_observation(
        &self,
        context: SamplingContextId,
        interval: FrameInterval,
    ) -> Result<TelemetryObservation, TelemetryError> {
        let (origin, method) = match self.recording.metadata().format() {
            FrameSourceFormat::FrameEventsV1 => (
                MeasurementOrigin::Raw,
                SamplingMethod::ApplicationInstrumentation,
            ),
            FrameSourceFormat::MangoHudCsv => (
                MeasurementOrigin::Normalized,
                SamplingMethod::ApplicationInstrumentation,
            ),
        };
        let provenance = self.provenance(method, origin)?;
        TelemetryObservation::measured(
            MetricId::FrameTime,
            ObservationTarget::GameSession(context),
            TelemetryMeasurement::new(
                MeasurementValue::Unsigned(interval.duration_ns),
                TelemetryUnit::Nanoseconds,
            ),
            TemporalMetadata::new(
                ObservationTime::monotonic(self.timestamp(interval.timestamp_ns)),
                None,
                None,
            )?,
            provenance,
            QualityMetadata::complete(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn derived_observation(
        &self,
        metric: MetricId,
        context: SamplingContextId,
        value: MeasurementValue,
        unit: TelemetryUnit,
        observation_time: ObservationTime,
        observation_count: Option<u32>,
        aggregation_window: Option<DurationNanos>,
        derivation: Option<(DerivationMethod, u32)>,
        degraded: bool,
    ) -> Result<TelemetryObservation, TelemetryError> {
        let sampling = SamplingMetadata::new(
            None,
            None,
            observation_count,
            aggregation_window,
            derivation.map_or(0, |(_, missing)| missing),
            derivation.map(|(method, _)| method),
        )?;
        let mut quality = if degraded {
            QualityMetadata::partial().with_flag(QualityFlag::InconsistentSourceData)
        } else {
            QualityMetadata::complete()
        };
        if metric.definition().kind == endfield_unix_telemetry::MetricKind::DerivedStatistic
            || metric == MetricId::FramesPerSecond
        {
            quality = quality.with_sampling(sampling);
        }
        let provenance = self.provenance(SamplingMethod::Derived, MeasurementOrigin::Derived)?;
        let temporal = TemporalMetadata::new(observation_time, None, None)?;
        if degraded {
            TelemetryObservation::degraded(
                metric,
                ObservationTarget::GameSession(context),
                Some(TelemetryMeasurement::new(value, unit)),
                temporal,
                provenance,
                Some(SafeDiagnostic::new(
                    endfield_unix_domain::DiagnosticCategory::StaleObservation,
                )),
                quality,
            )
        } else {
            TelemetryObservation::measured(
                metric,
                ObservationTarget::GameSession(context),
                TelemetryMeasurement::new(value, unit),
                temporal,
                provenance,
                quality,
            )
        }
    }

    fn frame_intervals(&self) -> Vec<FrameInterval> {
        match &self.recording {
            FrameRecording::EventTimeline(timeline) => event_intervals(timeline.events()),
            FrameRecording::MangoHud(recording) => mango_intervals(recording.samples()),
        }
    }

    fn recording_span_ns(&self) -> Option<u64> {
        match &self.recording {
            FrameRecording::EventTimeline(timeline) => {
                if timeline.events().len() < 2 {
                    return None;
                }
                let first = timeline.events().first()?;
                let last = timeline.events().last()?;
                last.elapsed_ns().checked_sub(first.elapsed_ns())
            }
            FrameRecording::MangoHud(recording) => {
                if recording.samples().len() < 2 {
                    return None;
                }
                let first = recording.samples().first()?;
                let last = recording.samples().last()?;
                last.elapsed_ns().checked_sub(first.elapsed_ns())
            }
        }
    }

    fn recording_window(&self) -> Result<ObservationWindow, TelemetryError> {
        let (first, last) = match &self.recording {
            FrameRecording::EventTimeline(timeline) => {
                let first = timeline
                    .events()
                    .first()
                    .ok_or(TelemetryError::InvalidSamplingMetadata)?;
                let last = timeline
                    .events()
                    .last()
                    .ok_or(TelemetryError::InvalidSamplingMetadata)?;
                (first.elapsed_ns(), last.elapsed_ns())
            }
            FrameRecording::MangoHud(recording) => {
                let first = recording
                    .samples()
                    .first()
                    .ok_or(TelemetryError::InvalidSamplingMetadata)?;
                let last = recording
                    .samples()
                    .last()
                    .ok_or(TelemetryError::InvalidSamplingMetadata)?;
                (first.elapsed_ns(), last.elapsed_ns())
            }
        };
        ObservationWindow::new(self.timestamp(first), self.timestamp(last))
    }

    fn timestamp(&self, elapsed_ns: u64) -> endfield_unix_telemetry::MonotonicTimestamp {
        endfield_unix_telemetry::MonotonicTimestamp::new(self.clock_domain, elapsed_ns)
    }

    fn provenance(
        &self,
        method: SamplingMethod,
        origin: MeasurementOrigin,
    ) -> Result<TelemetryProvenance, TelemetryError> {
        TelemetryProvenance::new(self.provider_id.clone(), method, origin)
    }

    fn provider_error(&self, kind: ProviderErrorKind) -> ProviderError {
        ProviderError::new(self.provider_id.clone(), kind)
    }
}

impl TelemetryProvider for FrameLogTelemetryProvider {
    fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    fn declared_metrics(&self) -> &[MetricId] {
        &FRAME_METRICS
    }

    fn collect(&self, request: &CollectionRequest) -> Result<ProviderCollection, ProviderError> {
        self.collect_report(request).map(|report| report.collection)
    }
}

fn event_intervals(events: &[FrameEvent]) -> Vec<FrameInterval> {
    let mut intervals = Vec::with_capacity(events.len().saturating_sub(1));
    for pair in events.windows(2) {
        if pair[1].frame_index() == pair[0].frame_index().saturating_add(1) {
            if let Some(duration_ns) = pair[1]
                .elapsed_ns()
                .checked_sub(pair[0].elapsed_ns())
                .filter(|duration| *duration > 0)
            {
                intervals.push(FrameInterval {
                    timestamp_ns: pair[1].elapsed_ns(),
                    duration_ns,
                });
            }
        }
    }
    intervals
}

fn mango_intervals(samples: &[MangoHudFrameSample]) -> Vec<FrameInterval> {
    samples
        .iter()
        .map(|sample| FrameInterval {
            timestamp_ns: sample.elapsed_ns(),
            duration_ns: sample.frametime_ns(),
        })
        .collect()
}
