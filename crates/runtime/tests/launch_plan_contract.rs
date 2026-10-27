mod common;

use std::ffi::OsString;

use endfield_unix_runtime::{
    CURRENT_LAUNCH_PLAN_SCHEMA_VERSION, CapabilityStatus, ContractError, EnvironmentChange,
    EnvironmentName, Executable, ExecutablePathPolicy, LaunchArgument, LaunchPlan,
    LaunchPlanReview, LaunchReadiness, MountAccess, MountRequirement, OutputCapturePolicy,
    PathClassification, PathOwnership, PlanAuthorization, PlanRisk, PrefixOwnership,
    PrefixReference, ProcessOwnershipPolicy, ProviderAvailability, RuntimeAdapterKind,
    SourceVerificationState, StructuralValidationState, ValueSensitivity, WorkingDirectory,
};

fn try_plan_with(
    arguments: Vec<LaunchArgument>,
    environment: Vec<EnvironmentChange>,
    executable: Executable,
    working_directory: WorkingDirectory,
    prefix: Option<PrefixReference>,
    mounts: Vec<MountRequirement>,
    risk: PlanRisk,
) -> Result<LaunchPlan, ContractError> {
    let runtime = common::wine_identity();
    let adapter = common::managed_wine_adapter(&runtime);
    let evidence = common::verified_compatibility_evidence(
        &runtime,
        RuntimeAdapterKind::ManagedWine,
        "launch-plan-evidence",
        None,
    );
    let compatibility = common::compatibility_assessment(vec![evidence]);
    LaunchPlan::new(
        endfield_unix_runtime::Identifier::new("launch-plan-fixture").expect("plan id"),
        CURRENT_LAUNCH_PLAN_SCHEMA_VERSION,
        runtime,
        adapter,
        executable,
        arguments,
        working_directory,
        environment,
        prefix,
        mounts,
        OutputCapturePolicy::Inherit,
        ProcessOwnershipPolicy::NoOwnershipTransfer,
        risk,
        compatibility,
    )
}

fn plan_with(
    arguments: Vec<LaunchArgument>,
    environment: Vec<EnvironmentChange>,
    executable: Executable,
    working_directory: WorkingDirectory,
    prefix: Option<PrefixReference>,
    mounts: Vec<MountRequirement>,
    risk: PlanRisk,
) -> LaunchPlan {
    try_plan_with(
        arguments,
        environment,
        executable,
        working_directory,
        prefix,
        mounts,
        risk,
    )
    .expect("valid data-only launch plan")
}

fn executable() -> Executable {
    Executable::new(
        "/home/tester/.local/share/endfield/wine/bin/wine",
        ExecutablePathPolicy::Absolute,
        PathClassification::Private,
    )
    .expect("executable")
}

fn cwd() -> WorkingDirectory {
    WorkingDirectory::new("/home/tester/games/endfield", PathClassification::Private)
        .expect("working directory")
}

fn default_plan() -> LaunchPlan {
    plan_with(
        vec![],
        vec![],
        executable(),
        cwd(),
        None,
        vec![],
        PlanRisk::UserScopedLaunch,
    )
}

#[test]
fn constructs_a_valid_non_executable_synthetic_plan() {
    let plan = default_plan();
    assert_eq!(
        plan.validation_state(),
        StructuralValidationState::StructurallyValid
    );
    assert_eq!(
        plan.source_verification(),
        SourceVerificationState::ArtifactIntegrityVerified
    );
    assert!(!plan.dry_run_json().expect("preview").is_empty());
}

#[test]
fn executable_and_working_directory_are_explicit_and_validated() {
    assert!(matches!(
        Executable::new(
            "",
            ExecutablePathPolicy::Absolute,
            PathClassification::Private,
        ),
        Err(ContractError::InvalidText { .. })
    ));
    assert!(matches!(
        Executable::new(
            "../relative/wine",
            ExecutablePathPolicy::RelativeToWorkingDirectory,
            PathClassification::Private,
        ),
        Err(ContractError::InvalidValue { .. })
    ));
    assert!(matches!(
        WorkingDirectory::new("implicit-current-directory", PathClassification::Private),
        Err(ContractError::InvalidValue { .. })
    ));
    assert!(matches!(
        WorkingDirectory::new(
            std::ffi::OsString::from("bad\0path"),
            PathClassification::Private
        ),
        Err(ContractError::InvalidText { .. })
    ));
}

