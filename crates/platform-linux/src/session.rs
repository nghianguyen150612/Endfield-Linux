//! Bounded, privacy-preserving graphical-session hints and local endpoint validation.

use std::path::{Component, Path, PathBuf};

use crate::display::{SessionAssessment, SessionTypeHint};
use crate::errors::{DataSourceError, DataSourceErrorKind};
use crate::sources::LinuxDataSource;

pub const MAX_SESSION_HINT_BYTES: usize = 128;

/// Values from the five allowlisted environment keys. They are input hints and
/// are never copied verbatim into the public report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionEnvironmentHints {
    pub xdg_session_type: Option<String>,
    pub wayland_display: Option<String>,
    pub x11_display: Option<String>,
    pub current_desktop: Option<String>,
    pub session_desktop: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalWaylandEndpoint {
    socket_name: String,
    socket_path: PathBuf,
}

impl LocalWaylandEndpoint {
    pub fn socket_name(&self) -> &str {
        &self.socket_name
    }

    pub(crate) fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalX11Endpoint {
    display: String,
}

impl LocalX11Endpoint {
    pub fn display(&self) -> &str {
        &self.display
    }
}

pub(crate) fn assess_hints(hints: &SessionEnvironmentHints) -> SessionAssessment {
    let xdg_session_type = hints
        .xdg_session_type
        .as_deref()
        .and_then(parse_session_type);
    let has_wayland = hints.wayland_display.is_some();
    let has_x11 = hints.x11_display.is_some();
    let hint = match (has_wayland, has_x11, xdg_session_type) {
        (true, true, _) => SessionTypeHint::Mixed,
        (true, false, Some(SessionTypeHint::X11 | SessionTypeHint::Headless)) => {
            SessionTypeHint::Mixed
        }
        (false, true, Some(SessionTypeHint::Wayland | SessionTypeHint::Headless)) => {
            SessionTypeHint::Mixed
        }
        (true, false, _) => SessionTypeHint::Wayland,
        (false, true, _) => SessionTypeHint::X11,
        (false, false, Some(SessionTypeHint::Wayland)) => SessionTypeHint::Wayland,
        (false, false, Some(SessionTypeHint::X11)) => SessionTypeHint::X11,
        (false, false, Some(SessionTypeHint::Headless)) => SessionTypeHint::Headless,
        (false, false, None)
            if hints.current_desktop.is_some() || hints.session_desktop.is_some() =>
        {
            SessionTypeHint::Unknown
        }
        (false, false, None) if hints.xdg_session_type.is_none() => SessionTypeHint::Headless,
        _ => SessionTypeHint::Unknown,
    };

    SessionAssessment {
        hint,
        xdg_session_type,
        wayland_endpoint: if has_wayland {
            super::display::ProviderStatus::NotAttempted
        } else {
            super::display::ProviderStatus::NotAdvertised
        },
        x11_endpoint: if has_x11 {
            super::display::ProviderStatus::NotAttempted
        } else {
            super::display::ProviderStatus::NotAdvertised
        },
        verified_protocol: super::display::VerifiedDisplayProtocol::Unknown,
    }
}

pub(crate) fn parse_session_type(value: &str) -> Option<SessionTypeHint> {
    match value.trim().to_ascii_lowercase().as_str() {
        "wayland" => Some(SessionTypeHint::Wayland),
        "x11" => Some(SessionTypeHint::X11),
        "tty" => Some(SessionTypeHint::Headless),
        _ => None,
    }
}

pub(crate) fn parse_wayland_socket_name(value: &str) -> Result<&str, DataSourceError> {
    if value.is_empty()
        || value.len() > 96
        || value == "."
        || value == ".."
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(DataSourceError::new(DataSourceErrorKind::UnsafeSource));
    }
    Ok(value)
}

pub(crate) fn wayland_endpoint<S: LinuxDataSource>(
    source: &S,
    hints: &SessionEnvironmentHints,
) -> Result<LocalWaylandEndpoint, DataSourceError> {
    let name = hints
        .wayland_display
        .as_deref()
        .ok_or_else(|| DataSourceError::new(DataSourceErrorKind::NotFound))?;
    let name = parse_wayland_socket_name(name)?;
    let runtime_dir = source
        .runtime_directory_path()?
        .ok_or_else(|| DataSourceError::new(DataSourceErrorKind::NotFound))?;
    validate_runtime_directory(source, &runtime_dir)?;
    let socket_path = runtime_dir.join(name);
    if socket_path.as_os_str().as_encoded_bytes().len() >= 108 {
        return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
    }
    Ok(LocalWaylandEndpoint {
        socket_name: name.to_owned(),
        socket_path,
    })
}

pub(crate) fn validate_wayland_socket<S: LinuxDataSource>(
    source: &S,
    endpoint: &LocalWaylandEndpoint,
) -> Result<(), DataSourceError> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let metadata = std::fs::symlink_metadata(endpoint.socket_path())
        .map_err(|error| map_io_error(error.kind()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_socket() {
        return Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType));
    }
    if metadata.uid() != source.effective_uid() {
        return Err(DataSourceError::new(DataSourceErrorKind::UnsafeSource));
    }
    Ok(())
}

