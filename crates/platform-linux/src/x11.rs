//! Read-only local X11/RandR output and mode discovery.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use x11rb::connection::{Connection, RequestConnection};
use x11rb::errors::{ConnectError, ConnectionError, ReplyError};
use x11rb::protocol::randr::{self, ConnectionExt as RandrConnectionExt};
use x11rb::protocol::xproto::ConnectionExt as XprotoConnectionExt;

use crate::display::{
    BackendDisplayOutput, BackendError, BackendErrorKind, BackendObservation, DisplayOutputState,
    ExactRefreshRate, LinuxDisplayMode, LogicalGeometry, MAX_DISPLAY_MODES, MAX_DISPLAY_OUTPUTS,
    OutputInventoryStatus,
};
use crate::session::LocalX11Endpoint;

const QUERY_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_SERVER_MODES: usize = MAX_DISPLAY_OUTPUTS * MAX_DISPLAY_MODES;

pub trait X11DisplayBackend: Send + Sync {
    fn inspect(&self, endpoint: &LocalX11Endpoint) -> Result<BackendObservation, BackendError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemX11Backend;

impl X11DisplayBackend for SystemX11Backend {
    fn inspect(&self, endpoint: &LocalX11Endpoint) -> Result<BackendObservation, BackendError> {
        let endpoint = endpoint.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("endfield-x11-display-probe".to_owned())
            .spawn(move || {
                let result = inspect_local_server(&endpoint);
                let _ = sender.send(result);
            })
            .map_err(|_| BackendError {
                kind: BackendErrorKind::Other,
            })?;
        receiver
            .recv_timeout(QUERY_TIMEOUT)
            .map_err(|error| BackendError {
                kind: match error {
                    mpsc::RecvTimeoutError::Timeout => BackendErrorKind::TimedOut,
                    mpsc::RecvTimeoutError::Disconnected => BackendErrorKind::Unavailable,
                },
            })?
    }
}

fn inspect_local_server(endpoint: &LocalX11Endpoint) -> Result<BackendObservation, BackendError> {
    // `LocalX11Endpoint` can only be constructed from a local `:display[.screen]`
    // value, so x11rb cannot select its TCP transport here. Normal Xauthority
    // lookup and authentication remain owned by x11rb.
    let (connection, screen_index) =
        x11rb::connect(Some(endpoint.display())).map_err(map_connect_error)?;
    let roots = &connection.setup().roots;
    let screen = roots.get(screen_index).ok_or(BackendError {
        kind: BackendErrorKind::Malformed,
    })?;
    let geometry = connection
        .get_geometry(screen.root)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| {
            (reply.width > 0 && reply.height > 0)
                .then_some((u32::from(reply.width), u32::from(reply.height)))
        });

    let extension = connection
        .extension_information(randr::X11_EXTENSION_NAME)
        .map_err(map_connection_error)?;
    if extension.is_none() {
        return Ok(protocol_unavailable_observation(
            None,
            geometry,
            "X11 connected but the RandR extension is unavailable",
        ));
    }

