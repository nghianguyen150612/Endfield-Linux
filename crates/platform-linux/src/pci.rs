//! Strict PCI identity parsing and display-controller class filtering.

use std::fmt;
use std::str::FromStr;

pub const MAX_PCI_DEVICES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PciBdf {
    pub domain: u16,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl FromStr for PciBdf {
    type Err = PciBdfError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if !value.is_ascii()
            || value.len() != 12
            || value.as_bytes()[4] != b':'
            || value.as_bytes()[7] != b':'
            || value.as_bytes()[10] != b'.'
            || ![0..4, 5..7, 8..10, 11..12]
                .into_iter()
                .all(|range| value[range].bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(PciBdfError);
        }
        let domain = u16::from_str_radix(&value[0..4], 16).map_err(|_| PciBdfError)?;
        let bus = u8::from_str_radix(&value[5..7], 16).map_err(|_| PciBdfError)?;
        let device = u8::from_str_radix(&value[8..10], 16).map_err(|_| PciBdfError)?;
        let function = u8::from_str_radix(&value[11..12], 16).map_err(|_| PciBdfError)?;
        if device > 31 || function > 7 {
            return Err(PciBdfError);
        }
        Ok(Self {
            domain,
            bus,
            device,
            function,
        })
    }
}

impl fmt::Display for PciBdf {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04x}:{:02x}:{:02x}.{}",
            self.domain, self.bus, self.device, self.function
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciBdfError;

impl PciBdf {
    pub const fn is_valid(self) -> bool {
        self.device <= 31 && self.function <= 7
    }

    pub const fn is_display_controller_class(class: u32) -> bool {
        class >> 16 == 0x03 && matches!((class >> 8) & 0xff, 0x00 | 0x02 | 0x80)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_strict_bdfs_and_display_classes() {
        assert_eq!(
            "0001:af:1f.7".parse::<PciBdf>().unwrap().to_string(),
            "0001:af:1f.7"
        );
        assert!("0000:00:20.0".parse::<PciBdf>().is_err());
        assert!("0000:00:01.8".parse::<PciBdf>().is_err());
        assert!(PciBdf::is_display_controller_class(0x030000));
        assert!(PciBdf::is_display_controller_class(0x030200));
        assert!(PciBdf::is_display_controller_class(0x038000));
        assert!(!PciBdf::is_display_controller_class(0x120000));
    }
}
