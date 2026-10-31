use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use endfield_unix_compatibility_diagnostics as diagnostics;
use endfield_unix_domain::{Availability, GpuClass};
use endfield_unix_platform_linux::{
    DeviceNodeAccess as LinuxNodeAccess, DrmNodeKind as LinuxNodeKind, GpuSourceStatus,
    LinuxGpuProbe, LinuxGpuReport, LinuxPlatformProbe, VulkanDeviceType as LinuxVulkanDeviceType,
    VulkanLoaderStatus, VulkanMapping, VulkanMetadataStatus,
};
use endfield_unix_prefix_management as prefixes;
use endfield_unix_runtime_selection as pins;
use endfield_unix_storage::AppPaths;

const MAX_JSON_BYTES: usize = 1024 * 1024;
const MAX_HUMAN_BYTES: usize = 64 * 1024;
const MAX_HUMAN_FINDINGS: usize = 48;

#[derive(Debug)]
pub enum CompatibilityError {
    InvalidProfile,
    InvalidPrefixId,
    Serialization,
    Output(io::Error),
    OutputLimit,
}

impl CompatibilityError {
    pub fn exit_status(&self) -> u8 {
        match self {
            Self::InvalidProfile | Self::InvalidPrefixId => 2,
            Self::Output(_) => 74,
            Self::Serialization | Self::OutputLimit => 70,
        }
    }
}

impl std::fmt::Display for CompatibilityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidProfile => "compatibility diagnostic profile identifier is invalid.",
            Self::InvalidPrefixId => "compatibility diagnostic prefix identifier is invalid.",
            Self::Serialization => "compatibility diagnostic report could not be serialized.",
            Self::Output(_) => "failed to write compatibility diagnostic output.",
            Self::OutputLimit => "compatibility diagnostic output exceeded its configured limit.",
        })
    }
}

impl std::error::Error for CompatibilityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Output(error) => Some(error),
            _ => None,
        }
    }
}

pub fn run(
    profile: Option<String>,
    prefix_id: Option<String>,
    json: bool,
    output: &mut impl Write,
) -> Result<(), CompatibilityError> {
    let profile_key = profile
        .as_ref()
        .map(|value| pins::ProfileKey::new(value.clone()))
        .transpose()
        .map_err(|_| CompatibilityError::InvalidProfile)?;
    let prefix_key = prefix_id
        .as_ref()
        .map(|value| prefixes::PrefixId::new(value.clone()))
        .transpose()
        .map_err(|_| CompatibilityError::InvalidPrefixId)?;

    let paths = AppPaths::discover().ok();
    let machine = pins::current_machine_id().ok();
    let (runner, registry) = collect_runner(paths.as_ref(), machine.as_ref(), profile_key.as_ref());
    let (platform, gpu_inventory_state, drm_inventory_state, drivers, unmatched_drm_nodes, vulkan) =
        collect_platform_and_graphics();
    let prefix = prefix_key
        .as_ref()
        .map(|id| collect_prefix(id, paths.as_ref(), machine.as_ref(), registry.as_ref()));

    let observations = diagnostics::CompatibilityObservations {
        observed_at_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        target: diagnostics::CompatibilityTarget {
            game: "Arknights: Endfield".into(),
            profile_scope: profile,
            prefix_id,
        },
        platform,
        runner,
        gpu_inventory_state,
        drm_inventory_state,
        drivers,
        unmatched_drm_nodes,
        vulkan,
        prefix,
    };
    let mut report = diagnostics::assess(observations);
    bound_report(&mut report);
    let bytes = if json {
        serde_json::to_vec_pretty(&report).map_err(|_| CompatibilityError::Serialization)?
    } else {
        render_human(&report)
    };
    let limit = if json {
        MAX_JSON_BYTES
    } else {
        MAX_HUMAN_BYTES
    };
    if bytes.len() > limit {
        return Err(CompatibilityError::OutputLimit);
    }
    output
        .write_all(&bytes)
        .map_err(CompatibilityError::Output)?;
    Ok(())
}