#[test]
fn arguments_preserve_order_unicode_quotes_spaces_and_shell_characters() {
    let input = [
        "--leading-option",
        "",
        "space in argument",
        "quote\"and'quote",
        r"backslash\\path",
        "literal;not-a-command",
        "$(touch /tmp/not-created)",
        "\u{60}literal-backticks\u{60}",
        "left&right",
        "東京",
    ];
    let arguments = input
        .iter()
        .map(|value| LaunchArgument::new(*value, ValueSensitivity::Public).expect("argument"))
        .collect();
    let plan = plan_with(
        arguments,
        vec![],
        executable(),
        cwd(),
        None,
        vec![],
        PlanRisk::UserScopedLaunch,
    );
    let preview: serde_json::Value =
        serde_json::from_str(&plan.dry_run_json().expect("preview")).expect("JSON");
    let actual: Vec<_> = preview["argv"]
        .as_array()
        .expect("argument array")
        .iter()
        .map(|value| value.as_str().expect("UTF-8 argument"))
        .collect();
    assert_eq!(actual, input);
}

#[test]
fn preserves_a_very_long_argument_within_the_explicit_bound() {
    let long = "x".repeat(20 * 1024);
    let argument = LaunchArgument::new(long.clone(), ValueSensitivity::Public).expect("argument");
    let plan = plan_with(
        vec![argument],
        vec![],
        executable(),
        cwd(),
        None,
        vec![],
        PlanRisk::UserScopedLaunch,
    );
    let preview: serde_json::Value =
        serde_json::from_str(&plan.dry_run_json().expect("preview")).expect("JSON");
    assert_eq!(preview["argv"][0].as_str(), Some(long.as_str()));
}

#[test]
fn native_non_utf8_arguments_and_paths_are_not_lossily_converted() {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let raw_argument = OsString::from_vec(vec![b'a', 0xff]);
        let argument =
            LaunchArgument::new(raw_argument.clone(), ValueSensitivity::Public).expect("argument");
        assert_eq!(argument.value().as_os_str().as_bytes(), &[b'a', 0xff]);

        let raw_path =
            OsString::from_vec(b"/tmp/non-utf8-".iter().copied().chain([0xff]).collect());
        let executable = Executable::new(
            raw_path,
            ExecutablePathPolicy::Absolute,
            PathClassification::Private,
        )
        .expect("native path");
        let plan = plan_with(
            vec![argument],
            vec![],
            executable,
            cwd(),
            None,
            vec![],
            PlanRisk::UserScopedLaunch,
        );
        let preview: serde_json::Value =
            serde_json::from_str(&plan.dry_run_json().expect("preview")).expect("JSON");
        assert_eq!(preview["argv"][0], "[non-UTF-8 value omitted]");
        assert_eq!(preview["executable"], "[redacted]");
    }
}

#[test]
fn rejects_nul_bytes_and_duplicate_environment_modifications() {
    assert!(LaunchArgument::new(OsString::from("arg\0tail"), ValueSensitivity::Public).is_err());
    assert!(
        EnvironmentChange::set(
            EnvironmentName::new("WINEPREFIX").expect("name"),
            OsString::from("prefix\0tail"),
            ValueSensitivity::Sensitive(endfield_unix_runtime::SensitiveValueKind::SensitivePath),
        )
        .is_err()
    );
    let name = EnvironmentName::new("WINEPREFIX").expect("name");
    let first = EnvironmentChange::set(
        name.clone(),
        "/prefix/one",
        ValueSensitivity::Sensitive(endfield_unix_runtime::SensitiveValueKind::SensitivePath),
    )
    .expect("first value");
    let second = EnvironmentChange::remove(name);
    assert!(
        try_plan_with(
            vec![],
            vec![first, second],
            executable(),
            cwd(),
            None,
            vec![],
            PlanRisk::UserScopedLaunch,
        )
        .is_err()
    );
}

