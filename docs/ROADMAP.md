# Roadmap P001–P090

This roadmap preserves the requested development order. P001 established the documentation baseline, and P002 completed the initial Rust workspace and shared executable skeleton. P005–P023 are implemented locally; P023 hosted qualification passed in [CI run 40684639686](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40684639686), with Rust/GTK, GCC C17, and Clang C17 jobs successful. P022 hosted qualification passed in [CI run 40648593964](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40648593964), with the Rust/GTK, GCC C17, and Clang C17 jobs all successful. The first P022A follow-up [run 40647230072](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40647230072) passed the empty-host fixture and real CLI smoke but failed an existing CLI help assertion whose expected command list omitted `runtimes`; the assertion was corrected. The initial P022 Rust job failed at the runtime smoke in [CI run 40619500314](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40619500314). These CI results validate discovery behavior, not runtime compatibility or target-device behavior. P018 hosted checks passed on a rerun; the first attempt reproduced the existing IPC capacity race, which remains unresolved and is recorded in [TESTING.md](TESTING.md). P019 hosted qualification passed in the supplied [CI run 40441896200](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40441896200), with all three jobs successful on the first attempt; this is CI evidence, not Endfield gameplay validation. P020 hosted qualification passed in [CI run 40505016060](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40505016060), also with all three jobs successful. P021 hosted qualification passed in [CI run 40527116968](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40527116968), with all three jobs successful. Real systemd user-manager, physical-device runtime discovery, and Endfield gameplay validation remain unverified. Other remaining items describe intended work, not shipped features. Linux-first implementation and validation runs through P060; macOS and FreeBSD are experimental in their respective ranges; cross-platform stabilization is P081–P090. No game compatibility claim is implied by a roadmap item.

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

- **P011** — **Completed** — Add the read-only Linux platform adapter, distribution parser, running-kernel and architecture identity, conservative container/WSL assessment, safe runtime-directory assessment, and headless `diagnose platform` command. Hosted Rust, GTK smoke/screenshot, IPC/daemon, metadata, GCC, and Clang checks passed in [CI run 40087592204](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40087592204). No Arch Linux physical-device validation is claimed.
- **P012** — **Completed** — Add bounded, read-only Linux CPU topology/model, process affinity and scheduler queries, RAM/swap, cgroup v2 resource controls, cpufreq and platform profile discovery, typed P007 mapping, partial reports, and headless `diagnose hardware`. Hosted Rust, GTK smoke/screenshot, CLI, IPC/daemon, metadata, GCC, and Clang qualification passed in [CI run 40161140586](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40161140586). Real Arch physical-device validation remains unverified.
- **P013** — **Completed** — Add bounded read-only PCI/DRM GPU discovery, kernel-driver binding, Vulkan physical-device enumeration, conservative correlation, typed snapshot integration, and `diagnose gpu`. Hosted checks passed in [CI run 40170150212](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40170150212); target Arch hardware validation remains unverified. See [GPU-LINUX.md](GPU-LINUX.md).
- **P014** — **Completed** — Add read-only Wayland/X11 session and output discovery, exact Linux display modes, conservative DRM connectors, P007 snapshot mapping, and headless `diagnose display`. Hosted Rust, GTK, CLI, Xvfb/RandR, IPC/daemon, metadata, GCC, and Clang qualification passed in [CI run 40176731426](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40176731426). Real Arch, Dell Vostro, Hyprland, and Niri validation remains unverified. See [DISPLAY-LINUX.md](DISPLAY-LINUX.md).
- **P015** — **Completed** — Add the library-only `endfield-unix-telemetry` crate with stable metric identities, normalized units, monotonic clock domains, observation windows, provenance, typed missing-data and quality states, freshness evaluation, snapshot-local targets, checked counter/rate helpers, bounded deterministic batches, and a future provider boundary. Hosted Rust/GTK/Linux/IPC and GCC/Clang qualification passed in [CI run 40184991675](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40184991675). P015 contains synthetic examples only; Linux telemetry collection remains P016–P018.
- **P016** — **Completed** — Collect one-shot CPU, per-logical-CPU, memory, swap, cgroup v2, and optional current-frequency observations through the Linux telemetry adapter. Hosted Rust/GTK/Linux smoke, IPC, metadata, GCC, and Clang qualification passed in [CI run 40205107552](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40205107552). Real Arch desktop and Dell Vostro validation remain unverified. See [Linux telemetry](TELEMETRY-LINUX.md) and the [validation record](TESTING.md).
- **P017** — **Completed** — Add optional read-only Linux GPU telemetry through PCI-correlated AMDGPU, Intel i915 and NVIDIA NVML gauges, conservative power-state skipping, validated P015 batches, and headless `diagnose telemetry gpu`. Utilization remains unsupported without a truthful rate window. Hosted qualification passed in [CI run 40316047680](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40316047680). Physical Arch targets remain unverified; see [implementation report](P017-IMPLEMENTATION-REPORT.md).
- **P018** — **Implemented; hosted checks passed on rerun** — Import explicitly selected canonical frame-event and supported MangoHud CSV files into validated P015 observations without automatic capture. [Hosted CI run 40341956819](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40341956819) passed on retry after its first attempt hit the known IPC capacity race; that race is not fixed by P018. See [frame telemetry](FRAME-TELEMETRY-LINUX.md) and the [qualification record](TESTING.md).
- **P019** — **Implemented; hosted qualification passed** — Add a reusable bounded session library, daemon-owned child monitoring, typed lifecycle/outcome summaries, and shutdown that cancels monitoring without terminating child processes. The supplied [CI run 40441896200](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40441896200) passed all three jobs on its first attempt. This does not establish real Endfield gameplay validation. No production launch, recovery, public session IPC, or automatic telemetry is included. See [SESSION-SUPERVISION.md](SESSION-SUPERVISION.md).
- **P020** — **Implemented; hosted qualification passed** — Add a canonical `Type=notify` user unit, safe user-local installation and owned-file removal, fixed `systemctl --user` lifecycle commands, readiness only after authenticated IPC Ping, graceful STOPPING notification, and bounded restart policy. All three jobs passed in [CI run 40505016060](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40505016060). Installation is opt-in and autostart remains disabled until enabled. `KillMode=control-group` is retained; no game-survival or logout guarantee is made. Real user-manager integration remains **UNVERIFIED** because the optional isolated-account gate was not enabled. See [SYSTEMD-USER.md](SYSTEMD-USER.md).

