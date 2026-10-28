//! Non-executable UMU-Proton launch preparation over P021 and P022 contracts.

use crate::umu_discovery::UmuCandidate;
use crate::umu_environment::{PrefixSelection, Store, build_environment};
use crate::{LinuxDiscoveryReport, LinuxRuntimeProvider};
use endfield_unix_runtime::{
    CompatibilityAssessment, CompatibilityScope, CompatibilityState, Executable, Identifier,
    LaunchArgument, LaunchPlan, PathClassification, PlanRisk, PrefixReference, ProcessArchitecture,
    ProcessOwnershipPolicy, RuntimeAdapterKind, RuntimeAdapterSelection, RuntimeFamily,
    RuntimePlatform, WorkingDirectory,
};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanPrerequisite {
    UmuUnavailable,
    ProtonSelectionRequired,
    ProtonSelectionAmbiguous,
    ProtonNotFound,
    ProtonFamilyMismatch,
    ProtonIdentityIncomplete,
    ProtonChanged,
    PlanValidationFailed,
    PrefixSelectionRequired,
    PrefixInvalid,
    GameExecutableRequired,
    GameExecutableInvalid,
    InvalidArguments,
    InvalidEnvironment,
}

#[derive(Clone, PartialEq, Eq)]
pub enum PlanPreparation {
    Complete(LaunchPlan),
    Incomplete(Vec<PlanPrerequisite>),
}

impl std::fmt::Debug for PlanPreparation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Complete(_) => f.write_str("Complete([non-executable plan redacted])"),
            Self::Incomplete(prerequisites) => {
                f.debug_tuple("Incomplete").field(prerequisites).finish()
            }
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct UmuLaunchRequest {
    pub selected_runtime_id: Option<String>,
    pub prefix: Option<PrefixSelection>,
    pub game_executable: Option<PathBuf>,
    pub arguments: Vec<OsString>,
    pub game_id: Option<String>,
    pub store: Option<Store>,
}

impl std::fmt::Debug for UmuLaunchRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UmuLaunchRequest")
            .field("selected_runtime_id", &self.selected_runtime_id)
            .field("prefix", &self.prefix.as_ref().map(|_| "[redacted]"))
            .field(
                "game_executable",
                &self.game_executable.as_ref().map(|_| "[redacted]"),
            )
            .field("arguments", &"[redacted]")
            .field("game_id", &self.game_id.as_ref().map(|_| "[redacted]"))
            .field("store", &self.store)
            .finish()
    }
}

