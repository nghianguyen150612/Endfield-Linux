//! Exact-scope evaluation of P021 compatibility evidence for a pinned runtime.
//!
//! P027 does not persist or create compatibility evidence. This API lets a
//! caller evaluate evidence it already holds while keeping the pin's stored
//! compatibility state at `NotVerified`. An assessment is not a launch token
//! and does not itself construct P021's `KnownGoodRuntimeReference`.

use endfield_unix_runtime::{
    CompatibilityAssessment, CompatibilityEvidence, CompatibilityScope, DigestAlgorithm,
    RuntimeIdentity, RuntimeLineage,
};

use crate::PinnedRuntimeReference;

/// Result of applying P021 evidence to an exact pinned runtime and target scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompatibilityEvidenceEvaluation {
    /// The caller supplied no evidence.
    NotProvided,
    /// The observed P021 identity does not prove the exact stored pin identity.
    PinnedIdentityMismatch,
    /// The requested target or evidence has a different exact P021 scope.
    ScopeMismatch,
    /// P021 rejected the evidence set, for example because IDs are duplicated.
    InvalidEvidence,
    /// An assessment of only evidence with the exact requested scope.
    Assessed(CompatibilityAssessment),
}

/// Evaluate P021 evidence without broadening its runtime or target scope.
///
/// Managed references must match the observed family, lineage, provider,
/// build label, and SHA-256 artifact identity. P022 external references do not
/// contain a complete artifact digest, so a P021 build identity cannot be
/// bound to them here; their evidence remains unassessed. Evidence with a
/// different runtime build, adapter, platform, architecture, game build,
/// prefix configuration, driver requirement, or environment restriction is
/// excluded. If nothing matches exactly, this returns `ScopeMismatch`.
pub fn evaluate_pinned_compatibility(
    pinned: &PinnedRuntimeReference,
    observed: &RuntimeIdentity,
    intended_scope: &CompatibilityScope,
    evidence: &[CompatibilityEvidence],
) -> CompatibilityEvidenceEvaluation {
    if evidence.is_empty() {
        return CompatibilityEvidenceEvaluation::NotProvided;
    }

    let observed_build = observed.build_identity();
    if intended_scope.runtime_build() != &observed_build
        || !pinned_identity_matches(pinned, observed)
    {
        return CompatibilityEvidenceEvaluation::PinnedIdentityMismatch;
    }

    let exact: Vec<_> = evidence
        .iter()
        .filter(|item| item.scope() == intended_scope)
        .cloned()
        .collect();
    if exact.is_empty() {
        return CompatibilityEvidenceEvaluation::ScopeMismatch;
    }

    match CompatibilityAssessment::from_evidence(intended_scope.clone(), exact) {
        Ok(assessment) => CompatibilityEvidenceEvaluation::Assessed(assessment),
        Err(_) => CompatibilityEvidenceEvaluation::InvalidEvidence,
    }
}

fn pinned_identity_matches(pinned: &PinnedRuntimeReference, observed: &RuntimeIdentity) -> bool {
    let build = observed.build_identity();
    match pinned {
        PinnedRuntimeReference::Managed(reference) => {
            let Some(digest) = build.integrity_identity() else {
                return false;
            };
            reference.family == build.family().as_str()
                && reference.lineage == lineage_label(build.lineage())
                && reference.provider == build.provider_id().as_str()
                && reference.build == build.version().as_str()
                && matches!(digest.algorithm(), DigestAlgorithm::Sha256)
                && reference.artifact_sha256 == digest.hexadecimal()
        }
        // The P022 structural snapshot is deliberately not treated as a
        // complete artifact digest or P021 integrity identity.
        PinnedRuntimeReference::External(_) => false,
    }
}

