//! Portable domain contracts for `endfield-unix`.
//!
//! This crate represents normalized platform facts; it does not discover
//! operating-system or hardware state. Platform-specific adapters can
//! implement [`PlatformProbe`] and [`CapabilityProvider`] in later milestones.

#![forbid(unsafe_code)]

pub mod capability;
pub mod display;
pub mod hardware;
pub mod interfaces;
pub mod platform;
pub mod provenance;
pub mod snapshot;
pub mod validation;

pub use capability::{
    AbiMeasurement, Availability, CapabilityId, CapabilityRecord, InstanceId, Measurement,
    MeasurementOrigin, MeasurementUnit, MeasurementValue,
};
pub use display::{DisplayConnection, DisplayInfo, Resolution};
pub use hardware::{
    Bytes, CapabilityValue, CpuInfo, CpuTopology, GpuClass, GpuInfo, GpuVendor, GraphicsDriver,
    Hertz, MemoryCapacity, MemoryInfo, SwapInfo,
};
pub use interfaces::{
    CapabilityProvider, CompositePlatformProbe, PlatformProbe, ProbeError, ProbeErrorKind,
    ProbeReport,
};
pub use platform::{
    Architecture, DistributionIdentity, KernelIdentity, OsIdentity, PlatformFamily,
    PlatformIdentity,
};
pub use provenance::{Diagnostic, DiagnosticCategory, Provenance, ProvenanceId};
pub use snapshot::{CURRENT_SNAPSHOT_SCHEMA_VERSION, SystemSnapshot};
pub use validation::{ValidationError, ValidationErrors};
