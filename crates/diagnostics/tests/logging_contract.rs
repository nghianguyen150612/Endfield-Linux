use std::fs;
use std::io::Write;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};

use endfield_unix_diagnostics::{
    Component, DiagnosticEvent, DiagnosticsError, EventCode, LinuxDiagnosticsEnvironment, LogStore,
    LogWorker, RecordDisposition, RetentionPolicy, Severity,
};
use tempfile::tempdir;

fn event(severity: Severity, timestamp: u64) -> DiagnosticEvent {
    DiagnosticEvent::new_at(
        severity,
        Component::Application,
        EventCode::AppStarting,
        timestamp,
    )
}

fn store_at(root: &std::path::Path) -> LogStore {
    LogStore::at_directory(root.join("state/endfield-unix/logs")).expect("log store")
}

#[test]
fn state_log_path_uses_xdg_state_home_without_changing_other_xdg_paths() {
    let environment = LinuxDiagnosticsEnvironment {
        home: Some(std::path::PathBuf::from("/home/example")),
        state_home: Some(std::path::PathBuf::from("/var/tmp/example-state")),
    };
    let paths = endfield_unix_diagnostics::DiagnosticsPaths::from_linux_environment(&environment)
        .expect("XDG state path");
    assert_eq!(
        paths.log_directory(),
        std::path::Path::new("/var/tmp/example-state/endfield-unix/logs")
    );

    let invalid_override = LinuxDiagnosticsEnvironment {
        home: Some(std::path::PathBuf::from("/home/example")),
        state_home: Some(std::path::PathBuf::from("relative/../state")),
    };
    let fallback =
        endfield_unix_diagnostics::DiagnosticsPaths::from_linux_environment(&invalid_override)
            .expect("safe home fallback");
    assert_eq!(
        fallback.log_directory(),
        std::path::Path::new("/home/example/.local/state/endfield-unix/logs")
    );
}

#[test]
fn event_schema_has_stable_codes_and_rejects_unapproved_fields_or_messages() {
    let encoded = serde_json::to_value(event(Severity::Info, 42)).expect("serialize event");
    assert_eq!(encoded["schema_version"], 1);
    assert_eq!(encoded["severity"], "INFO");
    assert_eq!(encoded["component"], "application");
    assert_eq!(encoded["code"], "APP_STARTING");
    assert_eq!(encoded["message"], "Application is starting.");

    let mut unsafe_message = encoded.clone();
    unsafe_message["message"] = serde_json::Value::String("token=synthetic-secret".to_owned());
    assert!(serde_json::from_value::<DiagnosticEvent>(unsafe_message).is_err());

    let mut arbitrary_field = encoded;
    arbitrary_field["command_line"] = serde_json::Value::String("game --password fake".to_owned());
    assert!(serde_json::from_value::<DiagnosticEvent>(arbitrary_field).is_err());

    assert!(
        event(Severity::Info, 42)
            .with_correlation_id("synthetic-correlation")
            .is_err()
    );
}

#[test]
fn event_size_limit_rejects_before_creating_log_storage() {
    let directory = tempdir().expect("temporary root");
    let policy = RetentionPolicy::new(64, 2, Duration::from_secs(60), 32).expect("valid limits");
    let store = LogStore::with_policy(directory.path().join("state/endfield-unix/logs"), policy)
        .expect("log store");

    assert_eq!(
        store.append_event(&event(Severity::Info, 1)),
        Err(DiagnosticsError::EventTooLarge)
    );
    assert!(!directory.path().join("state").exists());
}

#[test]
fn log_segments_rotate_before_the_size_bound_and_count_is_retained() {
    let directory = tempdir().expect("temporary root");
    let sample = serde_json::to_vec(&event(Severity::Info, 1)).expect("serialize event");
    let line_bytes = sample.len() + 1;
    let max_event = line_bytes;
    let max_segment = u64::try_from(line_bytes * 2 + line_bytes / 2).expect("segment limit");
    let policy = RetentionPolicy::new(
        max_segment,
        3,
        Duration::from_secs(7 * 24 * 60 * 60),
        max_event,
    )
    .expect("retention policy");
    let store = LogStore::with_policy(directory.path().join("state/endfield-unix/logs"), policy)
        .expect("log store");

    for timestamp in 0..20 {
        store
            .append_event(&event(Severity::Info, timestamp))
            .expect("append event");
    }

    let entries = fs::read_dir(store.directory())
        .expect("log directory")
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            name == "current.jsonl" || name.to_string_lossy().starts_with("segment-")
        })
        .collect::<Vec<_>>();
    assert!(entries.len() <= 3);
    assert!(
        entries
            .iter()
            .any(|entry| entry.file_name() != "current.jsonl")
    );
    for entry in entries {
        assert!(entry.metadata().expect("segment metadata").len() <= max_segment);
    }
}

#[test]
fn retention_removes_old_segments_without_touching_unrelated_files() {
    let directory = tempdir().expect("temporary root");
    let sample = serde_json::to_vec(&event(Severity::Info, 1)).expect("serialize event");
    let line_bytes = sample.len() + 1;
    let policy = RetentionPolicy::new(
        u64::try_from(line_bytes * 2).expect("segment limit"),
        5,
        Duration::from_secs(60),
        line_bytes,
    )
    .expect("retention policy");
    let store = LogStore::with_policy(directory.path().join("state/endfield-unix/logs"), policy)
        .expect("log store");
    store.initialize().expect("initialize");
    let unrelated = store.directory().join("notes.txt");
    fs::write(&unrelated, "preserve this file").expect("unrelated file");
    for timestamp in 0..8 {
        store
            .append_event(&event(Severity::Info, timestamp))
            .expect("append event");
    }

    store
        .enforce_retention_at(SystemTime::now() + Duration::from_secs(120))
        .expect("age retention");
    assert!(unrelated.exists());
    assert!(!store.directory().join("current.jsonl").exists());
    assert_eq!(
        fs::read(&unrelated).expect("preserved data"),
        b"preserve this file"
    );
}