#[test]
fn dry_run_redacts_secrets_and_private_paths_by_default() {
    let secret = EnvironmentChange::set(
        EnvironmentName::new("SECRET_TOKEN").expect("name"),
        "synthetic-secret-value",
        ValueSensitivity::Sensitive(endfield_unix_runtime::SensitiveValueKind::Token),
    )
    .expect("secret value");
    let plan = plan_with(
        vec![
            LaunchArgument::new(
                "synthetic-account-secret",
                ValueSensitivity::Sensitive(
                    endfield_unix_runtime::SensitiveValueKind::AccountRelated,
                ),
            )
            .expect("argument"),
        ],
        vec![secret],
        executable(),
        cwd(),
        Some(
            PrefixReference::new(
                endfield_unix_runtime::Identifier::new("fixture-prefix").expect("prefix id"),
                "/home/tester/.local/share/endfield/prefix",
                PathClassification::Private,
                PrefixOwnership::ExternalUserManaged,
            )
            .expect("prefix ref"),
        ),
        vec![],
        PlanRisk::UserScopedLaunch,
    );
    let preview = plan.dry_run_json().expect("preview");
    assert!(!preview.contains("synthetic-secret-value"));
    assert!(!preview.contains("synthetic-account-secret"));
    assert!(!preview.contains("/home/tester"));
    assert!(preview.contains("[redacted]"));
    let parsed: serde_json::Value = serde_json::from_str(&preview).expect("JSON");
    assert_eq!(parsed["environment"][0]["value"], "[redacted]");
    assert_eq!(parsed["argv"][0], "[redacted]");
    assert_eq!(parsed["executable_now"], false);
}

#[test]
fn empty_environment_value_is_distinct_from_remove_and_inherit() {
    let empty = EnvironmentChange::set(
        EnvironmentName::new("EMPTY_VALUE").expect("name"),
        OsString::new(),
        ValueSensitivity::Public,
    )
    .expect("empty set value");
    let remove = EnvironmentChange::remove(EnvironmentName::new("REMOVE_VALUE").expect("name"));
    let inherit = EnvironmentChange::preserve_inherited(
        EnvironmentName::new("INHERITED_VALUE").expect("name"),
    );
    assert_eq!(empty.operation(), "set");
    assert_eq!(empty.value().expect("set").as_os_str().to_str(), Some(""));
    assert_eq!(remove.operation(), "remove");
    assert_eq!(inherit.operation(), "preserve-inherited");
}

#[test]
fn invalid_prefix_and_mount_references_are_rejected() {
    assert!(
        PrefixReference::new(
            endfield_unix_runtime::Identifier::new("prefix").expect("id"),
            "relative/prefix",
            PathClassification::Private,
            PrefixOwnership::Unknown,
        )
        .is_err()
    );
    assert!(
        MountRequirement::new(
            "relative/source",
            PathClassification::Private,
            PathOwnership::External,
            "/target",
            PathClassification::Private,
            MountAccess::ReadOnly,
        )
        .is_err()
    );
    assert!(
        MountRequirement::new(
            "/source",
            PathClassification::Private,
            PathOwnership::Unknown,
            "/target",
            PathClassification::Private,
            MountAccess::ReadWrite,
        )
        .is_err()
    );
}

#[test]
fn writable_mounts_raise_risk_and_duplicate_targets_are_rejected() {
    let first = MountRequirement::new(
        "/external/save",
        PathClassification::Private,
        PathOwnership::External,
        "/game/save",
        PathClassification::Private,
        MountAccess::ReadWrite,
    )
    .expect("mount");
    let second = MountRequirement::new(
        "/external/other",
        PathClassification::Private,
        PathOwnership::External,
        "/game/save",
        PathClassification::Private,
        MountAccess::ReadWrite,
    )
    .expect("mount");
    assert!(
        try_plan_with(
            vec![],
            vec![],
            executable(),
            cwd(),
            None,
            vec![first.clone()],
            PlanRisk::UserScopedLaunch,
        )
        .is_err()
    );
    assert!(
        try_plan_with(
            vec![],
            vec![],
            executable(),
            cwd(),
            None,
            vec![first, second],
            PlanRisk::PrivilegedOrUnsupported,
        )
        .is_err()
    );
}

#[test]
fn deterministic_json_keeps_schema_order_and_literal_arguments() {
    let plan = plan_with(
        vec![
            LaunchArgument::new("first", ValueSensitivity::Public).expect("arg"),
            LaunchArgument::new("$second; & \u{60}third\u{60}", ValueSensitivity::Public)
                .expect("arg"),
        ],
        vec![],
        executable(),
        cwd(),
        None,
        vec![],
        PlanRisk::UserScopedLaunch,
    );
    let first = plan.dry_run_json().expect("preview");
    let second = plan.dry_run_json().expect("preview");
    assert_eq!(first, second);
    assert!(first.contains("\"schema_version\":1"));
    assert!(first.contains("$second; & \u{60}third\u{60}"));
    assert!(first.contains("\"executable_now\":false"));
}