fn collect_runner(
    paths: Option<&AppPaths>,
    machine: Option<&pins::MachineId>,
    profile: Option<&pins::ProfileKey>,
) -> (diagnostics::RunnerDiagnostic, Option<pins::PinRegistry>) {
    let selection_unknown = |reason: &str| {
        (
            diagnostics::RunnerDiagnostic {
                selection: diagnostics::RuntimeSelectionDiagnostic {
                    state: diagnostics::RuntimeSelectionState::Unknown,
                    source: None,
                    pin_id: None,
                    pin_revision: None,
                    registry_revision: None,
                    automatic_fallback: false,
                    diagnostic: Some(reason.into()),
                },
                runtime: None,
                installation_state: diagnostics::DiagnosticState::UnknownOrUnverifiable,
                identity_state: diagnostics::DiagnosticState::UnknownOrUnverifiable,
                integrity_state: diagnostics::DiagnosticState::UnknownOrUnverifiable,
                adapter_preference: None,
                adapter_state: diagnostics::DiagnosticState::UnknownOrUnverifiable,
                adapter_diagnostic: Some(reason.into()),
            },
            None,
        )
    };
    let (Some(paths), Some(machine)) = (paths, machine) else {
        return selection_unknown(
            "application paths or pseudonymous machine identity are unavailable",
        );
    };
    let store = pins::PinStore::for_app(paths);
    let registry = match store.load_read_only() {
        Ok(registry) => registry,
        Err(error) => {
            return selection_unknown(&format!("P027 read-only registry unavailable: {error}"));
        }
    };
    let report = pins::resolve(
        &registry,
        machine,
        profile,
        None,
        super::runtime_pin::validate_reference,
    );
    let source = selection_source(report.source).map(str::to_owned);
    let pin = report
        .pin_id
        .as_ref()
        .and_then(|pin_id| registry.pins.iter().find(|pin| &pin.pin_id == pin_id));
    let adapter_preference = pin
        .and_then(|pin| pin.adapter_preference)
        .map(|preference| format!("{preference:?}"));
    let runtime = report.runtime.as_ref().map(runtime_identity);
    let (selection_state, diagnostic) = match &report.state {
        pins::ResolutionState::NoPin => (diagnostics::RuntimeSelectionState::NoSelection, None),
        pins::ResolutionState::Stale(reason) => (
            diagnostics::RuntimeSelectionState::Stale,
            Some(reason.chars().take(256).collect::<String>()),
        ),
        pins::ResolutionState::Selected => (diagnostics::RuntimeSelectionState::Selected, None),
    };
    let mut adapter_state = diagnostics::DiagnosticState::NotApplicable;
    let mut adapter_diagnostic = None;
    if selection_state == diagnostics::RuntimeSelectionState::Selected {
        match pin.and_then(|pin| pin.adapter_preference) {
            Some(pins::AdapterPreference::UmuProton) => {
                if runtime
                    .as_ref()
                    .is_some_and(|runtime| runtime.family == diagnostics::RuntimeFamily::Proton)
                {
                    let observation = endfield_unix_runtime_linux::UmuProvider::default().scan();
                    if observation.discovery.validated().next().is_some() {
                        adapter_state = diagnostics::DiagnosticState::StaticCapabilityAvailable;
                        adapter_diagnostic = Some(
                            "UMU executable structure was observed; UMU execution and runtime/game compatibility were not tested.".into(),
                        );
                    } else {
                        adapter_state = diagnostics::DiagnosticState::MissingRequirement;
                        adapter_diagnostic = Some(format!(
                            "UMU adapter was not validated ({}). No direct-Proton fallback was selected.",
                            observation.discovery.outcome_label()
                        ));
                    }
                } else {
                    adapter_state = diagnostics::DiagnosticState::UnsupportedConfiguration;
                    adapter_diagnostic = Some(
                        "UMU-Proton was selected for a non-Proton or unidentified runtime.".into(),
                    );
                }
            }
            Some(pins::AdapterPreference::ManagedWine)
            | Some(pins::AdapterPreference::ExternalWine) => {
                adapter_state = diagnostics::DiagnosticState::UnsupportedConfiguration;
                adapter_diagnostic = Some(
                    "The P028 Wine initialization adapter remains unsupported; no Wine process was executed.".into(),
                );
            }
            None => {
                adapter_state = diagnostics::DiagnosticState::UnknownOrUnverifiable;
                adapter_diagnostic =
                    Some("No adapter preference is recorded for the selected runtime.".into());
            }
        }
    }
    let (installation_state, identity_state, integrity_state) = match report.state {
        pins::ResolutionState::Selected => {
            let integrity = match report.runtime.as_ref() {
                Some(pins::PinnedRuntimeReference::Managed(_)) => {
                    diagnostics::DiagnosticState::VerifiedObservation
                }
                Some(pins::PinnedRuntimeReference::External(_)) => {
                    diagnostics::DiagnosticState::UnknownOrUnverifiable
                }
                None => diagnostics::DiagnosticState::UnknownOrUnverifiable,
            };
            (
                diagnostics::DiagnosticState::VerifiedObservation,
                diagnostics::DiagnosticState::VerifiedObservation,
                integrity,
            )
        }
        pins::ResolutionState::Stale(_) => (
            diagnostics::DiagnosticState::StaleOrInconsistent,
            diagnostics::DiagnosticState::StaleOrInconsistent,
            diagnostics::DiagnosticState::UnknownOrUnverifiable,
        ),
        pins::ResolutionState::NoPin => (
            diagnostics::DiagnosticState::UnknownOrUnverifiable,
            diagnostics::DiagnosticState::UnknownOrUnverifiable,
            diagnostics::DiagnosticState::UnknownOrUnverifiable,
        ),
    };
    (
        diagnostics::RunnerDiagnostic {
            selection: diagnostics::RuntimeSelectionDiagnostic {
                state: selection_state,
                source,
                pin_id: report.pin_id,
                pin_revision: None,
                registry_revision: Some(report.registry_revision),
                automatic_fallback: false,
                diagnostic,
            },
            runtime,
            installation_state,
            identity_state,
            integrity_state,
            adapter_preference,
            adapter_state,
            adapter_diagnostic,
        },
        Some(registry),
    )
}

