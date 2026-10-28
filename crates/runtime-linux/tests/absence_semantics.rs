//! Deterministic tests for missing and failed runtime discovery sources.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use endfield_unix_runtime::{
    AuthenticityState, DiscoveryCompleteness, DiscoveryQuery, MetadataVerification,
    ProviderErrorKind, RuntimeFamily, RuntimeLineage, RuntimeProvider, SourceVerificationState,
};
use endfield_unix_runtime_linux::{
    DiscoveryEnvironment, DiscoveryOptions, LinuxRuntimeProvider, MAX_ROOTS, RootOutcome,
    SourceKind,
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "endfield-p022a-{}-{name}-{serial}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn provider(
    home: Option<PathBuf>,
    path: Option<OsString>,
    additional_roots: Vec<PathBuf>,
    include_path: bool,
    include_conventional: bool,
) -> LinuxRuntimeProvider {
    LinuxRuntimeProvider::new_with_environment(
        DiscoveryOptions {
            additional_roots,
            include_path,
            include_conventional,
        },
        DiscoveryEnvironment::isolated(home, path),
    )
    .unwrap()
}

fn make_proton(root: &Path, launcher_body: &str) {
    fs::create_dir_all(root).unwrap();
    let launcher = root.join("proton");
    fs::write(launcher, launcher_body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join("proton"), fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(root.join("version"), "GE-Proton10-26\n").unwrap();
    fs::write(
        root.join("compatibilitytool.vdf"),
        "\"compatibilitytool\" { \"compat_tools\" { \"GE-Proton10-26\" { \"display_name\" \"GE-Proton10-26\" } } }",
    )
    .unwrap();
}

#[test]
fn empty_home_and_missing_conventional_roots_are_a_complete_empty_inventory() {
    let home = Fixture::new("empty-home");
    let report = provider(
        Some(home.path().to_path_buf()),
        Some(OsString::new()),
        Vec::new(),
        true,
        true,
    )
    .scan();

    assert!(report.runtimes.is_empty());
    assert!(report.issues.is_empty());
    assert!(!report.all_sources_failed());
    assert_eq!(
        report.contract.completeness(),
        &DiscoveryCompleteness::Complete
    );
    assert!(report.roots.iter().all(|root| {
        root.outcome == RootOutcome::NotPresent || root.outcome == RootOutcome::Inspected
    }));
}

#[test]
fn multiple_absolute_path_directories_without_wine_are_absence_not_failure() {
    let home = Fixture::new("path-home");
    let paths = (0..8)
        .map(|index| {
            let directory = home.path().join(format!("bin-{index}"));
            fs::create_dir_all(&directory).unwrap();
            directory
        })
        .collect::<Vec<_>>();
    let path = std::env::join_paths(&paths).unwrap();
    let report = provider(None, Some(path), Vec::new(), true, false).scan();

    assert!(report.runtimes.is_empty());
    assert!(report.issues.is_empty());
    assert!(
        report
            .roots
            .iter()
            .all(|root| root.outcome == RootOutcome::NotPresent)
    );
    assert_eq!(
        report.contract.completeness(),
        &DiscoveryCompleteness::Complete
    );
}

#[test]
fn empty_custom_root_is_inspected_and_missing_custom_root_is_a_valid_observation() {
    let base = Fixture::new("custom-empty");
    let empty = base.path().join("empty");
    let missing = base.path().join("missing");
    fs::create_dir(&empty).unwrap();

    let report = provider(
        None,
        None,
        vec![empty.clone(), missing.clone()],
        false,
        false,
    )
    .scan();

    assert!(report.runtimes.is_empty());
    assert!(report.issues.is_empty());
    assert!(report.roots.iter().any(|root| {
        root.source == SourceKind::Explicit && root.outcome == RootOutcome::Inspected
    }));
    assert!(report.roots.iter().any(|root| {
        root.source == SourceKind::Explicit && root.outcome == RootOutcome::NotPresent
    }));
    assert_eq!(
        report.contract.completeness(),
        &DiscoveryCompleteness::Complete
    );
}

#[test]
fn inaccessible_custom_root_is_not_reported_as_empty() {
    use std::os::unix::fs::PermissionsExt;

    let base = Fixture::new("custom-denied");
    let root = base.path().join("denied");
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o000)).unwrap();
    let read_denied = fs::read_dir(&root).is_err();

    let report = provider(None, None, vec![root.clone()], false, false).scan();

    if read_denied {
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.reason == "permission-denied")
        );
        assert!(
            report
                .roots
                .iter()
                .any(|root| root.outcome == RootOutcome::Failed)
        );
        assert!(report.all_sources_failed());
        assert_eq!(
            report.contract.completeness(),
            &DiscoveryCompleteness::Partial {
                reason: endfield_unix_runtime::Identifier::new("one-or-more-sources-unavailable")
                    .unwrap()
            }
        );
    } else {
        assert!(report.issues.is_empty());
        assert!(
            report
                .roots
                .iter()
                .any(|root| root.outcome == RootOutcome::Inspected)
        );
        assert!(!report.all_sources_failed());
    }
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn malformed_root_is_a_typed_failure_and_all_failed_sources_are_detectable() {
    let base = Fixture::new("malformed-root");
    let root = base.path().join("not-a-directory");
    fs::write(&root, b"regular file").unwrap();
    let provider = provider(None, None, vec![root], false, false);
    let report = provider.scan();

    assert!(report.runtimes.is_empty());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.reason == "root-not-directory")
    );
    assert!(report.all_sources_failed());
    assert_eq!(
        report.contract.completeness(),
        &DiscoveryCompleteness::Partial {
            reason: endfield_unix_runtime::Identifier::new("one-or-more-sources-unavailable")
                .unwrap()
        }
    );
    let query = DiscoveryQuery::new(vec![RuntimeFamily::Proton], false).unwrap();
    assert_eq!(
        provider.discover(&query).unwrap_err().kind(),
        ProviderErrorKind::ProviderUnavailable
    );
}

