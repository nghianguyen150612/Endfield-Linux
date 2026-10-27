use std::io::{self, Write};
use std::time::Duration;

use endfield_unix_diagnostics::{
    Component, DiagnosticEvent, EventCode, LogHandle, LogWorker, Severity,
};

use crate::errors::{DaemonError, OptionalDiagnosticsFailure};
use crate::ipc_service::IpcService;
use crate::lifecycle::{FailureReason, LifecycleState, ShutdownCoordinator};
use crate::notify::NotificationClient;
use crate::session_service::SessionService;
use crate::signals::SignalListener;

const LOG_QUEUE_CAPACITY: usize = 64;
const DIAGNOSTICS_SHUTDOWN_LIMIT: Duration = Duration::from_millis(500);
const WORKER_SHUTDOWN_LIMIT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutdownPolicy {
    pub diagnostics_timeout: Duration,
    pub worker_timeout: Duration,
}

impl Default for ShutdownPolicy {
    fn default() -> Self {
        Self {
            diagnostics_timeout: DIAGNOSTICS_SHUTDOWN_LIMIT,
            worker_timeout: WORKER_SHUTDOWN_LIMIT,
        }
    }
}

pub fn run_foreground() -> Result<(), DaemonError> {
    run_foreground_with_policy(ShutdownPolicy::default())
}

