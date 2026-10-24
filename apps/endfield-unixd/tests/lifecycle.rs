use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use endfield_unixd::lifecycle::{
    FailureReason, LifecycleError, LifecycleState, ShutdownCoordinator, ShutdownReason,
};
use tempfile::TempDir;

#[test]
fn starts_running_shutdown_and_stopped_follow_the_validated_lifecycle() {
    let coordinator = ShutdownCoordinator::new();
    assert_eq!(
        coordinator.snapshot().unwrap().state,
        LifecycleState::Starting
    );
    coordinator.mark_running().expect("successful startup");
    assert_eq!(
        coordinator.snapshot().unwrap().state,
        LifecycleState::Running
    );
    assert!(
        coordinator
            .request_shutdown(ShutdownReason::Requested)
            .expect("request stop")
    );
    assert_eq!(
        coordinator.snapshot().unwrap().state,
        LifecycleState::Stopping
    );
    coordinator.complete_shutdown().expect("complete shutdown");
    assert_eq!(
        coordinator.snapshot().unwrap().state,
        LifecycleState::Stopped
    );
    coordinator
        .complete_shutdown()
        .expect("stopped completion is idempotent");
}

#[test]
fn startup_failure_is_typed_and_preserved() {
    let coordinator = ShutdownCoordinator::new();
    assert!(coordinator.fail(FailureReason::WorkerStartup).unwrap());
    assert_eq!(
        coordinator.snapshot().unwrap().state,
        LifecycleState::Failed(FailureReason::WorkerStartup)
    );
    assert!(!coordinator.fail(FailureReason::ResourceCleanup).unwrap());

    let stopping = ShutdownCoordinator::new();
    stopping.mark_running().expect("enter running");
    stopping
        .request_shutdown(ShutdownReason::Requested)
        .expect("request stop");
    assert!(stopping.fail(FailureReason::ResourceCleanup).unwrap());
    assert_eq!(
        stopping.snapshot().unwrap().state,
        LifecycleState::Failed(FailureReason::ResourceCleanup)
    );
}

#[test]
fn invalid_transitions_are_rejected_and_stopped_is_terminal() {
    let coordinator = ShutdownCoordinator::new();
    assert_eq!(
        coordinator.complete_shutdown(),
        Err(LifecycleError::InvalidTransition)
    );
    coordinator.mark_running().expect("enter running");
    coordinator
        .request_shutdown(ShutdownReason::Sigterm)
        .expect("request stop");
    coordinator.complete_shutdown().expect("stop");
    assert_eq!(
        coordinator.mark_running(),
        Err(LifecycleError::InvalidTransition)
    );
    assert_eq!(
        coordinator.fail(FailureReason::ResourceCleanup),
        Err(LifecycleError::InvalidTransition)
    );
}

#[test]
fn startup_shutdown_is_defined_and_repeated_requests_are_idempotent() {
    let coordinator = ShutdownCoordinator::new();
    assert!(
        coordinator
            .request_shutdown(ShutdownReason::Sigint)
            .unwrap()
    );
    assert_eq!(
        coordinator.snapshot().unwrap().state,
        LifecycleState::Stopping
    );
    assert_eq!(
        coordinator.snapshot().unwrap().shutdown_reason,
        Some(ShutdownReason::Sigint)
    );
    assert_eq!(
        coordinator.mark_running(),
        Err(LifecycleError::InvalidTransition)
    );
    assert!(
        !coordinator
            .request_shutdown(ShutdownReason::Sigterm)
            .unwrap()
    );
    coordinator
        .complete_shutdown()
        .expect("finish startup stop");
}

#[test]
fn concurrent_stop_requests_accept_exactly_one_cleanup_owner() {
    let coordinator = ShutdownCoordinator::new();
    coordinator.mark_running().expect("enter running");
    let requests = (0..32)
        .map(|index| {
            let coordinator = coordinator.clone();
            thread::spawn(move || {
                let reason = if index % 2 == 0 {
                    ShutdownReason::Sigterm
                } else {
                    ShutdownReason::Sigint
                };
                coordinator.request_shutdown(reason).unwrap()
            })
        })
        .collect::<Vec<_>>();
    let accepted = requests
        .into_iter()
        .map(|request| request.join().expect("stop requester"))
        .filter(|accepted| *accepted)
        .count();
    assert_eq!(accepted, 1);
    assert_eq!(
        coordinator.snapshot().unwrap().state,
        LifecycleState::Stopping
    );
}

