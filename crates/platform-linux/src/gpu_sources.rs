//! Fixed-family read-only source boundary for Linux graphics inventory.

use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
use std::path::Path;

use crate::errors::{DataSourceError, DataSourceErrorKind};
use crate::pci::{MAX_PCI_DEVICES, PciBdf};
use crate::sources::SystemLinuxDataSource;

pub const MAX_DRM_NODES: usize = 128;
const MAX_PCI_ATTRIBUTE_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PciAttribute {
    Class,
    Vendor,
    Device,
    SubsystemVendor,
    SubsystemDevice,
    Driver,
}

impl PciAttribute {
    const fn name(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Vendor => "vendor",
            Self::Device => "device",
            Self::SubsystemVendor => "subsystem_vendor",
            Self::SubsystemDevice => "subsystem_device",
            Self::Driver => "driver",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrmNodeKind {
    Primary,
    Render,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceNodeAccess {
    /// `faccessat(AT_EACCESS)` accepted read permission; the node was not opened.
    Accessible,
    Denied,
    Missing,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrmNodeObservation {
    pub name: String,
    pub kind: DrmNodeKind,
    pub pci_bdf: Option<String>,
    pub access: DeviceNodeAccess,
}

/// A fixed Linux graphics source interface. Implementations must cap all
/// returned vectors and reads and must not interpret callers' values as paths.
pub trait LinuxGpuDataSource: Send + Sync {
    fn enumerate_pci_entries(&self) -> Result<Vec<String>, DataSourceError>;
    fn read_pci_attribute(
        &self,
        bdf: &str,
        attribute: PciAttribute,
    ) -> Result<Vec<u8>, DataSourceError>;
    fn enumerate_drm_nodes(&self) -> Result<Vec<DrmNodeObservation>, DataSourceError>;
}

impl LinuxGpuDataSource for SystemLinuxDataSource {
    fn enumerate_pci_entries(&self) -> Result<Vec<String>, DataSourceError> {
        let directory = self.rooted_path("/sys/bus/pci/devices");
        let entries = fs::read_dir(directory).map_err(map_io_error)?;
        let mut names = Vec::new();
        for (index, entry) in entries.enumerate() {
            if index >= MAX_PCI_DEVICES {
                return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
            }
            let entry = entry.map_err(map_io_error)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.parse::<PciBdf>().is_err() {
                continue;
            }
            validate_under_sys_devices(
                &self.rooted_path(&format!("/sys/bus/pci/devices/{name}")),
                &self.rooted_path("/sys/devices"),
            )?;
            names.push(name);
        }
        if names.len() > MAX_PCI_DEVICES {
            return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
        }
        names.sort_unstable();
        Ok(names)
    }

    fn read_pci_attribute(
        &self,
        bdf: &str,
        attribute: PciAttribute,
    ) -> Result<Vec<u8>, DataSourceError> {
        let _: PciBdf = bdf
            .parse()
            .map_err(|_| DataSourceError::new(DataSourceErrorKind::UnsafeSource))?;
        validate_under_sys_devices(
            &self.rooted_path(&format!("/sys/bus/pci/devices/{bdf}")),
            &self.rooted_path("/sys/devices"),
        )?;
        let path = self.rooted_path(&format!("/sys/bus/pci/devices/{bdf}/{}", attribute.name()));
        if attribute == PciAttribute::Driver {
            let target = fs::read_link(path).map_err(map_io_error)?;
            let driver_root = self.rooted_path("/sys/bus/pci/drivers");
            let resolved =
                fs::canonicalize(self.rooted_path(&format!("/sys/bus/pci/devices/{bdf}/driver")))
                    .map_err(map_io_error)?;
            let canonical_root = fs::canonicalize(driver_root).map_err(map_io_error)?;
            if !resolved.starts_with(canonical_root) {
                return Err(DataSourceError::new(DataSourceErrorKind::UnsafeSource));
            }
            let name = target
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| DataSourceError::new(DataSourceErrorKind::UnsafeSource))?;
            if name.is_empty()
                || name.len() > 128
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err(DataSourceError::new(DataSourceErrorKind::MalformedData));
            }
            return Ok(name.as_bytes().to_vec());
        }
        read_attribute(&path, MAX_PCI_ATTRIBUTE_BYTES)
    }

    fn enumerate_drm_nodes(&self) -> Result<Vec<DrmNodeObservation>, DataSourceError> {
        let directory = self.rooted_path("/sys/class/drm");
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(map_io_error(error));
            }
            Err(error) => return Err(map_io_error(error)),
        };
        let mut nodes = Vec::new();
        for (index, entry) in entries.enumerate() {
            if index >= MAX_DRM_NODES {
                return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
            }
            let entry = entry.map_err(map_io_error)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let kind = if is_primary_node_name(&name) {
                DrmNodeKind::Primary
            } else if is_render_node_name(&name) {
                DrmNodeKind::Render
            } else {
                continue;
            };
            let device = entry.path().join("device");
            let bdf = resolve_drm_bdf(&device, &self.rooted_path("/sys/devices"))
                .ok()
                .flatten();
            let node_path = self.rooted_path(&format!("/dev/dri/{name}"));
            let access = match fs::symlink_metadata(&node_path) {
                Ok(metadata) if metadata.file_type().is_char_device() => {
                    use nix::unistd::AccessFlags;
                    match nix::unistd::faccessat(
                        None,
                        &node_path,
                        AccessFlags::R_OK,
                        nix::fcntl::AtFlags::AT_EACCESS | nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
                    ) {
                        Ok(()) => DeviceNodeAccess::Accessible,
                        Err(nix::errno::Errno::EACCES | nix::errno::Errno::EPERM) => {
                            DeviceNodeAccess::Denied
                        }
                        Err(nix::errno::Errno::ENOENT) => DeviceNodeAccess::Missing,
                        Err(_) => DeviceNodeAccess::Unknown,
                    }
                }
                Ok(_) => DeviceNodeAccess::Unknown,
                Err(error) if error.kind() == io::ErrorKind::NotFound => DeviceNodeAccess::Missing,
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                    DeviceNodeAccess::Denied
                }
                Err(_) => DeviceNodeAccess::Unknown,
            };
            nodes.push(DrmNodeObservation {
                name,
                kind,
                pci_bdf: bdf,
                access,
            });
        }
        if nodes.len() > MAX_DRM_NODES {
            return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
        }
        nodes.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(nodes)
    }
}

