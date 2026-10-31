//! P014 composite display/session probe built on the validated P013 snapshot.

use endfield_unix_domain::{
    Availability, CapabilityId, DiagnosticCategory, ProbeError, ProbeErrorKind, Provenance,
    ProvenanceId, SystemSnapshot,
};

use crate::display::{
    CompositorAssessment, DisplayOutputSource, DisplayProtocolFacts,
    LINUX_DISPLAY_REPORT_SCHEMA_VERSION, LinuxDisplayCompleteness, OutputInventoryStatus,
    ProviderStatus, SessionAssessment, SessionTypeHint, VerifiedDisplayProtocol,
};
use crate::display_mapping::{add_platform_capability, domain_diagnostic, map_outputs};
use crate::display_sources::LinuxDisplayDataSource;
use crate::drm_connectors::collect as collect_connectors;
use crate::errors::{DataSourceError, DataSourceErrorKind};
use crate::gpu_probe::{GpuSourceStatus, LinuxGpuProbe, LinuxGpuReport};
use crate::gpu_sources::LinuxGpuDataSource;
use crate::hardware_probe::LinuxHardwareReport;
use crate::hardware_sources::LinuxHardwareDataSource;
use crate::session::{
    SessionEnvironmentHints, assess_hints, safe_desktop_names, validate_wayland_socket,
    wayland_endpoint, x11_endpoint,
};
use crate::sources::{LinuxDataSource, SystemLinuxDataSource};
use crate::vulkan::{SystemVulkanBackend, VulkanBackend};
use crate::wayland::{SystemWaylandBackend, WaylandDisplayBackend};
use crate::x11::{SystemX11Backend, X11DisplayBackend};

const SESSION_PROVENANCE_ID: u32 = 13;
const WAYLAND_PROVENANCE_ID: u32 = 14;
const X11_PROVENANCE_ID: u32 = 15;
const DRM_CONNECTOR_PROVENANCE_ID: u32 = 16;

/// P014's Linux-specific display observations and P011–P013 composition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxDisplayReport {
    pub schema_version: u32,
    /// Contains the normalized snapshot after display records are appended.
    pub gpu: LinuxGpuReport,
    pub session: SessionAssessment,
    pub compositor: CompositorAssessment,
    pub wayland: DisplayProtocolFacts,
    pub x11: DisplayProtocolFacts,
    /// Wayland takes precedence when both protocols are live to avoid counting
    /// XWayland's compatibility view as another physical display inventory.
    pub output_source: Option<DisplayOutputSource>,
    pub outputs: Vec<crate::LinuxDisplayOutput>,
    pub drm_connector_status: GpuSourceStatus,
    /// `None` means the independent `/sys/class/drm` connector source failed.
    pub drm_connectors: Option<Vec<crate::DrmConnector>>,
    pub completeness: LinuxDisplayCompleteness,
}

impl LinuxDisplayReport {
    pub fn snapshot(&self) -> &SystemSnapshot {
        &self.gpu.snapshot
    }

    pub fn hardware(&self) -> &LinuxHardwareReport {
        &self.gpu.hardware
    }
}

pub struct LinuxDisplayProbe<
    S = SystemLinuxDataSource,
    V = SystemVulkanBackend,
    W = SystemWaylandBackend,
    X = SystemX11Backend,
> {
    source: S,
    vulkan: V,
    wayland: W,
    x11: X,
}

impl
    LinuxDisplayProbe<
        SystemLinuxDataSource,
        SystemVulkanBackend,
        SystemWaylandBackend,
        SystemX11Backend,
    >
{
    pub fn system() -> Self {
        Self {
            source: SystemLinuxDataSource::new(),
            vulkan: SystemVulkanBackend,
            wayland: SystemWaylandBackend,
            x11: SystemX11Backend,
        }
    }
}

impl<S, V, W, X> LinuxDisplayProbe<S, V, W, X> {
    pub fn with_components(source: S, vulkan: V, wayland: W, x11: X) -> Self {
        Self {
            source,
            vulkan,
            wayland,
            x11,
        }
    }
}

