# endfield-unix

`endfield-unix` is an independent community launcher and goal-driven optimization application for Arknights: Endfield on Unix-like systems. Linux is the first implementation target. The frontend is Rust 2024 with GTK4, libadwaita, Relm4, GTK CSS, and locally included SVG resources. Optimization decisions belong to the portable C17 library; Rust will validate and execute only user-approved actions.

**Status:** P001 documents the product and architecture. P002 established the Rust 2024 workspace and `endfield-unix` executable. P003 established C17 ABI 1.0 and its safe Rust wrapper; valid planner requests still return an empty plan set. P004 adds the GTK4/libadwaita/Relm4 application shell. P005 adds typed CLI commands. P006 adds versioned TOML preferences and transactional SQLite migrations. P007 adds portable domain models and probe interfaces. P008 adds structured local logging and privacy-safe support reports. P009 adds the independent `endfield-unixd` foreground daemon with a typed lifecycle, SIGTERM/SIGINT handling, and graceful shutdown. P010 adds Linux-only authenticated local IPC with versioned Ping and Status requests. P011 implements read-only Linux distribution, kernel, architecture, and runtime-environment discovery; hosted qualification passed in [CI run 40087592204](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40087592204). P012 adds read-only Linux CPU, memory, cgroup, scheduling, cpufreq, and power-interface inventory with `diagnose hardware`. P013 adds read-only PCI/DRM GPU, kernel graphics-driver, Vulkan physical-device, and conservative memory discovery with `diagnose gpu`; hosted qualification passed in [CI run 40170150212](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40170150212), while physical Arch desktop/laptop validation remains unverified. P014 implements read-only Wayland/X11 session and output discovery, precise Linux mode rates, conservative DRM connector reporting, and `diagnose display`; hosted qualification passed in [CI run 40176731426](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40176731426); real Arch/Wayland desktop validation remains unverified. P016 implements one-shot Linux CPU and memory telemetry through `diagnose telemetry cpu-memory`; hosted Rust, GTK, Linux smoke, IPC, GCC, and Clang qualification passed in [CI run 40205107552](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40205107552). P017 adds optional read-only Linux GPU gauges. P019 adds daemon-owned bounded process supervision for trusted owned child handles, without implementing game launching or telemetry collection; its supplied hosted qualification passed in [CI run 40441896200](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40441896200). P020 adds optional per-user systemd service management and truthful daemon readiness; its hosted Rust, GTK, IPC, screenshot, GCC, and Clang qualification passed in [CI run 40505016060](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40505016060). Real user-manager integration remains unverified. See [GPU discovery](docs/GPU-LINUX.md), [display discovery](docs/DISPLAY-LINUX.md), [Linux telemetry](docs/TELEMETRY-LINUX.md), [session supervision](docs/SESSION-SUPERVISION.md), and [systemd user services](docs/SYSTEMD-USER.md). The workspace has exactly two application executables: `endfield-unix` (GUI and CLI) and `endfield-unixd` (daemon). The GTK Settings page does not yet edit or apply stored preferences.

P015 adds the portable [`endfield-unix-telemetry` contract library](docs/TELEMETRY.md), including metric identities, units, timestamps, provenance, quality, freshness, missing-data states, snapshot-scoped targets, and bounded batches. P016 maps Linux CPU counters, RAM/swap gauges, cgroup v2 memory, and optional current frequency into those contracts. P017 adds optional GPU thermal, power, frequency and memory gauges. P018 imports explicitly selected timestamped frame events and supported MangoHud CSV variants into P015 without starting game capture. Process telemetry remains future work. See [Linux telemetry](docs/TELEMETRY-LINUX.md) and [frame telemetry](docs/FRAME-TELEMETRY-LINUX.md).

P021 adds the portable endfield-unix-runtime library with typed runtime
families, Wine/Proton adapter contracts, provider capability negotiation,
source and integrity metadata, compatibility evidence, ownership policy, and
non-executable LaunchPlan previews. Its three-job hosted qualification passed
in [CI run 40527116968](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40527116968).
P022 adds bounded read-only Linux discovery of installed Wine, Steam Proton,
GE-Proton/DWProton claims, Heroic/Lutris runners, and explicit runtime
directories through P021 provider contracts. `diagnose runtimes` never runs a
candidate, performs network access, or saves an inventory. Discovery does not
verify authenticity, complete integrity, launch readiness, or game
compatibility. P022A hosted qualification passed in [CI run 40648593964](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40648593964).
P023 adds read-only UMU discovery and non-executable UMU-Proton plan preparation.
P023 hosted qualification passed in [CI run 40684639686](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40684639686).
UMU version, authenticity, artifact integrity, execution readiness, and Endfield
compatibility remain unverified. See [UMU integration](docs/UMU-INTEGRATION.md),
[Linux runtime discovery](docs/RUNTIME-DISCOVERY-LINUX.md), and
[runtime provider contracts](docs/RUNTIME-PROVIDERS.md).

