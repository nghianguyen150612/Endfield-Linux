use super::tests::fixture;
use super::*;
use endfield_unix_runtime_selection::{PinStore, RuntimePin, RuntimePinScope};

#[test]
fn saved_pin_requires_approval_revalidation_and_cas_with_journal_recovery() {
    for fault in ["pin-intent", "pin-committed"] {
        let (tmp, mut b, id, scope, rev) = fixture();
        let m = b.create(&id, &scope, rev, true).unwrap();
        let pins = PinStore::new(tmp.path().join("pins/runtime-pins.json"));
        let mut assoc = m.source.runtime.clone();
        assoc.runtime = match assoc.runtime {
            crate::PinnedRuntimeReference::Managed(mut r) => {
                r.build = "changed-pin".into();
                r.into()
            }
            _ => unreachable!(),
        };
        let machine = match &scope {
            PrefixScope::Machine { machine } => machine.clone(),
            _ => unreachable!(),
        };
        let scope_pin = RuntimePinScope::Machine { machine };
        pins.set(
            RuntimePin::new(
                "other-pin",
                scope_pin.clone(),
                assoc.runtime,
                None,
                crate::now(),
            )
            .unwrap(),
            0,
            false,
        )
        .unwrap();
        assert!(
            b.restore_pin(&m.id, &scope, &pins, 1, "", |_| Ok(()))
                .is_err()
        );
        let confirmation = format!("pin:{}", m.id.as_str());
        assert!(
            b.restore_pin(&m.id, &scope, &pins, 1, &confirmation, |_| Err(
                "runtime removed".into()
            ))
            .is_err()
        );
        assert_eq!(pins.load().unwrap().revision, 1);
        b.fault = Some(fault);
        assert!(
            b.restore_pin(&m.id, &scope, &pins, 1, &confirmation, |_| Ok(()))
                .is_err()
        );
        b.fault = None;
        assert_eq!(
            b.restore_pin(&m.id, &scope, &pins, 1, &confirmation, |_| Ok(()))
                .unwrap(),
            2
        );
        assert_eq!(
            b.restore_pin(&m.id, &scope, &pins, 1, &confirmation, |_| Ok(()))
                .unwrap(),
            2
        );
        assert_eq!(
            pins.load().unwrap().pin(&scope_pin).unwrap().runtime,
            m.source.runtime.runtime
        );
        assert!(
            b.restore_pin(&m.id, &scope, &pins, 0, &confirmation, |_| Ok(()))
                .is_err()
        );
        // Prefix restoration has no path to PinStore; prove exact bytes remain.
        let pin_path = tmp.path().join("pins/runtime-pins.json");
        let before = fs::read(&pin_path).unwrap();
        let p = b.restore_plan(&id, &m.id, &scope).unwrap();
        b.restore(&id, &m.id, &scope, rev, &p.confirmation).unwrap();
        assert_eq!(before, fs::read(pin_path).unwrap());
    }
}
#[test]
fn deletion_preserves_prefix_and_interrupted_retirement_is_not_silent() {
    let (_tmp, mut b, id, scope, rev) = fixture();
    fs::write(b.fs.target_path(&id).join("save"), b"keep").unwrap();
    let m = b.create(&id, &scope, rev, true).unwrap();
    assert!(b.delete(&m.id, &scope, "bad").is_err());
    b.delete(&m.id, &scope, &format!("delete:{}", m.id.as_str()))
        .unwrap();
    assert!(b.list().unwrap().is_empty());
    assert_eq!(
        fs::read(b.fs.target_path(&id).join("save")).unwrap(),
        b"keep"
    );
    let m = b.create(&id, &scope, rev, true).unwrap();
    b.fault = Some("delete-retired");
    assert!(
        b.delete(&m.id, &scope, &format!("delete:{}", m.id.as_str()))
            .is_err()
    );
    assert!(
        b.fs.data_dir
            .join("backups/deleting")
            .join(m.id.as_str())
            .join("payload/save")
            .exists()
    );
}
#[test]
fn completed_restore_can_be_archived_without_removing_original_bytes() {
    let (_tmp, b, id, scope, rev) = fixture();
    fs::write(b.fs.target_path(&id).join("save"), b"old").unwrap();
    let m = b.create(&id, &scope, rev, true).unwrap();
    let p = b.restore_plan(&id, &m.id, &scope).unwrap();
    b.restore(&id, &m.id, &scope, rev, &p.confirmation).unwrap();
    assert!(b.release(&id, &scope, rev + 1, "bad").is_err());
    b.release(&id, &scope, rev + 1, "release:game").unwrap();
    let history = b.sub("history", false).unwrap().unwrap();
    assert_eq!(names(&history).unwrap().len(), 1);
    assert!(b.plan(&id, &scope).is_ok());
}
#[test]
fn read_only_backup_queries_do_not_create_storage() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = endfield_unix_storage::AppPaths::from_base_dirs(
        tmp.path().join("config"),
        tmp.path().join("data"),
        tmp.path().join("cache"),
    )
    .unwrap();
    let b = BackupStore::for_app(&paths);
    assert!(b.list().unwrap().is_empty());
    assert!(b.pending_backups().unwrap().is_empty());
    assert!(b.show(&BackupId::new("missing").unwrap()).is_err());
    assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);
}
#[test]
fn complete_staging_can_be_republished_and_incomplete_is_preserved() {
    for fault in ["backup-intent", "backup-staged"] {
        let (_tmp, mut b, id, scope, rev) = fixture();
        b.fault = Some(fault);
        assert!(b.create(&id, &scope, rev, true).is_err());
        b.fault = None;
        let pending = b.pending_backups().unwrap();
        assert_eq!(pending.len(), 1);
        let result = b.recover_backup(&pending[0], &scope, true);
        assert_eq!(result.is_ok(), fault == "backup-staged");
    }
}
#[test]
fn crash_child() {
    let Ok(data) = std::env::var("ENDFIELD_P029_CHILD_DATA") else {
        return;
    };
    let mut b = BackupStore {
        fs: LinuxPrefixFilesystem::new(&data),
        prefixes: PrefixStore::with_filesystem(std::sync::Arc::new(LinuxPrefixFilesystem::new(
            &data,
        ))),
        usage_override: Some(Some(false)),
        fault: None,
    };
    let id = PrefixId::new("game").unwrap();
    let scope = PrefixScope::machine(crate::MachineId::from_digest("a".repeat(64)).unwrap());
    let rev = b.prefixes.load_snapshot().unwrap().registry.revision;
    let backup = std::env::var("ENDFIELD_P029_CHILD_BACKUP").unwrap();
    if backup == "rollback" {
        let plan = b.rollback_plan(&id, &scope).unwrap();
        b.rollback(&id, &scope, rev, &plan.confirmation).unwrap();
    } else if let Some(id) = backup.strip_prefix("delete:") {
        let id = BackupId::new(id).unwrap();
        b.delete(&id, &scope, &format!("delete:{}", id.as_str()))
            .unwrap();
    } else if backup == "create" {
        b.create(&id, &scope, rev, true).unwrap();
    } else {
        let backup = BackupId::new(backup).unwrap();
        let p = b.restore_plan(&id, &backup, &scope).unwrap();
        b.fault = None;
        b.restore(&id, &backup, &scope, rev, &p.confirmation)
            .unwrap();
    }
}
#[test]
fn process_exit_releases_locks_and_published_restore_recovers() {
    for fault in [
        "backup-intent",
        "backup-staged",
        "backup-published",
        "restore-intent",
        "restore-staged",
        "restore-published",
        "restore-registry",
    ] {
        let (_tmp, b, id, scope, rev) = fixture();
        fs::write(b.fs.target_path(&id).join("save"), b"before").unwrap();
        let backup = if fault.starts_with("restore") {
            b.create(&id, &scope, rev, true).unwrap().id.0
        } else {
            "create".into()
        };
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux_fs::backup::recovery_tests::crash_child",
                "--nocapture",
            ])
            .env("ENDFIELD_P029_CHILD_DATA", &b.fs.data_dir)
            .env("ENDFIELD_P029_CHILD_BACKUP", backup)
            .env("ENDFIELD_P029_EXIT_AT", fault)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(91), "{fault}");
        if matches!(
            fault,
            "restore-staged" | "restore-published" | "restore-registry"
        ) {
            let current = b.prefixes.load_snapshot().unwrap().registry.revision;
            b.recover(&id, &scope, current, true).unwrap();
        }
        assert_eq!(
            fs::read(b.fs.target_path(&id).join("save")).unwrap(),
            b"before"
        );
    }
}

