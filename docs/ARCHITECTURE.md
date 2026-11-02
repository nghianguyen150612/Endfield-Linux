# Architecture baseline

## Status and dependency direction

P001 established these intended boundaries. P002 established the Rust workspace and shared executable; P003 added the C17 ABI skeleton; P004 adds the GTK shell; P005 completes the typed CLI foundation; P006 adds local persistence; P007 adds portable domain models and probe interfaces; P008 adds structured diagnostics and safe support reports; P009 adds the daemon lifecycle; P010 adds a versioned Linux local IPC channel; P011 adds Linux platform identity discovery; P012 adds read-only Linux CPU and memory resource inventory; P013 adds read-only Linux PCI/DRM, graphics-driver, and Vulkan discovery; P014 adds read-only Linux display-server/session and DRM connector discovery in the same adapter; P015 adds portable normalized telemetry contracts; P016 adds one-shot Linux CPU and memory telemetry; P017 adds optional PCI-correlated GPU gauges without Vulkan initialization; P018 adds bounded on-demand frame-log ingestion; P019 adds the in-memory daemon-owned process supervisor; P020 adds optional Linux user-service lifecycle management; P021 defines portable runtime provider, source verification, compatibility, ownership, and non-executable launch-plan contracts; P022 adds bounded read-only Linux Wine and Proton discovery; P023 adds bounded UMU discovery, explicit UMU-Proton preparation, and non-executable plan construction. Runtime acquisition, installed-file integrity audits, and production game launch remain unimplemented. The design is Linux-first while keeping portable lifecycle contracts independent from Linux process APIs. Dependency direction is:

```text
GTK4 desktop / CLI ──> Rust application services ──> portable domain contracts
                                  │                             │
                                  ├──> platform adapters        └──> C17 planner
                                  ├──> runtime-linux adapter ──> portable runtime contracts
                                  ├──> compatibility adapter ──> portable evidence assessment
                                  ├──> storage crate and shared IPC client
                                  └──> session daemon client
```

The GTK frontend and CLI use one Rust application core and one GUI/CLI executable named `endfield-unix`. The only companion executable is `endfield-unixd`. There is no separate GUI binary or CLI binary. GTK widgets and GTK-facing state remain on the GUI main thread. Filesystem, process, network, telemetry, benchmark, and other potentially blocking work runs in workers; workers send results back to the GUI through Relm4 messages. The synchronous C planner is bounded, deterministic planning work and must not perform I/O.

The desktop stack is fixed: Rust 2024, GTK4, libadwaita, Relm4, GTK CSS, and SVG resources. C optimizer code targets C17. Rust owns the application lifecycle, CLI argument handling, UI integration, game and runtime management, platform adapters, telemetry ingestion, daemon lifecycle and IPC, SQLite/TOML persistence, safe FFI wrappers, plan validation/execution, and recovery.

P021 adds endfield-unix-runtime as a portable contract library. P022/P023 add
the library-only endfield-unix-runtime-linux adapter, consumed by the headless
`diagnose runtimes` and `diagnose umu` CLI commands. The Linux adapter depends
on the portable runtime contracts and libc's bounded no-follow file-open
flags. It is independent of GTK, Vulkan, Wayland/X11 discovery, storage, IPC,
daemon, and process execution. The portable runtime crate itself remains
Linux-independent.

P030 adds `endfield-unix-compatibility-diagnostics`, a portable serde-only
assessment crate. It accepts normalized evidence and returns deterministic
typed findings; it does not acquire observations. The application CLI adapter
reuses P027 read-only resolution, P022/P025/P026 identity validation, P013
Linux GPU/Vulkan probing, P028 prefix inspection, and P029 metadata-only
recovery observation. Linux, filesystem, GTK, Vulkan, and P029 types stay
outside the portable assessment crate.

## P002 workspace and entry point

The Cargo workspace uses resolver version 3 and Rust 2024 with MSRV 1.85. It contains two application packages and exactly two application binaries: `apps/endfield-unix` produces `endfield-unix` (GTK GUI and CLI), and `apps/endfield-unixd` produces `endfield-unixd` (foreground daemon). The daemon package has no GTK, libadwaita, or Relm4 dependency. P004 adds Relm4 0.9.1, GTK4-rs 0.9.6, and libadwaita-rs 0.7.2; these bindings use the matching GTK4 0.9 / GLib 0.20 generation.

At the P002 baseline, dispatch recognized only `--help`, `--version`, and no arguments. P005 replaces that initial parser with the typed subcommand architecture below. `main` still obtains arguments as OS strings, skips the executable name, and delegates process work to the shared library. Help, version, about, and argument errors are resolved before GTK initialization. No arguments and `gui` call the same `start_gui` function; P004 initializes GTK/libadwaita on the main thread and starts the Relm4 application. Display initialization errors return status 69 with a diagnostic rather than panicking.

P002 performs no hardware, filesystem, process, network, Wine, Proton, or game probing. Invalid input and output failures return explicit nonzero statuses and diagnostics.

