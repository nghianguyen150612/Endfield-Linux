use crate::compatibility::CompatibilityEvidence;
use crate::errors::ContractError;
use crate::identity::{RuntimeBuildIdentity, RuntimeFamily, RuntimeIdentity, RuntimeOwnership};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeAdapterKind {
    UmuProton,
    VerifiedDirectProton,
    ManagedWine,
    ExternalWine,
}

impl RuntimeAdapterKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UmuProton => "umu-proton",
            Self::VerifiedDirectProton => "verified-direct-proton",
            Self::ManagedWine => "managed-wine",
            Self::ExternalWine => "external-wine",
        }
    }

    pub const fn family(self) -> RuntimeFamily {
        match self {
            Self::UmuProton | Self::VerifiedDirectProton => RuntimeFamily::Proton,
            Self::ManagedWine | Self::ExternalWine => RuntimeFamily::Wine,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeAdapterSelection {
    kind: RuntimeAdapterKind,
    build: RuntimeBuildIdentity,
    evidence_id: Option<crate::identity::EvidenceId>,
}

impl RuntimeAdapterSelection {
    pub fn new(
        kind: RuntimeAdapterKind,
        identity: &RuntimeIdentity,
        direct_proton_evidence: Option<&CompatibilityEvidence>,
    ) -> Result<Self, ContractError> {
        if kind.family() != identity.family() {
            return Err(ContractError::InvalidAdapterCombination);
        }
        match kind {
            RuntimeAdapterKind::UmuProton => {
                if direct_proton_evidence.is_some() {
                    return Err(ContractError::InvalidAdapterCombination);
                }
            }
            RuntimeAdapterKind::VerifiedDirectProton => {
                let evidence = direct_proton_evidence
                    .filter(|evidence| evidence.verifies_direct_proton(identity))
                    .ok_or(ContractError::MissingVerificationEvidence)?;
                return Ok(Self {
                    kind,
                    build: identity.build_identity(),
                    evidence_id: Some(evidence.id().clone()),
                });
            }
            RuntimeAdapterKind::ManagedWine => {
                if !matches!(identity.ownership(), RuntimeOwnership::EndfieldManaged)
                    || direct_proton_evidence.is_some()
                {
                    return Err(ContractError::InvalidAdapterCombination);
                }
            }
            RuntimeAdapterKind::ExternalWine => {
                if !identity.ownership().is_explicitly_external()
                    || direct_proton_evidence.is_some()
                {
                    return Err(ContractError::InvalidAdapterCombination);
                }
            }
        }
        Ok(Self {
            kind,
            build: identity.build_identity(),
            evidence_id: None,
        })
    }

    pub const fn kind(&self) -> RuntimeAdapterKind {
        self.kind
    }

    pub const fn build(&self) -> &RuntimeBuildIdentity {
        &self.build
    }

    pub const fn evidence_id(&self) -> Option<&crate::identity::EvidenceId> {
        self.evidence_id.as_ref()
    }

    pub fn applies_to(&self, identity: &RuntimeIdentity) -> bool {
        self.build == identity.build_identity()
    }
}
