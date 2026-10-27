mod common;

use endfield_unix_runtime::{
    CapabilityStatus, CompatibilityState, ContractError, DiscoveryCompleteness, DiscoveryQuery,
    DiscoveryReport, Identifier, NegotiationContext, NegotiationResult, ProcessArchitecture,
    ProviderAvailability, ProviderCapability, ProviderCapabilityKind, ProviderDeclaration,
    ProviderError, ProviderErrorKind, ProviderId, ProviderOperation, RequestedOperation,
    RuntimeAdapterKind, RuntimeCandidate, RuntimeFamily, RuntimeId, RuntimeLineage,
    RuntimeOwnership, RuntimePlatform, RuntimePresence, RuntimeProvider, RuntimeVersion,
    SourceVerificationState, UserAction, UserIntent, negotiate_capability,
};

struct EmptyDiscoveryProvider {
    declaration: ProviderDeclaration,
}

impl RuntimeProvider for EmptyDiscoveryProvider {
    fn declaration(&self) -> &ProviderDeclaration {
        &self.declaration
    }

    fn discover_impl(&self, _query: &DiscoveryQuery) -> Result<DiscoveryReport, ProviderError> {
        DiscoveryReport::new(
            self.declaration.provider_id().clone(),
            vec![],
            DiscoveryCompleteness::Complete,
        )
        .map_err(|_| {
            ProviderError::new(
                self.declaration.provider_id().identifier().clone(),
                ProviderOperation::Discover,
                ProviderErrorKind::InvalidMetadata,
            )
        })
    }
}

struct PermissionDeniedProvider {
    declaration: ProviderDeclaration,
}

impl RuntimeProvider for PermissionDeniedProvider {
    fn declaration(&self) -> &ProviderDeclaration {
        &self.declaration
    }

    fn discover_impl(&self, _query: &DiscoveryQuery) -> Result<DiscoveryReport, ProviderError> {
        Err(ProviderError::new(
            self.declaration.provider_id().identifier().clone(),
            ProviderOperation::Discover,
            ProviderErrorKind::PermissionDenied,
        ))
    }
}

struct DeclaredOnlyProvider {
    declaration: ProviderDeclaration,
}

impl RuntimeProvider for DeclaredOnlyProvider {
    fn declaration(&self) -> &ProviderDeclaration {
        &self.declaration
    }
}

fn declaration(
    availability: ProviderAvailability,
    capabilities: Vec<ProviderCapability>,
) -> ProviderDeclaration {
    ProviderDeclaration::new(
        ProviderId::new("fixture-provider").expect("provider"),
        availability,
        vec![RuntimeFamily::Wine, RuntimeFamily::Proton],
        capabilities,
    )
    .expect("provider declaration")
}

#[test]
fn validates_a_provider_declaration_and_keeps_installation_separate() {
    let provider = EmptyDiscoveryProvider {
        declaration: declaration(
            ProviderAvailability::Available,
            vec![ProviderCapability::new(
                ProviderCapabilityKind::DiscoverBuilds,
                CapabilityStatus::ImplementedByProvider,
            )],
        ),
    };
    let report = provider
        .discover(&DiscoveryQuery::new(vec![RuntimeFamily::Wine], false).expect("query"))
        .expect("empty discovery report");
    assert!(report.candidates().is_empty());
    assert_eq!(report.completeness(), &DiscoveryCompleteness::Complete);
}

#[test]
fn empty_capability_set_is_a_valid_non_claim() {
    let declaration = ProviderDeclaration::new(
        ProviderId::new("empty-provider").expect("provider"),
        ProviderAvailability::Unknown,
        vec![],
        vec![],
    )
    .expect("empty declaration");
    assert!(declaration.capabilities().is_empty());
    assert!(declaration.supported_families().is_empty());
}