impl<S, V, W, X> LinuxDisplayProbe<S, V, W, X>
where
    S: LinuxDataSource
        + LinuxHardwareDataSource
        + LinuxGpuDataSource
        + LinuxDisplayDataSource
        + Clone,
    V: VulkanBackend + Clone,
    W: WaylandDisplayBackend,
    X: X11DisplayBackend,
{
    /// Display, X11, Wayland, and DRM failures are independent partial results.
    /// A valid P013 Linux/GPU report remains required and is never discarded.
    pub fn inspect(&self) -> Result<LinuxDisplayReport, ProbeError> {
        let mut gpu =
            LinuxGpuProbe::with_source_and_vulkan(self.source.clone(), self.vulkan.clone())
                .inspect()?;
        let observed_at = self.source.observation_time_unix_nanos();
        let (hints, environment_error) = match self.source.session_environment() {
            Ok(hints) => (hints, None),
            Err(error) => (SessionEnvironmentHints::default(), Some(error)),
        };
        let mut session = assess_hints(&hints);
        if environment_error.is_some() {
            session.hint = SessionTypeHint::Unknown;
        }
        let compositor = CompositorAssessment {
            advertised_desktops: safe_desktop_names(&hints),
            verified_identity: None,
        };

        let mut wayland_observation = None;
        let mut wayland_status = if hints.wayland_display.is_some() {
            ProviderStatus::NotAttempted
        } else {
            ProviderStatus::NotAdvertised
        };
        let mut wayland_diagnostic = None;
        let mut wayland_provenance = None;
        if hints.wayland_display.is_some() {
            wayland_provenance = Some(push_provenance(
                &mut gpu.snapshot,
                WAYLAND_PROVENANCE_ID,
                "wayland",
                observed_at,
            )?);
            match wayland_endpoint(&self.source, &hints) {
                Ok(endpoint) => match validate_wayland_socket(&self.source, &endpoint) {
                    Ok(()) => match self.wayland.inspect(&endpoint) {
                        Ok(observation) => {
                            wayland_status = ProviderStatus::Connected;
                            wayland_diagnostic = observation.diagnostic;
                            wayland_observation = Some(observation);
                        }
                        Err(error) => {
                            wayland_status = provider_status(error.kind);
                            wayland_diagnostic = Some(provider_error_message(wayland_status));
                        }
                    },
                    Err(error) => {
                        wayland_status = source_provider_status(error.kind);
                        wayland_diagnostic = Some(provider_error_message(wayland_status));
                    }
                },
                Err(error) => {
                    wayland_status = source_provider_status(error.kind);
                    wayland_diagnostic = Some(provider_error_message(wayland_status));
                }
            }
        }

        let mut x11_observation = None;
        let mut x11_status = if hints.x11_display.is_some() {
            ProviderStatus::NotAttempted
        } else {
            ProviderStatus::NotAdvertised
        };
        let mut x11_diagnostic = None;
        let mut x11_provenance = None;
        if hints.x11_display.is_some() {
            x11_provenance = Some(push_provenance(
                &mut gpu.snapshot,
                X11_PROVENANCE_ID,
                "x11",
                observed_at,
            )?);
            match x11_endpoint(&hints) {
                Ok(endpoint) => match self.x11.inspect(&endpoint) {
                    Ok(observation) => {
                        x11_status = ProviderStatus::Connected;
                        x11_diagnostic = observation.diagnostic;
                        x11_observation = Some(observation);
                    }
                    Err(error) => {
                        x11_status = provider_status(error.kind);
                        x11_diagnostic = Some(provider_error_message(x11_status));
                    }
                },
                Err(error) => {
                    x11_status = if error.kind == DataSourceErrorKind::UnsafeSource {
                        ProviderStatus::RejectedRemote
                    } else {
                        ProviderStatus::InvalidEndpoint
                    };
                    x11_diagnostic = Some(provider_error_message(x11_status));
                }
            }
        }

        session.wayland_endpoint = wayland_status;
        session.x11_endpoint = x11_status;
        session.verified_protocol = match (wayland_status, x11_status) {
            (ProviderStatus::Connected, ProviderStatus::Connected) => VerifiedDisplayProtocol::Both,
            (ProviderStatus::Connected, _) => VerifiedDisplayProtocol::Wayland,
            (_, ProviderStatus::Connected) => VerifiedDisplayProtocol::X11,
            (ProviderStatus::NotAdvertised, ProviderStatus::NotAdvertised) => {
                VerifiedDisplayProtocol::None
            }
            _ => VerifiedDisplayProtocol::Unknown,
        };

        let (
            output_source,
            selected_observation,
            selected_status,
            selected_provenance,
            selected_provider,
        ) = if wayland_status == ProviderStatus::Connected {
            (
                Some(DisplayOutputSource::Wayland),
                wayland_observation.as_ref(),
                wayland_status,
                wayland_provenance,
                "wayland",
            )
        } else if x11_status == ProviderStatus::Connected {
            (
                Some(DisplayOutputSource::X11RandR),
                x11_observation.as_ref(),
                x11_status,
                x11_provenance,
                "x11",
            )
        } else {
            (
                None,
                None,
                ProviderStatus::NotAttempted,
                None,
                "linux-display-session",
            )
        };

        let selected_inventory = selected_observation
            .map(|observation| observation.inventory_status)
            .unwrap_or(OutputInventoryStatus::NotAttempted);
        let selected_outputs = selected_observation
            .map(|observation| observation.outputs.as_slice())
            .unwrap_or(&[]);
        let mapped = match (output_source, selected_provenance) {
            (Some(source), Some(provenance)) => map_outputs(
                source,
                selected_outputs,
                selected_inventory,
                selected_status,
                provenance,
                selected_provider,
            ),
            _ => crate::display_mapping::MappedOutputs {
                outputs: Vec::new(),
                capabilities: Vec::new(),
                diagnostics: Vec::new(),
            },
        };
        gpu.snapshot
            .displays
            .extend(mapped.outputs.iter().map(|output| output.info.clone()));
        gpu.snapshot.capabilities.extend(mapped.capabilities);
        gpu.snapshot.diagnostics.extend(mapped.diagnostics);

        let session_provenance = push_provenance(
            &mut gpu.snapshot,
            SESSION_PROVENANCE_ID,
            "linux-display-session",
            observed_at,
        )?;
        let connector_collection = collect_connectors(&self.source, &gpu);
        if connector_collection.connectors.is_some() {
            push_provenance(
                &mut gpu.snapshot,
                DRM_CONNECTOR_PROVENANCE_ID,
                "linux-drm-connectors",
                observed_at,
            )?;
        }

        append_observation_diagnostics(
            &mut gpu.snapshot,
            environment_error,
            wayland_status,
            wayland_diagnostic,
            x11_status,
            x11_diagnostic,
            selected_inventory,
            connector_collection.status,
            &connector_collection.issues,
        );

        let session_availability = if wayland_status == ProviderStatus::Connected
            || x11_status == ProviderStatus::Connected
        {
            Availability::Available
        } else if wayland_status == ProviderStatus::NotAdvertised
            && x11_status == ProviderStatus::NotAdvertised
            && environment_error.is_none()
            && session.hint == SessionTypeHint::Headless
        {
            Availability::Unavailable
        } else if wayland_status != ProviderStatus::NotAdvertised
            || x11_status != ProviderStatus::NotAdvertised
            || matches!(
                session.hint,
                SessionTypeHint::Wayland | SessionTypeHint::X11 | SessionTypeHint::Mixed
            )
        {
            Availability::Degraded
        } else {
            Availability::Unknown
        };
        let session_diagnostic = match session_availability {
            Availability::Unavailable => Some(domain_diagnostic(
                DiagnosticCategory::UnsupportedCapability,
                "linux-display-session",
                "no local graphical display endpoint was advertised",
            )),
            Availability::Degraded => Some(domain_diagnostic(
                DiagnosticCategory::TemporaryFailure,
                "linux-display-session",
                "advertised graphical endpoints could not be verified",
            )),
            Availability::Unknown => Some(domain_diagnostic(
                DiagnosticCategory::NotYetProbed,
                "linux-display-session",
                "graphical session hints could not be read",
            )),
            Availability::Available => None,
        };
        let session_capability = add_platform_capability(
            CapabilityId::CompositorSession,
            session_availability,
            Some(if session_availability == Availability::Available {
                wayland_provenance
                    .or(x11_provenance)
                    .unwrap_or(session_provenance)
            } else {
                session_provenance
            }),
            session_diagnostic,
        );
        gpu.snapshot.capabilities.push(session_capability);

        let display_availability = match selected_inventory {
            OutputInventoryStatus::Enumerated => {
                if mapped.outputs.is_empty()
                    || mapped
                        .outputs
                        .iter()
                        .all(|output| output.info.availability == Availability::Unavailable)
                {
                    Availability::Unavailable
                } else if selected_observation.is_some_and(|observation| {
                    observation.inventory_status == OutputInventoryStatus::Inconsistent
                }) {
                    Availability::Degraded
                } else {
                    Availability::Available
                }
            }
            OutputInventoryStatus::Inconsistent => Availability::Degraded,
            OutputInventoryStatus::ProtocolUnavailable
            | OutputInventoryStatus::NotAttempted
            | OutputInventoryStatus::Failed => Availability::Unknown,
        };
        let display_diagnostic = match display_availability {
            Availability::Unavailable => Some(domain_diagnostic(
                DiagnosticCategory::UnsupportedCapability,
                selected_provider,
                "the connected display protocol reported no output records",
            )),
            Availability::Degraded => Some(domain_diagnostic(
                DiagnosticCategory::MalformedSourceData,
                selected_provider,
                "display output inventory was partial or inconsistent",
            )),
            Availability::Unknown
                if selected_inventory == OutputInventoryStatus::ProtocolUnavailable =>
            {
                Some(domain_diagnostic(
                    DiagnosticCategory::NotYetProbed,
                    selected_provider,
                    "the connected display server did not expose output enumeration",
                ))
            }
            Availability::Unknown => None,
            Availability::Available => None,
        };
        gpu.snapshot.capabilities.push(add_platform_capability(
            CapabilityId::Display,
            display_availability,
            selected_provenance,
            display_diagnostic,
        ));

        let snapshot = gpu.snapshot.clone().normalize().map_err(|_| {
            probe_error(
                "linux-display",
                ProbeErrorKind::MalformedData,
                "combined P011/P012/P013/P014 snapshot failed validation",
            )
        })?;
        gpu.snapshot = snapshot;

        let wayland_facts = protocol_facts(
            wayland_status,
            wayland_observation.as_ref(),
            wayland_diagnostic,
        );
        let x11_facts = protocol_facts(x11_status, x11_observation.as_ref(), x11_diagnostic);
        let completeness = if gpu.completeness == crate::LinuxGpuCompleteness::Partial
            || wayland_status != ProviderStatus::Connected
                && x11_status != ProviderStatus::Connected
            || wayland_facts.inventory != OutputInventoryStatus::Enumerated
                && wayland_status == ProviderStatus::Connected
            || x11_facts.inventory != OutputInventoryStatus::Enumerated
                && x11_status == ProviderStatus::Connected
            || connector_collection.status != GpuSourceStatus::Available
            || !gpu.snapshot.diagnostics.is_empty()
        {
            LinuxDisplayCompleteness::Partial
        } else {
            LinuxDisplayCompleteness::Complete
        };

        Ok(LinuxDisplayReport {
            schema_version: LINUX_DISPLAY_REPORT_SCHEMA_VERSION,
            gpu,
            session,
            compositor,
            wayland: wayland_facts,
            x11: x11_facts,
            output_source,
            outputs: mapped.outputs,
            drm_connector_status: connector_collection.status,
            drm_connectors: connector_collection.connectors,
            completeness,
        })
    }
}

