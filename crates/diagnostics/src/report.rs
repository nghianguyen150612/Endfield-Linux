use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::errors::io_error;
use crate::redact::{RedactionPolicy, ReportDiagnostic};
use crate::retention::LogReadResult;
use crate::{DiagnosticEvent, DiagnosticsError, IoFailureKind, LogStore, Severity};

pub const REPORT_SCHEMA_VERSION: u32 = 1;
pub const MAX_REPORT_BYTES: usize = 256 * 1024;
pub const MAX_REPORT_EVENTS: usize = 128;
const APPLICATION_NAME: &str = "endfield-unix";
const APPLICATION_VERSION: &str = env!("CARGO_PKG_VERSION");
static EXPORT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildStatus {
    Development,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentId {
    Application,
    Cli,
    Gui,
    Diagnostics,
    Storage,
    DomainModels,
    Optimizer,
    HardwareProbes,
    Telemetry,
    Daemon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentAvailability {
    Available,
    Partial,
    NotImplemented,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ComponentStatus {
    pub component: ComponentId,
    pub status: ComponentAvailability,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogHistoryStatus {
    Available,
    Missing,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct EventCounts {
    pub error: u64,
    pub warn: u64,
    pub info: u64,
    pub debug: u64,
    pub trace: u64,
}

impl EventCounts {
    fn count(events: &[DiagnosticEvent]) -> Self {
        let mut counts = Self::default();
        for event in events {
            let count = match event.severity() {
                Severity::Error => &mut counts.error,
                Severity::Warn => &mut counts.warn,
                Severity::Info => &mut counts.info,
                Severity::Debug => &mut counts.debug,
                Severity::Trace => &mut counts.trace,
            };
            *count = count.saturating_add(1);
        }
        counts
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct LogHistorySummary {
    pub status: LogHistoryStatus,
    pub omitted_records: u64,
    pub event_counts: EventCounts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ApplicationMetadata {
    pub name: &'static str,
    pub version: &'static str,
    pub build_status: BuildStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SupportReport {
    pub schema_version: u32,
    pub generated_at_unix_nanos: u64,
    pub application: ApplicationMetadata,
    pub components: Vec<ComponentStatus>,
    pub log_history: LogHistorySummary,
    pub diagnostics: Vec<ReportDiagnostic>,
}

impl SupportReport {
    pub fn from_events_at(
        events: Vec<DiagnosticEvent>,
        generated_at_unix_nanos: u64,
        history_status: LogHistoryStatus,
        omitted_records: u64,
    ) -> Self {
        let mut events = events;
        events.sort_by(|left, right| {
            left.timestamp_unix_nanos()
                .cmp(&right.timestamp_unix_nanos())
                .then_with(|| left.cmp(right))
        });
        let event_counts = EventCounts::count(&events);
        let diagnostics = events
            .iter()
            .rev()
            .take(MAX_REPORT_EVENTS)
            .rev()
            .map(|event| RedactionPolicy.redact_event(event))
            .collect();

        Self {
            schema_version: REPORT_SCHEMA_VERSION,
            generated_at_unix_nanos,
            application: ApplicationMetadata {
                name: APPLICATION_NAME,
                version: APPLICATION_VERSION,
                build_status: if cfg!(debug_assertions) {
                    BuildStatus::Development
                } else {
                    BuildStatus::Release
                },
            },
            components: component_statuses(),
            log_history: LogHistorySummary {
                status: history_status,
                omitted_records,
                event_counts,
            },
            diagnostics,
        }
    }

    pub fn to_json(&self) -> Result<Vec<u8>, DiagnosticsError> {
        let output =
            serde_json::to_vec_pretty(self).map_err(|_| DiagnosticsError::Serialization)?;
        if output.len() > MAX_REPORT_BYTES {
            return Err(DiagnosticsError::ReportTooLarge);
        }
        Ok(output)
    }
}

/// Builds a report using only static application metadata and validated safe
/// log records already present on disk. This does not initialize logging,
/// create directories, read configuration, or run a probe.
pub fn generate_support_report() -> SupportReport {
    let (events, history_status, omitted_records) =
        match LogStore::discover_existing_from_environment() {
            Ok(None)
            | Err(DiagnosticsError::HomeUnavailable | DiagnosticsError::UnsupportedPlatform) => {
                (Vec::new(), LogHistoryStatus::Missing, 0)
            }
            Err(_) => (Vec::new(), LogHistoryStatus::Unavailable, 0),
            Ok(Some(store)) => match store.read_events() {
                Ok(LogReadResult {
                    events,
                    omitted_records,
                }) => (events, LogHistoryStatus::Available, omitted_records),
                Err(_) => (Vec::new(), LogHistoryStatus::Unavailable, 0),
            },
        };
    SupportReport::from_events_at(
        events,
        current_time_unix_nanos(),
        history_status,
        omitted_records,
    )
}

pub fn export_support_report(
    destination: impl AsRef<Path>,
    overwrite: bool,
) -> Result<(), DiagnosticsError> {
    let report = generate_support_report();
    write_report_to_path(&report, destination, overwrite)
}

pub fn write_report_to_path(
    report: &SupportReport,
    destination: impl AsRef<Path>,
    overwrite: bool,
) -> Result<(), DiagnosticsError> {
    write_report_to_path_before_publish(report, destination.as_ref(), overwrite, || Ok(()))
}

fn write_report_to_path_before_publish<F>(
    report: &SupportReport,
    destination: &Path,
    overwrite: bool,
    before_publish: F,
) -> Result<(), DiagnosticsError>
where
    F: FnOnce() -> std::io::Result<()>,
{
    let file_name = destination
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or(DiagnosticsError::InvalidPath(
            "output path has no file name",
        ))?;
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let parent_metadata =
        fs::metadata(parent).map_err(|error| io_error("inspect report directory", &error))?;
    if !parent_metadata.is_dir() {
        return Err(DiagnosticsError::InvalidPath(
            "output parent is not a directory",
        ));
    }
    check_destination(destination, overwrite)?;
    let bytes = report.to_json()?;
    let (temporary_path, mut temporary_file) = create_temporary_file(parent, file_name)?;
    let mut temporary = TemporaryFile::new(temporary_path.clone());
    temporary_file
        .write_all(&bytes)
        .map_err(|error| io_error("write support report", &error))?;
    temporary_file
        .flush()
        .map_err(|error| io_error("flush support report", &error))?;
    temporary_file
        .sync_all()
        .map_err(|error| io_error("synchronize support report", &error))?;
    drop(temporary_file);
    before_publish().map_err(|error| io_error("prepare support report publication", &error))?;

    if overwrite {
        check_destination(destination, true)?;
        fs::rename(&temporary_path, destination)
            .map_err(|error| io_error("publish support report", &error))?;
    } else {
        match fs::hard_link(&temporary_path, destination) {
            Ok(()) => {
                fs::remove_file(&temporary_path)
                    .map_err(|error| io_error("remove temporary support report", &error))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(if destination_is_symlink(destination) {
                    DiagnosticsError::UnsafePath("refused a symlink report destination")
                } else {
                    DiagnosticsError::OutputExists
                });
            }
            Err(error) => {
                return Err(io_error(
                    "publish support report without replacement",
                    &error,
                ));
            }
        }
    }
    temporary.commit();
    Ok(())
}

fn component_statuses() -> Vec<ComponentStatus> {
    use ComponentAvailability::{Available as Yes, NotImplemented as No, Partial};
    use ComponentId as Id;
    vec![
        ComponentStatus {
            component: Id::Application,
            status: Yes,
        },
        ComponentStatus {
            component: Id::Cli,
            status: Yes,
        },
        ComponentStatus {
            component: Id::Gui,
            status: Yes,
        },
        ComponentStatus {
            component: Id::Diagnostics,
            status: Yes,
        },
        ComponentStatus {
            component: Id::Storage,
            status: Yes,
        },
        ComponentStatus {
            component: Id::DomainModels,
            status: Yes,
        },
        ComponentStatus {
            component: Id::Optimizer,
            status: Partial,
        },
        ComponentStatus {
            component: Id::HardwareProbes,
            status: No,
        },
        ComponentStatus {
            component: Id::Telemetry,
            status: No,
        },
        ComponentStatus {
            component: Id::Daemon,
            status: Partial,
        },
    ]
}

fn check_destination(destination: &Path, overwrite: bool) -> Result<(), DiagnosticsError> {
    match fs::symlink_metadata(destination) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("inspect report destination", &error)),
        Ok(metadata) if metadata.file_type().is_symlink() => Err(DiagnosticsError::UnsafePath(
            "refused a symlink report destination",
        )),
        Ok(metadata) if !metadata.is_file() => Err(DiagnosticsError::UnsafePath(
            "report destination must be a regular file",
        )),
        Ok(_) if !overwrite => Err(DiagnosticsError::OutputExists),
        Ok(_) => Ok(()),
    }
}

fn destination_is_symlink(destination: &Path) -> bool {
    fs::symlink_metadata(destination).is_ok_and(|metadata| metadata.file_type().is_symlink())
}

fn create_temporary_file(
    parent: &Path,
    _destination_name: &std::ffi::OsStr,
) -> Result<(PathBuf, File), DiagnosticsError> {
    for _ in 0..64 {
        let sequence = EXPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".endfield-support-report-{}-{sequence}.tmp",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        match options.open(&path) {
            Ok(file) => {
                if let Err(error) = restrict_permissions(&file) {
                    let _ = fs::remove_file(&path);
                    return Err(error);
                }
                return Ok((path, file));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error("create temporary support report", &error)),
        }
    }
    Err(DiagnosticsError::Io {
        operation: "allocate temporary report name",
        kind: IoFailureKind::AlreadyExists,
    })
}

fn restrict_permissions(file: &File) -> Result<(), DiagnosticsError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| io_error("restrict report permissions", &error))?;
    }
    Ok(())
}

struct TemporaryFile {
    path: PathBuf,
    committed: bool,
}

impl TemporaryFile {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn current_time_unix_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, EventCode, LogHistoryStatus, Severity};
    use tempfile::tempdir;

    #[test]
    fn interrupted_export_removes_the_incomplete_temporary_file() {
        let directory = tempdir().expect("temporary directory");
        let destination = directory.path().join("support-report.json");
        let report = SupportReport::from_events_at(
            vec![DiagnosticEvent::new_at(
                Severity::Info,
                Component::Application,
                EventCode::AppStarting,
                1,
            )],
            2,
            LogHistoryStatus::Missing,
            0,
        );
        let result = write_report_to_path_before_publish(&report, &destination, false, || {
            Err(std::io::Error::other("injected interruption"))
        });
        assert!(result.is_err());
        assert!(!destination.exists());
        let leftovers = fs::read_dir(directory.path())
            .expect("output directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".endfield-support-report-")
            })
            .count();
        assert_eq!(leftovers, 0);
    }
}
