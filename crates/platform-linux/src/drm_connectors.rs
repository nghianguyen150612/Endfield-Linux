//! Conservative parsing and P013-backed correlation for DRM connectors.

use std::collections::{BTreeMap, BTreeSet};

use endfield_unix_domain::{DisplayConnection, InstanceId, Resolution};

use crate::display::{ConnectorEnabled, ConnectorState, DrmConnector, MAX_DRM_CONNECTORS};
use crate::display_sources::LinuxDisplayDataSource;
use crate::errors::{DataSourceError, DataSourceErrorKind};
use crate::gpu_probe::{GpuSourceStatus, LinuxGpuReport};
use crate::gpu_sources::DrmNodeKind;
use crate::pci::PciBdf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DrmConnectorCollection {
    pub status: GpuSourceStatus,
    /// `None` means enumeration failed; `Some(empty)` is an authoritative zero.
    pub connectors: Option<Vec<DrmConnector>>,
    pub issues: Vec<(&'static str, &'static str)>,
}

pub(crate) fn collect<S: LinuxDisplayDataSource>(
    source: &S,
    gpu: &LinuxGpuReport,
) -> DrmConnectorCollection {
    let raw = match source.enumerate_drm_connectors() {
        Ok(raw) => raw,
        Err(error) => {
            return DrmConnectorCollection {
                status: if error.kind == DataSourceErrorKind::NotFound {
                    GpuSourceStatus::Missing
                } else {
                    GpuSourceStatus::Degraded
                },
                connectors: None,
                issues: vec![(
                    "DRM connector source could not be enumerated",
                    "linux-drm-connectors",
                )],
            };
        }
    };
    let mut raw = raw;
    raw.sort_by(|left, right| {
        (&left.card_name, &left.connector_name).cmp(&(&right.card_name, &right.connector_name))
    });
    let mut issues = Vec::new();
    if raw.len() > MAX_DRM_CONNECTORS {
        raw.truncate(MAX_DRM_CONNECTORS);
        issues.push((
            "DRM connector enumeration exceeded its limit",
            "linux-drm-connectors",
        ));
    }

    let gpu_by_bdf = pci_gpu_map(gpu);
    let mut output = Vec::new();
    let mut identities = BTreeSet::new();
    for record in raw {
        if !valid_card_name(&record.card_name) || !valid_connector_name(&record.connector_name) {
            issues.push((
                "DRM connector identifier was malformed",
                "linux-drm-connectors",
            ));
            continue;
        }
        let identity = (record.card_name.clone(), record.connector_name.clone());
        if !identities.insert(identity) {
            issues.push((
                "duplicate DRM connector was ignored",
                "linux-drm-connectors",
            ));
            continue;
        }
        let pci_bdf = match record.pci_bdf.as_deref() {
            Some(value) => match value.parse::<PciBdf>() {
                Ok(bdf) => Some(bdf.to_string()),
                Err(_) => {
                    issues.push((
                        "DRM connector device relationship was malformed",
                        "linux-drm-connectors",
                    ));
                    None
                }
            },
            None => None,
        };
        let status = parse_state(&record.status, &mut issues);
        let enabled = parse_enabled(&record.enabled, &mut issues);
        let advertised_modes = parse_modes(&record.modes, &mut issues);
        let gpu_instance = pci_bdf.as_deref().and_then(|bdf| {
            let has_matching_card = gpu.drm_nodes.iter().any(|node| {
                node.name == record.card_name
                    && node.kind == DrmNodeKind::Primary
                    && node.pci_bdf.as_deref() == Some(bdf)
            });
            has_matching_card
                .then(|| gpu_by_bdf.get(bdf).copied())
                .flatten()
        });
        if pci_bdf.is_some() && gpu_instance.is_none() {
            issues.push((
                "DRM connector could not be mapped to a unique P013 GPU",
                "linux-drm-connectors",
            ));
        }
        let connection = connector_classification(&record.connector_name);
        output.push(DrmConnector {
            label: record.connector_name,
            state: status,
            enabled,
            advertised_modes,
            connection,
            gpu_instance,
        });
    }

    let status = if issues.is_empty() {
        GpuSourceStatus::Available
    } else {
        GpuSourceStatus::Degraded
    };
    DrmConnectorCollection {
        status,
        connectors: Some(output),
        issues,
    }
}

fn pci_gpu_map(gpu: &LinuxGpuReport) -> BTreeMap<String, InstanceId> {
    gpu.pci_devices
        .iter()
        .zip(&gpu.snapshot.gpus)
        .map(|(pci, info)| (pci.bdf.clone(), info.instance))
        .collect()
}

fn valid_card_name(value: &str) -> bool {
    value.strip_prefix("card").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.len() <= 4 && suffix.bytes().all(|b| b.is_ascii_digit())
    })
}

fn valid_connector_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn parse_state(
    value: &Result<Vec<u8>, DataSourceError>,
    issues: &mut Vec<(&'static str, &'static str)>,
) -> ConnectorState {
    match parse_ascii_value(value) {
        Ok("connected") => ConnectorState::Connected,
        Ok("disconnected") => ConnectorState::Disconnected,
        Ok("unknown") | Err(DataSourceErrorKind::NotFound) => ConnectorState::Unknown,
        Ok(_) | Err(_) => {
            issues.push((
                "DRM connector status was malformed or unavailable",
                "linux-drm-connectors",
            ));
            ConnectorState::Unknown
        }
    }
}

