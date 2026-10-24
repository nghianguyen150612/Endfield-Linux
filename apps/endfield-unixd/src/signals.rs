use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::errors::DaemonError;
use crate::lifecycle::{FailureReason, ShutdownCoordinator, ShutdownReason, WorkerCompletion};

#[cfg(unix)]
pub struct SignalListener {
    handle: signal_hook::iterator::Handle,
    completion: Receiver<Result<(), DaemonError>>,
    join: Option<JoinHandle<()>>,
}

#[cfg(unix)]
impl SignalListener {
    pub fn start(coordinator: ShutdownCoordinator) -> Result<Self, DaemonError> {
        use signal_hook::consts::signal::{SIGINT, SIGTERM};
        use signal_hook::iterator::Signals;

        let signals = Signals::new([SIGINT, SIGTERM])
            .map_err(|_| DaemonError::SignalHandlerInitialization)?;
        let handle = signals.handle();
        let completion = coordinator
            .register_worker()
            .map_err(|_| DaemonError::SignalHandlerInitialization)?;
        let (completion_sender, completion_receiver) = mpsc::sync_channel(1);
        let join = thread::Builder::new()
            .name("endfield-unixd-signals".to_owned())
            .spawn(move || signal_thread(signals, coordinator, completion, completion_sender))
            .map_err(|_| DaemonError::SignalHandlerInitialization)?;

        Ok(Self {
            handle,
            completion: completion_receiver,
            join: Some(join),
        })
    }

    /// Closing the iterator wakes its blocking signal-pipe wait. The worker
    /// drops its signal registration and reports completion before its final
    /// thread return is detached. This avoids an unbounded `JoinHandle::join`.
    pub fn close(&mut self, timeout: Duration) -> Result<(), DaemonError> {
        self.handle.close();
        let Some(join) = self.join.take() else {
            return Ok(());
        };
        match self.completion.recv_timeout(timeout) {
            Ok(result) => {
                drop(join);
                result
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.join = Some(join);
                Err(DaemonError::ShutdownTimeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                drop(join);
                Err(DaemonError::WorkerFailure)
            }
        }
    }
}

#[cfg(unix)]
impl Drop for SignalListener {
    fn drop(&mut self) {
        self.handle.close();
        // Normal shutdown consumes the completion notification first. On an
        // early runtime error, closing and detaching avoids an unbounded Drop.
        drop(self.join.take());
    }
}

#[cfg(unix)]
fn signal_thread(
    mut signals: signal_hook::iterator::SignalsInfo,
    coordinator: ShutdownCoordinator,
    completion: WorkerCompletion,
    completion_sender: mpsc::SyncSender<Result<(), DaemonError>>,
) {
    use signal_hook::consts::signal::{SIGINT, SIGTERM};

    let mut failure = None;
    for signal in signals.forever() {
        let reason = match signal {
            SIGINT => ShutdownReason::Sigint,
            SIGTERM => ShutdownReason::Sigterm,
            _ => continue,
        };
        if coordinator.request_shutdown(reason).is_err() {
            failure = Some(FailureReason::LifecyclePoisoned);
            break;
        }
    }
    drop(signals);
    let result = if let Some(reason) = failure {
        completion
            .fail(reason)
            .map_err(DaemonError::from)
            .map_err(|_| DaemonError::WorkerFailure)
    } else {
        completion
            .complete()
            .map_err(DaemonError::from)
            .map_err(|_| DaemonError::WorkerFailure)
    };
    let _ = completion_sender.send(result);
}

#[cfg(not(unix))]
pub struct SignalListener;

#[cfg(not(unix))]
impl SignalListener {
    pub fn start(_: ShutdownCoordinator) -> Result<Self, DaemonError> {
        Err(DaemonError::SignalHandlerInitialization)
    }

    pub fn close(&mut self, _: Duration) -> Result<(), DaemonError> {
        Ok(())
    }
}