pub fn run_foreground_with_policy(policy: ShutdownPolicy) -> Result<(), DaemonError> {
    let coordinator = ShutdownCoordinator::new();
    let mut signals = match SignalListener::start(coordinator.clone()) {
        Ok(signals) => signals,
        Err(error) => {
            let _ = coordinator.fail(FailureReason::SignalHandlerInitialization);
            return Err(error);
        }
    };

    let mut notifications = match NotificationClient::from_environment() {
        Ok(notifications) => notifications,
        Err(_) => {
            let _ = coordinator.fail(FailureReason::WorkerStartup);
            close_signals(&mut signals, &coordinator, policy.worker_timeout)?;
            let _ = coordinator.wait_for_workers(policy.worker_timeout);
            return Err(DaemonError::NotificationFailure);
        }
    };

    let (mut logger, log_handle) = match start_logger() {
        Ok((logger, handle)) => (Some(logger), Some(handle)),
        Err(_) => {
            write_optional_diagnostics_warning();
            (None, None)
        }
    };

    record(&log_handle, Severity::Info, EventCode::DaemonStarting);

    let mut session_service = match SessionService::start(coordinator.clone(), log_handle.clone()) {
        Ok(service) => service,
        Err(error) => {
            record(&log_handle, Severity::Error, EventCode::DaemonStartupFailed);
            let _ = coordinator.fail(FailureReason::WorkerStartup);
            stop_logger_and_report(&mut logger, policy.diagnostics_timeout);
            close_signals(&mut signals, &coordinator, policy.worker_timeout)?;
            let _ = coordinator.wait_for_workers(policy.worker_timeout);
            return Err(error);
        }
    };

    let mut ipc_service = match IpcService::start(coordinator.clone()) {
        Ok(service) => Some(service),
        Err(error) => match coordinator.snapshot()?.state {
            LifecycleState::Stopping => None,
            _ => {
                record(&log_handle, Severity::Error, EventCode::DaemonStartupFailed);
                let _ = coordinator.fail(FailureReason::WorkerStartup);
                let _ = session_service.shutdown(policy.worker_timeout);
                stop_logger_and_report(&mut logger, policy.diagnostics_timeout);
                close_signals(&mut signals, &coordinator, policy.worker_timeout)?;
                let _ = coordinator.wait_for_workers(policy.worker_timeout);
                return Err(error);
            }
        },
    };

    let startup_state = coordinator.snapshot()?.state;
    match startup_state {
        LifecycleState::Starting => {
            if let Err(error) = coordinator.mark_running() {
                if coordinator.snapshot()?.state != LifecycleState::Stopping {
                    record(&log_handle, Severity::Error, EventCode::DaemonStartupFailed);
                    let _ = coordinator.fail(FailureReason::WorkerStartup);
                    if let Some(service) = ipc_service.as_mut() {
                        let _ = service.shutdown(policy.worker_timeout);
                    }
                    let _ = session_service.shutdown(policy.worker_timeout);
                    stop_logger_and_report(&mut logger, policy.diagnostics_timeout);
                    close_signals(&mut signals, &coordinator, policy.worker_timeout)?;
                    let _ = coordinator.wait_for_workers(policy.worker_timeout);
                    return Err(error.into());
                }
            }
            if coordinator.snapshot()?.state == LifecycleState::Running {
                if let Some(service) = ipc_service.as_mut() {
                    if let Err(error) = service.activate() {
                        record(&log_handle, Severity::Error, EventCode::DaemonStartupFailed);
                        let _ = coordinator.fail(FailureReason::WorkerStartup);
                        let _ = service.shutdown(policy.worker_timeout);
                        let _ = session_service.shutdown(policy.worker_timeout);
                        stop_logger_and_report(&mut logger, policy.diagnostics_timeout);
                        close_signals(&mut signals, &coordinator, policy.worker_timeout)?;
                        let _ = coordinator.wait_for_workers(policy.worker_timeout);
                        return Err(error);
                    }
                }
                match notify_ready_if_running(&coordinator, &mut notifications) {
                    Ok(true) => record(&log_handle, Severity::Info, EventCode::DaemonRunning),
                    Ok(false) => {}
                    Err(error) => {
                        record(&log_handle, Severity::Error, EventCode::DaemonStartupFailed);
                        let _ = coordinator.fail(FailureReason::WorkerStartup);
                        if let Some(service) = ipc_service.as_mut() {
                            let _ = service.shutdown(policy.worker_timeout);
                        }
                        let _ = session_service.shutdown(policy.worker_timeout);
                        stop_logger_and_report(&mut logger, policy.diagnostics_timeout);
                        close_signals(&mut signals, &coordinator, policy.worker_timeout)?;
                        let _ = coordinator.wait_for_workers(policy.worker_timeout);
                        return Err(error);
                    }
                }
            }
        }
        LifecycleState::Stopping => {}
        LifecycleState::Failed(_) => {
            record(&log_handle, Severity::Error, EventCode::DaemonStartupFailed);
            if let Some(service) = ipc_service.as_mut() {
                let _ = service.shutdown(policy.worker_timeout);
            }
            let _ = session_service.shutdown(policy.worker_timeout);
            stop_logger_and_report(&mut logger, policy.diagnostics_timeout);
            close_signals(&mut signals, &coordinator, policy.worker_timeout)?;
            let _ = coordinator.wait_for_workers(policy.worker_timeout);
            return Err(DaemonError::WorkerFailure);
        }
        LifecycleState::Running | LifecycleState::Stopped => {
            let _ = coordinator.fail(FailureReason::WorkerStartup);
            if let Some(service) = ipc_service.as_mut() {
                let _ = service.shutdown(policy.worker_timeout);
            }
            let _ = session_service.shutdown(policy.worker_timeout);
            stop_logger_and_report(&mut logger, policy.diagnostics_timeout);
            close_signals(&mut signals, &coordinator, policy.worker_timeout)?;
            let _ = coordinator.wait_for_workers(policy.worker_timeout);
            return Err(DaemonError::WorkerStartup);
        }
    }

    let snapshot = coordinator.wait_for_shutdown_or_failure()?;
    if matches!(
        snapshot.state,
        LifecycleState::Stopping | LifecycleState::Failed(_)
    ) {
        if let Some(notifier) = notifications.as_mut() {
            if notifier.stopping().is_err() {
                let _ = writeln!(
                    io::stderr().lock(),
                    "endfield-unixd: could not send STOPPING=1; continuing graceful cleanup."
                );
            }
        }
    }
    let mut failed = matches!(snapshot.state, LifecycleState::Failed(_));
    if failed {
        record(&log_handle, Severity::Error, EventCode::DaemonRuntimeFailed);
    } else {
        record(
            &log_handle,
            Severity::Info,
            EventCode::DaemonShutdownRequested,
        );
    }

    if session_service.reject_new_registrations().is_err() {
        failed = true;
        let _ = coordinator.fail(FailureReason::WorkerFailure);
    }

    if let Some(service) = ipc_service.as_mut() {
        if let Err(error) = service.shutdown(policy.worker_timeout) {
            failed = true;
            let reason = if matches!(error, DaemonError::ShutdownTimeout) {
                FailureReason::ShutdownTimeout
            } else if matches!(error, DaemonError::ResourceCleanup) {
                FailureReason::ResourceCleanup
            } else {
                FailureReason::WorkerFailure
            };
            let _ = coordinator.fail(reason);
        }
    }

    if let Err(error) = session_service.shutdown(policy.worker_timeout) {
        failed = true;
        let reason = if matches!(error, DaemonError::ShutdownTimeout) {
            FailureReason::ShutdownTimeout
        } else {
            FailureReason::WorkerFailure
        };
        let _ = coordinator.fail(reason);
    }

    if let Err(error) = close_signals(&mut signals, &coordinator, policy.worker_timeout) {
        stop_logger_and_report(&mut logger, policy.diagnostics_timeout);
        return Err(error);
    }

    if let Err(error) = coordinator.wait_for_workers(policy.worker_timeout) {
        stop_logger_and_report(&mut logger, policy.diagnostics_timeout);
        match error {
            crate::lifecycle::LifecycleError::ShutdownTimeout => {
                let _ = coordinator.fail(FailureReason::ShutdownTimeout);
                return Err(DaemonError::ShutdownTimeout);
            }
            crate::lifecycle::LifecycleError::WorkerFailed(_) => {
                return Err(DaemonError::WorkerFailure);
            }
            other => return Err(other.into()),
        }
    }

    if failed {
        record(&log_handle, Severity::Error, EventCode::DaemonRuntimeFailed);
        stop_logger_and_report(&mut logger, policy.diagnostics_timeout);
        return Err(DaemonError::WorkerFailure);
    }
    record(&log_handle, Severity::Info, EventCode::DaemonStopped);
    stop_logger_and_report(&mut logger, policy.diagnostics_timeout);
    coordinator.complete_shutdown()?;
    Ok(())
}

