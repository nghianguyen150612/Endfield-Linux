use std::time::Instant;

use endfield_unix_storage::{
    AppPaths, DATABASE_SCHEMA_VERSION, DEFAULT_BUSY_TIMEOUT, Database, Storage, StorageError,
};
use rusqlite::{Connection, TransactionBehavior};
use tempfile::tempdir;

fn paths(root: &std::path::Path) -> AppPaths {
    AppPaths::from_base_dirs(root.join("config"), root.join("data"), root.join("cache"))
        .expect("absolute test paths")
}

#[test]
fn initial_migration_is_persistent_idempotent_and_metadata_is_durable() {
    let directory = tempdir().expect("temporary directory");
    let app_paths = paths(directory.path());
    let database = Database::initialize(&app_paths).expect("initialize database");
    assert_eq!(
        database.schema_version().expect("schema version"),
        DATABASE_SCHEMA_VERSION
    );
    assert_eq!(
        database.get_metadata("storage_format").expect("format"),
        Some("endfield-unix".into())
    );
    assert!(
        database
            .set_metadata("storage_format", "replacement")
            .is_err()
    );
    assert_eq!(
        database
            .get_metadata("storage_format")
            .expect("format remains"),
        Some("endfield-unix".into())
    );
    database
        .set_metadata("test.persisted", "hello")
        .expect("persistent metadata write");
    assert!(
        database
            .foreign_keys_enabled()
            .expect("foreign key setting")
    );
    drop(database);

    let reopened = Database::open(&app_paths).expect("open current schema");
    assert_eq!(
        reopened.schema_version().expect("version persists"),
        DATABASE_SCHEMA_VERSION
    );
    assert_eq!(
        reopened
            .get_metadata("test.persisted")
            .expect("metadata persists"),
        Some("hello".into())
    );
}

#[test]
fn storage_facade_initializes_and_reopens_both_services() {
    let directory = tempdir().expect("temporary directory");
    let app_paths = paths(directory.path());
    let storage = Storage::initialize(&app_paths).expect("initialize facade");
    assert!(
        !storage
            .config()
            .load()
            .expect("initialized config")
            .is_new()
    );
    assert_eq!(
        storage.database().schema_version().expect("initialized db"),
        1
    );
    drop(storage);

    let reopened = Storage::open(&app_paths).expect("reopen facade");
    assert_eq!(reopened.database().schema_version().expect("schema"), 1);
    assert_eq!(
        reopened
            .config()
            .load()
            .expect("config loads")
            .config
            .schema_version,
        1
    );
}

#[test]
fn unsupported_future_version_is_rejected_without_modification() {
    let directory = tempdir().expect("temporary directory");
    let app_paths = paths(directory.path());
    drop(Database::initialize(&app_paths).expect("initialize"));
    let path = app_paths.database_file();
    let connection = Connection::open(&path).expect("direct test connection");
    connection
        .pragma_update(None, "user_version", 77_u32)
        .expect("set future version");
    drop(connection);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("mark future database permissions");
    }

    assert!(matches!(
        Database::open(&app_paths),
        Err(StorageError::UnsupportedDatabaseVersion { found: 77, .. })
    ));
    let connection = Connection::open(&path).expect("database retained");
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("version");
    assert_eq!(version, 77);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path)
                .expect("future database metadata")
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
    }
}

#[test]
fn foreign_keys_are_enabled_for_each_opened_connection() {
    let directory = tempdir().expect("temporary directory");
    let app_paths = paths(directory.path());
    let first = Database::initialize(&app_paths).expect("initialize");
    assert!(first.foreign_keys_enabled().expect("foreign keys"));
    drop(first);
    let second = Database::open(&app_paths).expect("reopen");
    assert!(
        second
            .foreign_keys_enabled()
            .expect("foreign keys after reopen")
    );
}

#[test]
fn metadata_transaction_rolls_back_a_failed_update() {
    let directory = tempdir().expect("temporary directory");
    let app_paths = paths(directory.path());
    let database = Database::initialize(&app_paths).expect("initialize");
    database
        .set_metadata("test.rollback", "before")
        .expect("initial value");
    let connection = Connection::open(database.path()).expect("test connection");
    connection
        .execute_batch(
            "CREATE TRIGGER reject_test_update BEFORE UPDATE ON storage_metadata
             WHEN NEW.key = 'test.rollback'
             BEGIN SELECT RAISE(ABORT, 'injected write failure'); END;",
        )
        .expect("install failure trigger");

    assert!(database.set_metadata("test.rollback", "after").is_err());
    assert_eq!(
        database
            .get_metadata("test.rollback")
            .expect("old value remains"),
        Some("before".into())
    );
}

#[test]
fn busy_database_returns_within_the_configured_timeout() {
    let directory = tempdir().expect("temporary directory");
    let app_paths = paths(directory.path());
    let database = Database::initialize(&app_paths).expect("initialize");
    let mut blocker = Connection::open(database.path()).expect("second sqlite connection");
    let transaction = blocker
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("acquire writer lock");

    let started = Instant::now();
    let error = database
        .set_metadata("test.busy", "write")
        .expect_err("writer is busy");
    let elapsed = started.elapsed();
    assert!(matches!(error, StorageError::DatabaseBusy { .. }));
    assert!(elapsed >= DEFAULT_BUSY_TIMEOUT);
    assert!(elapsed < DEFAULT_BUSY_TIMEOUT + std::time::Duration::from_secs(3));
    transaction.rollback().expect("release writer lock");
}
