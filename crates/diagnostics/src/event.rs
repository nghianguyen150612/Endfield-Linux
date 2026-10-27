use std::time::{SystemTime, UNIX_EPOCH};

use endfield_unix_domain::{CapabilityId, DiagnosticCategory};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::DiagnosticsError;

pub const EVENT_SCHEMA_VERSION: u16 = 1;
pub const MAX_CORRELATION_ID_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Severity {
    const fn rank(self) -> u8 {
        match self {
            Self::Error => 0,
            Self::Warn => 1,
            Self::Info => 2,
            Self::Debug => 3,
            Self::Trace => 4,
        }
    }

    pub const fn is_at_least(self, minimum: Self) -> bool {
        self.rank() <= minimum.rank()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    Application,
    Gui,
    Cli,
    Diagnostics,
    PlatformProbe,
    Storage,
    Runtime,
    Daemon,
    Benchmark,
    Optimization,
}

impl Component {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::Gui => "gui",
            Self::Cli => "cli",
            Self::Diagnostics => "diagnostics",
            Self::PlatformProbe => "platform_probe",
            Self::Storage => "storage",
            Self::Runtime => "runtime",
            Self::Daemon => "daemon",
            Self::Benchmark => "benchmark",
            Self::Optimization => "optimization",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventCode {
    AppStarting,
    GuiInitialized,
    GuiStartupFailed,
    WindowClosedNormally,
    DiagnosticsUnavailable,
    DomainDiagnostic,
    ProbeFailure,
    ProbeSucceeded,
    OperationNotImplemented,
    DaemonStarting,
    DaemonRunning,
    DaemonShutdownRequested,
    DaemonStopped,
    DaemonStartupFailed,
    DaemonRuntimeFailed,
    SessionRegistered,
    SessionStarted,
    SessionExited,
    SessionMonitorFailed,
    SessionSupervisorStopped,
}

impl EventCode {
    pub const fn is_session_event(self) -> bool {
        matches!(
            self,
            Self::SessionRegistered
                | Self::SessionStarted
                | Self::SessionExited
                | Self::SessionMonitorFailed
                | Self::SessionSupervisorStopped
        )
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AppStarting => "APP_STARTING",
            Self::GuiInitialized => "GUI_INITIALIZED",
            Self::GuiStartupFailed => "GUI_STARTUP_FAILED",
            Self::WindowClosedNormally => "WINDOW_CLOSED_NORMALLY",
            Self::DiagnosticsUnavailable => "DIAGNOSTICS_UNAVAILABLE",
            Self::DomainDiagnostic => "DOMAIN_DIAGNOSTIC",
            Self::ProbeFailure => "PROBE_FAILURE",
            Self::ProbeSucceeded => "PROBE_SUCCEEDED",
            Self::OperationNotImplemented => "OPERATION_NOT_IMPLEMENTED",
            Self::DaemonStarting => "DAEMON_STARTING",
            Self::DaemonRunning => "DAEMON_RUNNING",
            Self::DaemonShutdownRequested => "DAEMON_SHUTDOWN_REQUESTED",
            Self::DaemonStopped => "DAEMON_STOPPED",
            Self::DaemonStartupFailed => "DAEMON_STARTUP_FAILED",
            Self::DaemonRuntimeFailed => "DAEMON_RUNTIME_FAILED",
            Self::SessionRegistered => "SESSION_REGISTERED",
            Self::SessionStarted => "SESSION_STARTED",
            Self::SessionExited => "SESSION_EXITED",
            Self::SessionMonitorFailed => "SESSION_MONITOR_FAILED",
            Self::SessionSupervisorStopped => "SESSION_SUPERVISOR_STOPPED",
        }
    }

    pub const fn safe_message(self) -> &'static str {
        match self {
            Self::AppStarting => "Application is starting.",
            Self::GuiInitialized => "The graphical application initialized.",
            Self::GuiStartupFailed => "The graphical application failed to start.",
            Self::WindowClosedNormally => "The application window closed normally.",
            Self::DiagnosticsUnavailable => "Persistent diagnostics are unavailable.",
            Self::DomainDiagnostic => "A structured domain diagnostic was recorded.",
            Self::ProbeFailure => "A platform probe operation failed or was unavailable.",
            Self::ProbeSucceeded => "A platform probe operation completed successfully.",
            Self::OperationNotImplemented => "This operation is not implemented in this build.",
            Self::DaemonStarting => "The daemon is starting.",
            Self::DaemonRunning => "The daemon entered the running state.",
            Self::DaemonShutdownRequested => "The daemon received a shutdown request.",
            Self::DaemonStopped => "The daemon stopped cleanly.",
            Self::DaemonStartupFailed => "The daemon failed during startup.",
            Self::DaemonRuntimeFailed => "The daemon encountered a runtime failure.",
            Self::SessionRegistered => "A trusted child process was registered for supervision.",
            Self::SessionStarted => "A supervised process entered the running state.",
            Self::SessionExited => "A supervised process exit was observed.",
            Self::SessionMonitorFailed => "Monitoring of a supervised process failed.",
            Self::SessionSupervisorStopped => "Session process monitoring stopped.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationOutcome {
    Succeeded,
    Unavailable,
    Failed,
    Incomplete,
    NotImplemented,
    NotAttempted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeDiagnosticCategory {
    UnsupportedCapability,
    MissingProvider,
    PermissionDenied,
    TemporaryFailure,
    MalformedSourceData,
    StaleObservation,
    NotYetProbed,
}

impl SafeDiagnosticCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedCapability => "unsupported_capability",
            Self::MissingProvider => "missing_provider",
            Self::PermissionDenied => "permission_denied",
            Self::TemporaryFailure => "temporary_failure",
            Self::MalformedSourceData => "malformed_source_data",
            Self::StaleObservation => "stale_observation",
            Self::NotYetProbed => "not_yet_probed",
        }
    }
}