#[test]
fn corrupt_manifest_unexpected_payload_and_adoption_are_blocked() {
    let (_tmp, b, id, scope, rev) = fixture();
    let m = b.create(&id, &scope, rev, true).unwrap();
    let root = b.fs.data_dir.join("backups/snapshots").join(m.id.as_str());
    fs::write(root.join("payload/unexpected"), b"unexpected").unwrap();
    assert!(b.verify(&m.id, &scope).is_err());
    fs::remove_file(root.join("payload/unexpected")).unwrap();
    fs::write(root.join("manifest.json"), b"{bad").unwrap();
    assert!(b.verify(&m.id, &scope).is_err());
    fs::remove_file(b.fs.data_dir.join("backups/owner.json")).unwrap();
    assert!(b.list().is_err());
}
#[test]
fn byte_and_entry_accounting_is_bounded_without_allocating_payload() {
    let e = InventoryEntry {
        path: "file".into(),
        directory: false,
        size: u64::MAX,
        mode: 0o600,
        sha256: "a".repeat(64),
        modified_seconds: 0,
        modified_nanoseconds: 0,
    };
    assert!(matches!(total(&[e.clone()]), Err(BackupError::Quota)));
    assert!(matches!(total(&[e.clone(), e]), Err(BackupError::Quota)));
}

#[test]
fn changed_completion_evidence_is_never_reported_as_completed() {
    let (_tmp, b, id, scope, rev) = fixture();
    let m = b.create(&id, &scope, rev, true).unwrap();
    let p = b.restore_plan(&id, &m.id, &scope).unwrap();
    b.restore(&id, &m.id, &scope, rev, &p.confirmation).unwrap();
    fs::write(
        b.fs.data_dir.join("backups/restores/game/completed.json"),
        b"{}",
    )
    .unwrap();
    assert!(b.recovery_status(&id).is_err());
    assert!(b.recover(&id, &scope, rev + 1, true).is_err());
    assert!(b.fs.data_dir.join("backups/restores/game/payload").is_dir());
}