pub(crate) fn x11_endpoint(
    hints: &SessionEnvironmentHints,
) -> Result<LocalX11Endpoint, DataSourceError> {
    let value = hints
        .x11_display
        .as_deref()
        .ok_or_else(|| DataSourceError::new(DataSourceErrorKind::NotFound))?;
    let display = value.strip_prefix("unix/").unwrap_or(value);
    let rest = display
        .strip_prefix(':')
        .ok_or_else(|| DataSourceError::new(DataSourceErrorKind::UnsafeSource))?;
    let (number, screen) = match rest.split_once('.') {
        Some((number, screen)) if !screen.is_empty() => (number, Some(screen)),
        Some(_) => return Err(DataSourceError::new(DataSourceErrorKind::MalformedData)),
        None => (rest, None),
    };
    let number = number
        .parse::<u16>()
        .map_err(|_| DataSourceError::new(DataSourceErrorKind::MalformedData))?;
    if let Some(screen) = screen {
        let _ = screen
            .parse::<u16>()
            .map_err(|_| DataSourceError::new(DataSourceErrorKind::MalformedData))?;
    }
    let normalized = match screen {
        Some(screen) => format!(":{number}.{screen}"),
        None => format!(":{number}"),
    };
    Ok(LocalX11Endpoint {
        display: normalized,
    })
}

fn validate_runtime_directory<S: LinuxDataSource>(
    source: &S,
    path: &Path,
) -> Result<(), DataSourceError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(DataSourceError::new(DataSourceErrorKind::UnsafeSource));
    }
    let metadata = source.runtime_directory_metadata(path)?;
    if !metadata.is_directory || metadata.is_symlink || metadata.owner_uid != source.effective_uid()
    {
        return Err(DataSourceError::new(DataSourceErrorKind::UnsafeSource));
    }
    if metadata.mode & 0o077 != 0 {
        return Err(DataSourceError::new(DataSourceErrorKind::UnsafeSource));
    }
    Ok(())
}

fn map_io_error(kind: std::io::ErrorKind) -> DataSourceError {
    match kind {
        std::io::ErrorKind::NotFound => DataSourceError::new(DataSourceErrorKind::NotFound),
        std::io::ErrorKind::PermissionDenied => {
            DataSourceError::new(DataSourceErrorKind::PermissionDenied)
        }
        _ => DataSourceError::new(DataSourceErrorKind::Other),
    }
}

pub(crate) fn valid_hint(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SESSION_HINT_BYTES
        && !value.chars().any(char::is_control)
}

pub(crate) fn safe_desktop_names(hints: &SessionEnvironmentHints) -> Vec<String> {
    let mut names = Vec::<String>::new();
    for value in [
        hints.current_desktop.as_deref(),
        hints.session_desktop.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if !valid_hint(value) {
            continue;
        }
        for token in value.split([':', ';', ',']).take(8) {
            let token = token.trim();
            if token.is_empty()
                || token.len() > 32
                || !token
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
            {
                continue;
            }
            if !names.iter().any(|name| name.eq_ignore_ascii_case(token)) {
                names.push(token.to_owned());
            }
            if names.len() == 8 {
                return names;
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_type_hints_remain_distinct_from_verified_connections() {
        let hints = SessionEnvironmentHints {
            xdg_session_type: Some("wayland".to_owned()),
            wayland_display: Some("wayland-0".to_owned()),
            x11_display: Some(":0".to_owned()),
            ..SessionEnvironmentHints::default()
        };
        let assessed = assess_hints(&hints);
        assert_eq!(assessed.hint, SessionTypeHint::Mixed);
        assert_eq!(
            assessed.verified_protocol,
            super::super::display::VerifiedDisplayProtocol::Unknown
        );
        assert_eq!(
            assessed.wayland_endpoint,
            super::super::display::ProviderStatus::NotAttempted
        );
    }

    #[test]
    fn conflicting_session_and_endpoint_hints_are_ambiguous() {
        let assessed = assess_hints(&SessionEnvironmentHints {
            xdg_session_type: Some("tty".to_owned()),
            wayland_display: Some("wayland-0".to_owned()),
            ..SessionEnvironmentHints::default()
        });
        assert_eq!(assessed.hint, SessionTypeHint::Mixed);
        assert_eq!(parse_session_type("mir"), None);
    }

    #[test]
    fn remote_and_arbitrary_display_endpoints_are_rejected() {
        assert!(
            x11_endpoint(&SessionEnvironmentHints {
                x11_display: Some("remote.example:0".to_owned()),
                ..SessionEnvironmentHints::default()
            })
            .is_err()
        );
        assert!(parse_wayland_socket_name("../wayland-0").is_err());
        assert!(parse_wayland_socket_name("/tmp/wayland-0").is_err());
    }

    #[test]
    fn only_local_x11_display_syntax_is_accepted() {
        for value in [":0", ":12.1", "unix/:0.0"] {
            assert!(
                x11_endpoint(&SessionEnvironmentHints {
                    x11_display: Some(value.to_owned()),
                    ..SessionEnvironmentHints::default()
                })
                .is_ok(),
                "{value}"
            );
        }
        for value in ["tcp/host:0", "localhost:0", "host/unix:0", ":0.bad"] {
            assert!(
                x11_endpoint(&SessionEnvironmentHints {
                    x11_display: Some(value.to_owned()),
                    ..SessionEnvironmentHints::default()
                })
                .is_err(),
                "{value}"
            );
        }
    }

    #[test]
    fn desktop_hints_are_bounded_and_sanitized() {
        let hints = SessionEnvironmentHints {
            current_desktop: Some("KDE:GNOME;../secret;Hyprland".to_owned()),
            session_desktop: Some("niri".to_owned()),
            ..SessionEnvironmentHints::default()
        };
        assert_eq!(
            safe_desktop_names(&hints),
            ["KDE", "GNOME", "Hyprland", "niri"]
        );
    }
}
