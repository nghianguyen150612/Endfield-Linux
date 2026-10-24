use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureReason {
    SignalHandlerInitialization,
    WorkerStartup,
    WorkerFailure,
    WorkerExitedUnexpectedly,
    ShutdownTimeout,
    ResourceCleanup,
    LifecyclePoisoned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownReason {
    Requested,
    Sigint,
    Sigterm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed(FailureReason),
}

impl LifecycleState {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Failed(_) => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LifecycleSnapshot {
    pub state: LifecycleState,
    pub shutdown_reason: Option<ShutdownReason>,
    pub active_workers: usize,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LifecycleError {
    #[error("invalid daemon lifecycle transition")]
    InvalidTransition,
    #[error("daemon lifecycle synchronization was poisoned")]
    Poisoned,
    #[error("owned worker count overflowed")]
    WorkerCountOverflow,
    #[error("an owned worker failed: {0:?}")]
    WorkerFailed(FailureReason),
    #[error("owned workers did not complete before the shutdown deadline")]
    ShutdownTimeout,
}

#[derive(Clone)]
pub struct ShutdownCoordinator {
    shared: Arc<Shared>,
}

struct Shared {
    snapshot: Mutex<LifecycleSnapshot>,
    changed: Condvar,
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl ShutdownCoordinator {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                snapshot: Mutex::new(LifecycleSnapshot {
                    state: LifecycleState::Starting,
                    shutdown_reason: None,
                    active_workers: 0,
                }),
                changed: Condvar::new(),
            }),
        }
    }

    pub fn snapshot(&self) -> Result<LifecycleSnapshot, LifecycleError> {
        self.shared
            .snapshot
            .lock()
            .map(|snapshot| *snapshot)
            .map_err(|_| LifecycleError::Poisoned)
    }

    pub fn mark_running(&self) -> Result<(), LifecycleError> {
        let mut snapshot = self.lock_snapshot()?;
        if snapshot.state != LifecycleState::Starting {
            return Err(LifecycleError::InvalidTransition);
        }
        snapshot.state = LifecycleState::Running;
        self.shared.changed.notify_all();
        Ok(())
    }

    /// Changes Starting or Running to Stopping. Later stop requests are safe
    /// and return `false`, allowing callers to run cleanup exactly once.
    pub fn request_shutdown(&self, reason: ShutdownReason) -> Result<bool, LifecycleError> {
        let mut snapshot = self.lock_snapshot()?;
        match snapshot.state {
            LifecycleState::Starting | LifecycleState::Running => {
                snapshot.state = LifecycleState::Stopping;
                snapshot.shutdown_reason = Some(reason);
                self.shared.changed.notify_all();
                Ok(true)
            }
            LifecycleState::Stopping | LifecycleState::Stopped | LifecycleState::Failed(_) => {
                Ok(false)
            }
        }
    }

    pub fn fail(&self, reason: FailureReason) -> Result<bool, LifecycleError> {
        let mut snapshot = self.lock_snapshot()?;
        match snapshot.state {
            LifecycleState::Starting | LifecycleState::Running | LifecycleState::Stopping => {
                snapshot.state = LifecycleState::Failed(reason);
                self.shared.changed.notify_all();
                Ok(true)
            }
            LifecycleState::Failed(_) => Ok(false),
            LifecycleState::Stopped => Err(LifecycleError::InvalidTransition),
        }
    }

    pub fn register_worker(&self) -> Result<WorkerCompletion, LifecycleError> {
        let mut snapshot = self.lock_snapshot()?;
        if !matches!(
            snapshot.state,
            LifecycleState::Starting | LifecycleState::Running
        ) {
            return Err(LifecycleError::InvalidTransition);
        }
        snapshot.active_workers = snapshot
            .active_workers
            .checked_add(1)
            .ok_or(LifecycleError::WorkerCountOverflow)?;
        Ok(WorkerCompletion {
            coordinator: self.clone(),
            completed: false,
        })
    }

    /// Blocks on a condition variable until shutdown starts or the lifecycle
    /// fails. No periodic polling is used.
    pub fn wait_for_shutdown_or_failure(&self) -> Result<LifecycleSnapshot, LifecycleError> {
        let mut snapshot = self.lock_snapshot()?;
        while matches!(
            snapshot.state,
            LifecycleState::Starting | LifecycleState::Running
        ) {
            snapshot = self
                .shared
                .changed
                .wait(snapshot)
                .map_err(|_| LifecycleError::Poisoned)?;
        }
        Ok(*snapshot)
    }

    /// Waits only for worker completion notifications. A timeout does not
    /// cancel a worker and is not a claim that its underlying work was bounded.
    pub fn wait_for_workers(&self, timeout: Duration) -> Result<(), LifecycleError> {
        let deadline = Instant::now().checked_add(timeout);
        let mut snapshot = self.lock_snapshot()?;
        while snapshot.active_workers > 0 {
            let Some(deadline) = deadline else {
                return Err(LifecycleError::ShutdownTimeout);
            };
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(LifecycleError::ShutdownTimeout);
            }
            let (next, result) = self
                .shared
                .changed
                .wait_timeout(snapshot, remaining)
                .map_err(|_| LifecycleError::Poisoned)?;
            snapshot = next;
            if result.timed_out() && snapshot.active_workers > 0 {
                return Err(LifecycleError::ShutdownTimeout);
            }
        }
        if let LifecycleState::Failed(reason) = snapshot.state {
            return Err(LifecycleError::WorkerFailed(reason));
        }
        Ok(())
    }

    pub fn complete_shutdown(&self) -> Result<(), LifecycleError> {
        let mut snapshot = self.lock_snapshot()?;
        match snapshot.state {
            LifecycleState::Stopping if snapshot.active_workers == 0 => {
                snapshot.state = LifecycleState::Stopped;
                self.shared.changed.notify_all();
                Ok(())
            }
            LifecycleState::Stopped => Ok(()),
            _ => Err(LifecycleError::InvalidTransition),
        }
    }

    fn lock_snapshot(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, LifecycleSnapshot>, LifecycleError> {
        self.shared
            .snapshot
            .lock()
            .map_err(|_| LifecycleError::Poisoned)
    }

    fn worker_completed(&self, failure: Option<FailureReason>) -> Result<(), LifecycleError> {
        let mut snapshot = self.lock_snapshot()?;
        if snapshot.active_workers == 0 {
            return Err(LifecycleError::InvalidTransition);
        }
        snapshot.active_workers -= 1;
        if let Some(reason) = failure {
            match snapshot.state {
                LifecycleState::Starting | LifecycleState::Running | LifecycleState::Stopping => {
                    snapshot.state = LifecycleState::Failed(reason);
                }
                LifecycleState::Failed(_) | LifecycleState::Stopped => {}
            }
        }
        self.shared.changed.notify_all();
        Ok(())
    }
}

pub struct WorkerCompletion {
    coordinator: ShutdownCoordinator,
    completed: bool,
}

impl WorkerCompletion {
    pub fn complete(mut self) -> Result<(), LifecycleError> {
        self.coordinator.worker_completed(None)?;
        self.completed = true;
        Ok(())
    }

    pub fn fail(mut self, reason: FailureReason) -> Result<(), LifecycleError> {
        self.coordinator.worker_completed(Some(reason))?;
        self.completed = true;
        Ok(())
    }
}

impl Drop for WorkerCompletion {
    fn drop(&mut self) {
        if !self.completed {
            let _ = self
                .coordinator
                .worker_completed(Some(FailureReason::WorkerExitedUnexpectedly));
            self.completed = true;
        }
    }
}