#[test]
fn restore_assessment_reports_changed_missing_and_blocking_profile_pins_without_fallback() {
    use endfield_unix_runtime_selection::{PinRegistry, ProfileKey};
    let (tmp, b, id, scope, rev) = fixture();
    let m = b.create(&id, &scope, rev, true).unwrap();
    let store = PinStore::new(tmp.path().join("pins/registry.json"));
    let machine = match &scope {
        PrefixScope::Machine { machine } => machine.clone(),
        _ => unreachable!(),
    };
    let pin = RuntimePin::new(
        m.source.runtime.pin_id.clone(),
        RuntimePinScope::Machine {
            machine: machine.clone(),
        },
        m.source.runtime.runtime.clone(),
        m.source.runtime.adapter_preference,
        1,
    )
    .unwrap();
    let registry = store.set(pin, 0, false).unwrap();
    let mut plan = b.restore_plan(&id, &m.id, &scope).unwrap();
    plan.assess_selection(Some(&registry), |_| Ok(()));
    let a = plan.current_selection.as_ref().unwrap();
    assert_eq!(a.state, SelectionAssessmentState::Selected);
    assert_eq!(a.matches_saved_association, Some(true));
    plan.assess_selection(Some(&PinRegistry::default()), |_| Ok(()));
    assert_eq!(
        plan.current_selection.as_ref().unwrap().state,
        SelectionAssessmentState::NoPin
    );
    plan.assess_selection(None, |_| Err("saved runtime removed".into()));
    assert!(!plan.current_selection.as_ref().unwrap().saved_runtime_valid);
    let profile = ProfileKey::new("game-profile").unwrap();
    let mut changed = m.source.runtime.runtime.clone();
    if let crate::PinnedRuntimeReference::Managed(ref mut r) = changed {
        r.build = "different".into();
    }
    let pin = RuntimePin::new(
        "blocking-profile",
        RuntimePinScope::Profile {
            machine: machine.clone(),
            profile: profile.clone(),
        },
        changed,
        None,
        2,
    )
    .unwrap();
    let registry = store.set(pin, 1, false).unwrap();
    plan.destination.scope = PrefixScope::profile(machine, profile);
    let original = plan.backup_runtime.clone();
    plan.assess_selection(Some(&registry), |r| {
        if r == &original.runtime {
            Ok(())
        } else {
            Err("changed profile runtime".into())
        }
    });
    let a = plan.current_selection.unwrap();
    assert_eq!(a.state, SelectionAssessmentState::Blocked);
    assert_eq!(a.source, "ProfilePin");
    assert_eq!(a.matches_saved_association, Some(false));
    assert!(!a.automatic_fallback);
    assert_eq!(store.load_read_only().unwrap().revision, 2);
    assert_eq!(original, m.source.runtime);
}
#[test]
fn corrupt_restore_intent_and_ambiguous_completion_block_before_any_publication() {
    for name in ["intent.json", "completed.json"] {
        let (_tmp, mut b, id, scope, rev) = fixture();
        fs::write(b.fs.target_path(&id).join("save"), b"original").unwrap();
        let m = b.create(&id, &scope, rev, true).unwrap();
        let p = b.restore_plan(&id, &m.id, &scope).unwrap();
        b.fault = Some("restore-staged");
        assert!(b.restore(&id, &m.id, &scope, rev, &p.confirmation).is_err());
        b.fault = None;
        let path = b.fs.data_dir.join("backups/restores/game").join(name);
        fs::write(&path, b"{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let registry = b.fs.load_registry_bytes().unwrap();
        assert!(b.recover(&id, &scope, rev, true).is_err());
        assert_eq!(registry, b.fs.load_registry_bytes().unwrap());
        assert_eq!(
            fs::read(b.fs.target_path(&id).join("save")).unwrap(),
            b"original"
        );
    }
}
#[test]
fn staged_directory_replacement_with_symlink_is_never_followed() {
    let (tmp, mut b, id, scope, rev) = fixture();
    let m = b.create(&id, &scope, rev, true).unwrap();
    let p = b.restore_plan(&id, &m.id, &scope).unwrap();
    b.fault = Some("restore-staged");
    assert!(b.restore(&id, &m.id, &scope, rev, &p.confirmation).is_err());
    b.fault = None;
    let tx = b.fs.data_dir.join("backups/restores/game");
    fs::rename(tx.join("payload"), tx.join("unexpected-preserved-payload")).unwrap();
    let external = tmp.path().join("external");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("keep"), b"external").unwrap();
    std::os::unix::fs::symlink(&external, tx.join("payload")).unwrap();
    assert!(b.recover(&id, &scope, rev, true).is_err());
    assert_eq!(fs::read(external.join("keep")).unwrap(), b"external");
}
#[test]
fn partial_deletion_inspection_is_read_only_and_flags_missing_or_modified_evidence() {
    let (_tmp, mut b, id, scope, rev) = fixture();
    fs::write(b.fs.target_path(&id).join("save"), b"snapshot").unwrap();
    let m = b.create(&id, &scope, rev, true).unwrap();
    b.fault = Some("delete-retired");
    assert!(
        b.delete(&m.id, &scope, &format!("delete:{}", m.id.as_str()))
            .is_err()
    );
    b.fault = None;
    let root = b.fs.data_dir.join("backups/deleting").join(m.id.as_str());
    let manifest = fs::read(root.join("manifest.json")).unwrap();
    fs::remove_file(root.join("payload/save")).unwrap();
    assert_eq!(b.deletions().unwrap()[0].state, DeletionState::Incomplete);
    assert_eq!(manifest, fs::read(root.join("manifest.json")).unwrap());
    fs::write(root.join("payload/unexpected"), b"changed").unwrap();
    assert_eq!(b.deletions().unwrap()[0].state, DeletionState::Unsafe);
    fs::write(root.join("manifest.json"), b"{}").unwrap();
    assert_eq!(b.deletions().unwrap()[0].state, DeletionState::Unverifiable);
    assert!(b.deletions().is_ok());
    assert!(root.join("payload/unexpected").exists());
}