fn notify_ready_if_running(
    coordinator: &ShutdownCoordinator,
    notifications: &mut Option<NotificationClient>,
) -> Result<bool, DaemonError> {
    let result = coordinator.while_running(|| {
        notifications
            .as_mut()
            .map_or(Ok(()), NotificationClient::ready)
    })?;
    match result {
        Some(Ok(())) => Ok(true),
        Some(Err(_)) => Err(DaemonError::NotificationFailure),
        None => Ok(false),
    }
}

fn record(handle: &Option<LogHandle>, severity: Severity, code: EventCode) {
    if record_event(handle, severity, code).is_err() {
        write_optional_diagnostics_warning();
    }
}

fn record_event(
    handle: &Option<LogHandle>,
    severity: Severity,
    code: EventCode,
) -> Result<(), OptionalDiagnosticsFailure> {
    let event = DiagnosticEvent::new(severity, Component::Daemon, code);
    if let Some(handle) = handle {
        handle
            .try_record(event)
            .map(|_| ())
            .map_err(OptionalDiagnosticsFailure::EventRejected)
    } else {
        Ok(())
    }
}

fn start_logger() -> Result<(LogWorker, LogHandle), OptionalDiagnosticsFailure> {
    let logger = LogWorker::start_discovering(LOG_QUEUE_CAPACITY, Severity::Info)
        .map_err(OptionalDiagnosticsFailure::Startup)?;
    let handle = logger.handle();
    Ok((logger, handle))
}

