use endfield_unix_domain::{
    Diagnostic, DiagnosticCategory, DistributionIdentity, KernelIdentity, OsIdentity,
    PlatformFamily, PlatformIdentity, PlatformProbe, ProbeError, ProbeErrorKind, Provenance,
    ProvenanceId, SystemSnapshot,
};

use crate::architecture::runtime_architecture;
use crate::environment::{
    EnvironmentStatus, RuntimeDirectoryAssessment, RuntimeDirectoryStatus, RuntimeEnvironmentFacts,
    assess_runtime_directory, assess_runtime_environment,
};
use crate::errors::{DataSourceError, DataSourceErrorKind, OsReleaseParseError};
use crate::kernel::RawKernelIdentity;
use crate::normalize::{DistributionNormalizeError, LinuxDistributionMetadata};
use crate::os_release::parse_os_release;
use crate::sources::{LinuxDataSource, OsReleaseFile, SystemLinuxDataSource};

pub const LINUX_PLATFORM_REPORT_SCHEMA_VERSION: u32 = 1;

/// Linux-specific extension around the portable P007 snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxPlatformReport {
    pub schema_version: u32,
    pub snapshot: SystemSnapshot,
    pub distribution: Option<LinuxDistributionMetadata>,
    pub runtime_environment: RuntimeEnvironmentFacts,
    pub user_runtime_directory: RuntimeDirectoryAssessment,
    pub completeness: LinuxReportCompleteness,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxReportCompleteness {
    Complete,
    Partial,
}

pub struct LinuxPlatformProbe<S = SystemLinuxDataSource> {
    source: S,
}

impl LinuxPlatformProbe<SystemLinuxDataSource> {
    pub fn system() -> Self {
        Self {
            source: SystemLinuxDataSource::new(),
        }
    }
}

impl<S> LinuxPlatformProbe<S> {
    pub fn with_source(source: S) -> Self {
        Self { source }
    }

    pub fn source(&self) -> &S {
        &self.source
    }
}

impl<S: LinuxDataSource> LinuxPlatformProbe<S> {
    /// Collects a typed report. Kernel identity is required; distribution and
    /// session observations degrade independently into a valid partial report.
    pub fn inspect(&self) -> Result<LinuxPlatformReport, ProbeError> {
        let raw_kernel = self.source.kernel_identity().map_err(|error| {
            let message = if error.kind == DataSourceErrorKind::Unsupported {
                "Linux platform probing is unsupported on this operating system"
            } else {
                "running-kernel identity could not be established"
            };
            probe_error("linux-uname", probe_error_kind(error.kind), message)
        })?;
        let kernel = normalize_kernel(&raw_kernel)?;
        let architecture = runtime_architecture(&raw_kernel.machine).map_err(|_| {
            probe_error(
                "linux-uname",
                ProbeErrorKind::MalformedData,
                "kernel-reported architecture was invalid",
            )
        })?;

        let (distribution, distribution_identity, os_release_error) =
            match load_distribution(&self.source) {
                Ok((metadata, identity)) => (Some(metadata), Some(identity), None),
                Err(error) => (None, None, Some(error)),
            };

        let platform = PlatformIdentity::new(
            PlatformFamily::Linux,
            OsIdentity::new("Linux", None).map_err(|_| {
                probe_error(
                    "linux-uname",
                    ProbeErrorKind::MalformedData,
                    "Linux operating-system identity was invalid",
                )
            })?,
            architecture,
            Some(kernel),
            distribution_identity,
        )
        .map_err(|_| {
            probe_error(
                "linux-uname",
                ProbeErrorKind::MalformedData,
                "Linux platform identity failed validation",
            )
        })?;

        let mut snapshot = SystemSnapshot::new(platform);
        snapshot.provenance.push(
            Provenance::new(
                ProvenanceId::new(1).map_err(|_| {
                    probe_error(
                        "linux-uname",
                        ProbeErrorKind::MalformedData,
                        "kernel provenance could not be represented",
                    )
                })?,
                "linux-uname",
                self.source.observation_time_unix_nanos(),
            )
            .map_err(|_| {
                probe_error(
                    "linux-uname",
                    ProbeErrorKind::MalformedData,
                    "kernel provenance could not be represented",
                )
            })?,
        );
        if distribution.is_some() {
            snapshot.provenance.push(
                Provenance::new(
                    ProvenanceId::new(2).map_err(|_| {
                        probe_error(
                            "linux-os-release",
                            ProbeErrorKind::MalformedData,
                            "distribution provenance could not be represented",
                        )
                    })?,
                    "linux-os-release",
                    self.source.observation_time_unix_nanos(),
                )
                .map_err(|_| {
                    probe_error(
                        "linux-os-release",
                        ProbeErrorKind::MalformedData,
                        "distribution provenance could not be represented",
                    )
                })?,
            );
        }
        if let Some(error) = os_release_error {
            snapshot.diagnostics.push(
                Diagnostic::new(
                    diagnostic_category(error.kind),
                    Some("linux-os-release".to_owned()),
                    Some(error.message.to_owned()),
                )
                .map_err(|_| {
                    probe_error(
                        "linux-os-release",
                        ProbeErrorKind::MalformedData,
                        "distribution diagnostic could not be represented",
                    )
                })?,
            );
        }

        let runtime_environment = assess_runtime_environment(&self.source, &raw_kernel);
        let user_runtime_directory = assess_runtime_directory(&self.source);
        add_environment_diagnostics(&mut snapshot, &runtime_environment, &user_runtime_directory)?;
        snapshot = snapshot.normalize().map_err(|_| {
            probe_error(
                "linux-platform",
                ProbeErrorKind::MalformedData,
                "normalized platform snapshot failed validation",
            )
        })?;

        let is_partial = distribution.is_none()
            || runtime_environment.status == EnvironmentStatus::Uncertain
            || user_runtime_directory.status != RuntimeDirectoryStatus::Available;
        Ok(LinuxPlatformReport {
            schema_version: LINUX_PLATFORM_REPORT_SCHEMA_VERSION,
            snapshot,
            distribution,
            runtime_environment,
            user_runtime_directory,
            completeness: if is_partial {
                LinuxReportCompleteness::Partial
            } else {
                LinuxReportCompleteness::Complete
            },
        })
    }
}