#[test]
fn process_exit_mid_copy_and_mid_unlink_preserves_source_and_inspectable_evidence() {
    for (mode, fault) in [
        ("create", "copy-chunk"),
        ("restore", "copy-chunk"),
        ("delete", "delete-unlink"),
    ] {
        let (_tmp, b, id, scope, rev) = fixture();
        let source = b.fs.target_path(&id);
        fs::write(source.join("a"), b"first").unwrap();
        fs::write(source.join("b"), b"second").unwrap();
        let child_arg = match mode {
            "create" => "create".into(),
            "restore" => b.create(&id, &scope, rev, true).unwrap().id.0,
            "delete" => format!(
                "delete:{}",
                b.create(&id, &scope, rev, true).unwrap().id.as_str()
            ),
            _ => unreachable!(),
        };
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux_fs::backup::recovery_tests::crash_child",
                "--nocapture",
            ])
            .env("ENDFIELD_P029_CHILD_DATA", &b.fs.data_dir)
            .env("ENDFIELD_P029_CHILD_BACKUP", child_arg)
            .env("ENDFIELD_P029_EXIT_AT", fault)
            .status()
            .unwrap();
        assert_eq!(result.code(), Some(91));
        assert_eq!(fs::read(source.join("a")).unwrap(), b"first");
        assert_eq!(fs::read(source.join("b")).unwrap(), b"second");
        match mode {
            "create" => {
                assert!(b.list().unwrap().is_empty());
                assert_eq!(b.pending_backups().unwrap().len(), 1);
            }
            "restore" => {
                assert!(b.recover(&id, &scope, rev, true).is_err());
            }
            "delete" => {
                let report = b.deletions().unwrap();
                assert_eq!(report[0].state, DeletionState::Incomplete);
                assert_eq!(report[0].remaining_entries, Some(1));
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn premature_completion_with_valid_shape_cannot_hide_an_uncommitted_registry() {
    let (_tmp, mut b, id, scope, rev) = fixture();
    let m = b.create(&id, &scope, rev, true).unwrap();
    let p = b.restore_plan(&id, &m.id, &scope).unwrap();
    b.fault = Some("restore-published");
    assert!(b.restore(&id, &m.id, &scope, rev, &p.confirmation).is_err());
    b.fault = None;
    let tx = b.transaction_dir(&id, false).unwrap().unwrap();
    let j: RestoreTransaction = read_json(&tx, "staged.json").unwrap();
    write_json(&tx, "completed.json", &j.staged).unwrap();
    let before = b.fs.load_registry_bytes().unwrap();
    assert!(b.recovery_status(&id).is_err());
    assert!(b.recover(&id, &scope, rev, true).is_err());
    assert_eq!(before, b.fs.load_registry_bytes().unwrap());
}

#[test]
fn synthetic_wine_layout_rejects_links_without_following_or_modifying_targets() {
    use std::os::unix::fs::symlink;
    let (tmp, b, id, scope, rev) = fixture();
    let root = b.fs.target_path(&id);
    for name in [
        "drive_c/Program Files",
        "drive_c/users",
        "drive_c/windows",
        "dosdevices",
    ] {
        fs::create_dir_all(root.join(name)).unwrap();
    }
    for name in ["system.reg", "user.reg", "userdef.reg"] {
        fs::write(root.join(name), b"synthetic registry fixture").unwrap();
    }
    let external = tmp.path().join("external-target");
    fs::write(&external, b"preserve external bytes").unwrap();
    symlink("../drive_c", root.join("dosdevices/c:")).unwrap();
    symlink(&external, root.join("dosdevices/z:")).unwrap();
    assert!(b.create(&id, &scope, rev, true).is_err());
    assert_eq!(fs::read(&external).unwrap(), b"preserve external bytes");
    assert_eq!(
        fs::read_link(root.join("dosdevices/c:")).unwrap(),
        std::path::Path::new("../drive_c")
    );
    assert_eq!(fs::read_link(root.join("dosdevices/z:")).unwrap(), external);
    fs::remove_file(root.join("dosdevices/c:")).unwrap();
    assert!(b.create(&id, &scope, rev, true).is_err());
    assert_eq!(fs::read(&external).unwrap(), b"preserve external bytes");
    assert!(b.list().unwrap().is_empty());
}

#[test]
fn new_restore_journals_detect_changed_original_before_and_after_exchange() {
    for fault in ["restore-staged", "restore-published"] {
        let (_tmp, mut b, id, scope, rev) = fixture();
        let live = b.fs.target_path(&id);
        fs::write(live.join("data"), b"snapshot").unwrap();
        let m = b.create(&id, &scope, rev, true).unwrap();
        fs::write(live.join("data"), b"original").unwrap();
        let p = b.restore_plan(&id, &m.id, &scope).unwrap();
        b.fault = Some(fault);
        assert!(b.restore(&id, &m.id, &scope, rev, &p.confirmation).is_err());
        b.fault = None;
        let tx = b.fs.data_dir.join("backups/restores/game");
        let changed = if fault == "restore-staged" {
            live.join("data")
        } else {
            tx.join("payload/data")
        };
        fs::write(&changed, b"modified").unwrap();
        let registry = b.fs.load_registry_bytes().unwrap();
        assert!(b.recover(&id, &scope, rev, true).is_err());
        assert_eq!(registry, b.fs.load_registry_bytes().unwrap());
        assert_eq!(fs::read(&changed).unwrap(), b"modified");
    }
}

#[test]
fn legacy_restore_journal_remains_eligible_for_forward_recovery() {
    let (_tmp, mut b, id, scope, rev) = fixture();
    let m = b.create(&id, &scope, rev, true).unwrap();
    let p = b.restore_plan(&id, &m.id, &scope).unwrap();
    b.fault = Some("restore-staged");
    assert!(b.restore(&id, &m.id, &scope, rev, &p.confirmation).is_err());
    b.fault = None;
    let path = b.fs.data_dir.join("backups/restores/game/staged.json");
    let mut journal: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    journal["schema_version"] = 1.into();
    journal
        .as_object_mut()
        .unwrap()
        .remove("original_inventory");
    fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    b.recover(&id, &scope, rev, true).unwrap();
}

#[test]
fn rollback_of_published_uncommitted_restore_is_explicit_preserves_both_trees_and_replays() {
    for fault in [
        "rollback-intent",
        "rollback-published",
        "rollback-completed",
    ] {
        let (tmp, mut b, id, scope, rev) = fixture();
        let live = b.fs.target_path(&id);
        fs::write(live.join("save"), b"snapshot").unwrap();
        let m = b.create(&id, &scope, rev, true).unwrap();
        let pin_path = tmp.path().join("pins/runtime-pins.json");
        let machine = match &scope {
            PrefixScope::Machine { machine } => machine.clone(),
            _ => unreachable!(),
        };
        let pin = RuntimePin::new(
            m.source.runtime.pin_id.clone(),
            RuntimePinScope::Machine { machine },
            m.source.runtime.runtime.clone(),
            m.source.runtime.adapter_preference,
            1,
        )
        .unwrap();
        PinStore::new(&pin_path).set(pin, 0, false).unwrap();
        let pin_bytes = fs::read(&pin_path).unwrap();
        fs::write(live.join("save"), b"original").unwrap();
        let plan = b.restore_plan(&id, &m.id, &scope).unwrap();
        b.fault = Some("restore-published");
        assert!(
            b.restore(&id, &m.id, &scope, rev, &plan.confirmation)
                .is_err()
        );
        b.fault = None;
        let plan = b.rollback_plan(&id, &scope).unwrap();
        let registry = b.fs.load_registry_bytes().unwrap();
        assert!(
            b.rollback(&id, &scope, rev + 1, &plan.confirmation)
                .is_err()
        );
        assert!(b.rollback(&id, &scope, rev, "wrong").is_err());
        b.fault = Some(fault);
        assert!(b.rollback(&id, &scope, rev, &plan.confirmation).is_err());
        b.fault = None;
        assert!(b.recover(&id, &scope, rev, true).is_err());
        b.rollback(&id, &scope, rev, &plan.confirmation).unwrap();
        b.rollback(&id, &scope, rev, &plan.confirmation).unwrap();
        assert!(matches!(
            b.recovery_status(&id).unwrap().action,
            RecoveryAction::RollbackCompleted
        ));
        assert_eq!(registry, b.fs.load_registry_bytes().unwrap());
        assert_eq!(pin_bytes, fs::read(&pin_path).unwrap());
        assert_eq!(fs::read(live.join("save")).unwrap(), b"original");
        assert_eq!(
            fs::read(b.fs.data_dir.join("backups/restores/game/payload/save")).unwrap(),
            b"snapshot"
        );
        b.release(&id, &scope, rev, "release:game").unwrap();
        let paths: Vec<_> = fs::read_dir(b.fs.data_dir.join("backups/history"))
            .unwrap()
            .collect();
        assert_eq!(paths.len(), 1);
        assert_eq!(
            fs::read(paths[0].as_ref().unwrap().path().join("payload/save")).unwrap(),
            b"snapshot"
        );
        assert!(!tmp.path().join("config").exists());
    }
}

#[test]
fn rollback_rejects_modified_original_and_committed_restore() {
    let (_tmp, mut b, id, scope, rev) = fixture();
    fs::write(b.fs.target_path(&id).join("save"), b"original").unwrap();
    let m = b.create(&id, &scope, rev, true).unwrap();
    let plan = b.restore_plan(&id, &m.id, &scope).unwrap();
    b.fault = Some("restore-published");
    assert!(
        b.restore(&id, &m.id, &scope, rev, &plan.confirmation)
            .is_err()
    );
    b.fault = None;
    fs::write(
        b.fs.data_dir.join("backups/restores/game/payload/save"),
        b"changed",
    )
    .unwrap();
    assert!(b.rollback_plan(&id, &scope).is_err());
    assert!(b.recover(&id, &scope, rev, true).is_err());
}

#[test]
fn process_exit_during_rollback_replays_without_pin_or_registry_changes() {
    for fault in [
        "rollback-intent",
        "rollback-published",
        "rollback-completed",
    ] {
        let (_tmp, mut b, id, scope, rev) = fixture();
        let live = b.fs.target_path(&id);
        fs::write(live.join("save"), b"backup").unwrap();
        let m = b.create(&id, &scope, rev, true).unwrap();
        fs::write(live.join("save"), b"original").unwrap();
        let plan = b.restore_plan(&id, &m.id, &scope).unwrap();
        b.fault = Some("restore-published");
        assert!(
            b.restore(&id, &m.id, &scope, rev, &plan.confirmation)
                .is_err()
        );
        b.fault = None;
        let before = b.fs.load_registry_bytes().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux_fs::backup::recovery_tests::crash_child",
                "--nocapture",
            ])
            .env("ENDFIELD_P029_CHILD_DATA", &b.fs.data_dir)
            .env("ENDFIELD_P029_CHILD_BACKUP", "rollback")
            .env("ENDFIELD_P029_EXIT_AT", fault)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(91));
        let plan = b.rollback_plan(&id, &scope).unwrap();
        b.rollback(&id, &scope, rev, &plan.confirmation).unwrap();
        assert_eq!(before, b.fs.load_registry_bytes().unwrap());
        assert_eq!(fs::read(live.join("save")).unwrap(), b"original");
        assert_eq!(
            fs::read(b.fs.data_dir.join("backups/restores/game/payload/save")).unwrap(),
            b"backup"
        );
    }
}

