//! Explicit opt-in only. This test is ignored by default and preserves all data.
//! See docs/WINE-QUALIFICATION.md. No game is ever invoked.
use endfield_unix_prefix_management::{
    self as prefix,
    wine_qualification::{self as qualification, QualificationState},
};
use endfield_unix_runtime_selection as pins;
use endfield_unix_storage::AppPaths;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[test]
#[ignore = "authentic Wine requires explicit opt-in, exact managed pin, private disposable root, qualified policy and plan approval"]
fn authentic_wine_initialization_opt_in() {
    assert_eq!(
        std::env::var("ENDFIELD_AUTHENTIC_WINE_OPT_IN").as_deref(),
        Ok("I_ACCEPT_DISPOSABLE_WINE_EXECUTION")
    );
    let paths = AppPaths::discover().expect("explicit source XDG environment required");
    let pin_id = std::env::var("ENDFIELD_AUTHENTIC_WINE_PIN").expect("exact pin required");
    let revision: u64 = std::env::var("ENDFIELD_AUTHENTIC_WINE_REVISION")
        .unwrap()
        .parse()
        .unwrap();
    let digest = std::env::var("ENDFIELD_AUTHENTIC_WINE_ARTIFACT_SHA256").unwrap();
    let registry = pins::PinStore::for_app(&paths).load_read_only().unwrap();
    assert_eq!(registry.revision, revision);
    let pin = registry.pins.iter().find(|p| p.pin_id == pin_id).unwrap();
    let pins::PinnedRuntimeReference::Managed(runtime) = &pin.runtime else {
        panic!("managed runtime required");
    };
    assert_eq!(runtime.artifact_sha256, digest);
    let report = qualification::qualify_managed_wine(&paths, &pin_id, revision, &pin.runtime);
    let root = PathBuf::from(
        std::env::var_os("ENDFIELD_AUTHENTIC_WINE_TEST_ROOT")
            .expect("new absolute disposable root required"),
    );
    assert!(root.is_absolute());
    let resume = root.exists();
    if resume {
        assert_eq!(
            std::env::var("ENDFIELD_AUTHENTIC_WINE_RESUME").as_deref(),
            Ok("I_REVIEWED_THE_PRESERVED_TEST_DATA")
        );
        let metadata = fs::symlink_metadata(&root).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        assert_eq!(metadata.mode() & 0o077, 0);
        let saved_metadata = fs::symlink_metadata(root.join("evidence.json")).unwrap();
        assert!(
            saved_metadata.is_file()
                && !saved_metadata.file_type().is_symlink()
                && saved_metadata.len() <= 1024 * 1024
        );
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("evidence.json")).unwrap()).unwrap();
        assert_eq!(
            saved["actual_wine_execution"], "NOT_RUN",
            "never resume or retry after any uncertain/observed execution"
        );
        assert_eq!(saved["qualification"]["artifact_sha256"], digest);
        assert_eq!(saved["qualification"]["pin_id"], pin_id);
        assert_eq!(saved["qualification"]["pin_revision"], revision);
    }
    assert!(
        !root.starts_with(paths.data_dir())
            && !root.starts_with(paths.config_file().parent().unwrap())
    );
    assert!(!root.components().any(|p| !matches!(
        p,
        std::path::Component::RootDir | std::path::Component::Normal(_)
    )));
    let parent = fs::symlink_metadata(root.parent().unwrap()).unwrap();
    assert!(parent.is_dir() && !parent.file_type().is_symlink());
    assert_eq!(parent.uid(), unsafe { libc::geteuid() });
    assert_eq!(parent.mode() & 0o077, 0, "use a private parent directory");
    if !resume {
        fs::create_dir(&root).unwrap();
    }
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut evidence = serde_json::json!({"schema_version":1,"scope":"exact-managed-artifact-host-and-private-test-prefix-only",
        "timestamp_unix_seconds":timestamp,"qualification":report,"actual_wine_execution":"NOT_RUN",
        "prefix_lifecycle":"not_created","process_outcome":null,"completion_verified":false,
        "limitations":["preserve all test data; inspect before manual cleanup","direct child does not own helper descendants","not Endfield compatibility evidence"]});
    let evidence_path = root.join("evidence.json");
    fs::write(
        &evidence_path,
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    // Authentic qualification itself is not required for a qualification run;
    // every prerequisite for safely invoking/observing Wine still is.
    if !report
        .blockers
        .iter()
        .all(|b| b == "authentic_qualification_not_run")
        || report.ownership != QualificationState::Verified
        || report.integrity != QualificationState::Verified
        || report.helper_inventory != QualificationState::Verified
        || report.invocation_contract != QualificationState::Verified
        || report.descendant_completion != QualificationState::Verified
    {
        eprintln!(
            "Authentic Wine NOT_RUN: qualified layout/invocation/quiescence prerequisites absent; evidence preserved."
        );
        return;
    }
    let test_paths =
        AppPaths::from_base_dirs(root.join("config"), root.join("data"), root.join("cache"))
            .unwrap();
    let store = prefix::PrefixStore::for_app(&test_paths);
    let (scope, source) = match &pin.scope {
        pins::RuntimePinScope::Machine { machine } => (
            prefix::PrefixScope::machine(machine.clone()),
            prefix::RuntimeSelectionSource::MachinePin,
        ),
        pins::RuntimePinScope::Profile { machine, profile } => (
            prefix::PrefixScope::profile(machine.clone(), profile.clone()),
            prefix::RuntimeSelectionSource::ProfilePin,
        ),
    };
    let association = prefix::PrefixRuntimeAssociation {
        runtime: pin.runtime.clone(),
        pin_id: pin_id.clone(),
        pin_revision: revision,
        selection_source: source,
        adapter_preference: pin.adapter_preference,
    };
    let id = prefix::PrefixId::new("authentic-disposable").unwrap();
    let rev = if resume {
        let (registry, records) = store.list(|_| Ok(())).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].record.runtime, association);
        assert_eq!(
            records[0].record.lifecycle,
            prefix::PrefixLifecycleState::Created
        );
        assert!(records[0].record.initialization_intent.is_none());
        registry.revision
    } else {
        let (_, rev) = store
            .create(id.clone(), scope, association.clone(), 0)
            .unwrap();
        rev
    };
    let plan = store
        .plan_initialization(&id, rev, &association, Some(false), |_| Ok(()))
        .unwrap();
    // Deliberately two-stage: inspect evidence/plan and re-invoke with exact approval.
    // No automatic approve from this program's own computed plan ID.
    evidence["prefix_lifecycle"] = "created".into();
    evidence["initialization_plan"] = serde_json::to_value(&plan).unwrap();
    fs::write(
        &evidence_path,
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    let confirmation = std::env::var("ENDFIELD_AUTHENTIC_WINE_PLAN_APPROVAL")
        .expect("exact separately reviewed plan approval required");
    let approval = plan
        .approve(&confirmation)
        .expect("production plan must independently be eligible");
    let authorization =
        qualification::authorize_production_initialization(&paths, &plan, &approval, &store)
            .unwrap();
    let coordinator = prefix::InitializationCoordinator::new(store);
    let validation_paths = paths.clone();
    let validation_id = pin_id.clone();
    let cancellation = endfield_unix_game_launch::linux_executor::CancellationToken::default();
    let start = coordinator
        .start(
            &plan,
            &approval,
            authorization,
            || Some(false),
            move |association| {
                let fresh = qualification::qualify_managed_wine(
                    &validation_paths,
                    &validation_id,
                    revision,
                    &association.runtime,
                );
                if fresh.production_initialization == QualificationState::Verified {
                    Ok(())
                } else {
                    Err("qualification changed".into())
                }
            },
            &cancellation,
        )
        .unwrap();
    let mut attempt = match start {
        prefix::InitializationStart::Running(attempt) => attempt,
        prefix::InitializationStart::RunningPublicationFailed { attempt, .. } => {
            attempt.abandon_unjournaled().unwrap();
            panic!("Running publication failed; preserve prefix for recovery");
        }
    };
    evidence["actual_wine_execution"] = "STARTED".into();
    fs::write(
        &evidence_path,
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    let observation = attempt.observe(Duration::from_secs(60)).unwrap();
    match observation {
        prefix::InitializationObservation::Initialized { .. } => {
            evidence["prefix_lifecycle"] = "initialized".into();
            evidence["completion_verified"] = true.into();
            evidence["process_outcome"] = "observed_exit_zero".into();
        }
        prefix::InitializationObservation::Running { .. } => {
            let _ = attempt.cancel();
            evidence["process_outcome"] =
                "timeout_direct_child_cancel_requested_recovery_required".into();
        }
        _ => {
            evidence["process_outcome"] = "failed_or_unverified_recovery_required".into();
        }
    }
    fs::write(
        &evidence_path,
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
}
