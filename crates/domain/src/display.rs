use crate::capability::{Availability, InstanceId};
use crate::hardware::Hertz;
use crate::validation::{ValidationError, validate_text};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub width_pixels: u32,
    pub height_pixels: u32,
}

impl Resolution {
    pub fn new(width_pixels: u32, height_pixels: u32) -> Result<Self, ValidationError> {
        let resolution = Self {
            width_pixels,
            height_pixels,
        };
        resolution.validate()?;
        Ok(resolution)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.width_pixels == 0 || self.height_pixels == 0 {
            return Err(ValidationError::InvalidValue {
                field: "display resolution",
                reason: "width and height must be positive",
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayConnection {
    Internal,
    External,
    Virtual,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayInfo {
    pub instance: InstanceId,
    /// A local label only. Adapters should omit EDID serials and other private identifiers.
    pub label: Option<String>,
    pub availability: Availability,
    pub resolution: Option<Resolution>,
    pub refresh_rate: Option<Hertz>,
    pub primary: Option<bool>,
    pub connection: Option<DisplayConnection>,
    pub connection_availability: Availability,
    pub session_availability: Availability,
}

impl DisplayInfo {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if let Some(label) = &self.label {
            validate_text(label, "display label", 128)?;
        }
        if matches!(
            self.availability,
            Availability::Unknown | Availability::Unavailable
        ) && (self.resolution.is_some() || self.refresh_rate.is_some() || self.primary.is_some())
        {
            return Err(ValidationError::ValueNotAllowedForAvailability {
                availability: self.availability.abi_id(),
            });
        }
        if let Some(resolution) = &self.resolution {
            resolution.validate()?;
        }
        if matches!(self.refresh_rate, Some(Hertz(0))) {
            return Err(ValidationError::InvalidValue {
                field: "display refresh rate",
                reason: "must be positive when measured",
            });
        }
        if matches!(
            self.connection_availability,
            Availability::Unknown | Availability::Unavailable
        ) && self.connection.is_some()
        {
            return Err(ValidationError::ValueNotAllowedForAvailability {
                availability: self.connection_availability.abi_id(),
            });
        }
        Ok(())
    }
}