    let version_cookie = match connection.randr_query_version(1, 6) {
        Ok(cookie) => cookie,
        Err(ConnectionError::UnsupportedExtension) => {
            return Ok(protocol_unavailable_observation(
                None,
                geometry,
                "X11 connected but RandR version negotiation is unavailable",
            ));
        }
        Err(error) => return Err(map_connection_error(error)),
    };
    let version = match version_cookie.reply() {
        Ok(reply) => reply,
        Err(ReplyError::X11Error(_)) => {
            return Ok(protocol_unavailable_observation(
                None,
                geometry,
                "X11 connected but RandR version negotiation was rejected",
            ));
        }
        Err(error) => return Err(map_reply_error(error)),
    };
    let version_pair = (version.major_version, version.minor_version);
    let resources_cookie = match connection.randr_get_screen_resources_current(screen.root) {
        Ok(cookie) => cookie,
        Err(ConnectionError::UnsupportedExtension) => {
            return Ok(protocol_unavailable_observation(
                Some(version_pair),
                geometry,
                "X11 connected but the negotiated RandR version has no current resources query",
            ));
        }
        Err(error) => return Err(map_connection_error(error)),
    };
    let resources = match resources_cookie.reply() {
        Ok(reply) => reply,
        Err(ReplyError::X11Error(_)) => {
            return Ok(protocol_unavailable_observation(
                Some(version_pair),
                geometry,
                "X11 connected but the RandR server rejected the current resources query",
            ));
        }
        Err(error) => return Err(map_reply_error(error)),
    };
    let mut inconsistent = false;
    if resources.outputs.len() > MAX_DISPLAY_OUTPUTS || resources.modes.len() > MAX_SERVER_MODES {
        inconsistent = true;
    }
    let mut modes_by_id = BTreeMap::new();
    for mode in resources.modes.iter().take(MAX_SERVER_MODES) {
        if modes_by_id.insert(mode.id, mode).is_some() {
            inconsistent = true;
        }
    }
    let known_crtcs = resources.crtcs.iter().copied().collect::<BTreeSet<_>>();
    if known_crtcs.len() != resources.crtcs.len() {
        inconsistent = true;
    }
    let primary_reported = connection
        .randr_get_output_primary(screen.root)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.output);
    let (primary, invalid_primary) = validated_primary_output(
        primary_reported,
        &resources.outputs[..resources.outputs.len().min(MAX_DISPLAY_OUTPUTS)],
    );
    inconsistent |= invalid_primary;

    let mut outputs = Vec::new();
    let mut output_ids = BTreeSet::new();
    for output_id in resources.outputs.iter().take(MAX_DISPLAY_OUTPUTS) {
        if !output_ids.insert(*output_id) {
            inconsistent = true;
            continue;
        }
        let info = match connection
            .randr_get_output_info(*output_id, resources.config_timestamp)
            .map_err(map_connection_error)
            .and_then(|cookie| cookie.reply().map_err(map_reply_error))
        {
            Ok(info) if info.status == randr::SetConfig::SUCCESS => info,
            _ => {
                inconsistent = true;
                continue;
            }
        };
        if info.modes.len() > MAX_DISPLAY_MODES
            || usize::from(info.num_preferred) > info.modes.len()
        {
            inconsistent = true;
        }
        let preferred_count = usize::from(info.num_preferred).min(info.modes.len());
        let mut modes = Vec::new();
        let mut mode_indices = BTreeMap::new();
        for (index, mode_id) in info.modes.iter().take(MAX_DISPLAY_MODES).enumerate() {
            let Some(mode) = modes_by_id.get(mode_id).copied() else {
                inconsistent = true;
                continue;
            };
            if mode.width == 0 || mode.height == 0 {
                inconsistent = true;
                continue;
            }
            if mode_indices.contains_key(mode_id) {
                inconsistent = true;
            }
            let mode_index = modes.len();
            modes.push(LinuxDisplayMode {
                width_pixels: u32::from(mode.width),
                height_pixels: u32::from(mode.height),
                refresh_rate: randr_refresh_rate(mode),
                current: false,
                preferred: index < preferred_count,
                interlaced: Some(mode.mode_flags.contains(randr::ModeFlag::INTERLACE)),
                doublescan: Some(mode.mode_flags.contains(randr::ModeFlag::DOUBLE_SCAN)),
            });
            mode_indices.insert(*mode_id, mode_index);
        }

        let mut state = match info.connection {
            value if value == randr::Connection::DISCONNECTED => DisplayOutputState::Disconnected,
            value if value == randr::Connection::CONNECTED => DisplayOutputState::ConnectedInactive,
            _ => DisplayOutputState::Unknown,
        };
        let mut logical_geometry = None;
        let mut rotation = None;
        let mut active_mode = None;
        if info.connection == randr::Connection::DISCONNECTED && info.crtc != 0 {
            inconsistent = true;
            state = DisplayOutputState::Unknown;
        } else if info.crtc != 0 {
            if !known_crtcs.contains(&info.crtc) {
                inconsistent = true;
                state = DisplayOutputState::Unknown;
            } else {
                match connection
                    .randr_get_crtc_info(info.crtc, resources.config_timestamp)
                    .map_err(map_connection_error)
                    .and_then(|cookie| cookie.reply().map_err(map_reply_error))
                {
                    Ok(crtc) if crtc.status == randr::SetConfig::SUCCESS => {
                        if !crtc.outputs.contains(output_id) {
                            inconsistent = true;
                            state = DisplayOutputState::Unknown;
                        } else if crtc.mode != 0 && crtc.width > 0 && crtc.height > 0 {
                            active_mode = Some(crtc.mode);
                            state = DisplayOutputState::Active;
                            logical_geometry = Some(LogicalGeometry {
                                x: i32::from(crtc.x),
                                y: i32::from(crtc.y),
                                width: u32::from(crtc.width),
                                height: u32::from(crtc.height),
                            });
                            rotation = rotation_degrees(crtc.rotation);
                        } else if state == DisplayOutputState::ConnectedInactive {
                            state = DisplayOutputState::ConnectedInactive;
                        }
                    }
                    _ => {
                        inconsistent = true;
                        state = DisplayOutputState::Unknown;
                    }
                }
            }
        }
        if let Some(active_mode) = active_mode {
            if let Some(mode) = modes_by_id.get(&active_mode).copied() {
                if let Some(index) = mode_indices.get(&active_mode).copied() {
                    modes[index].current = true;
                } else {
                    inconsistent = true;
                    modes.push(mode_to_linux(mode, true, false));
                }
            } else {
                inconsistent = true;
            }
        }
        normalize_local_modes(&mut modes);
        let name = if info.name.len() > 64 {
            inconsistent = true;
            None
        } else {
            match std::str::from_utf8(&info.name) {
                Ok(name) => Some(name.to_owned()),
                Err(_) => {
                    inconsistent = true;
                    None
                }
            }
        };
        outputs.push(BackendDisplayOutput {
            name,
            state,
            modes,
            primary: primary.map(|primary| primary == *output_id),
            logical_geometry,
            scale: None,
            rotation_degrees: rotation,
        });
    }
    if outputs.len() != resources.outputs.len().min(MAX_DISPLAY_OUTPUTS) {
        inconsistent = true;
    }
    Ok(BackendObservation {
        inventory_status: if inconsistent {
            OutputInventoryStatus::Inconsistent
        } else {
            OutputInventoryStatus::Enumerated
        },
        outputs,
        optional_logical_output_protocol: false,
        protocol_version: Some(version_pair),
        screen_geometry: geometry,
        diagnostic: inconsistent
            .then_some("X11 RandR returned partial or inconsistent output metadata"),
    })
}