#[test]
fn unsupported_and_unimplemented_operations_return_typed_errors() {
    let unsupported = EmptyDiscoveryProvider {
        declaration: declaration(
            ProviderAvailability::Available,
            vec![ProviderCapability::new(
                ProviderCapabilityKind::InspectIdentity,
                CapabilityStatus::Unsupported,
            )],
        ),
    };
    let candidate = RuntimeCandidate::new(
        RuntimeId::new("candidate").expect("runtime id"),
        ProviderId::new("fixture-provider").expect("provider"),
        RuntimeFamily::Wine,
        Some(RuntimeVersion::new("wine-10.0").expect("version")),
        RuntimeOwnership::ExternalUserManaged,
    );
    let unsupported_error = unsupported
        .inspect_identity(&candidate)
        .expect_err("unsupported operation");
    assert_eq!(unsupported_error.kind(), ProviderErrorKind::Unsupported);
    assert_eq!(
        unsupported_error.operation(),
        ProviderOperation::InspectIdentity
    );

    let declared_only = EmptyDiscoveryProvider {
        declaration: declaration(
            ProviderAvailability::Available,
            vec![ProviderCapability::new(
                ProviderCapabilityKind::InspectIdentity,
                CapabilityStatus::SupportedByContract,
            )],
        ),
    };
    assert_eq!(
        declared_only
            .inspect_identity(&candidate)
            .expect_err("default method is not implemented")
            .kind(),
        ProviderErrorKind::NotImplemented
    );
}

#[test]
fn provider_unavailability_is_distinct_from_unsupported_operations() {
    let provider = DeclaredOnlyProvider {
        declaration: declaration(
            ProviderAvailability::Unavailable,
            vec![ProviderCapability::new(
                ProviderCapabilityKind::DiscoverBuilds,
                CapabilityStatus::ImplementedByProvider,
            )],
        ),
    };
    assert_eq!(
        provider
            .discover(&DiscoveryQuery::new(vec![], true).expect("query"))
            .expect_err("unavailable provider")
            .kind(),
        ProviderErrorKind::ProviderUnavailable
    );
}

#[test]
fn provider_boundary_preserves_permission_denied_and_missing_metadata() {
    let permission_provider = PermissionDeniedProvider {
        declaration: declaration(
            ProviderAvailability::Available,
            vec![ProviderCapability::new(
                ProviderCapabilityKind::DiscoverBuilds,
                CapabilityStatus::ImplementedByProvider,
            )],
        ),
    };
    assert_eq!(
        permission_provider
            .discover(&DiscoveryQuery::new(vec![], true).expect("query"))
            .expect_err("denied by provider")
            .kind(),
        ProviderErrorKind::PermissionDenied
    );
    let missing = ProviderError::new(
        ProviderId::new("fixture-provider")
            .expect("provider")
            .identifier()
            .clone(),
        ProviderOperation::InspectIdentity,
        ProviderErrorKind::MissingMetadata,
    );
    assert_eq!(missing.kind(), ProviderErrorKind::MissingMetadata);
}

#[test]
fn rejects_impossible_capability_combinations() {
    assert!(matches!(
        ProviderDeclaration::new(
            ProviderId::new("updates-only").expect("provider"),
            ProviderAvailability::Available,
            vec![RuntimeFamily::Wine],
            vec![ProviderCapability::new(
                ProviderCapabilityKind::ManageUpdates,
                CapabilityStatus::ImplementedByProvider,
            )],
        ),
        Err(ContractError::InvalidValue { .. })
    ));
    assert!(
        ProviderDeclaration::new(
            ProviderId::new("umu-without-umu").expect("provider"),
            ProviderAvailability::Available,
            vec![RuntimeFamily::Proton],
            vec![
                ProviderCapability::new(
                    ProviderCapabilityKind::PrepareLaunchPlan,
                    CapabilityStatus::ImplementedByProvider,
                ),
                ProviderCapability::new(
                    ProviderCapabilityKind::LaunchWithAdapter(RuntimeAdapterKind::UmuProton),
                    CapabilityStatus::ImplementedByProvider,
                ),
            ],
        )
        .is_err()
    );
    assert!(
        ProviderDeclaration::new(
            ProviderId::new("verified-while-unavailable").expect("provider"),
            ProviderAvailability::Unavailable,
            vec![RuntimeFamily::Wine],
            vec![ProviderCapability::new(
                ProviderCapabilityKind::DiscoverBuilds,
                CapabilityStatus::VerifiedUsable,
            )],
        )
        .is_err()
    );
}

