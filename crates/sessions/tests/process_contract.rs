mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use endfield_unix_sessions::{
    ExitOutcome, MonitorStatus, SessionEvent, SessionState, SessionSupervisor,
};

#[test]
fn owned_linux_child_exits_are_reaped_and_classified() {
    for (command, expected) in [
        (0_u8, ExitOutcome::Exited { code: 0 }),
        (b'7', ExitOutcome::Exited { code: 7 }),
        (
            b's',
            ExitOutcome::Signaled {
                signal: libc::SIGABRT,
            },
        ),
    ] {
        let (supervisor, worker) = SessionSupervisor::new();
        let worker_thread = thread::spawn(move || worker.run(|_| {}).expect("monitor worker"));
        let mut fixture = common::FixtureChild::spawn();
        let (child, mut control) = fixture.transfer();
        let id = supervisor
            .register_owned_child(child)
            .expect("owned child registration");
        supervisor
            .wait_for_state(&id, SessionState::Running, Duration::from_secs(3))
            .expect("worker confirms running");
        control.write_all(&[command]).expect("release helper child");
        drop(control);
        let summary = supervisor
            .wait_for_state(&id, SessionState::Exited, Duration::from_secs(5))
            .expect("actual child exit observed");
        assert_eq!(summary.exit, Some(expected));
        assert_eq!(
            summary.process_id, None,
            "do not retain a stale PID after reap"
        );
        assert_eq!(summary.monitor_status, MonitorStatus::Complete);
        supervisor.request_shutdown().unwrap();
        worker_thread.join().expect("worker completion");
    }
}

#[test]
fn already_exited_owned_child_is_recorded_as_exited_during_startup() {
    let (supervisor, worker) = SessionSupervisor::new();
    let observed_events = Arc::new(Mutex::new(Vec::new()));
    let worker_events = Arc::clone(&observed_events);
    let worker_thread = thread::spawn(move || {
        worker
            .run(|event| worker_events.lock().unwrap().push(event))
            .expect("monitor worker")
    });
    // Fixed harmless OS utility only: no caller path or command text is used.
    let mut child = std::process::Command::new("true").spawn().unwrap();
    child.wait().unwrap();
    let id = supervisor.register_owned_child(child).unwrap();
    let summary = supervisor
        .wait_for_state(&id, SessionState::Exited, Duration::from_secs(3))
        .unwrap();
    assert_eq!(summary.exit, Some(ExitOutcome::Exited { code: 0 }));
    assert_eq!(
        *observed_events.lock().unwrap(),
        vec![SessionEvent::Registered, SessionEvent::Exited]
    );
    supervisor.request_shutdown().unwrap();
    worker_thread.join().unwrap();
}
