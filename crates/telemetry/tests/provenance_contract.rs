mod common;

use endfield_unix_domain::{DiagnosticCategory, MeasurementOrigin, ProvenanceId};
use endfield_unix_telemetry::{
    DeviceReference, MetricId, ObservationTarget, ProviderId, SafeDiagnostic, SamplingContextId,
    SamplingMethod, SnapshotRefId, TelemetryBatch, TelemetryError, TelemetryProvenance,
};

use common::{measurement, point_time, provenance, sample_context, snapshot_ref, two_gpu_snapshot};

#[test]
fn provider_ids_follow_p007_validation_and_reject_private_source_details() {
    assert_eq!(
        ProviderId::new("linux-cpu-sysfs").unwrap().as_str(),
        "linux-cpu-sysfs"
    );
    for invalid in [
        "",
        "/sys/devices",
        "provider with spaces",
        "TOKEN=value",
        "a\nb",
    ] {
        assert!(matches!(
            ProviderId::new(invalid),
            Err(TelemetryError::InvalidProviderId)
        ));
    }
}

#[test]
fn same_metric_from_different_providers_preserves_each_source() {
    let first = common::measured(
        MetricId::RamUsed,
        ObservationTarget::Platform,
        measurement(1_000, endfield_unix_telemetry::TelemetryUnit::Bytes),
        point_time(1),
        "synthetic.provider_a",
    );
    let second = common::measured(
        MetricId::RamUsed,
        ObservationTarget::Platform,
        measurement(1_100, endfield_unix_telemetry::TelemetryUnit::Bytes),
        point_time(1),
        "synthetic.provider_b",
    );
    let batch = TelemetryBatch::new(
        sample_context(),
        Some(common::clock()),
        None,
        vec![second, first],
    )
    .unwrap();
    assert_eq!(batch.observations().len(), 2);
    assert_eq!(
        batch.observations()[0].provenance().provider().as_str(),
        "synthetic.provider_a"
    );
    assert_eq!(
        batch.observations()[1].provenance().provider().as_str(),
        "synthetic.provider_b"
    );
}

#[test]
fn provenance_reuses_domain_provider_and_wall_clock_policy() {
    let wall = endfield_unix_telemetry::WallClockTimestamp::new(1_766_000_000_000_000_000);
    let source = TelemetryProvenance::new(
        ProviderId::new("synthetic.provider").unwrap(),
        SamplingMethod::Synthetic,
        MeasurementOrigin::Normalized,
    )
    .unwrap();
    let domain = source
        .to_domain_provenance(ProvenanceId::new(9).unwrap(), Some(wall))
        .unwrap();
    assert_eq!(domain.provider, "synthetic.provider");
    assert_eq!(domain.observed_at_unix_nanos, Some(wall.unix_nanos()));
}

#[test]
fn diagnostics_expose_only_fixed_categories_and_not_error_strings() {
    let diagnostic = SafeDiagnostic::new(DiagnosticCategory::PermissionDenied);
    assert_eq!(diagnostic.category(), DiagnosticCategory::PermissionDenied);
    // The diagnostic contract has no message, path, environment, or raw error field.
    let sample = endfield_unix_telemetry::TelemetryObservation::unavailable(
        MetricId::RamUsed,
        ObservationTarget::Platform,
        endfield_unix_telemetry::ObservationState::PermissionDenied,
        point_time(1),
        provenance("synthetic.memory"),
        Some(diagnostic),
    )
    .unwrap();
    assert_eq!(
        sample.diagnostic().unwrap().category(),
        DiagnosticCategory::PermissionDenied
    );
}

#[test]
fn snapshot_local_device_reference_is_resolved_only_in_its_snapshot() {
    let (_, context) = two_gpu_snapshot();
    let reference = DeviceReference::new(
        snapshot_ref(),
        endfield_unix_domain::InstanceId::new(2).unwrap(),
    );
    let target = ObservationTarget::Gpu(reference);
    let sample = common::measured(
        MetricId::GpuUtilization,
        target,
        measurement(500, endfield_unix_telemetry::TelemetryUnit::BasisPoints),
        common::window_time(1, 2),
        "synthetic.gpu",
    );
    assert!(
        TelemetryBatch::new(
            sample_context(),
            Some(common::clock()),
            Some(context),
            vec![sample],
        )
        .is_ok()
    );

    let (_, other_context) = two_gpu_snapshot();
    let mismatched_reference = DeviceReference::new(
        SnapshotRefId::new(777).unwrap(),
        endfield_unix_domain::InstanceId::new(2).unwrap(),
    );
    let mismatched = common::measured(
        MetricId::GpuUtilization,
        ObservationTarget::Gpu(mismatched_reference),
        measurement(600, endfield_unix_telemetry::TelemetryUnit::BasisPoints),
        common::window_time(1, 2),
        "synthetic.gpu",
    );
    assert!(matches!(
        TelemetryBatch::new(
            sample_context(),
            Some(common::clock()),
            Some(other_context),
            vec![mismatched],
        ),
        Err(TelemetryError::SnapshotReferenceMismatch)
    ));
}

#[test]
fn process_and_game_session_targets_use_opaque_contexts() {
    let process = SamplingContextId::new(44).unwrap();
    let session = SamplingContextId::new(45).unwrap();
    assert_ne!(process, session);
    assert!(SamplingContextId::new(0).is_err());
    assert!(SnapshotRefId::new(0).is_err());
    let _ = (
        ObservationTarget::Process(process),
        ObservationTarget::GameSession(session),
    );
}
