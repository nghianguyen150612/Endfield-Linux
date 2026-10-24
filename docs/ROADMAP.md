# Roadmap P001–P090

This roadmap preserves the requested development order. P001 established the documentation baseline, and P002 completed the initial Rust workspace and shared executable skeleton. P005–P009 are completed. Other remaining items describe intended work, not shipped features. Linux-first implementation and validation runs through P060; macOS and FreeBSD are experimental in their respective ranges; cross-platform stabilization is P081–P090. No game compatibility claim is implied by a roadmap item.

## Linux-first foundation and product: P001–P060

### Foundation and desktop shell: P001–P010

- **P001** — Establish the product, architecture, naming, security, testing, runtime, and cross-platform baseline.
- **P002** — **Completed** — Create the Rust 2024 workspace and the single `endfield-unix` GUI/CLI executable skeleton. The executable supports help/version and routes no-argument startup to the GUI boundary.
- **P003** — **Completed** — Establish the portable C17 ABI 1.0 library, safe Rust FFI wrapper, independent native/Rust contract tests, and headless CI checks. Valid requests currently return an empty plan set; candidate generation remains P041–P048.
- **P004** — **Completed** — Build the GTK4, libadwaita, and Relm4 application shell with responsive six-page navigation, GTK CSS, SVG resources, display-safe startup, and graphical smoke coverage. Game/runtime/planner services remain future work.
- **P005** — **Completed** — Add typed CLI subcommand parsing, static help/version/about output, shared GUI dispatch, and native CI coverage to the existing executable.
- **P006** — **Completed** — Implement versioned TOML configuration and transactional SQLite storage. Rust workspace, CLI/storage tests, GTK smoke/build, Cargo metadata, GCC, and Clang CI checks passed on `main`.
- **P007** — **Completed** — Add the `endfield-unix-domain` portable library, stable capability and measurement identifiers, typed platform/hardware/display/snapshot contracts, provenance and diagnostics, partial provider composition, and synthetic contract tests. Workspace regression checks, GTK smoke and screenshot checks, Cargo metadata validation, and GCC/Clang C17 ABI jobs passed in hosted CI. No real platform or hardware probing is implemented.
- **P008** — **Completed** — Add the reusable diagnostics library, typed allowlisted local logging, bounded private retention, safe P007 adapters, and headless JSON support reports. No hardware probing or network upload is included.
- **P009** — **Completed** — Add the standalone `endfield-unixd` lifecycle daemon, safe SIGTERM/SIGINT handling, bounded shutdown coordination, and P008 lifecycle events. Hosted Rust, GUI smoke, metadata, GCC, and Clang CI passed. Telemetry, session tracking, and service installation remain future milestones.
- **P010** — **Completed** — Add versioned, authenticated, bounded local IPC and the Linux per-user socket transport. Native hosted checks passed in [Rust CI run for commit `2de9b88`](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40083552910). See [IPC](IPC.md) and [testing evidence](TESTING.md).

### Linux capabilities, telemetry, and sessions: P011–P020

- **P011** — Probe Linux distribution, kernel, architecture, and runtime environment capabilities.
- **P012** — Discover CPU topology, memory, and available power/scheduling interfaces.
- **P013** — Discover GPUs, graphics drivers, Vulkan devices, and hardware-acceleration capabilities.
- **P014** — Discover display modes, compositor/session characteristics, and relevant display capabilities.
- **P015** — Define normalized telemetry units, timestamps, provenance, and missing-data semantics.
- **P016** — Collect CPU and memory observations through Linux telemetry adapters.
- **P017** — Collect GPU utilization, VRAM, thermal, and power observations where available.
- **P018** — Collect FPS and frametime observations through supported, non-invasive providers.
- **P019** — Supervise game sessions independently of GUI lifetime and handle graceful shutdown.
- **P020** — Integrate `endfield-unixd.service` with the Linux user service manager without requiring root by default.

### Runtime and game lifecycle: P021–P040

- **P021** — Define the runtime-provider, source-verification, and compatibility metadata models.
- **P022** — Detect installed Wine, Proton, GE-Proton, and DWProton runners.
- **P023** — Integrate UMU where compatible and appropriate.
- **P024** — Establish trusted-source manifests and runtime integrity verification.
- **P025** — Install verified user-managed runtime builds without automatic replacement of known-good versions.
- **P026** — Record runtime identity, verify installed files, and report integrity failures.
- **P027** — Implement per-machine/profile known-good runtime pinning and explicit runtime selection.
- **P028** — Create and manage isolated Wine prefixes with clear ownership and configuration.
- **P029** — Back up, restore, and recover prefixes and runtime state safely.
- **P030** — Diagnose runner, driver, Vulkan, and prefix compatibility without claiming unsupported fixes.
- **P031** — Import an existing game installation without altering game binaries.
- **P032** — Integrate only verified official game-installation sources; do not invent endpoints.
- **P033** — Validate dependencies and present actionable setup diagnostics.
- **P034** — Implement one-click launch through the selected runtime and prefix.
- **P035** — Connect launch, daemon session tracking, and session metadata.
- **P036** — Handle process exit, interrupted launch, cleanup, and restart recovery.
- **P037** — Validate runtime compatibility evidence and preserve user-selected known-good combinations.
- **P038** — Implement supported game update workflows with integrity checks and user-data preservation.
- **P039** — Verify and repair managed installation state without binary patching.
- **P040** — Gate the Linux launch workflow on installation, runtime, and recovery validation.

