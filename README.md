# endfield-unix

`endfield-unix` is an independent community launcher and goal-driven optimization application for Arknights: Endfield on Unix-like systems. Linux is the first implementation target. The frontend is Rust 2024 with GTK4, libadwaita, Relm4, GTK CSS, and locally included SVG resources. Optimization decisions belong to the portable C17 library; Rust will validate and execute only user-approved actions.

**Status:** P001 documents the product and architecture. P002 established the Rust 2024 workspace and the single `endfield-unix` executable. P003 established the C17 ABI 1.0 and safe Rust wrapper; valid planner requests still return an empty plan set. P004 adds the first real GTK4/libadwaita/Relm4 application shell with six navigable pages. P005 is completed with typed CLI commands. P006 is completed with a reusable Rust storage library for versioned TOML preferences, safe XDG paths, atomic conflict-aware saves, and transactional SQLite migrations. P007 adds an independent portable domain library with typed platform identities, capabilities, hardware contracts, provenance, snapshot validation, and synchronous probe interfaces. P008 adds structured local logging, typed domain diagnostic adapters, bounded retention, and allowlist-only JSON support reports. P007 and P008 do not add operating-system or hardware detection; platform adapters remain future work. The GTK Settings page does not yet edit or apply stored preferences. Game installation and launch, runtime management, optimization candidates, hardware probes, telemetry, benchmark execution, and the daemon are not implemented.

The public GitHub repository remains [Endfield-Linux](https://github.com/nghianguyen150612/Endfield-Linux). Local application and executable naming is `endfield-unix`; the future companion daemon is reserved as `endfield-unixd`.

## Documentation

- [Product scope and requirements](docs/PRODUCT.md)
- [Architecture and C ABI contract](docs/ARCHITECTURE.md)
- [UI design and component ownership](docs/UI.md)
- [Optimizer ABI v1, ownership, and status codes](docs/ABI.md)
- [Naming rules](docs/NAMING.md)
- [P001–P090 roadmap](docs/ROADMAP.md)
- [Security and recovery principles](docs/SECURITY.md)
- [Testing strategy and validation records](docs/TESTING.md)
- [CLI commands and architecture](docs/CLI.md)
- [Storage paths, configuration, SQLite, and recovery](docs/STORAGE.md)
- [Portable domain models and platform interfaces](docs/DOMAIN.md)
- [Structured diagnostics, log retention, privacy, and support reports](docs/DIAGNOSTICS.md)
- [Rust and C17 architecture decision](docs/adr/0001-rust-c17-architecture.md)
- [Cross-platform boundary decision](docs/adr/0002-cross-platform-boundaries.md)
- [Runtime and rollback decision](docs/adr/0003-runtime-and-rollback-policy.md)

## Build dependencies

Use the pinned Rust 1.85.0 toolchain, a C17 compiler, GTK4 development files, and libadwaita development files. The current adaptive navigation uses libadwaita 1.4 APIs.

Arch Linux:

```sh
sudo pacman -S --needed rustup gtk4 libadwaita pkgconf gcc
rustup toolchain install 1.85.0 --component clippy --component rustfmt
```

Ubuntu 24.04, matching the supported GitHub Actions build environment:

```sh
sudo apt-get install build-essential pkg-config libgtk-4-dev libadwaita-1-dev
```

For graphical smoke tests on Ubuntu, also install `xvfb` and `dbus-daemon`. A GTK display backend and normal user session are required to show a window. GNOME Shell is not required: GTK4/libadwaita applications can run in Wayland or X11 sessions such as Hyprland or Niri when their display and session-bus environment is available.

## Build and run

```sh
cargo +1.85.0 build --locked --workspace
cargo +1.85.0 run --locked -p endfield-unix -- --help
cargo +1.85.0 run --locked -p endfield-unix -- --version
cargo +1.85.0 run --locked -p endfield-unix -- about
cargo +1.85.0 run --locked -p endfield-unix -- diagnose report --output ./support-report.json
cargo +1.85.0 run --locked -p endfield-unix -- help gui
cargo +1.85.0 run --locked -p endfield-unix -- gui
cargo +1.85.0 run --locked -p endfield-unix
```

The no-argument command and `gui` open the same native application window. Help, version, and about do not initialize GTK or create application directories. `diagnose report` is headless and writes only to its explicit output path; it does not probe hardware or upload data. If GTK cannot connect to a working display, GUI startup prints a display-initialization diagnostic and exits with status 69. Invalid arguments print a diagnostic and exit with status 2; report and command output failures use status 74. The application does not infer display availability from `DISPLAY` or `WAYLAND_DISPLAY` alone. See [CLI usage and exit statuses](docs/CLI.md).

The shell provides Home, Optimize, Benchmark, Library, Diagnostics, and Settings. Pages distinguish unavailable services from detected facts. In particular, Play stays disabled until a launch backend exists; system detection and optimization recommendations are not claimed; and the current C17 implementation returns an empty plan set. P005 implements static CLI subcommands; game management, runtime setup, optimization, benchmarking, diagnostics, and daemon interaction remain future work. P006 provides a storage API, but GTK settings are not yet wired to it and stored preferences are not reported as applied. P007 provides a portable representation for future normalized probe results; it does not probe this machine or display synthetic fixture data.

The application does not access game credentials, modify Wine prefixes or system settings, run privileged commands, contact game services, collect telemetry, or start a daemon. Structured logs are local and allowlist-only; generated reports omit private paths, free-form provider details, correlation IDs, credentials, and environment contents. See [diagnostics policy and limitations](docs/DIAGNOSTICS.md). macOS and BSD UI behavior is unverified; Linux-first CI does not establish support on those platforms.