fn protocol_facts(
    status: ProviderStatus,
    observation: Option<&crate::display::BackendObservation>,
    diagnostic: Option<&'static str>,
) -> DisplayProtocolFacts {
    let (inventory, output_count, protocol_version, optional_protocol, diagnostic) =
        if let Some(observation) = observation {
            (
                observation.inventory_status,
                (observation.inventory_status == OutputInventoryStatus::Enumerated
                    || observation.inventory_status == OutputInventoryStatus::Inconsistent)
                    .then_some(observation.outputs.len()),
                observation.protocol_version,
                Some(observation.optional_logical_output_protocol),
                observation.diagnostic.or(diagnostic),
            )
        } else {
            (
                if status == ProviderStatus::NotAdvertised || status == ProviderStatus::NotAttempted
                {
                    OutputInventoryStatus::NotAttempted
                } else {
                    OutputInventoryStatus::Failed
                },
                None,
                None,
                None,
                diagnostic,
            )
        };
    DisplayProtocolFacts {
        status,
        inventory,
        output_count,
        protocol_version,
        optional_logical_output_protocol: optional_protocol,
        screen_geometry: observation.and_then(|observation| observation.screen_geometry),
        diagnostic,
    }
}

#[allow(clippy::too_many_arguments)]
fn append_observation_diagnostics(
    snapshot: &mut SystemSnapshot,
    environment_error: Option<DataSourceError>,
    wayland_status: ProviderStatus,
    wayland_diagnostic: Option<&'static str>,
    x11_status: ProviderStatus,
    x11_diagnostic: Option<&'static str>,
    inventory: OutputInventoryStatus,
    connector_status: GpuSourceStatus,
    connector_issues: &[(&'static str, &'static str)],
) {
    if environment_error.is_some() {
        snapshot.diagnostics.push(domain_diagnostic(
            source_diagnostic_category(
                environment_error
                    .map(|error| error.kind)
                    .unwrap_or(DataSourceErrorKind::Other),
            ),
            "linux-display-session",
            "allowlisted graphical-session environment hints could not be read",
        ));
    }
    append_provider_diagnostic(snapshot, "wayland", wayland_status, wayland_diagnostic);
    append_provider_diagnostic(snapshot, "x11", x11_status, x11_diagnostic);
    if inventory == OutputInventoryStatus::Inconsistent {
        snapshot.diagnostics.push(domain_diagnostic(
            DiagnosticCategory::MalformedSourceData,
            "linux-display-session",
            "display output enumeration returned partial or inconsistent data",
        ));
    } else if inventory == OutputInventoryStatus::ProtocolUnavailable {
        snapshot.diagnostics.push(domain_diagnostic(
            DiagnosticCategory::NotYetProbed,
            "linux-display-session",
            "display output enumeration was not exposed by the connected server",
        ));
    }
    if connector_status == GpuSourceStatus::Missing {
        snapshot.diagnostics.push(domain_diagnostic(
            DiagnosticCategory::MissingProvider,
            "linux-drm-connectors",
            "the DRM connector source is unavailable",
        ));
    }
    for (message, provider) in connector_issues {
        let category = if *message == "DRM connector source could not be enumerated" {
            DiagnosticCategory::TemporaryFailure
        } else {
            DiagnosticCategory::MalformedSourceData
        };
        snapshot
            .diagnostics
            .push(domain_diagnostic(category, provider, message));
    }
}

