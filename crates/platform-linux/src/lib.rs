//! Read-only Linux operating-system and runtime-environment discovery.
//!
//! This adapter reuses P007 domain types and keeps Linux source access behind
//! the bounded [`LinuxDataSource`] interface. It does not inspect hardware,
//! launch commands, write files, or require elevated privileges.

mod architecture;
mod environment;
mod errors;
mod kernel;
mod normalize;
mod os_release;
mod probe;
mod sources;

pub use environment::{
    EnvironmentClassification, EnvironmentConfidence, EnvironmentStatus,
    RuntimeDirectoryAssessment, RuntimeDirectoryStatus, RuntimeEnvironmentFacts,
};
pub use errors::{DataSourceError, DataSourceErrorKind, OsReleaseParseError};
pub use kernel::RawKernelIdentity;
pub use normalize::LinuxDistributionMetadata;
pub use os_release::{MAX_OS_RELEASE_BYTES, OsReleaseData, parse_os_release};
pub use probe::{
    LINUX_PLATFORM_REPORT_SCHEMA_VERSION, LinuxPlatformProbe, LinuxPlatformReport,
    LinuxReportCompleteness,
};
pub use sources::{
    ContainerMarker, LinuxDataSource, OsReleaseFile, RuntimeDirectoryMetadata,
    SystemLinuxDataSource,
};
