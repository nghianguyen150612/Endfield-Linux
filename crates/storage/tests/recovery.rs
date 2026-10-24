use endfield_unix_storage::{AppPaths, ConfigStore, Database, StorageError};
use rusqlite::Connection;
use tempfile::tempdir;

fn paths(root: &std::path::Path) -> AppPaths {
    AppPaths::from_base_dirs(root.join("config"), root.join("data"), root.join("cache"))
        .expect("absolute test paths")
}

#[test]
fn malformed_database_is_reported_and_never_recreated() {
    let directory = tempdir().expect("temporary directory");
    let app_paths = paths(directory.path());
    std::fs::create_dir_all(app_paths.data_dir()).expect("data directory");
    let path = app_paths.database_file();
    let corrupt = b"this is not a sqlite database\n";
    std::fs::write(&path, corrupt).expect("corrupt file");

    assert!(matches!(
        Database::initialize(&app_paths),
        Err(StorageError::DatabaseCorrupt { .. })
    ));
    assert_eq!(
        std::fs::read(path).expect("corrupt file preserved"),
        corrupt
    );
}

#[test]
fn failed_migration_rolls_back_and_keeps_the_existing_database() {
    let directory = tempdir().expect("temporary directory");
    let app_paths = paths(directory.path());
    std::fs::create_dir_all(app_paths.data_dir()).expect("data directory");
    let path = app_paths.database_file();
    let connection = Connection::open(&path).expect("create sqlite file");
    connection
        .execute_batch("CREATE TABLE storage_metadata (incompatible TEXT);")
        .expect("install conflicting preexisting table");
    drop(connection);

    assert!(matches!(
        Database::initialize(&app_paths),
        Err(StorageError::Migration { version: 1, .. })
    ));
    assert!(
        path.exists(),
        "failed migration must not delete the database"
    );

    let connection = Connection::open(path).expect("existing database remains readable");
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("version");
    assert_eq!(version, 0, "failed migration must not advance user_version");
    let table_exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='storage_metadata')",
            [],
            |row| row.get(0),
        )
        .expect("incompatible table remains");
    assert!(table_exists);
}

#[cfg(unix)]
#[test]
fn config_and_database_symlinks_are_rejected() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory");
    let app_paths = paths(directory.path());
    std::fs::create_dir_all(app_paths.data_dir()).expect("data directory");
    let external = directory.path().join("external.sqlite3");
    std::fs::write(&external, b"do not touch").expect("external target");
    symlink(&external, app_paths.database_file()).expect("database symlink");

    assert!(matches!(
        Database::initialize(&app_paths),
        Err(StorageError::InvalidPath { .. })
    ));
    assert_eq!(
        std::fs::read(external).expect("target unchanged"),
        b"do not touch"
    );
}

#[cfg(unix)]
#[test]
fn application_directory_symlink_is_rejected_without_writing_through_it() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory");
    let app_paths = paths(directory.path());
    let data_base = app_paths.data_dir().parent().expect("data base");
    std::fs::create_dir_all(data_base).expect("data base directory");
    let external = directory.path().join("external-data");
    std::fs::create_dir(&external).expect("external directory");
    let sentinel = external.join("keep.txt");
    std::fs::write(&sentinel, "untouched").expect("sentinel");
    symlink(&external, app_paths.data_dir()).expect("application directory symlink");

    assert!(matches!(
        Database::initialize(&app_paths),
        Err(StorageError::InvalidPath { .. })
    ));
    assert_eq!(
        std::fs::read_to_string(sentinel).expect("sentinel remains"),
        "untouched"
    );
    assert!(!external.join("state.sqlite3").exists());
}

#[cfg(unix)]
#[test]
fn config_load_refuses_to_follow_a_symlinked_application_directory() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory");
    let app_paths = paths(directory.path());
    let config_app_dir = app_paths.config_file().parent().expect("config app dir");
    std::fs::create_dir_all(config_app_dir.parent().expect("config base"))
        .expect("config base directory");
    let external = directory.path().join("external-config");
    std::fs::create_dir(&external).expect("external directory");
    std::fs::write(
        external.join("config.toml"),
        "schema_version = 1\n[ui]\ntheme = \"dark\"\n",
    )
    .expect("external config");
    symlink(&external, config_app_dir).expect("application directory symlink");

    assert!(matches!(
        ConfigStore::new(&app_paths).load(),
        Err(StorageError::InvalidPath { .. })
    ));
}

#[cfg(unix)]
#[test]
fn unreadable_config_and_database_report_permission_errors() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory");
    let app_paths = paths(directory.path());
    let config = ConfigStore::new(&app_paths);
    std::fs::create_dir_all(config.path().parent().expect("config dir")).expect("config dir");
    std::fs::write(config.path(), "schema_version = 1\n").expect("config file");
    std::fs::set_permissions(config.path(), std::fs::Permissions::from_mode(0o000))
        .expect("remove config permissions");
    assert!(matches!(
        config.load(),
        Err(StorageError::PermissionDenied { .. })
    ));

    std::fs::create_dir_all(app_paths.data_dir()).expect("data directory");
    let database_path = app_paths.database_file();
    std::fs::write(&database_path, b"").expect("empty database file");
    std::fs::set_permissions(&database_path, std::fs::Permissions::from_mode(0o000))
        .expect("remove database permissions");
    assert!(matches!(
        Database::open(&app_paths),
        Err(StorageError::PermissionDenied { .. })
    ));
}
