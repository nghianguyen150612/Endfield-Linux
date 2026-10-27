use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use endfield_unix_diagnostics::{Component, DiagnosticEvent, EventCode, LogHandle, Severity};
use endfield_unix_sessions::{SessionEvent, SessionSupervisor};

use crate::errors::DaemonError;
use crate::lifecycle::{FailureReason, ShutdownCoordinator, WorkerCompletion};

const STARTUP_READY_LIMIT: Duration = Duration::from_secs(1);

pub struct SessionService {
    supervisor: SessionSupervisor,
    completion: Receiver<Result<(), DaemonError>>,
    join: Option<JoinHandle<()>>,
    completed: bool,
    failed: bool,
}

impl SessionService {
    pub fn start(
        coordinator: ShutdownCoordinator,
        log: Option<LogHandle>,
    ) -> Result<Self, DaemonError> {
        let worker_completion = coordinator
            .register_worker()
            .map_err(|_| DaemonError::WorkerStartup)?;
        let (supervisor, worker) = SessionSupervisor::new();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let (completion_sender, completion_receiver) = mpsc::sync_channel(1);
        let join = thread::Builder::new()
            .name("endfield-unixd-session-monitor".to_owned())
            .spawn(move || {
                session_worker(
                    worker,
                    worker_completion,
                    completion_sender,
                    ready_sender,
                    log,
                )
            })
            .map_err(|_| DaemonError::WorkerStartup)?;
        match ready_receiver.recv_timeout(STARTUP_READY_LIMIT) {
            Ok(()) => Ok(Self {
                supervisor,
                completion: completion_receiver,
                join: Some(join),
                completed: false,
                failed: false,
            }),
            Err(_) => {
                let _ = supervisor.request_shutdown();
                // The worker has no registrations before readiness, so after
                // the shutdown command it can finish without waiting on a
                // child. Join it rather than detaching a failed startup.
                let _ = join.join();
                Err(DaemonError::WorkerStartup)
            }
        }
    }

    pub fn supervisor(&self) -> &SessionSupervisor {
        &self.supervisor
    }

    pub fn reject_new_registrations(&self) -> Result<(), DaemonError> {
        self.supervisor
            .reject_new_registrations()
            .map_err(|_| DaemonError::WorkerFailure)
    }

    pub fn shutdown(&mut self, timeout: Duration) -> Result<(), DaemonError> {
        if self.completed {
            return if self.failed {
                Err(DaemonError::WorkerFailure)
            } else {
                Ok(())
            };
        }
        self.supervisor
            .request_shutdown()
            .map_err(|_| DaemonError::WorkerFailure)?;
        let deadline = Instant::now().checked_add(timeout);
        let remaining = deadline
            .map(|value| value.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::ZERO);
        if remaining.is_zero() {
            return Err(DaemonError::ShutdownTimeout);
        }
        match self.completion.recv_timeout(remaining) {
            Ok(result) => {
                // Completion is reported after the worker loop and all owned
                // process handles have been released.
                drop(self.join.take());
                self.completed = true;
                self.failed = result.is_err();
                result
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Err(DaemonError::ShutdownTimeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(DaemonError::WorkerFailure),
        }
    }
}

fn session_worker(
    worker: endfield_unix_sessions::SupervisorWorker,
    completion: WorkerCompletion,
    completion_sender: SyncSender<Result<(), DaemonError>>,
    ready: SyncSender<()>,
    log: Option<LogHandle>,
) {
    let result = worker
        .run_with_ready(
            || {
                let _ = ready.send(());
            },
            |event| record_session_event(&log, event),
        )
        .map_err(|_| DaemonError::WorkerFailure);
    match result {
        Ok(()) => {
            if completion.complete().is_err() {
                let _ = completion_sender.send(Err(DaemonError::WorkerFailure));
                return;
            }
        }
        Err(_) => {
            let _ = completion.fail(FailureReason::WorkerFailure);
            let _ = completion_sender.send(Err(DaemonError::WorkerFailure));
            return;
        }
    }
    let _ = completion_sender.send(Ok(()));
}

fn record_session_event(log: &Option<LogHandle>, event: SessionEvent) {
    let Some(log) = log else { return };
    let (severity, code) = match event {
        SessionEvent::Registered => (Severity::Info, EventCode::SessionRegistered),
        SessionEvent::Started => (Severity::Info, EventCode::SessionStarted),
        SessionEvent::Exited => (Severity::Info, EventCode::SessionExited),
        SessionEvent::MonitorFailed => (Severity::Warn, EventCode::SessionMonitorFailed),
        SessionEvent::SupervisorStopped => (Severity::Info, EventCode::SessionSupervisorStopped),
    };
    let _ = log.try_record(DiagnosticEvent::new(severity, Component::Daemon, code));
}
