use std::io::{self, Write};
use std::time::Duration;

use endfield_unix_diagnostics::{
    Component, DiagnosticEvent, EventCode, LogHandle, LogWorker, Severity,
};

use crate::errors::{DaemonError, OptionalDiagnosticsFailure};
use crate::lifecycle::{FailureReason, LifecycleState, ShutdownCoordinator};
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

    let (mut logger, log_handle) = match start_logger() {
        Ok((logger, handle)) => (Some(logger), Some(handle)),
        Err(_) => {
            write_optional_diagnostics_warning();
            (None, None)
        }
    };

    record(&log_handle, Severity::Info, EventCode::DaemonStarting);

    let startup_state = coordinator.snapshot()?.state;
    match startup_state {
        LifecycleState::Starting => match coordinator.mark_running() {
            Ok(()) => record(&log_handle, Severity::Info, EventCode::DaemonRunning),
            Err(_) if coordinator.snapshot()?.state == LifecycleState::Stopping => {}
            Err(error) => {
                record(&log_handle, Severity::Error, EventCode::DaemonStartupFailed);
                let _ = coordinator.fail(FailureReason::WorkerStartup);
                stop_logger_and_report(&mut logger, policy.diagnostics_timeout);
                close_signals(&mut signals, &coordinator, policy.worker_timeout)?;
                return Err(error.into());
            }
        },
        LifecycleState::Stopping => {}
        LifecycleState::Failed(_) => {
            record(&log_handle, Severity::Error, EventCode::DaemonStartupFailed);
            stop_logger_and_report(&mut logger, policy.diagnostics_timeout);
            close_signals(&mut signals, &coordinator, policy.worker_timeout)?;
            return Err(DaemonError::WorkerFailure);
        }
        LifecycleState::Running | LifecycleState::Stopped => {
            let _ = coordinator.fail(FailureReason::WorkerStartup);
            stop_logger_and_report(&mut logger, policy.diagnostics_timeout);
            close_signals(&mut signals, &coordinator, policy.worker_timeout)?;
            return Err(DaemonError::WorkerStartup);
        }
    }

    let snapshot = coordinator.wait_for_shutdown_or_failure()?;
    let failed = matches!(snapshot.state, LifecycleState::Failed(_));
    if failed {
        record(&log_handle, Severity::Error, EventCode::DaemonRuntimeFailed);
    } else {
        record(
            &log_handle,
            Severity::Info,
            EventCode::DaemonShutdownRequested,
        );
        record(&log_handle, Severity::Info, EventCode::DaemonStopped);
    }

    stop_logger_and_report(&mut logger, policy.diagnostics_timeout);

    close_signals(&mut signals, &coordinator, policy.worker_timeout)?;

    if let Err(error) = coordinator.wait_for_workers(policy.worker_timeout) {
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
        return Err(DaemonError::WorkerFailure);
    }
    coordinator.complete_shutdown()?;
    Ok(())
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