fn append_provider_diagnostic(
    snapshot: &mut SystemSnapshot,
    provider: &'static str,
    status: ProviderStatus,
    message: Option<&'static str>,
) {
    let category = match status {
        ProviderStatus::NotAdvertised
        | ProviderStatus::Connected
        | ProviderStatus::NotAttempted => {
            return;
        }
        ProviderStatus::RejectedRemote | ProviderStatus::InvalidEndpoint => {
            DiagnosticCategory::MalformedSourceData
        }
        ProviderStatus::AuthenticationFailed | ProviderStatus::PermissionDenied => {
            DiagnosticCategory::PermissionDenied
        }
        ProviderStatus::TimedOut | ProviderStatus::Unavailable | ProviderStatus::Failed => {
            DiagnosticCategory::TemporaryFailure
        }
    };
    snapshot.diagnostics.push(domain_diagnostic(
        category,
        provider,
        message.unwrap_or(provider_error_message(status)),
    ));
}

fn source_provider_status(kind: DataSourceErrorKind) -> ProviderStatus {
    match kind {
        DataSourceErrorKind::NotFound => ProviderStatus::Unavailable,
        DataSourceErrorKind::PermissionDenied => ProviderStatus::PermissionDenied,
        DataSourceErrorKind::UnsafeSource | DataSourceErrorKind::UnexpectedType => {
            ProviderStatus::InvalidEndpoint
        }
        DataSourceErrorKind::TooLarge | DataSourceErrorKind::MalformedData => {
            ProviderStatus::InvalidEndpoint
        }
        DataSourceErrorKind::Unsupported
        | DataSourceErrorKind::Other
        | DataSourceErrorKind::InvalidUtf8 => ProviderStatus::Failed,
    }
}

fn provider_status(kind: crate::display::BackendErrorKind) -> ProviderStatus {
    match kind {
        crate::display::BackendErrorKind::Unavailable
        | crate::display::BackendErrorKind::Unsupported => ProviderStatus::Unavailable,
        crate::display::BackendErrorKind::AuthenticationFailed => {
            ProviderStatus::AuthenticationFailed
        }
        crate::display::BackendErrorKind::PermissionDenied => ProviderStatus::PermissionDenied,
        crate::display::BackendErrorKind::TimedOut => ProviderStatus::TimedOut,
        crate::display::BackendErrorKind::Malformed => ProviderStatus::Failed,
        crate::display::BackendErrorKind::Other => ProviderStatus::Failed,
    }
}

fn provider_error_message(status: ProviderStatus) -> &'static str {
    match status {
        ProviderStatus::NotAdvertised => "the display endpoint was not advertised",
        ProviderStatus::NotAttempted => "the display endpoint was not queried",
        ProviderStatus::Connected => "the display protocol connection was verified",
        ProviderStatus::RejectedRemote => "remote X11 destinations are not queried",
        ProviderStatus::InvalidEndpoint => "the display endpoint metadata was invalid",
        ProviderStatus::AuthenticationFailed => "the display server rejected authentication",
        ProviderStatus::PermissionDenied => {
            "permission to connect to the display endpoint was denied"
        }
        ProviderStatus::TimedOut => "display protocol discovery exceeded its bounded wait",
        ProviderStatus::Unavailable => "the advertised local display endpoint was unavailable",
        ProviderStatus::Failed => "the display protocol response could not be validated",
    }
}

