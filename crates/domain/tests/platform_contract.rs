#[allow(dead_code)]
mod common;

use common::{desktop_fixture, incomplete_fixture, report};
use endfield_unix_domain::provenance::DiagnosticCategory;
use endfield_unix_domain::{
    Architecture, CapabilityProvider, CompositePlatformProbe, OsIdentity, PlatformFamily,
    PlatformIdentity, PlatformProbe, ProbeError, ProbeErrorKind, ProbeReport, Provenance,
    ProvenanceId, SystemSnapshot,
};

struct FakePlatformProbe {
    result: Result<SystemSnapshot, ProbeError>,
}

impl PlatformProbe for FakePlatformProbe {
    fn probe(&self) -> Result<SystemSnapshot, ProbeError> {
        self.result.clone()
    }
}

struct FakeCapabilityProvider {
    provider_name: &'static str,
    result: Result<ProbeReport, ProbeError>,
}

impl CapabilityProvider for FakeCapabilityProvider {
    fn name(&self) -> &str {
        self.provider_name
    }

    fn collect(&self) -> Result<ProbeReport, ProbeError> {
        self.result.clone()
    }
}

fn identity(family: PlatformFamily) -> PlatformIdentity {
    let distribution = if family == PlatformFamily::Linux {
        Some(
            endfield_unix_domain::DistributionIdentity::new("synthetic-linux", None, None).unwrap(),
        )
    } else {
        None
    };
    PlatformIdentity::new(
        family,
        OsIdentity::new("synthetic OS", None).unwrap(),
        Architecture::parse("x86_64").unwrap(),
        None,
        distribution,
    )
    .unwrap()
}

#[test]
fn fake_platform_probe_returns_a_synthetic_snapshot() {
    let expected = desktop_fixture();
    let probe = FakePlatformProbe {
        result: Ok(expected.clone()),
    };
    assert_eq!(probe.probe().unwrap(), expected);
}

#[test]
fn fake_platform_probe_returns_typed_total_failure() {
    let probe = FakePlatformProbe {
        result: Err(ProbeError::new(
            "synthetic-provider",
            ProbeErrorKind::TemporaryFailure,
            Some("recoverable synthetic failure".to_owned()),
        )
        .unwrap()),
    };
    assert_eq!(
        probe.probe().unwrap_err().kind(),
        ProbeErrorKind::TemporaryFailure
    );
}

#[test]
fn one_subsystem_failure_does_not_erase_successful_provider_data() {
    let mut probe = CompositePlatformProbe::new(identity(PlatformFamily::Linux));
    probe.add_provider(FakeCapabilityProvider {
        provider_name: "synthetic-thermal-provider",
        result: Err(ProbeError::new(
            "synthetic-thermal-provider",
            ProbeErrorKind::PermissionDenied,
            Some("synthetic sensor access denied".to_owned()),
        )
        .unwrap()),
    });
    probe.add_provider(FakeCapabilityProvider {
        provider_name: "synthetic-inventory-provider",
        result: Ok(report(&desktop_fixture())),
    });

    let snapshot = probe.probe().unwrap();
    assert!(snapshot.cpu.is_some());
    assert_eq!(snapshot.gpus.len(), 1);
    let diagnostic = snapshot
        .diagnostics
        .iter()
        .find(|item| item.category == DiagnosticCategory::PermissionDenied)
        .unwrap();
    assert_eq!(
        diagnostic.provider.as_deref(),
        Some("synthetic-thermal-provider")
    );
    snapshot.validate().unwrap();
}

#[test]
fn a_valid_partial_provider_report_is_successful() {
    let partial = incomplete_fixture();
    let mut probe = CompositePlatformProbe::new(identity(PlatformFamily::FreeBsd));
    probe.add_provider(FakeCapabilityProvider {
        provider_name: "synthetic-partial-provider",
        result: Ok(report(&partial)),
    });

    let snapshot = probe.probe().unwrap();
    assert_eq!(snapshot.platform.family, PlatformFamily::FreeBsd);
    assert!(snapshot.cpu.is_none());
    assert!(snapshot.gpus.is_empty());
    assert_eq!(
        snapshot.diagnostics[0].category,
        DiagnosticCategory::TemporaryFailure
    );
}

#[test]
fn total_provider_failure_is_distinct_from_partial_success() {
    let mut empty = CompositePlatformProbe::new(identity(PlatformFamily::Linux));
    assert_eq!(
        empty.probe().unwrap_err().kind(),
        ProbeErrorKind::NoProviders
    );

    empty.add_provider(FakeCapabilityProvider {
        provider_name: "synthetic-failure",
        result: Err(
            ProbeError::new("synthetic-failure", ProbeErrorKind::MissingProvider, None).unwrap(),
        ),
    });
    assert_eq!(
        empty.probe().unwrap_err().kind(),
        ProbeErrorKind::AllProvidersFailed
    );
}

#[test]
fn platform_families_keep_their_abi_identity_without_claiming_compatibility() {
    for family in [
        PlatformFamily::Linux,
        PlatformFamily::MacOs,
        PlatformFamily::FreeBsd,
    ] {
        let snapshot = SystemSnapshot::new(identity(family));
        snapshot.validate().unwrap();
        assert_eq!(snapshot.platform.family, family);
    }

    let mut invalid = SystemSnapshot::new(identity(PlatformFamily::MacOs));
    invalid.platform.distribution = Some(
        endfield_unix_domain::DistributionIdentity::new("not-a-macos-distro", None, None).unwrap(),
    );
    assert!(invalid.validate().is_err());
}

#[test]
fn missing_provider_is_distinct_from_known_unsupported_capability() {
    let snapshot = incomplete_fixture();
    let gpu = snapshot
        .capabilities
        .iter()
        .find(|capability| capability.id == endfield_unix_domain::CapabilityId::GpuDevice)
        .unwrap();
    let thermal = snapshot
        .capabilities
        .iter()
        .find(|capability| capability.id == endfield_unix_domain::CapabilityId::CpuThermal)
        .unwrap();

    assert_eq!(
        gpu.availability,
        endfield_unix_domain::Availability::Unknown
    );
    assert_eq!(
        gpu.diagnostic.as_ref().unwrap().category,
        DiagnosticCategory::MissingProvider
    );
    assert_eq!(
        thermal.availability,
        endfield_unix_domain::Availability::Unavailable
    );
    assert_eq!(
        thermal.diagnostic.as_ref().unwrap().category,
        DiagnosticCategory::UnsupportedCapability
    );
}

#[test]
fn report_provenance_is_explicit_but_no_real_hardware_is_accessed() {
    let provenance = Provenance::new(
        ProvenanceId::new(20).unwrap(),
        "in-memory-test-provider",
        None,
    )
    .unwrap();
    assert_eq!(provenance.provider, "in-memory-test-provider");
    assert!(provenance.observed_at_unix_nanos.is_none());
    assert!(Provenance::new(ProvenanceId::new(21).unwrap(), "/home/user/procfs", None).is_err());
    assert!(ProbeError::new("provider with spaces", ProbeErrorKind::MalformedData, None).is_err());
}
