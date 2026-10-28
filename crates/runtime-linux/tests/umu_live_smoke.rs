use endfield_unix_runtime::{
    CapabilityStatus as Status, CompatibilityState, NegotiationContext, NegotiationResult,
    ProviderAvailability, ProviderCapabilityKind as Kind, RequestedOperation, RuntimeAdapterKind,
    RuntimeFamily, RuntimePresence, SourceVerificationState, negotiate_capability,
};
use endfield_unix_runtime::{CapabilityStatus, ProviderCapabilityKind};
use endfield_unix_runtime_linux::umu::{UMU_PROVIDER_ID, UmuProvider};
use endfield_unix_runtime_linux::umu_discovery::UmuDiscoveryEnvironment;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn live_umu_probe_is_local_read_only_and_never_claims_execution_readiness() {
    let report = UmuProvider::default().scan();
    assert!(
        report.discovery.candidates.len()
            <= endfield_unix_runtime_linux::umu_discovery::MAX_UMU_CANDIDATES
    );
    assert!(report.discovery.issues.len() <= 128);
    assert_ne!(
        report.discovery.outcome(),
        endfield_unix_runtime_linux::RootOutcome::Skipped
    );
    let plan_status = report
        .declaration
        .capability_status(ProviderCapabilityKind::PrepareLaunchPlan)
        .unwrap();
    assert!(matches!(
        plan_status,
        CapabilityStatus::Unavailable
            | CapabilityStatus::Unknown
            | CapabilityStatus::ImplementedByProvider
    ));
    assert_ne!(plan_status, CapabilityStatus::VerifiedUsable);
    assert_eq!(
        report
            .declaration
            .capability_status(ProviderCapabilityKind::VerifySource),
        Some(CapabilityStatus::Unsupported)
    );
    assert_eq!(
        report
            .declaration
            .capability_status(ProviderCapabilityKind::VerifyIntegrity),
        Some(CapabilityStatus::Unsupported)
    );
    assert_eq!(
        report
            .declaration
            .capability_status(ProviderCapabilityKind::AssessCompatibility),
        Some(CapabilityStatus::Unsupported)
    );
    assert!(
        report
            .discovery
            .validated()
            .all(|candidate| candidate.revalidate())
    );
}

#[test]
fn p021_negotiation_keeps_umu_availability_separate_from_launch_readiness() {
    let base = std::env::temp_dir().join(format!("endfield-umu-capability-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join(".local/bin")).unwrap();
    let binary = base.join(".local/bin/umu-run");
    fs::write(&binary, b"synthetic executable; never executed").unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    let provider = UmuProvider::new(UmuDiscoveryEnvironment::isolated(Some(base.clone()), None));
    let report = provider.scan();
    assert_eq!(
        report.declaration.availability(),
        ProviderAvailability::Available
    );
    assert_eq!(
        report.declaration.capability_status(Kind::IntegrateWithUmu),
        Some(Status::ImplementedByProvider)
    );
    assert_ne!(
        report.declaration.capability_status(Kind::IntegrateWithUmu),
        Some(Status::VerifiedUsable)
    );
    let context = NegotiationContext::new(RequestedOperation::PrepareLaunch, RuntimeFamily::Proton)
        .with_adapter(RuntimeAdapterKind::UmuProton)
        .with_presence(RuntimePresence::Installed)
        .with_verification(
            SourceVerificationState::Unverified,
            CompatibilityState::NotEvaluated,
        );
    assert_eq!(
        negotiate_capability(Some(&report.declaration), context),
        NegotiationResult::InsufficientEvidence
    );

    let missing = UmuProvider::new(UmuDiscoveryEnvironment::isolated(None, None)).scan();
    assert_eq!(
        missing.declaration.availability(),
        ProviderAvailability::Unavailable
    );
    assert_eq!(
        missing
            .declaration
            .capability_status(Kind::PrepareLaunchPlan),
        Some(Status::Unavailable)
    );
    assert_eq!(missing.declaration.provider_id().as_str(), UMU_PROVIDER_ID);

    let truncated = UmuProvider::new(UmuDiscoveryEnvironment::isolated(
        None,
        Some(std::ffi::OsString::from("x".repeat(
            endfield_unix_runtime_linux::umu_discovery::MAX_UMU_PATH_BYTES + 1,
        ))),
    ))
    .scan();
    assert_eq!(
        truncated.declaration.availability(),
        ProviderAvailability::Unknown
    );
    assert_eq!(
        truncated
            .declaration
            .capability_status(Kind::PrepareLaunchPlan),
        Some(Status::Unknown)
    );
    let _ = fs::remove_dir_all(base);
}