#[test]
fn source_collection_failure_without_a_root_is_a_typed_provider_error() {
    let path = OsString::from("x".repeat(16 * 1024 + 1));
    let provider = provider(None, Some(path), Vec::new(), true, false);
    let report = provider.scan();

    assert!(report.roots.is_empty());
    assert!(report.runtimes.is_empty());
    assert_eq!(report.issues[0].reason, "path-hint-too-large");
    assert!(report.all_sources_failed());
    let query = DiscoveryQuery::new(vec![RuntimeFamily::Wine], false).unwrap();
    assert_eq!(
        provider.discover(&query).unwrap_err().kind(),
        ProviderErrorKind::ProviderUnavailable
    );
}

#[test]
fn absent_path_hints_do_not_consume_root_budget_or_truncate_explicit_roots() {
    use std::os::unix::fs::PermissionsExt;

    let base = Fixture::new("path-budget");
    let mut path_directories = Vec::new();
    for index in 0..(MAX_ROOTS + 8) {
        let directory = base.path().join(format!("path-{index:02}"));
        fs::create_dir(&directory).unwrap();
        let wine = directory.join("wine");
        fs::write(&wine, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&wine, fs::Permissions::from_mode(0o700)).unwrap();
        path_directories.push(directory);
    }
    let explicit = base.path().join("explicit");
    make_proton(&explicit.join("GE-Proton10-26"), "#!/bin/sh\nexit 0\n");
    let path = std::env::join_paths(&path_directories).unwrap();

    let report = provider(None, Some(path), vec![explicit], true, false).scan();

    assert!(report.runtimes.iter().any(|runtime| {
        runtime.candidate.family() == RuntimeFamily::Proton
            && runtime.lineage == RuntimeLineage::GeProton
    }));
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.reason == "discovery-root-limit")
    );
    assert!(
        report
            .roots
            .iter()
            .any(|root| root.outcome == RootOutcome::Truncated)
    );
    assert!(!report.all_sources_failed());
    assert!(report.roots.iter().any(|root| {
        root.source == SourceKind::Explicit && root.outcome == RootOutcome::Inspected
    }));
    assert!(matches!(
        report.contract.completeness(),
        DiscoveryCompleteness::Partial { .. }
    ));
}

#[test]
fn runtime_and_missing_conventional_roots_preserve_truthful_identity_and_trust() {
    let base = Fixture::new("candidate-with-absent-roots");
    let home = base.path().join("home");
    let explicit = base.path().join("extra-runtimes");
    let launcher = explicit.join("GE-Proton10-26").join("proton");
    fs::create_dir_all(&home).unwrap();
    make_proton(&explicit.join("GE-Proton10-26"), "#!/bin/sh\nexit 0\n");
    let report = provider(
        Some(home),
        Some(OsString::new()),
        vec![explicit],
        true,
        true,
    )
    .scan();

    assert_eq!(report.runtimes.len(), 1);
    assert_eq!(report.runtimes[0].candidate.family(), RuntimeFamily::Proton);
    assert_eq!(report.runtimes[0].lineage, RuntimeLineage::GeProton);
    assert!(!report.all_sources_failed());
    assert!(report.issues.is_empty());
    assert!(launcher.is_file());
    let identity = provider(
        Some(base.path().join("home")),
        Some(OsString::new()),
        vec![base.path().join("extra-runtimes")],
        true,
        true,
    )
    .scan();
    assert_eq!(identity.runtimes.len(), 1);
    assert!(!format!("{identity:?}").contains(&base.path().display().to_string()));
    assert_eq!(
        identity.runtimes[0].candidate.ownership().as_str(),
        "unknown"
    );
    assert_eq!(
        identity.contract.completeness(),
        &DiscoveryCompleteness::Complete
    );
}