#[test]
fn partial_discovery_results_are_typed_and_bounded() {
    let provider_id = ProviderId::new("fixture-provider").expect("provider");
    let candidate = RuntimeCandidate::new(
        RuntimeId::new("wine-candidate").expect("runtime id"),
        provider_id.clone(),
        RuntimeFamily::Wine,
        None,
        RuntimeOwnership::ExternalUserManaged,
    );
    let report = DiscoveryReport::new(
        provider_id,
        vec![candidate],
        DiscoveryCompleteness::Partial {
            reason: Identifier::new("permission-limited-source").expect("reason"),
        },
    )
    .expect("partial result");
    assert_eq!(report.candidates().len(), 1);
    assert!(matches!(
        report.completeness(),
        DiscoveryCompleteness::Partial { .. }
    ));
}

#[test]
fn capability_negotiation_does_not_claim_a_runtime_is_installed() {
    let provider = declaration(
        ProviderAvailability::Available,
        vec![ProviderCapability::new(
            ProviderCapabilityKind::DiscoverBuilds,
            CapabilityStatus::ImplementedByProvider,
        )],
    );
    let result = negotiate_capability(
        Some(&provider),
        NegotiationContext::new(RequestedOperation::Discover, RuntimeFamily::Proton),
    );
    assert_eq!(result, NegotiationResult::AllowedByDeclaredContract);
}

#[test]
fn capability_negotiation_distinguishes_contract_support_from_implementation() {
    let provider = declaration(
        ProviderAvailability::Available,
        vec![ProviderCapability::new(
            ProviderCapabilityKind::DiscoverBuilds,
            CapabilityStatus::SupportedByContract,
        )],
    );
    assert_eq!(
        negotiate_capability(
            Some(&provider),
            NegotiationContext::new(RequestedOperation::Discover, RuntimeFamily::Wine),
        ),
        NegotiationResult::EligibleForFurtherVerification
    );
}

#[test]
fn negotiation_distinguishes_missing_provider_and_unknown_capability() {
    assert_eq!(
        negotiate_capability(
            None,
            NegotiationContext::new(RequestedOperation::Discover, RuntimeFamily::Wine),
        ),
        NegotiationResult::ProviderMissing
    );
    let provider = declaration(ProviderAvailability::Available, vec![]);
    assert_eq!(
        negotiate_capability(
            Some(&provider),
            NegotiationContext::new(RequestedOperation::Discover, RuntimeFamily::Wine),
        ),
        NegotiationResult::Unknown
    );
}

#[test]
fn wine_is_never_implicitly_substituted_for_proton_or_umu() {
    let wine = common::runtime_identity(
        RuntimeFamily::Wine,
        RuntimeLineage::UpstreamWine,
        "wine-provider",
        "wine-10.0",
        RuntimeOwnership::ExternalUserManaged,
        None,
        None,
    );
    assert!(
        endfield_unix_runtime::RuntimeAdapterSelection::new(
            RuntimeAdapterKind::UmuProton,
            &wine,
            None,
        )
        .is_err()
    );
    let proton = common::runtime_identity(
        RuntimeFamily::Proton,
        RuntimeLineage::GeProton,
        "proton-provider",
        "GE-Proton10-26",
        RuntimeOwnership::ExternalUserManaged,
        None,
        None,
    );
    assert!(
        endfield_unix_runtime::RuntimeAdapterSelection::new(
            RuntimeAdapterKind::ExternalWine,
            &proton,
            None,
        )
        .is_err()
    );
}