fn selection_source(source: pins::SelectionSource) -> Option<&'static str> {
    match source {
        pins::SelectionSource::ExplicitOverride => Some("operation_override"),
        pins::SelectionSource::ProfilePin => Some("profile_pin"),
        pins::SelectionSource::MachinePin => Some("machine_pin"),
        pins::SelectionSource::None => None,
    }
}

fn runtime_identity(runtime: &pins::PinnedRuntimeReference) -> diagnostics::RuntimeIdentity {
    match runtime {
        pins::PinnedRuntimeReference::Managed(reference) => diagnostics::RuntimeIdentity {
            exact_identity: format!(
                "managed:{}:artifact={}:manifest={}:generation={}",
                reference.installation_id,
                reference.artifact_sha256,
                reference.manifest_sha256,
                reference.generation
            ),
            family: family(reference.family.as_str()),
            build: Some(reference.build.clone()),
            origin: diagnostics::RuntimeOrigin::Managed,
            ownership: "Endfield-Unix managed".into(),
            provenance: format!("{:?}", reference.trust),
            source: "P025 managed installation; P026 baseline audit".into(),
        },
        pins::PinnedRuntimeReference::External(reference) => diagnostics::RuntimeIdentity {
            exact_identity: format!("external:p022-structural:{}", reference.identity_sha256),
            family: family(reference.family.as_str()),
            build: reference.build.clone(),
            origin: diagnostics::RuntimeOrigin::External,
            ownership: reference.ownership.clone(),
            provenance: "external source authenticity unverified".into(),
            source: reference.sources.join(", "),
        },
    }
}

fn family(value: &str) -> diagnostics::RuntimeFamily {
    match value {
        "wine" => diagnostics::RuntimeFamily::Wine,
        "proton" => diagnostics::RuntimeFamily::Proton,
        _ => diagnostics::RuntimeFamily::Unknown,
    }
}

fn collect_platform_and_graphics() -> (
    diagnostics::PlatformSummary,
    diagnostics::DiagnosticState,
    diagnostics::DiagnosticState,
    Vec<diagnostics::DriverDiagnostic>,
    Vec<diagnostics::DrmNodeDiagnostic>,
    diagnostics::VulkanDiagnostic,
) {
    let platform = LinuxPlatformProbe::system()
        .inspect()
        .ok()
        .map(|report| {
            let distribution = report.distribution.as_ref();
            diagnostics::PlatformSummary {
                os: "Linux".into(),
                distribution: distribution
                    .and_then(|item| item.pretty_name.as_deref().or(item.name.as_deref()))
                    .map(|value| bounded(value, 128)),
                distribution_version: distribution
                    .and_then(|item| item.version_id.as_deref().or(item.version.as_deref()))
                    .map(|value| bounded(value, 64)),
                kernel_release: report
                    .snapshot
                    .platform
                    .kernel
                    .as_ref()
                    .and_then(|kernel| kernel.version.as_deref())
                    .map(|value| bounded(value, 128)),
                architecture: Some(bounded(report.snapshot.platform.architecture.as_str(), 64)),
                environment: Some(format!("{:?}", report.runtime_environment.classification)),
            }
        })
        .unwrap_or_else(|| diagnostics::PlatformSummary {
            os: "Linux".into(),
            distribution: None,
            distribution_version: None,
            kernel_release: None,
            architecture: None,
            environment: None,
        });

    let Ok(report) = LinuxGpuProbe::system().inspect() else {
        return (
            platform,
            diagnostics::DiagnosticState::UnknownOrUnverifiable,
            diagnostics::DiagnosticState::UnknownOrUnverifiable,
            Vec::new(),
            Vec::new(),
            diagnostics::VulkanDiagnostic {
                loader_state: diagnostics::VulkanLoaderState::Unknown,
                loader_api_version: None,
                devices: Vec::new(),
                observation_state: diagnostics::DiagnosticState::UnknownOrUnverifiable,
                diagnostic: Some("P013 GPU/Vulkan probe did not return a report.".into()),
            },
        );
    };
    let gpu_state = source_state(report.pci_status);
    let drm_state = source_state(report.drm_status);
    let drivers = driver_diagnostics(&report);
    let unmatched_drm_nodes = report
        .drm_nodes
        .iter()
        .filter(|node| {
            node.pci_bdf
                .as_deref()
                .is_none_or(|bdf| !report.pci_devices.iter().any(|device| device.bdf == bdf))
        })
        .take(diagnostics::MAX_DRM_NODES)
        .map(|node| diagnostics::DrmNodeDiagnostic {
            name: bounded(&node.name, 32),
            kind: match node.kind {
                LinuxNodeKind::Primary => diagnostics::DrmNodeKind::Primary,
                LinuxNodeKind::Render => diagnostics::DrmNodeKind::Render,
            },
            access: match node.access {
                LinuxNodeAccess::Accessible => diagnostics::DeviceNodeAccess::Accessible,
                LinuxNodeAccess::Denied => diagnostics::DeviceNodeAccess::Denied,
                LinuxNodeAccess::Missing => diagnostics::DeviceNodeAccess::Missing,
                LinuxNodeAccess::Unknown => diagnostics::DeviceNodeAccess::Unknown,
            },
            pci_bdf: node.pci_bdf.as_deref().map(|bdf| bounded(bdf, 16)),
        })
        .collect();
    let vulkan = vulkan_diagnostic(&report);
    (
        platform,
        gpu_state,
        drm_state,
        drivers,
        unmatched_drm_nodes,
        vulkan,
    )
}