#[test]
fn structural_validation_does_not_authorize_privileged_or_unsupported_actions() {
    let plan = plan_with(
        vec![],
        vec![],
        executable(),
        cwd(),
        None,
        vec![],
        PlanRisk::PrivilegedOrUnsupported,
    );
    let review = LaunchPlanReview::new(
        endfield_unix_runtime::ProviderId::new("fixture-wine-provider").expect("provider"),
        RuntimeAdapterKind::ManagedWine,
        ProviderAvailability::Available,
        CapabilityStatus::VerifiedUsable,
        PlanAuthorization::ExplicitlyGranted {
            grant_id: endfield_unix_runtime::Identifier::new("grant").expect("grant"),
            plan_id: plan.plan_id().clone(),
        },
    );
    assert_eq!(plan.readiness(&review), LaunchReadiness::UnsupportedRisk);
}

#[test]
fn dry_run_with_review_reports_unsupported_provider_operations_explicitly() {
    let plan = default_plan();
    let review = LaunchPlanReview::new(
        endfield_unix_runtime::ProviderId::new("fixture-wine-provider").expect("provider"),
        RuntimeAdapterKind::ManagedWine,
        ProviderAvailability::Available,
        CapabilityStatus::Unsupported,
        PlanAuthorization::NotRequested,
    );
    let preview: serde_json::Value = serde_json::from_str(
        &plan
            .dry_run_json_with_review(&review)
            .expect("review preview"),
    )
    .expect("JSON");
    assert_eq!(preview["launch_capability"], "unsupported");
    assert_eq!(preview["readiness"], "unsupported-operation");
    assert_eq!(preview["executable_now"], false);
}

#[test]
fn launch_review_for_another_provider_or_adapter_is_rejected() {
    let plan = default_plan();
    let review = LaunchPlanReview::new(
        endfield_unix_runtime::ProviderId::new("other-provider").expect("provider"),
        RuntimeAdapterKind::ExternalWine,
        ProviderAvailability::Available,
        CapabilityStatus::VerifiedUsable,
        PlanAuthorization::NotRequested,
    );
    assert_eq!(
        plan.readiness(&review),
        LaunchReadiness::ReviewDoesNotMatchPlan
    );
}

#[test]
fn launch_approval_is_bound_to_this_plan_and_still_only_allows_review() {
    let plan = default_plan();
    let provider =
        endfield_unix_runtime::ProviderId::new("fixture-wine-provider").expect("provider");
    let no_approval = LaunchPlanReview::new(
        provider.clone(),
        RuntimeAdapterKind::ManagedWine,
        ProviderAvailability::Available,
        CapabilityStatus::ImplementedByProvider,
        PlanAuthorization::NotRequested,
    );
    assert_eq!(
        plan.readiness(&no_approval),
        LaunchReadiness::UserApprovalRequired
    );
    let wrong_plan = LaunchPlanReview::new(
        provider.clone(),
        RuntimeAdapterKind::ManagedWine,
        ProviderAvailability::Available,
        CapabilityStatus::ImplementedByProvider,
        PlanAuthorization::ExplicitlyGranted {
            grant_id: endfield_unix_runtime::Identifier::new("grant").expect("grant"),
            plan_id: endfield_unix_runtime::Identifier::new("different-plan").expect("id"),
        },
    );
    assert_eq!(
        plan.readiness(&wrong_plan),
        LaunchReadiness::UserApprovalRequired
    );
    let approved = LaunchPlanReview::new(
        provider,
        RuntimeAdapterKind::ManagedWine,
        ProviderAvailability::Available,
        CapabilityStatus::ImplementedByProvider,
        PlanAuthorization::ExplicitlyGranted {
            grant_id: endfield_unix_runtime::Identifier::new("grant").expect("grant"),
            plan_id: plan.plan_id().clone(),
        },
    );
    assert_eq!(
        plan.readiness(&approved),
        LaunchReadiness::EligibleForFurtherReview
    );
}

#[test]
fn a_plan_does_not_create_a_working_directory_or_prefix() {
    let path = "/tmp/endfield-p021-contract-fixture-that-is-not-created";
    let plan = plan_with(
        vec![],
        vec![],
        executable(),
        WorkingDirectory::new(path, PathClassification::Private).expect("cwd"),
        None,
        vec![],
        PlanRisk::UserScopedLaunch,
    );
    let before = std::path::Path::new(path).exists();
    let _ = plan.dry_run_json().expect("preview");
    assert_eq!(std::path::Path::new(path).exists(), before);
}