### Planner, plans, benchmarks, and Linux validation: P041–P060

- **P041** — Implement C17 snapshot and user-goal ABI validation.
- **P042** — Normalize Rust capability snapshots and safely marshal them across the ABI.
- **P043** — Implement L0 observation-only planning.
- **P044** — Interpret target FPS, resolution, smoothness, visual quality, latency, stability, thermal/noise, and efficiency goals.
- **P045** — Evaluate capability, policy, intensity, and compatibility constraints.
- **P046** — Generate multiple supported optimization candidate plans.
- **P047** — Rank candidates and expose comparable plan scores without presenting them as FPS predictions.
- **P048** — Explain benefits, costs, uncertainty, evidence, and rollback requirements.
- **P049** — Validate plans in Rust, request approval, and execute only supported approved actions.
- **P050** — Integrate planner, GTK/CLI presentation, and daemon observations end to end.
- **P051** — Define repeatable benchmark metadata, provenance, and environment capture.
- **P052** — Implement FPS/frametime benchmark collection and comparison.
- **P053** — Generate evidence-based recommendations from relevant benchmark history.
- **P054** — Implement action rollback and known-good profile restoration.
- **P055** — Save, inspect, compare, and restore user optimization profiles.
- **P056** — Validate low-end GPU planning and conservative fallback behavior.
- **P057** — Validate thermal, fan/noise, power, and laptop session behavior where sensors expose data.
- **P058** — Validate the desktop baseline, including pinned GE-Proton 10-26 observations, without generalizing unverified newer-runner failures.
- **P059** — Validate the Dell Vostro 3510 baseline and report hardware power-delivery risk without claiming a software fix.
- **P060** — Complete Linux release-gate validation, documentation, packaging, and compatibility reporting.

## Experimental macOS support: P061–P070

- **P061** — Assess macOS capabilities, security model, graphics stack, and runtime constraints.
- **P062** — Add an experimental macOS platform probe and capability report.
- **P063** — Add experimental macOS local service and IPC adapters.
- **P064** — Evaluate available runtime providers and installation policy without assuming Wine/Proton parity.
- **P065** — Add experimental hardware and power telemetry providers where supported.
- **P066** — Validate GTK4/libadwaita packaging and main-thread behavior on target macOS systems.
- **P067** — Validate session supervision and local diagnostics on macOS.
- **P068** — Exercise portable planner behavior using macOS normalized snapshots.
- **P069** — Document macOS gaps and gate any game-compatibility statement on real testing.
- **P070** — Decide whether experimental macOS support is ready for cross-platform stabilization.

## Experimental FreeBSD support: P071–P080

- **P071** — Assess FreeBSD capabilities, security model, graphics stack, and runtime constraints.
- **P072** — Add an experimental FreeBSD platform probe and capability report.
- **P073** — Add experimental FreeBSD local service and IPC adapters.
- **P074** — Evaluate available runtime providers and installation policy without assuming Linux compatibility.
- **P075** — Add experimental hardware and power telemetry providers where supported.
- **P076** — Validate GTK4/libadwaita packaging and main-thread behavior on target FreeBSD systems.
- **P077** — Validate session supervision and local diagnostics on FreeBSD.
- **P078** — Exercise portable planner behavior using FreeBSD normalized snapshots.
- **P079** — Document FreeBSD gaps and gate any game-compatibility statement on real testing.
- **P080** — Decide whether experimental FreeBSD support is ready for cross-platform stabilization.

## Cross-platform stabilization: P081–P090

- **P081** — Audit portable core and remove accidental platform dependencies.
- **P082** — Verify C17 ABI compatibility and ownership on every supported target toolchain.
- **P083** — Unify capability, telemetry, session, runtime, and service contracts across adapters.
- **P084** — Validate IPC authentication, permissions, and protocol compatibility on each target.
- **P085** — Validate runtime source verification, pinning, and rollback across supported platforms.
- **P086** — Run cross-platform GUI, accessibility, localization, and worker-lifecycle validation.
- **P087** — Run comparative benchmark and recommendation quality checks without unsupported performance claims.
- **P088** — Complete recovery, upgrade, downgrade, and user-data preservation drills.
- **P089** — Publish platform-specific support status and release evidence.
- **P090** — Establish cross-platform release gates and document the next roadmap.
