#[test]
#[ignore = "requires an X11 display and D-Bus session; CI runs this under Xvfb"]
fn gtk_window_navigation_resize_and_close_smoke_test() {
    let summary = endfield_unix::ui::run_smoke_test()
        .unwrap_or_else(|error| panic!("GTK smoke test failed: {error}"));

    assert_eq!(summary.header_identity, "endfield-unix");
    assert_eq!(summary.navigation_destinations, 6);
    assert_eq!(
        summary.visited_pages,
        [
            "home",
            "optimize",
            "benchmark",
            "library",
            "diagnostics",
            "settings"
        ]
    );
    assert_eq!(
        summary.page_titles,
        [
            "Home",
            "Optimize",
            "Benchmark",
            "Library",
            "Diagnostics",
            "Settings"
        ]
    );
    assert_eq!(summary.initial_size, (1100, 720));
    assert_eq!(summary.resized_size, (960, 660));
    assert!(summary.window_resizable);
    assert!(summary.svg_loaded);
    assert!(summary.closed_cleanly);
}
