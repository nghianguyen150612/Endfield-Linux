use endfield_unix_platform_linux::{gpu_nvidia::SystemNvml, gpu_telemetry::*, *};
use endfield_unix_telemetry::*;
#[test]
fn live_headless_gpu_telemetry_requires_no_vulkan_display_or_daemon() {
    let source = SystemLinuxDataSource::new();
    let base = LinuxPlatformProbe::system().inspect().unwrap().snapshot;
    let context = GpuIdentityContext::discover(&source, base, SnapshotRefId::new(1).unwrap()).ok();
    let count = context.as_ref().map(|c| c.devices().len());
    let provider = LinuxGpuTelemetryProvider::new(source, SystemNvml, context);
    let report = provider
        .collect(
            &CollectionRequest::new(
                SamplingContextId::new(1).unwrap(),
                GPU_METRICS.to_vec(),
                None,
            )
            .unwrap(),
        )
        .unwrap();
    report.batch().validate().unwrap();
    if count == Some(0) || count.is_none() {
        assert!(report.batch().observations().is_empty());
    }
    assert!(
        report
            .batch()
            .observations()
            .iter()
            .filter(|o| o.metric() == MetricId::GpuUtilization)
            .all(|o| !o.state().is_measured())
    );
    println!(
        "GPU telemetry live smoke: verified_gpus={count:?}, observations={}, result={:?}, batch_valid=true",
        report.batch().observations().len(),
        report.completeness()
    );
}