fn stop_logger(
    logger: &mut Option<LogWorker>,
    timeout: Duration,
) -> Result<(), OptionalDiagnosticsFailure> {
    let Some(worker) = logger.take() else {
        return Ok(());
    };
    worker
        .shutdown_bounded(timeout)
        .map_err(OptionalDiagnosticsFailure::Shutdown)
}

fn stop_logger_and_report(logger: &mut Option<LogWorker>, timeout: Duration) {
    if stop_logger(logger, timeout).is_err() {
        write_optional_diagnostics_warning();
    }
}

fn close_signals(
    signals: &mut SignalListener,
    coordinator: &ShutdownCoordinator,
    timeout: Duration,
) -> Result<(), DaemonError> {
    match signals.close(timeout) {
        Ok(()) => Ok(()),
        Err(DaemonError::ShutdownTimeout) => {
            let _ = coordinator.fail(FailureReason::ShutdownTimeout);
            Err(DaemonError::ShutdownTimeout)
        }
        Err(DaemonError::WorkerFailure) => {
            let _ = coordinator.fail(FailureReason::WorkerFailure);
            Err(DaemonError::WorkerFailure)
        }
        Err(_) => {
            let _ = coordinator.fail(FailureReason::ResourceCleanup);
            Err(DaemonError::ResourceCleanup)
        }
    }
}

