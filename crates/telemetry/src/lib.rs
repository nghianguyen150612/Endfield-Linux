//! Portable telemetry contracts for `endfield-unix`.
//!
//! This library defines metric identities, units, observation state, temporal
//! semantics, provenance, quality, freshness, batches, and a provider boundary.
//! It does not collect hardware data, start timers, poll in the background, or
//! access the filesystem, network, GUI, daemon, storage, or optimizer.

#![forbid(unsafe_code)]

pub mod batch;
pub mod errors;
pub mod freshness;
pub mod metric;
pub mod observation;
pub mod provenance;
pub mod provider;
pub mod quality;
pub mod scope;
pub mod timestamp;
pub mod units;
pub mod validation;

pub use batch::{CURRENT_TELEMETRY_SCHEMA_VERSION, MAX_OBSERVATIONS_PER_BATCH, TelemetryBatch};
pub use errors::TelemetryError;
pub use freshness::{
    FreshnessOutcome, FreshnessPolicy, FreshnessReason, InvalidTemporalRelationship,
    evaluate_freshness,
};
pub use metric::{
    MetricDefinition, MetricId, MetricKind, MetricScope, MetricValueKind, NumericRange,
    metric_registry,
};
pub use observation::{ObservationState, ObservationTime, TelemetryObservation, TemporalMetadata};
pub use provenance::{
    ProviderId, SafeDiagnostic, SamplingContextId, SamplingMethod, SnapshotRefId,
    TelemetryProvenance,
};
pub use provider::{
    CollectionCompleteness, CollectionRequest, ProviderCollection, ProviderError,
    ProviderErrorKind, ProviderIssue, ProviderIssueKind, TelemetryProvider,
};
pub use quality::{
    Completeness, DerivationMethod, ProviderUncertainty, QualityFlag, QualityMetadata,
    SamplingMetadata,
};
pub use scope::{DeviceReference, GpuEngineKind, ObservationTarget, SnapshotContext};
pub use timestamp::{
    ClockDomainId, DurationNanos, MonotonicObservation, MonotonicTimestamp, ObservationWindow,
    WallClockTimestamp,
};
pub use units::{
    BASIS_POINTS_PER_PERCENT, MAX_NORMALIZED_UTILIZATION_BASIS_POINTS, TelemetryMeasurement,
    TelemetryUnit, Utilization, UtilizationScope, checked_convert_to_domain,
    checked_convert_unsigned, checked_rate_from_counter, checked_rate_measurement_from_counter,
    to_domain_measurement,
};
pub use validation::{
    CounterDelta, CounterDeltaOutcome, CounterResetEvidence, CounterWrapBehavior, RateRounding,
    checked_counter_delta, checked_frame_rate,
};