#[test]
fn one_failed_source_plus_one_successful_source_is_partial_not_fatal() {
    let base = Fixture::new("mixed-roots");
    let failed = base.path().join("file-root");
    let successful = base.path().join("empty-root");
    fs::write(&failed, b"not a directory").unwrap();
    fs::create_dir(&successful).unwrap();

    let report = provider(None, None, vec![failed, successful], false, false).scan();

    assert!(report.runtimes.is_empty());
    assert!(!report.all_sources_failed());
    assert!(
        report
            .roots
            .iter()
            .any(|root| root.outcome == RootOutcome::Failed)
    );
    assert!(
        report
            .roots
            .iter()
            .any(|root| root.outcome == RootOutcome::Inspected)
    );
    assert!(matches!(
        report.contract.completeness(),
        DiscoveryCompleteness::Partial { .. }
    ));
}

#[test]
fn empty_inventory_and_recognized_candidate_do_not_claim_trust_or_compatibility() {
    let base = Fixture::new("claims");
    let empty = base.path().join("empty");
    fs::create_dir(&empty).unwrap();
    let empty_report = provider(None, None, vec![empty], false, false).scan();
    assert!(empty_report.runtimes.is_empty());
    assert_eq!(
        empty_report.contract.completeness(),
        &DiscoveryCompleteness::Complete
    );

    let roots = base.path().join("runtimes");
    make_proton(&roots.join("GE-Proton10-26"), "#!/bin/sh\nexit 0\n");
    let provider = provider(None, None, vec![roots], false, false);
    let report = provider.scan();
    let candidate = &report.contract.candidates()[0];
    let identity =
        endfield_unix_runtime::RuntimeProvider::inspect_identity(&provider, candidate).unwrap();
    let source = identity.source().unwrap();
    assert_eq!(
        source.verification().state(),
        SourceVerificationState::CandidateIdentified
    );
    assert_eq!(
        source.verification().authenticity(),
        &AuthenticityState::NotAssessed
    );
    assert_eq!(
        source.verification().metadata(),
        &MetadataVerification::NotVerified
    );
    assert!(source.verification().artifact_integrity().is_none());
}

#[test]
fn scan_does_not_write_fixtures_or_execute_wine_or_proton_launchers() {
    use std::os::unix::fs::PermissionsExt;

    let base = Fixture::new("side-effects");
    let bin = base.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let marker = base.path().join("runtime-was-executed");
    let wine = bin.join("wine");
    fs::write(
        &wine,
        format!("#!/bin/sh\nprintf executed > '{}'\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(&wine, fs::Permissions::from_mode(0o700)).unwrap();
    let runtime = base.path().join("custom");
    let proton = runtime.join("GE-Proton10-26/proton");
    make_proton(
        &runtime.join("GE-Proton10-26"),
        &format!("#!/bin/sh\nprintf executed > '{}'\n", marker.display()),
    );
    let wine_before = fs::read(&wine).unwrap();
    let proton_before = fs::read(&proton).unwrap();
    let before = fs::read_dir(base.path()).unwrap().count();
    let before_runtime = fs::read_dir(runtime.join("GE-Proton10-26"))
        .unwrap()
        .count();
    let report = provider(
        None,
        Some(bin.as_os_str().to_os_string()),
        vec![runtime.clone()],
        true,
        false,
    )
    .scan();

    assert_eq!(report.runtimes.len(), 2);
    assert!(!marker.exists());
    assert_eq!(fs::read(&wine).unwrap(), wine_before);
    assert_eq!(fs::read(&proton).unwrap(), proton_before);
    assert_eq!(fs::read_dir(base.path()).unwrap().count(), before);
    assert_eq!(
        fs::read_dir(runtime.join("GE-Proton10-26"))
            .unwrap()
            .count(),
        before_runtime
    );
}