P003 adds a workspace library package, `optimizer-ffi`, for the C17 build integration and safe FFI wrapper without adding an executable. P004 implements the GTK shell in the `endfield-unix` package. P009 adds the sole companion executable `endfield-unixd`; no separate GUI or CLI executable is introduced.

## P004 native application shell

The stable GTK application ID is `io.github.nghianguyen150612.endfieldunix`; GLib validates it before creating the application. `ui::app::App` is the top-level Relm4 component. Its model owns the active page, compact-navigation state, and window lifecycle. `AppMessage` carries navigation, responsive-layout, back-navigation, and close requests. GTK widgets are created and mutated only on the GUI main thread.

`ui::navigation` defines the six page identities and builds an `AdwNavigationSplitView` with a `GtkStackSidebar` and content stack. libadwaita collapses the split view at narrower widths; the header exposes a back affordance to return to the sidebar. `ui::pages` owns each page's view subtree. Page builders display shell status and send only navigation requests; they do not access services, system probes, or the C planner. Home keeps Play disabled until a launch service exists. Optimize identifies the C17 empty plan set and does not invoke an optimization workflow.

`ui::resources` embeds GTK CSS and the original SVG mark into the executable. CSS is installed only after graphics initialization. SVG decoding has a symbolic GTK icon fallback. CLI help, version, and about do not initialize GTK or load GUI resources. No GResource compiler or second binary is introduced.

The application window starts at 1100 × 720 logical pixels and remains resizable. AdwNavigationSplitView and scrollable page content adapt to narrower windows. GTK's platform typography, system font scaling, keyboard traversal, accessible widget roles, and native focus indication are retained. The stack uses no transition animation. The interface contains no network-loaded resource or continuously animated content.

The page model is presentation state, not a service layer. Future platform probing, runtime, library, benchmark, planner, and diagnostics work must arrive through service boundaries and Relm4 messages/workers; those integrations are not implemented in P004. No GTK widget or display-server interface enters the C17 planner or optimizer FFI. The selected `v1_4` libadwaita bindings require system libadwaita 1.4 or newer. macOS and BSD behavior remains unverified.

Define interfaces when implementation reaches the relevant feature; keep OS-specific behavior behind adapters. The following responsibilities anchor that design:

| Boundary | Responsibility |
| --- | --- |
| `PlatformProbe` | P007's synchronous boundary for a normalized snapshot. The generic composite probe retains successful subsystem reports when another provider fails. P011's Linux adapter implements it with kernel identity and optional distribution metadata. |
| `CapabilityProvider` | P007's synchronous boundary for one independent capability source and a partial discovery report. |
| `TelemetryProvider` | P015's synchronous boundary for declared metrics, caller-supplied sampling context, partial batches, and typed fatal failures. P016 implements bounded on-demand Linux CPU/memory collection; P017 implements optional Linux GPU gauges; P018 implements bounded frame-log import. |
| `RuntimeProvider` | P021's typed boundary for declaring capabilities and requesting discovery, inspection, source/integrity verification, compatibility assessment, installation plans, and data-only launch plans. P021 does not implement those system operations. |
| `HardwareTelemetryProvider` | Read available hardware, utilization, FPS/frametime, thermal, and power observations with source and availability metadata. |
| `SessionSupervisor` | Own explicitly transferred child handles, observe process exits, retain bounded lifecycle summaries, and cancel monitoring without killing processes at daemon shutdown. |
| `OptimizationPlanner` | Convert normalized snapshots and user goals into one or more proposed plans through the C17 engine. |
| `OptimizationExecutor` | Revalidate and apply only approved, supported actions; report partial failure and rollback state. |
| `BenchmarkProvider` | Run or ingest repeatable observations with configuration and provenance. |
| `ServiceManager` | P020's application-owned Linux adapter installs, enables, starts, stops, restarts, uninstalls, and inspects the optional unprivileged per-user `endfield-unixd` service. |

P019 implements the `SessionSupervisor` in `crates/sessions` as a library-only crate; the daemon owns its monitor worker. P020 keeps systemd calls and installation policy in the Linux-facing CLI adapter and adds no systemd dependency to the portable domain or telemetry crates. P007 defines `PlatformProbe` and `CapabilityProvider`; P011 implements `PlatformProbe` in the Linux adapter. P015 defines `TelemetryProvider` and portable telemetry observations; P016 implements the Linux CPU/memory adapter. Prefer capability checks over OS-name conditionals. Keep Linux sysfs, procfs, systemd, process-control, Vulkan, display-server, and driver-specific details in Linux adapters. Future macOS and FreeBSD adapters can implement the same domain-facing interfaces only for capabilities they can actually provide.

## P007 portable domain library

`crates/domain` is the `endfield-unix-domain` Rust library. It uses only the Rust standard library and forbids unsafe code. It has no dependency on the application, GUI, storage, optimizer FFI, or operating-system APIs. Application services and platform adapters may depend on it; the P011–P014 CLI commands consume its P007 snapshot through the separate Linux adapter.

## P011–P012 Linux platform adapter

