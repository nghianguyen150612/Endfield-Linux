use endfield_unix_domain::provenance::{Diagnostic, DiagnosticCategory};
use endfield_unix_domain::{
    Availability, CapabilityId, CapabilityRecord, Measurement, MeasurementOrigin, MeasurementUnit,
    MeasurementValue, PlatformFamily, ProvenanceId, ValidationError,
};

#[test]
fn platform_and_capability_abi_identifiers_are_explicit_and_stable() {
    let capabilities = [
        (CapabilityId::CpuTopology, 1),
        (CapabilityId::CpuScheduling, 2),
        (CapabilityId::CpuFrequencyPower, 3),
        (CapabilityId::GpuDevice, 10),
        (CapabilityId::Vram, 11),
        (CapabilityId::HardwareAcceleration, 12),
        (CapabilityId::GraphicsDriver, 13),
        (CapabilityId::VulkanDevice, 14),
        (CapabilityId::Ram, 20),
        (CapabilityId::Swap, 21),
        (CapabilityId::CpuThermal, 30),
        (CapabilityId::GpuThermal, 31),
        (CapabilityId::PowerTelemetry, 32),
        (CapabilityId::Display, 40),
        (CapabilityId::CompositorSession, 41),
        (CapabilityId::RuntimeDiscovery, 50),
        (CapabilityId::OptimizationTools, 51),
    ];
    for (id, abi_id) in capabilities {
        assert_eq!(id.abi_id(), abi_id);
        assert_eq!(CapabilityId::try_from(abi_id).unwrap(), id);
    }
    for reserved in [0, 4, 9, 15, 19, 22, 29, 33, 39, 42, 49, 52, 255] {
        assert!(CapabilityId::try_from(reserved).is_err(), "{reserved}");
    }

    assert_eq!(PlatformFamily::Linux.abi_id(), 1);
    assert_eq!(PlatformFamily::MacOs.abi_id(), 2);
    assert_eq!(PlatformFamily::FreeBsd.abi_id(), 3);
    assert_eq!(PlatformFamily::OtherUnknown.abi_id(), 255);
    for invalid in [0, 4, 255 - 1] {
        assert!(PlatformFamily::try_from(invalid).is_err());
    }
    assert_eq!(
        PlatformFamily::try_from(255).unwrap(),
        PlatformFamily::OtherUnknown
    );
}

#[test]
fn availability_and_measurement_unit_identifiers_match_the_c17_contract() {
    for (id, state) in [
        (0, Availability::Unknown),
        (1, Availability::Unavailable),
        (2, Availability::Available),
        (3, Availability::Degraded),
    ] {
        assert_eq!(state.abi_id(), id);
        assert_eq!(Availability::try_from(id).unwrap(), state);
    }
    assert!(Availability::try_from(4).is_err());

    let units = [
        MeasurementUnit::Count,
        MeasurementUnit::Bytes,
        MeasurementUnit::Hertz,
        MeasurementUnit::Pixels,
        MeasurementUnit::MilliFramesPerSecond,
        MeasurementUnit::Milliseconds,
        MeasurementUnit::Microseconds,
        MeasurementUnit::Nanoseconds,
        MeasurementUnit::MilliCelsius,
        MeasurementUnit::Milliwatts,
        MeasurementUnit::Millivolts,
        MeasurementUnit::Joules,
        MeasurementUnit::Millijoules,
    ];
    for (index, unit) in units.into_iter().enumerate() {
        let id = u32::try_from(index + 1).unwrap();
        assert_eq!(unit.abi_id(), id);
        assert_eq!(MeasurementUnit::try_from(id).unwrap(), unit);
    }
    assert!(MeasurementUnit::try_from(0).is_err());
    assert!(MeasurementUnit::try_from(14).is_err());
    assert!(MeasurementUnit::try_from(u32::MAX).is_err());
    assert!(CapabilityId::try_from(u32::MAX).is_err());
    assert!(Availability::try_from(u32::MAX).is_err());
    assert!(PlatformFamily::try_from(u32::MAX).is_err());
}