#[test]
fn rollback_refuses_schema_one_journals_and_ambiguous_completion_markers() {
    for case in ["legacy", "orphan-marker"] {
        let (_tmp, mut b, id, scope, rev) = fixture();
        let m = b.create(&id, &scope, rev, true).unwrap();
        let p = b.restore_plan(&id, &m.id, &scope).unwrap();
        b.fault = Some("restore-published");
        assert!(b.restore(&id, &m.id, &scope, rev, &p.confirmation).is_err());
        b.fault = None;
        let tx = b.transaction_dir(&id, false).unwrap().unwrap();
        if case == "legacy" {
            let path = b.fs.data_dir.join("backups/restores/game/staged.json");
            let mut journal: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            journal["schema_version"] = 1.into();
            journal
                .as_object_mut()
                .unwrap()
                .remove("original_inventory");
            fs::write(path, serde_json::to_vec(&journal).unwrap()).unwrap();
        } else {
            write_json(&tx, "rollback-completed.json", &"unexpected").unwrap();
        }
        let before = b.fs.load_registry_bytes().unwrap();
        assert!(b.rollback_plan(&id, &scope).is_err());
        if case == "orphan-marker" {
            assert!(b.recover(&id, &scope, rev, true).is_err());
        }
        assert_eq!(before, b.fs.load_registry_bytes().unwrap());
    }
}