fn source_state(status: GpuSourceStatus) -> diagnostics::DiagnosticState {
    match status {
        GpuSourceStatus::Available => diagnostics::DiagnosticState::VerifiedObservation,
        GpuSourceStatus::Missing => diagnostics::DiagnosticState::UnknownOrUnverifiable,
        GpuSourceStatus::Degraded => diagnostics::DiagnosticState::PartialObservation,
    }
}

fn driver_diagnostics(report: &LinuxGpuReport) -> Vec<diagnostics::DriverDiagnostic> {
    report
        .pci_devices
        .iter()
        .enumerate()
        .take(diagnostics::MAX_DEVICES)
        .map(|(index, pci)| {
            let gpu_class = report.snapshot.gpus.get(index).map(|gpu| {
                match gpu.class {
                    GpuClass::Integrated => "integrated",
                    GpuClass::Discrete => "discrete",
                    GpuClass::Unknown => "unknown",
                }
                .to_owned()
            });
            let nodes = report
                .drm_nodes
                .iter()
                .filter(|node| node.pci_bdf.as_deref() == Some(pci.bdf.as_str()))
                .take(32)
                .map(|node| diagnostics::DrmNodeDiagnostic {
                    name: bounded(&node.name, 32),
                    kind: match node.kind {
                        LinuxNodeKind::Primary => diagnostics::DrmNodeKind::Primary,
                        LinuxNodeKind::Render => diagnostics::DrmNodeKind::Render,
                    },
                    access: match node.access {
                        LinuxNodeAccess::Accessible => diagnostics::DeviceNodeAccess::Accessible,
                        LinuxNodeAccess::Denied => diagnostics::DeviceNodeAccess::Denied,
                        LinuxNodeAccess::Missing => diagnostics::DeviceNodeAccess::Missing,
                        LinuxNodeAccess::Unknown => diagnostics::DeviceNodeAccess::Unknown,
                    },
                    pci_bdf: node.pci_bdf.as_deref().map(|bdf| bounded(bdf, 16)),
                })
                .collect();
            diagnostics::DriverDiagnostic {
                pci_bdf: bounded(&pci.bdf, 16),
                vendor_id: pci.vendor_id,
                device_id: pci.device_id,
                vendor: vendor_label(pci.vendor_id).into(),
                gpu_class,
                kernel_driver: pci.driver_name.as_deref().map(|name| bounded(name, 128)),
                kernel_driver_state: match pci.driver_binding {
                    Availability::Available => diagnostics::DiagnosticState::VerifiedObservation,
                    Availability::Unavailable => diagnostics::DiagnosticState::MissingRequirement,
                    Availability::Unknown | Availability::Degraded => {
                        diagnostics::DiagnosticState::UnknownOrUnverifiable
                    }
                },
                kernel_driver_version: None,
                drm_nodes: nodes,
                vulkan_correlation: pci_correlation(report, &pci.bdf),
            }
        })
        .collect()
}

fn vendor_label(vendor: u16) -> &'static str {
    match vendor {
        0x8086 => "Intel",
        0x10de => "NVIDIA",
        0x1002 => "AMD",
        _ => "Unknown PCI vendor",
    }
}

fn pci_correlation(report: &LinuxGpuReport, bdf: &str) -> diagnostics::DeviceCorrelation {
    use diagnostics::DeviceCorrelation as Correlation;
    let matching = report
        .vulkan_devices
        .iter()
        .filter(|device| {
            device
                .physical
                .pci_identity
                .as_ref()
                .is_some_and(|identity| identity.to_string() == bdf)
        })
        .collect::<Vec<_>>();
    if matching.len() > 1
        || matching
            .iter()
            .any(|device| device.mapping == VulkanMapping::AmbiguousPciIdentity)
    {
        Correlation::AmbiguousPciIdentity
    } else if matching
        .iter()
        .any(|device| device.mapping == VulkanMapping::ContradictoryPciIdentity)
    {
        Correlation::ConflictingPciIdentity
    } else if matching
        .iter()
        .any(|device| device.mapping == VulkanMapping::PciIdentity)
    {
        Correlation::ExactPciIdentity
    } else if report.vulkan_status == VulkanLoaderStatus::Enumerated {
        Correlation::Unmatched
    } else {
        Correlation::Unknown
    }
}

