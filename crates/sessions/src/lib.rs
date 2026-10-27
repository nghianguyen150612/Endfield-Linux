//! Bounded, in-memory supervision for explicitly owned child processes.
//!
//! The supervisor accepts an owned [`std::process::Child`], never a PID. One
//! worker owns and checks all registered children, and shutdown only cancels
//! monitoring; it never signals a child. This crate does not launch processes.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, VecDeque};
use std::process::Child;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use endfield_unix_telemetry::SamplingContextId;
use thiserror::Error;

mod process;

pub const MAX_ACTIVE_SESSIONS: usize = 64;
pub const MAX_COMPLETED_SESSIONS: usize = 128;
pub const MONITOR_INTERVAL: Duration = Duration::from_millis(100);
const WORK_QUEUE_CAPACITY: usize = MAX_ACTIVE_SESSIONS + 1;
const SESSION_ID_BYTES: usize = 16;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(value: impl Into<String>) -> Result<Self, SessionError> {
        let value = value.into();
        if value.len() != SESSION_ID_BYTES * 2
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(SessionError::InvalidSessionId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn generate() -> Result<Self, SessionError> {
        let mut bytes = [0_u8; SESSION_ID_BYTES];
        getrandom::getrandom(&mut bytes).map_err(|_| SessionError::IdentitySourceUnavailable)?;
        let mut text = String::with_capacity(SESSION_ID_BYTES * 2);
        for byte in bytes {
            use std::fmt::Write as _;
            write!(&mut text, "{byte:02x}").map_err(|_| SessionError::IdentitySourceUnavailable)?;
        }
        Self::new(text)
    }
}

impl std::fmt::Debug for SessionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("SessionId")
            .field(&"[opaque]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Created,
    Starting,
    Running,
    Stopping,
    Exited,
    Failed,
    Stopped,
}

impl SessionState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Exited | Self::Failed | Self::Stopped)
    }

    pub fn transition(self, next: Self) -> Result<Self, SessionError> {
        let valid = matches!(
            (self, next),
            (Self::Created, Self::Starting)
                | (Self::Starting, Self::Running)
                | (Self::Starting, Self::Exited)
                | (Self::Starting, Self::Failed)
                | (Self::Running, Self::Exited)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::Stopping)
                | (Self::Stopping, Self::Stopped)
                | (Self::Stopping, Self::Failed)
        );
        if valid {
            Ok(next)
        } else {
            Err(SessionError::InvalidTransition)
        }
    }

    /// Applies a monitor observation to the portable lifecycle model. This is
    /// also the deterministic seam used by contract tests; no process is
    /// synthesized or attached by this operation.
    pub fn apply_observation(self, observation: ProcessObservation) -> Result<Self, SessionError> {
        match (self, observation) {
            (Self::Starting, ProcessObservation::Running) => self.transition(Self::Running),
            (Self::Running, ProcessObservation::Running) => Ok(Self::Running),
            (Self::Starting | Self::Running, ProcessObservation::Exited(_)) => {
                self.transition(Self::Exited)
            }
            (Self::Stopping, ProcessObservation::Exited(_)) => self.transition(Self::Stopped),
            (
                Self::Starting | Self::Running | Self::Stopping,
                ProcessObservation::MonitorFailed(_),
            ) => self.transition(Self::Failed),
            _ => Err(SessionError::InvalidTransition),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessObservation {
    Running,
    Exited(ExitOutcome),
    MonitorFailed(MonitorFailure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitOutcome {
    Exited { code: i32 },
    Signaled { signal: i32 },
    StatusUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonitorStatus {
    Active,
    Complete,
    Failed(MonitorFailure),
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonitorFailure {
    WaitFailed,
    OwnershipUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafeErrorCategory {
    ProcessWaitFailed,
    InvalidTransition,
    RegistryCapacity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: SessionId,
    pub state: SessionState,
    /// PID is cleared after reaping or a monitor failure; it is never used to
    /// reattach to a process.
    pub process_id: Option<u32>,
    pub sampling_context: SamplingContextId,
    pub monitor_status: MonitorStatus,
    pub exit: Option<ExitOutcome>,
    /// Time elapsed at the supervisor's observation, not a fabricated exit time.
    pub exit_observed_after: Option<Duration>,
    pub error: Option<SafeErrorCategory>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    Registered,
    Started,
    Exited,
    MonitorFailed,
    SupervisorStopped,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SessionError {
    #[error("session identifier is invalid")]
    InvalidSessionId,
    #[error("operating-system random source is unavailable")]
    IdentitySourceUnavailable,
    #[error("the session registry is unavailable")]
    RegistryUnavailable,
    #[error("the session supervisor is stopping")]
    ShuttingDown,
    #[error("the active session limit has been reached")]
    Capacity,
    #[error("the session identifier is already retained")]
    DuplicateId,
    #[error("the sampling context is already associated with a retained session")]
    DuplicateSamplingContext,
    #[error("the session does not exist")]
    NotFound,
    #[error("the requested session transition is invalid")]
    InvalidTransition,
    #[error("the session monitor is unavailable")]
    MonitorUnavailable,
    #[error("the session monitor worker failed")]
    WorkerFailed,
    #[error("the requested session observation did not arrive before the deadline")]
    ObservationTimeout,
}

pub struct RegistrationFailure {
    pub error: SessionError,
    pub child: Child,
}

impl std::fmt::Debug for RegistrationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RegistrationFailure")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct SessionSupervisor {
    shared: Arc<Shared>,
    commands: SyncSender<Command>,
}

pub struct SupervisorWorker {
    shared: Arc<Shared>,
    commands: Receiver<Command>,
}

struct Shared {
    registry: Mutex<Registry>,
    changed: Condvar,
    start: Instant,
}

struct Registry {
    accepting: bool,
    shutdown_requested: bool,
    sessions: BTreeMap<SessionId, SessionSummary>,
    completed: VecDeque<SessionId>,
}

enum Command {
    Register { id: SessionId, child: Child },
    Shutdown,
}

enum ReceivedCommand {
    Command(Command),
    Timeout,
    Disconnected,
}

impl SessionSupervisor {
    pub fn new() -> (Self, SupervisorWorker) {
        let shared = Arc::new(Shared {
            registry: Mutex::new(Registry {
                accepting: true,
                shutdown_requested: false,
                sessions: BTreeMap::new(),
                completed: VecDeque::new(),
            }),
            changed: Condvar::new(),
            start: Instant::now(),
        });
        let (sender, receiver) = mpsc::sync_channel(WORK_QUEUE_CAPACITY);
        (
            Self {
                shared: Arc::clone(&shared),
                commands: sender,
            },
            SupervisorWorker {
                shared,
                commands: receiver,
            },
        )
    }

    /// Transfers an explicitly owned child to the daemon's internal monitor.
    /// This is a trusted in-process boundary; callers must not register a PID
    /// or infer ownership from command lines, executable names, or IPC identity.
    pub fn register_owned_child(&self, child: Child) -> Result<SessionId, RegistrationFailure> {
        let pid = child.id();
        let mut registry = match self.shared.registry.lock() {
            Ok(value) => value,
            Err(_) => {
                return Err(RegistrationFailure {
                    error: SessionError::RegistryUnavailable,
                    child,
                });
            }
        };
        if !registry.accepting {
            return Err(RegistrationFailure {
                error: SessionError::ShuttingDown,
                child,
            });
        }
        let active = registry
            .sessions
            .values()
            .filter(|summary| !summary.state.is_terminal())
            .count();
        if active >= MAX_ACTIVE_SESSIONS {
            return Err(RegistrationFailure {
                error: SessionError::Capacity,
                child,
            });
        }
        let id = match unique_session_id(&registry, SessionId::generate) {
            Ok(value) => value,
            Err(error) => return Err(RegistrationFailure { error, child }),
        };
        let sampling_context = match unique_sampling_context(&registry) {
            Ok(value) => value,
            Err(error) => return Err(RegistrationFailure { error, child }),
        };
        let state = match SessionState::Created.transition(SessionState::Starting) {
            Ok(value) => value,
            Err(error) => return Err(RegistrationFailure { error, child }),
        };
        let summary = SessionSummary {
            id: id.clone(),
            state,
            process_id: Some(pid),
            sampling_context,
            monitor_status: MonitorStatus::Active,
            exit: None,
            exit_observed_after: None,
            error: None,
        };
        match self.commands.try_send(Command::Register {
            id: id.clone(),
            child,
        }) {
            Ok(()) => {
                registry.sessions.insert(id.clone(), summary);
                self.shared.changed.notify_all();
                Ok(id)
            }
            Err(TrySendError::Full(Command::Register { child, .. })) => Err(RegistrationFailure {
                error: SessionError::Capacity,
                child,
            }),
            Err(TrySendError::Disconnected(Command::Register { child, .. })) => {
                Err(RegistrationFailure {
                    error: SessionError::MonitorUnavailable,
                    child,
                })
            }
            Err(
                TrySendError::Full(Command::Shutdown)
                | TrySendError::Disconnected(Command::Shutdown),
            ) => unreachable!("registration sends only register commands"),
        }
    }

    pub fn get(&self, id: &SessionId) -> Result<SessionSummary, SessionError> {
        self.shared
            .registry
            .lock()
            .map_err(|_| SessionError::RegistryUnavailable)?
            .sessions
            .get(id)
            .cloned()
            .ok_or(SessionError::NotFound)
    }

    pub fn list(&self) -> Result<Vec<SessionSummary>, SessionError> {
        let registry = self
            .shared
            .registry
            .lock()
            .map_err(|_| SessionError::RegistryUnavailable)?;
        Ok(registry.sessions.values().cloned().collect())
    }

    pub fn wait_for_state(
        &self,
        id: &SessionId,
        state: SessionState,
        timeout: Duration,
    ) -> Result<SessionSummary, SessionError> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(SessionError::ObservationTimeout)?;
        let mut registry = self
            .shared
            .registry
            .lock()
            .map_err(|_| SessionError::RegistryUnavailable)?;
        loop {
            let summary = registry
                .sessions
                .get(id)
                .cloned()
                .ok_or(SessionError::NotFound)?;
            if summary.state == state {
                return Ok(summary);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(SessionError::ObservationTimeout);
            }
            let (next, result) = self
                .shared
                .changed
                .wait_timeout(registry, remaining)
                .map_err(|_| SessionError::RegistryUnavailable)?;
            registry = next;
            if result.timed_out()
                && registry
                    .sessions
                    .get(id)
                    .is_some_and(|summary| summary.state != state)
            {
                return Err(SessionError::ObservationTimeout);
            }
        }
    }

    pub fn request_shutdown(&self) -> Result<(), SessionError> {
        let mut registry = self
            .shared
            .registry
            .lock()
            .map_err(|_| SessionError::RegistryUnavailable)?;
        registry.accepting = false;
        if registry.shutdown_requested {
            return Ok(());
        }
        self.shared.changed.notify_all();
        match self.commands.try_send(Command::Shutdown) {
            Ok(()) => {
                registry.shutdown_requested = true;
                Ok(())
            }
            Err(TrySendError::Disconnected(_)) => Err(SessionError::MonitorUnavailable),
            // There can be at most 64 queued registrations and registration is
            // rejected at 64 active sessions. The worker is still draining.
            Err(TrySendError::Full(_)) => Err(SessionError::MonitorUnavailable),
        }
    }

    pub fn reject_new_registrations(&self) -> Result<(), SessionError> {
        let mut registry = self
            .shared
            .registry
            .lock()
            .map_err(|_| SessionError::RegistryUnavailable)?;
        registry.accepting = false;
        self.shared.changed.notify_all();
        Ok(())
    }

    pub fn is_accepting(&self) -> Result<bool, SessionError> {
        self.shared
            .registry
            .lock()
            .map(|registry| registry.accepting)
            .map_err(|_| SessionError::RegistryUnavailable)
    }
}

impl SupervisorWorker {
    /// Runs the sole bounded monitor worker until shutdown or a worker failure.
    pub fn run(self, on_event: impl FnMut(SessionEvent)) -> Result<(), SessionError> {
        self.run_with_ready(|| {}, on_event)
    }

    /// Runs the monitor loop and announces readiness after its bounded worker
    /// state is initialized but before it begins accepting registrations.
    pub fn run_with_ready(
        self,
        on_ready: impl FnOnce(),
        on_event: impl FnMut(SessionEvent),
    ) -> Result<(), SessionError> {
        let mut monitor = process::OwnedChildMonitor;
        self.run_with_monitor(on_ready, on_event, &mut monitor)
    }

    fn run_with_monitor(
        self,
        on_ready: impl FnOnce(),
        mut on_event: impl FnMut(SessionEvent),
        monitor: &mut impl process::ChildMonitor,
    ) -> Result<(), SessionError> {
        let mut children = BTreeMap::<SessionId, Child>::new();
        on_ready();
        loop {
            let received = if children.is_empty() {
                self.commands
                    .recv()
                    .map(ReceivedCommand::Command)
                    .unwrap_or(ReceivedCommand::Disconnected)
            } else {
                match self.commands.recv_timeout(MONITOR_INTERVAL) {
                    Ok(command) => ReceivedCommand::Command(command),
                    Err(mpsc::RecvTimeoutError::Timeout) => ReceivedCommand::Timeout,
                    Err(mpsc::RecvTimeoutError::Disconnected) => ReceivedCommand::Disconnected,
                }
            };
            match received {
                ReceivedCommand::Command(Command::Register { id, child }) => {
                    self.accept_child(id, child, monitor, &mut children, &mut on_event)?;
                }
                ReceivedCommand::Command(Command::Shutdown) | ReceivedCommand::Disconnected => {
                    self.cancel_monitoring(monitor, &mut children, &mut on_event)?;
                    on_event(SessionEvent::SupervisorStopped);
                    return Ok(());
                }
                ReceivedCommand::Timeout => {}
            }
            loop {
                match self.commands.try_recv() {
                    Ok(Command::Register { id, child }) => {
                        self.accept_child(id, child, monitor, &mut children, &mut on_event)?;
                    }
                    Ok(Command::Shutdown) => {
                        self.cancel_monitoring(monitor, &mut children, &mut on_event)?;
                        on_event(SessionEvent::SupervisorStopped);
                        return Ok(());
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.cancel_monitoring(monitor, &mut children, &mut on_event)?;
                        on_event(SessionEvent::SupervisorStopped);
                        return Ok(());
                    }
                }
            }
            self.poll_children(monitor, &mut children, &mut on_event)?;
        }
    }

    fn accept_child(
        &self,
        id: SessionId,
        mut child: Child,
        monitor: &mut impl process::ChildMonitor,
        children: &mut BTreeMap<SessionId, Child>,
        on_event: &mut impl FnMut(SessionEvent),
    ) -> Result<(), SessionError> {
        let initial = monitor.try_wait(&mut child);
        let mut events = vec![SessionEvent::Registered];
        {
            let mut registry = self
                .shared
                .registry
                .lock()
                .map_err(|_| SessionError::RegistryUnavailable)?;
            let Some(summary) = registry.sessions.get_mut(&id) else {
                return Err(SessionError::NotFound);
            };
            match initial {
                Ok(None) => {
                    summary.state = summary
                        .state
                        .apply_observation(ProcessObservation::Running)?;
                    events.push(SessionEvent::Started);
                    children.insert(id, child);
                }
                Ok(Some(status)) => {
                    let outcome = process::exit_outcome(status);
                    summary.state = summary
                        .state
                        .apply_observation(ProcessObservation::Exited(outcome))?;
                    summary.process_id = None;
                    summary.monitor_status = MonitorStatus::Complete;
                    summary.exit = Some(outcome);
                    summary.exit_observed_after = Some(self.shared.start.elapsed());
                    registry.completed.push_back(id);
                    events.push(SessionEvent::Exited);
                    trim_completed(&mut registry);
                }
                Err(error) => {
                    let failure = process::wait_failure(&error);
                    summary.state = summary
                        .state
                        .apply_observation(ProcessObservation::MonitorFailed(failure))?;
                    summary.process_id = None;
                    summary.monitor_status = MonitorStatus::Failed(failure);
                    summary.error = Some(SafeErrorCategory::ProcessWaitFailed);
                    registry.completed.push_back(id);
                    events.push(SessionEvent::MonitorFailed);
                    trim_completed(&mut registry);
                }
            }
            self.shared.changed.notify_all();
        }
        for event in events {
            on_event(event);
        }
        Ok(())
    }

    fn poll_children(
        &self,
        monitor: &mut impl process::ChildMonitor,
        children: &mut BTreeMap<SessionId, Child>,
        on_event: &mut impl FnMut(SessionEvent),
    ) -> Result<(), SessionError> {
        let ids = children.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            let child = children.get_mut(&id).expect("child key was collected");
            let observation = match monitor.try_wait(child) {
                Ok(Some(status)) => Some(Ok(status)),
                Ok(None) => None,
                Err(error) => Some(Err(process::wait_failure(&error))),
            };
            let Some(observation) = observation else {
                continue;
            };
            let event = {
                let Ok(mut registry) = self.shared.registry.lock() else {
                    return Err(SessionError::RegistryUnavailable);
                };
                let Some(summary) = registry.sessions.get_mut(&id) else {
                    continue;
                };
                if summary.state.is_terminal() {
                    continue;
                }
                let event = match observation {
                    Ok(status) => {
                        let outcome = process::exit_outcome(status);
                        summary.state = summary
                            .state
                            .apply_observation(ProcessObservation::Exited(outcome))?;
                        summary.process_id = None;
                        summary.exit = Some(outcome);
                        summary.exit_observed_after = Some(self.shared.start.elapsed());
                        summary.monitor_status = MonitorStatus::Complete;
                        SessionEvent::Exited
                    }
                    Err(failure) => {
                        summary.state = summary
                            .state
                            .apply_observation(ProcessObservation::MonitorFailed(failure))?;
                        summary.process_id = None;
                        summary.monitor_status = MonitorStatus::Failed(failure);
                        summary.error = Some(SafeErrorCategory::ProcessWaitFailed);
                        SessionEvent::MonitorFailed
                    }
                };
                registry.completed.push_back(id.clone());
                trim_completed(&mut registry);
                self.shared.changed.notify_all();
                event
            };
            children.remove(&id);
            on_event(event);
        }
        Ok(())
    }

    fn cancel_monitoring(
        &self,
        monitor: &mut impl process::ChildMonitor,
        children: &mut BTreeMap<SessionId, Child>,
        on_event: &mut impl FnMut(SessionEvent),
    ) -> Result<(), SessionError> {
        // One last try_wait preserves an exit already observable at shutdown.
        self.poll_children(monitor, children, on_event)?;
        if let Ok(mut registry) = self.shared.registry.lock() {
            for summary in registry.sessions.values_mut() {
                if !summary.state.is_terminal() {
                    summary.monitor_status = MonitorStatus::Cancelled;
                }
            }
        } else {
            return Err(SessionError::RegistryUnavailable);
        }
        self.shared.changed.notify_all();
        // Child::drop sends no signal. Running processes may outlive this daemon,
        // but they are no longer supervised after this point.
        children.clear();
        Ok(())
    }
}

fn trim_completed(registry: &mut Registry) {
    while registry.completed.len() > MAX_COMPLETED_SESSIONS {
        if let Some(id) = registry.completed.pop_front() {
            registry.sessions.remove(&id);
        }
    }
}

fn unique_session_id(
    registry: &Registry,
    mut generate: impl FnMut() -> Result<SessionId, SessionError>,
) -> Result<SessionId, SessionError> {
    for _ in 0..8 {
        let generated = generate()?;
        if !registry.sessions.contains_key(&generated) {
            return Ok(generated);
        }
    }
    Err(SessionError::DuplicateId)
}

fn unique_sampling_context(registry: &Registry) -> Result<SamplingContextId, SessionError> {
    unique_sampling_context_by(registry, || {
        let mut bytes = [0_u8; std::mem::size_of::<u64>()];
        getrandom::getrandom(&mut bytes).map_err(|_| SessionError::IdentitySourceUnavailable)?;
        Ok(SamplingContextId::new(u64::from_be_bytes(bytes)).ok())
    })
}

fn unique_sampling_context_by(
    registry: &Registry,
    mut generate: impl FnMut() -> Result<Option<SamplingContextId>, SessionError>,
) -> Result<SamplingContextId, SessionError> {
    for _ in 0..8 {
        let Some(context) = generate()? else {
            continue;
        };
        if !registry
            .sessions
            .values()
            .any(|summary| summary.sampling_context == context)
        {
            return Ok(context);
        }
    }
    Err(SessionError::DuplicateSamplingContext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::mpsc::{self, Receiver, SyncSender};
    use std::thread;

    use nix::errno::Errno;
    use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
    use nix::unistd::Pid;

    #[test]
    fn id_validation_and_state_terminal_contract() {
        assert!(SessionId::new("0123456789abcdef0123456789abcdef").is_ok());
        for value in [
            "",
            "a",
            "0123456789ABCDEF0123456789ABCDEF",
            "g123456789abcdef0123456789abcdef",
        ] {
            assert_eq!(SessionId::new(value), Err(SessionError::InvalidSessionId));
        }
        assert!(SessionState::Exited.is_terminal());
        assert!(SessionState::Failed.is_terminal());
        assert!(SessionState::Stopped.is_terminal());
        assert!(!SessionState::Running.is_terminal());
    }

    #[test]
    fn registry_rejects_registration_after_shutdown_without_losing_child() {
        let (supervisor, worker) = SessionSupervisor::new();
        // A dead harmless child remains a valid owned Child for this contract.
        let mut child = std::process::Command::new("true").spawn().unwrap();
        child.wait().unwrap();
        supervisor.request_shutdown().unwrap();
        let error = supervisor.register_owned_child(child).unwrap_err();
        assert_eq!(error.error, SessionError::ShuttingDown);
        drop(error.child);
        drop(worker);
    }

    #[test]
    fn retention_evicts_only_the_oldest_completed_summary() {
        let mut registry = Registry {
            accepting: true,
            shutdown_requested: false,
            sessions: BTreeMap::new(),
            completed: VecDeque::new(),
        };
        for value in 0..=MAX_COMPLETED_SESSIONS {
            let id = SessionId::new(format!("{value:032x}")).unwrap();
            registry.sessions.insert(
                id.clone(),
                SessionSummary {
                    id: id.clone(),
                    state: SessionState::Exited,
                    process_id: None,
                    sampling_context: SamplingContextId::new(value as u64 + 1).unwrap(),
                    monitor_status: MonitorStatus::Complete,
                    exit: Some(ExitOutcome::Exited { code: 0 }),
                    exit_observed_after: Some(Duration::ZERO),
                    error: None,
                },
            );
            registry.completed.push_back(id);
        }
        let active_id = SessionId::new("f".repeat(32)).unwrap();
        registry.sessions.insert(
            active_id.clone(),
            SessionSummary {
                id: active_id.clone(),
                state: SessionState::Running,
                process_id: Some(1234),
                sampling_context: SamplingContextId::new(999).unwrap(),
                monitor_status: MonitorStatus::Complete,
                exit: None,
                exit_observed_after: None,
                error: None,
            },
        );
        trim_completed(&mut registry);
        assert_eq!(registry.completed.len(), MAX_COMPLETED_SESSIONS);
        assert_eq!(registry.sessions.len(), MAX_COMPLETED_SESSIONS + 1);
        assert_eq!(registry.sessions[&active_id].state, SessionState::Running);
        assert!(
            !registry
                .sessions
                .contains_key(&SessionId::new("0".repeat(32)).unwrap())
        );
    }

    #[test]
    fn generated_identity_collisions_are_retried_and_never_overwrite() {
        let duplicate = SessionId::new("1".repeat(32)).unwrap();
        let unique = SessionId::new("2".repeat(32)).unwrap();
        let mut registry = Registry {
            accepting: true,
            shutdown_requested: false,
            sessions: BTreeMap::new(),
            completed: VecDeque::new(),
        };
        registry.sessions.insert(
            duplicate.clone(),
            SessionSummary {
                id: duplicate.clone(),
                state: SessionState::Exited,
                process_id: None,
                sampling_context: SamplingContextId::new(1).unwrap(),
                monitor_status: MonitorStatus::Complete,
                exit: Some(ExitOutcome::Exited { code: 0 }),
                exit_observed_after: Some(Duration::ZERO),
                error: None,
            },
        );
        let mut candidates = [duplicate.clone(), unique.clone()].into_iter();
        assert_eq!(
            unique_session_id(&registry, || Ok(candidates.next().unwrap())),
            Ok(unique)
        );
        assert_eq!(registry.sessions[&duplicate].state, SessionState::Exited);
        assert_eq!(
            unique_session_id(&registry, || Ok(duplicate.clone())),
            Err(SessionError::DuplicateId)
        );
    }

    #[test]
    fn sampling_context_collisions_are_retried_without_reassigning_sessions() {
        let first = SamplingContextId::new(41).unwrap();
        let next = SamplingContextId::new(42).unwrap();
        let mut registry = Registry {
            accepting: true,
            shutdown_requested: false,
            sessions: BTreeMap::new(),
            completed: VecDeque::new(),
        };
        let id = SessionId::new("3".repeat(32)).unwrap();
        registry.sessions.insert(
            id.clone(),
            SessionSummary {
                id,
                state: SessionState::Running,
                process_id: None,
                sampling_context: first,
                monitor_status: MonitorStatus::Active,
                exit: None,
                exit_observed_after: None,
                error: None,
            },
        );
        let mut candidates = [first, next].into_iter();
        assert_eq!(
            unique_sampling_context_by(&registry, || Ok(Some(candidates.next().unwrap()))),
            Ok(next)
        );
        assert_eq!(
            registry.sessions.values().next().unwrap().sampling_context,
            first
        );
        assert_eq!(
            unique_sampling_context_by(&registry, || Ok(Some(first))),
            Err(SessionError::DuplicateSamplingContext)
        );
    }

    #[test]
    fn injected_monitor_error_fails_session_without_fabricating_exit() {
        let (supervisor, worker) = SessionSupervisor::new();
        let worker_supervisor = supervisor.clone();
        let mut child = TestChild::spawn();
        let id = supervisor.register_owned_child(child.take_child()).unwrap();
        let worker_thread = thread::spawn(move || {
            let mut monitor = FailingMonitor;
            worker.run_with_monitor(|| {}, |_| {}, &mut monitor)
        });

        let summary = worker_supervisor
            .wait_for_state(&id, SessionState::Failed, Duration::from_secs(3))
            .unwrap();
        assert_eq!(
            summary.monitor_status,
            MonitorStatus::Failed(MonitorFailure::WaitFailed)
        );
        assert_eq!(summary.exit, None);
        assert_eq!(summary.process_id, None);

        child.release();
        worker_supervisor.request_shutdown().unwrap();
        worker_thread.join().unwrap().unwrap();
        drop(child);
    }

    #[test]
    fn blocked_monitor_observation_is_not_reported_as_completed_shutdown() {
        let (supervisor, worker) = SessionSupervisor::new();
        let mut child = TestChild::spawn();
        let id = supervisor.register_owned_child(child.take_child()).unwrap();
        let (entered_sender, entered_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let (done_sender, done_receiver) = mpsc::sync_channel(1);
        let worker_thread = thread::spawn(move || {
            let mut monitor = BlockingMonitor {
                entered: entered_sender,
                release: release_receiver,
                block_once: true,
            };
            let result = worker.run_with_monitor(|| {}, |_| {}, &mut monitor);
            let _ = done_sender.send(result);
        });

        entered_receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("controlled monitor is blocked on its first observation");
        supervisor.request_shutdown().unwrap();
        assert_eq!(
            done_receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty),
            "shutdown request does not claim worker completion"
        );

        release_sender.send(()).unwrap();
        done_receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("monitor resumes and acknowledges shutdown")
            .unwrap();
        worker_thread.join().unwrap();
        let summary = supervisor.get(&id).unwrap();
        assert_eq!(summary.state, SessionState::Running);
        assert_eq!(summary.monitor_status, MonitorStatus::Cancelled);

        child.release();
        drop(child);
    }

    struct FailingMonitor;

    impl process::ChildMonitor for FailingMonitor {
        fn try_wait(
            &mut self,
            _child: &mut Child,
        ) -> std::io::Result<Option<std::process::ExitStatus>> {
            Err(std::io::Error::other("controlled monitor failure"))
        }
    }

    struct BlockingMonitor {
        entered: SyncSender<()>,
        release: Receiver<()>,
        block_once: bool,
    }

    impl process::ChildMonitor for BlockingMonitor {
        fn try_wait(
            &mut self,
            child: &mut Child,
        ) -> std::io::Result<Option<std::process::ExitStatus>> {
            if self.block_once {
                self.block_once = false;
                let _ = self.entered.send(());
                let _ = self.release.recv();
            }
            child.try_wait()
        }
    }

    struct TestChild {
        child: Option<Child>,
        writer: Option<ChildStdin>,
        pid: u32,
    }

    impl TestChild {
        fn spawn() -> Self {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "tests::controlled_monitor_fixture",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let pid = child.id();
            let writer = child.stdin.take().unwrap();
            Self {
                child: Some(child),
                writer: Some(writer),
                pid,
            }
        }

        fn take_child(&mut self) -> Child {
            self.child.take().unwrap()
        }

        fn release(&mut self) {
            if let Some(mut writer) = self.writer.take() {
                let _ = writer.write_all(&[0]);
            }
        }
    }

    impl Drop for TestChild {
        fn drop(&mut self) {
            self.release();
            if let Some(mut child) = self.child.take() {
                let _ = child.wait();
            } else {
                reap_test_child(self.pid);
            }
        }
    }

    fn reap_test_child(pid: u32) {
        let pid = Pid::from_raw(i32::try_from(pid).unwrap());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::StillAlive) => {}
                Ok(_) | Err(Errno::ECHILD) => return,
                Err(error) => panic!("controlled child cleanup failed: {error}"),
            }
            assert!(
                Instant::now() < deadline,
                "controlled child exceeded deadline"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    #[ignore]
    fn controlled_monitor_fixture() {
        let mut command = [0_u8; 1];
        let _ = std::io::stdin().read_exact(&mut command);
    }
}