#[test]
fn committed_restore_is_not_eligible_for_rollback() {
    let (_tmp, b, id, scope, rev) = fixture();
    let m = b.create(&id, &scope, rev, true).unwrap();
    let p = b.restore_plan(&id, &m.id, &scope).unwrap();
    b.restore(&id, &m.id, &scope, rev, &p.confirmation).unwrap();
    let before = b.fs.load_registry_bytes().unwrap();
    assert!(b.rollback_plan(&id, &scope).is_err());
    assert_eq!(before, b.fs.load_registry_bytes().unwrap());
}

#[test]
fn rollback_blocks_when_a_pin_recovery_journal_references_the_snapshot() {
    let (_tmp, mut b, id, scope, rev) = fixture();
    let m = b.create(&id, &scope, rev, true).unwrap();
    let p = b.restore_plan(&id, &m.id, &scope).unwrap();
    b.fault = Some("restore-published");
    assert!(b.restore(&id, &m.id, &scope, rev, &p.confirmation).is_err());
    b.fault = None;
    let journals = b.sub("pin-recovery", true).unwrap().unwrap();
    let _journal = exclusive_dir(&journals, &format!("{}-7", m.id.as_str())).unwrap();
    let before = b.fs.load_registry_bytes().unwrap();
    assert!(b.rollback_plan(&id, &scope).is_err());
    assert_eq!(before, b.fs.load_registry_bytes().unwrap());
}

#[test]
fn empty_deletion_replays_process_exits_without_touching_source_or_pins() {
    for point in [
        "delete-empty-intent",
        "delete-retired",
        "delete-empty-ready",
        "delete-empty-payload",
        "delete-empty-source",
        "delete-empty-manifest",
        "delete-empty-root",
        "delete-empty-completed",
    ] {
        let (tmp, b, id, scope, rev) = fixture();
        let m = b.create(&id, &scope, rev, true).unwrap();
        fs::write(b.fs.target_path(&id).join("keep"), b"source data").unwrap();
        let pin_path = tmp.path().join("pins/runtime-pins.json");
        let machine = match &scope {
            PrefixScope::Machine { machine } => machine.clone(),
            _ => unreachable!(),
        };
        let pin = RuntimePin::new(
            m.source.runtime.pin_id.clone(),
            RuntimePinScope::Machine { machine },
            m.source.runtime.runtime.clone(),
            m.source.runtime.adapter_preference,
            1,
        )
        .unwrap();
        PinStore::new(&pin_path).set(pin, 0, false).unwrap();
        let pins = fs::read(&pin_path).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux_fs::backup::recovery_tests::crash_child",
                "--nocapture",
            ])
            .env("ENDFIELD_P029_CHILD_DATA", &b.fs.data_dir)
            .env(
                "ENDFIELD_P029_CHILD_BACKUP",
                format!("delete:{}", m.id.as_str()),
            )
            .env("ENDFIELD_P029_EXIT_AT", point)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(91), "{point}");
        let plan = b.deletion_plan(&m.id).unwrap();
        let journals = b.deletion_journals().unwrap();
        assert_eq!(journals.len(), 1);
        assert_eq!(journals[0].state.as_ref(), Some(&plan.state));
        assert!(journals[0].issue.is_none());
        assert!(
            b.deletion_resume(&m.id, &scope, rev + 1, &plan.confirmation)
                .is_err()
        );
        assert!(b.deletion_resume(&m.id, &scope, rev, "wrong").is_err());
        b.deletion_resume(&m.id, &scope, rev, &plan.confirmation)
            .unwrap();
        assert_eq!(
            fs::read(b.fs.target_path(&id).join("keep")).unwrap(),
            b"source data"
        );
        assert_eq!(pins, fs::read(pin_path).unwrap());
        assert!(b.list().unwrap().is_empty());
        let collision = b.fs.data_dir.join("backups/snapshots").join(m.id.as_str());
        fs::create_dir(&collision).unwrap();
        fs::write(collision.join("unrelated"), b"preserve").unwrap();
        assert!(
            b.delete(&m.id, &scope, &format!("delete:{}", m.id.as_str()))
                .is_err()
        );
        assert!(
            b.deletion_resume(&m.id, &scope, rev, &plan.confirmation)
                .is_err()
        );
        assert_eq!(fs::read(collision.join("unrelated")).unwrap(), b"preserve");
    }
}

#[test]
fn nonempty_deletion_replays_every_durable_unlink_boundary() {
    for point in [
        "delete-intent",
        "delete-retired",
        "delete-ready",
        "delete-unlink",
        "delete-payload",
        "delete-meta-intent",
        "delete-meta-manifest",
        "delete-root",
        "delete-completed",
    ] {
        let (tmp, b, id, scope, rev) = fixture();
        let prefix = b.fs.target_path(&id);
        fs::create_dir_all(prefix.join("tree/nested")).unwrap();
        fs::write(prefix.join("tree/nested/save.bin"), b"nested bytes\0\xff").unwrap();
        fs::write(prefix.join("top-level"), b"another payload").unwrap();
        let manifest = b.create(&id, &scope, rev, true).unwrap();
        let pin_path = tmp.path().join("pins/runtime-pins.json");
        let machine = match &scope {
            PrefixScope::Machine { machine } => machine.clone(),
            _ => unreachable!(),
        };
        let pin = RuntimePin::new(
            manifest.source.runtime.pin_id.clone(),
            RuntimePinScope::Machine { machine },
            manifest.source.runtime.runtime.clone(),
            manifest.source.runtime.adapter_preference,
            1,
        )
        .unwrap();
        PinStore::new(&pin_path).set(pin, 0, false).unwrap();
        let pin_bytes = fs::read(&pin_path).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux_fs::backup::recovery_tests::crash_child",
                "--nocapture",
            ])
            .env("ENDFIELD_P029_CHILD_DATA", &b.fs.data_dir)
            .env(
                "ENDFIELD_P029_CHILD_BACKUP",
                format!("delete:{}", manifest.id.as_str()),
            )
            .env("ENDFIELD_P029_EXIT_AT", point)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(91), "{point}");
        let plan = b.deletion_plan(&manifest.id).unwrap();
        assert_eq!(plan.prefix_registry_revision, rev);
        b.deletion_resume(&manifest.id, &scope, rev, &plan.confirmation)
            .unwrap();
        b.deletion_resume(&manifest.id, &scope, rev, &plan.confirmation)
            .unwrap();
        assert_eq!(
            b.deletion_plan(&manifest.id).unwrap().state,
            DeletionRecoveryState::Completed,
            "{point}"
        );
        assert!(
            !b.fs
                .data_dir
                .join("backups/snapshots")
                .join(manifest.id.as_str())
                .exists()
        );
        assert!(
            !b.fs
                .data_dir
                .join("backups/deleting")
                .join(manifest.id.as_str())
                .exists()
        );
        assert_eq!(
            fs::read(prefix.join("tree/nested/save.bin")).unwrap(),
            b"nested bytes\0\xff"
        );
        assert_eq!(
            fs::read(prefix.join("top-level")).unwrap(),
            b"another payload"
        );
        assert_eq!(pin_bytes, fs::read(pin_path).unwrap());
    }
}