impl From<DiagnosticCategory> for SafeDiagnosticCategory {
    fn from(value: DiagnosticCategory) -> Self {
        match value {
            DiagnosticCategory::UnsupportedCapability => Self::UnsupportedCapability,
            DiagnosticCategory::MissingProvider => Self::MissingProvider,
            DiagnosticCategory::PermissionDenied => Self::PermissionDenied,
            DiagnosticCategory::TemporaryFailure => Self::TemporaryFailure,
            DiagnosticCategory::MalformedSourceData => Self::MalformedSourceData,
            DiagnosticCategory::StaleObservation => Self::StaleObservation,
            DiagnosticCategory::NotYetProbed => Self::NotYetProbed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeProvider {
    LinuxProcfs,
    LinuxSysfs,
    LinuxUname,
    LinuxOsRelease,
    LinuxEnvironment,
    LinuxXdgRuntimeDir,
    LinuxCpuSysfs,
    LinuxCpuinfo,
    LinuxMeminfo,
    LinuxCgroup,
    LinuxCpufreq,
    LinuxScheduler,
    LinuxPower,
    Drm,
    VulkanLoader,
    Wayland,
    X11,
    SystemdUser,
    Wine,
    Proton,
}

impl SafeProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LinuxProcfs => "linux-procfs",
            Self::LinuxSysfs => "linux-sysfs",
            Self::LinuxUname => "linux-uname",
            Self::LinuxOsRelease => "linux-os-release",
            Self::LinuxEnvironment => "linux-environment",
            Self::LinuxXdgRuntimeDir => "linux-xdg-runtime-dir",
            Self::LinuxCpuSysfs => "linux-cpu-sysfs",
            Self::LinuxCpuinfo => "linux-cpuinfo",
            Self::LinuxMeminfo => "linux-meminfo",
            Self::LinuxCgroup => "linux-cgroup",
            Self::LinuxCpufreq => "linux-cpufreq",
            Self::LinuxScheduler => "linux-scheduler",
            Self::LinuxPower => "linux-power",
            Self::Drm => "drm",
            Self::VulkanLoader => "vulkan-loader",
            Self::Wayland => "wayland",
            Self::X11 => "x11",
            Self::SystemdUser => "systemd-user",
            Self::Wine => "wine",
            Self::Proton => "proton",
        }
    }

