//! UMU adapter discovery and P021 capability declaration.

use crate::RootOutcome;
use crate::umu_discovery::{UmuDiscoveryEnvironment, UmuDiscoveryReport, discover_umu};
use endfield_unix_runtime::{
    CapabilityStatus, ProviderAvailability, ProviderCapability, ProviderCapabilityKind,
    ProviderDeclaration, ProviderId, RuntimeAdapterKind, RuntimeFamily,
};

pub const UMU_PROVIDER_ID: &str = "linux-umu-adapter";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UmuReport {
    pub discovery: UmuDiscoveryReport,
    pub declaration: ProviderDeclaration,
}

pub struct UmuProvider {
    environment: UmuDiscoveryEnvironment,
}
impl Default for UmuProvider {
    fn default() -> Self {
        Self::new(UmuDiscoveryEnvironment::from_process())
    }
}
impl UmuProvider {
    pub fn new(environment: UmuDiscoveryEnvironment) -> Self {
        Self { environment }
    }
    pub fn scan(&self) -> UmuReport {
        let discovery = discover_umu(&self.environment, None);
        let ready = discovery.validated().next().is_some();
        let availability = if ready {
            ProviderAvailability::Available
        } else if discovery.outcome() == RootOutcome::NotPresent {
            ProviderAvailability::Unavailable
        } else {
            ProviderAvailability::Unknown
        };
        let implemented = if ready {
            CapabilityStatus::ImplementedByProvider
        } else if discovery.outcome() == RootOutcome::NotPresent {
            CapabilityStatus::Unavailable
        } else {
            CapabilityStatus::Unknown
        };
        let unsupported = CapabilityStatus::Unsupported;
        let declaration = ProviderDeclaration::new(
            ProviderId::new(UMU_PROVIDER_ID).expect("fixed provider ID"),
            availability,
            vec![RuntimeFamily::Proton],
            vec![
                ProviderCapability::new(ProviderCapabilityKind::IntegrateWithUmu, implemented),
                ProviderCapability::new(ProviderCapabilityKind::PrepareLaunchPlan, implemented),
                ProviderCapability::new(
                    ProviderCapabilityKind::LaunchWithAdapter(RuntimeAdapterKind::UmuProton),
                    implemented,
                ),
                ProviderCapability::new(ProviderCapabilityKind::DiscoverBuilds, unsupported),
                ProviderCapability::new(ProviderCapabilityKind::InspectIdentity, unsupported),
                ProviderCapability::new(ProviderCapabilityKind::VerifySource, unsupported),
                ProviderCapability::new(ProviderCapabilityKind::VerifyIntegrity, unsupported),
                ProviderCapability::new(ProviderCapabilityKind::AssessCompatibility, unsupported),
                ProviderCapability::new(ProviderCapabilityKind::PlanInstallation, unsupported),
                ProviderCapability::new(ProviderCapabilityKind::ManageInstallations, unsupported),
                ProviderCapability::new(ProviderCapabilityKind::ManageUpdates, unsupported),
            ],
        )
        .expect("fixed UMU capability declaration");
        UmuReport {
            discovery,
            declaration,
        }
    }
}