#[cfg(unix)]
#[test]
fn log_directories_and_files_have_private_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary root");
    let store = store_at(directory.path());
    store
        .append_event(&event(Severity::Info, 1))
        .expect("append event");

    let log_mode = fs::metadata(store.directory())
        .expect("log directory metadata")
        .permissions()
        .mode()
        & 0o777;
    let namespace_mode = fs::metadata(store.directory().parent().expect("namespace"))
        .expect("namespace metadata")
        .permissions()
        .mode()
        & 0o777;
    let file_mode = fs::metadata(store.directory().join("current.jsonl"))
        .expect("current log metadata")
        .permissions()
        .mode()
        & 0o777;
    let lock_mode = fs::metadata(store.directory().join(".writer.lock"))
        .expect("lock metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(log_mode, 0o700);
    assert_eq!(namespace_mode, 0o700);
    assert_eq!(file_mode, 0o600);
    assert_eq!(lock_mode, 0o600);
}

#[cfg(unix)]
#[test]
fn current_log_symlink_is_refused_without_writing_to_its_target() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary root");
    let store = store_at(directory.path());
    store.initialize().expect("initialize");
    let target = directory.path().join("outside.txt");
    fs::write(&target, b"keep").expect("outside file");
    symlink(&target, store.directory().join("current.jsonl")).expect("symlink");

    assert!(matches!(
        store.append_event(&event(Severity::Info, 1)),
        Err(DiagnosticsError::UnsafePath(_))
    ));
    assert_eq!(fs::read(target).expect("target unchanged"), b"keep");
}

#[test]
fn concurrent_threads_share_the_cross_writer_lock_safely() {
    let directory = tempdir().expect("temporary root");
    let store = Arc::new(store_at(directory.path()));
    let threads = (0..4)
        .map(|worker| {
            let store = store.clone();
            thread::spawn(move || {
                for offset in 0..20 {
                    store
                        .append_event(&event(Severity::Info, worker * 100 + offset))
                        .expect("concurrent append");
                }
            })
        })
        .collect::<Vec<_>>();
    for handle in threads {
        handle.join().expect("writer thread");
    }

    let read = store.read_events().expect("read complete segments");
    assert_eq!(read.events.len(), 80);
    assert_eq!(read.omitted_records, 0);
}

#[test]
fn invalid_and_truncated_json_lines_are_omitted() {
    let directory = tempdir().expect("temporary root");
    let store = store_at(directory.path());
    store
        .append_event(&event(Severity::Info, 1))
        .expect("valid event");
    let mut current = fs::OpenOptions::new()
        .append(true)
        .open(store.directory().join("current.jsonl"))
        .expect("open current log");
    current
        .write_all(b"{\"schema_version\":99}\n{\"partial\"")
        .expect("append corrupt lines");

    let read = store.read_events().expect("read safe records");
    assert_eq!(read.events.len(), 1);
    assert!(read.omitted_records >= 2);
}

#[test]
fn oversized_legacy_segment_is_not_read_or_extended() {
    let directory = tempdir().expect("temporary root");
    let policy =
        RetentionPolicy::new(128, 3, Duration::from_secs(60), 64).expect("retention policy");
    let store = LogStore::with_policy(directory.path().join("state/endfield-unix/logs"), policy)
        .expect("log store");
    store.initialize().expect("initialize");
    fs::write(store.directory().join("current.jsonl"), vec![b'x'; 129])
        .expect("oversized segment fixture");

    let read = store.read_events().expect("read bounded log history");
    assert!(read.events.is_empty());
    assert_eq!(read.omitted_records, 1);
    assert_eq!(
        fs::metadata(store.directory().join("current.jsonl"))
            .expect("preserved log")
            .len(),
        129
    );
    assert!(store.append_event(&event(Severity::Info, 2)).is_err());
}

#[test]
fn failed_file_write_does_not_follow_or_overwrite_non_file_paths() {
    let directory = tempdir().expect("temporary root");
    let store = store_at(directory.path());
    store.initialize().expect("initialize");
    fs::create_dir(store.directory().join("current.jsonl")).expect("unsafe current path");

    assert!(matches!(
        store.append_event(&event(Severity::Error, 1)),
        Err(DiagnosticsError::UnsafePath(_))
    ));
    assert!(store.directory().join("current.jsonl").is_dir());
}

#[test]
fn severity_filter_and_worker_shutdown_are_bounded_and_deterministic() {
    let directory = tempdir().expect("temporary root");
    let store = store_at(directory.path());
    let worker =
        LogWorker::start_with_store(store.clone(), 8, Severity::Info).expect("logging worker");
    let handle = worker.handle();
    assert_eq!(
        handle.try_record(event(Severity::Debug, 1)),
        Ok(RecordDisposition::Filtered)
    );
    assert_eq!(
        handle.try_record(event(Severity::Info, 2)),
        Ok(RecordDisposition::Queued)
    );
    worker
        .shutdown(Duration::from_secs(2))
        .expect("shutdown worker");
    assert_eq!(handle.dropped_event_count(), 0);
    assert!(matches!(
        handle.try_record(event(Severity::Info, 3)),
        Err(DiagnosticsError::WorkerStopped)
    ));
    let read = store.read_events().expect("read stored events");
    assert_eq!(read.events.len(), 1);
    assert_eq!(read.events[0].severity(), Severity::Info);
}