`crates/platform-linux` is the library-only `endfield-unix-platform-linux` adapter. It depends on `endfield-unix-domain` and safe `nix` interfaces; it has no GUI, daemon, optimizer, or executable dependency. A narrow read-only source trait injects fixed OS metadata, native `uname`, allowlisted environment markers, and runtime-directory metadata for tests. The adapter uses the running kernel's `uname` machine value for P007 `Architecture`, retains the actual kernel release, and maps validated os-release fields into `DistributionIdentity`. Environment and runtime-directory observations live in a separately versioned `LinuxPlatformReport`, so snapshot schema 1 and the C17 ABI remain unchanged. The headless CLI calls this adapter directly; it does not query the daemon or initialize GTK.

The crate defines the P001 platform-family and capability identifiers, the four availability states, measurement units and origins, typed CPU/GPU/memory/display contracts, provenance and diagnostic records, and a schema-versioned `SystemSnapshot`. Validation rejects unknown numeric identifiers, contradictory units and availability/value combinations, unresolved instance/provenance references, and duplicate capability identities. `normalize` sorts unordered inventories and records to create deterministic in-memory snapshots. Snapshot schema version 1 is independent of package, configuration, database, and C ABI versions; no snapshot serialization or persistence is introduced.

P012 extends the same crate with an injectable `LinuxHardwareDataSource` that names fixed procfs/sysfs families. The composite `LinuxHardwareProbe` first keeps P011's validated kernel/distribution/environment result, then collects CPU topology, process affinity and scheduler queries, model text, meminfo, current-process cgroup v2 controls, cpufreq policies, and platform-profile state independently. It returns a versioned `LinuxHardwareReport` beside the normalized P007 snapshot, with provider provenance and safe bounded diagnostics. Optional hardware-source failures produce partial results instead of discarding P011 identity or unrelated valid CPU/memory facts. The adapter remains library-only and has no GTK or daemon dependency. A narrowly documented `libc` affinity query handles dynamic masks; it never calls a setter.

P007 snapshot schema 1, C17 ABI 1.0, TOML/SQLite schemas, and P010 IPC v1.0 are unchanged. Linux-specific cgroup, cpufreq policy, CPU-set and platform-profile details stay outside the portable snapshot where P007 has no suitable field. The CLI requests a one-shot report; the probe does not monitor resources or apply policies. See [HARDWARE-LINUX.md](HARDWARE-LINUX.md) for source precedence, limits, and known gaps.

## P014 Linux display and session adapter

P014 composes `LinuxDisplayProbe` over the validated P013 report and retains
P011 platform plus P012 CPU/memory facts. Its injectable source reads only
allowlisted session hints and bounded `/sys/class/drm` connector attributes;
the Wayland and X11 adapters query local protocol endpoints. The pinned native
dependencies are `wayland-client` 0.31.11, `wayland-protocols` 0.32.9, and
`x11rb` 0.13.2. They remain scoped to the Linux library and do not enter the
domain, GUI toolkit, daemon, IPC, or C17 optimizer crates.

Environment session type, local endpoint validation, live protocol
connection, output inventory, compositor-name hints, and DRM connector state
remain separate evidence. Wayland is selected for the output inventory when
both Wayland and X11 connect, avoiding a duplicate XWayland view; both
protocol statuses remain reportable. Desktop environment strings never
verify compositor identity. Current pixel mode and exact rational refresh,
advertised modes, logical geometry, scale, primary status, and DRM connectors
are retained in a Linux-specific report. P007 capability IDs 40 and 41 and its
existing `DisplayInfo` are reused without changing snapshot schema 1. No
output-to-connector mapping is inferred from matching labels. See
[DISPLAY-LINUX.md](DISPLAY-LINUX.md) for protocol behavior, bounds, privacy,
partial failures, validation status, and known provider cancellation limits.

## P021 runtime provider contracts

The crates/runtime package is the library-only endfield-unix-runtime package.
It uses Rust 2024/MSRV 1.85.0, forbids unsafe code, and has only the portable
serde_json dependency for deterministic data-only previews. It defines runtime
families and build identities, adapter constraints, bounded provider
capabilities and typed provider outcomes, source provenance and verification
states, SHA-256 metadata comparison, scoped compatibility evidence,
ownership-aware management policy, and versioned launch-plan data.

The crate does not inspect host paths, discover Wine or Proton, fetch release
metadata, hash installed files, install/update/remove a runtime, create a
prefix, launch a process, or query graphics APIs. The providers used by tests
are synthetic in-memory doubles. P022 discovery, P023 UMU integration, P024
trusted manifests, P025 acquisition, P026 installed integrity, P027 runtime
selection, P028 isolated prefix lifecycle, P029 backup/recovery, and P034–P036
launch/session lifecycle remain separate milestones. P028 is implemented by
the library-only `endfield-unix-prefix-management` crate and the headless
`endfield-unix prefix` command. It depends on P027 selection and P006 storage,
uses a mockable filesystem adapter, and adds no executable, GUI type, daemon,
process runner, or C17 ABI surface. See
[PREFIX-MANAGEMENT.md](PREFIX-MANAGEMENT.md) and
[RUNTIME-PROVIDERS.md](RUNTIME-PROVIDERS.md).

## P015 normalized telemetry contracts

