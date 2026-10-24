use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use endfield_unix_ipc::{
    DaemonLifecycle, DaemonStatus, LocalSocketServer, ShutdownHandle, handle_connection,
};

use crate::errors::DaemonError;
use crate::lifecycle::{FailureReason, LifecycleState, ShutdownCoordinator, WorkerCompletion};

const WORKER_COUNT: usize = endfield_unix_ipc::MAX_SIMULTANEOUS_CLIENTS;
const QUEUE_CAPACITY: usize = endfield_unix_ipc::MAX_QUEUED_CLIENTS;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(25);
const STARTUP_READY_LIMIT: Duration = Duration::from_secs(1);

struct PendingConnection {
    stream: UnixStream,
    accepted_at: Instant,
}

struct ListenerChannels {
    work_sender: SyncSender<PendingConnection>,
    activation: Receiver<()>,
    ready: SyncSender<()>,
    completion_sender: SyncSender<Result<(), DaemonError>>,
    worker_exit_receiver: Receiver<()>,
}

pub struct IpcService {
    shutdown: ShutdownHandle,
    activation: Option<SyncSender<()>>,
    completion: Receiver<Result<(), DaemonError>>,
    expected_workers: usize,
    listener_join: Option<JoinHandle<()>>,
    worker_joins: Vec<JoinHandle<()>>,
    coordinator: ShutdownCoordinator,
}

impl IpcService {
    pub fn start(coordinator: ShutdownCoordinator) -> Result<Self, DaemonError> {
        let server = LocalSocketServer::bind().map_err(|_| DaemonError::IpcStartup)?;
        let shutdown = server.shutdown_handle();
        let (work_sender, work_receiver) = mpsc::sync_channel::<PendingConnection>(QUEUE_CAPACITY);
        let work_receiver = Arc::new(Mutex::new(work_receiver));
        let (completion_sender, completion_receiver) = mpsc::sync_channel(WORKER_COUNT + 1);
        let (worker_exit_sender, worker_exit_receiver) = mpsc::sync_channel(WORKER_COUNT);
        let mut worker_joins = Vec::with_capacity(WORKER_COUNT);

        for worker_number in 0..WORKER_COUNT {
            let completion = coordinator
                .register_worker()
                .map_err(|_| DaemonError::WorkerStartup)?;
            let receiver = Arc::clone(&work_receiver);
            let worker_coordinator = coordinator.clone();
            let worker_completion_sender = completion_sender.clone();
            let worker_exit_sender = worker_exit_sender.clone();
            let join = thread::Builder::new()
                .name(format!("endfield-unixd-ipc-{worker_number}"))
                .spawn(move || {
                    connection_worker(
                        receiver,
                        worker_coordinator,
                        completion,
                        worker_completion_sender,
                        worker_exit_sender,
                    )
                })
                .map_err(|_| DaemonError::WorkerStartup)?;
            worker_joins.push(join);
        }
        drop(worker_exit_sender);
        let listener_sender = completion_sender.clone();
        drop(completion_sender);

        let listener_completion = coordinator
            .register_worker()
            .map_err(|_| DaemonError::WorkerStartup)?;
        let (activation_sender, activation_receiver) = mpsc::sync_channel(1);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let listener_coordinator = coordinator.clone();
        let listener_completion_sender = listener_sender;
        let listener_join = thread::Builder::new()
            .name("endfield-unixd-ipc-listener".to_owned())
            .spawn(move || {
                listener_worker(
                    server,
                    listener_coordinator,
                    listener_completion,
                    ListenerChannels {
                        work_sender,
                        activation: activation_receiver,
                        ready: ready_sender,
                        completion_sender: listener_completion_sender,
                        worker_exit_receiver,
                    },
                )
            })
            .map_err(|_| DaemonError::WorkerStartup)?;

        match ready_receiver.recv_timeout(STARTUP_READY_LIMIT) {
            Ok(()) => Ok(Self {
                shutdown,
                activation: Some(activation_sender),
                completion: completion_receiver,
                expected_workers: WORKER_COUNT + 1,
                listener_join: Some(listener_join),
                worker_joins,
                coordinator,
            }),
            Err(_) => {
                let _ = activation_sender.send(());
                let _ = shutdown.request_shutdown();
                drop(listener_join);
                drop(worker_joins);
                Err(DaemonError::WorkerStartup)
            }
        }
    }

    pub fn activate(&mut self) -> Result<(), DaemonError> {
        match self.coordinator.snapshot()?.state {
            LifecycleState::Running => {}
            LifecycleState::Stopping | LifecycleState::Failed(_) => {
                let _ = self.activation.take().map(|sender| sender.send(()));
                return Ok(());
            }
            _ => return Err(DaemonError::WorkerStartup),
        }
        self.activation
            .take()
            .ok_or(DaemonError::WorkerStartup)?
            .send(())
            .map_err(|_| DaemonError::WorkerFailure)
    }

    pub fn shutdown(&mut self, timeout: Duration) -> Result<(), DaemonError> {
        let _ = self.shutdown.request_shutdown();
        if let Some(activation) = self.activation.take() {
            let _ = activation.send(());
        }
        let deadline = Instant::now().checked_add(timeout);
        let mut failure = None;
        for _ in 0..self.expected_workers {
            let remaining = deadline
                .map(|value| value.saturating_duration_since(Instant::now()))
                .unwrap_or(Duration::ZERO);
            if remaining.is_zero() {
                failure = Some(DaemonError::ShutdownTimeout);
                break;
            }
            match self.completion.recv_timeout(remaining) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    failure.get_or_insert(error);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    failure = Some(DaemonError::ShutdownTimeout);
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    failure.get_or_insert(DaemonError::WorkerFailure);
                    break;
                }
            };
        }
        // Completion tokens are sent just before each worker's final return.
        // Dropping the handles avoids an unbounded join on a thread epilogue.
        drop(self.listener_join.take());
        self.worker_joins.clear();
        failure.map_or(Ok(()), Err)
    }
}

