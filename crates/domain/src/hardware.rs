use crate::capability::{Availability, InstanceId};
use crate::validation::{ValidationError, validate_text};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bytes(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hertz(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityValue<T> {
    pub availability: Availability,
    pub value: Option<T>,
}

impl<T> CapabilityValue<T> {
    pub fn new(availability: Availability, value: Option<T>) -> Self {
        Self {
            availability,
            value,
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if matches!(
            self.availability,
            Availability::Unknown | Availability::Unavailable
        ) && self.value.is_some()
        {
            return Err(ValidationError::ValueNotAllowedForAvailability {
                availability: self.availability.abi_id(),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpuTopology {
    pub logical_processor_count: u32,
    pub physical_core_count: Option<u32>,
}

impl CpuTopology {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.logical_processor_count == 0 {
            return Err(ValidationError::InvalidValue {
                field: "logical processor count",
                reason: "must be greater than zero when measured",
            });
        }
        if let Some(physical) = self.physical_core_count {
            if physical == 0 || physical > self.logical_processor_count {
                return Err(ValidationError::InvalidValue {
                    field: "physical core count",
                    reason: "must be positive and no greater than logical processors",
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpuInfo {
    pub instance: InstanceId,
    pub model_identifier: Option<String>,
    pub topology: CapabilityValue<CpuTopology>,
    pub observed_frequency: CapabilityValue<Hertz>,
    pub scheduling: Availability,
    pub power_management: Availability,
}

impl CpuInfo {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if let Some(model) = &self.model_identifier {
            validate_text(model, "CPU model identifier", 256)?;
        }
        self.topology.validate()?;
        if let Some(topology) = &self.topology.value {
            topology.validate()?;
        }
        self.observed_frequency.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GpuVendor {
    Amd,
    Intel,
    Nvidia,
    Apple,
    Other(String),
}

impl GpuVendor {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if let Self::Other(value) = self {
            validate_text(value, "GPU vendor", 128)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuClass {
    Integrated,
    Discrete,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphicsDriver {
    pub name: String,
    pub version: Option<String>,
}

impl GraphicsDriver {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_text(&self.name, "graphics driver name", 128)?;
        if let Some(version) = &self.version {
            validate_text(version, "graphics driver version", 128)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuInfo {
    pub instance: InstanceId,
    pub availability: Availability,
    pub vendor: Option<GpuVendor>,
    pub model: Option<String>,
    pub class: GpuClass,
    pub graphics_driver: Option<GraphicsDriver>,
    pub driver_availability: Availability,
    pub vram: CapabilityValue<Bytes>,
    pub hardware_acceleration: Availability,
    pub vulkan_device: Availability,
}

impl GpuInfo {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if matches!(
            self.availability,
            Availability::Unknown | Availability::Unavailable
        ) && (self.vendor.is_some()
            || self.model.is_some()
            || self.class != GpuClass::Unknown
            || self.graphics_driver.is_some())
        {
            return Err(ValidationError::ValueNotAllowedForAvailability {
                availability: self.availability.abi_id(),
            });
        }
        if let Some(vendor) = &self.vendor {
            vendor.validate()?;
        }
        if let Some(model) = &self.model {
            validate_text(model, "GPU model", 256)?;
        }
        if let Some(driver) = &self.graphics_driver {
            driver.validate()?;
        }
        if matches!(
            self.driver_availability,
            Availability::Unknown | Availability::Unavailable
        ) && self.graphics_driver.is_some()
        {
            return Err(ValidationError::ValueNotAllowedForAvailability {
                availability: self.driver_availability.abi_id(),
            });
        }
        self.vram.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryCapacity {
    pub total: Option<Bytes>,
    pub available: Option<Bytes>,
}

impl MemoryCapacity {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if matches!(self.total, Some(Bytes(0))) {
            return Err(ValidationError::InvalidValue {
                field: "total RAM",
                reason: "zero is not a valid total memory capacity",
            });
        }
        if let (Some(Bytes(total)), Some(Bytes(available))) = (self.total, self.available) {
            if available > total {
                return Err(ValidationError::InvalidValue {
                    field: "available RAM",
                    reason: "cannot exceed total RAM",
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwapInfo {
    pub availability: Availability,
    pub present: Option<bool>,
    pub total: Option<Bytes>,
}

impl SwapInfo {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if matches!(
            self.availability,
            Availability::Unknown | Availability::Unavailable
        ) && (self.present.is_some() || self.total.is_some())
        {
            return Err(ValidationError::ValueNotAllowedForAvailability {
                availability: self.availability.abi_id(),
            });
        }
        match (self.present, self.total) {
            (Some(false), Some(_)) => Err(ValidationError::InvalidValue {
                field: "swap size",
                reason: "size is not applicable when swap is absent",
            }),
            (Some(true), Some(Bytes(0))) => Err(ValidationError::InvalidValue {
                field: "swap size",
                reason: "must be positive when swap is present",
            }),
            (None, Some(_)) => Err(ValidationError::InvalidValue {
                field: "swap size",
                reason: "cannot be known when swap presence is unknown",
            }),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryInfo {
    pub ram: CapabilityValue<MemoryCapacity>,
    pub swap: SwapInfo,
}

impl MemoryInfo {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.ram.validate()?;
        if let Some(ram) = &self.ram.value {
            ram.validate()?;
        }
        self.swap.validate()
    }
}