    pub(crate) fn from_domain_name(value: &str) -> Option<Self> {
        match value {
            "linux-procfs" => Some(Self::LinuxProcfs),
            "linux-sysfs" => Some(Self::LinuxSysfs),
            "linux-uname" => Some(Self::LinuxUname),
            "linux-os-release" => Some(Self::LinuxOsRelease),
            "linux-environment" => Some(Self::LinuxEnvironment),
            "linux-xdg-runtime-dir" => Some(Self::LinuxXdgRuntimeDir),
            "linux-cpu-sysfs" => Some(Self::LinuxCpuSysfs),
            "linux-cpuinfo" => Some(Self::LinuxCpuinfo),
            "linux-meminfo" => Some(Self::LinuxMeminfo),
            "linux-cgroup" => Some(Self::LinuxCgroup),
            "linux-cpufreq" => Some(Self::LinuxCpufreq),
            "linux-scheduler" => Some(Self::LinuxScheduler),
            "linux-power" => Some(Self::LinuxPower),
            "drm" => Some(Self::Drm),
            "vulkan-loader" => Some(Self::VulkanLoader),
            "wayland" => Some(Self::Wayland),
            "x11" => Some(Self::X11),
            "systemd-user" => Some(Self::SystemdUser),
            "wine" => Some(Self::Wine),
            "proton" => Some(Self::Proton),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct DiagnosticEvent {
    schema_version: u16,
    timestamp_unix_nanos: u64,
    severity: Severity,
    component: Component,
    code: EventCode,
    message: &'static str,
    outcome: Option<OperationOutcome>,
    category: Option<SafeDiagnosticCategory>,
    capability_id: Option<u32>,
    provider: Option<SafeProvider>,
    correlation_id: Option<CorrelationId>,
}

impl DiagnosticEvent {
    pub fn new(severity: Severity, component: Component, code: EventCode) -> Self {
        Self::new_at(severity, component, code, current_time_unix_nanos())
    }

    pub fn new_at(
        severity: Severity,
        component: Component,
        code: EventCode,
        timestamp_unix_nanos: u64,
    ) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            timestamp_unix_nanos,
            severity,
            component,
            code,
            message: code.safe_message(),
            outcome: None,
            category: None,
            capability_id: None,
            provider: None,
            correlation_id: None,
        }
    }

    pub fn with_correlation_id(
        mut self,
        value: impl Into<String>,
    ) -> Result<Self, DiagnosticsError> {
        self.correlation_id = Some(CorrelationId::new(value.into())?);
        self.validate()?;
        Ok(self)
    }

    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    pub const fn timestamp_unix_nanos(&self) -> u64 {
        self.timestamp_unix_nanos
    }

    pub const fn severity(&self) -> Severity {
        self.severity
    }

    pub const fn component(&self) -> Component {
        self.component
    }

    pub const fn code(&self) -> EventCode {
        self.code
    }

    pub const fn message(&self) -> &'static str {
        self.message
    }

    pub const fn outcome(&self) -> Option<OperationOutcome> {
        self.outcome
    }

    pub const fn category(&self) -> Option<SafeDiagnosticCategory> {
        self.category
    }

    pub const fn capability_id(&self) -> Option<u32> {
        self.capability_id
    }

    pub const fn provider(&self) -> Option<SafeProvider> {
        self.provider
    }

    pub fn validate(&self) -> Result<(), DiagnosticsError> {
        if self.schema_version != EVENT_SCHEMA_VERSION {
            return Err(DiagnosticsError::InvalidEvent("unsupported event schema"));
        }
        if self.message != self.code.safe_message() {
            return Err(DiagnosticsError::InvalidEvent(
                "event message is not the approved static message",
            ));
        }
        if let Some(capability_id) = self.capability_id {
            CapabilityId::try_from(capability_id)
                .map_err(|_| DiagnosticsError::InvalidEvent("unknown capability identifier"))?;
        }
        let has_domain_fields =
            self.category.is_some() || self.capability_id.is_some() || self.provider.is_some();
        let operation_event = matches!(
            self.code,
            EventCode::DomainDiagnostic
                | EventCode::ProbeFailure
                | EventCode::ProbeSucceeded
                | EventCode::OperationNotImplemented
        );
        let daemon_lifecycle_event = matches!(
            self.code,
            EventCode::DaemonStarting
                | EventCode::DaemonRunning
                | EventCode::DaemonShutdownRequested
                | EventCode::DaemonStopped
                | EventCode::DaemonStartupFailed
                | EventCode::DaemonRuntimeFailed
                | EventCode::SessionRegistered
                | EventCode::SessionStarted
                | EventCode::SessionExited
                | EventCode::SessionMonitorFailed
                | EventCode::SessionSupervisorStopped
        );
        if daemon_lifecycle_event && self.component != Component::Daemon {
            return Err(DiagnosticsError::InvalidEvent(
                "daemon lifecycle events require the daemon component",
            ));
        }
        if self.correlation_id.is_some() && !operation_event {
            return Err(DiagnosticsError::InvalidEvent(
                "correlation identifiers require an operation event",
            ));
        }
        match self.code {
            EventCode::DomainDiagnostic | EventCode::ProbeFailure => {
                let category = self.category.ok_or(DiagnosticsError::InvalidEvent(
                    "domain events require a diagnostic category",
                ))?;
                if self.outcome != Some(outcome_for_category(category)) {
                    return Err(DiagnosticsError::InvalidEvent(
                        "domain event outcome conflicts with its category",
                    ));
                }
            }
            EventCode::ProbeSucceeded => {
                if self.outcome != Some(OperationOutcome::Succeeded) || has_domain_fields {
                    return Err(DiagnosticsError::InvalidEvent(
                        "successful probe event has invalid structured fields",
                    ));
                }
            }
            EventCode::OperationNotImplemented => {
                if self.outcome != Some(OperationOutcome::NotImplemented) || has_domain_fields {
                    return Err(DiagnosticsError::InvalidEvent(
                        "unimplemented operation event has invalid structured fields",
                    ));
                }
            }
            _ if self.outcome.is_some() || has_domain_fields => {
                return Err(DiagnosticsError::InvalidEvent(
                    "lifecycle events cannot carry operation fields",
                ));
            }
            _ => {}
        }
        if let Some(correlation_id) = &self.correlation_id {
            validate_correlation_id(correlation_id.as_str())?;
        }
        Ok(())
    }

