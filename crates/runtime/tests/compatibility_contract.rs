mod common;

use endfield_unix_runtime::{
    CompatibilityAssessment, CompatibilityEvidence, CompatibilityEvidenceResult,
    CompatibilityEvidenceStrength, CompatibilityScope, CompatibilityState,
    CompatibilityTestProvenance, ContractError, EvidenceId, GraphicsApi, Identifier,
    KnownGoodRuntimeReference, ProcessArchitecture, RuntimeAdapterKind, RuntimeFamily,
    RuntimeLineage, RuntimeOwnership, RuntimePlatform, StaticCompatibilityRequest,
    assess_static_compatibility,
};

#[test]
fn empty_evidence_is_not_evaluated() {
    let runtime = common::wine_identity();
    let scope = common::compatibility_scope(&runtime, RuntimeAdapterKind::ManagedWine, None);
    let assessment = CompatibilityAssessment::from_evidence(scope, vec![]).expect("assessment");
    assert_eq!(assessment.state(), CompatibilityState::NotEvaluated);
}

#[test]
fn static_constraints_only_establish_eligibility_for_testing() {
    let evidence = CompatibilityEvidence::new(
        EvidenceId::new("static-check").expect("evidence"),
        common::compatibility_scope(
            &common::wine_identity(),
            RuntimeAdapterKind::ManagedWine,
            None,
        ),
        CompatibilityEvidenceStrength::StaticCapabilityAssessment,
        CompatibilityEvidenceResult::Pass,
        None,
        None,
    )
    .expect("static evidence");
    let assessment = common::compatibility_assessment(vec![evidence]);
    assert_eq!(
        assessment.state(),
        CompatibilityState::TechnicallyEligibleForTesting
    );
}

#[test]
fn exact_verified_evidence_can_support_only_its_scoped_build() {
    let runtime = common::wine_identity();
    let evidence = common::verified_compatibility_evidence(
        &runtime,
        RuntimeAdapterKind::ManagedWine,
        "verified-fixture",
        None,
    );
    let assessment = common::compatibility_assessment(vec![evidence]);
    assert_eq!(
        assessment.state(),
        CompatibilityState::SupportedByVerifiedEvidence
    );
    assert_eq!(
        assessment.scope().runtime_build(),
        &runtime.build_identity()
    );
}

#[test]
fn contradictory_evidence_is_preserved_as_conflicting() {
    let runtime = common::wine_identity();
    let scope = common::compatibility_scope(&runtime, RuntimeAdapterKind::ManagedWine, None);
    let pass = verified_evidence(
        "verified-pass",
        scope.clone(),
        CompatibilityEvidenceResult::Pass,
    );
    let fail = verified_evidence(
        "verified-fail",
        scope.clone(),
        CompatibilityEvidenceResult::Fail,
    );
    let assessment =
        CompatibilityAssessment::from_evidence(scope, vec![pass, fail]).expect("assessment");
    assert_eq!(assessment.state(), CompatibilityState::ConflictingEvidence);
}

#[test]
fn wrong_architecture_is_ineligible_without_claiming_verified_incompatibility() {
    let request = StaticCompatibilityRequest::new(
        RuntimeFamily::Wine,
        RuntimeAdapterKind::ManagedWine,
        RuntimePlatform::Linux,
        ProcessArchitecture::Aarch64,
        None,
        None,
        vec![RuntimePlatform::Linux],
        vec![ProcessArchitecture::X86_64],
    )
    .expect("request");
    assert_eq!(
        assess_static_compatibility(&request),
        CompatibilityState::IneligibleByStaticConstraints
    );
}

#[test]
fn wrong_runtime_family_and_adapter_are_rejected() {
    let request = StaticCompatibilityRequest::new(
        RuntimeFamily::Wine,
        RuntimeAdapterKind::UmuProton,
        RuntimePlatform::Linux,
        ProcessArchitecture::X86_64,
        None,
        None,
        vec![],
        vec![],
    )
    .expect("request");
    assert_eq!(
        assess_static_compatibility(&request),
        CompatibilityState::InsufficientEvidence
    );
    assert!(matches!(
        CompatibilityScope::new(
            common::wine_identity().build_identity(),
            RuntimeAdapterKind::VerifiedDirectProton,
            RuntimePlatform::Linux,
            ProcessArchitecture::X86_64,
            None,
            None,
            vec![],
            None,
            None,
            vec![],
        ),
        Err(ContractError::InvalidCompatibilityEvidence)
    ));
}

