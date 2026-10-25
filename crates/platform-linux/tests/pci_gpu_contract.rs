use endfield_unix_platform_linux::{MAX_PCI_DEVICES, PciBdf};

#[test]
fn bdf_preserves_domains_and_rejects_invalid_components() {
    let addresses = ["0000:00:02.0", "0001:af:1f.7", "0003:80:00.0"];
    for value in addresses {
        assert_eq!(value.parse::<PciBdf>().unwrap().to_string(), value);
    }
    for value in [
        "00:00:02.0",
        "0000:00:20.0",
        "0000:00:01.8",
        "0000:0g:01.0",
        "0000:00:01.00",
    ] {
        assert!(value.parse::<PciBdf>().is_err(), "{value}");
    }
    assert_eq!(MAX_PCI_DEVICES, 512);
}

#[test]
fn filters_only_display_controller_classes() {
    for class in [0x030000, 0x030200, 0x038000] {
        assert!(PciBdf::is_display_controller_class(class));
    }
    for class in [0x020000, 0x120000, 0x030100, 0x040000] {
        assert!(!PciBdf::is_display_controller_class(class));
    }
}