#[test]
fn nonempty_deletion_blocks_changed_entries_and_registry_revision_conflicts() {
    for case in ["replacement", "symlink", "hardlink", "extra", "revision"] {
        let (tmp, b, id, scope, rev) = fixture();
        let prefix = b.fs.target_path(&id);
        fs::create_dir_all(prefix.join("tree")).unwrap();
        fs::write(prefix.join("tree/save"), b"original backup bytes").unwrap();
        let manifest = b.create(&id, &scope, rev, true).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux_fs::backup::recovery_tests::crash_child",
                "--nocapture",
            ])
            .env("ENDFIELD_P029_CHILD_DATA", &b.fs.data_dir)
            .env(
                "ENDFIELD_P029_CHILD_BACKUP",
                format!("delete:{}", manifest.id.as_str()),
            )
            .env("ENDFIELD_P029_EXIT_AT", "delete-ready")
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(91));
        let retired =
            b.fs.data_dir
                .join("backups/deleting")
                .join(manifest.id.as_str());
        match case {
            "replacement" => {
                let file = retired.join("payload/tree/save");
                fs::remove_file(&file).unwrap();
                fs::write(&file, b"original backup bytes").unwrap();
                assert!(b.deletion_plan(&manifest.id).is_err());
            }
            "symlink" | "hardlink" => {
                let file = retired.join("payload/tree/save");
                let outside = tmp.path().join("outside-save");
                fs::write(&outside, b"original backup bytes").unwrap();
                fs::remove_file(&file).unwrap();
                if case == "symlink" {
                    std::os::unix::fs::symlink(&outside, &file).unwrap();
                } else {
                    fs::hard_link(&outside, &file).unwrap();
                }
                assert!(b.deletion_plan(&manifest.id).is_err());
                assert_eq!(fs::read(outside).unwrap(), b"original backup bytes");
            }
            "extra" => {
                fs::write(retired.join("payload/unexpected"), b"must remain").unwrap();
                assert!(b.deletion_plan(&manifest.id).is_err());
                assert_eq!(
                    fs::read(retired.join("payload/unexpected")).unwrap(),
                    b"must remain"
                );
            }
            _ => {
                let next = PrefixId::new("other").unwrap();
                b.prefixes
                    .create(next, scope.clone(), manifest.source.runtime.clone(), rev)
                    .unwrap();
                assert!(b.deletion_plan(&manifest.id).is_err());
            }
        }
        assert!(retired.exists());
        assert!(retired.join("payload/tree/save").exists());
        assert_eq!(
            fs::read(prefix.join("tree/save")).unwrap(),
            b"original backup bytes"
        );
        assert!(b.fs.data_dir.join("backups").exists());
    }
}
#[test]
fn empty_deletion_rejects_replaced_payload_and_unexpected_entries() {
    for case in ["symlink", "unexpected", "missing-intent"] {
        let (tmp, mut b, id, scope, rev) = fixture();
        let m = b.create(&id, &scope, rev, true).unwrap();
        b.fault = Some("delete-retired");
        assert!(
            b.delete(&m.id, &scope, &format!("delete:{}", m.id.as_str()))
                .is_err()
        );
        b.fault = None;
        let retired = b.fs.data_dir.join("backups/deleting").join(m.id.as_str());
        let outside = tmp.path().join("external");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), b"outside").unwrap();
        match case {
            "symlink" => {
                fs::remove_dir(retired.join("payload")).unwrap();
                std::os::unix::fs::symlink(&outside, retired.join("payload")).unwrap();
            }
            "unexpected" => {
                fs::write(retired.join("payload/unexpected"), b"keep").unwrap();
            }
            _ => {
                fs::remove_file(
                    b.fs.data_dir
                        .join("backups/deletion-journals")
                        .join(m.id.as_str())
                        .join("intent.json"),
                )
                .unwrap();
            }
        }
        assert!(b.deletion_plan(&m.id).is_err());
        let reports = b.deletion_journals().unwrap();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].state.is_none());
        assert!(reports[0].issue.is_some());
        assert_eq!(fs::read(outside.join("keep")).unwrap(), b"outside");
        assert!(retired.exists());
    }
}

#[test]
fn deletion_intent_blocks_a_conflicting_restore_before_retirement() {
    let (_tmp, mut b, id, scope, rev) = fixture();
    let m = b.create(&id, &scope, rev, true).unwrap();
    b.fault = Some("delete-empty-intent");
    assert!(
        b.delete(&m.id, &scope, &format!("delete:{}", m.id.as_str()))
            .is_err()
    );
    b.fault = None;
    let plan = b.deletion_plan(&m.id).unwrap();
    assert_eq!(plan.state, DeletionRecoveryState::PendingRetirement);
    assert!(b.restore_plan(&id, &m.id, &scope).is_err());
    assert!(
        b.delete(&m.id, &scope, &format!("delete:{}", m.id.as_str()))
            .is_err()
    );
    b.deletion_resume(&m.id, &scope, rev, &plan.confirmation)
        .unwrap();
}