fn parse_enabled(
    value: &Result<Vec<u8>, DataSourceError>,
    issues: &mut Vec<(&'static str, &'static str)>,
) -> ConnectorEnabled {
    match parse_ascii_value(value) {
        Ok("enabled") => ConnectorEnabled::Enabled,
        Ok("disabled") => ConnectorEnabled::Disabled,
        Ok("unknown") | Err(DataSourceErrorKind::NotFound) => ConnectorEnabled::Unknown,
        Ok(_) | Err(_) => {
            issues.push((
                "DRM connector enabled state was malformed or unavailable",
                "linux-drm-connectors",
            ));
            ConnectorEnabled::Unknown
        }
    }
}

fn parse_modes(
    value: &Result<Vec<u8>, DataSourceError>,
    issues: &mut Vec<(&'static str, &'static str)>,
) -> Option<Vec<Resolution>> {
    let bytes = match value {
        Ok(bytes) => bytes,
        Err(DataSourceError {
            kind: DataSourceErrorKind::NotFound,
        }) => return None,
        Err(_) => {
            issues.push((
                "DRM connector mode list could not be read",
                "linux-drm-connectors",
            ));
            return None;
        }
    };
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => {
            issues.push((
                "DRM connector mode list was not valid UTF-8",
                "linux-drm-connectors",
            ));
            return None;
        }
    };
    let mut modes = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        if index >= crate::display::MAX_DISPLAY_MODES {
            issues.push((
                "DRM connector mode count exceeded its limit",
                "linux-drm-connectors",
            ));
            break;
        }
        let Some((width, height)) = line.trim().split_once('x') else {
            issues.push(("DRM connector mode was malformed", "linux-drm-connectors"));
            continue;
        };
        let parsed = width
            .parse::<u32>()
            .ok()
            .zip(height.parse::<u32>().ok())
            .and_then(|(width, height)| Resolution::new(width, height).ok());
        if let Some(mode) = parsed {
            modes.insert((mode.width_pixels, mode.height_pixels));
        } else {
            issues.push((
                "DRM connector mode dimensions were invalid",
                "linux-drm-connectors",
            ));
        }
    }
    Some(
        modes
            .into_iter()
            .filter_map(|(width, height)| Resolution::new(width, height).ok())
            .collect(),
    )
}

fn parse_ascii_value(
    value: &Result<Vec<u8>, DataSourceError>,
) -> Result<&str, DataSourceErrorKind> {
    let bytes = value.as_ref().map_err(|error| error.kind)?;
    let text = std::str::from_utf8(bytes).map_err(|_| DataSourceErrorKind::InvalidUtf8)?;
    let text = text.trim();
    if text.len() > 64 || text.chars().any(char::is_control) {
        return Err(DataSourceErrorKind::MalformedData);
    }
    Ok(text)
}

fn connector_classification(name: &str) -> DisplayConnection {
    if ["eDP-", "LVDS-", "DSI-"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
    {
        DisplayConnection::Internal
    } else if [
        "DP-",
        "HDMI-",
        "VGA-",
        "DVI-",
        "USB-C-",
        "TV-",
        "SVIDEO-",
        "DisplayPort-",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
    {
        DisplayConnection::External
    } else if name.starts_with("Virtual-") || name.starts_with("Writeback-") {
        DisplayConnection::Virtual
    } else {
        DisplayConnection::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connector_modes_preserve_only_valid_dimensions_and_are_deduplicated() {
        let mut issues = Vec::new();
        let modes = parse_modes(
            &Ok(b"1920x1080\n2560x1440\n1920x1080\n0x720\nprivate\n".to_vec()),
            &mut issues,
        );
        let modes = modes.unwrap();
        assert_eq!(modes.len(), 2);
        assert_eq!(modes[0].width_pixels, 1920);
        assert!(issues.len() >= 2);
    }

    #[test]
    fn connector_classification_uses_kernel_connector_types_only() {
        assert_eq!(
            connector_classification("eDP-1"),
            DisplayConnection::Internal
        );
        assert_eq!(
            connector_classification("HDMI-A-1"),
            DisplayConnection::External
        );
        assert_eq!(
            connector_classification("Virtual-1"),
            DisplayConnection::Virtual
        );
        assert_eq!(
            connector_classification("monitor-serial-1"),
            DisplayConnection::Unknown
        );
    }

    #[test]
    fn connector_mode_line_limit_is_reported_even_when_entries_repeat() {
        let mut issues = Vec::new();
        let input = "1920x1080\n".repeat(crate::display::MAX_DISPLAY_MODES + 1);
        let modes = parse_modes(&Ok(input.into_bytes()), &mut issues);
        assert_eq!(modes.as_ref().unwrap().len(), 1);
        assert!(issues.contains(&(
            "DRM connector mode count exceeded its limit",
            "linux-drm-connectors"
        )));
    }

    #[test]
    fn missing_and_empty_connector_mode_lists_remain_distinct() {
        let mut issues = Vec::new();
        assert_eq!(
            parse_modes(
                &Err(crate::errors::DataSourceError::new(
                    DataSourceErrorKind::NotFound
                )),
                &mut issues
            ),
            None
        );
        assert_eq!(parse_modes(&Ok(Vec::new()), &mut issues), Some(Vec::new()));
        assert!(issues.is_empty());
    }
}