#[test]
fn unknown_unavailable_missing_and_numeric_zero_remain_distinct() {
    let mut unknown = CapabilityRecord::new(CapabilityId::CpuThermal, Availability::Unknown);
    unknown.diagnostic = Some(Diagnostic {
        category: DiagnosticCategory::NotYetProbed,
        provider: None,
        message: None,
    });
    unknown.validate().unwrap();

    let mut unavailable =
        CapabilityRecord::new(CapabilityId::GpuThermal, Availability::Unavailable);
    unavailable.provenance = Some(ProvenanceId::new(1).unwrap());
    unavailable.diagnostic = Some(Diagnostic {
        category: DiagnosticCategory::UnsupportedCapability,
        provider: None,
        message: Some("sensor is not present".to_owned()),
    });
    unavailable.validate().unwrap();

    let mut available_zero =
        CapabilityRecord::new(CapabilityId::CpuThermal, Availability::Available);
    available_zero.provenance = Some(ProvenanceId::new(1).unwrap());
    available_zero.measurement = Some(Measurement::unsigned(
        0,
        MeasurementUnit::MilliCelsius,
        MeasurementOrigin::Raw,
    ));
    available_zero.validate().unwrap();
    assert_eq!(
        available_zero.measurement.unwrap().value(),
        MeasurementValue::Unsigned(0)
    );

    let mut available_without_value =
        CapabilityRecord::new(CapabilityId::HardwareAcceleration, Availability::Available);
    available_without_value.provenance = Some(ProvenanceId::new(1).unwrap());
    available_without_value.validate().unwrap();
    assert!(available_without_value.measurement.is_none());
}

#[test]
fn invalid_state_value_and_unit_combinations_are_rejected() {
    let mut unknown_with_zero =
        CapabilityRecord::new(CapabilityId::CpuTopology, Availability::Unknown);
    unknown_with_zero.measurement = Some(Measurement::unsigned(
        0,
        MeasurementUnit::Count,
        MeasurementOrigin::Raw,
    ));
    assert!(matches!(
        unknown_with_zero.validate(),
        Err(ValidationError::ValueNotAllowedForAvailability { .. })
    ));

    let mut ram_as_frequency = CapabilityRecord::new(CapabilityId::Ram, Availability::Available);
    ram_as_frequency.provenance = Some(ProvenanceId::new(1).unwrap());
    ram_as_frequency.measurement = Some(Measurement::unsigned(
        1024,
        MeasurementUnit::Hertz,
        MeasurementOrigin::Normalized,
    ));
    assert!(matches!(
        ram_as_frequency.validate(),
        Err(ValidationError::InvalidUnitForCapability { .. })
    ));

    let mut degraded = CapabilityRecord::new(CapabilityId::VulkanDevice, Availability::Degraded);
    degraded.provenance = Some(ProvenanceId::new(1).unwrap());
    assert!(matches!(
        degraded.validate(),
        Err(ValidationError::MissingDiagnostic { .. })
    ));
}

#[test]
fn exact_conversions_preserve_zero_and_reject_overflow_or_precision_loss() {
    let zero = Measurement::checked_convert_unsigned(
        0,
        1024,
        1,
        MeasurementUnit::Bytes,
        MeasurementOrigin::Normalized,
    )
    .unwrap();
    assert_eq!(zero.value(), MeasurementValue::Unsigned(0));

    assert!(matches!(
        Measurement::checked_convert_unsigned(
            u64::MAX,
            2,
            1,
            MeasurementUnit::Bytes,
            MeasurementOrigin::Normalized,
        ),
        Err(ValidationError::MeasurementOverflow)
    ));
    assert!(matches!(
        Measurement::checked_convert_unsigned(
            1,
            1,
            2,
            MeasurementUnit::Bytes,
            MeasurementOrigin::Normalized,
        ),
        Err(ValidationError::InexactConversion)
    ));
    assert!(matches!(
        Measurement::checked_convert_unsigned(
            1,
            1,
            0,
            MeasurementUnit::Bytes,
            MeasurementOrigin::Normalized,
        ),
        Err(ValidationError::InvalidConversionScale)
    ));
}

#[test]
fn negative_temperature_is_preserved_in_domain_and_rejected_by_abi_v1() {
    let below_zero =
        Measurement::signed(-1250, MeasurementUnit::MilliCelsius, MeasurementOrigin::Raw).unwrap();
    assert_eq!(below_zero.value(), MeasurementValue::Signed(-1250));
    assert!(below_zero.to_abi().is_err());

    let zero = Measurement::unsigned(0, MeasurementUnit::MilliCelsius, MeasurementOrigin::Raw);
    assert_eq!(zero.to_abi().unwrap().value, 0);
    assert!(Measurement::signed(1, MeasurementUnit::Bytes, MeasurementOrigin::Raw).is_err());
    assert!(Measurement::from_abi(u64::MAX, u32::MAX).is_err());
}
