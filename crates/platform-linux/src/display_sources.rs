//! Fixed-family read-only session-hint and DRM connector sources.

use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::errors::{DataSourceError, DataSourceErrorKind};
use crate::pci::PciBdf;
use crate::session::SessionEnvironmentHints;
use crate::sources::SystemLinuxDataSource;

pub const MAX_DRM_CONNECTOR_ATTRIBUTE_BYTES: usize = 4 * 1024;
const MAX_DRM_DIRECTORY_ENTRIES: usize = crate::display::MAX_DRM_CONNECTORS * 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawDrmConnector {
    pub card_name: String,
    pub connector_name: String,
    pub pci_bdf: Option<String>,
    pub status: Result<Vec<u8>, DataSourceError>,
    pub enabled: Result<Vec<u8>, DataSourceError>,
    pub modes: Result<Vec<u8>, DataSourceError>,
}

/// Input boundary for the fixed session variables and `/sys/class/drm`
/// connector family. Implementations must not accept arbitrary path input.
pub trait LinuxDisplayDataSource: Send + Sync {
    fn session_environment(&self) -> Result<SessionEnvironmentHints, DataSourceError>;
    fn enumerate_drm_connectors(&self) -> Result<Vec<RawDrmConnector>, DataSourceError>;
}

impl LinuxDisplayDataSource for SystemLinuxDataSource {
    fn session_environment(&self) -> Result<SessionEnvironmentHints, DataSourceError> {
        Ok(SessionEnvironmentHints {
            xdg_session_type: read_environment_hint("XDG_SESSION_TYPE", 32),
            wayland_display: read_environment_hint("WAYLAND_DISPLAY", 96),
            x11_display: read_environment_hint("DISPLAY", 128),
            current_desktop: read_environment_hint("XDG_CURRENT_DESKTOP", 128),
            session_desktop: read_environment_hint("XDG_SESSION_DESKTOP", 128),
        })
    }

    fn enumerate_drm_connectors(&self) -> Result<Vec<RawDrmConnector>, DataSourceError> {
        let root = self.rooted_path("/sys/class/drm");
        let root_metadata =
            fs::canonicalize(self.rooted_path("/sys/devices")).map_err(map_io_error)?;
        let entries = fs::read_dir(root).map_err(map_io_error)?;
        let mut connectors = Vec::new();
        for (index, entry) in entries.enumerate() {
            if index >= MAX_DRM_DIRECTORY_ENTRIES {
                return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
            }
            let entry = entry.map_err(map_io_error)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some((card_name, connector_name)) = split_connector_name(&name) else {
                continue;
            };
            if connectors.len() >= crate::display::MAX_DRM_CONNECTORS {
                return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
            }
            let link_path = entry.path();
            let canonical = fs::canonicalize(&link_path).map_err(map_io_error)?;
            if !canonical.starts_with(&root_metadata) {
                return Err(DataSourceError::new(DataSourceErrorKind::UnsafeSource));
            }
            let pci_bdf = resolve_pci_bdf(&canonical);
            connectors.push(RawDrmConnector {
                card_name: card_name.to_owned(),
                connector_name: connector_name.to_owned(),
                pci_bdf,
                status: read_connector_attribute(&canonical.join("status")),
                enabled: read_connector_attribute(&canonical.join("enabled")),
                modes: read_connector_attribute(&canonical.join("modes")),
            });
        }
        if connectors.len() > crate::display::MAX_DRM_CONNECTORS {
            return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
        }
        connectors.sort_by(|left, right| {
            (&left.card_name, &left.connector_name).cmp(&(&right.card_name, &right.connector_name))
        });
        Ok(connectors)
    }
}

fn read_environment_hint(name: &str, limit: usize) -> Option<String> {
    let value = std::env::var_os(name)?;
    let value = value.to_str()?;
    if value.is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return None;
    }
    Some(value.to_owned())
}

fn split_connector_name(value: &str) -> Option<(&str, &str)> {
    let rest = value.strip_prefix("card")?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 4 || rest.as_bytes().get(digits) != Some(&b'-') {
        return None;
    }
    let card_name = &value[..4 + digits];
    let connector_name = &rest[digits + 1..];
    if connector_name.is_empty()
        || connector_name.len() > 64
        || !connector_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return None;
    }
    Some((card_name, connector_name))
}