impl<S: LinuxDataSource> PlatformProbe for LinuxPlatformProbe<S> {
    fn probe(&self) -> Result<SystemSnapshot, ProbeError> {
        self.inspect().map(|report| report.snapshot)
    }
}

struct DistributionLoadError {
    kind: ProbeErrorKind,
    message: &'static str,
}

fn load_distribution<S: LinuxDataSource>(
    source: &S,
) -> Result<(LinuxDistributionMetadata, DistributionIdentity), DistributionLoadError> {
    let (bytes, source_id) = match source.read_os_release(OsReleaseFile::Etc) {
        Ok(bytes) => (bytes, "etc-os-release"),
        Err(error) if error.kind == DataSourceErrorKind::NotFound => source
            .read_os_release(OsReleaseFile::UsrLib)
            .map(|bytes| (bytes, "usr-lib-os-release"))
            .map_err(source_distribution_error)?,
        Err(error) => return Err(source_distribution_error(error)),
    };
    let parsed = parse_os_release(&bytes).map_err(parse_distribution_error)?;
    let metadata = LinuxDistributionMetadata::from_os_release(parsed, source_id)
        .map_err(normalize_distribution_error)?;
    let identity = metadata
        .distribution_identity()
        .map_err(|_| DistributionLoadError {
            kind: ProbeErrorKind::MalformedData,
            message: "selected distribution fields failed domain validation",
        })?;
    Ok((metadata, identity))
}

fn normalize_kernel(raw: &RawKernelIdentity) -> Result<KernelIdentity, ProbeError> {
    if raw.system_name != "Linux"
        || !valid_kernel_text(&raw.release, 128, true)
        || !valid_kernel_text(&raw.machine, 64, false)
    {
        return Err(probe_error(
            "linux-uname",
            ProbeErrorKind::MalformedData,
            "kernel identity contained invalid fields",
        ));
    }
    KernelIdentity::new(raw.system_name.clone(), Some(raw.release.clone())).map_err(|_| {
        probe_error(
            "linux-uname",
            ProbeErrorKind::MalformedData,
            "kernel identity failed domain validation",
        )
    })
}

fn valid_kernel_text(value: &str, max_bytes: usize, allow_whitespace: bool) -> bool {
    !value.trim().is_empty()
        && value == value.trim()
        && value.len() <= max_bytes
        && (allow_whitespace || !value.chars().any(char::is_whitespace))
        && value.chars().all(|character| !character.is_control())
}

