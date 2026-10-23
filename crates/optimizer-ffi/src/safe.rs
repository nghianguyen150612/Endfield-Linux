use super::raw;
use core::fmt;
use core::marker::PhantomData;
use core::ptr::NonNull;
use std::rc::Rc;

const MAX_CAPABILITIES: usize = 4096;
const MAX_GOALS: usize = 8;
const MAX_PROVENANCE_BYTES: usize = 4096;

/// Returns the ABI version reported by the linked C17 library.
pub fn abi_version() -> u32 {
    raw::abi_version()
}

/// Checks major-version compatibility between a client and the linked library.
///
/// Compatible minor versions may add trailing size-versioned fields. This wrapper
/// only emits the ABI 1.0 request prefix and accepts any version with major 1.
pub fn validate_abi_compatibility(client_version: u32, library_version: u32) -> Result<(), Error> {
    let supported_major = super::ABI_VERSION >> 16;
    if (client_version >> 16) != supported_major || (library_version >> 16) != supported_major {
        return Err(Error::UnsupportedAbi {
            requested: client_version,
            available: library_version,
        });
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOs,
    FreeBsd,
    OtherUnknown,
}

impl Platform {
    pub(crate) fn as_raw(self) -> u32 {
        match self {
            Self::Linux => 1,
            Self::MacOs => 2,
            Self::FreeBsd => 3,
            Self::OtherUnknown => 255,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intensity {
    L0,
    L1,
    L2,
    L3,
    L4,
}

impl Intensity {
    pub(crate) fn as_raw(self) -> u32 {
        match self {
            Self::L0 => 0,
            Self::L1 => 1,
            Self::L2 => 2,
            Self::L3 => 3,
            Self::L4 => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Unknown,
    Unavailable,
    Available,
    Degraded,
}

impl Availability {
    pub(crate) fn as_raw(self) -> u32 {
        match self {
            Self::Unknown => 0,
            Self::Unavailable => 1,
            Self::Available => 2,
            Self::Degraded => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Count,
    Bytes,
    Hertz,
    Pixels,
    MilliFramesPerSecond,
    Milliseconds,
    Microseconds,
    Nanoseconds,
    MilliCelsius,
    Milliwatts,
    Millivolts,
    Joules,
    Millijoules,
}

impl Unit {
    pub(crate) fn as_raw(self) -> u32 {
        match self {
            Self::Count => 1,
            Self::Bytes => 2,
            Self::Hertz => 3,
            Self::Pixels => 4,
            Self::MilliFramesPerSecond => 5,
            Self::Milliseconds => 6,
            Self::Microseconds => 7,
            Self::Nanoseconds => 8,
            Self::MilliCelsius => 9,
            Self::Milliwatts => 10,
            Self::Millivolts => 11,
            Self::Joules => 12,
            Self::Millijoules => 13,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityId {
    CpuTopology,
    CpuScheduling,
    CpuFrequencyPower,
    GpuDevice,
    Vram,
    HardwareAcceleration,
    GraphicsDriver,
    VulkanDevice,
    Ram,
    Swap,
    CpuThermal,
    GpuThermal,
    PowerTelemetry,
    Display,
    CompositorSession,
    RuntimeDiscovery,
    OptimizationTools,
}

impl CapabilityId {
    pub(crate) fn as_raw(self) -> u32 {
        match self {
            Self::CpuTopology => 1,
            Self::CpuScheduling => 2,
            Self::CpuFrequencyPower => 3,
            Self::GpuDevice => 10,
            Self::Vram => 11,
            Self::HardwareAcceleration => 12,
            Self::GraphicsDriver => 13,
            Self::VulkanDevice => 14,
            Self::Ram => 20,
            Self::Swap => 21,
            Self::CpuThermal => 30,
            Self::GpuThermal => 31,
            Self::PowerTelemetry => 32,
            Self::Display => 40,
            Self::CompositorSession => 41,
            Self::RuntimeDiscovery => 50,
            Self::OptimizationTools => 51,
        }
    }
}

impl TryFrom<u32> for CapabilityId {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::CpuTopology),
            2 => Ok(Self::CpuScheduling),
            3 => Ok(Self::CpuFrequencyPower),
            10 => Ok(Self::GpuDevice),
            11 => Ok(Self::Vram),
            12 => Ok(Self::HardwareAcceleration),
            13 => Ok(Self::GraphicsDriver),
            14 => Ok(Self::VulkanDevice),
            20 => Ok(Self::Ram),
            21 => Ok(Self::Swap),
            30 => Ok(Self::CpuThermal),
            31 => Ok(Self::GpuThermal),
            32 => Ok(Self::PowerTelemetry),
            40 => Ok(Self::Display),
            41 => Ok(Self::CompositorSession),
            50 => Ok(Self::RuntimeDiscovery),
            51 => Ok(Self::OptimizationTools),
            identifier => Err(Error::UnsupportedIdentifier {
                identifier: Some(identifier),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measurement {
    pub value: u64,
    pub unit: Unit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    pub(crate) id: CapabilityId,
    pub(crate) instance_id: u32,
    pub(crate) availability: Availability,
    pub(crate) value: Option<Measurement>,
    pub(crate) provenance: Option<String>,
}

impl Capability {
    /// Creates an owned capability record. Instance zero denotes a platform-wide
    /// record; nonzero instance IDs distinguish devices or sensors in this request.
    pub fn new(
        id: CapabilityId,
        instance_id: u32,
        availability: Availability,
        value: Option<Measurement>,
        provenance: Option<String>,
    ) -> Result<Self, Error> {
        let capability = Self {
            id,
            instance_id,
            availability,
            value,
            provenance,
        };
        capability.validate()?;
        Ok(capability)
    }

    fn validate(&self) -> Result<(), Error> {
        if matches!(
            self.availability,
            Availability::Unknown | Availability::Unavailable
        ) && self.value.is_some()
        {
            return Err(Error::InvalidArguments(
                "unknown or unavailable capability cannot carry a measurement",
            ));
        }
        if self
            .provenance
            .as_ref()
            .is_some_and(|source| source.len() > MAX_PROVENANCE_BYTES)
        {
            return Err(Error::InvalidArguments("provenance text is too long"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalPreference {
    Smoothness,
    VisualQuality,
    InputLatency,
    Stability,
    ThermalNoise,
    PowerEfficiency,
}

impl GoalPreference {
    fn as_raw(self) -> u32 {
        match self {
            Self::Smoothness => 3,
            Self::VisualQuality => 4,
            Self::InputLatency => 5,
            Self::Stability => 6,
            Self::ThermalNoise => 7,
            Self::PowerEfficiency => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Goal {
    TargetFpsMilli(u64),
    TargetResolutionPixels { width: u32, height: u32 },
    Preference { goal: GoalPreference, priority: u16 },
}

impl Goal {
    pub(crate) fn to_raw(self) -> (u32, u64, u64) {
        match self {
            Self::TargetFpsMilli(milli_fps) => (1, milli_fps, 0),
            Self::TargetResolutionPixels { width, height } => {
                (2, u64::from(width), u64::from(height))
            }
            Self::Preference { goal, priority } => (goal.as_raw(), u64::from(priority), 0),
        }
    }

    fn validate(self) -> Result<(), Error> {
        match self {
            Self::TargetResolutionPixels { width: 0, .. }
            | Self::TargetResolutionPixels { height: 0, .. } => Err(Error::InvalidArguments(
                "target resolution dimensions must be nonzero pixels",
            )),
            Self::Preference { priority, .. } if priority > 1000 => Err(Error::InvalidArguments(
                "goal preference priority must be between 0 and 1000",
            )),
            _ => Ok(()),
        }
    }

    fn id(self) -> u32 {
        self.to_raw().0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub(crate) platform: Platform,
    pub(crate) intensity: Intensity,
    pub(crate) capabilities: Vec<Capability>,
    pub(crate) goals: Vec<Goal>,
}

impl Request {
    pub fn new(platform: Platform, intensity: Intensity) -> Self {
        Self {
            platform,
            intensity,
            capabilities: Vec::new(),
            goals: Vec::new(),
        }
    }

    pub fn with_capability(mut self, capability: Capability) -> Self {
        self.capabilities.push(capability);
        self
    }

    pub fn with_goal(mut self, goal: Goal) -> Self {
        self.goals.push(goal);
        self
    }

    pub fn add_capability(&mut self, capability: Capability) {
        self.capabilities.push(capability);
    }

    pub fn add_goal(&mut self, goal: Goal) {
        self.goals.push(goal);
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.capabilities.len() > MAX_CAPABILITIES || self.goals.len() > MAX_GOALS {
            return Err(Error::InvalidArguments(
                "request array exceeds the ABI limit",
            ));
        }
        for (index, capability) in self.capabilities.iter().enumerate() {
            capability.validate()?;
            if self.capabilities[..index].iter().any(|previous| {
                previous.id == capability.id && previous.instance_id == capability.instance_id
            }) {
                return Err(Error::InvalidArguments(
                    "duplicate capability and instance identifier",
                ));
            }
        }
        for (index, goal) in self.goals.iter().copied().enumerate() {
            goal.validate()?;
            if self.goals[..index]
                .iter()
                .any(|previous| previous.id() == goal.id())
            {
                return Err(Error::InvalidArguments("duplicate goal identifier"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidArguments(&'static str),
    UnsupportedAbi { requested: u32, available: u32 },
    UnsupportedIdentifier { identifier: Option<u32> },
    IncompatibleConstraints,
    ResourceExhaustion,
    InternalFailure,
    UnknownNativeStatus(u32),
    IndexOutOfBounds { index: u32, count: u32 },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments(context) => write!(formatter, "invalid arguments: {context}"),
            Self::UnsupportedAbi {
                requested,
                available,
            } => write!(
                formatter,
                "unsupported ABI version {requested:#010x}; library reports {available:#010x}"
            ),
            Self::UnsupportedIdentifier {
                identifier: Some(identifier),
            } => {
                write!(formatter, "unsupported identifier {identifier}")
            }
            Self::UnsupportedIdentifier { identifier: None } => {
                formatter.write_str("native planner rejected an unsupported identifier")
            }
            Self::IncompatibleConstraints => {
                formatter.write_str("request has incompatible or unsafe constraints")
            }
            Self::ResourceExhaustion => formatter.write_str("native planner ran out of resources"),
            Self::InternalFailure => formatter.write_str("native planner failed internally"),
            Self::UnknownNativeStatus(status) => {
                write!(formatter, "native planner returned unknown status {status}")
            }
            Self::IndexOutOfBounds { index, count } => {
                write!(
                    formatter,
                    "plan index {index} is outside plan count {count}"
                )
            }
        }
    }
}

impl std::error::Error for Error {}

fn map_native_status(status: u32) -> Error {
    match status {
        raw::STATUS_INVALID_ARGUMENT => {
            Error::InvalidArguments("native ABI rejected the request or output")
        }
        raw::STATUS_UNSUPPORTED_ABI => Error::UnsupportedAbi {
            requested: super::ABI_VERSION,
            available: raw::abi_version(),
        },
        raw::STATUS_UNSUPPORTED_IDENTIFIER => Error::UnsupportedIdentifier { identifier: None },
        raw::STATUS_INCOMPATIBLE_CONSTRAINTS => Error::IncompatibleConstraints,
        raw::STATUS_RESOURCE_EXHAUSTION => Error::ResourceExhaustion,
        raw::STATUS_INTERNAL_FAILURE => Error::InternalFailure,
        other => Error::UnknownNativeStatus(other),
    }
}

pub struct Optimizer {
    library_abi_version: u32,
}

impl Optimizer {
    pub fn new() -> Result<Self, Error> {
        let library_abi_version = abi_version();
        validate_abi_compatibility(super::ABI_VERSION, library_abi_version)?;
        Ok(Self {
            library_abi_version,
        })
    }

    pub fn abi_version(&self) -> u32 {
        self.library_abi_version
    }

    pub fn plan(&self, request: &Request) -> Result<PlanSet, Error> {
        validate_abi_compatibility(super::ABI_VERSION, self.library_abi_version)?;
        request.validate()?;
        let handle = raw::plan(request).map_err(map_native_status)?;
        Ok(PlanSet {
            handle,
            _not_send_or_sync: PhantomData,
        })
    }
}

/// Owns one C-allocated plan set. Drop calls the C destroy function exactly once.
///
/// This wrapper is deliberately neither Send nor Sync. Its native handle remains
/// confined to the thread that acquired it until the library documents a stronger guarantee.
pub struct PlanSet {
    handle: NonNull<raw::PlanSet>,
    _not_send_or_sync: PhantomData<Rc<()>>,
}

impl PlanSet {
    pub fn count(&self) -> Result<u32, Error> {
        raw::plan_count(self.handle).map_err(map_native_status)
    }

    pub fn get(&self, index: u32) -> Result<PlanView<'_>, Error> {
        let count = self.count()?;
        if index >= count {
            return Err(Error::IndexOutOfBounds { index, count });
        }
        let metadata = raw::plan_get(self.handle, index).map_err(map_native_status)?;
        Ok(PlanView {
            rank: metadata.rank,
            score: metadata.score,
            action_count: metadata.action_count,
            impact_count: metadata.impact_count,
            evidence_count: metadata.evidence_count,
            constraint_count: metadata.constraint_count,
            uncertainty: metadata.uncertainty,
            rollback_requirement: metadata.rollback_requirement,
            _owner: PhantomData,
        })
    }
}

impl Drop for PlanSet {
    fn drop(&mut self) {
        let _ = raw::destroy(self.handle);
    }
}

/// Scalar metadata copied from a plan view. The borrow lifetime prevents callers
/// from retaining a view past its owning plan set.
pub struct PlanView<'a> {
    rank: u32,
    score: u32,
    action_count: u32,
    impact_count: u32,
    evidence_count: u32,
    constraint_count: u32,
    uncertainty: u32,
    rollback_requirement: u32,
    _owner: PhantomData<&'a PlanSet>,
}

impl PlanView<'_> {
    pub fn rank(&self) -> u32 {
        self.rank
    }

    pub fn score(&self) -> u32 {
        self.score
    }

    pub fn action_count(&self) -> u32 {
        self.action_count
    }

    pub fn impact_count(&self) -> u32 {
        self.impact_count
    }

    pub fn evidence_count(&self) -> u32 {
        self.evidence_count
    }

    pub fn constraint_count(&self) -> u32 {
        self.constraint_count
    }

    pub fn uncertainty(&self) -> u32 {
        self.uncertainty
    }

    pub fn rollback_requirement(&self) -> u32 {
        self.rollback_requirement
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_native_status_has_a_typed_error() {
        assert!(matches!(
            map_native_status(raw::STATUS_INVALID_ARGUMENT),
            Error::InvalidArguments(_)
        ));
        assert!(matches!(
            map_native_status(raw::STATUS_UNSUPPORTED_ABI),
            Error::UnsupportedAbi { .. }
        ));
        assert!(matches!(
            map_native_status(raw::STATUS_UNSUPPORTED_IDENTIFIER),
            Error::UnsupportedIdentifier { .. }
        ));
        assert_eq!(
            map_native_status(raw::STATUS_INCOMPATIBLE_CONSTRAINTS),
            Error::IncompatibleConstraints
        );
        assert_eq!(
            map_native_status(raw::STATUS_RESOURCE_EXHAUSTION),
            Error::ResourceExhaustion
        );
        assert_eq!(
            map_native_status(raw::STATUS_INTERNAL_FAILURE),
            Error::InternalFailure
        );
        assert_eq!(map_native_status(99), Error::UnknownNativeStatus(99));
    }
}