fn listener_worker(
    server: LocalSocketServer,
    coordinator: ShutdownCoordinator,
    completion: WorkerCompletion,
    channels: ListenerChannels,
) {
    let _ = channels.ready.send(());
    if channels.activation.recv().is_err() {
        drop(channels.work_sender);
        let worker_result = wait_for_connection_workers(&channels.worker_exit_receiver);
        let _ = server.close();
        finish_worker(completion, channels.completion_sender, worker_result);
        return;
    }

    let mut worker_result = Ok(());
    loop {
        match coordinator.snapshot() {
            Ok(snapshot)
                if matches!(
                    snapshot.state,
                    LifecycleState::Starting | LifecycleState::Running
                ) => {}
            Ok(_) => break,
            Err(_) => {
                worker_result = Err(DaemonError::WorkerFailure);
                break;
            }
        }
        match server.accept() {
            Ok(Some(stream)) => {
                let pending = PendingConnection {
                    stream,
                    accepted_at: Instant::now(),
                };
                match channels.work_sender.try_send(pending) {
                    Ok(()) | Err(TrySendError::Disconnected(_)) => {}
                    Err(TrySendError::Full(_)) => {}
                }
            }
            Ok(None) => break,
            Err(error) => {
                let _ = error;
                worker_result = Err(DaemonError::WorkerFailure);
                break;
            }
        }
    }

    drop(channels.work_sender);
    if wait_for_connection_workers(&channels.worker_exit_receiver).is_err() {
        worker_result = Err(DaemonError::ShutdownTimeout);
    }
    let cleanup = server.close().map_err(|_| DaemonError::ResourceCleanup);
    if cleanup.is_err() {
        worker_result = Err(DaemonError::ResourceCleanup);
    }
    finish_worker(completion, channels.completion_sender, worker_result);
}

fn connection_worker(
    receiver: Arc<Mutex<Receiver<PendingConnection>>>,
    coordinator: ShutdownCoordinator,
    completion: WorkerCompletion,
    completion_sender: SyncSender<Result<(), DaemonError>>,
    worker_exit_sender: SyncSender<()>,
) {
    let mut worker_result = Ok(());
    loop {
        match coordinator.snapshot() {
            Ok(snapshot) if snapshot.state == LifecycleState::Running => {}
            Ok(snapshot) if snapshot.state == LifecycleState::Starting => {}
            Ok(_) => break,
            Err(_) => {
                worker_result = Err(DaemonError::WorkerFailure);
                break;
            }
        }
        let result = match receiver.lock() {
            Ok(receiver) => receiver.recv_timeout(WORKER_POLL_INTERVAL),
            Err(_) => {
                worker_result = Err(DaemonError::WorkerFailure);
                break;
            }
        };
        match result {
            Ok(pending) => {
                let current = coordinator.snapshot();
                if matches!(current, Ok(snapshot) if snapshot.state == LifecycleState::Running) {
                    let expected_uid = nix::unistd::Uid::effective().as_raw();
                    let _ = handle_connection(
                        pending.stream,
                        expected_uid,
                        pending.accepted_at,
                        || daemon_status(&coordinator),
                    );
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    finish_worker(completion, completion_sender, worker_result);
    let _ = worker_exit_sender.send(());
}

fn wait_for_connection_workers(receiver: &Receiver<()>) -> Result<(), DaemonError> {
    let deadline =
        Instant::now() + endfield_unix_ipc::REQUEST_DEADLINE + Duration::from_millis(250);
    for _ in 0..WORKER_COUNT {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || receiver.recv_timeout(remaining).is_err() {
            return Err(DaemonError::ShutdownTimeout);
        }
    }
    Ok(())
}

fn daemon_status(coordinator: &ShutdownCoordinator) -> DaemonStatus {
    let lifecycle = coordinator
        .snapshot()
        .map(|snapshot| snapshot.state)
        .unwrap_or(LifecycleState::Failed(FailureReason::LifecyclePoisoned));
    let lifecycle = match lifecycle {
        LifecycleState::Starting => DaemonLifecycle::Starting,
        LifecycleState::Running => DaemonLifecycle::Running,
        LifecycleState::Stopping => DaemonLifecycle::Stopping,
        LifecycleState::Stopped => DaemonLifecycle::Stopped,
        LifecycleState::Failed(_) => DaemonLifecycle::Failed,
    };
    DaemonStatus::for_lifecycle(
        lifecycle,
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION"),
    )
}

fn finish_worker(
    completion: WorkerCompletion,
    sender: SyncSender<Result<(), DaemonError>>,
    result: Result<(), DaemonError>,
) {
    let completion_result = match result {
        Ok(()) => completion.complete().map_err(DaemonError::from),
        Err(DaemonError::ResourceCleanup) => completion
            .fail(FailureReason::ResourceCleanup)
            .map_err(DaemonError::from),
        Err(_) => completion
            .fail(FailureReason::WorkerFailure)
            .map_err(DaemonError::from),
    };
    let _ = sender.send(completion_result);
}