pub struct UmuLaunchPlanBuilder;
impl UmuLaunchPlanBuilder {
    /// Prepares a P021 LaunchPlan. This does not spawn a process, create a prefix,
    /// write files, or authorize launch. P034 must repeat identity checks at launch.
    pub fn prepare(
        provider: &LinuxRuntimeProvider,
        discovery: &LinuxDiscoveryReport,
        umu: &UmuCandidate,
        request: &UmuLaunchRequest,
    ) -> PlanPreparation {
        let mut missing = Vec::new();
        if !umu.is_structurally_valid() || !umu.revalidate() {
            missing.push(PlanPrerequisite::UmuUnavailable);
        }
        let runtime = if let Some(id) = request.selected_runtime_id.as_deref() {
            let matches: Vec<_> = discovery
                .runtimes
                .iter()
                .filter(|item| item.candidate.runtime_id().as_str() == id)
                .collect();
            match matches.as_slice() {
                [] => {
                    missing.push(PlanPrerequisite::ProtonNotFound);
                    None
                }
                [runtime] => Some(*runtime),
                _ => {
                    missing.push(PlanPrerequisite::ProtonSelectionAmbiguous);
                    None
                }
            }
        } else {
            missing.push(PlanPrerequisite::ProtonSelectionRequired);
            None
        };
        if let Some(runtime) = runtime {
            if runtime.candidate.family() != RuntimeFamily::Proton {
                missing.push(PlanPrerequisite::ProtonFamilyMismatch);
            }
        }
        if request.prefix.is_none() {
            missing.push(PlanPrerequisite::PrefixSelectionRequired);
        } else if let Some(prefix) = &request.prefix {
            if !fs::metadata(prefix.path()).is_ok_and(|m| m.is_dir()) {
                missing.push(PlanPrerequisite::PrefixInvalid);
            }
        }
        let game = request.game_executable.as_deref();
        if game.is_none() {
            missing.push(PlanPrerequisite::GameExecutableRequired);
        } else if game.is_some_and(|p| !valid_game_executable(p)) {
            missing.push(PlanPrerequisite::GameExecutableInvalid);
        }
        if request.arguments.len().saturating_add(1)
            > endfield_unix_runtime::validation::MAX_ARGUMENTS
            || request.arguments.iter().any(|a| {
                a.as_encoded_bytes().contains(&0)
                    || a.as_encoded_bytes().len()
                        > endfield_unix_runtime::validation::MAX_ARGUMENT_BYTES
            })
        {
            missing.push(PlanPrerequisite::InvalidArguments);
        }
        if !missing.is_empty() {
            return PlanPreparation::Incomplete(missing);
        }
        let runtime = runtime.expect("selection checked");
        if runtime.candidate.family() != RuntimeFamily::Proton {
            return PlanPreparation::Incomplete(vec![PlanPrerequisite::ProtonFamilyMismatch]);
        }
        let identity = match endfield_unix_runtime::RuntimeProvider::inspect_identity(
            provider,
            &runtime.candidate,
        ) {
            Ok(identity) => identity,
            Err(error)
                if error.kind()
                    == endfield_unix_runtime::ProviderErrorKind::ProviderUnavailable =>
            {
                return PlanPreparation::Incomplete(vec![PlanPrerequisite::ProtonChanged]);
            }
            Err(_) => {
                return PlanPreparation::Incomplete(vec![
                    PlanPrerequisite::ProtonIdentityIncomplete,
                ]);
            }
        };
        if !runtime.path().is_absolute() || !runtime.path().is_dir() {
            return PlanPreparation::Incomplete(vec![PlanPrerequisite::ProtonChanged]);
        }
        let prefix = request.prefix.as_ref().expect("checked");
        let game = game.expect("checked");
        let adapter =
            match RuntimeAdapterSelection::new(RuntimeAdapterKind::UmuProton, &identity, None) {
                Ok(adapter) => adapter,
                Err(_) => {
                    return PlanPreparation::Incomplete(vec![
                        PlanPrerequisite::ProtonFamilyMismatch,
                    ]);
                }
            };
        let environment = match build_environment(
            Some(runtime.path()),
            Some(prefix),
            request.game_id.as_deref(),
            request.store,
        ) {
            Ok(environment) => environment,
            Err(_) => {
                return PlanPreparation::Incomplete(vec![PlanPrerequisite::InvalidEnvironment]);
            }
        };
        let arguments = match request
            .arguments
            .iter()
            .cloned()
            .map(|value| {
                LaunchArgument::new(
                    value,
                    endfield_unix_runtime::ValueSensitivity::Sensitive(
                        endfield_unix_runtime::SensitiveValueKind::OtherSecret,
                    ),
                )
            })
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(arguments) => arguments,
            Err(_) => return PlanPreparation::Incomplete(vec![PlanPrerequisite::InvalidArguments]),
        };
        let Some(umu_path) = umu.resolved_path() else {
            return PlanPreparation::Incomplete(vec![PlanPrerequisite::UmuUnavailable]);
        };
        let scope = match CompatibilityScope::new(
            identity.build_identity(),
            RuntimeAdapterKind::UmuProton,
            RuntimePlatform::Linux,
            ProcessArchitecture::Unknown,
            None,
            None,
            vec![],
            None,
            None,
            vec![],
        ) {
            Ok(scope) => scope,
            Err(_) => {
                return PlanPreparation::Incomplete(vec![
                    PlanPrerequisite::ProtonIdentityIncomplete,
                ]);
            }
        };
        let compatibility = CompatibilityAssessment::from_evidence(scope, vec![])
            .expect("empty assessment is valid");
        debug_assert_eq!(compatibility.state(), CompatibilityState::NotEvaluated);
        let prefix_reference = PrefixReference::new(
            Identifier::new("explicit-prefix").expect("fixed identifier"),
            prefix.path().to_path_buf(),
            PathClassification::Private,
            prefix.ownership(),
        );
        let plan = LaunchPlan::new(
            Identifier::new("umu-proton-preflight").expect("fixed identifier"),
            endfield_unix_runtime::CURRENT_LAUNCH_PLAN_SCHEMA_VERSION,
            identity,
            adapter,
            Executable::new(
                umu_path,
                endfield_unix_runtime::ExecutablePathPolicy::Absolute,
                PathClassification::Private,
            )
            .expect("validated UMU executable"),
            std::iter::once(
                LaunchArgument::new(
                    game.as_os_str().to_owned(),
                    endfield_unix_runtime::ValueSensitivity::Sensitive(
                        endfield_unix_runtime::SensitiveValueKind::SensitivePath,
                    ),
                )
                .expect("validated game path"),
            )
            .chain(arguments)
            .collect(),
            WorkingDirectory::new(
                game.parent().unwrap_or(Path::new("/")),
                PathClassification::Private,
            )
            .expect("game parent is absolute"),
            environment,
            Some(prefix_reference.expect("prefix reference is valid")),
            vec![],
            endfield_unix_runtime::OutputCapturePolicy::Inherit,
            ProcessOwnershipPolicy::NoOwnershipTransfer,
            PlanRisk::UserScopedLaunch,
            compatibility,
        );
        match plan {
            Ok(plan) => PlanPreparation::Complete(plan),
            Err(_) => PlanPreparation::Incomplete(vec![PlanPrerequisite::PlanValidationFailed]),
        }
    }
}

fn valid_game_executable(path: &Path) -> bool {
    path.is_absolute()
        && path.as_os_str().len() <= 4096
        && !path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
        && fs::metadata(path).is_ok_and(|m| m.is_file())
}

/// Convenience helper for diagnostic output without exposing private paths or arguments.
pub fn redacted_plan_summary(plan: &LaunchPlan, umu: &UmuCandidate) -> String {
    format!(
        "adapter_provider={} umu_availability={} umu_version={} runtime_provider={} adapter={} family={} lineage={} runtime_version={} source={} compatibility={} missing_prerequisites=none executable=<redacted> prefix=<redacted> arguments=<redacted> execution_authorized=false executable_now=false",
        crate::umu::UMU_PROVIDER_ID,
        umu.availability().as_str(),
        umu.version().unwrap_or("unknown"),
        plan.runtime().provider_id().as_str(),
        plan.adapter().kind().as_str(),
        plan.runtime().family().as_str(),
        plan.runtime().lineage().as_str(),
        plan.runtime().version().as_str(),
        plan.source_verification().as_str(),
        plan.compatibility().state().as_str()
    )
}
