mod common;

use std::io::Write;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use endfield_unix_sessions::{MonitorStatus, SessionState, SessionSupervisor};

#[test]
fn shutdown_cancels_monitoring_without_signalling_owned_processes() {
    let (supervisor, worker) = SessionSupervisor::new();
    let worker_thread = thread::spawn(move || worker.run(|_| {}).expect("monitor worker"));
    let mut fixture = common::FixtureChild::spawn();
    let (child, mut control) = fixture.transfer();
    let id = supervisor.register_owned_child(child).unwrap();
    let summary = supervisor
        .wait_for_state(&id, SessionState::Running, Duration::from_secs(3))
        .unwrap();
    let pid = summary.process_id.unwrap();

    supervisor.request_shutdown().unwrap();
    worker_thread.join().expect("monitoring worker completed");
    let summary = supervisor.get(&id).unwrap();
    assert_eq!(
        summary.state,
        SessionState::Running,
        "daemon shutdown is not a game stop"
    );
    assert_eq!(summary.monitor_status, MonitorStatus::Cancelled);
    assert!(
        std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "running helper remains alive after monitoring stops"
    );

    control.write_all(&[0]).unwrap();
    drop(control);
    // The parent test process still owns the helper's process relationship and
    // reaps it after the supervisor deliberately drops its monitoring handle.
    fixture.pid = pid;
}

#[test]
fn child_exit_racing_supervisor_shutdown_has_one_truthful_terminal_observation() {
    let (supervisor, worker) = SessionSupervisor::new();
    let worker_thread = thread::spawn(move || worker.run(|_| {}).expect("monitor worker"));
    let mut fixture = common::FixtureChild::spawn();
    let (child, writer) = fixture.transfer();
    let id = supervisor.register_owned_child(child).unwrap();
    supervisor
        .wait_for_state(&id, SessionState::Running, Duration::from_secs(3))
        .unwrap();

    let barrier = Arc::new(Barrier::new(3));
    let exit_barrier = Arc::clone(&barrier);
    let exit_thread = thread::spawn(move || {
        let mut writer = writer;
        exit_barrier.wait();
        let _ = writer.write_all(&[0]);
    });
    let stop_barrier = Arc::clone(&barrier);
    let stop_supervisor = supervisor.clone();
    let stop_thread = thread::spawn(move || {
        stop_barrier.wait();
        stop_supervisor.request_shutdown().unwrap();
    });
    barrier.wait();
    exit_thread.join().unwrap();
    stop_thread.join().unwrap();
    worker_thread.join().expect("shutdown completes once");

    let summary = supervisor.get(&id).unwrap();
    match summary.state {
        SessionState::Exited => {
            assert!(summary.exit.is_some());
            assert_eq!(summary.process_id, None);
        }
        SessionState::Running => {
            assert_eq!(
                summary.exit, None,
                "no exit may be fabricated after monitoring stops"
            );
            assert_eq!(summary.monitor_status, MonitorStatus::Cancelled);
        }
        state => panic!("unexpected raced lifecycle result: {state:?}"),
    }
}