fn mode_to_linux(mode: &randr::ModeInfo, current: bool, preferred: bool) -> LinuxDisplayMode {
    LinuxDisplayMode {
        width_pixels: u32::from(mode.width),
        height_pixels: u32::from(mode.height),
        refresh_rate: randr_refresh_rate(mode),
        current,
        preferred,
        interlaced: Some(mode.mode_flags.contains(randr::ModeFlag::INTERLACE)),
        doublescan: Some(mode.mode_flags.contains(randr::ModeFlag::DOUBLE_SCAN)),
    }
}

pub(crate) fn randr_refresh_rate(mode: &randr::ModeInfo) -> Option<ExactRefreshRate> {
    if mode.dot_clock == 0
        || mode.htotal == 0
        || mode.vtotal == 0
        || mode.mode_flags.contains(randr::ModeFlag::DOUBLE_CLOCK)
        || mode.mode_flags.contains(randr::ModeFlag::HALVE_CLOCK)
    {
        return None;
    }
    let mut numerator = u64::from(mode.dot_clock);
    let mut denominator = u64::from(mode.htotal).checked_mul(u64::from(mode.vtotal))?;
    if mode.mode_flags.contains(randr::ModeFlag::INTERLACE) {
        numerator = numerator.checked_mul(2)?;
    }
    if mode.mode_flags.contains(randr::ModeFlag::DOUBLE_SCAN) {
        denominator = denominator.checked_mul(2)?;
    }
    ExactRefreshRate::new(numerator, denominator)
}

fn protocol_unavailable_observation(
    version: Option<(u32, u32)>,
    geometry: Option<(u32, u32)>,
    diagnostic: &'static str,
) -> BackendObservation {
    BackendObservation {
        inventory_status: OutputInventoryStatus::ProtocolUnavailable,
        outputs: Vec::new(),
        optional_logical_output_protocol: false,
        protocol_version: version,
        screen_geometry: geometry,
        diagnostic: Some(diagnostic),
    }
}

fn validated_primary_output(reported: Option<u32>, outputs: &[u32]) -> (Option<u32>, bool) {
    match reported {
        Some(0) => (None, false),
        Some(primary) if outputs.contains(&primary) => (Some(primary), false),
        Some(_) | None => (None, true),
    }
}

fn normalize_local_modes(modes: &mut Vec<LinuxDisplayMode>) {
    let mut normalized: Vec<LinuxDisplayMode> = Vec::new();
    for mode in modes.drain(..).filter(|mode| mode.is_valid()) {
        if let Some(existing) = normalized.iter_mut().find(|existing| {
            existing.width_pixels == mode.width_pixels
                && existing.height_pixels == mode.height_pixels
                && existing.refresh_rate == mode.refresh_rate
                && existing.interlaced == mode.interlaced
                && existing.doublescan == mode.doublescan
        }) {
            existing.current |= mode.current;
            existing.preferred |= mode.preferred;
        } else if normalized.len() < MAX_DISPLAY_MODES {
            normalized.push(mode);
        }
    }
    normalized.sort_by_key(|mode| {
        (
            !mode.current,
            !mode.preferred,
            mode.width_pixels,
            mode.height_pixels,
            mode.refresh_rate,
        )
    });
    *modes = normalized;
}

