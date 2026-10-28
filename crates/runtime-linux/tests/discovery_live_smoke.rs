//! Read-only smoke against fixed system executable locations only. User Steam,
//! Heroic, Lutris, and PATH sources remain covered by isolated fixtures/CLI CI.

use endfield_unix_runtime::{
    DiscoveryCompleteness, DiscoveryQuery, RuntimeFamily, RuntimeProvider,
};
use endfield_unix_runtime_linux::{DiscoveryOptions, LinuxRuntimeProvider};

#[test]
fn system_wine_probe_returns_a_valid_empty_or_partial_contract_report() {
    let provider = LinuxRuntimeProvider::new(DiscoveryOptions {
        additional_roots: Vec::new(),
        include_path: false,
        include_conventional: false,
    })
    .expect("fixed empty source selection is valid");
    let query = DiscoveryQuery::new(vec![RuntimeFamily::Wine], false).unwrap();
    let report = provider
        .discover(&query)
        .expect("implemented system discovery runs");
    assert_eq!(report.provider_id().as_str(), "linux-runtime-discovery");
    for candidate in report.candidates() {
        assert_eq!(candidate.family(), RuntimeFamily::Wine);
        assert_eq!(candidate.provider_id(), report.provider_id());
    }
    assert!(matches!(
        report.completeness(),
        DiscoveryCompleteness::Complete | DiscoveryCompleteness::Partial { .. }
    ));
}
