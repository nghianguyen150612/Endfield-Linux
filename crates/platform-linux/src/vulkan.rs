//! Dynamically loaded Vulkan physical-device inventory. No device or surface is created.

use crate::pci::PciBdf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VulkanLoaderStatus {
    Missing,
    InitializationFailed,
    AvailableNoDevices,
    Enumerated,
    Degraded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VulkanDeviceType {
    Integrated,
    Discrete,
    Virtual,
    Cpu,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VulkanDriverIdentity {
    /// Raw Vulkan `VkDriverId`; it is not a kernel driver version.
    pub driver_id: i32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VulkanPhysicalDevice {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    /// Present only when `VK_EXT_pci_bus_info` is supported and returns a
    /// validated PCI address for this physical device.
    pub pci_identity: Option<PciBdf>,
    /// Optional metadata from `VK_KHR_driver_properties`.
    pub driver_identity: Option<VulkanDriverIdentity>,
    pub device_type: VulkanDeviceType,
    pub api_version: Option<(u32, u32, u32)>,
    /// Vulkan heap sizes are reported with Vulkan semantics. They are not
    /// silently converted into dedicated VRAM capacity.
    pub device_local_heap_bytes: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VulkanFacts {
    pub status: VulkanLoaderStatus,
    pub devices: Vec<VulkanPhysicalDevice>,
    pub diagnostic: Option<&'static str>,
}

pub trait VulkanBackend: Send + Sync {
    fn enumerate(&self) -> VulkanFacts;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemVulkanBackend;

impl VulkanBackend for SystemVulkanBackend {
    fn enumerate(&self) -> VulkanFacts {
        // `ash` resolves the system Vulkan loader at runtime. All unsafe calls
        // are isolated here; handles remain scoped to this function and the
        // instance is destroyed on every successful creation path.
        let entry = match unsafe { ash::Entry::load() } {
            Ok(entry) => entry,
            Err(ash::LoadingError::LibraryLoadFailure(_)) => {
                return VulkanFacts {
                    status: VulkanLoaderStatus::Missing,
                    devices: Vec::new(),
                    diagnostic: Some("Vulkan loader library was missing or could not be loaded"),
                };
            }
            Err(ash::LoadingError::MissingEntryPoint(_)) => {
                return VulkanFacts {
                    status: VulkanLoaderStatus::InitializationFailed,
                    devices: Vec::new(),
                    diagnostic: Some("Vulkan loader is missing a required entry point"),
                };
            }
        };
        let loader_version = unsafe { entry.try_enumerate_instance_version() }
            .ok()
            .flatten()
            .unwrap_or(ash::vk::API_VERSION_1_0);
        let core_properties2 = loader_version >= ash::vk::API_VERSION_1_1;
        let properties2_extension = if core_properties2 {
            false
        } else {
            unsafe { entry.enumerate_instance_extension_properties(None) }
                .ok()
                .is_some_and(|extensions| {
                    extensions.iter().any(|extension| {
                        extension.extension_name_as_c_str().ok()
                            == Some(ash::vk::KHR_GET_PHYSICAL_DEVICE_PROPERTIES2_NAME)
                    })
                })
        };
        let enabled_extensions = if properties2_extension {
            [ash::vk::KHR_GET_PHYSICAL_DEVICE_PROPERTIES2_NAME.as_ptr()]
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let requested_api = if core_properties2 {
            ash::vk::API_VERSION_1_1
        } else {
            ash::vk::API_VERSION_1_0
        };
        let application = ash::vk::ApplicationInfo::default().api_version(requested_api);
        let create_info = ash::vk::InstanceCreateInfo::default()
            .application_info(&application)
            .enabled_extension_names(&enabled_extensions);
        let instance = match unsafe { entry.create_instance(&create_info, None) } {
            Ok(instance) => instance,
            Err(_) => {
                return VulkanFacts {
                    status: VulkanLoaderStatus::InitializationFailed,
                    devices: Vec::new(),
                    diagnostic: Some("Vulkan instance initialization failed"),
                };
            }
        };
        let enumeration = unsafe { instance.enumerate_physical_devices() };
        let physical_devices = match enumeration {
            Ok(devices) => devices,
            Err(_) => {
                unsafe { instance.destroy_instance(None) };
                return VulkanFacts {
                    status: VulkanLoaderStatus::Degraded,
                    devices: Vec::new(),
                    diagnostic: Some("Vulkan physical-device enumeration failed"),
                };
            }
        };
        let properties2_khr = if properties2_extension {
            Some(ash::khr::get_physical_device_properties2::Instance::new(
                &entry, &instance,
            ))
        } else {
            None
        };
        let mut devices = Vec::new();
        let mut malformed = false;
        for physical in physical_devices.iter().take(64) {
            // A physical device is a handle returned by this live instance.
            let properties = unsafe { instance.get_physical_device_properties(*physical) };
            let name_bytes = unsafe {
                let bytes = properties.device_name.as_ptr().cast::<u8>();
                let all = std::slice::from_raw_parts(bytes, properties.device_name.len());
                let length = all.iter().position(|byte| *byte == 0).unwrap_or(all.len());
                &all[..length]
            };
            let Ok(name) = std::str::from_utf8(name_bytes) else {
                malformed = true;
                continue;
            };
            if name.is_empty()
                || name.len() > 256
                || name.chars().any(char::is_control)
                || name
                    .chars()
                    .any(|character| matches!(character, '/' | '\\'))
            {
                malformed = true;
                continue;
            }
            let device_type = match properties.device_type {
                ash::vk::PhysicalDeviceType::INTEGRATED_GPU => VulkanDeviceType::Integrated,
                ash::vk::PhysicalDeviceType::DISCRETE_GPU => VulkanDeviceType::Discrete,
                ash::vk::PhysicalDeviceType::VIRTUAL_GPU => VulkanDeviceType::Virtual,
                ash::vk::PhysicalDeviceType::CPU => VulkanDeviceType::Cpu,
                _ => VulkanDeviceType::Other,
            };
            let (pci_identity, driver_identity, invalid_extended_properties) =
                read_extended_device_properties(
                    &instance,
                    *physical,
                    core_properties2,
                    properties2_khr.as_ref(),
                );
            malformed |= invalid_extended_properties;
            let memory = unsafe { instance.get_physical_device_memory_properties(*physical) };
            let reported_heap_count =
                usize::try_from(memory.memory_heap_count).unwrap_or(usize::MAX);
            if reported_heap_count > memory.memory_heaps.len() {
                malformed = true;
            }
            let heap_count = reported_heap_count.min(memory.memory_heaps.len());
            let device_local_heap_bytes = memory.memory_heaps[..heap_count]
                .iter()
                .filter(|heap| heap.flags.contains(ash::vk::MemoryHeapFlags::DEVICE_LOCAL))
                .map(|heap| heap.size)
                .collect();
            let version = properties.api_version;
            let major = ash::vk::api_version_major(version);
            let api_version = if major == 0 {
                malformed = true;
                None
            } else {
                Some((
                    major,
                    ash::vk::api_version_minor(version),
                    ash::vk::api_version_patch(version),
                ))
            };
            devices.push(VulkanPhysicalDevice {
                name: name.to_owned(),
                vendor_id: properties.vendor_id,
                device_id: properties.device_id,
                pci_identity,
                driver_identity,
                device_type,
                api_version,
                device_local_heap_bytes,
            });
        }
        unsafe { instance.destroy_instance(None) };
        let truncated = physical_devices.len() > 64;
        let degraded = malformed || truncated;
        VulkanFacts {
            status: if degraded {
                VulkanLoaderStatus::Degraded
            } else if devices.is_empty() {
                VulkanLoaderStatus::AvailableNoDevices
            } else {
                VulkanLoaderStatus::Enumerated
            },
            devices,
            diagnostic: if malformed {
                Some("Vulkan returned invalid physical-device metadata")
            } else if truncated {
                Some("Vulkan physical-device count exceeded the enumeration limit")
            } else {
                None
            },
        }
    }
}

fn read_extended_device_properties(
    instance: &ash::Instance,
    physical: ash::vk::PhysicalDevice,
    core_properties2: bool,
    properties2_khr: Option<&ash::khr::get_physical_device_properties2::Instance>,
) -> (Option<PciBdf>, Option<VulkanDriverIdentity>, bool) {
    // Query an extension structure only after confirming that the device
    // advertises the corresponding extension. Missing data remains optional.
    let Ok(extensions) = (unsafe { instance.enumerate_device_extension_properties(physical) })
    else {
        return (None, None, false);
    };
    let supports_pci_bus_info = extensions.iter().any(|extension| {
        extension.extension_name_as_c_str().ok() == Some(ash::vk::EXT_PCI_BUS_INFO_NAME)
    });
    let supports_driver_properties = extensions.iter().any(|extension| {
        extension.extension_name_as_c_str().ok() == Some(ash::vk::KHR_DRIVER_PROPERTIES_NAME)
    });
    if !supports_pci_bus_info && !supports_driver_properties {
        return (None, None, false);
    }
    let mut bus = ash::vk::PhysicalDevicePCIBusInfoPropertiesEXT::default();
    let mut driver = ash::vk::PhysicalDeviceDriverProperties::default();
    let mut properties = ash::vk::PhysicalDeviceProperties2::default();
    if supports_pci_bus_info {
        properties = properties.push_next(&mut bus);
    }
    if supports_driver_properties {
        properties = properties.push_next(&mut driver);
    }
    // `VK_KHR_get_physical_device_properties2` is enabled for Vulkan 1.0
    // instances when present; core 1.1 is requested when supported.
    if core_properties2 {
        unsafe { instance.get_physical_device_properties2(physical, &mut properties) };
    } else {
        let Some(extension) = properties2_khr else {
            return (None, None, false);
        };
        unsafe { extension.get_physical_device_properties2(physical, &mut properties) };
    }
    let mut malformed = false;
    let pci_identity = if supports_pci_bus_info {
        let domain = u16::try_from(bus.pci_domain).ok();
        let bus_number = u8::try_from(bus.pci_bus).ok();
        let device = u8::try_from(bus.pci_device).ok();
        let function = u8::try_from(bus.pci_function).ok();
        match (domain, bus_number, device, function) {
            (Some(domain), Some(bus), Some(device), Some(function))
                if device <= 31 && function <= 7 =>
            {
                Some(PciBdf {
                    domain,
                    bus,
                    device,
                    function,
                })
            }
            _ => {
                malformed = true;
                None
            }
        }
    } else {
        None
    };
    let driver_identity = if supports_driver_properties {
        let name = driver
            .driver_name_as_c_str()
            .ok()
            .and_then(|value| value.to_str().ok());
        match name {
            Some(name)
                if !name.is_empty()
                    && name.len() <= 256
                    && !name.chars().any(char::is_control)
                    && !name
                        .chars()
                        .any(|character| matches!(character, '/' | '\\'))
                    && driver.driver_id.as_raw() >= 0 =>
            {
                Some(VulkanDriverIdentity {
                    driver_id: driver.driver_id.as_raw(),
                    name: name.to_owned(),
                })
            }
            _ => {
                malformed = true;
                None
            }
        }
    } else {
        None
    };
    (pci_identity, driver_identity, malformed)
}