### Runtime and game lifecycle: P021–P040

- **P021** — **Completed** — Add portable provider, identity, provenance, integrity, compatibility, ownership, capability negotiation, and non-executable LaunchPlan contracts without performing runtime discovery or launch. Local Rust 1.85.0 checks passed; all three hosted jobs passed in [CI run 40527116968](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40527116968). This qualification does not establish real systemd user-manager behavior or game compatibility. See [runtime provider contracts](RUNTIME-PROVIDERS.md) and [testing evidence](TESTING.md).
- **P022** — **Implemented; hosted qualification passed** — Add bounded read-only Linux discovery of system/PATH Wine, Steam and Flatpak Steam Proton, GE-Proton/DWProton claims, Heroic/Lutris runners, and explicit roots through P021 contracts. P022A corrects absence/failure semantics after the first hosted smoke failure and adds isolated regressions. The corrected [run 40648593964](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40648593964) passed all three hosted jobs. No trust, integrity, compatibility, install, or launch claim is made. See [Linux runtime discovery](RUNTIME-DISCOVERY-LINUX.md) and [validation record](TESTING.md).
- **P023** — **Implemented; hosted qualification passed** — Add bounded read-only UMU discovery, explicit UMU-Proton mapping to selected P022 candidates, UMU environment contracts, and non-executable P021 launch plans. All three hosted jobs passed in [CI run 40684639686](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40684639686). UMU execution, trusted source verification, installed-file integrity, prefix management, and Endfield compatibility remain unverified. See [UMU integration](UMU-INTEGRATION.md) and [validation record](TESTING.md).
- **P024** — Implement offline exact-byte Ed25519 manifests, scoped anchors, checkpoint/expiry checks, streaming SHA-256 and read-only CLI verification. Real publisher authenticity remains UNVERIFIED. See [runtime trust manifests](RUNTIME-TRUST-MANIFESTS.md) and [qualification](TESTING.md).
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

## P017 GPU telemetry

Read-only Linux GPU telemetry is implemented through PCI-correlated optional
AMDGPU sysfs/hwmon, Intel i915 current frequency, and dynamically loaded NVIDIA
NVML gauges. `endfield-unix diagnose telemetry gpu --interval-ms 250` runs
headlessly, without Vulkan/display initialization or the daemon. Unsupported
utilization windows and missing sensors remain explicit partial data. Suspended
and unknown-power devices are skipped conservatively. No live samples are
persisted or included in default support reports. See [GPU-TELEMETRY-LINUX.md](GPU-TELEMETRY-LINUX.md) for source semantics, limitations, tests, and manual physical-device checks.
