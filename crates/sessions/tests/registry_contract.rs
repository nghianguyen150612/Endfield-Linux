mod common;

use std::io::Write;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use endfield_unix_sessions::{
    MAX_ACTIVE_SESSIONS, SessionError, SessionId, SessionState, SessionSupervisor,
};

#[test]
fn identifiers_are_validated_opaque_and_sampling_contexts_are_separate() {
    let a = SessionId::new("0123456789abcdef0123456789abcdef").unwrap();
    let b = SessionId::new("ffffffffffffffffffffffffffffffff").unwrap();
    assert_ne!(a, b);
    assert_eq!(format!("{a:?}"), "SessionId(\"[opaque]\")");
    for invalid in [
        "",
        "abc",
        "0123456789ABCDEF0123456789ABCDEF",
        "z123456789abcdef0123456789abcdef",
    ] {
        assert_eq!(SessionId::new(invalid), Err(SessionError::InvalidSessionId));
    }
}

#[test]
fn concurrent_registrations_are_bounded_and_shutdown_rejects_more() {
    let (supervisor, worker) = SessionSupervisor::new();
    let worker_thread = thread::spawn(move || worker.run(|_| {}).expect("monitor worker"));

    let mut fixtures = (0..MAX_ACTIVE_SESSIONS)
        .map(|_| common::FixtureChild::spawn())
        .collect::<Vec<_>>();
    let (sender, receiver) = mpsc::channel();
    let mut writers = Vec::new();
    let mut threads = Vec::new();
    for fixture in &mut fixtures {
        let (child, writer) = fixture.transfer();
        writers.push(writer);
        let sender = sender.clone();
        let client = supervisor.clone();
        threads.push(thread::spawn(move || {
            let id = client
                .register_owned_child(child)
                .map_err(|failure| failure.error);
            sender.send(id).unwrap();
        }));
    }
    drop(sender);
    let mut ids = Vec::new();
    for result in receiver.iter() {
        ids.push(result.expect("trusted registration succeeds at limit"));
    }
    for handle in threads {
        handle.join().expect("registration thread");
    }
    assert_eq!(ids.len(), MAX_ACTIVE_SESSIONS);
    assert_eq!(supervisor.list().unwrap().len(), MAX_ACTIVE_SESSIONS);
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        MAX_ACTIVE_SESSIONS
    );
    assert!(ids.iter().all(|id| {
        id.as_str().len() == 32
            && id
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }));
    let contexts = supervisor
        .list()
        .unwrap()
        .into_iter()
        .map(|summary| summary.sampling_context.get())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(contexts.len(), MAX_ACTIVE_SESSIONS);
    for id in &ids {
        supervisor
            .wait_for_state(id, SessionState::Running, Duration::from_secs(3))
            .unwrap();
    }

    let mut overflow = common::FixtureChild::spawn();
    let (child, writer) = overflow.transfer();
    overflow.writer = Some(writer);
    let failure = supervisor.register_owned_child(child).unwrap_err();
    assert_eq!(failure.error, SessionError::Capacity);
    overflow.child = Some(failure.child);

    for writer in &mut writers {
        let _ = writer.write_all(&[0]);
    }
    drop(writers);
    for id in &ids {
        let summary = supervisor
            .wait_for_state(id, SessionState::Exited, Duration::from_secs(5))
            .unwrap();
        assert!(summary.exit.is_some());
    }
    supervisor.request_shutdown().unwrap();
    assert_eq!(
        supervisor
            .register_owned_child(common::FixtureChild::spawn().child.take().unwrap())
            .unwrap_err()
            .error,
        SessionError::ShuttingDown
    );
    worker_thread.join().expect("supervisor stops once");
    drop(overflow);
}

#[test]
fn client_closure_does_not_release_supervisor_owned_child() {
    let (client, worker) = SessionSupervisor::new();
    let worker_thread = thread::spawn(move || worker.run(|_| {}).expect("monitor worker"));
    let daemon_supervisor = client.clone();
    let mut fixture = common::FixtureChild::spawn();
    let (child, mut writer) = fixture.transfer();
    let id = client.register_owned_child(child).unwrap();
    let pid = client
        .wait_for_state(&id, SessionState::Running, Duration::from_secs(3))
        .unwrap()
        .process_id
        .unwrap();
    drop(client);
    assert_eq!(
        daemon_supervisor.get(&id).unwrap().state,
        SessionState::Running
    );
    writer.write_all(&[0]).unwrap();
    drop(writer);
    let summary = daemon_supervisor
        .wait_for_state(&id, SessionState::Exited, Duration::from_secs(3))
        .unwrap();
    assert_eq!(summary.process_id, None);
    assert_eq!(
        summary.exit,
        Some(endfield_unix_sessions::ExitOutcome::Exited { code: 0 })
    );
    assert_ne!(pid, 0);
    daemon_supervisor.request_shutdown().unwrap();
    worker_thread.join().unwrap();
}