    pub(crate) fn set_domain_fields(
        &mut self,
        outcome: OperationOutcome,
        category: SafeDiagnosticCategory,
        capability_id: Option<u32>,
        provider: Option<SafeProvider>,
    ) -> Result<(), DiagnosticsError> {
        self.outcome = Some(outcome);
        self.category = Some(category);
        self.capability_id = capability_id;
        self.provider = provider;
        self.validate()
    }

    pub(crate) fn set_outcome(&mut self, outcome: OperationOutcome) {
        self.outcome = Some(outcome);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct CorrelationId(String);

impl CorrelationId {
    pub fn new(value: String) -> Result<Self, DiagnosticsError> {
        validate_correlation_id(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for CorrelationId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDiagnosticEvent {
    schema_version: u16,
    timestamp_unix_nanos: u64,
    severity: Severity,
    component: Component,
    code: EventCode,
    message: String,
    outcome: Option<OperationOutcome>,
    category: Option<SafeDiagnosticCategory>,
    capability_id: Option<u32>,
    provider: Option<SafeProvider>,
    correlation_id: Option<CorrelationId>,
}

impl<'de> Deserialize<'de> for DiagnosticEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawDiagnosticEvent::deserialize(deserializer)?;
        if raw.schema_version != EVENT_SCHEMA_VERSION {
            return Err(de::Error::custom("unsupported event schema"));
        }
        if raw.message != raw.code.safe_message() {
            return Err(de::Error::custom("unapproved event message"));
        }
        let event = Self {
            schema_version: raw.schema_version,
            timestamp_unix_nanos: raw.timestamp_unix_nanos,
            severity: raw.severity,
            component: raw.component,
            code: raw.code,
            message: raw.code.safe_message(),
            outcome: raw.outcome,
            category: raw.category,
            capability_id: raw.capability_id,
            provider: raw.provider,
            correlation_id: raw.correlation_id,
        };
        event.validate().map_err(de::Error::custom)?;
        Ok(event)
    }
}

pub(crate) fn map_domain_category(
    category: DiagnosticCategory,
) -> (SafeDiagnosticCategory, OperationOutcome) {
    let category = SafeDiagnosticCategory::from(category);
    (category, outcome_for_category(category))
}

pub(crate) fn outcome_for_category(category: SafeDiagnosticCategory) -> OperationOutcome {
    match category {
        SafeDiagnosticCategory::UnsupportedCapability | SafeDiagnosticCategory::MissingProvider => {
            OperationOutcome::Unavailable
        }
        SafeDiagnosticCategory::PermissionDenied
        | SafeDiagnosticCategory::TemporaryFailure
        | SafeDiagnosticCategory::MalformedSourceData => OperationOutcome::Failed,
        SafeDiagnosticCategory::StaleObservation => OperationOutcome::Incomplete,
        SafeDiagnosticCategory::NotYetProbed => OperationOutcome::NotAttempted,
    }
}

pub(crate) fn current_time_unix_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
        .unwrap_or(0)
}

fn validate_correlation_id(value: &str) -> Result<(), DiagnosticsError> {
    if value.is_empty()
        || value.len() > MAX_CORRELATION_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(DiagnosticsError::InvalidEvent(
            "correlation identifier must be a bounded opaque ASCII token",
        ));
    }
    Ok(())
}
