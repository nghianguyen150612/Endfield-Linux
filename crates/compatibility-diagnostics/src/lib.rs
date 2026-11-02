//! Portable, deterministic compatibility assessment over normalized evidence.
//!
//! This crate performs no discovery, filesystem access, process execution, or
//! compatibility scoring. Linux/application adapters acquire observations and
//! pass them here for explanation and cross-component assessment.

use serde::{Deserialize, Serialize};

pub const REPORT_SCHEMA_VERSION: u32 = 1;
pub const MAX_FINDINGS: usize = 128;
pub const MAX_DEVICES: usize = 64;
pub const MAX_DRM_NODES: usize = 128;
pub const MAX_EXTENSIONS: usize = 256;
pub const MAX_FEATURES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticState {
    VerifiedObservation,
    StaticCapabilityAvailable,
    MissingRequirement,
    UnsupportedConfiguration,
    StaleOrInconsistent,
    AccessDenied,
    PartialObservation,
    UnknownOrUnverifiable,
    NotTested,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticConfidence {
    High,
    Medium,
    Low,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStrength {
    DirectObservation,
    ValidatedReference,
    DerivedCorrelation,
    MissingObservation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Information,
    Warning,
    Blocker,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticVerdict {
    NoKnownStaticBlocker,
    BlockedByObservedFacts,
    IncompleteObservations,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameCompatibilityState {
    NotTested,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatibilityTarget {
    pub game: String,
    pub profile_scope: Option<String>,
    pub prefix_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformSummary {
    pub os: String,
    pub distribution: Option<String>,
    pub distribution_version: Option<String>,
    pub kernel_release: Option<String>,
    pub architecture: Option<String>,
    pub environment: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSelectionState {
    Selected,
    NoSelection,
    Stale,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFamily {
    Wine,
    Proton,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeOrigin {
    Managed,
    External,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeIdentity {
    /// Managed installation ID or external P022 structural digest; never a path.
    pub exact_identity: String,
    pub family: RuntimeFamily,
    pub build: Option<String>,
    pub origin: RuntimeOrigin,
    pub ownership: String,
    pub provenance: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSelectionDiagnostic {
    pub state: RuntimeSelectionState,
    pub source: Option<String>,
    pub pin_id: Option<String>,
    pub pin_revision: Option<u64>,
    pub registry_revision: Option<u64>,
    pub automatic_fallback: bool,
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerDiagnostic {
    pub selection: RuntimeSelectionDiagnostic,
    pub runtime: Option<RuntimeIdentity>,
    pub installation_state: DiagnosticState,
    pub identity_state: DiagnosticState,
    pub integrity_state: DiagnosticState,
    pub adapter_preference: Option<String>,
    pub adapter_state: DiagnosticState,
    pub adapter_diagnostic: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceNodeAccess {
    Accessible,
    Denied,
    Missing,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrmNodeKind {
    Primary,
    Render,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrmNodeDiagnostic {
    pub name: String,
    pub kind: DrmNodeKind,
    pub access: DeviceNodeAccess,
    pub pci_bdf: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceCorrelation {
    ExactPciIdentity,
    AmbiguousPciIdentity,
    ConflictingPciIdentity,
    Unmatched,
    NotApplicable,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverDiagnostic {
    pub pci_bdf: String,
    pub vendor_id: u16,
    pub device_id: u16,
    pub vendor: String,
    pub gpu_class: Option<String>,
    pub kernel_driver: Option<String>,
    pub kernel_driver_state: DiagnosticState,
    /// No kernel-release inference is made. Present only when a source reports it.
    pub kernel_driver_version: Option<String>,
    pub drm_nodes: Vec<DrmNodeDiagnostic>,
    pub vulkan_correlation: DeviceCorrelation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VulkanLoaderState {
    Missing,
    InitializationFailed,
    AvailableNoDevices,
    Enumerated,
    Partial,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VulkanDeviceType {
    Integrated,
    Discrete,
    Virtual,
    Cpu,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VulkanMetadataState {
    Available,
    Partial,
    Unavailable,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VulkanDeviceDiagnostic {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub pci_bdf: Option<String>,
    pub correlation: DeviceCorrelation,
    pub device_type: VulkanDeviceType,
    pub api_version: Option<(u32, u32, u32)>,
    pub driver_id: Option<i32>,
    pub driver_name: Option<String>,
    pub driver_info: Option<String>,
    pub extension_state: VulkanMetadataState,
    pub supported_extensions: Vec<String>,
    pub feature_state: VulkanMetadataState,
    /// Names of a bounded set of Vulkan 1.0 features observed as supported.
    pub supported_features: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VulkanDiagnostic {
    pub loader_state: VulkanLoaderState,
    pub loader_api_version: Option<(u32, u32, u32)>,
    pub devices: Vec<VulkanDeviceDiagnostic>,
    pub observation_state: DiagnosticState,
    pub diagnostic: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefixLifecycle {
    Planned,
    Created,
    Initialized,
    Incomplete,
    RecoveryRequired,
    Missing,
    Blocked,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefixRecoveryState {
    None,
    IntentOnly,
    Staged,
    PublishedPending,
    Completed,
    RollbackPending,
    RollbackCompleted,
    PendingDeletion,
    PendingPinRecovery,
    IncompleteOrInconsistent,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefixDiagnostic {
    pub prefix_id: String,
    pub scope: String,
    pub lifecycle: PrefixLifecycle,
    pub ownership_state: DiagnosticState,
    pub filesystem_state: DiagnosticState,
    pub runtime_association_state: DiagnosticState,
    pub original_pin_id: Option<String>,
    pub original_pin_revision: Option<u64>,
    pub adapter_preference: Option<String>,
    pub current_pin_state: DiagnosticState,
    pub current_pin_relation: Option<String>,
    pub initialization_state: DiagnosticState,
    pub initialization_evidence_available: bool,
    pub recovery_state: PrefixRecoveryState,
    pub recovery_diagnostic: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticEvidence {
    pub source: String,
    pub observed_value: Option<String>,
    pub strength: EvidenceStrength,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestedRemediation {
    pub action: String,
    pub supported_by_endfield_unix: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatibilityFinding {
    pub id: String,
    pub component: String,
    pub state: DiagnosticState,
    pub severity: DiagnosticSeverity,
    pub confidence: DiagnosticConfidence,
    pub evidence: Vec<DiagnosticEvidence>,
    pub expected_condition: Option<String>,
    pub reason: String,
    pub evidence_limitations: String,
    pub suggested_remediation: Option<SuggestedRemediation>,
    pub blocks_later_launch_qualification: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatibilityDiagnosticReport {
    pub schema_version: u32,
    pub observed_at_unix_seconds: u64,
    pub target: CompatibilityTarget,
    pub platform: PlatformSummary,
    pub runner: RunnerDiagnostic,
    pub gpu_inventory_state: DiagnosticState,
    pub drm_inventory_state: DiagnosticState,
    pub drivers: Vec<DriverDiagnostic>,
    pub unmatched_drm_nodes: Vec<DrmNodeDiagnostic>,
    pub vulkan: VulkanDiagnostic,
    /// Static DXVK/VKD3D-Proton requirements and selected-runner component
    /// presence are unknown without authoritative versioned metadata.
    pub translation_layer_readiness: DiagnosticState,
    pub prefix: Option<PrefixDiagnostic>,
    pub verdict: DiagnosticVerdict,
    pub game_compatibility: GameCompatibilityState,
    pub findings: Vec<CompatibilityFinding>,
    pub compatibility_limitations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatibilityObservations {
    pub observed_at_unix_seconds: u64,
    pub target: CompatibilityTarget,
    pub platform: PlatformSummary,
    pub runner: RunnerDiagnostic,
    pub gpu_inventory_state: DiagnosticState,
    pub drm_inventory_state: DiagnosticState,
    pub drivers: Vec<DriverDiagnostic>,
    pub unmatched_drm_nodes: Vec<DrmNodeDiagnostic>,
    pub vulkan: VulkanDiagnostic,
    pub prefix: Option<PrefixDiagnostic>,
}

/// Adds deterministic explanations to already normalized observations.
pub fn assess(mut observations: CompatibilityObservations) -> CompatibilityDiagnosticReport {
    observations.drivers.truncate(MAX_DEVICES);
    observations.unmatched_drm_nodes.truncate(MAX_DRM_NODES);
    observations.vulkan.devices.truncate(MAX_DEVICES);
    for device in &mut observations.vulkan.devices {
        device.supported_extensions.truncate(MAX_EXTENSIONS);
        device.supported_features.truncate(MAX_FEATURES);
    }
    let mut findings = Vec::new();
    assess_runner(&observations.runner, &mut findings);
    assess_gpu_inventory(&observations, &mut findings);
    assess_drivers(&observations.drivers, &mut findings);
    assess_unmatched_drm_nodes(&observations.unmatched_drm_nodes, &mut findings);
    assess_vulkan(&observations.vulkan, &observations.drivers, &mut findings);
    if let Some(prefix) = &observations.prefix {
        assess_prefix(prefix, &mut findings);
    }
    assess_cross_component(&observations, &mut findings);
    findings.sort_by(|left, right| left.id.cmp(&right.id));
    findings.dedup_by(|left, right| left.id == right.id);
    findings.truncate(MAX_FINDINGS);
    let verdict = determine_verdict(&findings);

    CompatibilityDiagnosticReport {
        schema_version: REPORT_SCHEMA_VERSION,
        observed_at_unix_seconds: observations.observed_at_unix_seconds,
        target: observations.target,
        platform: observations.platform,
        runner: observations.runner,
        gpu_inventory_state: observations.gpu_inventory_state,
        drm_inventory_state: observations.drm_inventory_state,
        drivers: observations.drivers,
        unmatched_drm_nodes: observations.unmatched_drm_nodes,
        vulkan: observations.vulkan,
        translation_layer_readiness: DiagnosticState::UnknownOrUnverifiable,
        prefix: observations.prefix,
        verdict,
        game_compatibility: GameCompatibilityState::NotTested,
        findings,
        compatibility_limitations: vec![
            "Static observations do not prove that Arknights: Endfield will launch or render correctly.".into(),
            "Vulkan availability is not evidence of Direct3D translation-layer availability or compatibility.".into(),
            "Wine/Proton identity, prefix creation, and advertised GPU capabilities do not establish successful Wine initialization or game compatibility.".into(),
            "No game, runtime, benchmark, driver, or compatibility layer was executed by this diagnostic.".into(),
        ],
    }
}

fn assess_gpu_inventory(
    observations: &CompatibilityObservations,
    findings: &mut Vec<CompatibilityFinding>,
) {
    if observations.gpu_inventory_state != DiagnosticState::VerifiedObservation {
        findings.push(finding(
            "gpu.inventory_partial",
            "gpu",
            observations.gpu_inventory_state,
            DiagnosticSeverity::Warning,
            DiagnosticConfidence::Medium,
            "The PCI GPU inventory is missing, partial, or unverifiable.",
            Some("A bounded P013 PCI inventory with validated BDF identity."),
            vec![evidence(
                "P013 PCI inventory",
                Some(&format!("{} device records", observations.drivers.len())),
                EvidenceStrength::DirectObservation,
            )],
            "An incomplete inventory cannot establish that no GPU is present.",
            Some("Reinspect Linux PCI and DRM observations."),
            false,
        ));
    }
    if observations.drm_inventory_state != DiagnosticState::VerifiedObservation {
        findings.push(finding(
            "gpu.drm_inventory_partial",
            "gpu",
            observations.drm_inventory_state,
            DiagnosticSeverity::Warning,
            DiagnosticConfidence::Medium,
            "The DRM node inventory is missing, partial, or unverifiable.",
            Some("A validated DRM primary/render-node inventory and access observation."),
            vec![evidence(
                "P013 DRM node inventory",
                None,
                EvidenceStrength::MissingObservation,
            )],
            "Missing DRM observations do not prove that no render node exists.",
            Some("Check GPU/DRM node visibility and user permissions."),
            false,
        ));
    }
}

fn assess_drivers(drivers: &[DriverDiagnostic], findings: &mut Vec<CompatibilityFinding>) {
    if drivers.len() > 1 {
        findings.push(finding(
            "gpu.multiple_devices_observed",
            "gpu",
            DiagnosticState::VerifiedObservation,
            DiagnosticSeverity::Information,
            DiagnosticConfidence::High,
            format!("{} PCI graphics devices were observed.", drivers.len()),
            None,
            vec![evidence(
                "P013 PCI inventory",
                Some(&format!("{} devices", drivers.len())),
                EvidenceStrength::DirectObservation,
            )],
            "A future game process's selected GPU is not known from this inventory.",
            Some("Use exact PCI/Vulkan correlation; do not infer or set a default GPU."),
            false,
        ));
    }
    for driver in drivers {
        let finding_suffix = driver.pci_bdf.as_str();
        if driver.kernel_driver_state == DiagnosticState::VerifiedObservation
            && driver.kernel_driver_version.is_none()
        {
            findings.push(finding(
                &format!("driver.version_unreported.{finding_suffix}"),
                "driver",
                DiagnosticState::UnknownOrUnverifiable,
                DiagnosticSeverity::Information,
                DiagnosticConfidence::High,
                format!("No source-provided kernel driver version was reported for {}.", driver.pci_bdf),
                None,
                vec![evidence("P013 PCI driver-binding observation", driver.kernel_driver.as_deref(), EvidenceStrength::DirectObservation)],
                "Kernel release and Vulkan driver properties are not substituted for a kernel module version.",
                Some("Reinspect the reported PCI and Vulkan identity evidence."),
                false,
            ));
        }
        if driver.drm_nodes.is_empty() {
            findings.push(finding(
                &format!("gpu.drm_nodes_unobserved.{finding_suffix}"),
                "driver",
                DiagnosticState::UnknownOrUnverifiable,
                DiagnosticSeverity::Warning,
                DiagnosticConfidence::Medium,
                format!("No DRM primary or render node was correlated to {}.", driver.pci_bdf),
                Some("A validated DRM node relationship when later qualification requires direct device access."),
                vec![evidence("P013 DRM node inventory", Some(&driver.pci_bdf), EvidenceStrength::MissingObservation)],
                "A missing relationship does not prove the device cannot be used by another graphics path.",
                Some("Check GPU/DRM node visibility and user permissions."),
                true,
            ));
        }
        if driver.kernel_driver_state == DiagnosticState::MissingRequirement {
            findings.push(finding(
                &format!("gpu.kernel_driver_unbound.{finding_suffix}"),
                "driver",
                DiagnosticState::MissingRequirement,
                DiagnosticSeverity::Warning,
                DiagnosticConfidence::High,
                format!("No bound kernel graphics driver was observed for {}.", driver.pci_bdf),
                Some("A kernel driver binding when this device is expected to provide graphics."),
                vec![evidence("P013 PCI sysfs driver link", Some(&driver.pci_bdf), EvidenceStrength::DirectObservation)],
                "Kernel binding does not establish userspace driver consistency or future process visibility.",
                Some("Check the distribution-supported kernel driver and userspace graphics stack."),
                false,
            ));
        }
        if driver
            .drm_nodes
            .iter()
            .any(|node| node.access == DeviceNodeAccess::Denied)
        {
            findings.push(finding(
                &format!("gpu.drm_access_denied.{finding_suffix}"),
                "driver",
                DiagnosticState::AccessDenied,
                DiagnosticSeverity::Warning,
                DiagnosticConfidence::High,
                format!(
                    "A DRM node for {} is not readable by the current user.",
                    driver.pci_bdf
                ),
                Some("A readable render node if later qualification requires direct GPU access."),
                vec![evidence(
                    "P013 DRM permission check",
                    Some(&driver.pci_bdf),
                    EvidenceStrength::DirectObservation,
                )],
                "The probe checks read permission and does not open the DRM device node.",
                Some(
                    "Check the device node's owner, group, and distribution-supported permissions.",
                ),
                false,
            ));
        }
    }
}

fn assess_unmatched_drm_nodes(
    nodes: &[DrmNodeDiagnostic],
    findings: &mut Vec<CompatibilityFinding>,
) {
    for (index, node) in nodes.iter().enumerate() {
        let state = match node.access {
            DeviceNodeAccess::Denied => DiagnosticState::AccessDenied,
            DeviceNodeAccess::Unknown => DiagnosticState::UnknownOrUnverifiable,
            DeviceNodeAccess::Accessible | DeviceNodeAccess::Missing => {
                DiagnosticState::PartialObservation
            }
        };
        findings.push(finding(
            &format!("gpu.drm_node_identity_unmatched.{}.{}", node.name, index),
            "gpu",
            state,
            if node.access == DeviceNodeAccess::Denied {
                DiagnosticSeverity::Warning
            } else {
                DiagnosticSeverity::Information
            },
            DiagnosticConfidence::Medium,
            format!(
                "DRM node {} has no exact validated PCI GPU correlation.",
                node.name
            ),
            Some("A validated DRM-to-PCI BDF relationship."),
            vec![evidence(
                "P013 DRM sysfs topology",
                node.pci_bdf.as_deref(),
                if node.pci_bdf.is_some() {
                    EvidenceStrength::DerivedCorrelation
                } else {
                    EvidenceStrength::MissingObservation
                },
            )],
            "Node names and enumeration order do not establish which GPU owns an unmatched node.",
            Some("Check GPU/DRM node visibility and user permissions."),
            false,
        ));
    }
}

fn assess_runner(runner: &RunnerDiagnostic, findings: &mut Vec<CompatibilityFinding>) {
    match runner.selection.state {
        RuntimeSelectionState::NoSelection => findings.push(finding(
            "runner.no_selection",
            "runner",
            DiagnosticState::UnknownOrUnverifiable,
            DiagnosticSeverity::Warning,
            DiagnosticConfidence::High,
            "No machine/profile runtime pin is selected for this target.",
            Some("An explicitly selected P027 runtime reference"),
            vec![evidence("P027 read-only pin registry", None, EvidenceStrength::MissingObservation)],
            "Selection is absent; no runtime discovery or fallback was performed.",
            Some("Inspect available runtimes and select a runtime explicitly."),
            false,
        )),
        RuntimeSelectionState::Stale => findings.push(finding(
            "runner.selected_reference_stale",
            "runner",
            DiagnosticState::StaleOrInconsistent,
            DiagnosticSeverity::Blocker,
            DiagnosticConfidence::High,
            runner.selection.diagnostic.as_deref().unwrap_or("The selected runtime reference could not be revalidated."),
            Some("The exact selected runtime identity and structural integrity must revalidate."),
            vec![evidence("P027 selected pin and P022/P025/P026 revalidation", runner.runtime.as_ref().map(|runtime| runtime.exact_identity.as_str()), EvidenceStrength::ValidatedReference)],
            "A stale selected reference blocks runtime readiness. Lower-precedence pins and system Wine were not substituted.",
            Some("Inspect the selected pin and revalidate its exact runtime reference."),
            true,
        )),
        RuntimeSelectionState::Unknown => findings.push(finding(
            "runner.selection_unknown",
            "runner",
            DiagnosticState::UnknownOrUnverifiable,
            DiagnosticSeverity::Warning,
            DiagnosticConfidence::Low,
            runner.selection.diagnostic.as_deref().unwrap_or("The runtime selection could not be observed."),
            Some("A readable, valid P027 pin registry and machine identity."),
            vec![evidence("P027 read-only pin registry", None, EvidenceStrength::MissingObservation)],
            "The selection source is unavailable or inconsistent, so runtime readiness is unknown.",
            Some("Check the read-only P027 registry and machine/profile identity."),
            true,
        )),
        RuntimeSelectionState::Selected => {
            let exact = runner.runtime.as_ref().map(|runtime| runtime.exact_identity.as_str());
            findings.push(finding(
                "runner.selected_reference_observed",
                "runner",
                if runner.identity_state == DiagnosticState::VerifiedObservation { DiagnosticState::VerifiedObservation } else { runner.identity_state },
                if runner.identity_state == DiagnosticState::VerifiedObservation { DiagnosticSeverity::Information } else { DiagnosticSeverity::Blocker },
                DiagnosticConfidence::High,
                "The explicitly selected P027 runtime reference was observed.",
                Some("The selected runtime family, identity, ownership, and provenance remain explicit."),
                vec![evidence("P027 selection", exact, EvidenceStrength::ValidatedReference)],
                "A selected and structurally revalidated runtime is not proof of Endfield compatibility.",
                Some("Use later real-device qualification for runtime and game behavior."),
                false,
            ));
            if runner.integrity_state != DiagnosticState::VerifiedObservation {
                findings.push(finding(
                    "runner.integrity_unverified",
                    "runner",
                    runner.integrity_state,
                    DiagnosticSeverity::Warning,
                    DiagnosticConfidence::Medium,
                    runner.selection.diagnostic.as_deref().unwrap_or("The runtime integrity baseline was not established."),
                    Some("Managed P025 runtimes should match their P026 baseline; external P022 runtimes remain structural-only."),
                    vec![evidence("P026 managed audit or P022 external structural identity", exact, EvidenceStrength::ValidatedReference)],
                    "Runtime identity or provenance does not establish compatibility or trusted origin.",
                    Some("Reinspect the exact selected runtime and its recorded provenance."),
                    true,
                ));
            }
            if runner.adapter_state != DiagnosticState::StaticCapabilityAvailable {
                findings.push(finding(
                    "runner.adapter_unavailable",
                    "adapter",
                    runner.adapter_state,
                    if runner.adapter_state == DiagnosticState::UnsupportedConfiguration { DiagnosticSeverity::Blocker } else { DiagnosticSeverity::Warning },
                    DiagnosticConfidence::High,
                    runner.adapter_diagnostic.as_deref().unwrap_or("The selected adapter is not available through a validated execution adapter."),
                    Some("The stored adapter preference must have a validated adapter implementation and be available."),
                    vec![evidence("P027 adapter preference and P023 UMU discovery", runner.adapter_preference.as_deref(), EvidenceStrength::ValidatedReference)],
                    "No adapter was changed or substituted automatically.",
                    Some("Inspect the stored adapter preference; adapter setup is outside this read-only diagnostic."),
                    runner.adapter_preference.is_some(),
                ));
            }
        }
    }
}

fn assess_vulkan(
    vulkan: &VulkanDiagnostic,
    drivers: &[DriverDiagnostic],
    findings: &mut Vec<CompatibilityFinding>,
) {
    let inconsistent = match vulkan.loader_state {
        VulkanLoaderState::Enumerated => vulkan.devices.is_empty(),
        VulkanLoaderState::Missing
        | VulkanLoaderState::InitializationFailed
        | VulkanLoaderState::AvailableNoDevices => !vulkan.devices.is_empty(),
        VulkanLoaderState::Partial | VulkanLoaderState::Unknown => false,
    };
    if inconsistent {
        findings.push(finding(
            "vulkan.status_metadata_inconsistent",
            "vulkan",
            DiagnosticState::StaleOrInconsistent,
            DiagnosticSeverity::Blocker,
            DiagnosticConfidence::High,
            "The Vulkan loader state conflicts with the normalized physical-device list.",
            Some("Loader state and physical-device enumeration results agree."),
            vec![evidence(
                "P013 normalized Vulkan observation",
                Some(&format!(
                    "{:?}; {} devices",
                    vulkan.loader_state,
                    vulkan.devices.len()
                )),
                EvidenceStrength::DirectObservation,
            )],
            "Inconsistent normalized observations cannot support a positive Vulkan assessment.",
            Some("Reinspect the Vulkan provider output and its validation result."),
            true,
        ));
    }
    match vulkan.loader_state {
        VulkanLoaderState::Missing => findings.push(finding(
            "vulkan.loader_unavailable",
            "vulkan",
            DiagnosticState::MissingRequirement,
            DiagnosticSeverity::Warning,
            DiagnosticConfidence::High,
            vulkan.diagnostic.as_deref().unwrap_or("The Vulkan loader could not be loaded."),
            Some("A loader that can initialize and enumerate Vulkan physical devices."),
            vec![evidence("P013 ash-based Vulkan provider", None, EvidenceStrength::DirectObservation)],
            "Rendering capability is unknown because the loader was unavailable; no game or translation layer was tested.",
            Some("Check the distribution-supported Vulkan loader and userspace driver installation."),
            true,
        )),
        VulkanLoaderState::InitializationFailed => findings.push(finding(
            "vulkan.instance_initialization_failed",
            "vulkan",
            DiagnosticState::PartialObservation,
            DiagnosticSeverity::Warning,
            DiagnosticConfidence::High,
            vulkan.diagnostic.as_deref().unwrap_or("Vulkan instance initialization failed."),
            Some("A Vulkan instance and physical-device inventory."),
            vec![evidence("P013 ash-based Vulkan provider", None, EvidenceStrength::DirectObservation)],
            "The loader was present but instance initialization did not complete.",
            Some("Check loader and userspace driver diagnostics with distribution-supported tools."),
            true,
        )),
        VulkanLoaderState::AvailableNoDevices => findings.push(finding(
            "vulkan.no_physical_devices",
            "vulkan",
            DiagnosticState::PartialObservation,
            DiagnosticSeverity::Warning,
            DiagnosticConfidence::High,
            "The loader initialized, but no physical device was enumerated.",
            Some("At least one Vulkan physical device, if Vulkan rendering is required."),
            vec![evidence("P013 ash-based Vulkan provider", Some("zero physical devices"), EvidenceStrength::DirectObservation)],
            "This observation does not identify whether the cause is driver configuration, permissions, or device support.",
            Some("Check the Vulkan loader and userspace driver installation."),
            true,
        )),
        VulkanLoaderState::Partial | VulkanLoaderState::Unknown => findings.push(finding(
            "vulkan.observation_partial",
            "vulkan",
            DiagnosticState::PartialObservation,
            DiagnosticSeverity::Warning,
            DiagnosticConfidence::Low,
            vulkan.diagnostic.as_deref().unwrap_or("Vulkan enumeration is incomplete or unavailable."),
            Some("Complete loader, instance, and physical-device observations."),
            vec![evidence("P013 ash-based Vulkan provider", None, EvidenceStrength::MissingObservation)],
            "Unobserved devices and capabilities remain unknown.",
            Some("Reinspect Vulkan loader and userspace driver availability."),
            true,
        )),
        VulkanLoaderState::Enumerated => findings.push(finding(
            "vulkan.physical_devices_observed",
            "vulkan",
            DiagnosticState::StaticCapabilityAvailable,
            DiagnosticSeverity::Information,
            DiagnosticConfidence::High,
            format!("{} Vulkan physical device(s) were enumerated.", vulkan.devices.len()),
            None,
            vec![evidence("P013 ash-based Vulkan provider", Some("physical-device enumeration succeeded"), EvidenceStrength::DirectObservation)],
            "Vulkan enumeration does not establish Direct3D translation, performance, or Endfield rendering.",
            Some("Use later scoped real-device qualification to evaluate translation and game behavior."),
            false,
        )),
    }
    for (device_index, device) in vulkan.devices.iter().enumerate() {
        let finding_suffix = device.pci_bdf.clone().unwrap_or_else(|| {
            format!(
                "{:08x}:{:08x}:{device_index}",
                device.vendor_id, device.device_id
            )
        });
        if device.device_type == VulkanDeviceType::Cpu {
            findings.push(finding(
                &format!("vulkan.software_device_observed.{finding_suffix}"),
                "vulkan",
                DiagnosticState::StaticCapabilityAvailable,
                DiagnosticSeverity::Information,
                DiagnosticConfidence::High,
                format!("A Vulkan CPU-type device was enumerated: {}.", device.name),
                None,
                vec![evidence("P013 Vulkan physical-device properties", Some("device_type=cpu"), EvidenceStrength::DirectObservation)],
                "Enumeration does not establish performance or prove which device a future process selects.",
                Some("Use later scoped real-device qualification to evaluate translation and game behavior."),
                false,
            ));
        }
        if device.extension_state != VulkanMetadataState::Available {
            findings.push(finding(
                &format!("vulkan.extension_metadata_partial.{finding_suffix}"),
                "vulkan",
                DiagnosticState::PartialObservation,
                DiagnosticSeverity::Warning,
                DiagnosticConfidence::Medium,
                format!(
                    "Supported extension metadata is {:?} for {}.",
                    device.extension_state, device.name
                ),
                Some("A complete bounded device-extension observation."),
                vec![evidence(
                    "P013 Vulkan device-extension enumeration",
                    Some(&format!("{:?}", device.extension_state)),
                    EvidenceStrength::MissingObservation,
                )],
                "P030 does not infer application or translation-layer extension requirements.",
                Some("Reinspect Vulkan loader and userspace driver availability."),
                true,
            ));
        }
        if device.feature_state != VulkanMetadataState::Available {
            findings.push(finding(
                &format!("vulkan.feature_metadata_partial.{finding_suffix}"),
                "vulkan",
                DiagnosticState::PartialObservation,
                DiagnosticSeverity::Warning,
                DiagnosticConfidence::Medium,
                format!(
                    "Supported core feature metadata is {:?} for {}.",
                    device.feature_state, device.name
                ),
                Some("A complete bounded Vulkan core-feature observation."),
                vec![evidence(
                    "P013 Vulkan core feature query",
                    Some(&format!("{:?}", device.feature_state)),
                    EvidenceStrength::MissingObservation,
                )],
                "Observed core features are not treated as Direct3D translation requirements.",
                Some("Reinspect Vulkan loader and userspace driver availability."),
                true,
            ));
        }
        match device.correlation {
            DeviceCorrelation::AmbiguousPciIdentity => findings.push(finding(
                &format!("vulkan.device_correlation_ambiguous.{finding_suffix}"),
                "vulkan",
                DiagnosticState::PartialObservation,
                DiagnosticSeverity::Warning,
                DiagnosticConfidence::High,
                format!("Vulkan device {} has ambiguous PCI identity evidence.", device.name),
                Some("One validated Vulkan PCI BDF mapped to one PCI GPU."),
                vec![evidence("P013 VK_EXT_pci_bus_info correlation", device.pci_bdf.as_deref(), EvidenceStrength::DerivedCorrelation)],
                "No device selection is inferred from Vulkan enumeration order.",
                Some("Inspect the PCI/Vulkan identities and preserve the ambiguity."),
                false,
            )),
            DeviceCorrelation::ConflictingPciIdentity => findings.push(finding(
                &format!("vulkan.device_identity_conflict.{finding_suffix}"),
                "vulkan",
                DiagnosticState::StaleOrInconsistent,
                DiagnosticSeverity::Warning,
                DiagnosticConfidence::High,
                format!("Vulkan device {} has PCI identity evidence that conflicts with the PCI inventory.", device.name),
                Some("Vendor/device IDs agree with the exact BDF."),
                vec![evidence("P013 PCI and VK_EXT_pci_bus_info identities", device.pci_bdf.as_deref(), EvidenceStrength::DerivedCorrelation)],
                "Conflicting identities remain unmatched and do not select a GPU.",
                Some("Reinspect device identity and userspace driver metadata."),
                false,
            )),
            DeviceCorrelation::Unmatched => findings.push(finding(
                &format!("vulkan.device_identity_unmatched.{finding_suffix}"),
                "vulkan",
                DiagnosticState::UnknownOrUnverifiable,
                DiagnosticSeverity::Information,
                DiagnosticConfidence::Medium,
                format!("No exact PCI identity was correlated for Vulkan device {}.", device.name),
                Some("A validated exact BDF correlation when one is available."),
                vec![evidence("P013 PCI/Vulkan identity correlation", device.pci_bdf.as_deref(), EvidenceStrength::MissingObservation)],
                "Name, vendor/device IDs alone, or enumeration order are not physical identity evidence.",
                Some("Inspect the reported PCI and Vulkan identity evidence."),
                false,
            )),
            _ => {}
        }
        if device.driver_id.is_none() || device.driver_name.is_none() {
            findings.push(finding(
                &format!("vulkan.driver_properties_unavailable.{finding_suffix}"),
                "vulkan",
                DiagnosticState::UnknownOrUnverifiable,
                DiagnosticSeverity::Information,
                DiagnosticConfidence::Medium,
                format!(
                    "Vulkan driver properties were not fully reported for {}.",
                    device.name
                ),
                None,
                vec![evidence(
                    "P013 VK_KHR_driver_properties",
                    device.driver_name.as_deref(),
                    EvidenceStrength::MissingObservation,
                )],
                "Missing Vulkan properties do not establish a missing kernel or userspace driver.",
                Some("Reinspect Vulkan loader and userspace driver availability."),
                false,
            ));
        }
    }
    for driver in drivers {
        match driver.vulkan_correlation {
            DeviceCorrelation::AmbiguousPciIdentity => findings.push(finding(
            &format!("gpu.vulkan_correlation_ambiguous.{}", driver.pci_bdf),
                "gpu",
                DiagnosticState::PartialObservation,
                DiagnosticSeverity::Warning,
                DiagnosticConfidence::High,
                format!("Vulkan-to-PCI correlation for {} is ambiguous.", driver.pci_bdf),
                Some("A unique valid Vulkan PCI BDF match."),
                vec![evidence("P013 VK_EXT_pci_bus_info correlation", Some(&driver.pci_bdf), EvidenceStrength::DerivedCorrelation)],
                "No GPU selection was inferred from enumeration order or marketing identity.",
                Some("Inspect the PCI/Vulkan identities and preserve the ambiguity."),
                false,
            )),
            DeviceCorrelation::ConflictingPciIdentity => findings.push(finding(
            &format!("gpu.vulkan_identity_conflict.{}", driver.pci_bdf),
                "gpu",
                DiagnosticState::StaleOrInconsistent,
                DiagnosticSeverity::Warning,
                DiagnosticConfidence::High,
                format!("Vulkan and PCI IDs conflict for {}.", driver.pci_bdf),
                Some("Vendor and device IDs consistent with the exact PCI BDF."),
                vec![evidence("P013 PCI and VK_EXT_pci_bus_info identities", Some(&driver.pci_bdf), EvidenceStrength::DerivedCorrelation)],
                "The conflicting Vulkan device remains unmatched.",
                Some("Reinspect device identity and userspace driver metadata."),
                false,
            )),
            DeviceCorrelation::Unmatched => findings.push(finding(
            &format!("gpu.vulkan_identity_unmatched.{}", driver.pci_bdf),
                "gpu",
                DiagnosticState::UnknownOrUnverifiable,
                DiagnosticSeverity::Information,
                DiagnosticConfidence::Medium,
                format!("No exact Vulkan PCI identity was correlated for {}.", driver.pci_bdf),
                Some("An exact BDF supplied by VK_EXT_pci_bus_info."),
                vec![evidence("P013 PCI/Vulkan identity correlation", Some(&driver.pci_bdf), EvidenceStrength::MissingObservation)],
                "Device order, vendor/device IDs alone, and DRM node indices are not physical identity evidence.",
                Some("Inspect the reported PCI and Vulkan identity evidence."),
                false,
            )),
            _ => {}
        }
    }
}

fn assess_prefix(prefix: &PrefixDiagnostic, findings: &mut Vec<CompatibilityFinding>) {
    if prefix.ownership_state != DiagnosticState::VerifiedObservation {
        findings.push(finding(
            "prefix.ownership_unverified",
            "prefix",
            prefix.ownership_state,
            DiagnosticSeverity::Blocker,
            DiagnosticConfidence::High,
            "Managed prefix ownership could not be validated for the selected record.",
            Some("A registered Endfield-Unix-owned prefix with the expected current user."),
            vec![evidence(
                "P028 ownership and registry validation",
                Some(&prefix.prefix_id),
                EvidenceStrength::ValidatedReference,
            )],
            "No prefix payload or external prefix was adopted or inspected.",
            Some("Inspect the registered managed prefix record and ownership state."),
            true,
        ));
    }
    if prefix.filesystem_state != DiagnosticState::VerifiedObservation {
        findings.push(finding(
            "prefix.filesystem_unverified",
            "prefix",
            prefix.filesystem_state,
            DiagnosticSeverity::Blocker,
            DiagnosticConfidence::High,
            "Managed prefix filesystem identity or lifecycle validation is incomplete.",
            Some("The registered prefix directory matches its recorded no-follow filesystem identity."),
            vec![evidence("P028 selected-prefix inspection", Some(&prefix.prefix_id), EvidenceStrength::DirectObservation)],
            "No unrelated prefix paths or managed prefix payload content were scanned.",
            Some("Inspect the selected managed prefix and any reported recovery state."),
            true,
        ));
    }
    if prefix.lifecycle == PrefixLifecycle::Created
        || prefix.initialization_state != DiagnosticState::VerifiedObservation
    {
        findings.push(finding(
            "prefix.initialization_unverified",
            "prefix",
            DiagnosticState::UnknownOrUnverifiable,
            DiagnosticSeverity::Warning,
            DiagnosticConfidence::High,
            "The managed prefix is recorded as Created or has no verified initialization evidence.",
            Some("Verified Wine initialization evidence for any execution-readiness claim."),
            vec![evidence("P028 lifecycle and initialization evidence", Some(match prefix.lifecycle { PrefixLifecycle::Created => "created; initialization unsupported", _ => "initialization evidence unavailable" }), EvidenceStrength::ValidatedReference)],
            "Created means only that the managed directory exists. P028 initialization_unsupported remains unchanged.",
            Some("Wine initialization remains outside this milestone; use later validated initialization work."),
            true,
        ));
    }
    if prefix.recovery_state != PrefixRecoveryState::None
        && prefix.recovery_state != PrefixRecoveryState::Completed
        && prefix.recovery_state != PrefixRecoveryState::RollbackCompleted
    {
        findings.push(finding(
            "prefix.recovery_pending",
            "prefix",
            if prefix.recovery_state == PrefixRecoveryState::IncompleteOrInconsistent { DiagnosticState::StaleOrInconsistent } else { DiagnosticState::PartialObservation },
            DiagnosticSeverity::Blocker,
            DiagnosticConfidence::High,
            prefix.recovery_diagnostic.as_deref().unwrap_or("An outstanding or incomplete recovery transaction was observed."),
            Some("No unresolved or inconsistent prefix recovery transaction."),
            vec![evidence("P029 metadata-only recovery journal inspection", Some(&format!("{:?}", prefix.recovery_state)), EvidenceStrength::DirectObservation)],
            "Recovery markers block positive readiness conclusions; prefix payload bytes were not inspected.",
            Some("Inspect the existing prefix recovery state before any later qualification."),
            true,
        ));
    }
    if prefix.runtime_association_state != DiagnosticState::VerifiedObservation {
        findings.push(finding(
            "prefix.runtime_association_unverified",
            "prefix",
            prefix.runtime_association_state,
            DiagnosticSeverity::Blocker,
            DiagnosticConfidence::High,
            "The prefix's original runtime association did not pass exact reference validation.",
            Some("Exact associated P025/P026 or P022 runtime identity revalidation."),
            vec![evidence("P028 saved P027 association", prefix.original_pin_id.as_deref(), EvidenceStrength::ValidatedReference)],
            "The original association is retained; a changed or stale pin does not migrate the prefix.",
            Some("Reinspect the saved runtime association and current P027 pin."),
            true,
        ));
    }
    if prefix.current_pin_state == DiagnosticState::StaleOrInconsistent {
        findings.push(finding(
            "prefix.current_pin_stale",
            "prefix",
            prefix.current_pin_state,
            DiagnosticSeverity::Warning,
            DiagnosticConfidence::Medium,
            prefix.current_pin_relation.as_deref().unwrap_or("The current P027 selection is stale."),
            Some("Readable current machine/profile selection without fallback from a broken profile pin."),
            vec![evidence("P027 read-only pin registry", prefix.original_pin_id.as_deref(), EvidenceStrength::ValidatedReference)],
            "Current selection is observational; P027 was not modified.",
            Some("Inspect the applicable pin scope and stale-reference details."),
            true,
        ));
    }
    if prefix.current_pin_state == DiagnosticState::UnknownOrUnverifiable {
        findings.push(finding(
            "prefix.current_pin_unavailable",
            "prefix",
            prefix.current_pin_state,
            DiagnosticSeverity::Warning,
            DiagnosticConfidence::Low,
            prefix.current_pin_relation.as_deref().unwrap_or("The current P027 pin relation could not be established."),
            Some("A readable current machine/profile selection."),
            vec![evidence("P027 read-only pin registry", prefix.original_pin_id.as_deref(), EvidenceStrength::MissingObservation)],
            "The saved runtime association remains unchanged; the current selection could not be assessed.",
            Some("Check the read-only P027 registry and machine/profile identity."),
            true,
        ));
    }
    if prefix.current_pin_state == DiagnosticState::PartialObservation {
        findings.push(finding(
            "prefix.current_pin_changed",
            "prefix",
            prefix.current_pin_state,
            DiagnosticSeverity::Warning,
            DiagnosticConfidence::High,
            prefix.current_pin_relation.as_deref().unwrap_or("The current P027 selection differs from the saved prefix association."),
            Some("The current selection matches the saved P028 runtime and adapter association."),
            vec![evidence("P027 current selection and P028 saved association", prefix.original_pin_id.as_deref(), EvidenceStrength::DerivedCorrelation)],
            "P028 preserves the original runtime association; changing a pin does not migrate a prefix.",
            Some("Inspect the saved runtime association and current P027 pin."),
            false,
        ));
    }
}

fn assess_cross_component(
    observations: &CompatibilityObservations,
    findings: &mut Vec<CompatibilityFinding>,
) {
    if observations.runner.selection.state == RuntimeSelectionState::Selected
        && observations.vulkan.loader_state == VulkanLoaderState::Missing
    {
        findings.push(finding(
            "cross.runtime_valid_vulkan_missing",
            "cross_component",
            DiagnosticState::UnknownOrUnverifiable,
            DiagnosticSeverity::Warning,
            DiagnosticConfidence::High,
            "The selected runtime reference is available, while the Vulkan loader could not be observed.",
            Some("Runtime identity and Vulkan/translation capabilities must be assessed independently."),
            vec![evidence("P027 runtime selection", observations.runner.runtime.as_ref().map(|runtime| runtime.exact_identity.as_str()), EvidenceStrength::ValidatedReference), evidence("P013 Vulkan provider", None, EvidenceStrength::MissingObservation)],
            "Vulkan absence blocks only the observation of Vulkan availability; it does not prove game incompatibility or trigger runtime fallback.",
            Some("Check loader and userspace driver installation, then perform later real-device qualification."),
            true,
        ));
    }
    if observations.runner.selection.state == RuntimeSelectionState::Stale
        && observations.vulkan.loader_state == VulkanLoaderState::Enumerated
    {
        findings.push(finding(
            "cross.vulkan_does_not_repair_stale_runtime",
            "cross_component",
            DiagnosticState::StaleOrInconsistent,
            DiagnosticSeverity::Blocker,
            DiagnosticConfidence::High,
            "Vulkan physical devices were observed, but the selected runtime reference is stale.",
            Some("The explicitly selected runtime reference must independently revalidate."),
            vec![
                evidence(
                    "P013 Vulkan physical devices",
                    Some("enumerated"),
                    EvidenceStrength::DirectObservation,
                ),
                evidence(
                    "P027 selected runtime",
                    observations.runner.selection.diagnostic.as_deref(),
                    EvidenceStrength::ValidatedReference,
                ),
            ],
            "Vulkan availability does not repair runner identity. No fallback was performed.",
            Some("Inspect the stale P027 reference."),
            true,
        ));
    }
    let prefix_ready = observations.prefix.as_ref().is_some_and(|prefix| {
        prefix.lifecycle == PrefixLifecycle::Initialized
            && prefix.ownership_state == DiagnosticState::VerifiedObservation
            && prefix.filesystem_state == DiagnosticState::VerifiedObservation
            && prefix.runtime_association_state == DiagnosticState::VerifiedObservation
            && prefix.current_pin_state == DiagnosticState::VerifiedObservation
            && prefix.initialization_state == DiagnosticState::VerifiedObservation
            && prefix.recovery_state == PrefixRecoveryState::None
    });
    let static_observations_ready = observations.runner.selection.state
        == RuntimeSelectionState::Selected
        && observations.runner.identity_state == DiagnosticState::VerifiedObservation
        && observations.runner.integrity_state == DiagnosticState::VerifiedObservation
        && observations.runner.adapter_state == DiagnosticState::StaticCapabilityAvailable
        && observations.gpu_inventory_state == DiagnosticState::VerifiedObservation
        && observations.drm_inventory_state == DiagnosticState::VerifiedObservation
        && observations.unmatched_drm_nodes.is_empty()
        && observations.vulkan.loader_state == VulkanLoaderState::Enumerated
        && !observations.vulkan.devices.is_empty()
        && !observations.drivers.is_empty()
        && observations.drivers.iter().all(|driver| {
            driver.kernel_driver_state == DiagnosticState::VerifiedObservation
                && !driver
                    .drm_nodes
                    .iter()
                    .any(|node| node.access == DeviceNodeAccess::Denied)
                && driver.drm_nodes.iter().any(|node| {
                    node.kind == DrmNodeKind::Render && node.access == DeviceNodeAccess::Accessible
                })
                && driver.vulkan_correlation == DeviceCorrelation::ExactPciIdentity
        });
    if observations.runner.selection.state == RuntimeSelectionState::Selected
        && observations.vulkan.loader_state == VulkanLoaderState::Enumerated
        && !prefix_ready
    {
        findings.push(finding(
            "cross.static_prerequisites_incomplete",
            "cross_component",
            DiagnosticState::PartialObservation,
            DiagnosticSeverity::Information,
            DiagnosticConfidence::High,
            "Some runtime and Vulkan observations are available, but prefix initialization, association, or recovery evidence is incomplete.",
            None,
            vec![evidence("P027 runtime and P013 Vulkan observations", Some("runtime selected; Vulkan devices enumerated"), EvidenceStrength::DerivedCorrelation)],
            "No known static blocker from these observations would still not prove that Endfield launches or renders correctly.",
            Some("Complete later scoped prefix and real-device qualification."),
            true,
        ));
    }
    if static_observations_ready && prefix_ready {
        findings.push(finding(
            "cross.no_known_static_blocker",
            "cross_component",
            DiagnosticState::StaticCapabilityAvailable,
            DiagnosticSeverity::Information,
            DiagnosticConfidence::High,
            "The checked static diagnostic prerequisites were observed and no known blocker was found in those observations.",
            None,
            vec![evidence("P027/P028/P029 and P013 observations", Some("selected runtime, associated initialized prefix, and Vulkan devices observed"), EvidenceStrength::DerivedCorrelation)],
            "Wine initialization evidence and static Vulkan capabilities do not prove successful Endfield launch or rendering.",
            Some("Perform later scoped real-device qualification."),
            false,
        ));
    }
}

fn determine_verdict(findings: &[CompatibilityFinding]) -> DiagnosticVerdict {
    if findings
        .iter()
        .any(|finding| finding.severity == DiagnosticSeverity::Blocker)
    {
        DiagnosticVerdict::BlockedByObservedFacts
    } else if findings.iter().any(|finding| {
        matches!(
            finding.state,
            DiagnosticState::MissingRequirement
                | DiagnosticState::UnsupportedConfiguration
                | DiagnosticState::StaleOrInconsistent
                | DiagnosticState::AccessDenied
                | DiagnosticState::PartialObservation
                | DiagnosticState::UnknownOrUnverifiable
                | DiagnosticState::NotTested
        )
    }) {
        DiagnosticVerdict::IncompleteObservations
    } else if findings.is_empty() {
        DiagnosticVerdict::Unknown
    } else {
        DiagnosticVerdict::NoKnownStaticBlocker
    }
}

fn evidence(
    source: &str,
    observed_value: Option<&str>,
    strength: EvidenceStrength,
) -> DiagnosticEvidence {
    DiagnosticEvidence {
        source: source.into(),
        observed_value: observed_value.map(str::to_owned),
        strength,
    }
}

#[allow(clippy::too_many_arguments)]
fn finding(
    id: &str,
    component: &str,
    state: DiagnosticState,
    severity: DiagnosticSeverity,
    confidence: DiagnosticConfidence,
    reason: impl Into<String>,
    expected_condition: Option<&str>,
    evidence: Vec<DiagnosticEvidence>,
    evidence_limitations: &str,
    action: Option<&str>,
    blocks_later_launch_qualification: bool,
) -> CompatibilityFinding {
    let action = action.map(str::to_owned);
    CompatibilityFinding {
        id: id.into(),
        component: component.into(),
        state,
        severity,
        confidence,
        evidence,
        expected_condition: expected_condition.map(str::to_owned),
        reason: reason.into(),
        evidence_limitations: evidence_limitations.into(),
        suggested_remediation: action.map(|action| SuggestedRemediation {
            supported_by_endfield_unix: matches!(
                action.as_str(),
                "Inspect available runtimes and select a runtime explicitly."
                    | "Inspect the selected pin and revalidate its exact runtime reference."
                    | "Check the read-only P027 registry and machine/profile identity."
                    | "Reinspect the exact selected runtime and its recorded provenance."
                    | "Inspect the stored adapter preference; adapter setup is outside this read-only diagnostic."
                    | "Use exact PCI/Vulkan correlation; do not infer or set a default GPU."
                    | "Inspect the PCI/Vulkan identities and preserve the ambiguity."
                    | "Inspect the reported PCI and Vulkan identity evidence."
                    | "Inspect the existing prefix recovery state before any later qualification."
                    | "Reinspect the saved runtime association and current P027 pin."
                    | "Inspect the applicable pin scope and stale-reference details."
                    | "Inspect the stale P027 reference."
            ),
            action,
        }),
        blocks_later_launch_qualification,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> CompatibilityObservations {
        CompatibilityObservations {
            observed_at_unix_seconds: 1,
            target: CompatibilityTarget {
                game: "Arknights: Endfield".into(),
                profile_scope: None,
                prefix_id: None,
            },
            platform: PlatformSummary {
                os: "Linux".into(),
                distribution: None,
                distribution_version: None,
                kernel_release: None,
                architecture: Some("x86_64".into()),
                environment: None,
            },
            gpu_inventory_state: DiagnosticState::UnknownOrUnverifiable,
            drm_inventory_state: DiagnosticState::UnknownOrUnverifiable,
            runner: RunnerDiagnostic {
                selection: RuntimeSelectionDiagnostic {
                    state: RuntimeSelectionState::NoSelection,
                    source: None,
                    pin_id: None,
                    pin_revision: None,
                    registry_revision: Some(0),
                    automatic_fallback: false,
                    diagnostic: None,
                },
                runtime: None,
                installation_state: DiagnosticState::UnknownOrUnverifiable,
                identity_state: DiagnosticState::UnknownOrUnverifiable,
                integrity_state: DiagnosticState::UnknownOrUnverifiable,
                adapter_preference: None,
                adapter_state: DiagnosticState::NotApplicable,
                adapter_diagnostic: None,
            },
            drivers: vec![],
            unmatched_drm_nodes: vec![],
            vulkan: VulkanDiagnostic {
                loader_state: VulkanLoaderState::Missing,
                loader_api_version: None,
                devices: vec![],
                observation_state: DiagnosticState::MissingRequirement,
                diagnostic: None,
            },
            prefix: None,
        }
    }

    #[test]
    fn empty_host_is_valid_partial_and_does_not_invent_devices_or_runtimes() {
        let report = assess(base());
        assert_eq!(report.schema_version, 1);
        assert_eq!(report.verdict, DiagnosticVerdict::IncompleteObservations);
        assert_eq!(
            report.runner.selection.state,
            RuntimeSelectionState::NoSelection
        );
        assert!(report.runner.runtime.is_none());
        assert!(report.drivers.is_empty());
        assert!(report.vulkan.devices.is_empty());
        assert_eq!(report.game_compatibility, GameCompatibilityState::NotTested);
        assert_eq!(
            report.translation_layer_readiness,
            DiagnosticState::UnknownOrUnverifiable
        );
    }

    #[test]
    fn vulkan_devices_do_not_repair_stale_runtime_selection() {
        let mut observations = base();
        observations.runner.selection.state = RuntimeSelectionState::Stale;
        observations.runner.selection.diagnostic = Some("P026 baseline mismatch".into());
        observations.vulkan.loader_state = VulkanLoaderState::Enumerated;
        observations.vulkan.observation_state = DiagnosticState::StaticCapabilityAvailable;
        let report = assess(observations);
        assert_eq!(report.verdict, DiagnosticVerdict::BlockedByObservedFacts);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.id == "cross.vulkan_does_not_repair_stale_runtime")
        );
        assert!(
            report
                .findings
                .iter()
                .all(|finding| !finding.id.contains("fallback"))
        );
    }

    #[test]
    fn valid_runtime_with_missing_vulkan_stays_unknown_about_rendering() {
        let mut observations = base();
        observations.runner.selection.state = RuntimeSelectionState::Selected;
        observations.runner.identity_state = DiagnosticState::VerifiedObservation;
        observations.runner.integrity_state = DiagnosticState::VerifiedObservation;
        observations.runner.runtime = Some(RuntimeIdentity {
            exact_identity: "proton-fixture".into(),
            family: RuntimeFamily::Proton,
            build: Some("test".into()),
            origin: RuntimeOrigin::Managed,
            ownership: "managed".into(),
            provenance: "user_pinned_limited_trust".into(),
            source: "P025".into(),
        });
        let report = assess(observations);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.id == "cross.runtime_valid_vulkan_missing")
        );
        assert_eq!(report.game_compatibility, GameCompatibilityState::NotTested);
    }

    #[test]
    fn created_prefix_is_not_wine_initialized_or_ready() {
        let mut observations = base();
        observations.prefix = Some(PrefixDiagnostic {
            prefix_id: "game".into(),
            scope: "profile".into(),
            lifecycle: PrefixLifecycle::Created,
            ownership_state: DiagnosticState::VerifiedObservation,
            filesystem_state: DiagnosticState::VerifiedObservation,
            runtime_association_state: DiagnosticState::VerifiedObservation,
            original_pin_id: Some("pin-1".into()),
            original_pin_revision: Some(1),
            adapter_preference: Some("umu_proton".into()),
            current_pin_state: DiagnosticState::VerifiedObservation,
            current_pin_relation: Some("same identity".into()),
            initialization_state: DiagnosticState::UnknownOrUnverifiable,
            initialization_evidence_available: false,
            recovery_state: PrefixRecoveryState::None,
            recovery_diagnostic: None,
        });
        let report = assess(observations);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.id == "prefix.initialization_unverified"
                    && finding.blocks_later_launch_qualification)
        );
    }

    #[test]
    fn findings_are_stable_and_arrays_are_bounded() {
        let mut observations = base();
        observations.drivers = (0..MAX_DEVICES + 5)
            .map(|index| DriverDiagnostic {
                pci_bdf: format!("0000:{index:02x}:00.0"),
                vendor_id: 0x8086,
                device_id: 1,
                vendor: "Intel".into(),
                gpu_class: None,
                kernel_driver: None,
                kernel_driver_state: DiagnosticState::UnknownOrUnverifiable,
                kernel_driver_version: None,
                drm_nodes: vec![],
                vulkan_correlation: DeviceCorrelation::Unknown,
            })
            .collect();
        let first = assess(observations.clone());
        let second = assess(observations);
        assert_eq!(first, second);
        assert_eq!(first.drivers.len(), MAX_DEVICES);
        assert!(first.findings.len() <= MAX_FINDINGS);
    }

    fn selected_runner(adapter_state: DiagnosticState) -> RunnerDiagnostic {
        RunnerDiagnostic {
            selection: RuntimeSelectionDiagnostic {
                state: RuntimeSelectionState::Selected,
                source: Some("profile_pin".into()),
                pin_id: Some("pin-fixture".into()),
                pin_revision: None,
                registry_revision: Some(3),
                automatic_fallback: false,
                diagnostic: None,
            },
            runtime: Some(RuntimeIdentity {
                exact_identity: "external:p022-structural:fixture".into(),
                family: RuntimeFamily::Proton,
                build: Some("fixture-build".into()),
                origin: RuntimeOrigin::External,
                ownership: "external-user-managed".into(),
                provenance: "external source authenticity unverified".into(),
                source: "explicit-root".into(),
            }),
            installation_state: DiagnosticState::VerifiedObservation,
            identity_state: DiagnosticState::VerifiedObservation,
            integrity_state: DiagnosticState::VerifiedObservation,
            adapter_preference: Some("umu_proton".into()),
            adapter_state,
            adapter_diagnostic: None,
        }
    }

    fn initialized_prefix(recovery_state: PrefixRecoveryState) -> PrefixDiagnostic {
        PrefixDiagnostic {
            prefix_id: "fixture".into(),
            scope: "profile".into(),
            lifecycle: PrefixLifecycle::Initialized,
            ownership_state: DiagnosticState::VerifiedObservation,
            filesystem_state: DiagnosticState::VerifiedObservation,
            runtime_association_state: DiagnosticState::VerifiedObservation,
            original_pin_id: Some("pin-fixture".into()),
            original_pin_revision: Some(2),
            adapter_preference: Some("umu_proton".into()),
            current_pin_state: DiagnosticState::VerifiedObservation,
            current_pin_relation: Some("same identity".into()),
            initialization_state: DiagnosticState::VerifiedObservation,
            initialization_evidence_available: true,
            recovery_state,
            recovery_diagnostic: None,
        }
    }

    fn vulkan_device(correlation: DeviceCorrelation) -> VulkanDeviceDiagnostic {
        VulkanDeviceDiagnostic {
            name: "Fixture GPU".into(),
            vendor_id: 0x8086,
            device_id: 7,
            pci_bdf: Some("0000:00:02.0".into()),
            correlation,
            device_type: VulkanDeviceType::Integrated,
            api_version: Some((1, 3, 280)),
            driver_id: Some(1),
            driver_name: Some("fixture-driver".into()),
            driver_info: Some("source-reported build 1.2".into()),
            extension_state: VulkanMetadataState::Available,
            supported_extensions: vec!["VK_KHR_swapchain".into()],
            feature_state: VulkanMetadataState::Available,
            supported_features: vec!["geometry_shader".into()],
        }
    }

    fn correlated_driver() -> DriverDiagnostic {
        DriverDiagnostic {
            pci_bdf: "0000:00:02.0".into(),
            vendor_id: 0x8086,
            device_id: 7,
            vendor: "Intel".into(),
            gpu_class: Some("integrated".into()),
            kernel_driver: Some("i915".into()),
            kernel_driver_state: DiagnosticState::VerifiedObservation,
            kernel_driver_version: None,
            drm_nodes: vec![DrmNodeDiagnostic {
                name: "renderD128".into(),
                kind: DrmNodeKind::Render,
                access: DeviceNodeAccess::Accessible,
                pci_bdf: Some("0000:00:02.0".into()),
            }],
            vulkan_correlation: DeviceCorrelation::ExactPciIdentity,
        }
    }

    #[test]
    fn driver_and_hybrid_gpu_findings_preserve_ambiguity_and_access_limits() {
        let mut observations = base();
        observations.gpu_inventory_state = DiagnosticState::VerifiedObservation;
        observations.drm_inventory_state = DiagnosticState::VerifiedObservation;
        let mut intel = correlated_driver();
        intel.kernel_driver_version = None;
        let mut nvidia = correlated_driver();
        nvidia.pci_bdf = "0001:01:00.0".into();
        nvidia.vendor_id = 0x10de;
        nvidia.vendor = "NVIDIA".into();
        nvidia.kernel_driver = None;
        nvidia.kernel_driver_state = DiagnosticState::MissingRequirement;
        nvidia.drm_nodes[0].name = "renderD129".into();
        nvidia.drm_nodes[0].access = DeviceNodeAccess::Denied;
        nvidia.vulkan_correlation = DeviceCorrelation::AmbiguousPciIdentity;
        observations.drivers = vec![intel, nvidia];
        observations.vulkan.loader_state = VulkanLoaderState::Enumerated;
        observations.vulkan.devices = vec![vulkan_device(DeviceCorrelation::AmbiguousPciIdentity)];

        let report = assess(observations);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.id == "gpu.multiple_devices_observed")
        );
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.id.starts_with("gpu.kernel_driver_unbound."))
        );
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.id.starts_with("gpu.drm_access_denied."))
        );
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.id.starts_with("gpu.vulkan_correlation_ambiguous."))
        );
        assert_eq!(report.drivers[0].kernel_driver_version, None);
        assert_eq!(
            report.drivers[1].vulkan_correlation,
            DeviceCorrelation::AmbiguousPciIdentity
        );
        assert_eq!(report.game_compatibility, GameCompatibilityState::NotTested);
    }

    #[test]
    fn drm_nodes_without_pci_correlation_are_retained_as_partial_evidence() {
        let mut observations = base();
        observations.drm_inventory_state = DiagnosticState::PartialObservation;
        observations.unmatched_drm_nodes = vec![DrmNodeDiagnostic {
            name: "renderD129".into(),
            kind: DrmNodeKind::Render,
            access: DeviceNodeAccess::Denied,
            pci_bdf: None,
        }];
        let report = assess(observations);
        assert_eq!(report.unmatched_drm_nodes.len(), 1);
        assert!(report.findings.iter().any(|finding| {
            finding
                .id
                .starts_with("gpu.drm_node_identity_unmatched.renderD129")
                && finding.state == DiagnosticState::AccessDenied
        }));
    }

    #[test]
    fn vulkan_metadata_and_driver_reported_values_are_preserved_without_dx_inference() {
        let mut observations = base();
        observations.vulkan.loader_state = VulkanLoaderState::Enumerated;
        observations.vulkan.observation_state = DiagnosticState::StaticCapabilityAvailable;
        observations.vulkan.devices = vec![vulkan_device(DeviceCorrelation::ExactPciIdentity)];
        let report = assess(observations);
        let device = &report.vulkan.devices[0];
        assert_eq!(device.api_version, Some((1, 3, 280)));
        assert_eq!(
            device.driver_info.as_deref(),
            Some("source-reported build 1.2")
        );
        assert_eq!(device.supported_extensions, ["VK_KHR_swapchain"]);
        assert_eq!(device.supported_features, ["geometry_shader"]);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.id == "vulkan.physical_devices_observed")
        );
        assert!(
            report
                .compatibility_limitations
                .iter()
                .any(|limit| limit.contains("Direct3D translation"))
        );
    }

    #[test]
    fn vulkan_failure_states_remain_partial_and_do_not_claim_game_requirements() {
        for (loader_state, finding_id) in [
            (VulkanLoaderState::Missing, "vulkan.loader_unavailable"),
            (
                VulkanLoaderState::InitializationFailed,
                "vulkan.instance_initialization_failed",
            ),
            (
                VulkanLoaderState::AvailableNoDevices,
                "vulkan.no_physical_devices",
            ),
            (VulkanLoaderState::Partial, "vulkan.observation_partial"),
            (VulkanLoaderState::Unknown, "vulkan.observation_partial"),
        ] {
            let mut observations = base();
            observations.vulkan.loader_state = loader_state;
            let report = assess(observations);
            assert!(
                report
                    .findings
                    .iter()
                    .any(|finding| finding.id == finding_id)
            );
            assert_ne!(report.verdict, DiagnosticVerdict::NoKnownStaticBlocker);
            assert_eq!(report.game_compatibility, GameCompatibilityState::NotTested);
        }
    }

    #[test]
    fn unresolved_prefix_recovery_blocks_positive_readiness() {
        for recovery in [
            PrefixRecoveryState::RollbackPending,
            PrefixRecoveryState::PendingDeletion,
            PrefixRecoveryState::PendingPinRecovery,
            PrefixRecoveryState::IncompleteOrInconsistent,
        ] {
            let mut observations = base();
            observations.prefix = Some(initialized_prefix(recovery));
            let report = assess(observations);
            assert_eq!(report.verdict, DiagnosticVerdict::BlockedByObservedFacts);
            assert!(
                report
                    .findings
                    .iter()
                    .any(|finding| finding.id == "prefix.recovery_pending"
                        && finding.severity == DiagnosticSeverity::Blocker)
            );
            assert!(
                !report
                    .findings
                    .iter()
                    .any(|finding| finding.id == "cross.no_known_static_blocker")
            );
        }
    }

    #[test]
    fn changed_current_pin_is_reported_without_rewriting_saved_prefix_association() {
        let mut observations = base();
        let mut prefix = initialized_prefix(PrefixRecoveryState::None);
        prefix.current_pin_state = DiagnosticState::PartialObservation;
        prefix.current_pin_relation =
            Some("current P027 selection changed; original association retained".into());
        observations.prefix = Some(prefix);
        let report = assess(observations);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.id == "prefix.current_pin_changed")
            .unwrap();
        assert_eq!(finding.severity, DiagnosticSeverity::Warning);
        assert!(!finding.blocks_later_launch_qualification);
    }

    #[test]
    fn no_known_static_blocker_requires_complete_static_facts_and_matching_initialized_prefix() {
        let mut observations = base();
        observations.runner = selected_runner(DiagnosticState::StaticCapabilityAvailable);
        observations.gpu_inventory_state = DiagnosticState::VerifiedObservation;
        observations.drm_inventory_state = DiagnosticState::VerifiedObservation;
        observations.drivers = vec![correlated_driver()];
        observations.vulkan.loader_state = VulkanLoaderState::Enumerated;
        observations.vulkan.devices = vec![vulkan_device(DeviceCorrelation::ExactPciIdentity)];
        observations.prefix = Some(initialized_prefix(PrefixRecoveryState::None));
        let report = assess(observations);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.id == "cross.no_known_static_blocker")
        );
        assert_eq!(report.game_compatibility, GameCompatibilityState::NotTested);

        let mut partial = base();
        partial.runner = selected_runner(DiagnosticState::UnknownOrUnverifiable);
        partial.vulkan.loader_state = VulkanLoaderState::Enumerated;
        partial.prefix = Some(initialized_prefix(PrefixRecoveryState::None));
        assert!(
            !assess(partial)
                .findings
                .iter()
                .any(|finding| finding.id == "cross.no_known_static_blocker")
        );

        let mut inaccessible_render_node = base();
        inaccessible_render_node.runner =
            selected_runner(DiagnosticState::StaticCapabilityAvailable);
        inaccessible_render_node.gpu_inventory_state = DiagnosticState::VerifiedObservation;
        inaccessible_render_node.drm_inventory_state = DiagnosticState::VerifiedObservation;
        let mut driver = correlated_driver();
        driver.drm_nodes[0].access = DeviceNodeAccess::Denied;
        inaccessible_render_node.drivers = vec![driver];
        inaccessible_render_node.vulkan.loader_state = VulkanLoaderState::Enumerated;
        inaccessible_render_node.vulkan.devices =
            vec![vulkan_device(DeviceCorrelation::ExactPciIdentity)];
        inaccessible_render_node.prefix = Some(initialized_prefix(PrefixRecoveryState::None));
        assert!(
            !assess(inaccessible_render_node)
                .findings
                .iter()
                .any(|finding| finding.id == "cross.no_known_static_blocker")
        );
    }

    #[test]
    fn inconsistent_vulkan_results_and_missing_metadata_are_explicit() {
        let mut inconsistent = base();
        inconsistent.vulkan.loader_state = VulkanLoaderState::Enumerated;
        let report = assess(inconsistent);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.id == "vulkan.status_metadata_inconsistent")
        );

        let mut metadata_missing = base();
        metadata_missing.vulkan.loader_state = VulkanLoaderState::Enumerated;
        let mut software = vulkan_device(DeviceCorrelation::Unmatched);
        software.device_type = VulkanDeviceType::Cpu;
        software.extension_state = VulkanMetadataState::Unavailable;
        software.feature_state = VulkanMetadataState::Partial;
        software.driver_id = None;
        software.driver_name = None;
        software.supported_extensions.clear();
        metadata_missing.vulkan.devices = vec![software];
        let report = assess(metadata_missing);
        for prefix in [
            "vulkan.software_device_observed.",
            "vulkan.extension_metadata_partial.",
            "vulkan.feature_metadata_partial.",
            "vulkan.device_identity_unmatched.",
            "vulkan.driver_properties_unavailable.",
        ] {
            assert!(
                report
                    .findings
                    .iter()
                    .any(|finding| finding.id.starts_with(prefix)),
                "missing finding {prefix}"
            );
        }
    }
}