fn source_distribution_error(error: DataSourceError) -> DistributionLoadError {
    let kind = probe_error_kind(error.kind);
    let message = match error.kind {
        DataSourceErrorKind::NotFound => "os-release metadata was not found",
        DataSourceErrorKind::PermissionDenied => "os-release metadata access was denied",
        DataSourceErrorKind::InvalidUtf8 => "os-release metadata was not valid UTF-8",
        DataSourceErrorKind::MalformedData => "os-release metadata was malformed",
        DataSourceErrorKind::TooLarge => "os-release metadata exceeded its size limit",
        DataSourceErrorKind::UnexpectedType => "os-release metadata had an unsafe file type",
        DataSourceErrorKind::Unsupported => "os-release metadata is unsupported on this platform",
        DataSourceErrorKind::Other => "os-release metadata could not be read",
    };
    DistributionLoadError { kind, message }
}

fn parse_distribution_error(_error: OsReleaseParseError) -> DistributionLoadError {
    DistributionLoadError {
        kind: ProbeErrorKind::MalformedData,
        message: "os-release metadata did not satisfy the parser contract",
    }
}

fn normalize_distribution_error(error: DistributionNormalizeError) -> DistributionLoadError {
    let message = match error {
        DistributionNormalizeError::MissingId => "os-release metadata did not provide a valid ID",
        DistributionNormalizeError::InvalidDomainValue => {
            "selected distribution fields failed domain validation"
        }
    };
    DistributionLoadError {
        kind: ProbeErrorKind::MalformedData,
        message,
    }
}

fn diagnostic_category(kind: ProbeErrorKind) -> DiagnosticCategory {
    match kind {
        ProbeErrorKind::Unsupported => DiagnosticCategory::UnsupportedCapability,
        ProbeErrorKind::MissingProvider => DiagnosticCategory::MissingProvider,
        ProbeErrorKind::PermissionDenied => DiagnosticCategory::PermissionDenied,
        ProbeErrorKind::MalformedData => DiagnosticCategory::MalformedSourceData,
        _ => DiagnosticCategory::TemporaryFailure,
    }
}

fn probe_error_kind(kind: DataSourceErrorKind) -> ProbeErrorKind {
    match kind {
        DataSourceErrorKind::PermissionDenied => ProbeErrorKind::PermissionDenied,
        DataSourceErrorKind::InvalidUtf8
        | DataSourceErrorKind::MalformedData
        | DataSourceErrorKind::TooLarge
        | DataSourceErrorKind::UnexpectedType => ProbeErrorKind::MalformedData,
        DataSourceErrorKind::Unsupported => ProbeErrorKind::Unsupported,
        DataSourceErrorKind::NotFound => ProbeErrorKind::MissingProvider,
        DataSourceErrorKind::Other => ProbeErrorKind::TemporaryFailure,
    }
}

fn add_environment_diagnostics(
    snapshot: &mut SystemSnapshot,
    environment: &RuntimeEnvironmentFacts,
    runtime_directory: &RuntimeDirectoryAssessment,
) -> Result<(), ProbeError> {
    if let Some(message) = environment.diagnostic.or(environment.uncertainty) {
        snapshot.diagnostics.push(
            Diagnostic::new(
                if environment.status == EnvironmentStatus::Uncertain {
                    DiagnosticCategory::NotYetProbed
                } else {
                    DiagnosticCategory::UnsupportedCapability
                },
                Some("linux-environment".to_owned()),
                Some(message.to_owned()),
            )
            .map_err(|_| {
                probe_error(
                    "linux-environment",
                    ProbeErrorKind::MalformedData,
                    "environment diagnostic could not be represented",
                )
            })?,
        );
    }
    if let Some(message) = runtime_directory.diagnostic {
        let category = match runtime_directory.status {
            RuntimeDirectoryStatus::PermissionDenied => DiagnosticCategory::PermissionDenied,
            RuntimeDirectoryStatus::InvalidOrUnsafe => DiagnosticCategory::MalformedSourceData,
            RuntimeDirectoryStatus::Unknown => DiagnosticCategory::TemporaryFailure,
            RuntimeDirectoryStatus::Missing => DiagnosticCategory::MissingProvider,
            RuntimeDirectoryStatus::Available => DiagnosticCategory::NotYetProbed,
        };
        snapshot.diagnostics.push(
            Diagnostic::new(
                category,
                Some("linux-environment".to_owned()),
                Some(message.to_owned()),
            )
            .map_err(|_| {
                probe_error(
                    "linux-environment",
                    ProbeErrorKind::MalformedData,
                    "runtime-directory diagnostic could not be represented",
                )
            })?,
        );
    }
    Ok(())
}

fn probe_error(provider: &str, kind: ProbeErrorKind, message: &str) -> ProbeError {
    ProbeError::new(provider, kind, Some(message.to_owned()))
        .unwrap_or_else(|_| ProbeError::new("linux-platform", kind, None).expect("static provider"))
}
