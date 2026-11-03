use endfield_unix_game_sources::{
    self as sources, SourceCapability, SourceError, SourceVerificationStatus,
};

#[test]
fn reviewed_catalog_preserves_identity_urls_order_and_scope() {
    let catalog = sources::catalog();
    sources::validate_catalog(&catalog).unwrap();
    assert_eq!(
        catalog.sources.iter().map(|s| s.id).collect::<Vec<_>>(),
        ["epic", "gryphlink", "steam"]
    );
    assert_eq!(catalog.schema_version, 1);
    assert_eq!(catalog.live_availability, SourceVerificationStatus::Unknown);
    assert_eq!(sources::STEAM_APP_ID, "4732690");
    assert_eq!(
        sources::source("steam").unwrap().official_page,
        "https://store.steampowered.com/app/4732690/"
    );
    assert_eq!(
        sources::source("epic").unwrap().official_page,
        "https://store.epicgames.com/p/arknightsendfield-f0f167"
    );
    assert_eq!(
        sources::source("gryphlink").unwrap().official_page,
        "https://endfield.gryphline.com/"
    );
    for source in &catalog.sources {
        assert_eq!(source.publisher, "GRYPHLINE");
        assert_eq!(source.developer, "Hypergryph");
        assert!(source.official_page.starts_with("https://"));
        assert!(
            source
                .capabilities
                .contains(&SourceCapability::OfficialHandoff)
        );
        assert!(
            !source
                .capabilities
                .contains(&SourceCapability::VerifiedArtifactAcquisition)
        );
        assert!(source.unsupported_operations.contains(&"direct_download"));
        for evidence in &source.evidence {
            assert_eq!(evidence.observed_on, sources::OBSERVED_ON);
            assert!(evidence.url.starts_with("https://"));
        }
    }
    assert!(matches!(
        sources::source("mirror"),
        Err(SourceError::UnknownSource)
    ));
    assert!(serde_json::to_vec(&catalog).unwrap().len() < 32 * 1024);
    assert_eq!(
        serde_json::to_vec(&catalog).unwrap(),
        serde_json::to_vec(&sources::catalog()).unwrap()
    );
}
#[test]
fn capability_and_evidence_changes_fail_catalog_policy() {
    let mut value = sources::catalog();
    value.sources[0]
        .capabilities
        .push(SourceCapability::VerifiedArtifactAcquisition);
    assert!(sources::validate_catalog(&value).is_err());
    let mut value = sources::catalog();
    value.sources[0].channel = value.sources[2].channel;
    assert!(sources::validate_catalog(&value).is_err());
    let mut value = sources::catalog();
    value.sources[2].evidence[0].claim = "download API";
    assert!(sources::validate_catalog(&value).is_err());
    let mut value = sources::catalog();
    value.schema_version = 99;
    assert!(sources::validate_catalog(&value).is_err());
}