`crates/telemetry` is the library-only `endfield-unix-telemetry` crate. It
depends only on `endfield-unix-domain`, uses Rust 2024/MSRV 1.85.0, and forbids
unsafe Rust. It defines stable telemetry-only metric IDs, explicit P007 and
basis-point units, signed temperature values, monotonic clock domains,
observation windows, collection/ingestion times, typed missing-data states,
freshness outcomes, safe provenance, snapshot-scoped device targets, quality,
counter/rate validation, bounded normalized batches, and a synchronous future
provider boundary. The in-memory telemetry schema v1 is independent of the P007
snapshot schema, P003 ABI, P010 IPC, TOML, SQLite, and diagnostics schema.

The crate does not query CPU/GPU/FPS/thermal sources, start timers or workers,
write storage, expose IPC, or marshal telemetry to C. Synthetic examples and
contract tests validate the data model. Linux providers are separate adapter
code; P016's implementation is described in [TELEMETRY-LINUX.md](TELEMETRY-LINUX.md).

## P016 Linux CPU and memory telemetry

P016 extends `crates/platform-linux` rather than creating a competing Linux
adapter. It depends on the portable `endfield-unix-telemetry` contracts and
reuses P012's bounded source APIs, `/proc/meminfo` parser, cpufreq policy
discovery, and cgroup v2 resolver. A single synchronous provider call performs
one bounded two-read `/proc/stat` sample plus only the requested gauge reads.
Its wait defaults to 250 ms and is limited to 100–2000 ms. CLI use is headless;
any future GUI caller must run the synchronous call from an owned worker.

P016 maps CPU utilization to P015 basis-point metrics and validated targets,
`MemAvailable` to RAM available, `MemTotal - MemAvailable` to estimated RAM
used, `SwapTotal - SwapFree` to swap used, and validated v2 `memory.current`
to a caller-scoped cgroup target. Optional current frequency comes from
`cpuinfo_cur_freq`; maximum frequency is never treated as current. Independent
source failures preserve unaffected observations, and each observation carries
P015 state, quality, provenance, and monotonic timing. The batch remains at
most 4096 observations, and no snapshot reference is invented when P012 CPU
identity cannot be validated. P016 adds no domain, telemetry, ABI, IPC,
storage, or diagnostics schema fields, and it does not start daemon sampling.
See [TELEMETRY-LINUX.md](TELEMETRY-LINUX.md) and [CLI.md](CLI.md).

`PlatformProbe` and `CapabilityProvider` are synchronous, portable traits. `CompositePlatformProbe` runs providers independently and preserves successful results alongside typed diagnostics when one provider fails. It returns a typed total error if there are no providers or none can produce a valid report. The test adapters and desktop, laptop, and incomplete fixtures are in-memory synthetic data; they never read the host machine.

## P008 diagnostics library

`crates/diagnostics` is the library-only `endfield-unix-diagnostics` crate. It
depends on the P007 domain contracts and has no GTK4, libadwaita, Relm4, Wine,
Proton, C17, or executable dependency. It maps safe domain categories and
registered identifiers into typed event records while discarding provider
messages and probe error details. A custom tracing layer accepts only static
application lifecycle names and ignores arbitrary fields.

Persistent JSON Lines writes use a bounded worker, private XDG state directory,
advisory cross-process lock, size rotation, age/count cleanup, and no-follow
file handling. The support report serializer has a separate allowlist schema;
it includes validated events and static subsystem availability without
probing hardware or reading application configuration. The logger is
initialized only for GUI startup, after static CLI parsing.

## Rust and C17 ownership

Platform adapters gather system data, map unavailable or unknown data explicitly, normalize units and identifiers, and may later call the C planner. P007 establishes the portable representation and ABI compatibility checks; it performs no collection or planner marshalling. P011 collects Linux OS, kernel, architecture, and basic environment facts; P012 adds CPU, memory, affinity, cgroup, scheduling, cpufreq, and power-interface discovery. C17 evaluates capabilities, interprets goals and intensity, evaluates constraints, generates multiple candidates, rejects unsupported or unsafe combinations, scores and ranks plans, and reports benefits and trade-offs. C17 does not read sysfs, inspect Wine, call GTK, access the network, execute commands, change system state, or decide user approval. Rust validates each returned action against current capabilities and policy before presenting or executing it.

### Normalized planner inputs and outputs

The snapshot schema must represent, when available:

- CPU model and topology, plus scheduling, frequency, and power capabilities.
- GPU vendor/model, available VRAM, graphics driver, Vulkan devices, and hardware-acceleration capabilities.
- Total and available RAM, swap, and memory pressure observations.
- Thermal and power telemetry with sensor/source, timestamp, and availability. Missing sensors remain unknown, never zero.
- OS family/version, Linux distribution/version when applicable, and kernel version.
- Display resolution/refresh characteristics and compositor/session properties.
- Installed or discoverable runtime versions and available optimization tools/capabilities.