The public GitHub repository remains [Endfield-Linux](https://github.com/nghianguyen150612/Endfield-Linux). The application and daemon executable names are `endfield-unix` and `endfield-unixd`.

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
- [Normalized telemetry contracts](docs/TELEMETRY.md)
- [Linux CPU and memory telemetry](docs/TELEMETRY-LINUX.md)
- [Linux frame telemetry import](docs/FRAME-TELEMETRY-LINUX.md)
- [Linux platform and runtime discovery](docs/PLATFORM-LINUX.md)
- [Linux hardware discovery](docs/HARDWARE-LINUX.md)
- [Linux GPU discovery](docs/GPU-LINUX.md)
- [Linux display and session discovery](docs/DISPLAY-LINUX.md)
- [Structured diagnostics, log retention, privacy, and support reports](docs/DIAGNOSTICS.md)
- [Daemon lifecycle and shutdown behavior](docs/DAEMON.md)
- [P019 session supervision](docs/SESSION-SUPERVISION.md)
- [P020 systemd user service](docs/SYSTEMD-USER.md)
- [P021 runtime provider contracts](docs/RUNTIME-PROVIDERS.md)
- [P022 Linux Wine and Proton discovery](docs/RUNTIME-DISCOVERY-LINUX.md)
- [P023 UMU integration and safe launch-plan preparation](docs/UMU-INTEGRATION.md)
- [Authenticated local IPC protocol and Linux transport](docs/IPC.md)
- [Rust and C17 architecture decision](docs/adr/0001-rust-c17-architecture.md)
- [Cross-platform boundary decision](docs/adr/0002-cross-platform-boundaries.md)
- [Runtime and rollback decision](docs/adr/0003-runtime-and-rollback-policy.md)

## Build dependencies

Use the pinned Rust 1.85.0 toolchain. Building the GUI also requires a C17 compiler, GTK4 development files, and libadwaita development files; the daemon itself does not require GTK. The current adaptive navigation uses libadwaita 1.4 APIs.

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
cargo +1.85.0 run --locked -p endfield-unix -- diagnose platform
cargo +1.85.0 run --locked -p endfield-unix -- diagnose hardware
cargo +1.85.0 run --locked -p endfield-unix -- diagnose gpu
cargo +1.85.0 run --locked -p endfield-unix -- diagnose display
cargo +1.85.0 run --locked -p endfield-unix -- diagnose runtimes
cargo +1.85.0 run --locked -p endfield-unix -- diagnose runtimes --family proton --root /absolute/path/to/runners
cargo +1.85.0 run --locked -p endfield-unix -- diagnose umu
cargo +1.85.0 run --locked -p endfield-unix -- diagnose telemetry cpu-memory
cargo +1.85.0 run --locked -p endfield-unix -- diagnose telemetry cpu-memory --interval-ms 500
cargo +1.85.0 run --locked -p endfield-unix -- diagnose telemetry frames --format frame-events-v1 --input ./frames.csv
cargo +1.85.0 run --locked -p endfield-unix -- diagnose telemetry frames --format mangohud-csv --input ./mangohud.csv
cargo +1.85.0 run --locked -p endfield-unix -- help gui
cargo +1.85.0 run --locked -p endfield-unix -- gui
cargo +1.85.0 run --locked -p endfield-unix
cargo +1.85.0 run --locked -p endfield-unixd -- --help
cargo +1.85.0 run --locked -p endfield-unixd -- --foreground
cargo +1.85.0 run --locked -p endfield-unix -- service ping
cargo +1.85.0 run --locked -p endfield-unix -- service status
```

The no-argument command and `gui` open the same native application window. Help, version, and about do not initialize GTK or create application directories. `diagnose report` is headless and writes only to its explicit output path; it does not probe the host or upload data. `diagnose platform`, `diagnose hardware`, `diagnose gpu`, and `diagnose display` are headless, read bounded Linux sources, need no daemon, and do not initialize GTK or change system state. GPU facts distinguish PCI presence, driver binding, DRM access, Vulkan enumeration, and application rendering; see [GPU-LINUX.md](docs/GPU-LINUX.md). Display facts distinguish session hints, live protocol connections, output modes, DRM connectors, and compositor identity; desktop environment names remain unverified hints unless a live source proves identity. See [DISPLAY-LINUX.md](docs/DISPLAY-LINUX.md). Hardware and display facts are displayed locally only when explicitly requested; they are not automatically added to logs or support reports. If GTK cannot connect to a working display, GUI startup prints a display-initialization diagnostic and exits with status 69. Invalid arguments print a diagnostic and exit with status 2; report and command output failures use status 74. The application does not infer display availability from `DISPLAY` or `WAYLAND_DISPLAY` alone. See [CLI usage and exit statuses](docs/CLI.md), the [Linux platform adapter](docs/PLATFORM-LINUX.md), and [Linux hardware discovery](docs/HARDWARE-LINUX.md).

The shell provides Home, Optimize, Benchmark, Library, Diagnostics, and Settings. Pages distinguish unavailable services from detected facts. In particular, Play stays disabled until a launch backend exists; P012 inventory and P016 CPU/memory telemetry, P013 GPU discovery, and P014 display/session discovery are available through headless CLI commands but are not integrated into the GUI or optimizer; the current C17 implementation returns an empty plan set. Game management, runtime setup, optimization, benchmarking, and GUI-to-daemon probe queries remain future work. P006 provides a storage API, but GTK settings are not yet wired to it and stored preferences are not reported as applied. Synthetic fixtures remain test-only and are never displayed as host data.

The GUI does not start or own the daemon, and closing the GUI does not stop an independently launched daemon or user service. P020 adds explicit opt-in per-user systemd installation and lifecycle commands; it still does not launch games, expose session control over IPC, or collect session telemetry. P019 shutdown stops monitoring without signaling owned child processes, while systemd's conservative `KillMode=control-group` can clean up remaining processes on service stop; P020 makes no game-survival claim. Real Endfield launch and survival are unverified until P034/P035. Neither executable accesses game credentials or runs privileged commands. Structured logs are local and allowlist-only; generated reports omit private paths, free-form provider details, correlation IDs, credentials, and environment contents. See [diagnostics policy](docs/DIAGNOSTICS.md), [daemon lifecycle](docs/DAEMON.md), [session supervision](docs/SESSION-SUPERVISION.md), [systemd user services](docs/SYSTEMD-USER.md), and [IPC protocol](docs/IPC.md). macOS and BSD UI behavior is unverified; Linux-first CI does not establish support on those platforms.

## P017 GPU telemetry

Read-only Linux GPU telemetry is implemented through PCI-correlated optional
AMDGPU sysfs/hwmon, Intel i915 current frequency, and dynamically loaded NVIDIA
NVML gauges. `endfield-unix diagnose telemetry gpu --interval-ms 250` runs
headlessly, without Vulkan/display initialization or the daemon. Unsupported
utilization windows and missing sensors remain explicit partial data. Suspended
and unknown-power devices are skipped conservatively. No live samples are
persisted or included in default support reports. See [GPU-TELEMETRY-LINUX.md](docs/GPU-TELEMETRY-LINUX.md) for source semantics, limitations, tests, and manual physical-device checks.

## P018 frame telemetry import

`diagnose telemetry frames` reads only an explicitly selected file. The
versioned `frame-events-v1` format supports measured frame-event rate and
frametime statistics; MangoHud legacy and versioned-v1 CSV headers retain
sampled frametime meaning and do not fabricate a recording-wide FPS window.
The command is headless and does not instrument a game or persist data. See
[FRAME-TELEMETRY-LINUX.md](docs/FRAME-TELEMETRY-LINUX.md) for upstream format
evidence, statistics, limits, privacy, and validation status.

## P024 runtime manifest verification

The library-only offline verifier and headless `diagnose runtime-integrity` command
verify exact-byte Ed25519 signatures, scoped explicit anchors, version/expiry policy,
and actual local artifact size/SHA-256. User-pinned matches remain limited trust
(exit 3), without official publisher authentication. Discovery and UMU remain
unverified; no download, extraction, installation or runtime execution is added.
See [runtime trust manifests](docs/RUNTIME-TRUST-MANIFESTS.md) for schema, CLI flags, evidence mapping,
file-race and rollback limits, synthetic tests, and P025/P026 boundaries.

## P025 runtime installation

P025 now provides local tar.gz installation with P024 artifact verification, a deliberate user-pinned limited-trust path, exact-plan approval, private staging, bounded extraction, no-overwrite publication and transaction recovery. It does not establish publisher authenticity or support HTTPS acquisition. Hosted qualification is pending. See [Linux runtime installation](docs/RUNTIME-INSTALLATION-LINUX.md) for the workflow and current evidence.