fn source_diagnostic_category(kind: DataSourceErrorKind) -> DiagnosticCategory {
    match kind {
        DataSourceErrorKind::NotFound => DiagnosticCategory::MissingProvider,
        DataSourceErrorKind::PermissionDenied => DiagnosticCategory::PermissionDenied,
        DataSourceErrorKind::Unsupported => DiagnosticCategory::UnsupportedCapability,
        DataSourceErrorKind::InvalidUtf8
        | DataSourceErrorKind::MalformedData
        | DataSourceErrorKind::TooLarge
        | DataSourceErrorKind::UnexpectedType
        | DataSourceErrorKind::UnsafeSource => DiagnosticCategory::MalformedSourceData,
        DataSourceErrorKind::Other => DiagnosticCategory::TemporaryFailure,
    }
}

fn push_provenance(
    snapshot: &mut SystemSnapshot,
    id: u32,
    provider: &str,
    observed_at: Option<u64>,
) -> Result<ProvenanceId, ProbeError> {
    let id = ProvenanceId::new(id).map_err(|_| {
        probe_error(
            provider,
            ProbeErrorKind::MalformedData,
            "display provenance ID was invalid",
        )
    })?;
    snapshot
        .provenance
        .push(Provenance::new(id, provider, observed_at).map_err(|_| {
            probe_error(
                provider,
                ProbeErrorKind::MalformedData,
                "display provenance was invalid",
            )
        })?);
    Ok(id)
}

