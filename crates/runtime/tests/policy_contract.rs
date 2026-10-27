mod common;

use endfield_unix_runtime::{
    Identifier, ManagementAction, ManagementDecision, ManagementTarget, ProviderId,
    RuntimeAdapterKind, RuntimeAdapterSelection, RuntimeManagementPolicy, RuntimeOwnership,
    UserAction, UserIntent,
};

#[test]
fn managed_runtime_update_requires_explicit_resource_scoped_intent() {
    let resource_id = Identifier::new("managed-runtime").expect("resource");
    let target = ManagementTarget::ExistingRuntime {
        resource_id: resource_id.clone(),
        ownership: RuntimeOwnership::EndfieldManaged,
    };
    let policy = RuntimeManagementPolicy;
    assert_eq!(
        policy
            .evaluate(ManagementAction::Update, &target, &UserIntent::NotProvided,)
            .expect("decision"),
        ManagementDecision::RequiresExplicitApproval
    );
    assert_eq!(
        policy
            .evaluate(
                ManagementAction::Update,
                &target,
                &UserIntent::ExplicitlyApproved {
                    action: UserAction::UpdateRuntime,
                    resource_id,
                },
            )
            .expect("decision"),
        ManagementDecision::AllowedByPolicy
    );
}

#[test]
fn an_approval_for_another_action_or_resource_does_not_authorize_mutation() {
    let target = ManagementTarget::ExistingRuntime {
        resource_id: Identifier::new("runtime-one").expect("resource"),
        ownership: RuntimeOwnership::EndfieldManaged,
    };
    assert_eq!(
        RuntimeManagementPolicy
            .evaluate(
                ManagementAction::Remove,
                &target,
                &UserIntent::ExplicitlyApproved {
                    action: UserAction::UpdateRuntime,
                    resource_id: Identifier::new("runtime-one").expect("resource"),
                },
            )
            .expect("decision"),
        ManagementDecision::InvalidApprovalScope
    );
    assert_eq!(
        RuntimeManagementPolicy
            .evaluate(
                ManagementAction::Remove,
                &target,
                &UserIntent::ExplicitlyApproved {
                    action: UserAction::RemoveRuntime,
                    resource_id: Identifier::new("runtime-two").expect("resource"),
                },
            )
            .expect("decision"),
        ManagementDecision::InvalidApprovalScope
    );
}

#[test]
fn external_system_and_other_launcher_runtimes_cannot_be_updated_or_removed() {
    for ownership in [
        RuntimeOwnership::ExternalUserManaged,
        RuntimeOwnership::DistributionManaged,
        RuntimeOwnership::OtherLauncherManaged(ProviderId::new("steam").expect("provider")),
    ] {
        let target = ManagementTarget::ExistingRuntime {
            resource_id: Identifier::new("external-runtime").expect("resource"),
            ownership,
        };
        for action in [ManagementAction::Update, ManagementAction::Remove] {
            assert_eq!(
                RuntimeManagementPolicy
                    .evaluate(action, &target, &UserIntent::NotProvided)
                    .expect("decision"),
                ManagementDecision::DeniedExternalOwnership
            );
        }
    }
}

#[test]
fn unknown_ownership_is_not_managed_by_default() {
    let target = ManagementTarget::ExistingRuntime {
        resource_id: Identifier::new("unknown-runtime").expect("resource"),
        ownership: RuntimeOwnership::Unknown,
    };
    let approved = UserIntent::ExplicitlyApproved {
        action: UserAction::RemoveRuntime,
        resource_id: Identifier::new("unknown-runtime").expect("resource"),
    };
    assert_eq!(
        RuntimeManagementPolicy
            .evaluate(ManagementAction::Remove, &target, &approved)
            .expect("decision"),
        ManagementDecision::DeniedUnknownOwnership
    );
}

#[test]
fn new_managed_installation_is_explicit_and_cannot_overwrite_a_runtime() {
    let target = ManagementTarget::NewManagedLocation(
        Identifier::new("new-runtime-target").expect("target"),
    );
    assert_eq!(
        RuntimeManagementPolicy
            .evaluate(ManagementAction::Install, &target, &UserIntent::NotProvided)
            .expect("decision"),
        ManagementDecision::RequiresExplicitApproval
    );
    assert_eq!(
        RuntimeManagementPolicy
            .evaluate(
                ManagementAction::Update,
                &target,
                &UserIntent::ExplicitlyApproved {
                    action: UserAction::UpdateRuntime,
                    resource_id: Identifier::new("new-runtime-target").expect("target"),
                },
            )
            .expect("decision"),
        ManagementDecision::InvalidActionForTarget
    );
}

#[test]
fn external_wine_adapter_does_not_transfer_runtime_ownership() {
    let identity = common::runtime_identity(
        endfield_unix_runtime::RuntimeFamily::Wine,
        endfield_unix_runtime::RuntimeLineage::UpstreamWine,
        "external-provider",
        "wine-10.0",
        RuntimeOwnership::DistributionManaged,
        None,
        None,
    );
    let _adapter = RuntimeAdapterSelection::new(RuntimeAdapterKind::ExternalWine, &identity, None)
        .expect("external wine adapter");
    assert_eq!(
        RuntimeManagementPolicy
            .evaluate(
                ManagementAction::Remove,
                &ManagementTarget::ExistingRuntime {
                    resource_id: Identifier::new("distro-wine").expect("resource"),
                    ownership: identity.ownership().clone(),
                },
                &UserIntent::NotProvided,
            )
            .expect("decision"),
        ManagementDecision::DeniedExternalOwnership
    );
}