pub(crate) fn is_primary_node_name(name: &str) -> bool {
    name.len() <= 16
        && name.is_ascii()
        && name
            .strip_prefix("card")
            .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
}
pub(crate) fn is_render_node_name(name: &str) -> bool {
    name.len() <= 16
        && name.is_ascii()
        && name
            .strip_prefix("renderD")
            .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
}

fn resolve_drm_bdf(device: &Path, sys_devices: &Path) -> Result<Option<String>, DataSourceError> {
    let canonical = fs::canonicalize(device).map_err(map_io_error)?;
    let sys_devices = fs::canonicalize(sys_devices).map_err(map_io_error)?;
    if !canonical.starts_with(sys_devices) {
        return Err(DataSourceError::new(DataSourceErrorKind::UnsafeSource));
    }
    for component in canonical.components().rev() {
        if let Some(value) = component.as_os_str().to_str() {
            if value.parse::<PciBdf>().is_ok() {
                return Ok(Some(value.to_owned()));
            }
        }
    }
    Ok(None)
}

fn validate_under_sys_devices(path: &Path, sys_devices: &Path) -> Result<(), DataSourceError> {
    let canonical = fs::canonicalize(path).map_err(map_io_error)?;
    let root = fs::canonicalize(sys_devices).map_err(map_io_error)?;
    if canonical.starts_with(root) {
        Ok(())
    } else {
        Err(DataSourceError::new(DataSourceErrorKind::UnsafeSource))
    }
}

