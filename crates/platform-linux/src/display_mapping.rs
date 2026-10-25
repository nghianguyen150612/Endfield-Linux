//! Deterministic P007 display mapping without guessing physical identities.

use endfield_unix_domain::{
    Availability, CapabilityId, CapabilityRecord, Diagnostic, DiagnosticCategory, DisplayInfo,
    Hertz, InstanceId, ProvenanceId,
};
use std::collections::BTreeSet;

use crate::display::{
    BackendDisplayOutput, DisplayOutputSource, DisplayOutputState, LinuxDisplayMode,
    LinuxDisplayOutput, MAX_DISPLAY_OUTPUTS, OutputInventoryStatus, ProviderStatus,
};

pub(crate) struct MappedOutputs {
    pub outputs: Vec<LinuxDisplayOutput>,
    pub capabilities: Vec<CapabilityRecord>,
    pub diagnostics: Vec<Diagnostic>,
}

pub(crate) fn map_outputs(
    source: DisplayOutputSource,
    backend_outputs: &[BackendDisplayOutput],
    inventory: OutputInventoryStatus,
    session_status: ProviderStatus,
    provenance: ProvenanceId,
    provider: &'static str,
) -> MappedOutputs {
    let mut candidates = backend_outputs
        .iter()
        .take(MAX_DISPLAY_OUTPUTS)
        .cloned()
        .map(|output| {
            let label = safe_label(output.name.as_deref(), source);
            (label, output)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        (
            left.0.to_ascii_lowercase(),
            left.0.as_str(),
            output_sort_key(&left.1),
        )
            .cmp(&(
                right.0.to_ascii_lowercase(),
                right.0.as_str(),
                output_sort_key(&right.1),
            ))
    });

    let source_degraded = inventory == OutputInventoryStatus::Inconsistent;
    let mut outputs = Vec::new();
    let mut capabilities = Vec::new();
    let mut diagnostics = Vec::new();
    let mut used_labels = BTreeSet::new();
    for (index, (mut label, output)) in candidates.into_iter().enumerate() {
        let base_label = label.clone();
        if !used_labels.insert(label.clone()) {
            let mut suffix = 2usize;
            loop {
                label = format!("{base_label}-{suffix}");
                if used_labels.insert(label.clone()) {
                    break;
                }
                suffix += 1;
            }
            diagnostics.push(domain_diagnostic(
                DiagnosticCategory::MalformedSourceData,
                provider,
                "display output labels were duplicated and normalized",
            ));
        }
        let instance = InstanceId::new(
            crate::gpu_probe::DISPLAY_INSTANCE_ID_BASE + u32::try_from(index + 1).unwrap_or(64),
        )
        .expect("display instance range is nonzero");
        let current_mode = current_mode(&output.modes);
        let display_availability = match output.state {
            DisplayOutputState::Disconnected => Availability::Unavailable,
            DisplayOutputState::Unknown => Availability::Unknown,
            DisplayOutputState::Active | DisplayOutputState::ConnectedInactive
                if source_degraded =>
            {
                Availability::Degraded
            }
            DisplayOutputState::Active | DisplayOutputState::ConnectedInactive => {
                Availability::Available
            }
        };
        let diagnostic = if display_availability == Availability::Degraded {
            Some(domain_diagnostic(
                DiagnosticCategory::MalformedSourceData,
                provider,
                "display output metadata was partial or inconsistent",
            ))
        } else {
            None
        };
        let session_availability = match session_status {
            ProviderStatus::Connected => Availability::Available,
            ProviderStatus::TimedOut
            | ProviderStatus::Unavailable
            | ProviderStatus::PermissionDenied
            | ProviderStatus::AuthenticationFailed
            | ProviderStatus::Failed
            | ProviderStatus::RejectedRemote
            | ProviderStatus::InvalidEndpoint => Availability::Unknown,
            ProviderStatus::NotAdvertised | ProviderStatus::NotAttempted => Availability::Unknown,
        };
        let resolution = current_mode.and_then(LinuxDisplayMode::resolution);
        let refresh_rate = current_mode
            .and_then(|mode| mode.refresh_rate)
            .and_then(|rate| rate.exact_whole_hertz())
            .map(Hertz);
        let primary = (output.state != DisplayOutputState::Disconnected)
            .then_some(output.primary)
            .flatten();
        let info = DisplayInfo {
            instance,
            label: Some(label),
            availability: display_availability,
            resolution,
            refresh_rate,
            primary,
            connection: None,
            connection_availability: Availability::Unknown,
            session_availability,
        };
        let mut capability = CapabilityRecord::new(CapabilityId::Display, display_availability);
        capability.instance = Some(instance);
        capability.provenance = Some(provenance);
        capability.diagnostic = diagnostic.clone();
        capabilities.push(capability);

        let mut session_capability =
            CapabilityRecord::new(CapabilityId::CompositorSession, session_availability);
        session_capability.instance = Some(instance);
        if session_availability != Availability::Unknown {
            session_capability.provenance = Some(provenance);
        }
        capabilities.push(session_capability);
        if let Some(diagnostic) = diagnostic {
            diagnostics.push(diagnostic);
        }
        outputs.push(LinuxDisplayOutput {
            info,
            source,
            state: output.state,
            modes: output.modes,
            logical_geometry: output.logical_geometry,
            scale: output.scale,
            rotation_degrees: output.rotation_degrees,
        });
    }
    MappedOutputs {
        outputs,
        capabilities,
        diagnostics,
    }
}

