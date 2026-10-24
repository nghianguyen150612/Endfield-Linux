use crate::validation::{ValidationError, validate_text};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum PlatformFamily {
    Linux = 1,
    MacOs = 2,
    FreeBsd = 3,
    OtherUnknown = 255,
}

impl PlatformFamily {
    pub const fn abi_id(self) -> u32 {
        match self {
            Self::Linux => 1,
            Self::MacOs => 2,
            Self::FreeBsd => 3,
            Self::OtherUnknown => 255,
        }
    }
}

impl TryFrom<u32> for PlatformFamily {
    type Error = ValidationError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Linux),
            2 => Ok(Self::MacOs),
            3 => Ok(Self::FreeBsd),
            255 => Ok(Self::OtherUnknown),
            _ => Err(ValidationError::UnknownIdentifier {
                kind: "platform family",
                value: u64::from(value),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Architecture {
    Unknown,
    X86,
    X86_64,
    Arm,
    Aarch64,
    RiscV32,
    RiscV64,
    PowerPc,
    PowerPc64,
    S390x,
    Other(String),
}

impl Architecture {
    /// Parses a portable architecture name without consulting the host.
    pub fn parse(name: &str) -> Result<Self, ValidationError> {
        validate_text(name, "architecture", 64)?;
        let normalized = name.trim().to_ascii_lowercase();
        Ok(match normalized.as_str() {
            "unknown" | "unspecified" => Self::Unknown,
            "i386" | "i486" | "i586" | "i686" | "x86" => Self::X86,
            "x86_64" | "amd64" | "x64" => Self::X86_64,
            "arm" | "armv6" | "armv7" | "armv7l" => Self::Arm,
            "aarch64" | "arm64" => Self::Aarch64,
            "riscv32" | "riscv32gc" => Self::RiscV32,
            "riscv64" | "riscv64gc" => Self::RiscV64,
            "powerpc" | "ppc" => Self::PowerPc,
            "powerpc64" | "ppc64" | "ppc64le" => Self::PowerPc64,
            "s390x" => Self::S390x,
            _ => Self::Other(normalized),
        })
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Unknown => "unknown",
            Self::X86 => "x86",
            Self::X86_64 => "x86_64",
            Self::Arm => "arm",
            Self::Aarch64 => "aarch64",
            Self::RiscV32 => "riscv32",
            Self::RiscV64 => "riscv64",
            Self::PowerPc => "powerpc",
            Self::PowerPc64 => "powerpc64",
            Self::S390x => "s390x",
            Self::Other(value) => value,
        }
    }

    fn validate(&self) -> Result<(), ValidationError> {
        if let Self::Other(value) = self {
            validate_text(value, "architecture", 64)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsIdentity {
    pub name: String,
    pub version: Option<String>,
}

impl OsIdentity {
    pub fn new(name: impl Into<String>, version: Option<String>) -> Result<Self, ValidationError> {
        let identity = Self {
            name: name.into(),
            version,
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_text(&self.name, "OS name", 128)?;
        validate_optional_text(self.version.as_deref(), "OS version", 128)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KernelIdentity {
    pub name: String,
    pub version: Option<String>,
}

impl KernelIdentity {
    pub fn new(name: impl Into<String>, version: Option<String>) -> Result<Self, ValidationError> {
        let identity = Self {
            name: name.into(),
            version,
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_text(&self.name, "kernel name", 128)?;
        validate_optional_text(self.version.as_deref(), "kernel version", 128)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DistributionIdentity {
    pub id: String,
    pub version: Option<String>,
    pub display_name: Option<String>,
}

impl DistributionIdentity {
    pub fn new(
        id: impl Into<String>,
        version: Option<String>,
        display_name: Option<String>,
    ) -> Result<Self, ValidationError> {
        let identity = Self {
            id: id.into(),
            version,
            display_name,
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_text(&self.id, "distribution identifier", 128)?;
        validate_optional_text(self.version.as_deref(), "distribution version", 128)?;
        validate_optional_text(self.display_name.as_deref(), "distribution name", 128)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformIdentity {
    pub family: PlatformFamily,
    pub os: OsIdentity,
    pub architecture: Architecture,
    pub kernel: Option<KernelIdentity>,
    pub distribution: Option<DistributionIdentity>,
}

impl PlatformIdentity {
    pub fn new(
        family: PlatformFamily,
        os: OsIdentity,
        architecture: Architecture,
        kernel: Option<KernelIdentity>,
        distribution: Option<DistributionIdentity>,
    ) -> Result<Self, ValidationError> {
        let identity = Self {
            family,
            os,
            architecture,
            kernel,
            distribution,
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        self.os.validate()?;
        self.architecture.validate()?;
        if let Some(kernel) = &self.kernel {
            kernel.validate()?;
        }
        if let Some(distribution) = &self.distribution {
            if self.family != PlatformFamily::Linux {
                return Err(ValidationError::InvalidPlatformDistribution);
            }
            distribution.validate()?;
        }
        Ok(())
    }
}

fn validate_optional_text(
    value: Option<&str>,
    field: &'static str,
    max_bytes: usize,
) -> Result<(), ValidationError> {
    if let Some(value) = value {
        validate_text(value, field, max_bytes)?;
    }
    Ok(())
}
