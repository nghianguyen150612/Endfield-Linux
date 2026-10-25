mod common;

use endfield_unix_domain::{DiagnosticCategory, MeasurementValue};
use endfield_unix_telemetry::{
    DurationNanos, FreshnessOutcome, FreshnessPolicy, MetricId, ObservationState,
    ObservationTarget, QualityMetadata, SafeDiagnostic, TelemetryError, TelemetryObservation,
    TelemetryUnit, evaluate_freshness,
};

use common::{measurement, mono, point_time, provenance};

#[test]
fn measured_zero_unknown_unsupported_missing_and_failure_remain_distinct() {
    let zero = TelemetryObservation::measured(
        MetricId::RamUsed,
        ObservationTarget::Platform,
        measurement(0, TelemetryUnit::Bytes),
        point_time(1),
        provenance("synthetic.memory"),
        QualityMetadata::complete(),
    )
    .unwrap();
    assert_eq!(
        zero.state(),
        ObservationState::Measured(measurement(0, TelemetryUnit::Bytes))
    );

    let cases = [
        (
            ObservationState::NotYetSampled,
            Some(DiagnosticCategory::NotYetProbed),
        ),
        (ObservationState::Unknown, None),
        (
            ObservationState::ProviderMissing,
            Some(DiagnosticCategory::MissingProvider),
        ),
        (
            ObservationState::Unsupported,
            Some(DiagnosticCategory::UnsupportedCapability),
        ),
        (
            ObservationState::PermissionDenied,
            Some(DiagnosticCategory::PermissionDenied),
        ),
        (
            ObservationState::TemporaryFailure,
            Some(DiagnosticCategory::TemporaryFailure),
        ),
        (
            ObservationState::MalformedSourceData,
            Some(DiagnosticCategory::MalformedSourceData),
        ),
    ];
    let mut observations = Vec::new();
    for (index, (state, category)) in cases.into_iter().enumerate() {
        let diagnostic = category.map(SafeDiagnostic::new);
        observations.push(
            TelemetryObservation::unavailable(
                MetricId::RamUsed,
                ObservationTarget::Platform,
                state,
                point_time(10 + index as u64),
                provenance("synthetic.memory"),
                diagnostic,
            )
            .unwrap(),
        );
    }
    assert!(
        observations
            .iter()
            .all(|observation| observation.value().is_none())
    );
    assert_ne!(observations[1].state(), observations[3].state());
    assert_ne!(observations[2].state(), observations[3].state());
    assert_ne!(observations[4].state(), observations[5].state());
}

#[test]
fn degraded_observation_can_keep_a_value_without_becoming_complete() {
    let degraded = TelemetryObservation::degraded(
        MetricId::RamAvailable,
        ObservationTarget::Platform,
        Some(measurement(500, TelemetryUnit::Bytes)),
        point_time(1),
        provenance("synthetic.memory"),
        Some(SafeDiagnostic::new(DiagnosticCategory::TemporaryFailure)),
        QualityMetadata::partial(),
    )
    .unwrap();
    assert_eq!(
        degraded.value().unwrap().value(),
        MeasurementValue::Unsigned(500)
    );
    assert_eq!(
        degraded.quality().completeness(),
        endfield_unix_telemetry::Completeness::Partial
    );

    let invalid = TelemetryObservation::degraded(
        MetricId::RamAvailable,
        ObservationTarget::Platform,
        Some(measurement(500, TelemetryUnit::Bytes)),
        point_time(2),
        provenance("synthetic.memory"),
        None,
        QualityMetadata::complete(),
    );
    assert!(matches!(
        invalid,
        Err(TelemetryError::InvalidObservationState)
    ));
}

#[test]
fn failure_states_cannot_carry_values_or_conflicting_categories() {
    assert!(matches!(
        TelemetryObservation::unavailable(
            MetricId::RamUsed,
            ObservationTarget::Platform,
            ObservationState::Measured(measurement(0, TelemetryUnit::Bytes)),
            point_time(1),
            provenance("synthetic.memory"),
            None,
        ),
        Err(TelemetryError::InvalidObservationState)
    ));
    assert!(matches!(
        TelemetryObservation::unavailable(
            MetricId::RamUsed,
            ObservationTarget::Platform,
            ObservationState::PermissionDenied,
            point_time(1),
            provenance("synthetic.memory"),
            Some(SafeDiagnostic::new(
                DiagnosticCategory::UnsupportedCapability
            )),
        ),
        Err(TelemetryError::InvalidObservationState)
    ));
}

#[test]
fn measured_state_cannot_claim_partial_quality_without_degraded_status() {
    let inconsistent = TelemetryObservation::measured(
        MetricId::RamUsed,
        ObservationTarget::Platform,
        measurement(1, TelemetryUnit::Bytes),
        point_time(1),
        provenance("synthetic.memory"),
        QualityMetadata::partial(),
    );
    assert!(matches!(
        inconsistent,
        Err(TelemetryError::InvalidObservationState)
    ));
}

#[test]
fn stale_observation_keeps_the_value_and_is_explicit_at_evaluation_time() {
    let sample = common::example_memory_sample(0);
    let policy = FreshnessPolicy::new(DurationNanos(1)).unwrap();
    let evaluated = evaluate_freshness(&sample, mono(9_000_000_000), policy);
    assert!(matches!(evaluated, FreshnessOutcome::Stale { .. }));
    assert_eq!(
        sample.state(),
        ObservationState::Measured(measurement(0, TelemetryUnit::Bytes))
    );
    assert_eq!(
        sample.value().unwrap().value(),
        MeasurementValue::Unsigned(0)
    );
}