The caller supplies optional target FPS, target resolution, and priorities/preferences for smoothness, visual quality, input latency, stability, thermal/noise, and power efficiency. Preferences use documented bounded scales; target FPS and resolution have explicit units. A missing goal is distinct from a numeric zero. The planner must state which available capabilities and goals affected each candidate, reject incompatible or unsafe combinations, and return multiple ranked plans where supported. Candidate output includes action identifiers and parameters, rank, comparable score, qualitative benefit/cost dimensions, evidence references, uncertainty, and rollback requirements. Quantified FPS/frametime estimates require attributable benchmark evidence; otherwise the estimate is unknown.

### C ABI contract

The initial implementation must publish a C-compatible header and keep unsafe Rust FFI code in a small audited module. The ABI is a process-local library boundary, not a wire or persisted-data format.

- Use fixed-width integer fields (`uint8_t`, `uint16_t`, `uint32_t`, `uint64_t`, and signed counterparts as needed), explicit byte lengths, and documented units. Do not expose C enums, C `bool`, Rust `bool`, `usize`, Rust enums, `String`, `Vec`, trait objects, or compiler-sized integers across the ABI.
- Use numeric constants for every cross-boundary identifier. Values below are reserved by this contract and must not be reused. `0` means unspecified/invalid unless explicitly defined otherwise.
- Platform-family identifiers: `1` Linux, `2` macOS, `3` FreeBSD, and `255` other/unknown. Capability-availability identifiers: `0` unknown, `1` unavailable, `2` available, and `3` degraded/partially available. Distinguish an absent optional field from an unsupported value; do not coerce either to a successful capability.
- Capability identifiers: `1` CPU topology, `2` CPU scheduling, `3` CPU frequency/power controls; `10` GPU device, `11` VRAM, `12` hardware acceleration, `13` graphics driver, `14` Vulkan device; `20` RAM, `21` swap; `30` CPU thermal, `31` GPU thermal, `32` power telemetry; `40` display, `41` compositor/session; `50` runtime discovery, and `51` optimization-tool availability. Unlisted values are reserved. Measurements and device records carry availability/provenance separately from capability identity.
- Goal identifiers: `1` target FPS, `2` target resolution, `3` smoothness, `4` visual quality, `5` input latency, `6` stability, `7` thermal/noise, and `8` power efficiency.
- Preference values for goals `3–8` are unsigned priorities from `0` through `1000`, where larger values mean higher priority. Goal `1` is milli-frames per second. Goal `2` is a width/height pair in pixels. Presence flags indicate whether a goal was set; zero is not a missing-value marker.
- Intensity values: `0` L0, `1` L1, `2` L2, `3` L3, `4` L4. Values outside this range are invalid.
- Action identifiers use a stable registry: `1` observation-only; `100–199` reversible game/runtime-scoped settings; `200–299` reversible process/session-scoped settings; `300–399` reversible user-session power/display settings; `400–499` measurement or verification actions; `900–999` experimental actions. Individual action values are assigned in the public header when a real action is implemented. Unassigned values are rejected, not guessed. Once assigned, an identifier is never recycled.
- Plan impact dimensions: `1` throughput/FPS, `2` frametime consistency, `3` visual quality, `4` input latency, `5` stability, `6` thermal/noise, and `7` power efficiency. Impact values are `0` unknown, `1` adverse, `2` no material change, `3` beneficial, and `4` mixed; they describe qualitative direction, not a measured magnitude.
- Status identifiers: `0` success, `1` invalid argument or malformed snapshot, `2` unsupported ABI major, `3` unsupported identifier or value, `4` incompatible/unsafe candidate constraints, `5` resource exhaustion, and `6` internal engine failure. A returned status is authoritative; no error crosses the ABI as a panic, exception, or process termination.
- Capability availability and evidence must distinguish at least unknown, unavailable, and available. A missing measurement is not zero. Normalize memory to bytes, frequency to hertz, dimensions to pixels, and frame rate to milli-frames per second; define units for every later field before adding it.
- Version every request structure with an ABI version and structure size. Pass arrays as pointer-plus-length pairs; encode text as length-delimited UTF-8 bytes rather than NUL-terminated strings. Inputs are borrowed for the duration of a call; the engine must not retain caller pointers. If the engine needs data after planning, it copies that data into its own plan object.
- An opaque plan handle is owned and destroyed by the C library that created it. Accessors return read-only borrowed views valid only while that handle remains alive. Rust must call the matching destroy function exactly once. Never free C memory with Rust's allocator or Rust memory with C's allocator.
- Calls are reentrant for independent request/plan handles and must not rely on mutable process-global state. The Rust wrapper validates pointer/length pairs, versions, structure sizes, counts, and numeric ranges before calling C, and contains all `unsafe` operations.

The planned header-level entry points return `uint32_t` statuses where applicable: `endfield_optimizer_abi_version()`, `endfield_optimizer_plan(request, &plan_set)`, `endfield_optimizer_plan_count(plan_set, &count)`, `endfield_optimizer_plan_get(plan_set, index, &view)`, and `endfield_optimizer_plan_set_destroy(plan_set)`. All count/index fields are `uint32_t`; every structure has `abi_version` and `struct_size`. The plan function initializes a valid output handle to null before work and returns no handle on error. A plan view is caller-provided storage containing read-only pointers into the live plan set; those pointers expire when the set is destroyed. ABI structs use the target C ABI's natural alignment and matching Rust `repr(C)` layout; do not use packed structs. The exact public declarations are finalized in the first ABI header, without changing these ownership and versioning rules.

