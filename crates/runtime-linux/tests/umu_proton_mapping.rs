use endfield_unix_runtime::{
    RuntimeAdapterKind, RuntimeAdapterSelection, RuntimeFamily, RuntimeProvider,
};
use endfield_unix_runtime_linux::{DiscoveryEnvironment, DiscoveryOptions, LinuxRuntimeProvider};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn root() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "endfield-umu-map-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn p022_wine_identity_cannot_be_selected_for_umu_proton() {
    let base = root();
    let wine = base.join("Wine runner");
    fs::create_dir_all(wine.join("bin")).unwrap();
    fs::write(wine.join("bin/wine"), b"not executed").unwrap();
    fs::set_permissions(wine.join("bin/wine"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(wine.join("version"), "wine-synthetic-build\n").unwrap();
    let provider = LinuxRuntimeProvider::new_with_environment(
        DiscoveryOptions {
            additional_roots: vec![base.clone()],
            include_path: false,
            include_conventional: false,
        },
        DiscoveryEnvironment::isolated(None, None),
    )
    .unwrap();
    let report = provider.scan();
    let candidate = report
        .runtimes
        .iter()
        .find(|r| r.candidate.family() == RuntimeFamily::Wine)
        .expect("P022 Wine fixture discovered");
    let identity = RuntimeProvider::inspect_identity(&provider, &candidate.candidate).unwrap();
    assert!(RuntimeAdapterSelection::new(RuntimeAdapterKind::UmuProton, &identity, None).is_err());
    let _ = fs::remove_dir_all(base);
}

#[test]
fn p022_identity_reinspection_rejects_stale_candidate_metadata() {
    let base = root();
    let proton = base.join("GE-Proton10-26");
    fs::create_dir_all(&proton).unwrap();
    fs::write(proton.join("proton"), b"not executed").unwrap();
    fs::set_permissions(proton.join("proton"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(proton.join("version"), "GE-Proton10-26\n").unwrap();
    fs::write(proton.join("compatibilitytool.vdf"), "\"compatibilitytool\" { \"compat_tools\" { \"GE-Proton10-26\" { \"version\" \"GE-Proton10-26\" } } }").unwrap();
    let provider = LinuxRuntimeProvider::new_with_environment(
        DiscoveryOptions {
            additional_roots: vec![base.clone()],
            include_path: false,
            include_conventional: false,
        },
        DiscoveryEnvironment::isolated(None, None),
    )
    .unwrap();
    let report = provider.scan();
    let candidate = report
        .runtimes
        .iter()
        .find(|r| r.candidate.family() == RuntimeFamily::Proton)
        .unwrap();
    fs::write(proton.join("version"), "replacement-build\n").unwrap();
    assert!(RuntimeProvider::inspect_identity(&provider, &candidate.candidate).is_err());
    let _ = fs::remove_dir_all(base);
}