#[test]
fn shutdown_completion_is_observable_without_polling() {
    let coordinator = ShutdownCoordinator::new();
    coordinator.mark_running().expect("enter running");
    let waiter = coordinator.clone();
    let thread = thread::spawn(move || waiter.wait_for_shutdown_or_failure().unwrap());
    coordinator
        .request_shutdown(ShutdownReason::Requested)
        .unwrap();
    assert_eq!(thread.join().unwrap().state, LifecycleState::Stopping);
}

#[test]
fn controlled_worker_failure_becomes_a_typed_lifecycle_failure() {
    let coordinator = ShutdownCoordinator::new();
    coordinator.mark_running().expect("enter running");
    let completion = coordinator.register_worker().expect("register worker");
    completion
        .fail(FailureReason::WorkerFailure)
        .expect("report worker failure");
    assert_eq!(
        coordinator.snapshot().unwrap().state,
        LifecycleState::Failed(FailureReason::WorkerFailure)
    );
    assert_eq!(
        coordinator.wait_for_workers(Duration::from_secs(1)),
        Err(LifecycleError::WorkerFailed(FailureReason::WorkerFailure))
    );
}

#[test]
fn worker_timeout_is_reported_without_claiming_cancellation() {
    let coordinator = ShutdownCoordinator::new();
    coordinator.mark_running().expect("enter running");
    let completion = coordinator.register_worker().expect("register worker");
    coordinator
        .request_shutdown(ShutdownReason::Requested)
        .unwrap();
    let (release, wait) = mpsc::channel();
    let thread = thread::spawn(move || {
        wait.recv().expect("test releases controlled worker");
        completion.complete().expect("worker completion");
    });

    assert_eq!(
        coordinator.wait_for_workers(Duration::from_millis(20)),
        Err(LifecycleError::ShutdownTimeout)
    );
    assert_eq!(coordinator.snapshot().unwrap().active_workers, 1);
    release.send(()).expect("release controlled worker");
    thread.join().expect("reap controlled worker");
    coordinator
        .wait_for_workers(Duration::from_secs(1))
        .expect("worker completes after timeout");
    coordinator
        .complete_shutdown()
        .expect("finish after worker");
}

