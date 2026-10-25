//! P013 Linux GPU discovery and P007 snapshot composition.

use endfield_unix_domain::{
    Availability, Bytes, CapabilityId, CapabilityRecord, CapabilityValue, Diagnostic,
    DiagnosticCategory, GpuClass, GpuInfo, GpuVendor, GraphicsDriver, InstanceId, Measurement,
    MeasurementOrigin, MeasurementUnit, ProbeError, ProbeErrorKind, Provenance, ProvenanceId,
    SystemSnapshot,
};

use crate::errors::DataSourceErrorKind;
use crate::gpu_sources::{DrmNodeObservation, LinuxGpuDataSource, PciAttribute};
use crate::hardware_probe::{LinuxHardwareProbe, LinuxHardwareReport};
use crate::pci::{MAX_PCI_DEVICES, PciBdf};
use crate::sources::{LinuxDataSource, SystemLinuxDataSource};
use crate::vulkan::{
    SystemVulkanBackend, VulkanBackend, VulkanDeviceType, VulkanLoaderStatus, VulkanPhysicalDevice,
};

pub const LINUX_GPU_REPORT_SCHEMA_VERSION: u32 = 1;
pub const GPU_INSTANCE_ID_BASE: u32 = 2;
pub const DISPLAY_INSTANCE_ID_BASE: u32 = 1_000_000;
const PCI_PROVENANCE_ID: u32 = 10;
const DRM_PROVENANCE_ID: u32 = 11;
const VULKAN_PROVENANCE_ID: u32 = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxGpuCompleteness {
    Complete,
    Partial,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuSourceStatus {
    Available,
    Missing,
    Degraded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PciGraphicsDevice {
    pub bdf: String,
    pub class_code: u32,
    pub vendor_id: u16,
    pub device_id: u16,
    pub subsystem_vendor_id: Option<u16>,
    pub subsystem_device_id: Option<u16>,
    pub driver_name: Option<String>,
    pub driver_binding: Availability,
    pub driver_diagnostic: Option<(DiagnosticCategory, &'static str)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VulkanMapping {
    PciIdentity,
    AmbiguousPciIdentity,
    ContradictoryPciIdentity,
    Unmatched,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxVulkanDevice {
    pub physical: VulkanPhysicalDevice,
    pub mapping: VulkanMapping,
    pub gpu_instance: Option<InstanceId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxGpuReport {
    pub schema_version: u32,
    /// P011/P012 collection is preserved, with P013 GPU facts added to this
    /// independently validated normalized snapshot.
    pub snapshot: SystemSnapshot,
    pub hardware: LinuxHardwareReport,
    pub pci_devices: Vec<PciGraphicsDevice>,
    pub pci_status: GpuSourceStatus,
    pub drm_nodes: Vec<DrmNodeObservation>,
    pub drm_status: GpuSourceStatus,
    pub vulkan_status: VulkanLoaderStatus,
    pub vulkan_devices: Vec<LinuxVulkanDevice>,
    pub vulkan_diagnostic: Option<&'static str>,
    pub completeness: LinuxGpuCompleteness,
}

pub struct LinuxGpuProbe<S = SystemLinuxDataSource, V = SystemVulkanBackend> {
    source: S,
    vulkan: V,
}

impl LinuxGpuProbe<SystemLinuxDataSource, SystemVulkanBackend> {
    pub fn system() -> Self {
        Self {
            source: SystemLinuxDataSource::new(),
            vulkan: SystemVulkanBackend,
        }
    }
}

impl<S> LinuxGpuProbe<S, SystemVulkanBackend> {
    pub fn with_source(source: S) -> Self {
        Self {
            source,
            vulkan: SystemVulkanBackend,
        }
    }
}

impl<S, V> LinuxGpuProbe<S, V> {
    pub fn with_source_and_vulkan(source: S, vulkan: V) -> Self {
        Self { source, vulkan }
    }
}

impl<S, V> LinuxGpuProbe<S, V>
where
    S: LinuxDataSource
        + crate::hardware_sources::LinuxHardwareDataSource
        + LinuxGpuDataSource
        + Clone,
    V: VulkanBackend,
{
    pub fn inspect(&self) -> Result<LinuxGpuReport, ProbeError> {
        let hardware = LinuxHardwareProbe::with_source(self.source.clone()).inspect()?;
        let mut snapshot = hardware.snapshot.clone();
        let mut diagnostics = Vec::<(&'static str, DiagnosticCategory, &'static str)>::new();
        let mut pci_status = GpuSourceStatus::Available;
        let mut candidates = match self.source.enumerate_pci_entries() {
            Ok(entries) => entries,
            Err(error) => {
                pci_status = if error.kind == DataSourceErrorKind::NotFound {
                    GpuSourceStatus::Missing
                } else {
                    GpuSourceStatus::Degraded
                };
                diagnostics.push((
                    "linux-pci",
                    category(error.kind),
                    "PCI graphics devices could not be enumerated",
                ));
                Vec::new()
            }
        };
        candidates.sort_unstable();
        if candidates.len() > MAX_PCI_DEVICES {
            pci_status = GpuSourceStatus::Degraded;
            candidates.truncate(MAX_PCI_DEVICES);
            diagnostics.push((
                "linux-pci",
                DiagnosticCategory::MalformedSourceData,
                "PCI graphics enumeration exceeded its limit",
            ));
        }
        let mut pci_devices = Vec::new();
        for raw_bdf in candidates {
            if raw_bdf.len() != 12 || !raw_bdf.is_ascii() {
                pci_status = GpuSourceStatus::Degraded;
                diagnostics.push((
                    "linux-pci",
                    DiagnosticCategory::MalformedSourceData,
                    "PCI enumeration returned an oversized or invalid address",
                ));
                continue;
            }
            let Ok(bdf) = raw_bdf.parse::<PciBdf>() else {
                pci_status = GpuSourceStatus::Degraded;
                diagnostics.push((
                    "linux-pci",
                    DiagnosticCategory::MalformedSourceData,
                    "PCI enumeration returned an invalid address",
                ));
                continue;
            };
            match read_pci_device(&self.source, bdf.to_string()) {
                Ok(Some(device)) => pci_devices.push(device),
                Ok(None) => (),
                Err((kind, message)) => {
                    pci_status = GpuSourceStatus::Degraded;
                    diagnostics.push(("linux-pci", category(kind), message));
                }
            }
        }
        pci_devices.sort_by(|left, right| left.bdf.cmp(&right.bdf));
        pci_devices.dedup_by(|left, right| {
            if left.bdf == right.bdf {
                pci_status = GpuSourceStatus::Degraded;
                diagnostics.push((
                    "linux-pci",
                    DiagnosticCategory::MalformedSourceData,
                    "duplicate PCI graphics identity was ignored",
                ));
                true
            } else {
                false
            }
        });

        let mut drm_status = GpuSourceStatus::Available;
        let mut raw_drm_nodes = match self.source.enumerate_drm_nodes() {
            Ok(nodes) => nodes,
            Err(error) => {
                drm_status = if error.kind == DataSourceErrorKind::NotFound {
                    GpuSourceStatus::Missing
                } else {
                    GpuSourceStatus::Degraded
                };
                diagnostics.push((
                    "linux-drm",
                    category(error.kind),
                    "DRM nodes could not be enumerated",
                ));
                Vec::new()
            }
        };
        raw_drm_nodes.sort_by(|left, right| left.name.cmp(&right.name));
        if raw_drm_nodes.len() > crate::gpu_sources::MAX_DRM_NODES {
            drm_status = GpuSourceStatus::Degraded;
            raw_drm_nodes.truncate(crate::gpu_sources::MAX_DRM_NODES);
            diagnostics.push((
                "linux-drm",
                DiagnosticCategory::MalformedSourceData,
                "DRM node enumeration exceeded its limit",
            ));
        }
        let mut drm_nodes = Vec::new();
        for mut node in raw_drm_nodes {
            let valid_name = match node.kind {
                crate::gpu_sources::DrmNodeKind::Primary => {
                    crate::gpu_sources::is_primary_node_name(&node.name)
                }
                crate::gpu_sources::DrmNodeKind::Render => {
                    crate::gpu_sources::is_render_node_name(&node.name)
                }
            };
            if !valid_name {
                drm_status = GpuSourceStatus::Degraded;
                diagnostics.push((
                    "linux-drm",
                    DiagnosticCategory::MalformedSourceData,
                    "DRM source returned an invalid node identifier",
                ));
                continue;
            }
            if let Some(bdf) = node.pci_bdf.as_deref() {
                match bdf.parse::<PciBdf>() {
                    Ok(parsed) => node.pci_bdf = Some(parsed.to_string()),
                    Err(_) => {
                        drm_status = GpuSourceStatus::Degraded;
                        diagnostics.push((
                            "linux-drm",
                            DiagnosticCategory::MalformedSourceData,
                            "DRM source returned an invalid PCI relationship",
                        ));
                        node.pci_bdf = None;
                    }
                }
            }
            drm_nodes.push(node);
        }
        drm_nodes.sort_by(|left, right| left.name.cmp(&right.name));
        drm_nodes.dedup_by(|left, right| {
            if left.name == right.name {
                drm_status = GpuSourceStatus::Degraded;
                diagnostics.push((
                    "linux-drm",
                    DiagnosticCategory::MalformedSourceData,
                    "duplicate DRM node was ignored",
                ));
                true
            } else {
                false
            }
        });
        let pci_bdfs = pci_devices
            .iter()
            .map(|device| device.bdf.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for node in &drm_nodes {
            if node
                .pci_bdf
                .as_deref()
                .is_none_or(|bdf| !pci_bdfs.contains(bdf))
            {
                drm_status = GpuSourceStatus::Degraded;
                diagnostics.push((
                    "linux-drm",
                    DiagnosticCategory::NotYetProbed,
                    "DRM node has no validated PCI graphics-device relationship",
                ));
            }
            if node.access == crate::gpu_sources::DeviceNodeAccess::Unknown {
                drm_status = GpuSourceStatus::Degraded;
                diagnostics.push((
                    "linux-drm",
                    DiagnosticCategory::NotYetProbed,
                    "DRM device-node access could not be assessed",
                ));
            }
        }
        let mut vulkan = self.vulkan.enumerate();
        let status_forbids_devices = matches!(
            vulkan.status,
            VulkanLoaderStatus::Missing
                | VulkanLoaderStatus::InitializationFailed
                | VulkanLoaderStatus::AvailableNoDevices
        );
        if (status_forbids_devices && !vulkan.devices.is_empty())
            || (vulkan.status == VulkanLoaderStatus::Enumerated && vulkan.devices.is_empty())
        {
            vulkan.devices.clear();
            vulkan.status = VulkanLoaderStatus::Degraded;
            vulkan.diagnostic = Some("Vulkan loader status conflicted with its device results");
        }
        if let Some(message) = vulkan.diagnostic {
            let diag_category = match vulkan.status {
                VulkanLoaderStatus::Missing => DiagnosticCategory::MissingProvider,
                VulkanLoaderStatus::InitializationFailed | VulkanLoaderStatus::Degraded => {
                    DiagnosticCategory::TemporaryFailure
                }
                VulkanLoaderStatus::AvailableNoDevices | VulkanLoaderStatus::Enumerated => {
                    DiagnosticCategory::NotYetProbed
                }
            };
            diagnostics.push(("linux-vulkan", diag_category, message));
        }
        let devices = validate_vulkan_devices(vulkan.devices, &mut vulkan.status, &mut diagnostics);
        let mappings = map_vulkan_devices(&pci_devices, &devices);
        if mappings.contains(&VulkanMapping::AmbiguousPciIdentity) {
            diagnostics.push((
                "linux-vulkan",
                DiagnosticCategory::MalformedSourceData,
                "Vulkan PCI bus identity was reported by multiple devices",
            ));
        }
        if mappings.contains(&VulkanMapping::ContradictoryPciIdentity) {
            diagnostics.push((
                "linux-vulkan",
                DiagnosticCategory::MalformedSourceData,
                "Vulkan PCI identity conflicted with the PCI device identifiers",
            ));
        }
        let gpu_ids = allocate_gpu_ids(pci_devices.len())?;
        let mut mapped_class = vec![GpuClass::Unknown; pci_devices.len()];
        let mut mapped_name = vec![None; pci_devices.len()];
        let mut mapped_vulkan = vec![false; pci_devices.len()];
        for (vulkan_index, mapping) in mappings.iter().enumerate() {
            if let VulkanMapping::PciIdentity = mapping {
                if let Some(gpu_index) = unique_pci_match(&pci_devices, &devices[vulkan_index]) {
                    mapped_vulkan[gpu_index] = true;
                    mapped_name[gpu_index] = Some(devices[vulkan_index].name.clone());
                    mapped_class[gpu_index] = match devices[vulkan_index].device_type {
                        VulkanDeviceType::Integrated => GpuClass::Integrated,
                        VulkanDeviceType::Discrete => GpuClass::Discrete,
                        _ => GpuClass::Unknown,
                    };
                }
            }
        }

        let pci_provenance = if pci_status != GpuSourceStatus::Missing {
            Some(push_provenance(
                &mut snapshot,
                PCI_PROVENANCE_ID,
                "linux-pci",
                self.source.observation_time_unix_nanos(),
            )?)
        } else {
            None
        };
        let _drm_provenance = if drm_status != GpuSourceStatus::Missing {
            Some(push_provenance(
                &mut snapshot,
                DRM_PROVENANCE_ID,
                "linux-drm",
                self.source.observation_time_unix_nanos(),
            )?)
        } else {
            None
        };
        let vulkan_provenance = Some(push_provenance(
            &mut snapshot,
            VULKAN_PROVENANCE_ID,
            "linux-vulkan",
            self.source.observation_time_unix_nanos(),
        )?);

        for (index, pci) in pci_devices.iter().enumerate() {
            if let Some((diag_category, message)) = pci.driver_diagnostic {
                diagnostics.push(("linux-driver", diag_category, message));
            }
            let instance = gpu_ids[index];
            let driver = pci.driver_name.as_ref().map(|name| GraphicsDriver {
                name: name.clone(),
                version: None,
            });
            let vulkan_device = if mapped_vulkan[index] {
                Availability::Available
            } else if vulkan.status == VulkanLoaderStatus::AvailableNoDevices {
                Availability::Unavailable
            } else {
                Availability::Unknown
            };
            let acceleration = if mapped_vulkan[index]
                && matches!(
                    mapped_class[index],
                    GpuClass::Integrated | GpuClass::Discrete
                ) {
                Availability::Available
            } else {
                Availability::Unknown
            };
            let vram = CapabilityValue::new(Availability::Unknown, None);
            snapshot.gpus.push(GpuInfo {
                instance,
                availability: Availability::Available,
                vendor: Some(gpu_vendor(pci.vendor_id)),
                model: mapped_name[index].clone(),
                class: mapped_class[index],
                graphics_driver: driver,
                driver_availability: pci.driver_binding,
                vram: vram.clone(),
                hardware_acceleration: acceleration,
                vulkan_device,
            });
            push_capability(
                &mut snapshot,
                CapabilityId::GpuDevice,
                instance,
                Availability::Available,
                None,
                pci_provenance,
            );
            push_capability(
                &mut snapshot,
                CapabilityId::Vram,
                instance,
                Availability::Unknown,
                None,
                pci_provenance,
            );
            push_capability(
                &mut snapshot,
                CapabilityId::HardwareAcceleration,
                instance,
                acceleration,
                None,
                if acceleration == Availability::Available {
                    vulkan_provenance
                } else {
                    pci_provenance
                },
            );
            push_capability(
                &mut snapshot,
                CapabilityId::GraphicsDriver,
                instance,
                pci.driver_binding,
                None,
                pci_provenance,
            );
            push_capability(
                &mut snapshot,
                CapabilityId::VulkanDevice,
                instance,
                vulkan_device,
                None,
                vulkan_provenance,
            );
        }
        for (provider, category, message) in diagnostics {
            snapshot.diagnostics.push(
                Diagnostic::new(
                    category,
                    Some(provider.to_owned()),
                    Some(message.to_owned()),
                )
                .map_err(|_| {
                    probe_error(
                        "linux-gpu",
                        ProbeErrorKind::MalformedData,
                        "GPU diagnostic failed validation",
                    )
                })?,
            );
        }
        let snapshot = snapshot.normalize().map_err(|_| {
            probe_error(
                "linux-gpu",
                ProbeErrorKind::MalformedData,
                "combined P011/P012/P013 snapshot failed validation",
            )
        })?;
        let vulkan_devices = devices
            .into_iter()
            .enumerate()
            .map(|(index, physical)| {
                let mapping = mappings[index];
                let gpu_instance = if mapping == VulkanMapping::PciIdentity {
                    unique_pci_match(&pci_devices, &physical).map(|gpu| gpu_ids[gpu])
                } else {
                    None
                };
                LinuxVulkanDevice {
                    physical,
                    mapping,
                    gpu_instance,
                }
            })
            .collect::<Vec<_>>();
        let incomplete = !snapshot.diagnostics.is_empty()
            || matches!(
                vulkan.status,
                VulkanLoaderStatus::Missing
                    | VulkanLoaderStatus::InitializationFailed
                    | VulkanLoaderStatus::Degraded
            );
        Ok(LinuxGpuReport {
            schema_version: LINUX_GPU_REPORT_SCHEMA_VERSION,
            snapshot,
            hardware,
            pci_devices,
            pci_status,
            drm_nodes,
            drm_status,
            vulkan_status: vulkan.status,
            vulkan_devices,
            vulkan_diagnostic: vulkan.diagnostic,
            completeness: if incomplete {
                LinuxGpuCompleteness::Partial
            } else {
                LinuxGpuCompleteness::Complete
            },
        })
    }
}

fn read_pci_device<S: LinuxGpuDataSource>(
    source: &S,
    bdf: String,
) -> Result<Option<PciGraphicsDevice>, (DataSourceErrorKind, &'static str)> {
    let read = |field| {
        source
            .read_pci_attribute(&bdf, field)
            .map_err(|error| (error.kind, "PCI graphics attributes could not be read"))
    };
    let class = match parse_hex(&read(PciAttribute::Class)?, 6) {
        Ok(value) => value as u32,
        Err(_) => {
            return Err((
                DataSourceErrorKind::MalformedData,
                "PCI class code was malformed",
            ));
        }
    };
    if !PciBdf::is_display_controller_class(class) {
        return Ok(None);
    }
    let vendor_id = parse_hex(&read(PciAttribute::Vendor)?, 4).map_err(|_| {
        (
            DataSourceErrorKind::MalformedData,
            "PCI vendor identifier was malformed",
        )
    })? as u16;
    let device_id = parse_hex(&read(PciAttribute::Device)?, 4).map_err(|_| {
        (
            DataSourceErrorKind::MalformedData,
            "PCI device identifier was malformed",
        )
    })? as u16;
    let optional_id = |field| {
        read(field)
            .ok()
            .and_then(|bytes| parse_hex(&bytes, 4).ok())
            .map(|value| value as u16)
    };
    let (driver, driver_binding, driver_diagnostic) = match source
        .read_pci_attribute(&bdf, PciAttribute::Driver)
    {
        Ok(value) if value.len() <= 128 => match String::from_utf8(value) {
            Ok(name)
                if !name.is_empty()
                    && name.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
                    }) =>
            {
                (Some(name), Availability::Available, None)
            }
            _ => (
                None,
                Availability::Unknown,
                Some((
                    DiagnosticCategory::MalformedSourceData,
                    "kernel driver name was malformed",
                )),
            ),
        },
        Ok(_) => (
            None,
            Availability::Unknown,
            Some((
                DiagnosticCategory::MalformedSourceData,
                "kernel driver name exceeded its size limit",
            )),
        ),
        Err(error) if error.kind == DataSourceErrorKind::NotFound => {
            (None, Availability::Unavailable, None)
        }
        Err(error) => (
            None,
            Availability::Unknown,
            Some((
                category(error.kind),
                match error.kind {
                    DataSourceErrorKind::PermissionDenied => {
                        "kernel driver binding access was denied"
                    }
                    DataSourceErrorKind::UnsafeSource => "kernel driver binding link was unsafe",
                    _ => "kernel driver binding could not be read",
                },
            )),
        ),
    };
    let subsystem_vendor_id = optional_id(PciAttribute::SubsystemVendor);
    let subsystem_device_id = optional_id(PciAttribute::SubsystemDevice);
    Ok(Some(PciGraphicsDevice {
        bdf: bdf.clone(),
        class_code: class,
        vendor_id,
        device_id,
        subsystem_vendor_id,
        subsystem_device_id,
        driver_name: driver,
        driver_binding,
        driver_diagnostic,
    }))
}

fn parse_hex(bytes: &[u8], digits: usize) -> Result<u64, ()> {
    if bytes.len() > digits + 4 {
        return Err(());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ())?.trim();
    let text = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    if text.len() != digits || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(());
    }
    u64::from_str_radix(text, 16).map_err(|_| ())
}

fn validate_vulkan_devices(
    mut devices: Vec<VulkanPhysicalDevice>,
    status: &mut VulkanLoaderStatus,
    diagnostics: &mut Vec<(&'static str, DiagnosticCategory, &'static str)>,
) -> Vec<VulkanPhysicalDevice> {
    if devices.len() > 64 {
        devices.truncate(64);
        *status = VulkanLoaderStatus::Degraded;
        diagnostics.push((
            "linux-vulkan",
            DiagnosticCategory::MalformedSourceData,
            "Vulkan physical-device count exceeded its limit",
        ));
    }
    devices.retain_mut(|device| {
        let valid = !device.name.trim().is_empty()
            && device.name.len() <= 256
            && !device.name.chars().any(char::is_control)
            && !device
                .name
                .chars()
                .any(|character| matches!(character, '/' | '\\'))
            && device.device_local_heap_bytes.len() <= 16;
        if !valid {
            *status = VulkanLoaderStatus::Degraded;
            diagnostics.push((
                "linux-vulkan",
                DiagnosticCategory::MalformedSourceData,
                "Vulkan returned invalid physical-device metadata",
            ));
        }
        if device
            .pci_identity
            .is_some_and(|identity| !identity.is_valid())
        {
            device.pci_identity = None;
            *status = VulkanLoaderStatus::Degraded;
            diagnostics.push((
                "linux-vulkan",
                DiagnosticCategory::MalformedSourceData,
                "Vulkan returned an invalid PCI bus identity",
            ));
        }
        if device.driver_identity.as_ref().is_some_and(|identity| {
            identity.driver_id < 0
                || identity.name.trim().is_empty()
                || identity.name.len() > 256
                || identity.name.chars().any(char::is_control)
                || identity
                    .name
                    .chars()
                    .any(|character| matches!(character, '/' | '\\'))
        }) {
            device.driver_identity = None;
            *status = VulkanLoaderStatus::Degraded;
            diagnostics.push((
                "linux-vulkan",
                DiagnosticCategory::MalformedSourceData,
                "Vulkan returned invalid driver identity metadata",
            ));
        }
        valid
    });
    devices
}

fn map_vulkan_devices(
    pci: &[PciGraphicsDevice],
    vulkan: &[VulkanPhysicalDevice],
) -> Vec<VulkanMapping> {
    vulkan
        .iter()
        .map(|device| {
            if device.device_type == VulkanDeviceType::Cpu {
                return VulkanMapping::Unmatched;
            }
            let Some(identity) = device.pci_identity else {
                return VulkanMapping::Unmatched;
            };
            let matches = pci
                .iter()
                .filter(|gpu| gpu.bdf.parse::<PciBdf>().ok() == Some(identity))
                .count();
            let reverse = vulkan
                .iter()
                .filter(|candidate| candidate.pci_identity == Some(identity))
                .count();
            if matches > 1 || reverse > 1 {
                VulkanMapping::AmbiguousPciIdentity
            } else if matches == 1 && reverse == 1 {
                let pci_device = pci
                    .iter()
                    .find(|gpu| gpu.bdf.parse::<PciBdf>().ok() == Some(identity));
                if pci_device.is_some_and(|gpu| {
                    u32::from(gpu.vendor_id) == device.vendor_id
                        && u32::from(gpu.device_id) == device.device_id
                }) {
                    VulkanMapping::PciIdentity
                } else {
                    VulkanMapping::ContradictoryPciIdentity
                }
            } else {
                VulkanMapping::Unmatched
            }
        })
        .collect()
}

fn unique_pci_match(pci: &[PciGraphicsDevice], device: &VulkanPhysicalDevice) -> Option<usize> {
    let matches = pci
        .iter()
        .enumerate()
        .filter(|(_, gpu)| {
            device
                .pci_identity
                .is_some_and(|identity| gpu.bdf.parse::<PciBdf>().ok() == Some(identity))
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches[0])
}

fn allocate_gpu_ids(count: usize) -> Result<Vec<InstanceId>, ProbeError> {
    (0..count)
        .map(|index| {
            let index = u32::try_from(index).map_err(|_| {
                probe_error(
                    "linux-gpu",
                    ProbeErrorKind::MalformedData,
                    "GPU count exceeded the instance identifier range",
                )
            })?;
            InstanceId::new(GPU_INSTANCE_ID_BASE.checked_add(index).ok_or_else(|| {
                probe_error(
                    "linux-gpu",
                    ProbeErrorKind::MalformedData,
                    "GPU instance identifier overflowed",
                )
            })?)
            .map_err(|_| {
                probe_error(
                    "linux-gpu",
                    ProbeErrorKind::MalformedData,
                    "GPU instance identifier was invalid",
                )
            })
        })
        .collect()
}

fn push_provenance(
    snapshot: &mut SystemSnapshot,
    id: u32,
    provider: &str,
    observed: Option<u64>,
) -> Result<ProvenanceId, ProbeError> {
    let id = ProvenanceId::new(id).map_err(|_| {
        probe_error(
            provider,
            ProbeErrorKind::MalformedData,
            "GPU provenance identifier was invalid",
        )
    })?;
    snapshot
        .provenance
        .push(Provenance::new(id, provider, observed).map_err(|_| {
            probe_error(
                provider,
                ProbeErrorKind::MalformedData,
                "GPU provenance was invalid",
            )
        })?);
    Ok(id)
}

fn push_capability(
    snapshot: &mut SystemSnapshot,
    id: CapabilityId,
    instance: InstanceId,
    availability: Availability,
    bytes: Option<Bytes>,
    provenance: Option<ProvenanceId>,
) {
    let mut record = CapabilityRecord::new(id, availability);
    record.instance = Some(instance);
    record.provenance = provenance;
    if let Some(Bytes(value)) = bytes {
        record.measurement = Some(Measurement::unsigned(
            value,
            MeasurementUnit::Bytes,
            MeasurementOrigin::Normalized,
        ));
    }
    snapshot.capabilities.push(record);
}

fn gpu_vendor(id: u16) -> GpuVendor {
    match id {
        0x8086 => GpuVendor::Intel,
        0x10de => GpuVendor::Nvidia,
        0x1002 => GpuVendor::Amd,
        other => GpuVendor::Other(format!("PCI vendor 0x{other:04x}")),
    }
}

fn category(kind: DataSourceErrorKind) -> DiagnosticCategory {
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

fn probe_error(provider: &str, kind: ProbeErrorKind, message: &str) -> ProbeError {
    ProbeError::new(provider, kind, Some(message.to_owned()))
        .unwrap_or_else(|_| ProbeError::new("linux-gpu", kind, None).expect("static provider"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::DataSourceError;
    use crate::vulkan::VulkanDriverIdentity;
    use std::collections::BTreeMap;

    #[derive(Clone)]
    struct PciAttributeFixture {
        values: BTreeMap<PciAttribute, Result<Vec<u8>, DataSourceErrorKind>>,
    }

    impl LinuxGpuDataSource for PciAttributeFixture {
        fn enumerate_pci_entries(&self) -> Result<Vec<String>, DataSourceError> {
            Ok(vec!["0000:00:02.0".to_owned()])
        }

        fn read_pci_attribute(
            &self,
            _: &str,
            attribute: PciAttribute,
        ) -> Result<Vec<u8>, DataSourceError> {
            match self.values.get(&attribute) {
                Some(Ok(value)) => Ok(value.clone()),
                Some(Err(kind)) => Err(DataSourceError::new(*kind)),
                None => Err(DataSourceError::new(DataSourceErrorKind::NotFound)),
            }
        }

        fn enumerate_drm_nodes(&self) -> Result<Vec<DrmNodeObservation>, DataSourceError> {
            Ok(Vec::new())
        }
    }

    fn attributes(driver: Result<Vec<u8>, DataSourceErrorKind>) -> PciAttributeFixture {
        PciAttributeFixture {
            values: BTreeMap::from([
                (PciAttribute::Class, Ok(b"0x030000\n".to_vec())),
                (PciAttribute::Vendor, Ok(b"0x8086\n".to_vec())),
                (PciAttribute::Device, Ok(b"0x9a49\n".to_vec())),
                (PciAttribute::Driver, driver),
            ]),
        }
    }

    fn pci(bdf: &str, vendor_id: u16, device_id: u16) -> PciGraphicsDevice {
        PciGraphicsDevice {
            bdf: bdf.to_owned(),
            class_code: 0x030000,
            vendor_id,
            device_id,
            subsystem_vendor_id: None,
            subsystem_device_id: None,
            driver_name: None,
            driver_binding: Availability::Unknown,
            driver_diagnostic: None,
        }
    }

    fn vk(
        name: &str,
        vendor_id: u32,
        device_id: u32,
        device_type: VulkanDeviceType,
    ) -> VulkanPhysicalDevice {
        VulkanPhysicalDevice {
            name: name.to_owned(),
            vendor_id,
            device_id,
            pci_identity: None,
            driver_identity: None,
            device_type,
            api_version: Some((1, 3, 0)),
            device_local_heap_bytes: vec![8 * 1024 * 1024 * 1024],
        }
    }

    #[test]
    fn multi_gpu_correlation_uses_verified_pci_identity_not_device_order() {
        let pci_devices = vec![
            pci("0000:00:02.0", 0x8086, 0x9a49),
            pci("0001:01:00.0", 0x10de, 0x1fef),
        ];
        let mut vulkan_devices = vec![
            vk(
                "Synthetic NVIDIA",
                0x10de,
                0x1fef,
                VulkanDeviceType::Discrete,
            ),
            vk(
                "Synthetic Intel",
                0x8086,
                0x9a49,
                VulkanDeviceType::Integrated,
            ),
        ];
        vulkan_devices[0].pci_identity = Some("0001:01:00.0".parse().unwrap());
        vulkan_devices[1].pci_identity = Some("0000:00:02.0".parse().unwrap());
        let mappings = map_vulkan_devices(&pci_devices, &vulkan_devices);
        assert_eq!(
            mappings,
            [VulkanMapping::PciIdentity, VulkanMapping::PciIdentity]
        );
        assert_eq!(unique_pci_match(&pci_devices, &vulkan_devices[0]), Some(1));
        assert_eq!(
            allocate_gpu_ids(2)
                .unwrap()
                .iter()
                .map(|id| id.get())
                .collect::<Vec<_>>(),
            [2, 3]
        );
    }

    #[test]
    fn duplicate_vendor_device_pairs_match_only_when_pci_identity_is_exact() {
        let duplicate = vec![
            pci("0000:01:00.0", 0x10de, 0x1234),
            pci("0000:02:00.0", 0x10de, 0x1234),
        ];
        let mut devices = vec![
            vk(
                "Synthetic device A",
                0x10de,
                0x1234,
                VulkanDeviceType::Discrete,
            ),
            vk(
                "Synthetic device B",
                0x10de,
                0x1234,
                VulkanDeviceType::Discrete,
            ),
            vk("Software renderer", 0x10005, 1, VulkanDeviceType::Cpu),
        ];
        devices[0].pci_identity = Some("0000:01:00.0".parse().unwrap());
        devices[1].pci_identity = Some("0000:01:00.0".parse().unwrap());
        assert_eq!(
            map_vulkan_devices(&duplicate, &devices),
            [
                VulkanMapping::AmbiguousPciIdentity,
                VulkanMapping::AmbiguousPciIdentity,
                VulkanMapping::Unmatched
            ]
        );
    }

    #[test]
    fn vendor_and_device_ids_without_pci_extension_remain_unmatched() {
        let pci_devices = vec![pci("0000:01:00.0", 0x10de, 0x1234)];
        let vulkan_devices = vec![vk(
            "Synthetic device",
            0x10de,
            0x1234,
            VulkanDeviceType::Discrete,
        )];
        assert_eq!(
            map_vulkan_devices(&pci_devices, &vulkan_devices),
            [VulkanMapping::Unmatched]
        );
    }

    #[test]
    fn contradictory_vendor_device_metadata_does_not_corrupt_pci_gpu() {
        let pci_devices = vec![pci("0000:01:00.0", 0x10de, 0x1234)];
        let mut vulkan_devices = vec![vk(
            "Synthetic device",
            0x10de,
            0x5678,
            VulkanDeviceType::Discrete,
        )];
        vulkan_devices[0].pci_identity = Some("0000:01:00.0".parse().unwrap());
        assert_eq!(
            map_vulkan_devices(&pci_devices, &vulkan_devices),
            [VulkanMapping::ContradictoryPciIdentity]
        );
    }

    #[test]
    fn synthetic_desktop_and_amd_devices_use_verified_bus_identity() {
        // Synthetic reference label; production model names come only from a
        // Vulkan device correlated to this exact BDF.
        let desktop_pci = vec![pci("0000:01:00.0", 0x10de, 0x5070)];
        let mut desktop_vulkan = vec![vk(
            "Synthetic NVIDIA RTX 5070",
            0x10de,
            0x5070,
            VulkanDeviceType::Discrete,
        )];
        desktop_vulkan[0].pci_identity = Some("0000:01:00.0".parse().unwrap());
        assert_eq!(
            map_vulkan_devices(&desktop_pci, &desktop_vulkan),
            [VulkanMapping::PciIdentity]
        );

        let mut amd_pci = pci("0002:02:00.0", 0x1002, 0x744c);
        amd_pci.driver_name = Some("amdgpu".to_owned());
        amd_pci.driver_binding = Availability::Available;
        let mut amd_vulkan = vec![vk(
            "Synthetic AMD graphics device",
            0x1002,
            0x744c,
            VulkanDeviceType::Discrete,
        )];
        amd_vulkan[0].pci_identity = Some("0002:02:00.0".parse().unwrap());
        assert_eq!(gpu_vendor(amd_pci.vendor_id), GpuVendor::Amd);
        assert_eq!(amd_pci.driver_name.as_deref(), Some("amdgpu"));
        assert_eq!(
            map_vulkan_devices(&[amd_pci], &amd_vulkan),
            [VulkanMapping::PciIdentity]
        );
    }

    #[test]
    fn vulkan_driver_identity_is_kept_separate_from_api_and_kernel_driver_versions() {
        let mut device = vk(
            "Synthetic device",
            0x10de,
            0x1234,
            VulkanDeviceType::Discrete,
        );
        device.driver_identity = Some(VulkanDriverIdentity {
            driver_id: 13,
            name: "Mesa lavapipe".to_owned(),
        });
        let mut status = VulkanLoaderStatus::Enumerated;
        let mut diagnostics = Vec::new();
        let valid = validate_vulkan_devices(vec![device], &mut status, &mut diagnostics);
        assert_eq!(status, VulkanLoaderStatus::Enumerated);
        assert_eq!(valid[0].api_version, Some((1, 3, 0)));
        assert_eq!(valid[0].driver_identity.as_ref().unwrap().driver_id, 13);
        assert_eq!(
            valid[0].driver_identity.as_ref().unwrap().name,
            "Mesa lavapipe"
        );

        let mut invalid = vk(
            "Synthetic device",
            0x10de,
            0x1234,
            VulkanDeviceType::Discrete,
        );
        invalid.driver_identity = Some(VulkanDriverIdentity {
            driver_id: -1,
            name: "\u{1b}bad".to_owned(),
        });
        let mut status = VulkanLoaderStatus::Enumerated;
        let mut diagnostics = Vec::new();
        let valid = validate_vulkan_devices(vec![invalid], &mut status, &mut diagnostics);
        assert_eq!(status, VulkanLoaderStatus::Degraded);
        assert!(valid[0].driver_identity.is_none());
        assert!(!diagnostics.is_empty());

        let path_like = vk(
            "Mesa renderer /dev/dri/renderD128",
            0x10005,
            1,
            VulkanDeviceType::Cpu,
        );
        let mut status = VulkanLoaderStatus::Enumerated;
        let mut diagnostics = Vec::new();
        assert!(validate_vulkan_devices(vec![path_like], &mut status, &mut diagnostics).is_empty());
        assert_eq!(status, VulkanLoaderStatus::Degraded);
    }

    #[test]
    fn kernel_driver_binding_names_are_preserved_without_version_inference() {
        for name in ["i915", "xe", "amdgpu", "nouveau", "nvidia", "future-gpu"] {
            let source = attributes(Ok(name.as_bytes().to_vec()));
            let device = read_pci_device(&source, "0000:00:02.0".to_owned())
                .unwrap()
                .unwrap();
            assert_eq!(device.driver_name.as_deref(), Some(name));
            assert_eq!(device.driver_binding, Availability::Available);
        }
    }

    #[test]
    fn absent_denied_and_malformed_driver_bindings_keep_distinct_states() {
        let absent = attributes(Err(DataSourceErrorKind::NotFound));
        let device = read_pci_device(&absent, "0000:00:02.0".to_owned())
            .unwrap()
            .unwrap();
        assert_eq!(device.driver_binding, Availability::Unavailable);
        assert!(device.driver_name.is_none());
        assert!(device.driver_diagnostic.is_none());

        let denied = attributes(Err(DataSourceErrorKind::PermissionDenied));
        let device = read_pci_device(&denied, "0000:00:02.0".to_owned())
            .unwrap()
            .unwrap();
        assert_eq!(device.driver_binding, Availability::Unknown);
        assert!(device.driver_diagnostic.is_some());

        let malformed = attributes(Ok(b"../nvidia".to_vec()));
        let device = read_pci_device(&malformed, "0000:00:02.0".to_owned())
            .unwrap()
            .unwrap();
        assert_eq!(device.driver_binding, Availability::Unknown);
        assert!(device.driver_name.is_none());
    }

    #[test]
    fn non_graphics_pci_devices_are_excluded_before_vendor_lookup() {
        let mut source = attributes(Err(DataSourceErrorKind::NotFound));
        source
            .values
            .insert(PciAttribute::Class, Ok(b"0x120000\n".to_vec()));
        source
            .values
            .insert(PciAttribute::Vendor, Ok(b"not-a-vendor".to_vec()));
        assert!(
            read_pci_device(&source, "0000:00:02.0".to_owned())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn unknown_pci_vendor_retains_its_numeric_identity() {
        assert_eq!(gpu_vendor(0x8086), GpuVendor::Intel);
        assert_eq!(gpu_vendor(0x10de), GpuVendor::Nvidia);
        assert_eq!(gpu_vendor(0x1002), GpuVendor::Amd);
        assert_eq!(
            gpu_vendor(0xffff),
            GpuVendor::Other("PCI vendor 0xffff".to_owned())
        );
    }
}