fn vulkan_diagnostic(report: &LinuxGpuReport) -> diagnostics::VulkanDiagnostic {
    let loader_state = match report.vulkan_status {
        VulkanLoaderStatus::Missing => diagnostics::VulkanLoaderState::Missing,
        VulkanLoaderStatus::InitializationFailed => {
            diagnostics::VulkanLoaderState::InitializationFailed
        }
        VulkanLoaderStatus::AvailableNoDevices => {
            diagnostics::VulkanLoaderState::AvailableNoDevices
        }
        VulkanLoaderStatus::Enumerated => diagnostics::VulkanLoaderState::Enumerated,
        VulkanLoaderStatus::Degraded => diagnostics::VulkanLoaderState::Partial,
    };
    let devices = report
        .vulkan_devices
        .iter()
        .take(diagnostics::MAX_DEVICES)
        .map(|device| {
            let physical = &device.physical;
            let (driver_id, driver_name, driver_info) = physical
                .driver_identity
                .as_ref()
                .map(|identity| {
                    (
                        Some(identity.driver_id),
                        Some(bounded(&identity.name, 256)),
                        identity
                            .driver_info
                            .as_deref()
                            .map(|info| bounded(info, 256)),
                    )
                })
                .unwrap_or((None, None, None));
            diagnostics::VulkanDeviceDiagnostic {
                name: bounded(&physical.name, 256),
                vendor_id: physical.vendor_id,
                device_id: physical.device_id,
                pci_bdf: physical.pci_identity.map(|identity| identity.to_string()),
                correlation: correlation(device.mapping),
                device_type: match physical.device_type {
                    LinuxVulkanDeviceType::Integrated => diagnostics::VulkanDeviceType::Integrated,
                    LinuxVulkanDeviceType::Discrete => diagnostics::VulkanDeviceType::Discrete,
                    LinuxVulkanDeviceType::Virtual => diagnostics::VulkanDeviceType::Virtual,
                    LinuxVulkanDeviceType::Cpu => diagnostics::VulkanDeviceType::Cpu,
                    LinuxVulkanDeviceType::Other => diagnostics::VulkanDeviceType::Other,
                },
                api_version: physical.api_version,
                driver_id,
                driver_name,
                driver_info,
                extension_state: match physical.extensions_status {
                    VulkanMetadataStatus::Available => diagnostics::VulkanMetadataState::Available,
                    VulkanMetadataStatus::Partial => diagnostics::VulkanMetadataState::Partial,
                    VulkanMetadataStatus::Unavailable => {
                        diagnostics::VulkanMetadataState::Unavailable
                    }
                },
                supported_extensions: physical
                    .supported_extensions
                    .iter()
                    .take(256)
                    .map(|name| bounded(name, 256))
                    .collect(),
                feature_state: match physical.features_status {
                    VulkanMetadataStatus::Available => diagnostics::VulkanMetadataState::Available,
                    VulkanMetadataStatus::Partial => diagnostics::VulkanMetadataState::Partial,
                    VulkanMetadataStatus::Unavailable => {
                        diagnostics::VulkanMetadataState::Unavailable
                    }
                },
                supported_features: physical
                    .supported_features
                    .iter()
                    .take(64)
                    .map(|name| bounded(name, 64))
                    .collect(),
            }
        })
        .collect();
    let observation_state = match loader_state {
        diagnostics::VulkanLoaderState::Enumerated => {
            diagnostics::DiagnosticState::StaticCapabilityAvailable
        }
        diagnostics::VulkanLoaderState::Missing => diagnostics::DiagnosticState::MissingRequirement,
        diagnostics::VulkanLoaderState::InitializationFailed
        | diagnostics::VulkanLoaderState::AvailableNoDevices
        | diagnostics::VulkanLoaderState::Partial => {
            diagnostics::DiagnosticState::PartialObservation
        }
        diagnostics::VulkanLoaderState::Unknown => {
            diagnostics::DiagnosticState::UnknownOrUnverifiable
        }
    };
    diagnostics::VulkanDiagnostic {
        loader_state,
        loader_api_version: None,
        devices,
        observation_state,
        diagnostic: report
            .vulkan_diagnostic
            .map(|message| bounded(message, 256)),
    }
}

fn correlation(mapping: VulkanMapping) -> diagnostics::DeviceCorrelation {
    match mapping {
        VulkanMapping::PciIdentity => diagnostics::DeviceCorrelation::ExactPciIdentity,
        VulkanMapping::AmbiguousPciIdentity => diagnostics::DeviceCorrelation::AmbiguousPciIdentity,
        VulkanMapping::ContradictoryPciIdentity => {
            diagnostics::DeviceCorrelation::ConflictingPciIdentity
        }
        VulkanMapping::Unmatched => diagnostics::DeviceCorrelation::Unmatched,
    }
}