fn resolve_pci_bdf(canonical: &Path) -> Option<String> {
    canonical
        .components()
        .rev()
        .filter_map(|component| component.as_os_str().to_str())
        .find_map(|component| component.parse::<PciBdf>().ok().map(|bdf| bdf.to_string()))
}

fn read_connector_attribute(path: &Path) -> Result<Vec<u8>, DataSourceError> {
    let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType));
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(map_io_error)?;
    let mut bytes = Vec::new();
    file.take((MAX_DRM_CONNECTOR_ATTRIBUTE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(map_io_error)?;
    if bytes.len() > MAX_DRM_CONNECTOR_ATTRIBUTE_BYTES {
        return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
    }
    Ok(bytes)
}

fn map_io_error(error: io::Error) -> DataSourceError {
    match error.kind() {
        io::ErrorKind::NotFound => DataSourceError::new(DataSourceErrorKind::NotFound),
        io::ErrorKind::PermissionDenied => {
            DataSourceError::new(DataSourceErrorKind::PermissionDenied)
        }
        _ => DataSourceError::new(DataSourceErrorKind::Other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    #[test]
    fn connector_name_parser_accepts_only_card_connector_entries() {
        assert_eq!(
            split_connector_name("card0-eDP-1"),
            Some(("card0", "eDP-1"))
        );
        assert_eq!(
            split_connector_name("card12-HDMI-A-1"),
            Some(("card12", "HDMI-A-1"))
        );
        assert_eq!(split_connector_name("card0"), None);
        assert_eq!(split_connector_name("card0-../../secret"), None);
        assert_eq!(split_connector_name("cardx-DP-1"), None);
    }

    #[test]
    fn connector_enumeration_rejects_symlinks_outside_sys_devices() {
        let temp = tempdir().unwrap();
        let sys_devices = temp.path().join("sys/devices");
        let class_drm = temp.path().join("sys/class/drm");
        let outside = temp.path().join("outside/card0/card0-DP-1");
        std::fs::create_dir_all(&sys_devices).unwrap();
        std::fs::create_dir_all(&class_drm).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        for (name, value) in [
            ("status", "connected\n"),
            ("enabled", "disabled\n"),
            ("modes", "1920x1080\n"),
        ] {
            std::fs::write(outside.join(name), value).unwrap();
        }
        symlink(&outside, class_drm.join("card0-DP-1")).unwrap();

        let result = SystemLinuxDataSource::rooted_at(temp.path())
            .enumerate_drm_connectors()
            .unwrap_err();
        assert_eq!(result.kind, DataSourceErrorKind::UnsafeSource);
    }

    #[test]
    fn connector_enumeration_reads_only_fixed_sysfs_attributes() {
        let temp = tempdir().unwrap();
        let connector = temp
            .path()
            .join("sys/devices/pci0000:00/0000:01:00.0/drm/card0/card0-eDP-1");
        let class_drm = temp.path().join("sys/class/drm");
        std::fs::create_dir_all(&connector).unwrap();
        std::fs::create_dir_all(&class_drm).unwrap();
        for (name, value) in [
            ("status", "connected\n"),
            ("enabled", "disabled\n"),
            ("modes", "1920x1080\n"),
        ] {
            std::fs::write(connector.join(name), value).unwrap();
        }
        symlink(&connector, class_drm.join("card0-eDP-1")).unwrap();

        let connectors = SystemLinuxDataSource::rooted_at(temp.path())
            .enumerate_drm_connectors()
            .unwrap();
        assert_eq!(connectors.len(), 1);
        assert_eq!(connectors[0].card_name, "card0");
        assert_eq!(connectors[0].connector_name, "eDP-1");
        assert_eq!(connectors[0].pci_bdf.as_deref(), Some("0000:01:00.0"));
        assert_eq!(
            connectors[0].status.as_deref(),
            Ok(b"connected\n".as_slice())
        );
    }

    #[test]
    fn connector_attributes_are_bounded_before_parsing() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("modes");
        std::fs::write(&path, vec![b'1'; MAX_DRM_CONNECTOR_ATTRIBUTE_BYTES + 1]).unwrap();
        assert_eq!(
            read_connector_attribute(&path).unwrap_err().kind,
            DataSourceErrorKind::TooLarge
        );
    }
}
