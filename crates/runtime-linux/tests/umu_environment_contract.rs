use endfield_unix_runtime::{EnvironmentChange, PrefixOwnership, ValueSensitivity};
use endfield_unix_runtime_linux::umu_environment::{
    NEUTRAL_GAME_ID, PrefixSelection, Store, build_environment,
};
use std::fs;
use std::path::PathBuf;

fn env_path() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "endfield-umu-env-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ))
}

#[test]
fn explicit_absolute_paths_neutral_game_id_and_conservative_controls_are_built() {
    let root = env_path();
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(root.join("prefix")).unwrap();
    let proton = root.join("GE-Proton10-26");
    fs::create_dir_all(&proton).unwrap();
    let prefix =
        PrefixSelection::new(root.join("prefix"), PrefixOwnership::ExternalUserManaged).unwrap();
    let changes = build_environment(Some(&proton), Some(&prefix), None, None).unwrap();
    let names: Vec<_> = changes
        .iter()
        .map(|change| change.name().as_str())
        .collect();
    assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        changes
            .iter()
            .filter(|c| c.name().as_str() == "GAMEID")
            .count(),
        1
    );
    assert_eq!(
        changes
            .iter()
            .find(|c| c.name().as_str() == "GAMEID")
            .unwrap()
            .value()
            .unwrap(),
        NEUTRAL_GAME_ID
    );
    assert_eq!(
        changes
            .iter()
            .find(|c| c.name().as_str() == "PROTONPATH")
            .unwrap()
            .value()
            .unwrap(),
        proton.as_os_str()
    );
    assert_eq!(
        changes
            .iter()
            .find(|c| c.name().as_str() == "WINEPREFIX")
            .unwrap()
            .value()
            .unwrap(),
        prefix.path().as_os_str()
    );
    assert_eq!(
        changes
            .iter()
            .find(|c| c.name().as_str() == "UMU_RUNTIME_UPDATE")
            .unwrap()
            .value()
            .unwrap(),
        "0"
    );
    assert_eq!(
        changes
            .iter()
            .find(|c| c.name().as_str() == "PROTON_VERB")
            .unwrap()
            .value()
            .unwrap(),
        "waitforexitandrun"
    );
    assert_eq!(
        changes
            .iter()
            .find(|c| c.name().as_str() == "PROTONFIXES_DISABLE")
            .unwrap()
            .value()
            .unwrap(),
        "1"
    );
    assert!(matches!(
        changes
            .iter()
            .find(|c| c.name().as_str() == "STORE")
            .unwrap(),
        EnvironmentChange::Remove { .. }
    ));
    assert!(changes.iter().filter(|c| matches!(c, EnvironmentChange::Set { name, .. } if name.as_str() == "PROTONPATH" || name.as_str() == "WINEPREFIX")).all(|c| c.sensitivity() == ValueSensitivity::Sensitive(endfield_unix_runtime::SensitiveValueKind::SensitivePath)));
    assert!(changes.iter().any(
        |c| matches!(c, EnvironmentChange::Remove { name } if name.as_str() == "UMU_NO_PROTON")
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_supported_game_store_values_are_preserved_and_invalid_inputs_rejected() {
    let root = env_path();
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(root.join("prefix")).unwrap();
    let proton = root.join("proton");
    fs::create_dir_all(&proton).unwrap();
    let prefix =
        PrefixSelection::new(root.join("prefix"), PrefixOwnership::ExternalUserManaged).unwrap();
    let changes = build_environment(
        Some(&proton),
        Some(&prefix),
        Some("umu-example"),
        Some(Store::Egs),
    )
    .unwrap();
    assert_eq!(
        changes
            .iter()
            .find(|c| c.name().as_str() == "GAMEID")
            .unwrap()
            .value()
            .unwrap(),
        "umu-example"
    );
    assert_eq!(
        changes
            .iter()
            .find(|c| c.name().as_str() == "STORE")
            .unwrap()
            .value()
            .unwrap(),
        "egs"
    );
    assert!(
        build_environment(
            Some(PathBuf::from("relative").as_path()),
            Some(&prefix),
            None,
            None
        )
        .is_err()
    );
    assert!(build_environment(Some(&proton), Some(&prefix), Some("bad\nvalue"), None).is_err());
    assert!(build_environment(Some(&proton), Some(&prefix), Some("/tmp/prefix"), None).is_err());
    assert!(build_environment(Some(&proton), Some(&prefix), Some("bad\0value"), None).is_err());
    assert!(
        build_environment(
            Some(&proton),
            Some(&prefix),
            Some(&"x".repeat(endfield_unix_runtime_linux::umu_environment::MAX_GAME_ID_BYTES + 1)),
            None
        )
        .is_err()
    );
    assert!(
        PrefixSelection::new(
            root.join("p/../escape"),
            PrefixOwnership::ExternalUserManaged
        )
        .is_err()
    );
    assert!(PrefixSelection::new(root.join("prefix"), PrefixOwnership::Unknown).is_err());
    assert!(build_environment(None, Some(&prefix), None, None).is_err());
    assert!(build_environment(Some(&proton), None, None, None).is_err());
    let _ = fs::remove_dir_all(root);
}