fn collect_prefix(
    id: &prefixes::PrefixId,
    paths: Option<&AppPaths>,
    machine: Option<&pins::MachineId>,
    registry: Option<&pins::PinRegistry>,
) -> diagnostics::PrefixDiagnostic {
    let unavailable = |state, reason: &str| diagnostics::PrefixDiagnostic {
        prefix_id: id.as_str().to_owned(),
        scope: "unknown".into(),
        lifecycle: diagnostics::PrefixLifecycle::Unknown,
        ownership_state: diagnostics::DiagnosticState::UnknownOrUnverifiable,
        filesystem_state: state,
        runtime_association_state: diagnostics::DiagnosticState::UnknownOrUnverifiable,
        original_pin_id: None,
        original_pin_revision: None,
        adapter_preference: None,
        current_pin_state: diagnostics::DiagnosticState::UnknownOrUnverifiable,
        current_pin_relation: None,
        initialization_state: diagnostics::DiagnosticState::UnknownOrUnverifiable,
        initialization_evidence_available: false,
        recovery_state: diagnostics::PrefixRecoveryState::Unknown,
        recovery_diagnostic: Some(reason.into()),
    };
    let Some(paths) = paths else {
        return unavailable(
            diagnostics::DiagnosticState::UnknownOrUnverifiable,
            "application data paths are unavailable",
        );
    };
    let store = prefixes::PrefixStore::for_app(paths);
    let inspection = match store.show(id, super::runtime_pin::validate_reference) {
        Ok(inspection) => inspection,
        Err(prefixes::PrefixError::NotRegistered) => {
            let mut diagnostic = unavailable(
                diagnostics::DiagnosticState::MissingRequirement,
                "no registered managed prefix has this ID; external prefixes were not inspected",
            );
            diagnostic.lifecycle = diagnostics::PrefixLifecycle::Missing;
            return diagnostic;
        }
        Err(prefixes::PrefixError::Missing) => {
            let mut diagnostic = unavailable(
                diagnostics::DiagnosticState::MissingRequirement,
                "registered managed prefix directory is missing",
            );
            diagnostic.lifecycle = diagnostics::PrefixLifecycle::Missing;
            return diagnostic;
        }
        Err(error) => {
            return unavailable(
                diagnostics::DiagnosticState::StaleOrInconsistent,
                prefix_error_message(&error),
            );
        }
    };
    let record = &inspection.record;
    let lifecycle = match inspection.observed_lifecycle {
        prefixes::PrefixLifecycleState::Planned => diagnostics::PrefixLifecycle::Planned,
        prefixes::PrefixLifecycleState::Created => diagnostics::PrefixLifecycle::Created,
        prefixes::PrefixLifecycleState::Initialized => diagnostics::PrefixLifecycle::Initialized,
        prefixes::PrefixLifecycleState::Incomplete => diagnostics::PrefixLifecycle::Incomplete,
        prefixes::PrefixLifecycleState::RecoveryRequired => {
            diagnostics::PrefixLifecycle::RecoveryRequired
        }
        prefixes::PrefixLifecycleState::Missing => diagnostics::PrefixLifecycle::Missing,
        prefixes::PrefixLifecycleState::Blocked => diagnostics::PrefixLifecycle::Blocked,
    };
    let runtime_association_state = match &inspection.runtime {
        prefixes::RuntimeValidation::Valid => diagnostics::DiagnosticState::VerifiedObservation,
        prefixes::RuntimeValidation::Stale(_) => diagnostics::DiagnosticState::StaleOrInconsistent,
        prefixes::RuntimeValidation::NotAssessed => {
            diagnostics::DiagnosticState::UnknownOrUnverifiable
        }
    };
    let ownership_state = if record.ownership == prefixes::PrefixOwnership::EndfieldUnixManaged
        && record.owner_uid == nix::unistd::Uid::current().as_raw()
        && lifecycle != diagnostics::PrefixLifecycle::Blocked
    {
        diagnostics::DiagnosticState::VerifiedObservation
    } else {
        diagnostics::DiagnosticState::StaleOrInconsistent
    };
    let filesystem_state = match lifecycle {
        diagnostics::PrefixLifecycle::Created | diagnostics::PrefixLifecycle::Initialized => {
            diagnostics::DiagnosticState::VerifiedObservation
        }
        diagnostics::PrefixLifecycle::Missing => diagnostics::DiagnosticState::MissingRequirement,
        diagnostics::PrefixLifecycle::Incomplete
        | diagnostics::PrefixLifecycle::RecoveryRequired => {
            diagnostics::DiagnosticState::PartialObservation
        }
        diagnostics::PrefixLifecycle::Blocked => diagnostics::DiagnosticState::StaleOrInconsistent,
        _ => diagnostics::DiagnosticState::UnknownOrUnverifiable,
    };
    let initialization_state = match (&lifecycle, &record.initialization_evidence) {
        (diagnostics::PrefixLifecycle::Initialized, Some(_)) => {
            diagnostics::DiagnosticState::VerifiedObservation
        }
        (diagnostics::PrefixLifecycle::Created, None) => {
            diagnostics::DiagnosticState::UnknownOrUnverifiable
        }
        _ => diagnostics::DiagnosticState::NotTested,
    };
    let current_pin = assess_current_pin(record, machine, registry);
    let recovery =
        prefixes::backup::BackupStore::for_app(paths).compatibility_recovery_observation(id);
    let (recovery_state, recovery_diagnostic) = match recovery {
        Ok(report) => map_recovery(&report),
        Err(_) => (
            diagnostics::PrefixRecoveryState::Unknown,
            Some("P029 recovery metadata could not be safely assessed".into()),
        ),
    };
    diagnostics::PrefixDiagnostic {
        prefix_id: record.id.as_str().to_owned(),
        scope: match record.scope {
            prefixes::PrefixScope::Machine { .. } => "machine".into(),
            prefixes::PrefixScope::Profile { .. } => "profile".into(),
        },
        lifecycle,
        ownership_state,
        filesystem_state,
        runtime_association_state,
        original_pin_id: Some(record.runtime.pin_id.clone()),
        original_pin_revision: Some(record.runtime.pin_revision),
        adapter_preference: record
            .runtime
            .adapter_preference
            .map(|value| format!("{value:?}")),
        current_pin_state: current_pin.0,
        current_pin_relation: Some(current_pin.1),
        initialization_state,
        initialization_evidence_available: record.initialization_evidence.is_some(),
        recovery_state,
        recovery_diagnostic,
    }
}