#[test]
fn absent_graphics_evidence_is_not_inferred_from_a_vulkan_requirement() {
    let request = StaticCompatibilityRequest::new(
        RuntimeFamily::Proton,
        RuntimeAdapterKind::UmuProton,
        RuntimePlatform::Linux,
        ProcessArchitecture::X86_64,
        Some(GraphicsApi::Vulkan),
        None,
        vec![RuntimePlatform::Linux],
        vec![ProcessArchitecture::X86_64],
    )
    .expect("request");
    assert_eq!(
        assess_static_compatibility(&request),
        CompatibilityState::InsufficientEvidence
    );
}

#[test]
fn unknown_architecture_and_platform_do_not_become_eligible() {
    let request = StaticCompatibilityRequest::new(
        RuntimeFamily::Wine,
        RuntimeAdapterKind::ManagedWine,
        RuntimePlatform::Unknown,
        ProcessArchitecture::Unknown,
        None,
        None,
        vec![],
        vec![],
    )
    .expect("request");
    assert_eq!(
        assess_static_compatibility(&request),
        CompatibilityState::InsufficientEvidence
    );
}

#[test]
fn evidence_for_another_exact_build_is_not_reused() {
    let first = common::wine_identity();
    let second = common::runtime_identity(
        RuntimeFamily::Wine,
        RuntimeLineage::UpstreamWine,
        "fixture-wine-provider",
        "wine-10.0",
        RuntimeOwnership::EndfieldManaged,
        Some(common::sha256('b')),
        None,
    );
    let evidence = common::verified_compatibility_evidence(
        &first,
        RuntimeAdapterKind::ManagedWine,
        "evidence-for-first",
        None,
    );
    let second_scope = common::compatibility_scope(&second, RuntimeAdapterKind::ManagedWine, None);
    assert!(matches!(
        CompatibilityAssessment::from_evidence(second_scope, vec![evidence]),
        Err(ContractError::InvalidCompatibilityEvidence)
    ));
}

#[test]
fn user_report_and_documentation_are_not_promoted_to_verified_results() {
    for strength in [
        CompatibilityEvidenceStrength::ProviderDocumentation,
        CompatibilityEvidenceStrength::UserReportedObservation,
        CompatibilityEvidenceStrength::ReproducibleLocalTest,
    ] {
        let runtime = common::wine_identity();
        let scope = common::compatibility_scope(&runtime, RuntimeAdapterKind::ManagedWine, None);
        let provenance = (strength == CompatibilityEvidenceStrength::ReproducibleLocalTest)
            .then(|| test_provenance("local-test"));
        let source = (strength != CompatibilityEvidenceStrength::ReproducibleLocalTest)
            .then(|| Identifier::new("source-note").expect("source"));
        let evidence = CompatibilityEvidence::new(
            EvidenceId::new(format!("weak-{strength:?}")).expect("evidence"),
            scope.clone(),
            strength,
            CompatibilityEvidenceResult::Pass,
            source,
            provenance,
        )
        .expect("weaker evidence");
        let assessment =
            CompatibilityAssessment::from_evidence(scope, vec![evidence]).expect("assessment");
        assert_ne!(
            assessment.state(),
            CompatibilityState::SupportedByVerifiedEvidence
        );
    }
}

#[test]
fn no_benchmark_or_fps_claim_is_part_of_the_evidence_model() {
    let runtime = common::wine_identity();
    let evidence = common::verified_compatibility_evidence(
        &runtime,
        RuntimeAdapterKind::ManagedWine,
        "safe-summary",
        None,
    );
    assert!(
        !evidence
            .provenance()
            .expect("test provenance")
            .safe_result_summary()
            .contains("FPS")
    );
    assert!(evidence.provenance().is_some());
}

