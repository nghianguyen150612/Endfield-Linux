use std::sync::{Arc, Barrier};
use std::thread;

use endfield_unix_storage::{
    AppConfig, AppPaths, CONFIG_SCHEMA_VERSION, ConfigStore, LanguagePreference,
    LinuxDirectoryEnvironment, StorageError, ThemePreference,
};
use tempfile::tempdir;

fn paths(root: &std::path::Path) -> AppPaths {
    AppPaths::from_base_dirs(root.join("config"), root.join("data"), root.join("cache"))
        .expect("absolute test paths")
}

#[test]
fn default_configuration_is_valid_and_round_trips() {
    let directory = tempdir().expect("temporary directory");
    let store = ConfigStore::new(&paths(directory.path()));
    let mut loaded = store.load().expect("default configuration");

    assert!(loaded.is_new());
    assert_eq!(loaded.config, AppConfig::default());
    assert_eq!(loaded.config.schema_version, CONFIG_SCHEMA_VERSION);
    loaded.config.ui.theme = ThemePreference::Dark;
    loaded.config.ui.language = LanguagePreference::English;
    store.save(&mut loaded).expect("save typed config");

    let reopened = store.load().expect("reopen saved config");
    assert_eq!(reopened.config, loaded.config);
    assert!(!reopened.is_new());
}

#[test]
fn missing_file_returns_defaults_without_creating_directories_or_file() {
    let directory = tempdir().expect("temporary directory");
    let app_paths = paths(directory.path());
    let store = ConfigStore::new(&app_paths);

    assert_eq!(store.load().expect("defaults").config, AppConfig::default());
    assert!(!app_paths.config_file().exists());
    assert!(!app_paths.config_file().parent().expect("parent").exists());
}

#[test]
fn malformed_toml_is_distinct_from_invalid_typed_values() {
    let directory = tempdir().expect("temporary directory");
    let store = ConfigStore::new(&paths(directory.path()));
    std::fs::create_dir_all(store.path().parent().expect("parent")).expect("config dir");
    std::fs::write(store.path(), "schema_version = [\n").expect("malformed config");
    assert!(matches!(
        store.load(),
        Err(StorageError::ConfigParse { .. })
    ));

    std::fs::write(store.path(), "schema_version = 1\n[ui]\ntheme = \"neon\"\n")
        .expect("invalid theme config");
    assert!(matches!(
        store.load(),
        Err(StorageError::InvalidConfigValue {
            field: "ui.theme",
            ..
        })
    ));
}

#[test]
fn absent_optional_preferences_use_defaults_but_invalid_values_fail() {
    let directory = tempdir().expect("temporary directory");
    let store = ConfigStore::new(&paths(directory.path()));
    std::fs::create_dir_all(store.path().parent().expect("parent")).expect("config dir");
    std::fs::write(store.path(), "schema_version = 1\n").expect("minimal config");
    assert_eq!(
        store.load().expect("optional values default").config,
        AppConfig::default()
    );

    std::fs::write(
        store.path(),
        "schema_version = 1\n[ui]\nlanguage = \"klingon\"\n",
    )
    .expect("invalid language");
    assert!(matches!(
        store.load(),
        Err(StorageError::InvalidConfigValue {
            field: "ui.language",
            ..
        })
    ));
}

#[test]
fn unsupported_version_is_preserved_and_never_downgraded() {
    let directory = tempdir().expect("temporary directory");
    let store = ConfigStore::new(&paths(directory.path()));
    std::fs::create_dir_all(store.path().parent().expect("parent")).expect("config dir");
    let original = b"schema_version = 99\nfuture_setting = \"keep me\"\n";
    std::fs::write(store.path(), original).expect("future config");

    assert!(matches!(
        store.load(),
        Err(StorageError::UnsupportedConfigVersion { found: 99, .. })
    ));
    assert_eq!(
        std::fs::read(store.path()).expect("preserved config"),
        original
    );
}

#[test]
fn stale_document_reports_conflict_instead_of_overwriting() {
    let directory = tempdir().expect("temporary directory");
    let store = ConfigStore::new(&paths(directory.path()));
    let mut first = store.initialize().expect("initialize");
    let mut stale = store.load().expect("second snapshot");

    first.config.ui.theme = ThemePreference::Dark;
    store.save(&mut first).expect("first update");
    stale.config.ui.language = LanguagePreference::English;
    assert!(matches!(
        store.save(&mut stale),
        Err(StorageError::ConcurrentUpdate { .. })
    ));
    assert_eq!(
        store.load().expect("winning config").config.ui.theme,
        ThemePreference::Dark
    );
}

#[test]
fn concurrent_first_writers_have_one_winner() {
    let directory = tempdir().expect("temporary directory");
    let app_paths = paths(directory.path());
    let store = ConfigStore::new(&app_paths);
    let mut a = store.load().expect("first missing snapshot");
    let mut b = store.load().expect("second missing snapshot");
    a.config.ui.theme = ThemePreference::Light;
    b.config.ui.theme = ThemePreference::Dark;
    let barrier = Arc::new(Barrier::new(3));

    let spawn = |mut document, barrier: Arc<Barrier>| {
        let store = store.clone();
        thread::spawn(move || {
            barrier.wait();
            store.save(&mut document)
        })
    };
    let first = spawn(a, Arc::clone(&barrier));
    let second = spawn(b, Arc::clone(&barrier));
    barrier.wait();

    let outcomes = [
        first.join().expect("first writer"),
        second.join().expect("second writer"),
    ];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(StorageError::ConcurrentUpdate { .. })))
            .count(),
        1
    );
}

#[test]
fn xdg_overrides_are_respected_and_invalid_relative_values_fall_back() {
    let environment = LinuxDirectoryEnvironment {
        home: Some("/home/tester".into()),
        config_home: Some("relative/config".into()),
        data_home: Some("/mnt/data".into()),
        cache_home: None,
    };
    let resolved = AppPaths::from_linux_environment(&environment).expect("resolve XDG paths");

    assert_eq!(
        resolved.config_file(),
        std::path::Path::new("/home/tester/.config/endfield-unix/config.toml")
    );
    assert_eq!(
        resolved.database_file(),
        std::path::Path::new("/mnt/data/endfield-unix/state.sqlite3")
    );
    assert_eq!(
        resolved.cache_dir(),
        std::path::Path::new("/home/tester/.cache/endfield-unix")
    );
}

#[test]
fn xdg_overrides_work_without_a_home_if_all_are_valid() {
    let environment = LinuxDirectoryEnvironment {
        home: None,
        config_home: Some("/one/config".into()),
        data_home: Some("/two/data".into()),
        cache_home: Some("/three/cache".into()),
    };
    assert!(AppPaths::from_linux_environment(&environment).is_ok());
}

#[test]
fn unrelated_files_are_untouched_and_private_modes_are_used() {
    let directory = tempdir().expect("temporary directory");
    let unrelated = directory.path().join("keep.txt");
    std::fs::write(&unrelated, "untouched").expect("unrelated file");
    let store = ConfigStore::new(&paths(directory.path()));
    let mut document = store.initialize().expect("config initialization");
    document.config.ui.theme = ThemePreference::Light;
    store.save(&mut document).expect("save config");

    assert_eq!(
        std::fs::read_to_string(unrelated).expect("unrelated file"),
        "untouched"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(store.path())
                .expect("config metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(store.path().parent().expect("config dir"))
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}