fn rotation_degrees(rotation: randr::Rotation) -> Option<u16> {
    if rotation.contains(randr::Rotation::ROTATE90) {
        Some(90)
    } else if rotation.contains(randr::Rotation::ROTATE180) {
        Some(180)
    } else if rotation.contains(randr::Rotation::ROTATE270) {
        Some(270)
    } else if rotation.contains(randr::Rotation::ROTATE0) {
        Some(0)
    } else {
        None
    }
}

fn map_connect_error(error: ConnectError) -> BackendError {
    let kind = match error {
        ConnectError::SetupAuthenticate(_) | ConnectError::SetupFailed(_) => {
            BackendErrorKind::AuthenticationFailed
        }
        ConnectError::IoError(error) => match error.kind() {
            std::io::ErrorKind::PermissionDenied => BackendErrorKind::PermissionDenied,
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
                BackendErrorKind::TimedOut
            }
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                BackendErrorKind::Unavailable
            }
            _ => BackendErrorKind::Other,
        },
        ConnectError::DisplayParsingError(_) => BackendErrorKind::Malformed,
        ConnectError::InvalidScreen => BackendErrorKind::Malformed,
        _ => BackendErrorKind::Unavailable,
    };
    BackendError { kind }
}

fn map_connection_error(error: ConnectionError) -> BackendError {
    let kind = match error {
        ConnectionError::UnsupportedExtension => BackendErrorKind::Unsupported,
        ConnectionError::IoError(error) => match error.kind() {
            std::io::ErrorKind::PermissionDenied => BackendErrorKind::PermissionDenied,
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
                BackendErrorKind::TimedOut
            }
            _ => BackendErrorKind::Unavailable,
        },
        _ => BackendErrorKind::Malformed,
    };
    BackendError { kind }
}

fn map_reply_error(error: ReplyError) -> BackendError {
    match error {
        ReplyError::ConnectionError(error) => map_connection_error(error),
        ReplyError::X11Error(_) => BackendError {
            kind: BackendErrorKind::Malformed,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn randr_refresh_calculation_preserves_fractional_timing_and_scan_flags() {
        let mode = randr::ModeInfo {
            id: 1,
            width: 1920,
            height: 1080,
            dot_clock: 148_351_648,
            hsync_start: 0,
            hsync_end: 0,
            htotal: 2200,
            hskew: 0,
            vsync_start: 0,
            vsync_end: 0,
            vtotal: 1125,
            name_len: 0,
            mode_flags: randr::ModeFlag::default(),
        };
        assert_eq!(
            randr_refresh_rate(&mode),
            ExactRefreshRate::new(148_351_648, 2_475_000)
        );
        assert_eq!(
            randr_refresh_rate(&mode).and_then(ExactRefreshRate::exact_whole_hertz),
            None
        );
    }

    #[test]
    fn invalid_randr_timing_denominators_stay_unknown() {
        let mut mode = randr::ModeInfo {
            id: 1,
            width: 1920,
            height: 1080,
            dot_clock: 148_500_000,
            hsync_start: 0,
            hsync_end: 0,
            htotal: 2200,
            hskew: 0,
            vsync_start: 0,
            vsync_end: 0,
            vtotal: 1125,
            name_len: 0,
            mode_flags: randr::ModeFlag::default(),
        };
        assert_eq!(randr_refresh_rate(&mode), ExactRefreshRate::new(60, 1));
        mode.htotal = 0;
        assert_eq!(randr_refresh_rate(&mode), None);
        mode.htotal = 1;
        mode.mode_flags = randr::ModeFlag::INTERLACE | randr::ModeFlag::DOUBLE_SCAN;
        assert_eq!(
            randr_refresh_rate(&mode),
            ExactRefreshRate::new(148_500_000, 1125)
        );
        mode.mode_flags = randr::ModeFlag::DOUBLE_CLOCK;
        assert_eq!(randr_refresh_rate(&mode), None);
    }

    #[test]
    fn absent_randr_primary_is_unknown_and_invalid_primary_degrades_inventory() {
        assert_eq!(validated_primary_output(Some(0), &[1, 2]), (None, false));
        assert_eq!(validated_primary_output(Some(2), &[1, 2]), (Some(2), false));
        assert_eq!(validated_primary_output(Some(3), &[1, 2]), (None, true));
        assert_eq!(validated_primary_output(None, &[1, 2]), (None, true));
    }
}