Use a 32-bit ABI version with the major in the high 16 bits and minor in the low 16 bits. The first published ABI is `0x00010000` (major 1, minor 0). A major mismatch returns status `2`; additive, size-versioned compatible fields increment the minor. Never reinterpret an existing field or identifier. A breaking change requires a new major and explicit wrapper negotiation. This ABI-version rule is separate from runtime IPC and persisted configuration versions.

Plan rank is a one-based `uint32_t`; the comparable score is a bounded integer from `0` through `1,000,000` and is meaningful only for candidates from the same request and engine version. It is not predicted FPS. Machine-readable plans identify action IDs and parameters, constraints, rank and score, evidence references, expected qualitative benefit/cost dimensions, uncertainty, and rollback requirements. Any numeric performance estimate must cite benchmark observations and their conditions; absent evidence, leave it unknown. The C planner never executes a plan.

### P003 ABI 1.0 implementation

The published ABI header is `native/optimizer/include/endfield_optimizer.h`; its layouts, identifier constants, validation rules, ownership, and status mapping are documented in [ABI.md](ABI.md). ABI 1.0 defines versioned capability, goal, request, action-view, impact-view, and plan-view records. Measurements carry availability, an explicit unit, and a value-kind marker so unknown data and numeric zero remain distinct. Goal records use explicit presence flags. Capability provenance is length-delimited UTF-8.

`optimizer-ffi` owns the safe request model and C-owned plan-set guard. Its raw module contains every unsafe FFI declaration and call. Rust input arrays and strings remain alive through the planning call only; C retains no input pointer. The opaque plan set is allocated and destroyed by C. The Rust handle is deliberately neither `Send` nor `Sync`.

P003's C library validates metadata and identifiers, then returns an empty plan set for valid input. Candidate generation, goal interpretation, constraint analysis, ranking, and trade-off generation remain future work in P041–P048. The successful empty response does not represent a functioning optimizer or an optimization recommendation.

## Runtime, game, and persistence services

Runtime acquisition is verified, user-scoped, integrity-checked, and pin-aware. A runtime pin records provider, exact build/version, integrity identity where available, and association with the game/profile/prefix. Known-good versions are preserved. P028 manages isolated empty Wine prefix directories with explicit ownership and P027 runtime association. Wine initialization, prefix update, backup, verification, and restore remain future Rust-owned workflows with explicit user-data handling. The presence of Proton does not imply that a separate system Wine package is needed.

SQLite is suitable for structured local session/benchmark records; TOML is suitable for human-editable application configuration and profile definitions. Schema and config versions must support safe migrations. Telemetry remains local by default. Do not store passwords or platform credentials when a supported system authentication flow can avoid it.

## P009 daemon lifecycle foundation

`apps/endfield-unixd` builds the independent `endfield-unixd` Rust executable. It does not link the GTK application, initialize a display, or depend on GUI libraries. With no arguments or `--foreground`, it registers SIGTERM/SIGINT handling, starts optional P008 diagnostics, transitions through its typed lifecycle, then blocks on a condition variable until shutdown. It does not poll while idle. `--help` and `--version` return before diagnostics or long-running startup.

`ShutdownCoordinator` owns one synchronized lifecycle state and worker count. It validates Starting → Running → Stopping → Stopped; failure reasons are typed and terminal. A shutdown request during Starting goes directly to Stopping and skips Running. The first request owns cleanup; repeated requests are idempotent. Worker registration returns an explicit completion token. A dropped token records a typed worker failure. Before Stopped, the runtime closes the signal iterator and waits for the signal registration and worker token to be released. It detaches only the thread's final return path, avoiding an unbounded join. Shutdown timeouts bound coordinator waits and diagnostic queue draining; they cannot cancel a thread blocked inside synchronous filesystem I/O. The daemon reports optional logger failures with one sanitized notice and proceeds without persistent logging.

The daemon writes only approved fixed lifecycle events using `Component::Daemon` and the existing schema-version-1 diagnostics event type. Its private log worker uses P008's bounded queue, private XDG paths, advisory lock, rotation, and retention. Error strings and signal-handler details are never added to persistent events. There are no periodic idle events. Support reports mark the daemon **Partial** to mean lifecycle-only support; the serialized report schema remains version 1. The report does not claim the daemon is live.

At P009, the daemon opened no socket, started no process, collected no telemetry, and initialized no storage. P010 later added read-only IPC, P019 added owned-child monitoring without production launch, and P020 added optional per-user systemd management. GUI startup does not start the daemon, and closing the GUI does not stop an independently launched daemon or enabled user service. A real game surviving GUI closure remains unverified until P034/P035 integration.

## P010 authenticated IPC boundary