#[test]
fn empty_deletion_rejects_corrupt_journal_parent_replacement_and_wrong_scope() {
    for case in ["corrupt", "parent", "scope", "ambiguous"] {
        let (tmp, mut b, id, scope, rev) = fixture();
        let m = b.create(&id, &scope, rev, true).unwrap();
        b.fault = Some("delete-retired");
        assert!(
            b.delete(&m.id, &scope, &format!("delete:{}", m.id.as_str()))
                .is_err()
        );
        b.fault = None;
        let plan = b.deletion_plan(&m.id).unwrap();
        let before = b.fs.load_registry_bytes().unwrap();
        match case {
            "corrupt" => {
                let path =
                    b.fs.data_dir
                        .join("backups/deletion-journals")
                        .join(m.id.as_str())
                        .join("intent.json");
                let mut record: serde_json::Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                record["body"]["root"]["inode"] = 1.into();
                fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
                assert!(
                    b.deletion_resume(&m.id, &scope, rev, &plan.confirmation)
                        .is_err()
                );
            }
            "parent" => {
                let old = b.fs.data_dir.join("backups/deleting");
                fs::rename(&old, tmp.path().join("preserved-deleting")).unwrap();
                fs::create_dir(&old).unwrap();
                fs::set_permissions(&old, fs::Permissions::from_mode(0o700)).unwrap();
                assert!(
                    b.deletion_resume(&m.id, &scope, rev, &plan.confirmation)
                        .is_err()
                );
                assert!(
                    tmp.path()
                        .join("preserved-deleting")
                        .join(m.id.as_str())
                        .is_dir()
                );
            }
            "scope" => {
                let other =
                    PrefixScope::machine(crate::MachineId::from_digest("b".repeat(64)).unwrap());
                assert!(
                    b.deletion_resume(&m.id, &other, rev, &plan.confirmation)
                        .is_err()
                );
                assert!(
                    b.fs.data_dir
                        .join("backups/deleting")
                        .join(m.id.as_str())
                        .is_dir()
                );
            }
            _ => {
                let journal =
                    b.fs.data_dir
                        .join("backups/deletion-journals")
                        .join(m.id.as_str());
                fs::write(
                    journal.join("completed.json"),
                    serde_json::to_vec(&FilesystemIdentity {
                        device: 1,
                        inode: 1,
                    })
                    .unwrap(),
                )
                .unwrap();
                assert!(b.deletion_plan(&m.id).is_err());
                assert!(
                    b.deletion_resume(&m.id, &scope, rev, &plan.confirmation)
                        .is_err()
                );
            }
        }
        assert_eq!(before, b.fs.load_registry_bytes().unwrap());
    }
}

#[test]
fn deserialized_traversal_identifiers_are_rejected_at_new_filesystem_boundaries() {
    let (_tmp, b, _id, _scope, _rev) = fixture();
    let bad_backup: BackupId = serde_json::from_str("\"../outside\"").unwrap();
    let bad_prefix: PrefixId = serde_json::from_str("\"../outside\"").unwrap();
    assert!(b.ensure_no_deletion_journal(&bad_backup).is_err());
    assert!(b.transaction_dir(&bad_prefix, false).is_err());
}
#[test]
fn archived_and_pin_history_detect_corrupt_evidence_without_mutation() {
    let (tmp, mut b, id, scope, rev) = fixture();
    let m = b.create(&id, &scope, rev, true).unwrap();
    let pins = PinStore::new(tmp.path().join("pins/pins.json"));
    b.fault = Some("pin-intent");
    assert!(
        b.restore_pin(
            &m.id,
            &scope,
            &pins,
            0,
            &format!("pin:{}", m.id.as_str()),
            |_| Ok(())
        )
        .is_err()
    );
    b.fault = None;
    assert!(matches!(
        b.pin_recoveries().unwrap()[0].state,
        PinRecoveryState::Pending
    ));
    b.restore_pin(
        &m.id,
        &scope,
        &pins,
        0,
        &format!("pin:{}", m.id.as_str()),
        |_| Ok(()),
    )
    .unwrap();
    assert!(matches!(
        b.pin_recoveries().unwrap()[0].state,
        PinRecoveryState::Completed
    ));
    let pin_path = tmp.path().join("pins/pins.json");
    let before = fs::read(&pin_path).unwrap();
    let marker =
        b.fs.data_dir
            .join("backups/pin-recovery")
            .join(format!("{}-0", m.id.as_str()))
            .join("completed.json");
    fs::write(marker, b"999").unwrap();
    assert!(b.pin_recoveries().is_err());
    assert_eq!(before, fs::read(pin_path).unwrap());
}

#[test]
fn archived_history_reports_identity_only_and_rejects_corrupt_journal() {
    let (_tmp, b, id, scope, rev) = fixture();
    let m = b.create(&id, &scope, rev, true).unwrap();
    let p = b.restore_plan(&id, &m.id, &scope).unwrap();
    b.restore(&id, &m.id, &scope, rev, &p.confirmation).unwrap();
    b.release(&id, &scope, rev + 1, "release:game").unwrap();
    let entries = b.history(&id).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0].outcome, HistoryOutcome::Restore));
    assert_eq!(
        entries[0].validation,
        "directory identity only; payload integrity not assessed"
    );
    let tx =
        b.fs.data_dir
            .join("backups/history")
            .join(&entries[0].transaction);
    let before = b.fs.load_registry_bytes().unwrap();
    fs::write(tx.join("staged.json"), b"{}").unwrap();
    assert!(b.history(&id).is_err());
    assert_eq!(before, b.fs.load_registry_bytes().unwrap());
    assert!(tx.join("payload").is_dir());
}
