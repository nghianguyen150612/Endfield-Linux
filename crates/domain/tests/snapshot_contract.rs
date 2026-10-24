#[allow(dead_code)]
mod common;

use common::{desktop_fixture, incomplete_fixture, laptop_fixture};
use endfield_unix_domain::{
    Availability, Bytes, CURRENT_SNAPSHOT_SCHEMA_VERSION, CapabilityId, CapabilityValue,
    Measurement, MeasurementOrigin, MeasurementUnit, MemoryCapacity, ValidationError,
};

#[test]
fn partial_snapshots_validate_without_inventing_missing_subsystems() {
    let incomplete = incomplete_fixture();
    incomplete.validate().unwrap();
    assert!(incomplete.cpu.is_none());
    assert!(incomplete.gpus.is_empty());
    assert!(incomplete.displays.is_empty());
    assert_eq!(
        incomplete.memory.as_ref().unwrap().ram.availability,
        Availability::Unknown
    );
}

#[test]
fn laptop_snapshot_represents_multiple_gpus_without_preference_order() {
    let laptop = laptop_fixture();
    laptop.validate().unwrap();
    assert_eq!(laptop.gpus.len(), 2);
    assert_ne!(laptop.gpus[0].instance, laptop.gpus[1].instance);
    assert!(
        laptop
            .gpus
            .iter()
            .any(|gpu| gpu.class == endfield_unix_domain::GpuClass::Integrated)
    );
    assert!(
        laptop
            .gpus
            .iter()
            .any(|gpu| gpu.class == endfield_unix_domain::GpuClass::Discrete)
    );
    assert!(laptop.gpus.iter().all(|gpu| gpu.instance.get() != 0));
}

#[test]
fn duplicate_device_and_capability_identities_are_rejected() {
    let mut duplicate_device = laptop_fixture();
    duplicate_device.gpus[1].instance = duplicate_device.gpus[0].instance;
    assert!(
        duplicate_device
            .validate()
            .unwrap_err()
            .0
            .iter()
            .any(|error| matches!(error, ValidationError::DuplicateInstance { .. }))
    );

    let mut duplicate_capability = desktop_fixture();
    let existing = duplicate_capability.capabilities[0].clone();
    duplicate_capability.capabilities.push(existing);
    assert!(
        duplicate_capability
            .validate()
            .unwrap_err()
            .0
            .iter()
            .any(|error| matches!(error, ValidationError::DuplicateCapability { .. }))
    );
}

#[test]
fn capability_instance_and_provenance_references_must_resolve() {
    let mut missing_instance = desktop_fixture();
    missing_instance.capabilities[0].instance =
        Some(endfield_unix_domain::InstanceId::new(99).unwrap());
    assert!(
        missing_instance
            .validate()
            .unwrap_err()
            .0
            .iter()
            .any(|error| matches!(error, ValidationError::UnknownInstanceReference { .. }))
    );

    let mut missing_provenance = desktop_fixture();
    missing_provenance.capabilities[0].provenance =
        Some(endfield_unix_domain::ProvenanceId::new(99).unwrap());
    assert!(
        missing_provenance
            .validate()
            .unwrap_err()
            .0
            .iter()
            .any(|error| matches!(error, ValidationError::UnknownProvenanceReference { .. }))
    );
}

#[test]
fn unsupported_snapshot_schema_fails_safely() {
    let mut snapshot = desktop_fixture();
    snapshot.schema_version = CURRENT_SNAPSHOT_SCHEMA_VERSION + 1;
    assert!(matches!(
        snapshot.validate().unwrap_err().0.first(),
        Some(ValidationError::UnsupportedSnapshotSchema { .. })
    ));
}

#[test]
fn normalization_is_deterministic_and_record_order_has_no_semantics() {
    let expected = laptop_fixture().normalize().unwrap();
    let mut reordered = laptop_fixture();
    reordered.gpus.reverse();
    reordered.capabilities.reverse();
    reordered.provenance.reverse();
    let actual = reordered.normalize().unwrap();
    assert_eq!(actual, expected);

    let mut unsorted = desktop_fixture();
    unsorted.capabilities.reverse();
    assert!(unsorted.validate().is_ok());
}

#[test]
fn invalid_memory_relationships_and_required_metadata_are_rejected() {
    let mut snapshot = desktop_fixture();
    snapshot.memory.as_mut().unwrap().ram = CapabilityValue::new(
        Availability::Available,
        Some(MemoryCapacity {
            total: Some(Bytes(4)),
            available: Some(Bytes(8)),
        }),
    );
    assert!(snapshot.validate().is_err());

    let mut invalid_platform = desktop_fixture();
    invalid_platform.platform.os.name.clear();
    assert!(matches!(
        invalid_platform.validate().unwrap_err().0.first(),
        Some(ValidationError::InvalidText { field: "OS name" })
    ));
}

#[test]
fn typed_inventory_status_and_measurement_must_match_capability_records() {
    let mut missing = desktop_fixture();
    missing
        .capabilities
        .retain(|capability| capability.id != CapabilityId::CpuTopology);
    assert!(
        missing
            .validate()
            .unwrap_err()
            .0
            .iter()
            .any(|error| matches!(error, ValidationError::MissingCapabilityRecord { .. }))
    );

    let mut mismatched_measurement = desktop_fixture();
    let topology = mismatched_measurement
        .capabilities
        .iter_mut()
        .find(|capability| capability.id == CapabilityId::CpuTopology)
        .unwrap();
    topology.measurement = Some(Measurement::unsigned(
        19,
        MeasurementUnit::Count,
        MeasurementOrigin::Normalized,
    ));
    assert!(
        mismatched_measurement
            .validate()
            .unwrap_err()
            .0
            .iter()
            .any(|error| matches!(error, ValidationError::CapabilityMeasurementMismatch { .. }))
    );
}