`crates/ipc` is the library-only `endfield-unix-ipc` crate. Its typed protocol
models and response validation are independent from GTK and the daemon's
lifecycle implementation. Linux transport uses a private per-user Unix-domain
socket, a lock-protected socket owner, bounded JSON framing, and kernel peer
credentials. The GTK application and CLI share the same client library; P010
does not make GUI startup depend on the daemon. The daemon owns its bounded
listener and handler workers through P009 completion tokens. P010 implements
only Ping and GetStatus. See [IPC.md](IPC.md) for the wire schema, limits,
ownership policy, shutdown behavior, and portability boundary.

## P005 typed CLI and command dispatch

The app package uses pinned Clap 4.5.53, whose crate metadata declares Rust 1.74 support, below the workspace's Rust 1.85 minimum. Parsing uses Clap's non-terminating matches API and consumes `OsString` arguments. Parser errors return to the dispatcher, which owns predictable exit-status mapping and injected stdout/stderr writers. The `cli` module separates the typed command model, parser, output formatting, and dispatch.

The implemented command model contains general and command-specific help,
version, about, GUI startup, report export, P010's `service ping` and
`service status`, and P020's explicit `service systemd` management commands.
Help documents only implemented commands. About reads its
version, purpose, license, and repository from Cargo package metadata and
static development-stage text; it does not probe hardware, runtimes, or games.
The game, runtime, optimize, benchmark, and profile namespaces remain planned.

Static CLI commands run before GTK or libadwaita initialization. Both no-argument startup and `gui` call the same existing `start_gui` boundary. The CLI model has no GTK widget types. `service ping` and `service status` use the shared typed IPC client and return distinct IPC exit statuses without blocking GUI startup. `service systemd status` separately reports manager/unit state and makes a real Ping to identify discrepancies. systemd operations use fixed trusted executables and argument lists, bounded output/timeouts, and never invoke a shell. Future frontends must call the same Rust application services rather than duplicating Wine/Proton, optimizer, benchmark, or game-management behavior.

## P006 Rust storage crate

The `endfield-unix-storage` workspace library is independent of the application executable and the `optimizer-ffi` crate. It depends on Serde/TOML for typed configuration and rusqlite for SQLite; it does not depend on GTK, libadwaita, Relm4, Wine/Proton, or the C optimizer. Application services can construct `AppPaths`, `ConfigStore`, `Database`, or the combined `Storage` facade. No global mutable connection is exposed.

Linux path resolution follows XDG configuration, data, and cache bases and namespaces them under `endfield-unix`. The v1 config stores only UI theme/language preferences. Theme defaults to `system`; language defaults to `system`. Persistence does not mean that GTK has consumed a preference: the current Settings page remains a placeholder. Static CLI commands continue to resolve help, version, and about without discovering paths or creating storage.

Config saves validate and serialize before filesystem changes, take an exclusive sidecar lock, compare the exact loaded revision, write a mode-0600 temporary file in the same directory, synchronize it, then atomically rename it into place. A stale snapshot returns a conflict. The SQLite service serializes access to its connection with a mutex, enables foreign keys, applies a bounded busy timeout, and uses `PRAGMA user_version` as the sole schema-version authority. Migration v1 creates only the `storage_metadata` table in a transaction. The full format and failure behavior are in [STORAGE.md](STORAGE.md).

Storage methods are synchronous and may block on filesystem or SQLite work. GUI callers must invoke them through a worker or another off-main-thread service boundary; P006 does not initialize persistence in the GTK startup path.

## Compatibility policy

Linux is the only initial implementation target. macOS and FreeBSD remain experimental until platform-specific functionality and game/runtime behavior have been validated on real systems. Do not advertise game compatibility, optimization availability, or equivalent telemetry merely because a platform adapter can identify the OS.

## P017 GPU telemetry

Read-only Linux GPU telemetry is implemented through PCI-correlated optional
AMDGPU sysfs/hwmon, Intel i915 current frequency, and dynamically loaded NVIDIA
NVML gauges. `endfield-unix diagnose telemetry gpu --interval-ms 250` runs
headlessly, without Vulkan/display initialization or the daemon. Unsupported
utilization windows and missing sensors remain explicit partial data. Suspended
and unknown-power devices are skipped conservatively. No live samples are
persisted or included in default support reports. See [GPU-TELEMETRY-LINUX.md](GPU-TELEMETRY-LINUX.md) for source semantics, limitations, tests, and manual physical-device checks.

## P018 frame-log ingestion

P018 extends the Linux adapter with a bounded source parser and
`FrameLogTelemetryProvider`. The provider consumes one typed recording plus a
caller-supplied P015 sampling context and import-local clock domain. The
canonical `frame-events-v1` source derives a rate only from a continuous
timestamped event span. Supported MangoHud legacy and versioned-v1 CSV schemas
preserve sampled frametime meaning and do not imply that rows cover every
frame or that the `fps` column is a recording-window rate. Batches use the
existing P015 metric IDs and validators.

The single `endfield-unix` CLI exposes import headlessly. The adapter does not
start MangoHud, a game, the daemon, Vulkan, or a display connection, and it
adds no telemetry persistence or executable. See
[FRAME-TELEMETRY-LINUX.md](FRAME-TELEMETRY-LINUX.md) for schema evidence,
statistics, privacy, and test coverage.