#[test]
fn known_good_reference_requires_exact_verified_runtime_and_game_scope() {
    let runtime = common::wine_identity();
    let game_build = common::fixture_game_build();
    let evidence = common::verified_compatibility_evidence(
        &runtime,
        RuntimeAdapterKind::ManagedWine,
        "known-good-evidence",
        Some(game_build.clone()),
    );
    let assessment = common::compatibility_assessment(vec![evidence.clone()]);
    let known_good = KnownGoodRuntimeReference::new(
        &runtime,
        game_build.clone(),
        common::fixture_profile(),
        &evidence,
        &assessment,
    )
    .expect("known-good reference");
    assert_eq!(known_good.target_game_build(), &game_build);
    assert_eq!(known_good.runtime(), &runtime.build_identity());

    let unverified_identity = common::runtime_identity(
        RuntimeFamily::Wine,
        RuntimeLineage::UpstreamWine,
        "fixture-wine-provider",
        "wine-10.0",
        RuntimeOwnership::EndfieldManaged,
        Some(common::sha256('a')),
        None,
    );
    assert!(
        KnownGoodRuntimeReference::new(
            &unverified_identity,
            game_build,
            common::fixture_profile(),
            &evidence,
            &assessment,
        )
        .is_err()
    );
}

#[test]
fn direct_proton_adapter_requires_exact_verified_build_evidence() {
    let proton = common::runtime_identity(
        RuntimeFamily::Proton,
        RuntimeLineage::GeProton,
        "proton-provider",
        "GE-Proton10-26",
        RuntimeOwnership::OtherLauncherManaged(
            endfield_unix_runtime::ProviderId::new("launcher").expect("provider"),
        ),
        Some(common::sha256('8')),
        None,
    );
    assert!(
        endfield_unix_runtime::RuntimeAdapterSelection::new(
            RuntimeAdapterKind::VerifiedDirectProton,
            &proton,
            None,
        )
        .is_err()
    );
    let evidence = common::verified_compatibility_evidence(
        &proton,
        RuntimeAdapterKind::VerifiedDirectProton,
        "direct-proton-evidence",
        None,
    );
    let selection = endfield_unix_runtime::RuntimeAdapterSelection::new(
        RuntimeAdapterKind::VerifiedDirectProton,
        &proton,
        Some(&evidence),
    )
    .expect("specific build evidence");
    assert_eq!(selection.kind(), RuntimeAdapterKind::VerifiedDirectProton);
    let other_build = common::runtime_identity(
        RuntimeFamily::Proton,
        RuntimeLineage::GeProton,
        "proton-provider",
        "GE-Proton10-26",
        RuntimeOwnership::OtherLauncherManaged(
            endfield_unix_runtime::ProviderId::new("launcher").expect("provider"),
        ),
        Some(common::sha256('9')),
        None,
    );
    assert!(
        endfield_unix_runtime::RuntimeAdapterSelection::new(
            RuntimeAdapterKind::VerifiedDirectProton,
            &other_build,
            Some(&evidence),
        )
        .is_err()
    );
}

fn verified_evidence(
    id: &str,
    scope: CompatibilityScope,
    result: CompatibilityEvidenceResult,
) -> CompatibilityEvidence {
    CompatibilityEvidence::new(
        EvidenceId::new(id).expect("evidence id"),
        scope,
        CompatibilityEvidenceStrength::VerifiedCompatibilityTest,
        result,
        None,
        Some(test_provenance(id)),
    )
    .expect("verified test fixture")
}

fn test_provenance(id: &str) -> CompatibilityTestProvenance {
    CompatibilityTestProvenance::new(
        Identifier::new(format!("test-{id}")).expect("test id"),
        RuntimePlatform::Linux,
        ProcessArchitecture::X86_64,
        1_780_000_000,
        Identifier::new("synthetic-test-method").expect("method"),
        "Synthetic fixture; no runtime was launched.",
    )
    .expect("provenance")
}