#[test]
fn discovery_family_requests_do_not_assume_a_provider_supports_all_runtimes() {
    let provider = ProviderDeclaration::new(
        ProviderId::new("wine-only").expect("provider"),
        ProviderAvailability::Available,
        vec![RuntimeFamily::Wine],
        vec![ProviderCapability::new(
            ProviderCapabilityKind::DiscoverBuilds,
            CapabilityStatus::ImplementedByProvider,
        )],
    )
    .expect("declaration");
    assert!(!provider.supports_family(RuntimeFamily::Proton));
    assert_eq!(
        negotiate_capability(
            Some(&provider),
            NegotiationContext::new(RequestedOperation::Discover, RuntimeFamily::Proton),
        ),
        NegotiationResult::IncompatibleRequest
    );
}

#[test]
fn launch_negotiation_requires_installation_adapter_target_and_explicit_approval() {
    let provider = ProviderDeclaration::new(
        ProviderId::new("proton-provider").expect("provider"),
        ProviderAvailability::Available,
        vec![RuntimeFamily::Proton],
        vec![
            ProviderCapability::new(
                ProviderCapabilityKind::PrepareLaunchPlan,
                CapabilityStatus::ImplementedByProvider,
            ),
            ProviderCapability::new(
                ProviderCapabilityKind::LaunchWithAdapter(RuntimeAdapterKind::UmuProton),
                CapabilityStatus::ImplementedByProvider,
            ),
            ProviderCapability::new(
                ProviderCapabilityKind::IntegrateWithUmu,
                CapabilityStatus::ImplementedByProvider,
            ),
            ProviderCapability::new(
                ProviderCapabilityKind::SupportsArchitecture(ProcessArchitecture::X86_64),
                CapabilityStatus::VerifiedUsable,
            ),
            ProviderCapability::new(
                ProviderCapabilityKind::SupportsPlatform(RuntimePlatform::Linux),
                CapabilityStatus::VerifiedUsable,
            ),
        ],
    )
    .expect("provider declaration");
    let resource = Identifier::new("proton-build").expect("resource");
    let context = NegotiationContext::new(RequestedOperation::PrepareLaunch, RuntimeFamily::Proton)
        .with_adapter(RuntimeAdapterKind::UmuProton)
        .with_presence(RuntimePresence::Installed)
        .with_verification(
            SourceVerificationState::ArtifactIntegrityVerified,
            CompatibilityState::SupportedByVerifiedEvidence,
        )
        .with_target(ProcessArchitecture::X86_64, RuntimePlatform::Linux)
        .with_management_context(
            RuntimeOwnership::OtherLauncherManaged(ProviderId::new("steam").expect("launcher")),
            resource.clone(),
            UserIntent::ExplicitlyApproved {
                action: UserAction::PrepareLaunch,
                resource_id: resource,
            },
        );
    assert_eq!(
        negotiate_capability(Some(&provider), context.clone()),
        NegotiationResult::AllowedByDeclaredContract
    );
    assert_eq!(
        negotiate_capability(
            Some(&provider),
            context.with_presence(RuntimePresence::NotInstalled),
        ),
        NegotiationResult::IncompatibleRequest
    );
}

#[test]
fn missing_adapter_capability_does_not_fall_back_to_another_adapter() {
    let provider = declaration(
        ProviderAvailability::Available,
        vec![ProviderCapability::new(
            ProviderCapabilityKind::PrepareLaunchPlan,
            CapabilityStatus::ImplementedByProvider,
        )],
    );
    let context = NegotiationContext::new(RequestedOperation::PrepareLaunch, RuntimeFamily::Proton)
        .with_adapter(RuntimeAdapterKind::VerifiedDirectProton)
        .with_presence(RuntimePresence::Installed)
        .with_verification(
            SourceVerificationState::ArtifactIntegrityVerified,
            CompatibilityState::SupportedByVerifiedEvidence,
        );
    assert_eq!(
        negotiate_capability(Some(&provider), context),
        NegotiationResult::Unsupported
    );
}