fn assess_current_pin(
    record: &prefixes::PrefixRecord,
    machine: Option<&pins::MachineId>,
    registry: Option<&pins::PinRegistry>,
) -> (diagnostics::DiagnosticState, String) {
    let recorded_machine = match &record.scope {
        prefixes::PrefixScope::Machine { machine }
        | prefixes::PrefixScope::Profile { machine, .. } => machine,
    };
    let Some(machine) = machine else {
        return (
            diagnostics::DiagnosticState::UnknownOrUnverifiable,
            "current pseudonymous machine identity is unavailable; no pin fallback was attempted"
                .into(),
        );
    };
    if machine != recorded_machine {
        return (
            diagnostics::DiagnosticState::StaleOrInconsistent,
            "prefix is bound to a different machine identity; its saved association is preserved"
                .into(),
        );
    }
    let Some(registry) = registry else {
        return (
            diagnostics::DiagnosticState::UnknownOrUnverifiable,
            "P027 read-only registry is unavailable".into(),
        );
    };
    let profile = match &record.scope {
        prefixes::PrefixScope::Machine { .. } => None,
        prefixes::PrefixScope::Profile { profile, .. } => Some(profile),
    };
    let selected = pins::resolve(
        registry,
        machine,
        profile,
        None,
        super::runtime_pin::validate_reference,
    );
    match selected.state {
        pins::ResolutionState::NoPin => (
            diagnostics::DiagnosticState::UnknownOrUnverifiable,
            "no current pin is selected for the recorded scope; automatic fallback was not performed".into(),
        ),
        pins::ResolutionState::Stale(reason) => (
            diagnostics::DiagnosticState::StaleOrInconsistent,
            format!("current selected pin is stale; no fallback was performed ({})", bounded(&reason, 256)),
        ),
        pins::ResolutionState::Selected => {
            let pin = selected.pin_id.as_ref().and_then(|id| registry.pins.iter().find(|pin| &pin.pin_id == id));
            let scope_matches = matches!(
                (record.runtime.selection_source, selected.source),
                (prefixes::RuntimeSelectionSource::ProfilePin, pins::SelectionSource::ProfilePin)
                    | (prefixes::RuntimeSelectionSource::MachinePin, pins::SelectionSource::MachinePin)
            );
            let matches = selected.runtime.as_ref() == Some(&record.runtime.runtime)
                && selected.pin_id.as_deref() == Some(record.runtime.pin_id.as_str())
                && scope_matches
                && pin.is_some_and(|pin| pin.adapter_preference == record.runtime.adapter_preference);
            if matches {
                (
                    diagnostics::DiagnosticState::VerifiedObservation,
                    if selected.registry_revision == record.runtime.pin_revision {
                        "current pin matches the runtime association recorded when the prefix was created".into()
                    } else {
                        format!("same runtime identity and pin; registry revision changed from {} to {}", record.runtime.pin_revision, selected.registry_revision)
                    },
                )
            } else {
                (
                    diagnostics::DiagnosticState::PartialObservation,
                    "current P027 selection changed; the prefix's original runtime association remains unchanged".into(),
                )
            }
        }
    }
}

fn map_recovery(
    report: &prefixes::backup::CompatibilityRecoveryObservation,
) -> (diagnostics::PrefixRecoveryState, Option<String>) {
    use prefixes::backup::CompatibilityRecoveryState as RestoreState;
    let state = match report.restore_state {
        RestoreState::None => diagnostics::PrefixRecoveryState::None,
        RestoreState::IntentOnly => diagnostics::PrefixRecoveryState::IntentOnly,
        RestoreState::Staged => diagnostics::PrefixRecoveryState::Staged,
        RestoreState::PublishedPending => diagnostics::PrefixRecoveryState::PublishedPending,
        RestoreState::Completed => diagnostics::PrefixRecoveryState::Completed,
        RestoreState::RollbackPending => diagnostics::PrefixRecoveryState::RollbackPending,
        RestoreState::RollbackCompleted => diagnostics::PrefixRecoveryState::RollbackCompleted,
        RestoreState::IncompleteOrInconsistent => {
            diagnostics::PrefixRecoveryState::IncompleteOrInconsistent
        }
    };
    let state = if matches!(
        state,
        diagnostics::PrefixRecoveryState::None
            | diagnostics::PrefixRecoveryState::Completed
            | diagnostics::PrefixRecoveryState::RollbackCompleted
    ) {
        if report.pending_deletion {
            diagnostics::PrefixRecoveryState::PendingDeletion
        } else if report.pending_pin_recovery {
            diagnostics::PrefixRecoveryState::PendingPinRecovery
        } else {
            state
        }
    } else {
        state
    };
    (state, report.diagnostic.map(str::to_owned))
}

