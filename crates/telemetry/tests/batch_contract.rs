mod common;

use endfield_unix_telemetry::{
    CollectionCompleteness, CollectionRequest, CounterResetEvidence, MAX_OBSERVATIONS_PER_BATCH,
    MetricId, ObservationTarget, ProviderCollection, ProviderError, ProviderErrorKind,
    ProviderIssue, ProviderIssueKind, SamplingContextId, TelemetryBatch, TelemetryError,
    TelemetryProvider, TelemetryUnit,
};

use common::{
    example_frame_rate, measurement, point_time, sample_context, two_gpu_snapshot, window_time,
};

#[test]
fn empty_batches_are_valid_and_have_no_observations() {
    let batch = TelemetryBatch::new(sample_context(), None, None, Vec::new()).unwrap();
    assert!(batch.validate().is_ok());
    assert_eq!(batch.schema_version(), 1);
    assert!(batch.observations().is_empty());

    let declared_clock =
        TelemetryBatch::new(sample_context(), Some(common::clock()), None, Vec::new()).unwrap();
    assert!(declared_clock.validate().is_ok());
}

#[test]
fn multiple_metrics_and_two_gpu_targets_resolve_in_one_snapshot() {
    let (_, snapshot) = two_gpu_snapshot();
    let cpu = common::example_cpu_sample();
    let gpu_a = common::measured(
        MetricId::GpuUtilization,
        common::gpu_target(2),
        measurement(3_000, TelemetryUnit::BasisPoints),
        window_time(10, 20),
        "synthetic.gpu",
    );
    let gpu_b = common::measured(
        MetricId::GpuUtilization,
        common::gpu_target(3),
        measurement(7_500, TelemetryUnit::BasisPoints),
        window_time(10, 20),
        "synthetic.gpu",
    );
    let engine = common::measured(
        MetricId::GpuEngineUtilization,
        ObservationTarget::GpuEngine {
            gpu: match common::gpu_target(2) {
                ObservationTarget::Gpu(reference) => reference,
                _ => unreachable!(),
            },
            engine: endfield_unix_telemetry::GpuEngineKind::Graphics,
        },
        measurement(9_000, TelemetryUnit::BasisPoints),
        window_time(10, 20),
        "synthetic.gpu",
    );
    let frame = example_frame_rate();
    let batch = TelemetryBatch::new(
        sample_context(),
        Some(common::clock()),
        Some(snapshot),
        vec![gpu_b, frame, cpu, gpu_a, engine],
    )
    .unwrap();
    assert_eq!(batch.observations().len(), 5);
    assert!(batch.observations().windows(2).all(|pair| {
        (pair[0].metric().stable_id(), pair[0].target())
            <= (pair[1].metric().stable_id(), pair[1].target())
    }));
}

#[test]
fn indistinguishable_same_provider_samples_are_rejected_but_distinct_times_are_valid() {
    let duplicate_a = common::example_memory_sample(1);
    let duplicate_b = common::example_memory_sample(2);
    assert!(matches!(
        TelemetryBatch::new(
            sample_context(),
            Some(common::clock()),
            None,
            vec![duplicate_a.clone(), duplicate_b],
        ),
        Err(TelemetryError::DuplicateObservation)
    ));

    let later = common::measured(
        MetricId::RamUsed,
        ObservationTarget::Platform,
        measurement(2, TelemetryUnit::Bytes),
        point_time(3_000_000_001),
        "synthetic.memory",
    );
    let batch = TelemetryBatch::new(
        sample_context(),
        Some(common::clock()),
        None,
        vec![later, duplicate_a],
    )
    .unwrap();
    assert_eq!(batch.observations().len(), 2);
}

#[test]
fn clock_domain_mixture_and_unresolved_device_targets_fail_closed() {
    let other = endfield_unix_telemetry::ClockDomainId::new(99).unwrap();
    let incompatible = common::measured(
        MetricId::RamUsed,
        ObservationTarget::Platform,
        measurement(1, TelemetryUnit::Bytes),
        endfield_unix_telemetry::TemporalMetadata::new(
            endfield_unix_telemetry::ObservationTime::monotonic(
                endfield_unix_telemetry::MonotonicTimestamp::new(other, 4),
            ),
            None,
            None,
        )
        .unwrap(),
        "synthetic.otherclock",
    );
    assert!(matches!(
        TelemetryBatch::new(
            sample_context(),
            Some(common::clock()),
            None,
            vec![incompatible],
        ),
        Err(TelemetryError::ClockDomainMismatch)
    ));

    let gpu = common::measured(
        MetricId::GpuUtilization,
        common::gpu_target(2),
        measurement(100, TelemetryUnit::BasisPoints),
        window_time(1, 2),
        "synthetic.gpu",
    );
    assert!(matches!(
        TelemetryBatch::new(sample_context(), Some(common::clock()), None, vec![gpu]),
        Err(TelemetryError::UnresolvedDeviceReference)
    ));
}