## P019 session supervision

`crates/sessions` defines a library-only typed session model. A random opaque
`SessionId` is distinct from both the child's PID and P015's separately
allocated `SamplingContextId`. The daemon accepts only an explicitly owned
`std::process::Child` through an internal API; no arbitrary PID attachment,
command execution, or session IPC is available. One daemon worker owns and
checks up to 64 child handles with bounded `try_wait` intervals. Completed
summaries are retained in memory up to 128 records. Owned-child `try_wait`
provides lifetime-safe reaping without relying on PID identity or pidfds.

Daemon shutdown closes the P010 listener, rejects new registrations, performs
a final nonblocking observation, marks remaining monitoring cancelled, drops
child handles without signaling processes, and reports the P009 worker token.
A running process may outlive daemon shutdown, but is no longer supervised.
No session persistence or restart adoption exists. P019 associates one stable
sampling context with a session but starts no telemetry provider. See
[SESSION-SUPERVISION.md](SESSION-SUPERVISION.md) for state, privacy, limits,
tests, and the P020/P034 boundary.

## P020 systemd user service

`endfield-unix service systemd` manages a generated user unit that executes the
installed `~/.local/bin/endfield-unixd --foreground` directly. Installation
and autostart are separate explicit actions. The unit uses `Type=notify`; the
daemon reports READY only after worker startup, listener activation, and a
successful authenticated Ping. The existing P010 runtime-directory and socket
ownership rules remain authoritative. SIGTERM follows the P009/P019 cleanup
path. The unit retains `KillMode=control-group`, so P019's no-signal behavior
does not imply child survival after a service stop. No production game launch,
logout persistence, lingering, or restart recovery is provided. See
[SYSTEMD-USER.md](SYSTEMD-USER.md) for installation, lifecycle, testing, and
manual Arch validation.

## P024 runtime manifest verification

The library-only offline verifier and headless `diagnose runtime-integrity` command
verify exact-byte Ed25519 signatures, scoped explicit anchors, version/expiry policy,
and actual local artifact size/SHA-256. User-pinned matches remain limited trust
(exit 3), without official publisher authentication. Discovery and UMU remain
unverified; no download, extraction, installation or runtime execution is added.
See [runtime trust manifests](RUNTIME-TRUST-MANIFESTS.md) for schema, CLI flags, evidence mapping,
file-race and rollback limits, synthetic tests, and P025/P026 boundaries.

## P025 runtime installation

P025 provides qualified local tar.gz installation with P024 artifact verification, a deliberate user-pinned limited-trust path, exact-plan approval, private staging, bounded extraction, no-overwrite publication and transaction recovery. Hosted Rust/GTK, GCC and Clang qualification passed in [CI run 40872573654](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40872573654). It does not establish publisher authenticity or support HTTPS acquisition. See [Linux runtime installation](RUNTIME-INSTALLATION-LINUX.md) for the workflow and limits.


P026 installed runtime auditing is documented in [RUNTIME-INTEGRITY-AUDIT-LINUX.md](RUNTIME-INTEGRITY-AUDIT-LINUX.md). It compares new-install local snapshots and leaves P025 schema-v1 installations without baselines Unverifiable. P026 supports optional P024 source-backed reconstruction with user-pinned CLI evidence; hosted qualification remains pending, and no game compatibility claim is made.

## P027 runtime selection

`crates/runtime-selection` is a portable library for strict versioned machine/profile pin records, XDG-backed atomic persistence, revision conflicts, and deterministic non-executable resolution. The headless `runtime pin` CLI uses exact P025 managed identities and P026 full audits, or explicit P022 external candidates with a canonical location and bounded structural fingerprint. List/show are metadata-only. Profile keys bind to a domain-separated machine-id digest. Stale profile pins block machine fallback. See [runtime pinning](RUNTIME-PINNING.md).

## P029 headless snapshots

The P028 prefix-management crate owns the snapshot API; its Linux adapter reuses existing dirfd, ownership, locking and CAS contracts. Planning and verification remain observational. Restore exchanges fully verified staging with live managed data, then separately commits the P028 registry, retaining the original. P027 pin restoration is an independent approved public-API CAS transaction with its own evidence. No executable, daemon protocol or C17 ABI change is added. See [backup/recovery](BACKUP-RECOVERY.md).


P029B adds a limited rollback transition to the existing prefix backup adapter. A schema-2 journal records original content evidence; a durable intent precedes exchange and a completion marker follows revalidation. The prefix registry is still the original record in this supported phase, so it is checked under the lock without a registry write. No cross-registry atomicity is claimed.

Snapshot deletion uses a separate durable schema-1 journal under `backups/deletion-journals/<backup-id>`. It records the complete verified manifest, original per-entry identities, source record, original metadata hashes, parent/root/payload identities, mount ID and registry revision. An explicit clearing marker authorizes deletion replay. Recovery validates the exact remaining subset and syncs each parent after unlink; completion is recorded outside the removed snapshot. Old partial deletions without this evidence stay blocked. The journal's checksum detects accidental corruption but is not an authenticity mechanism.