#[cfg(target_os = "linux")]
#[test]
fn real_daemon_handles_sigterm_and_sigint_headlessly() {
    for signal in [libc::SIGTERM, libc::SIGINT] {
        let mut daemon = TestDaemon::start();
        daemon.wait_for_code("DAEMON_RUNNING", Duration::from_secs(4));
        daemon.assert_idle_cpu_ticks_do_not_increase(Duration::from_millis(300));
        daemon.send_signal(signal);
        let status = daemon.wait_for_exit(Duration::from_secs(4));
        assert_eq!(status.code(), Some(0), "orderly signal exit status");
        let log = daemon.read_log();
        for code in [
            "DAEMON_STARTING",
            "DAEMON_RUNNING",
            "DAEMON_SHUTDOWN_REQUESTED",
            "DAEMON_STOPPED",
        ] {
            assert_eq!(log.matches(code).count(), 1, "expected one {code} event");
        }
        assert!(!log.contains("synthetic-private-detail"));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn repeated_real_shutdown_signals_do_not_repeat_cleanup_or_leave_a_child() {
    let mut daemon = TestDaemon::start();
    daemon.wait_for_code("DAEMON_RUNNING", Duration::from_secs(4));
    daemon.send_signal(libc::SIGTERM);
    daemon.send_signal_if_still_running(libc::SIGINT);
    let status = daemon.wait_for_exit(Duration::from_secs(4));
    assert_eq!(status.code(), Some(0));
    let log = daemon.read_log();
    assert_eq!(log.matches("DAEMON_SHUTDOWN_REQUESTED").count(), 1);
    assert_eq!(log.matches("DAEMON_STOPPED").count(), 1);
    assert!(
        daemon
            .child
            .try_wait()
            .expect("check reaped child")
            .is_some()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn unavailable_optional_diagnostics_does_not_prevent_signal_shutdown() {
    let mut daemon = TestDaemon::start_with_unavailable_diagnostics();
    thread::sleep(Duration::from_millis(100));
    assert!(
        daemon
            .child
            .try_wait()
            .expect("daemon still running")
            .is_none()
    );
    daemon.send_signal(libc::SIGTERM);
    assert_eq!(daemon.wait_for_exit(Duration::from_secs(4)).code(), Some(0));
    let stderr = daemon.read_stderr();
    assert!(stderr.contains("persistent diagnostics unavailable"));
    assert_eq!(
        stderr.matches("persistent diagnostics unavailable").count(),
        1
    );
    assert!(!stderr.contains(daemon.state_home.to_str().unwrap()));
}

#[cfg(target_os = "linux")]
struct TestDaemon {
    child: Child,
    _directory: TempDir,
    state_home: PathBuf,
}

#[cfg(target_os = "linux")]
impl TestDaemon {
    fn start() -> Self {
        Self::start_inner(false)
    }

    fn start_with_unavailable_diagnostics() -> Self {
        Self::start_inner(true)
    }

    fn start_inner(unavailable_diagnostics: bool) -> Self {
        let directory = tempfile::tempdir().expect("isolated daemon test directory");
        let state_home = directory.path().join("state");
        if unavailable_diagnostics {
            fs::write(&state_home, b"not a directory").expect("block log directory creation");
        }
        let child = Command::new(env!("CARGO_BIN_EXE_endfield-unixd"))
            .env_clear()
            .env("HOME", directory.path().join("home"))
            .env("XDG_STATE_HOME", &state_home)
            .arg("--foreground")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start real daemon child");
        Self {
            child,
            _directory: directory,
            state_home,
        }
    }

    fn log_path(&self) -> PathBuf {
        self.state_home.join("endfield-unix/logs/current.jsonl")
    }

    fn read_log(&self) -> String {
        fs::read_to_string(self.log_path()).unwrap_or_default()
    }

    fn read_stderr(&mut self) -> String {
        let mut output = String::new();
        self.child
            .stderr
            .take()
            .expect("captured daemon stderr")
            .read_to_string(&mut output)
            .expect("read daemon stderr");
        output
    }

    fn wait_for_code(&mut self, code: &str, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            if self.read_log().contains(code) {
                return;
            }
            if let Some(status) = self.child.try_wait().expect("check daemon startup") {
                panic!("daemon exited before {code} was logged: {status}");
            }
            assert!(Instant::now() < deadline, "timed out waiting for {code}");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn send_signal(&mut self, signal: i32) {
        assert!(
            self.child
                .try_wait()
                .expect("daemon still active")
                .is_none(),
            "daemon exited before signal delivery"
        );
        let pid = i32::try_from(self.child.id()).expect("pid fits pid_t");
        // SAFETY: pid is the still-running child owned by this test.
        let result = unsafe { libc::kill(pid, signal) };
        assert_eq!(result, 0, "send signal to test child");
    }

    fn send_signal_if_still_running(&mut self, signal: i32) {
        if self.child.try_wait().expect("check child").is_none() {
            let pid = i32::try_from(self.child.id()).expect("pid fits pid_t");
            // SAFETY: pid is the still-running child owned by this test.
            let _ = unsafe { libc::kill(pid, signal) };
        }
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> std::process::ExitStatus {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait().expect("poll daemon exit") {
                return status;
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                panic!("daemon did not exit before the bounded test timeout");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn assert_idle_cpu_ticks_do_not_increase(&self, observation: Duration) {
        let pid = self.child.id();
        let before = process_cpu_ticks(pid).expect("read initial process CPU ticks");
        thread::sleep(observation);
        let after = process_cpu_ticks(pid).expect("read final process CPU ticks");
        assert_eq!(before, after, "idle daemon consumed CPU ticks");
    }
}

#[cfg(target_os = "linux")]
impl Drop for TestDaemon {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(target_os = "linux")]
fn process_cpu_ticks(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat")).ok()?;
    let close = stat.rfind(')')?;
    let fields = stat
        .get(close + 2..)?
        .split_whitespace()
        .collect::<Vec<_>>();
    let user = fields.get(11)?.parse::<u64>().ok()?;
    let system = fields.get(12)?.parse::<u64>().ok()?;
    Some(user.saturating_add(system))
}