fn probe_error(provider: &str, kind: ProbeErrorKind, message: &str) -> ProbeError {
    ProbeError::new(provider, kind, Some(message.to_owned())).unwrap_or_else(|_| {
        ProbeError::new("linux-display", kind, None).expect("static provider name")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::path::{Path, PathBuf};

    use crate::display::{
        BackendDisplayOutput, BackendError, BackendErrorKind, BackendObservation,
        DisplayOutputState, ExactRefreshRate, LinuxDisplayMode, LogicalGeometry, ScaleFactor,
    };
    use crate::display_sources::RawDrmConnector;
    use crate::errors::DataSourceError;
    use crate::gpu_sources::{DeviceNodeAccess, DrmNodeKind, DrmNodeObservation, PciAttribute};
    use crate::hardware_sources::{
        CgroupSource, CpuListSource, CpuTopologyField, LinuxHardwareDataSource,
        PlatformProfileField, RawSchedulerState,
    };
    use crate::kernel::RawKernelIdentity;
    use crate::pci::PciBdf;
    use crate::session::SessionEnvironmentHints;
    use crate::sources::{ContainerMarker, OsReleaseFile, RuntimeDirectoryMetadata};
    use crate::vulkan::{VulkanDeviceType, VulkanFacts, VulkanLoaderStatus, VulkanPhysicalDevice};
    use endfield_unix_domain::{CapabilityId, ProvenanceId};
    use tempfile::tempdir;

    #[test]
    fn protocol_selection_prefers_wayland_to_avoid_xwayland_duplicate_outputs() {
        assert_eq!(
            choose_protocol(ProviderStatus::Connected, ProviderStatus::Connected),
            Some(DisplayOutputSource::Wayland)
        );
        assert_eq!(
            choose_protocol(ProviderStatus::Unavailable, ProviderStatus::Connected),
            Some(DisplayOutputSource::X11RandR)
        );
    }

    #[derive(Clone)]
    struct FixtureSource {
        hints: SessionEnvironmentHints,
        runtime_dir: Option<PathBuf>,
        connectors: Vec<RawDrmConnector>,
        uid: u32,
    }

    impl Default for FixtureSource {
        fn default() -> Self {
            Self {
                hints: SessionEnvironmentHints::default(),
                runtime_dir: None,
                connectors: vec![RawDrmConnector {
                    card_name: "card0".to_owned(),
                    connector_name: "eDP-1".to_owned(),
                    pci_bdf: Some("0000:00:02.0".to_owned()),
                    status: Ok(b"connected\n".to_vec()),
                    enabled: Ok(b"disabled\n".to_vec()),
                    modes: Ok(b"1920x1080\n".to_vec()),
                }],
                uid: nix::unistd::geteuid().as_raw(),
            }
        }
    }

    impl LinuxDataSource for FixtureSource {
        fn read_os_release(&self, _: OsReleaseFile) -> Result<Vec<u8>, DataSourceError> {
            Ok(b"ID=arch\nNAME=Arch Linux\nPRETTY_NAME=Arch Linux\n".to_vec())
        }
        fn kernel_identity(&self) -> Result<RawKernelIdentity, DataSourceError> {
            Ok(RawKernelIdentity {
                system_name: "Linux".to_owned(),
                release: "fixture-kernel".to_owned(),
                machine: "x86_64".to_owned(),
            })
        }
        fn container_marker_present(&self, _: ContainerMarker) -> Result<bool, DataSourceError> {
            Ok(false)
        }
        fn runtime_directory_path(&self) -> Result<Option<PathBuf>, DataSourceError> {
            Ok(self.runtime_dir.clone())
        }
        fn runtime_directory_metadata(
            &self,
            _: &Path,
        ) -> Result<RuntimeDirectoryMetadata, DataSourceError> {
            Ok(RuntimeDirectoryMetadata {
                is_directory: true,
                is_symlink: false,
                owner_uid: self.uid,
                mode: 0o700,
            })
        }
        fn effective_uid(&self) -> u32 {
            self.uid
        }
        fn observation_time_unix_nanos(&self) -> Option<u64> {
            Some(1_761_286_800_000_000_000)
        }
    }

    impl LinuxHardwareDataSource for FixtureSource {
        fn read_proc_cpuinfo(&self) -> Result<Vec<u8>, DataSourceError> {
            Ok(b"processor : 0\nmodel name : Synthetic display-test CPU\n".to_vec())
        }
        fn read_proc_meminfo(&self) -> Result<Vec<u8>, DataSourceError> {
            Ok(b"MemTotal: 16384000 kB\nMemAvailable: 12000000 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n".to_vec())
        }
        fn read_cpu_list(&self, _: CpuListSource) -> Result<Vec<u8>, DataSourceError> {
            Ok(b"0\n".to_vec())
        }
        fn read_cpu_topology_field(
            &self,
            _: u32,
            _: CpuTopologyField,
        ) -> Result<Option<i64>, DataSourceError> {
            Ok(Some(0))
        }
        fn process_affinity(&self) -> Result<Vec<u32>, DataSourceError> {
            Ok(vec![0])
        }
        fn scheduler_state(&self) -> Result<RawSchedulerState, DataSourceError> {
            Ok(RawSchedulerState {
                policy: 0,
                priority: 0,
            })
        }
        fn read_platform_profile_field(
            &self,
            _: PlatformProfileField,
        ) -> Result<Option<Vec<u8>>, DataSourceError> {
            Ok(None)
        }
        fn cgroup_source(&self) -> Result<CgroupSource, DataSourceError> {
            Ok(CgroupSource::None)
        }
    }

    impl LinuxGpuDataSource for FixtureSource {
        fn enumerate_pci_entries(&self) -> Result<Vec<String>, DataSourceError> {
            Ok(vec!["0000:00:02.0".to_owned()])
        }
        fn read_pci_attribute(
            &self,
            _: &str,
            attribute: PciAttribute,
        ) -> Result<Vec<u8>, DataSourceError> {
            Ok(match attribute {
                PciAttribute::Class => b"0x030000".to_vec(),
                PciAttribute::Vendor => b"0x8086".to_vec(),
                PciAttribute::Device => b"0x9a49".to_vec(),
                PciAttribute::SubsystemVendor => b"0x8086".to_vec(),
                PciAttribute::SubsystemDevice => b"0x1234".to_vec(),
                PciAttribute::Driver => b"i915".to_vec(),
            })
        }
        fn enumerate_drm_nodes(&self) -> Result<Vec<DrmNodeObservation>, DataSourceError> {
            Ok(vec![
                DrmNodeObservation {
                    name: "card0".to_owned(),
                    kind: DrmNodeKind::Primary,
                    pci_bdf: Some("0000:00:02.0".to_owned()),
                    access: DeviceNodeAccess::Accessible,
                },
                DrmNodeObservation {
                    name: "renderD128".to_owned(),
                    kind: DrmNodeKind::Render,
                    pci_bdf: Some("0000:00:02.0".to_owned()),
                    access: DeviceNodeAccess::Accessible,
                },
            ])
        }
    }

    impl LinuxDisplayDataSource for FixtureSource {
        fn session_environment(&self) -> Result<SessionEnvironmentHints, DataSourceError> {
            Ok(self.hints.clone())
        }
        fn enumerate_drm_connectors(&self) -> Result<Vec<RawDrmConnector>, DataSourceError> {
            Ok(self.connectors.clone())
        }
    }

    #[derive(Clone)]
    struct FixtureVulkan;

    impl VulkanBackend for FixtureVulkan {
        fn enumerate(&self) -> VulkanFacts {
            VulkanFacts {
                status: VulkanLoaderStatus::Enumerated,
                diagnostic: None,
                devices: vec![VulkanPhysicalDevice {
                    name: "Synthetic integrated GPU".to_owned(),
                    vendor_id: 0x8086,
                    device_id: 0x9a49,
                    pci_identity: Some("0000:00:02.0".parse::<PciBdf>().unwrap()),
                    driver_identity: None,
                    device_type: VulkanDeviceType::Integrated,
                    api_version: Some((1, 3, 0)),
                    supported_extensions: Vec::new(),
                    extensions_status: crate::VulkanMetadataStatus::Unavailable,
                    supported_features: Vec::new(),
                    features_status: crate::VulkanMetadataStatus::Available,
                    device_local_heap_bytes: Vec::new(),
                }],
            }
        }
    }

    #[derive(Clone)]
    struct FixtureWayland(Result<BackendObservation, BackendError>);

    impl WaylandDisplayBackend for FixtureWayland {
        fn inspect(
            &self,
            _: &crate::session::LocalWaylandEndpoint,
        ) -> Result<BackendObservation, BackendError> {
            self.0.clone()
        }
    }

    #[derive(Clone)]
    struct FixtureX11(Result<BackendObservation, BackendError>);

    impl X11DisplayBackend for FixtureX11 {
        fn inspect(
            &self,
            _: &crate::session::LocalX11Endpoint,
        ) -> Result<BackendObservation, BackendError> {
            self.0.clone()
        }
    }

    fn observation(
        outputs: Vec<BackendDisplayOutput>,
        optional_logical_output_protocol: bool,
        protocol_version: (u32, u32),
        screen_geometry: Option<(u32, u32)>,
    ) -> BackendObservation {
        BackendObservation {
            inventory_status: OutputInventoryStatus::Enumerated,
            outputs,
            optional_logical_output_protocol,
            protocol_version: Some(protocol_version),
            screen_geometry,
            diagnostic: None,
        }
    }

    fn test_output(
        name: &str,
        width: u32,
        height: u32,
        refresh_millihertz: u32,
        primary: Option<bool>,
        x: i32,
        scale: Option<ScaleFactor>,
    ) -> BackendDisplayOutput {
        BackendDisplayOutput {
            name: Some(name.to_owned()),
            state: DisplayOutputState::Active,
            modes: vec![LinuxDisplayMode {
                width_pixels: width,
                height_pixels: height,
                refresh_rate: ExactRefreshRate::from_millihertz(refresh_millihertz),
                current: true,
                preferred: true,
                interlaced: Some(false),
                doublescan: Some(false),
            }],
            primary,
            logical_geometry: Some(LogicalGeometry {
                x,
                y: 0,
                width: width / scale.map_or(1, |scale| scale.numerator),
                height: height / scale.map_or(1, |scale| scale.numerator),
            }),
            scale,
            rotation_degrees: Some(0),
        }
    }

    fn probe(
        source: FixtureSource,
        wayland: Result<BackendObservation, BackendError>,
        x11: Result<BackendObservation, BackendError>,
    ) -> LinuxDisplayProbe<FixtureSource, FixtureVulkan, FixtureWayland, FixtureX11> {
        LinuxDisplayProbe::with_components(
            source,
            FixtureVulkan,
            FixtureWayland(wayland),
            FixtureX11(x11),
        )
    }

    #[test]
    fn headless_report_preserves_p011_to_p013_facts_and_reports_connector_independently() {
        let report = probe(
            FixtureSource::default(),
            Err(BackendError {
                kind: BackendErrorKind::Unavailable,
            }),
            Err(BackendError {
                kind: BackendErrorKind::Unavailable,
            }),
        )
        .inspect()
        .expect("headless report");

        report.snapshot().validate().expect("validated snapshot");
        assert_eq!(report.session.hint, SessionTypeHint::Headless);
        assert_eq!(
            report.session.verified_protocol,
            VerifiedDisplayProtocol::None
        );
        assert!(report.outputs.is_empty());
        assert_eq!(report.drm_connectors.as_ref().unwrap().len(), 1);
        assert_eq!(
            report.drm_connectors.as_ref().unwrap()[0].gpu_instance,
            report.snapshot().gpus.first().map(|gpu| gpu.instance)
        );
        assert!(report.snapshot().cpu.is_some());
        assert!(report.snapshot().memory.is_some());
        assert_eq!(report.snapshot().gpus.len(), 1);
        assert!(
            report
                .snapshot()
                .capabilities
                .iter()
                .any(|capability| capability.id == CapabilityId::GpuDevice)
        );
        assert!(report.snapshot().capabilities.iter().any(|capability| {
            capability.id == CapabilityId::Display
                && capability.availability == Availability::Unknown
        }));
        let provenance = report
            .snapshot()
            .provenance
            .iter()
            .map(|entry| entry.id)
            .collect::<std::collections::BTreeSet<ProvenanceId>>();
        assert_eq!(provenance.len(), report.snapshot().provenance.len());
    }

    #[test]
    fn successful_zero_output_inventory_is_not_a_connection_failure() {
        let runtime = tempdir().expect("runtime directory");
        let _listener = UnixListener::bind(runtime.path().join("wayland-0")).unwrap();
        let source = FixtureSource {
            hints: SessionEnvironmentHints {
                xdg_session_type: Some("wayland".to_owned()),
                wayland_display: Some("wayland-0".to_owned()),
                ..SessionEnvironmentHints::default()
            },
            runtime_dir: Some(runtime.path().to_path_buf()),
            ..FixtureSource::default()
        };
        let report = probe(
            source,
            Ok(observation(Vec::new(), false, (1, 4), None)),
            Err(BackendError {
                kind: BackendErrorKind::Unavailable,
            }),
        )
        .inspect()
        .expect("zero-output Wayland report");

        assert_eq!(report.wayland.status, ProviderStatus::Connected);
        assert_eq!(report.wayland.inventory, OutputInventoryStatus::Enumerated);
        assert_eq!(report.wayland.output_count, Some(0));
        assert!(report.outputs.is_empty());
        assert!(report.snapshot().capabilities.iter().any(|capability| {
            capability.id == CapabilityId::Display
                && capability.availability == Availability::Unavailable
        }));
    }

    #[test]
    fn mixed_xwayland_report_uses_wayland_outputs_once_and_preserves_exact_modes() {
        let runtime = tempdir().expect("runtime directory");
        let _listener = UnixListener::bind(runtime.path().join("wayland-0")).unwrap();
        let mut source = FixtureSource {
            hints: SessionEnvironmentHints {
                xdg_session_type: Some("wayland".to_owned()),
                wayland_display: Some("wayland-0".to_owned()),
                x11_display: Some(":0".to_owned()),
                current_desktop: Some("Hyprland".to_owned()),
                ..SessionEnvironmentHints::default()
            },
            runtime_dir: Some(runtime.path().to_path_buf()),
            ..FixtureSource::default()
        };
        source.uid = nix::unistd::geteuid().as_raw();
        let wayland = observation(
            vec![
                test_output(
                    "eDP-1",
                    2560,
                    1440,
                    99_950,
                    None,
                    0,
                    Some(ScaleFactor::new(2, 1).unwrap()),
                ),
                test_output("DP-1", 1920, 1080, 75_000, None, 1280, None),
            ],
            true,
            (1, 4),
            None,
        );
        let x11 = observation(
            vec![test_output(
                "screen",
                2560,
                1440,
                60_000,
                Some(true),
                0,
                None,
            )],
            false,
            (1, 6),
            Some((2560, 1440)),
        );
        let report = probe(source, Ok(wayland), Ok(x11))
            .inspect()
            .expect("mixed display report");

        report.snapshot().validate().expect("validated snapshot");
        assert_eq!(report.session.hint, SessionTypeHint::Mixed);
        assert_eq!(
            report.session.verified_protocol,
            VerifiedDisplayProtocol::Both
        );
        assert_eq!(report.output_source, Some(DisplayOutputSource::Wayland));
        assert_eq!(report.outputs.len(), 2);
        assert_eq!(report.wayland.output_count, Some(2));
        assert_eq!(report.x11.output_count, Some(1));
        assert_eq!(report.compositor.verified_identity, None);
        assert_eq!(report.compositor.advertised_desktops, ["Hyprland"]);
        let internal = report
            .outputs
            .iter()
            .find(|output| output.info.label.as_deref() == Some("eDP-1"))
            .unwrap();
        assert_eq!(internal.info.resolution.unwrap().width_pixels, 2560);
        assert_eq!(internal.info.refresh_rate, None);
        assert_eq!(
            internal.modes[0].refresh_rate,
            ExactRefreshRate::new(1999, 20)
        );
        assert_eq!(internal.logical_geometry.unwrap().width, 1280);
        assert_eq!(internal.scale, Some(ScaleFactor::new(2, 1).unwrap()));
        assert_eq!(internal.info.primary, None);
        assert_eq!(report.snapshot().cpu.as_ref().unwrap().instance.get(), 1);
        assert_eq!(report.snapshot().gpus[0].instance.get(), 2);
        assert!(
            report
                .outputs
                .iter()
                .all(|output| output.info.instance.get() >= 1_000_000)
        );
        assert!(report.snapshot().capabilities.iter().any(|capability| {
            capability.id == CapabilityId::Display
                && capability.instance == Some(internal.info.instance)
        }));
        let provenance = report
            .snapshot()
            .provenance
            .iter()
            .map(|entry| entry.id)
            .collect::<std::collections::BTreeSet<ProvenanceId>>();
        assert_eq!(provenance.len(), report.snapshot().provenance.len());
    }

    #[test]
    fn x11_report_keeps_randr_primary_and_disconnected_output_distinct() {
        let source = FixtureSource {
            hints: SessionEnvironmentHints {
                xdg_session_type: Some("x11".to_owned()),
                x11_display: Some(":7".to_owned()),
                ..SessionEnvironmentHints::default()
            },
            ..FixtureSource::default()
        };
        let mut disconnected = test_output("HDMI-1", 1920, 1080, 60_000, Some(false), 0, None);
        disconnected.state = DisplayOutputState::Disconnected;
        disconnected.modes.clear();
        disconnected.logical_geometry = None;
        let x11 = observation(
            vec![
                test_output("DP-1", 2560, 1440, 74_970, Some(true), 0, None),
                disconnected,
            ],
            false,
            (1, 6),
            Some((3840, 1440)),
        );
        let report = probe(
            source,
            Err(BackendError {
                kind: BackendErrorKind::Unavailable,
            }),
            Ok(x11),
        )
        .inspect()
        .expect("X11 report");

        report.snapshot().validate().expect("validated snapshot");
        assert_eq!(report.session.hint, SessionTypeHint::X11);
        assert_eq!(
            report.session.verified_protocol,
            VerifiedDisplayProtocol::X11
        );
        assert_eq!(report.output_source, Some(DisplayOutputSource::X11RandR));
        assert_eq!(report.x11.screen_geometry, Some((3840, 1440)));
        assert_eq!(report.outputs.len(), 2);
        assert_eq!(report.outputs[0].info.primary, Some(true));
        assert_eq!(
            report.outputs[1].info.availability,
            Availability::Unavailable
        );
        assert_eq!(report.outputs[1].info.resolution, None);
        assert_eq!(report.outputs[1].info.primary, None);
    }

    #[test]
    fn stale_wayland_hint_and_x11_authentication_failure_remain_partial() {
        let source = FixtureSource {
            hints: SessionEnvironmentHints {
                xdg_session_type: Some("wayland".to_owned()),
                wayland_display: Some("stale-wayland".to_owned()),
                x11_display: Some(":4".to_owned()),
                ..SessionEnvironmentHints::default()
            },
            runtime_dir: None,
            ..FixtureSource::default()
        };
        let report = probe(
            source,
            Err(BackendError {
                kind: BackendErrorKind::Unavailable,
            }),
            Err(BackendError {
                kind: BackendErrorKind::AuthenticationFailed,
            }),
        )
        .inspect()
        .expect("partial report");
        report
            .snapshot()
            .validate()
            .expect("validated partial snapshot");
        assert_eq!(report.wayland.status, ProviderStatus::Unavailable);
        assert_eq!(report.x11.status, ProviderStatus::AuthenticationFailed);
        assert_eq!(
            report.session.verified_protocol,
            VerifiedDisplayProtocol::Unknown
        );
        assert!(report.outputs.is_empty());
        assert_eq!(report.snapshot().gpus.len(), 1);
        assert!(report.snapshot().cpu.is_some());
        assert!(report.snapshot().memory.is_some());
    }

    fn choose_protocol(
        wayland: ProviderStatus,
        x11: ProviderStatus,
    ) -> Option<DisplayOutputSource> {
        if wayland == ProviderStatus::Connected {
            Some(DisplayOutputSource::Wayland)
        } else if x11 == ProviderStatus::Connected {
            Some(DisplayOutputSource::X11RandR)
        } else {
            None
        }
    }
}
