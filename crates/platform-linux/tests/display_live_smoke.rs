use endfield_unix_domain::CapabilityId;
use endfield_unix_platform_linux::{
    LinuxDisplayCompleteness, LinuxDisplayProbe, MAX_DISPLAY_OUTPUTS, MAX_DRM_CONNECTORS,
};

#[test]
fn live_display_probe_is_read_only_bounded_and_preserves_prior_snapshot_facts() {
    let report = LinuxDisplayProbe::system()
        .inspect()
        .expect("Linux platform identity is available");
    report
        .snapshot()
        .validate()
        .expect("combined P011-P014 snapshot validates");

    assert!(report.outputs.len() <= MAX_DISPLAY_OUTPUTS);
    assert!(
        report
            .drm_connectors
            .as_ref()
            .is_none_or(|items| items.len() <= MAX_DRM_CONNECTORS)
    );
    assert!(
        report.snapshot().cpu.is_some(),
        "P012 CPU observation remains"
    );
    assert!(
        report.snapshot().memory.is_some(),
        "P012 memory observation remains"
    );
    assert_eq!(report.snapshot().gpus.len(), report.gpu.pci_devices.len());
    assert!(
        report
            .snapshot()
            .capabilities
            .iter()
            .any(|capability| { capability.id == CapabilityId::Display })
    );
    assert!(
        report
            .snapshot()
            .capabilities
            .iter()
            .any(|capability| { capability.id == CapabilityId::CompositorSession })
    );
    if report.completeness == LinuxDisplayCompleteness::Complete {
        assert!(
            report.session.wayland_endpoint
                == endfield_unix_platform_linux::ProviderStatus::Connected
                || report.session.x11_endpoint
                    == endfield_unix_platform_linux::ProviderStatus::Connected
        );
        assert!(report.snapshot().diagnostics.is_empty());
    }

    println!(
        "live display smoke: session_hint={:?}, verified_protocol={:?}, wayland={:?}/{:?}, x11={:?}/{:?}, selected_source={:?}, outputs={}, drm_status={:?}, drm_connectors={}, completeness={:?}, snapshot_valid=true",
        report.session.hint,
        report.session.verified_protocol,
        report.wayland.status,
        report.wayland.inventory,
        report.x11.status,
        report.x11.inventory,
        report.output_source,
        report.outputs.len(),
        report.drm_connector_status,
        report.drm_connectors.as_ref().map_or(0, Vec::len),
        report.completeness
    );
}