fn prefix_error_message(error: &prefixes::PrefixError) -> &'static str {
    match error {
        prefixes::PrefixError::RegistryCorrupt
        | prefixes::PrefixError::UnsupportedSchemaVersion(_) => {
            "managed prefix registry is corrupt or unsupported"
        }
        prefixes::PrefixError::Blocked
        | prefixes::PrefixError::RecoveryBlocked
        | prefixes::PrefixError::RecoveryRequired => {
            "managed prefix ownership or recovery evidence is blocked"
        }
        prefixes::PrefixError::Missing => "registered managed prefix directory is missing",
        prefixes::PrefixError::NotRegistered => "managed prefix is not registered",
        _ => "managed prefix metadata could not be safely inspected",
    }
}

fn render_human(report: &diagnostics::CompatibilityDiagnosticReport) -> Vec<u8> {
    let mut output = String::new();
    output.push_str("Compatibility diagnostics\n\n");
    output.push_str(&format!("Target: {}\n", report.target.game));
    output.push_str(&format!(
        "Platform: {} ({})\n",
        report.platform.os,
        report
            .platform
            .architecture
            .as_deref()
            .unwrap_or("architecture unknown")
    ));
    output.push_str(&format!("Verdict: {:?}\n", report.verdict));
    match &report.runner.runtime {
        Some(runtime) => output.push_str(&format!(
            "Selected runner: {:?} {:?} {} ({:?}; {})\n",
            runtime.family,
            runtime.build.as_deref().unwrap_or("build unknown"),
            runtime.exact_identity,
            runtime.origin,
            report
                .runner
                .selection
                .source
                .as_deref()
                .unwrap_or("scope unknown")
        )),
        None => output.push_str(&format!(
            "Selected runner: {:?}\n",
            report.runner.selection.state
        )),
    }
    output.push_str(&format!(
        "PCI inventory: {:?}; DRM inventory: {:?}\n",
        report.gpu_inventory_state, report.drm_inventory_state
    ));
    output.push_str(&format!(
        "GPU devices: {}; unmatched DRM nodes: {}; Vulkan loader: {:?}; Vulkan devices: {}\n",
        report.drivers.len(),
        report.unmatched_drm_nodes.len(),
        report.vulkan.loader_state,
        report.vulkan.devices.len()
    ));
    output.push_str(&format!(
        "Translation-layer readiness: {:?}\n",
        report.translation_layer_readiness
    ));
    if let Some(prefix) = &report.prefix {
        output.push_str(&format!(
            "Prefix {}: {:?} lifecycle, recovery {:?}\n",
            prefix.prefix_id, prefix.lifecycle, prefix.recovery_state
        ));
    }
    output.push_str("\nFindings:\n");
    for finding in report.findings.iter().take(MAX_HUMAN_FINDINGS) {
        output.push_str(&format!(
            "- [{:?}/{:?}] {}: {}\n",
            finding.severity, finding.state, finding.id, finding.reason
        ));
    }
    if report.findings.len() > MAX_HUMAN_FINDINGS {
        output.push_str(&format!(
            "Additional findings omitted: {}\n",
            report.findings.len() - MAX_HUMAN_FINDINGS
        ));
    }
    output.push_str("\nGame compatibility: NOT TESTED\n");
    output.push_str(
        "Static diagnostics are not proof that Endfield will launch or render correctly.\n",
    );
    output.into_bytes()
}

fn bound_report(report: &mut diagnostics::CompatibilityDiagnosticReport) {
    const TOTAL_EXTENSIONS: usize = 512;
    let mut extensions_left = TOTAL_EXTENSIONS;
    for device in &mut report.vulkan.devices {
        device.supported_extensions.truncate(extensions_left);
        extensions_left = extensions_left.saturating_sub(device.supported_extensions.len());
        device.supported_features.truncate(64);
    }
}

fn bounded(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_targets_fail_before_any_observation() {
        let mut output = Vec::new();
        assert!(matches!(
            run(Some("../bad".into()), None, true, &mut output),
            Err(CompatibilityError::InvalidProfile)
        ));
        assert!(matches!(
            run(None, Some("../bad".into()), true, &mut output),
            Err(CompatibilityError::InvalidPrefixId)
        ));
        assert!(output.is_empty());
    }

    #[test]
    fn profile_and_machine_selection_labels_are_stable() {
        assert_eq!(
            selection_source(pins::SelectionSource::ProfilePin),
            Some("profile_pin")
        );
        assert_eq!(
            selection_source(pins::SelectionSource::MachinePin),
            Some("machine_pin")
        );
        assert_eq!(selection_source(pins::SelectionSource::None), None);
    }
}
