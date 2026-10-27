use crate::errors::ContractError;
use crate::identity::{Identifier, RuntimeOwnership};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagementAction {
    Install,
    Update,
    Remove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserAction {
    InstallRuntime,
    UpdateRuntime,
    RemoveRuntime,
    PrepareLaunch,
}

impl ManagementAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Update => "update",
            Self::Remove => "remove",
        }
    }

    pub const fn user_action(self) -> UserAction {
        match self {
            Self::Install => UserAction::InstallRuntime,
            Self::Update => UserAction::UpdateRuntime,
            Self::Remove => UserAction::RemoveRuntime,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManagementTarget {
    NewManagedLocation(Identifier),
    ExistingRuntime {
        resource_id: Identifier,
        ownership: RuntimeOwnership,
    },
}

impl ManagementTarget {
    pub const fn resource_id(&self) -> &Identifier {
        match self {
            Self::NewManagedLocation(resource_id) => resource_id,
            Self::ExistingRuntime { resource_id, .. } => resource_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserIntent {
    NotProvided,
    ExplicitlyApproved {
        action: UserAction,
        resource_id: Identifier,
    },
}

impl UserIntent {
    pub fn approves(&self, action: UserAction, resource_id: &Identifier) -> bool {
        matches!(
            self,
            Self::ExplicitlyApproved {
                action: approved_action,
                resource_id: approved_resource,
            } if *approved_action == action && approved_resource == resource_id
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagementDecision {
    AllowedByPolicy,
    RequiresExplicitApproval,
    DeniedExternalOwnership,
    DeniedUnknownOwnership,
    InvalidApprovalScope,
    InvalidActionForTarget,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeManagementPolicy;

impl RuntimeManagementPolicy {
    pub fn evaluate(
        &self,
        action: ManagementAction,
        target: &ManagementTarget,
        intent: &UserIntent,
    ) -> Result<ManagementDecision, ContractError> {
        let (eligible, external, unknown) = match (action, target) {
            (ManagementAction::Install, ManagementTarget::NewManagedLocation(_)) => {
                (true, false, false)
            }
            (ManagementAction::Install, ManagementTarget::ExistingRuntime { .. }) => {
                return Ok(ManagementDecision::InvalidActionForTarget);
            }
            (
                ManagementAction::Update | ManagementAction::Remove,
                ManagementTarget::ExistingRuntime { ownership, .. },
            ) => (
                ownership.is_endfield_managed(),
                ownership.is_explicitly_external(),
                matches!(ownership, RuntimeOwnership::Unknown),
            ),
            (
                ManagementAction::Update | ManagementAction::Remove,
                ManagementTarget::NewManagedLocation(_),
            ) => return Ok(ManagementDecision::InvalidActionForTarget),
        };
        if external {
            return Ok(ManagementDecision::DeniedExternalOwnership);
        }
        if unknown {
            return Ok(ManagementDecision::DeniedUnknownOwnership);
        }
        if !eligible {
            return Ok(ManagementDecision::DeniedUnknownOwnership);
        }
        if intent.approves(action.user_action(), target.resource_id()) {
            Ok(ManagementDecision::AllowedByPolicy)
        } else if matches!(intent, UserIntent::ExplicitlyApproved { .. }) {
            Ok(ManagementDecision::InvalidApprovalScope)
        } else {
            Ok(ManagementDecision::RequiresExplicitApproval)
        }
    }
}