pub(crate) fn read_attribute(path: &Path, limit: usize) -> Result<Vec<u8>, DataSourceError> {
    let metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(DataSourceError::new(DataSourceErrorKind::UnexpectedType));
    }
    // Sysfs attributes commonly report PAGE_SIZE as st_size regardless of
    // their contents; enforce the limit on bytes actually read instead.
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(map_io_error)?;
    let mut bytes = Vec::with_capacity(limit.min(128));
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(map_io_error)?;
    if bytes.len() > limit {
        return Err(DataSourceError::new(DataSourceErrorKind::TooLarge));
    }
    Ok(bytes)
}

pub(crate) fn map_io_error(error: io::Error) -> DataSourceError {
    DataSourceError::new(match error.kind() {
        io::ErrorKind::NotFound => DataSourceErrorKind::NotFound,
        io::ErrorKind::PermissionDenied => DataSourceErrorKind::PermissionDenied,
        _ => DataSourceErrorKind::Other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    #[test]
    fn accepts_only_bounded_primary_and_render_node_names() {
        assert!(is_primary_node_name("card0"));
        assert!(is_primary_node_name("card128"));
        assert!(is_render_node_name("renderD128"));
        assert!(!is_primary_node_name("card0-DP-1"));
        assert!(!is_primary_node_name("controlD64"));
        assert!(!is_render_node_name("renderD"));
        assert!(!is_render_node_name(&format!("renderD{}", "1".repeat(32))));
    }

    #[test]
    fn pci_enumeration_rejects_unsafe_links_and_enforces_the_entry_limit() {
        let root = tempdir().unwrap();
        let sys_devices = root.path().join("sys/devices");
        let pci_bus = root.path().join("sys/bus/pci/devices");
        fs::create_dir_all(&sys_devices).unwrap();
        fs::create_dir_all(&pci_bus).unwrap();
        let outside = root.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, pci_bus.join("0000:00:00.0")).unwrap();
        let source = SystemLinuxDataSource::rooted_at(root.path());
        assert_eq!(
            source.enumerate_pci_entries().unwrap_err().kind,
            DataSourceErrorKind::UnsafeSource
        );

        fs::remove_file(pci_bus.join("0000:00:00.0")).unwrap();
        for index in 0..=MAX_PCI_DEVICES {
            let address = format!(
                "{:04x}:{:02x}:{:02x}.0",
                index / 8192,
                (index / 32) % 256,
                index % 32
            );
            let target = sys_devices.join(format!("pci-fixture-{index}"));
            fs::create_dir(&target).unwrap();
            symlink(
                Path::new("../../../devices").join(format!("pci-fixture-{index}")),
                pci_bus.join(address),
            )
            .unwrap();
        }
        assert_eq!(
            source.enumerate_pci_entries().unwrap_err().kind,
            DataSourceErrorKind::TooLarge
        );
    }

    #[test]
    fn drm_nodes_map_by_sysfs_topology_and_missing_device_files_stay_missing() {
        let root = tempdir().unwrap();
        let gpu = root.path().join("sys/devices/pci0000:00/0000:00:02.0");
        fs::create_dir_all(gpu.join("drm/card0")).unwrap();
        fs::create_dir_all(root.path().join("sys/class/drm/card0")).unwrap();
        symlink(&gpu, root.path().join("sys/class/drm/card0/device")).unwrap();
        let source = SystemLinuxDataSource::rooted_at(root.path());
        let nodes = source.enumerate_drm_nodes().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].pci_bdf.as_deref(), Some("0000:00:02.0"));
        assert_eq!(nodes[0].kind, DrmNodeKind::Primary);
        assert_eq!(nodes[0].access, DeviceNodeAccess::Missing);
    }
}
