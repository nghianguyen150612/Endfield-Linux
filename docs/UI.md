# Native UI foundation

P004 introduces the native desktop shell; it does not connect the window to game, runtime, hardware, planner, benchmark, or daemon services.

## Navigation and page ownership

The one `endfield-unix` application uses the stable GTK application ID `io.github.nghianguyen150612.endfieldunix`. `ui::app::App` is the Relm4 component and owns the `AppModel`, which tracks the active `Page`, window lifecycle, and whether the adaptive navigation sidebar or page is visible. `AppMessage` contains view-level requests such as navigation, compact-layout changes, return-to-sidebar, and window close.

`ui::navigation` owns page identity and creates an `AdwNavigationSplitView`, `GtkStackSidebar`, and `GtkStack`. libadwaita collapses the split view as space narrows; in collapsed mode the header's Back button returns to page navigation. The stack and sidebar use the same six `Page` values, so page IDs, labels, icons, and selected content stay aligned.

Each module in `ui::pages` builds only its view subtree. Home sends a destination request for Optimize or Library; these callbacks go through the Relm4 sender. Pages do not query GTK platform state, access the filesystem, probe hardware, invoke the C ABI, or run processes. The C17 planner remains in `optimizer-ffi` and is not called by P004.

## Main-thread and future service boundaries

GTK, libadwaita, Relm4 model updates, CSS setup, and window lifecycle work occur on the GUI main thread. P004 starts no asynchronous runtime and has no background workers. Future blocking probes, downloads, optimization execution, and telemetry should run through Relm4 workers or other service boundaries and return typed messages to the component; GTK widgets must still be mutated on the main thread.

The page text currently represents implementation availability, not detected machine state. A future service integration should replace those labels with explicit `unknown`, `unavailable`, or measured states. It must not treat missing hardware/telemetry data as zero or infer runtime detection from the absence of configuration.

## Visual tokens and resources

`ui/resources/style.css` defines a compact local palette and a small set of shell classes:

| Token | Value or use |
| --- | --- |
| `endfield_yellow` | `#d3b34b`, muted industrial accent |
| `endfield_yellow_soft` | Accent at low opacity for the Home callout |
| `endfield_border` | Subtle border derived from the active theme text color |
| Surface cards | Neutral theme-derived translucent panel backgrounds |

The application prefers libadwaita's dark color scheme within its own process for the graphite desktop appearance; it does not change the desktop's theme. CSS styles named application classes rather than replacing global GTK widget styling. The original `icons/endfield-mark.svg` is embedded with `include_bytes!`; CSS is embedded with `include_str!` and installed after GTK initialization. If SVG decoding is unsupported, the header uses a bundled GTK symbolic icon. No GResource toolchain step, remote asset, game artwork, or external font is required.

## Accessibility and responsiveness

The shell uses GTK and libadwaita widgets for the header, navigation, buttons, labels, and scrollable content. Navigation entries and icon-only concepts retain text names. Disabled Play and import actions have visible labels and explanatory tooltips. Text wraps in status cards, the page body scrolls, and navigation adapts to the allocated width. The 1100 × 720 default is resizable and is not tied to a physical monitor size.

Native typography, system font scaling, keyboard traversal, and GTK focus indications are left intact. Color communicates emphasis but does not carry status by itself. The page stack has no transition animation, and the application performs no continuous animation; reduced-motion settings therefore do not need a custom animation override.

## Unimplemented work

The Optimize page reserves sections for target FPS, playing preference, intensity, capabilities, candidate plans, benchmark results, review, and rollback. They are status-only placeholders. The C17 ABI still returns an empty plan set, so the interface does not claim that the planner produces recommendations. Benchmark, Library, Diagnostics, and Settings likewise identify unavailable behavior without fabricated system details or persisted preferences.

The shell is validated for Linux CI only. macOS and BSD GTK/libadwaita packaging and behavior remain unverified.