pub(crate) fn add_platform_capability(
    capability: CapabilityId,
    availability: Availability,
    provenance: Option<ProvenanceId>,
    diagnostic: Option<Diagnostic>,
) -> CapabilityRecord {
    let mut record = CapabilityRecord::new(capability, availability);
    record.provenance = provenance;
    record.diagnostic = diagnostic;
    record
}

pub(crate) fn domain_diagnostic(
    category: DiagnosticCategory,
    provider: &'static str,
    message: &'static str,
) -> Diagnostic {
    Diagnostic::new(
        category,
        Some(provider.to_owned()),
        Some(message.to_owned()),
    )
    .expect("fixed display diagnostic validates")
}

fn safe_label(value: Option<&str>, source: DisplayOutputSource) -> String {
    let Some(value) = value.map(str::trim) else {
        return fallback_label(source);
    };
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return fallback_label(source);
    }
    value.to_owned()
}

fn fallback_label(source: DisplayOutputSource) -> String {
    match source {
        DisplayOutputSource::Wayland => "wayland-output".to_owned(),
        DisplayOutputSource::X11RandR => "x11-output".to_owned(),
    }
}

fn current_mode(modes: &[LinuxDisplayMode]) -> Option<&LinuxDisplayMode> {
    modes.iter().find(|mode| mode.current && mode.is_valid())
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct OutputSortKey {
    state: u8,
    primary: Option<bool>,
    geometry: Option<(i32, i32, u32, u32)>,
    scale: Option<(u32, u32)>,
    rotation_degrees: Option<u16>,
    modes: Vec<ModeSortKey>,
}

type ModeSortKey = (
    u32,
    u32,
    Option<crate::display::ExactRefreshRate>,
    bool,
    bool,
    Option<bool>,
    Option<bool>,
);

fn output_sort_key(output: &BackendDisplayOutput) -> OutputSortKey {
    let mut modes = output
        .modes
        .iter()
        .map(|mode| {
            (
                mode.width_pixels,
                mode.height_pixels,
                mode.refresh_rate,
                mode.current,
                mode.preferred,
                mode.interlaced,
                mode.doublescan,
            )
        })
        .collect::<Vec<_>>();
    modes.sort_unstable();
    OutputSortKey {
        state: match output.state {
            DisplayOutputState::Active => 0,
            DisplayOutputState::ConnectedInactive => 1,
            DisplayOutputState::Disconnected => 2,
            DisplayOutputState::Unknown => 3,
        },
        primary: output.primary,
        geometry: output
            .logical_geometry
            .map(|geometry| (geometry.x, geometry.y, geometry.width, geometry.height)),
        scale: output
            .scale
            .map(|scale| (scale.numerator, scale.denominator)),
        rotation_degrees: output.rotation_degrees,
        modes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::{ExactRefreshRate, LogicalGeometry};

    fn raw_output(name: &str, width: u32, refresh_millihertz: u32) -> BackendDisplayOutput {
        BackendDisplayOutput {
            name: Some(name.to_owned()),
            state: DisplayOutputState::Active,
            modes: vec![LinuxDisplayMode {
                width_pixels: width,
                height_pixels: 1080,
                refresh_rate: ExactRefreshRate::from_millihertz(refresh_millihertz),
                current: true,
                preferred: true,
                interlaced: None,
                doublescan: None,
            }],
            primary: None,
            logical_geometry: Some(LogicalGeometry {
                x: width as i32,
                y: 0,
                width,
                height: 1080,
            }),
            scale: None,
            rotation_degrees: None,
        }
    }

    #[test]
    fn precise_refresh_is_kept_and_only_whole_hertz_enters_p007() {
        let provenance = endfield_unix_domain::ProvenanceId::new(14).unwrap();
        let mapped = map_outputs(
            DisplayOutputSource::Wayland,
            &[raw_output("DP-1", 2560, 99_950)],
            OutputInventoryStatus::Enumerated,
            ProviderStatus::Connected,
            provenance,
            "wayland",
        );
        assert_eq!(
            mapped.outputs[0].modes[0].refresh_rate,
            ExactRefreshRate::new(1999, 20)
        );
        assert_eq!(mapped.outputs[0].info.refresh_rate, None);
        assert_eq!(
            mapped.outputs[0].info.resolution.unwrap().width_pixels,
            2560
        );
        assert!(mapped.outputs[0].logical_geometry.is_some());
    }

    #[test]
    fn output_ids_are_sorted_by_sanitized_identity_not_provider_order() {
        let provenance = endfield_unix_domain::ProvenanceId::new(14).unwrap();
        let mapped = map_outputs(
            DisplayOutputSource::Wayland,
            &[
                raw_output("DP-2", 1920, 60_000),
                raw_output("eDP-1", 2560, 60_000),
            ],
            OutputInventoryStatus::Enumerated,
            ProviderStatus::Connected,
            provenance,
            "wayland",
        );
        assert_eq!(mapped.outputs[0].info.label.as_deref(), Some("DP-2"));
        assert_eq!(
            mapped.outputs[0].info.instance.get(),
            crate::gpu_probe::DISPLAY_INSTANCE_ID_BASE + 1
        );
        assert_eq!(mapped.outputs[1].info.label.as_deref(), Some("eDP-1"));
        assert_eq!(
            mapped.outputs[1].info.instance.get(),
            crate::gpu_probe::DISPLAY_INSTANCE_ID_BASE + 2
        );
    }

    #[test]
    fn unsafe_provider_labels_are_not_exported() {
        assert_eq!(
            safe_label(
                Some("Monitor serial 0012345"),
                DisplayOutputSource::X11RandR
            ),
            "x11-output"
        );
        assert_eq!(
            safe_label(Some("../../private"), DisplayOutputSource::Wayland),
            "wayland-output"
        );
    }

    #[test]
    fn duplicate_and_suffix_colliding_labels_are_globally_unique() {
        let provenance = endfield_unix_domain::ProvenanceId::new(14).unwrap();
        let mapped = map_outputs(
            DisplayOutputSource::Wayland,
            &[
                raw_output("DP-1", 1920, 60_000),
                raw_output("DP-1", 2560, 60_000),
                raw_output("DP-1-2", 3840, 60_000),
            ],
            OutputInventoryStatus::Enumerated,
            ProviderStatus::Connected,
            provenance,
            "wayland",
        );
        let labels = mapped
            .outputs
            .iter()
            .map(|output| output.info.label.as_deref().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(labels.len(), 3);
        assert!(labels.contains("DP-1"));
        assert!(labels.contains("DP-1-2"));
        assert!(labels.contains("DP-1-2-2"));
    }

    #[test]
    fn duplicate_labels_with_matching_geometry_sort_by_observed_modes() {
        let provenance = endfield_unix_domain::ProvenanceId::new(14).unwrap();
        let first = raw_output("wayland-output", 1920, 60_000);
        let mut second = raw_output("unsafe display name", 1920, 60_000);
        second.modes[0].refresh_rate = ExactRefreshRate::from_millihertz(75_000);
        let map = |outputs: &[BackendDisplayOutput]| {
            map_outputs(
                DisplayOutputSource::Wayland,
                outputs,
                OutputInventoryStatus::Enumerated,
                ProviderStatus::Connected,
                provenance,
                "wayland",
            )
            .outputs
            .iter()
            .map(|output| {
                (
                    output.info.label.clone().unwrap(),
                    output.modes[0].refresh_rate,
                    output.info.instance,
                )
            })
            .collect::<Vec<_>>()
        };
        assert_eq!(map(&[first.clone(), second.clone()]), map(&[second, first]));
    }
}
