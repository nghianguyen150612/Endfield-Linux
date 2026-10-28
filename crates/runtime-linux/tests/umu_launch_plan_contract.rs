use endfield_unix_runtime::{RuntimeFamily, RuntimeProvider};
use endfield_unix_runtime_linux::umu_discovery::{UmuDiscoveryEnvironment, discover_umu};
use endfield_unix_runtime_linux::umu_environment::{PrefixSelection, Store};
use endfield_unix_runtime_linux::umu_plan::{
    PlanPreparation, UmuLaunchPlanBuilder, UmuLaunchRequest,
};
use endfield_unix_runtime_linux::{DiscoveryOptions, LinuxRuntimeProvider};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn root() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "endfield-umu-plan-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}
fn executable(path: &std::path::Path) {
    fs::write(path, b"synthetic fixture; not executed").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn explicit_p022_proton_selection_prepares_redacted_non_executable_plan() {
    let base = root();
    let runtimes = base.join("runtimes");
    let proton = runtimes.join("GE-Proton10-26");
    fs::create_dir_all(&proton).unwrap();
    executable(&proton.join("proton"));
    fs::write(proton.join("version"), "GE-Proton10-26\n").unwrap();
    fs::write(proton.join("compatibilitytool.vdf"), "\"compatibilitytool\" { \"compat_tools\" { \"GE-Proton10-26\" { \"version\" \"GE-Proton10-26\" \"architecture\" \"x86_64\" } } }").unwrap();
    let options = DiscoveryOptions {
        additional_roots: vec![runtimes],
        include_path: false,
        include_conventional: false,
    };
    let provider = LinuxRuntimeProvider::new_with_environment(
        options,
        endfield_unix_runtime_linux::DiscoveryEnvironment::isolated(None, None),
    )
    .unwrap();
    let report = provider.scan();
    let selected = report
        .runtimes
        .iter()
        .find(|r| r.candidate.family() == RuntimeFamily::Proton)
        .expect("synthetic Proton must be discovered");
    assert!(RuntimeProvider::inspect_identity(&provider, &selected.candidate).is_ok());

    let umu_path = base.join("umu-run");
    executable(&umu_path);
    let environment = UmuDiscoveryEnvironment::isolated(None, None);
    let umu = discover_umu(&environment, Some(&umu_path))
        .candidates
        .remove(0);
    let prefix = base.join("existing-prefix");
    fs::create_dir_all(&prefix).unwrap();
    let game = base.join("game.exe");
    fs::write(&game, b"synthetic game file; not executed").unwrap();
    let before = fs::read_dir(&base).unwrap().count();
    let request = UmuLaunchRequest {
        selected_runtime_id: Some(selected.candidate.runtime_id().as_str().to_owned()),
        prefix: Some(
            PrefixSelection::new(
                &prefix,
                endfield_unix_runtime::PrefixOwnership::ExternalUserManaged,
            )
            .unwrap(),
        ),
        game_executable: Some(game.clone()),
        arguments: vec![
            "argument with spaces".into(),
            "$literal; *".into(),
            "雪".into(),
        ],
        game_id: None,
        store: Some(Store::Egs),
    };
    let prepared = UmuLaunchPlanBuilder::prepare(&provider, &report, &umu, &request);
    let PlanPreparation::Complete(plan) = &prepared else {
        panic!("complete synthetic P022 plan expected")
    };
    assert_eq!(plan.adapter().kind().as_str(), "umu-proton");
    assert_eq!(plan.source_verification().as_str(), "candidate-identified");
    assert_eq!(plan.compatibility().state().as_str(), "not-evaluated");
    let preview = plan.dry_run_json().unwrap();
    assert!(preview.contains("\"executable_now\":false"));
    assert!(preview.contains("\"readiness\":\"not-evaluated\""));
    assert!(
        preview.contains("\"argv\":[\"[redacted]\",\"[redacted]\",\"[redacted]\",\"[redacted]\"]")
    );
    assert_eq!(plan.arguments()[1].value(), "argument with spaces");
    assert_eq!(plan.arguments()[2].value(), "$literal; *");
    assert_eq!(plan.arguments()[3].value(), "雪");
    assert!(!preview.contains(&base.display().to_string()));
    assert!(!format!("{request:?}").contains(&base.display().to_string()));
    assert!(!format!("{request:?}").contains("argument with spaces"));
    assert!(!format!("{prepared:?}").contains(&base.display().to_string()));
    let summary = endfield_unix_runtime_linux::umu_plan::redacted_plan_summary(plan, &umu);
    assert!(summary.contains("adapter_provider=linux-umu-adapter"));
    assert!(summary.contains("runtime_provider=linux-runtime-discovery"));
    assert!(summary.contains("umu_version=unknown"));
    assert!(summary.contains("compatibility=not-evaluated"));
    assert!(summary.contains("execution_authorized=false executable_now=false"));
    assert!(!summary.contains(&base.display().to_string()));
    assert_eq!(fs::read_dir(&base).unwrap().count(), before);
    let _ = fs::remove_dir_all(base);
}

#[test]
fn missing_selection_prefix_and_game_are_typed_incomplete_results() {
    let base = root();
    let provider = LinuxRuntimeProvider::new_with_environment(
        DiscoveryOptions {
            additional_roots: vec![],
            include_path: false,
            include_conventional: false,
        },
        endfield_unix_runtime_linux::DiscoveryEnvironment::isolated(None, None),
    )
    .unwrap();
    let report = provider.scan();
    let umu_path = base.join("umu-run");
    executable(&umu_path);
    let umu = discover_umu(
        &UmuDiscoveryEnvironment::isolated(None, None),
        Some(&umu_path),
    )
    .candidates
    .remove(0);
    assert!(
        matches!(UmuLaunchPlanBuilder::prepare(&provider, &report, &umu, &UmuLaunchRequest::default()), PlanPreparation::Incomplete(missing) if missing.contains(&endfield_unix_runtime_linux::umu_plan::PlanPrerequisite::ProtonSelectionRequired) && missing.contains(&endfield_unix_runtime_linux::umu_plan::PlanPrerequisite::PrefixSelectionRequired) && missing.contains(&endfield_unix_runtime_linux::umu_plan::PlanPrerequisite::GameExecutableRequired))
    );
    let invalid_arguments = UmuLaunchRequest {
        arguments: vec![std::ffi::OsString::from("bad\0argument")],
        ..UmuLaunchRequest::default()
    };
    assert!(
        matches!(UmuLaunchPlanBuilder::prepare(&provider, &report, &umu, &invalid_arguments), PlanPreparation::Incomplete(missing) if missing.contains(&endfield_unix_runtime_linux::umu_plan::PlanPrerequisite::InvalidArguments))
    );
    let _ = fs::remove_dir_all(base);
}
