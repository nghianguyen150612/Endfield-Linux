#![cfg(target_os = "linux")]

use endfield_unix_domain::{PlatformFamily, PlatformProbe};
use endfield_unix_platform_linux::LinuxPlatformProbe;

#[test]
fn reads_and_validates_the_running_kernel_and_architecture() {
    let report = LinuxPlatformProbe::system()
        .inspect()
        .expect("live Linux kernel should be available");
    let snapshot = report.snapshot;
    assert_eq!(snapshot.platform.family, PlatformFamily::Linux);
    assert_eq!(snapshot.platform.kernel.as_ref().unwrap().name, "Linux");
    assert!(
        snapshot
            .platform
            .kernel
            .as_ref()
            .and_then(|kernel| kernel.version.as_deref())
            .is_some_and(|release| !release.is_empty())
    );
    assert_ne!(snapshot.platform.architecture.as_str(), "unknown");
    assert!(snapshot.validate().is_ok());

    let through_domain_trait = LinuxPlatformProbe::system()
        .probe()
        .expect("P007 trait is implemented");
    assert_eq!(through_domain_trait.platform.family, PlatformFamily::Linux);
}