#[test]
fn schema_version_and_observation_count_are_checked() {
    assert!(matches!(
        TelemetryBatch::with_schema_version(2, sample_context(), None, None, Vec::new()),
        Err(TelemetryError::UnsupportedSchemaVersion {
            found: 2,
            supported: 1
        })
    ));

    let at_limit = (0..MAX_OBSERVATIONS_PER_BATCH)
        .map(|index| {
            common::measured(
                MetricId::RamUsed,
                ObservationTarget::Platform,
                measurement(index as u64, TelemetryUnit::Bytes),
                point_time(index as u64),
                "synthetic.large",
            )
        })
        .collect();
    assert!(TelemetryBatch::new(sample_context(), Some(common::clock()), None, at_limit,).is_ok());

    let above_limit = (0..=MAX_OBSERVATIONS_PER_BATCH)
        .map(|index| {
            common::measured(
                MetricId::RamUsed,
                ObservationTarget::Platform,
                measurement(index as u64, TelemetryUnit::Bytes),
                point_time(index as u64),
                "synthetic.large",
            )
        })
        .collect();
    assert!(matches!(
        TelemetryBatch::new(sample_context(), Some(common::clock()), None, above_limit,),
        Err(TelemetryError::BatchTooLarge { .. })
    ));
}

#[test]
fn normalization_is_deterministic_and_does_not_mutate_callers_data() {
    let first = common::example_memory_sample(1);
    let second = common::measured(
        MetricId::RamAvailable,
        ObservationTarget::Platform,
        measurement(2, TelemetryUnit::Bytes),
        point_time(3_000_000_001),
        "synthetic.memory",
    );
    let original = vec![second, first];
    let preserved = original.clone();
    let first_batch = TelemetryBatch::new(
        sample_context(),
        Some(common::clock()),
        None,
        original.clone(),
    )
    .unwrap();
    let second_batch =
        TelemetryBatch::new(sample_context(), Some(common::clock()), None, original).unwrap();
    assert_eq!(first_batch, second_batch);
    assert_eq!(preserved[0].metric(), MetricId::RamAvailable);
    assert_eq!(first_batch.observations()[0].metric(), MetricId::RamUsed);
}

#[test]
fn provider_contract_distinguishes_partial_results_from_fatal_failure() {
    struct SyntheticProvider {
        id: endfield_unix_telemetry::ProviderId,
        metrics: Vec<MetricId>,
    }
    impl TelemetryProvider for SyntheticProvider {
        fn provider_id(&self) -> &endfield_unix_telemetry::ProviderId {
            &self.id
        }

        fn declared_metrics(&self) -> &[MetricId] {
            &self.metrics
        }

        fn collect(
            &self,
            request: &CollectionRequest,
        ) -> Result<ProviderCollection, ProviderError> {
            if request.metrics().contains(&MetricId::RamUsed) {
                let batch = TelemetryBatch::new(
                    request.context(),
                    request.requested_at().map(|timestamp| timestamp.domain()),
                    None,
                    vec![common::example_memory_sample(0)],
                )
                .unwrap();
                ProviderCollection::partial(
                    batch,
                    vec![ProviderIssue::new(
                        MetricId::RamAvailable,
                        ProviderIssueKind::PermissionDenied,
                    )],
                )
                .map_err(|_| {
                    ProviderError::new(self.id.clone(), ProviderErrorKind::MalformedSourceData)
                })
            } else {
                Err(ProviderError::new(
                    self.id.clone(),
                    ProviderErrorKind::TemporaryFailure,
                ))
            }
        }
    }

    let provider = SyntheticProvider {
        id: common::provider("synthetic.provider"),
        metrics: vec![MetricId::RamUsed, MetricId::RamAvailable],
    };
    let request = CollectionRequest::new(
        sample_context(),
        vec![MetricId::RamAvailable, MetricId::RamUsed, MetricId::RamUsed],
        Some(common::mono(3_000_000_000)),
    )
    .unwrap();
    assert_eq!(request.metrics().len(), 2);
    let partial = provider.collect(&request).unwrap();
    assert_eq!(partial.completeness(), CollectionCompleteness::Partial);
    assert_eq!(
        partial.issues()[0].kind(),
        ProviderIssueKind::PermissionDenied
    );
    assert_eq!(
        provider
            .collect(
                &CollectionRequest::new(sample_context(), vec![MetricId::GpuPower], None).unwrap()
            )
            .unwrap_err()
            .kind(),
        ProviderErrorKind::TemporaryFailure
    );
    assert!(SamplingContextId::new(0).is_err());
    assert_eq!(CounterResetEvidence::Unknown, CounterResetEvidence::Unknown);
}
