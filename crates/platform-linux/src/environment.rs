use crate::errors::{DataSourceError, DataSourceErrorKind};
use crate::kernel::RawKernelIdentity;
use crate::sources::{ContainerMarker, LinuxDataSource, RuntimeDirectoryMetadata};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentClassification {
    NativeLinux,
    Container,
    Wsl,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentConfidence {
    High,
    Moderate,
    Low,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentStatus {
    Identified,
    Uncertain,
}

/// Evidence-based classification. `NativeLinux` means no recognized
/// container/WSL indicator was found and is deliberately low confidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeEnvironmentFacts {
    pub classification: EnvironmentClassification,
    pub status: EnvironmentStatus,
    pub confidence: EnvironmentConfidence,
    /// Stable source identifiers only; no source contents or IDs are retained.
    pub source_ids: Vec<&'static str>,
    pub diagnostic: Option<&'static str>,
    pub uncertainty: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeDirectoryStatus {
    Available,
    Missing,
    InvalidOrUnsafe,
    PermissionDenied,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDirectoryAssessment {
    pub status: RuntimeDirectoryStatus,
    pub source_id: &'static str,
    pub diagnostic: Option<&'static str>,
}

pub(crate) fn assess_runtime_environment<S: LinuxDataSource + ?Sized>(
    source: &S,
    kernel: &RawKernelIdentity,
) -> RuntimeEnvironmentFacts {
    let wsl = is_wsl_release(&kernel.release);
    let mut source_ids = if wsl { vec!["linux-uname"] } else { Vec::new() };
    let mut container_found = false;
    let mut failures = 0usize;
    for marker in [
        ContainerMarker::Docker,
        ContainerMarker::Podman,
        ContainerMarker::Systemd,
    ] {
        match source.container_marker_present(marker) {
            Ok(true) => {
                container_found = true;
                source_ids.push(marker.source_id());
            }
            Ok(false) => {}
            Err(_) => failures += 1,
        }
    }
    if source_ids.is_empty() && failures != 0 {
        source_ids.push("linux-environment");
    }
    if source_ids.is_empty() && !wsl && !container_found {
        source_ids.push("linux-environment");
    }

    if wsl && container_found {
        return RuntimeEnvironmentFacts {
            classification: EnvironmentClassification::Unknown,
            status: EnvironmentStatus::Uncertain,
            confidence: EnvironmentConfidence::Low,
            source_ids,
            diagnostic: Some("conflicting WSL and container indicators were observed"),
            uncertainty: Some("the execution environment could not be classified safely"),
        };
    }
    if wsl {
        return RuntimeEnvironmentFacts {
            classification: EnvironmentClassification::Wsl,
            status: if failures == 0 {
                EnvironmentStatus::Identified
            } else {
                EnvironmentStatus::Uncertain
            },
            confidence: if failures == 0 {
                EnvironmentConfidence::High
            } else {
                EnvironmentConfidence::Moderate
            },
            source_ids,
            diagnostic: (failures != 0)
                .then_some("one or more container indicators could not be checked"),
            uncertainty: Some("WSL generation and nested-container state are not inferred"),
        };
    }
    if container_found {
        return RuntimeEnvironmentFacts {
            classification: EnvironmentClassification::Container,
            status: if failures == 0 {
                EnvironmentStatus::Identified
            } else {
                EnvironmentStatus::Uncertain
            },
            confidence: if failures == 0 {
                EnvironmentConfidence::High
            } else {
                EnvironmentConfidence::Moderate
            },
            source_ids,
            diagnostic: (failures != 0)
                .then_some("one or more environment indicators could not be checked"),
            uncertainty: Some("container type and host virtualization are not inferred"),
        };
    }
    if failures != 0 {
        return RuntimeEnvironmentFacts {
            classification: EnvironmentClassification::Unknown,
            status: EnvironmentStatus::Uncertain,
            confidence: EnvironmentConfidence::Low,
            source_ids,
            diagnostic: Some("environment indicators could not all be checked"),
            uncertainty: Some("absence of a readable marker is not evidence of a bare-metal host"),
        };
    }

    RuntimeEnvironmentFacts {
        classification: EnvironmentClassification::NativeLinux,
        status: EnvironmentStatus::Uncertain,
        confidence: EnvironmentConfidence::Low,
        source_ids,
        diagnostic: None,
        uncertainty: Some(
            "no recognized container or WSL marker was found; bare-metal status is not established",
        ),
    }
}

pub(crate) fn assess_runtime_directory<S: LinuxDataSource + ?Sized>(
    source: &S,
) -> RuntimeDirectoryAssessment {
    let path = match source.runtime_directory_path() {
        Ok(Some(path)) => path,
        Ok(None) => {
            return RuntimeDirectoryAssessment {
                status: RuntimeDirectoryStatus::Missing,
                source_id: "linux-xdg-runtime-dir",
                diagnostic: Some("XDG_RUNTIME_DIR is not set"),
            };
        }
        Err(error) => return assessment_from_error(error),
    };
    if !path.is_absolute() {
        return RuntimeDirectoryAssessment {
            status: RuntimeDirectoryStatus::InvalidOrUnsafe,
            source_id: "linux-xdg-runtime-dir",
            diagnostic: Some("XDG_RUNTIME_DIR must be absolute"),
        };
    }
    let socket_path = path.join("endfield-unix").join("endfield-unixd.sock");
    if socket_path.as_os_str().as_encoded_bytes().len() + 1 > 108 {
        return RuntimeDirectoryAssessment {
            status: RuntimeDirectoryStatus::InvalidOrUnsafe,
            source_id: "linux-xdg-runtime-dir",
            diagnostic: Some("XDG_RUNTIME_DIR leaves no room for the P010 IPC socket path"),
        };
    }
    let metadata = match source.runtime_directory_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => return assessment_from_error(error),
    };
    if !is_runtime_directory_safe(metadata, source.effective_uid()) {
        return RuntimeDirectoryAssessment {
            status: RuntimeDirectoryStatus::InvalidOrUnsafe,
            source_id: "linux-xdg-runtime-dir",
            diagnostic: Some("XDG_RUNTIME_DIR does not satisfy the P010 directory policy"),
        };
    }
    RuntimeDirectoryAssessment {
        status: RuntimeDirectoryStatus::Available,
        source_id: "linux-xdg-runtime-dir",
        diagnostic: None,
    }
}

pub(crate) fn is_runtime_directory_safe(
    metadata: RuntimeDirectoryMetadata,
    effective_uid: u32,
) -> bool {
    metadata.is_directory
        && !metadata.is_symlink
        && metadata.owner_uid == effective_uid
        && metadata.mode & 0o077 == 0
}

fn assessment_from_error(error: DataSourceError) -> RuntimeDirectoryAssessment {
    let (status, diagnostic) = match error.kind {
        DataSourceErrorKind::NotFound => (
            RuntimeDirectoryStatus::Missing,
            Some("XDG_RUNTIME_DIR does not exist"),
        ),
        DataSourceErrorKind::PermissionDenied => (
            RuntimeDirectoryStatus::PermissionDenied,
            Some("XDG_RUNTIME_DIR metadata could not be accessed"),
        ),
        DataSourceErrorKind::Unsupported => (
            RuntimeDirectoryStatus::Unknown,
            Some("runtime-directory inspection is unsupported on this platform"),
        ),
        _ => (
            RuntimeDirectoryStatus::Unknown,
            Some("runtime-directory metadata could not be verified"),
        ),
    };
    RuntimeDirectoryAssessment {
        status,
        source_id: "linux-xdg-runtime-dir",
        diagnostic,
    }
}

fn is_wsl_release(release: &str) -> bool {
    let release = release.to_ascii_lowercase();
    release.contains("microsoft") || release.contains("wsl")
}