fn lineage_label(lineage: &RuntimeLineage) -> String {
    match lineage {
        RuntimeLineage::OtherVerifiedDerivative(id) => {
            format!("other-verified-derivative:{}", id.as_str())
        }
        other => other.as_str().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AdapterPreference, CompatibilityStatus, ManagedRuntimeReference, PinnedRuntimeReference,
        Provenance, RuntimePin, RuntimePinScope,
    };
    use endfield_unix_runtime::{
        CompatibilityEvidenceResult, CompatibilityEvidenceStrength, CompatibilityTestProvenance,
        Digest, EvidenceId, GameBuildId, Identifier, ProcessArchitecture, ProviderId,
        RuntimeAdapterKind, RuntimeFamily, RuntimeLineage, RuntimeOwnership, RuntimePlatform,
        RuntimeRequirements, RuntimeVersion,
    };

    fn machine() -> crate::MachineId {
        crate::MachineId::from_digest("1".repeat(64)).expect("machine ID")
    }

    fn managed(build: &str, artifact: &str) -> PinnedRuntimeReference {
        PinnedRuntimeReference::Managed(ManagedRuntimeReference {
            installation_id: format!("proton-{build}"),
            family: "proton".into(),
            lineage: "ge-proton".into(),
            provider: "fixture-provider".into(),
            project: "fixture-project".into(),
            build: build.into(),
            artifact_sha256: artifact.into(),
            manifest_sha256: "b".repeat(64),
            generation: 1,
            ownership_record_sha256: "c".repeat(64),
            trust: Provenance::UserPinnedLimitedTrust,
        })
    }

    fn identity(build: &str, artifact: &str) -> RuntimeIdentity {
        RuntimeIdentity::new(
            RuntimeFamily::Proton,
            RuntimeLineage::GeProton,
            ProviderId::new("fixture-provider").expect("provider"),
            RuntimeVersion::new(build).expect("version"),
            None,
            RuntimeRequirements::new(
                [RuntimePlatform::Linux, RuntimePlatform::Windows],
                [ProcessArchitecture::X86_64, ProcessArchitecture::X86],
            )
            .expect("requirements"),
            None,
            Some(Digest::new(DigestAlgorithm::Sha256, artifact).expect("digest")),
            RuntimeOwnership::EndfieldManaged,
        )
        .expect("runtime identity")
    }

    fn scope(
        identity: &RuntimeIdentity,
        architecture: ProcessArchitecture,
        game_build: Option<GameBuildId>,
    ) -> CompatibilityScope {
        scope_on(identity, RuntimePlatform::Linux, architecture, game_build)
    }

    fn scope_on(
        identity: &RuntimeIdentity,
        platform: RuntimePlatform,
        architecture: ProcessArchitecture,
        game_build: Option<GameBuildId>,
    ) -> CompatibilityScope {
        CompatibilityScope::new(
            identity.build_identity(),
            RuntimeAdapterKind::UmuProton,
            platform,
            architecture,
            Some(architecture),
            None,
            vec![],
            game_build,
            None,
            vec![],
        )
        .expect("compatibility scope")
    }

    fn verified_evidence(scope: &CompatibilityScope, id: &str) -> CompatibilityEvidence {
        let provenance = CompatibilityTestProvenance::new(
            Identifier::new(format!("test-{id}")).expect("test ID"),
            scope.host_platform(),
            scope.process_architecture(),
            1_780_000_000,
            Identifier::new("synthetic-method").expect("method ID"),
            "Synthetic model test only; no runtime or game was executed.",
        )
        .expect("test provenance");
        CompatibilityEvidence::new(
            EvidenceId::new(id).expect("evidence ID"),
            scope.clone(),
            CompatibilityEvidenceStrength::VerifiedCompatibilityTest,
            CompatibilityEvidenceResult::Pass,
            None,
            Some(provenance),
        )
        .expect("compatibility evidence")
    }

    fn pin(runtime: PinnedRuntimeReference) -> PinnedRuntimeReference {
        let _ = RuntimePin::new(
            "pin-fixture",
            RuntimePinScope::Machine { machine: machine() },
            runtime.clone(),
            Some(AdapterPreference::UmuProton),
            1,
        )
        .expect("valid pin");
        runtime
    }

    #[test]
    fn exact_p021_evidence_is_assessed_for_the_exact_managed_build() {
        let artifact = "a".repeat(64);
        let pinned = pin(managed("GE-Proton10-26", &artifact));
        let observed = identity("GE-Proton10-26", &artifact);
        let target = scope(&observed, ProcessArchitecture::X86_64, None);
        let evidence = verified_evidence(&target, "evidence-exact");

        let result = evaluate_pinned_compatibility(&pinned, &observed, &target, &[evidence]);
        let CompatibilityEvidenceEvaluation::Assessed(assessment) = result else {
            panic!("exact evidence should be assessed");
        };
        assert_eq!(
            assessment.state(),
            endfield_unix_runtime::CompatibilityState::SupportedByVerifiedEvidence
        );
        assert_eq!(assessment.scope(), &target);
        // The stored pin still carries no automatic compatibility promotion.
        assert_eq!(
            RuntimePin::new(
                "pin-status",
                RuntimePinScope::Machine { machine: machine() },
                pinned,
                Some(AdapterPreference::UmuProton),
                2,
            )
            .expect("valid pin")
            .compatibility,
            CompatibilityStatus::NotVerified
        );
    }

    #[test]
    fn evidence_for_another_runtime_build_is_rejected() {
        let artifact_a = "a".repeat(64);
        let artifact_b = "d".repeat(64);
        let pinned = pin(managed("GE-Proton10-26", &artifact_a));
        let observed = identity("GE-Proton10-26", &artifact_a);
        let other = identity("GE-Proton10-27", &artifact_b);
        let target = scope(&observed, ProcessArchitecture::X86_64, None);
        let evidence = verified_evidence(
            &scope(&other, ProcessArchitecture::X86_64, None),
            "evidence-other-build",
        );

        assert_eq!(
            evaluate_pinned_compatibility(&pinned, &observed, &target, &[evidence]),
            CompatibilityEvidenceEvaluation::ScopeMismatch
        );
    }

    #[test]
    fn evidence_for_another_architecture_or_game_build_is_rejected() {
        let artifact = "a".repeat(64);
        let pinned = pin(managed("GE-Proton10-26", &artifact));
        let observed = identity("GE-Proton10-26", &artifact);
        let target = scope(&observed, ProcessArchitecture::X86_64, None);
        let wrong_architecture = verified_evidence(
            &scope(&observed, ProcessArchitecture::X86, None),
            "evidence-other-arch",
        );
        let other_game = verified_evidence(
            &scope(
                &observed,
                ProcessArchitecture::X86_64,
                Some(GameBuildId::new("synthetic-game-build-2").expect("game build")),
            ),
            "evidence-other-game",
        );

        for evidence in [wrong_architecture, other_game] {
            assert_eq!(
                evaluate_pinned_compatibility(&pinned, &observed, &target, &[evidence],),
                CompatibilityEvidenceEvaluation::ScopeMismatch
            );
        }
    }

    #[test]
    fn evidence_for_another_host_platform_is_rejected() {
        let artifact = "a".repeat(64);
        let pinned = pin(managed("GE-Proton10-26", &artifact));
        let observed = identity("GE-Proton10-26", &artifact);
        let target = scope(&observed, ProcessArchitecture::X86_64, None);
        let windows_evidence = verified_evidence(
            &scope_on(
                &observed,
                RuntimePlatform::Windows,
                ProcessArchitecture::X86_64,
                None,
            ),
            "evidence-other-platform",
        );

        assert_eq!(
            evaluate_pinned_compatibility(&pinned, &observed, &target, &[windows_evidence]),
            CompatibilityEvidenceEvaluation::ScopeMismatch
        );
    }

    #[test]
    fn external_structural_identity_does_not_bind_p021_integrity_evidence() {
        let artifact = "a".repeat(64);
        let observed = identity("GE-Proton10-26", &artifact);
        let target = scope(&observed, ProcessArchitecture::X86_64, None);
        let evidence = verified_evidence(&target, "evidence-external");
        let external = PinnedRuntimeReference::External(crate::ExternalRuntimeReference {
            location: "/synthetic/runtime".into(),
            family: "proton".into(),
            lineage: "ge-proton".into(),
            lineage_basis: "structural".into(),
            provider: "fixture-provider".into(),
            sources: vec!["explicit-root".into()],
            build: Some("GE-Proton10-26".into()),
            architectures: vec!["x86_64".into()],
            structure: "synthetic".into(),
            ownership: "external-user-managed".into(),
            identity_sha256: "e".repeat(64),
            trust: Provenance::UnknownExternalSource,
        });
        assert_eq!(
            evaluate_pinned_compatibility(&external, &observed, &target, &[evidence]),
            CompatibilityEvidenceEvaluation::PinnedIdentityMismatch
        );
    }

    #[test]
    fn user_reported_evidence_is_not_promoted_to_a_verified_test() {
        let artifact = "a".repeat(64);
        let pinned = pin(managed("GE-Proton10-26", &artifact));
        let observed = identity("GE-Proton10-26", &artifact);
        let target = scope(&observed, ProcessArchitecture::X86_64, None);
        let report = CompatibilityEvidence::new(
            EvidenceId::new("evidence-user-report").expect("evidence ID"),
            target.clone(),
            CompatibilityEvidenceStrength::UserReportedObservation,
            CompatibilityEvidenceResult::Pass,
            Some(Identifier::new("user-report").expect("source ID")),
            None,
        )
        .expect("user report evidence");

        let CompatibilityEvidenceEvaluation::Assessed(assessment) =
            evaluate_pinned_compatibility(&pinned, &observed, &target, &[report])
        else {
            panic!("matching user evidence should remain inspectable");
        };
        assert_ne!(
            assessment.state(),
            endfield_unix_runtime::CompatibilityState::SupportedByVerifiedEvidence
        );
    }

    #[test]
    fn static_capability_pass_is_not_game_compatibility_verification() {
        let artifact = "a".repeat(64);
        let pinned = pin(managed("GE-Proton10-26", &artifact));
        let observed = identity("GE-Proton10-26", &artifact);
        let target = scope(&observed, ProcessArchitecture::X86_64, None);
        let static_evidence = CompatibilityEvidence::new(
            EvidenceId::new("evidence-static-capability").expect("evidence ID"),
            target.clone(),
            CompatibilityEvidenceStrength::StaticCapabilityAssessment,
            CompatibilityEvidenceResult::Pass,
            None,
            None,
        )
        .expect("static evidence");

        let CompatibilityEvidenceEvaluation::Assessed(assessment) =
            evaluate_pinned_compatibility(&pinned, &observed, &target, &[static_evidence])
        else {
            panic!("matching static evidence should remain inspectable");
        };
        assert_eq!(
            assessment.state(),
            endfield_unix_runtime::CompatibilityState::TechnicallyEligibleForTesting
        );
    }

    #[test]
    fn mismatched_pinned_artifact_identity_is_rejected() {
        let observed_artifact = "a".repeat(64);
        let pinned = pin(managed("GE-Proton10-26", &"d".repeat(64)));
        let observed = identity("GE-Proton10-26", &observed_artifact);
        let target = scope(&observed, ProcessArchitecture::X86_64, None);
        let evidence = verified_evidence(&target, "evidence-wrong-artifact");

        assert_eq!(
            evaluate_pinned_compatibility(&pinned, &observed, &target, &[evidence]),
            CompatibilityEvidenceEvaluation::PinnedIdentityMismatch
        );
    }
}