fn write_optional_diagnostics_warning() {
    static NOTICE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if NOTICE.set(()).is_ok() {
        let _ = writeln!(
            io::stderr().lock(),
            "endfield-unixd: persistent diagnostics unavailable; continuing without file logging."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::ShutdownReason;
    use std::os::unix::net::UnixDatagram;
    use tempfile::tempdir;

    #[test]
    fn shutdown_before_readiness_sends_no_ready_notification() {
        let directory = tempdir().expect("private notify directory");
        let path = directory.path().join("notify.sock");
        let listener = UnixDatagram::bind(&path).expect("bind private notify socket");
        listener
            .set_read_timeout(Some(Duration::from_millis(100)))
            .expect("bound notification receive");
        let mut notifications = Some(
            crate::notify::NotificationClient::for_test(path.as_os_str().to_os_string())
                .expect("validate private socket"),
        );
        let coordinator = ShutdownCoordinator::new();
        coordinator.mark_running().expect("enter running");
        coordinator
            .request_shutdown(ShutdownReason::Sigterm)
            .expect("request shutdown before readiness");

        assert!(!notify_ready_if_running(&coordinator, &mut notifications).unwrap());
        let mut message = [0_u8; 32];
        assert!(matches!(
            listener.recv_from(&mut message).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ));
    }

    #[test]
    fn readiness_notification_failure_is_a_typed_startup_error() {
        let directory = tempdir().expect("private notify directory");
        let path = directory.path().join("notify.sock");
        let listener = UnixDatagram::bind(&path).expect("bind private notify socket");
        let mut notifications = Some(
            crate::notify::NotificationClient::for_test(path.as_os_str().to_os_string())
                .expect("validate private socket"),
        );
        drop(listener);
        std::fs::remove_file(&path).expect("remove private notify socket");
        let coordinator = ShutdownCoordinator::new();
        coordinator.mark_running().expect("enter running");

        assert!(matches!(
            notify_ready_if_running(&coordinator, &mut notifications),
            Err(DaemonError::NotificationFailure)
        ));
    }

    #[test]
    fn lifecycle_records_use_only_fixed_daemon_event_codes() {
        for (code, expected) in [
            (EventCode::DaemonStarting, "DAEMON_STARTING"),
            (EventCode::DaemonRunning, "DAEMON_RUNNING"),
            (
                EventCode::DaemonShutdownRequested,
                "DAEMON_SHUTDOWN_REQUESTED",
            ),
            (EventCode::DaemonStopped, "DAEMON_STOPPED"),
            (EventCode::DaemonStartupFailed, "DAEMON_STARTUP_FAILED"),
            (EventCode::DaemonRuntimeFailed, "DAEMON_RUNTIME_FAILED"),
        ] {
            assert_eq!(code.as_str(), expected);
            let event = DiagnosticEvent::new(Severity::Info, Component::Daemon, code);
            event.validate().expect("approved daemon lifecycle event");
            let json = serde_json::to_value(event).expect("serialize approved event");
            assert_eq!(json["component"], "daemon");
            assert_eq!(json["code"], expected);
            assert_eq!(json["schema_version"], 1);
            assert!(json["message"].as_str().is_some());
            assert!(!json.to_string().contains("synthetic-private-detail"));
        }
    }

    #[test]
    fn session_events_are_allowlisted_static_and_redacted_by_construction() {
        for (code, expected) in [
            (EventCode::SessionRegistered, "SESSION_REGISTERED"),
            (EventCode::SessionStarted, "SESSION_STARTED"),
            (EventCode::SessionExited, "SESSION_EXITED"),
            (EventCode::SessionMonitorFailed, "SESSION_MONITOR_FAILED"),
            (
                EventCode::SessionSupervisorStopped,
                "SESSION_SUPERVISOR_STOPPED",
            ),
        ] {
            let severity = if code == EventCode::SessionMonitorFailed {
                Severity::Warn
            } else {
                Severity::Info
            };
            let event = DiagnosticEvent::new(severity, Component::Daemon, code);
            event.validate().expect("approved safe session event");
            let json = serde_json::to_string(&event).expect("serialize event");
            assert!(json.contains(expected));
            assert!(!json.contains("pid"));
            assert!(!json.contains("private-path"));
        }
    }

    #[test]
    fn existing_event_serialization_is_unchanged_and_details_are_rejected() {
        let existing = DiagnosticEvent::new(
            Severity::Info,
            Component::Application,
            EventCode::AppStarting,
        );
        let encoded = serde_json::to_value(existing).expect("serialize existing event");
        assert_eq!(encoded["schema_version"], 1);
        assert_eq!(encoded["component"], "application");
        assert_eq!(encoded["code"], "APP_STARTING");
        assert_eq!(encoded["message"], "Application is starting.");

        let prior_daemon_gap =
            endfield_unix_diagnostics::event_for_unimplemented_operation(Component::Daemon, 7);
        prior_daemon_gap
            .validate()
            .expect("P008 daemon component events retain their prior contract");

        let mut unsafe_event = serde_json::to_value(DiagnosticEvent::new(
            Severity::Error,
            Component::Daemon,
            EventCode::DaemonRuntimeFailed,
        ))
        .expect("serialize failure event");
        unsafe_event["message"] =
            serde_json::Value::String("path=/home/example/private-error-detail".to_owned());
        assert!(serde_json::from_value::<DiagnosticEvent>(unsafe_event).is_err());
    }

    #[test]
    fn daemon_events_reject_a_non_daemon_component() {
        let event = DiagnosticEvent::new(
            Severity::Info,
            Component::Application,
            EventCode::DaemonRunning,
        );
        assert!(event.validate().is_err());
    }

    #[test]
    fn failed_optional_logger_does_not_change_lifecycle_policy() {
        let coordinator = ShutdownCoordinator::new();
        coordinator.mark_running().expect("enter running");
        coordinator
            .request_shutdown(ShutdownReason::Sigterm)
            .expect("request stop");
        coordinator
            .complete_shutdown()
            .expect("logger is optional and no workers are owned");
        assert_eq!(
            coordinator.snapshot().expect("snapshot").state,
            LifecycleState::Stopped
        );
    }
}