#[cfg(target_os = "linux")]
mod steam {
    use super::*;
    use std::{
        fs,
        os::unix::fs::symlink,
        path::{Path, PathBuf},
    };
    fn fixture(root: &Path, id: &str, dir: &str) -> PathBuf {
        fs::create_dir_all(root.join("steamapps/common/Unfixed Folder Δ")).unwrap();
        let path = root.join("steamapps/appmanifest_4732690.acf");
        fs::write(&path, format!("\"AppState\" {{ \"appid\" \"{id}\" \"installdir\" \"{dir}\" \"StateFlags\" \"4\" \"buildid\" \"12345\" \"InstalledDepots\" {{ \"123\" {{ \"manifest\" \"987\" }} }} \"LastOwner\" \"PRIVATE-ACCOUNT-FIXTURE\" }}")).unwrap();
        path
    }
    #[test]
    fn selected_library_and_manifest_are_equivalent_but_never_authenticate_payloads() {
        let temp = tempfile::tempdir().unwrap();
        let path = fixture(temp.path(), "4732690", "Unfixed Folder Δ");
        let a = sources::inspect_steam(temp.path()).unwrap();
        let b = sources::inspect_steam(&path).unwrap();
        assert_eq!(a, b);
        assert!(a.installation_present);
        assert_eq!(a.reported_build_id.as_deref(), Some("12345"));
        assert_eq!(a.status, SourceVerificationStatus::ObservedLocalMetadata);
        assert_eq!(
            a.publisher_authenticity,
            SourceVerificationStatus::Unverifiable
        );
        assert_eq!(a.artifact_integrity, SourceVerificationStatus::Unverifiable);
        assert_eq!(a.game_build, SourceVerificationStatus::Unknown);
        assert_eq!(a.game_compatibility, "not_tested");
        assert!(
            !serde_json::to_string(&a)
                .unwrap()
                .contains("PRIVATE-ACCOUNT-FIXTURE")
        );
        assert_eq!(
            sources::revalidate_steam(&path, &a.observation_token)
                .unwrap()
                .status,
            a.status
        );
        fs::write(
            temp.path()
                .join("steamapps/common/Unfixed Folder Δ/game.bin"),
            "payload changed",
        )
        .unwrap();
        // Directory content and game-byte integrity are deliberately outside the token.
        assert_eq!(
            sources::revalidate_steam(&path, &a.observation_token)
                .unwrap()
                .status,
            a.status
        );
    }
    #[test]
    fn wrong_app_display_name_and_nested_app_id_cannot_establish_identity() {
        let temp = tempfile::tempdir().unwrap();
        let path = fixture(temp.path(), "1", "Unfixed Folder Δ");
        assert!(matches!(
            sources::inspect_steam(&path),
            Err(SourceError::WrongApp)
        ));
        fs::write(&path,r#""AppState" { "name" "Arknights: Endfield" "installdir" "Unfixed Folder Δ" "Nested" { "appid" "4732690" } }"#).unwrap();
        assert!(matches!(
            sources::inspect_steam(&path),
            Err(SourceError::WrongApp)
        ));
    }
    #[test]
    fn malformed_truncated_duplicate_and_oversized_metadata_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let path = fixture(temp.path(), "4732690", "Unfixed Folder Δ");
        for bad in [
            r#""AppState" { "appid" "4732690" "installdir" "x" "#,
            r#""AppState" { "appid" "4732690" "APPID" "1" "installdir" "x" }"#,
            r#""AppState" { "appid" { "nested" "4732690" } "installdir" "x" }"#,
            r#""AppState" { "appid" "4732690" "installdir" "x" "buildid" "execute me" }"#,
            r#""AppState" { "appid" "4732690" "installdir" "x" } "AppState" {}"#,
            r#"#include "credentials""#,
        ] {
            fs::write(&path, bad).unwrap();
            assert!(sources::inspect_steam(&path).is_err(), "{bad}");
        }
        fs::write(&path, vec![b' '; 65537]).unwrap();
        assert!(matches!(
            sources::inspect_steam(&path),
            Err(SourceError::InvalidMetadata)
        ));
    }
    #[test]
    fn missing_and_partial_are_explicit_and_unknown_build_is_not_invented() {
        let temp = tempfile::tempdir().unwrap();
        assert!(matches!(
            sources::inspect_steam(temp.path()),
            Err(SourceError::Missing)
        ));
        let path = fixture(temp.path(), "4732690", "not installed");
        let report = sources::inspect_steam(&path).unwrap();
        assert!(!report.installation_present);
        fs::write(
            &path,
            r#""AppState" { "appid" "4732690" "installdir" "Unfixed Folder Δ" }"#,
        )
        .unwrap();
        let report = sources::inspect_steam(&path).unwrap();
        assert!(report.reported_build_id.is_none());
        assert!(report.reported_state_flags.is_none());
        assert_eq!(report.game_build, SourceVerificationStatus::Unknown);
    }
    #[test]
    fn traversal_absolute_and_windows_paths_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        for dir in [
            "..",
            "../escape",
            "/absolute",
            "C:/game",
            "a/b",
            "a\\\\b",
            ".",
            "",
        ] {
            let path = fixture(temp.path(), "4732690", dir);
            assert!(sources::inspect_steam(&path).is_err(), "{dir}");
        }
        assert!(sources::inspect_steam(Path::new("relative")).is_err());
        assert!(sources::inspect_steam(&temp.path().join("../alias")).is_err());
    }
    #[test]
    fn symlink_and_hardlink_metadata_and_directory_escapes_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let path = fixture(temp.path(), "4732690", "Unfixed Folder Δ");
        let saved = temp.path().join("saved");
        fs::rename(&path, &saved).unwrap();
        symlink(&saved, &path).unwrap();
        assert!(sources::inspect_steam(&path).is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&saved, &path).unwrap();
        assert!(sources::inspect_steam(&path).is_err());
        fs::remove_file(&path).unwrap();
        fs::rename(&saved, &path).unwrap();
        let dir = temp.path().join("steamapps/common/Unfixed Folder Δ");
        fs::remove_dir(&dir).unwrap();
        symlink(temp.path(), &dir).unwrap();
        assert!(sources::inspect_steam(&path).is_err());
        let alias = temp.path().join("alias");
        symlink(temp.path().join("steamapps"), &alias).unwrap();
        assert!(sources::inspect_steam(&alias.join("appmanifest_4732690.acf")).is_err());
    }
    #[test]
    fn changed_metadata_missing_evidence_and_directory_replacement_are_detected() {
        let temp = tempfile::tempdir().unwrap();
        let path = fixture(temp.path(), "4732690", "Unfixed Folder Δ");
        let a = sources::inspect_steam(&path).unwrap();
        let bytes = fs::read(&path).unwrap();
        let mut changed = bytes.clone();
        changed.push(b'\n');
        fs::write(&path, changed).unwrap();
        assert_eq!(
            sources::revalidate_steam(&path, &a.observation_token)
                .unwrap()
                .status,
            SourceVerificationStatus::Changed
        );
        let a = sources::inspect_steam(&path).unwrap();
        let dir = temp.path().join("steamapps/common/Unfixed Folder Δ");
        fs::rename(&dir, temp.path().join("old")).unwrap();
        fs::create_dir(&dir).unwrap();
        assert_eq!(
            sources::revalidate_steam(&path, &a.observation_token)
                .unwrap()
                .status,
            SourceVerificationStatus::Changed
        );
        fs::remove_file(&path).unwrap();
        assert!(matches!(
            sources::revalidate_steam(&path, &a.observation_token),
            Err(SourceError::Missing)
        ));
        assert!(sources::revalidate_steam(&path, "invalid").is_err());
    }
    #[test]
    fn selection_never_scans_other_candidate_libraries() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("one");
        let b = temp.path().join("two");
        fixture(&a, "4732690", "Unfixed Folder Δ");
        fixture(&b, "1", "Unfixed Folder Δ");
        assert!(matches!(
            sources::inspect_steam(temp.path()),
            Err(SourceError::Missing)
        ));
        assert!(sources::inspect_steam(&a).is_ok());
        assert!(sources::inspect_steam(&b).is_err());
    }
}
