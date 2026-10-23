# Architecture baseline

## Status and dependency direction

P001 established these intended boundaries. P002 established the initial Rust workspace and shared executable; P003 added the C17 ABI skeleton; P004 adds the first native GTK shell. Platform adapters, candidate generation, daemon, runtime, and game services remain unimplemented. The design is Linux-first while keeping portable domain logic independent from operating-system APIs. Dependency direction is:

```text
GTK4 desktop / CLI ──> Rust application services ──> portable domain contracts
                                  │                             │
                                  ├──> platform adapters        └──> C17 planner
                                  ├──> storage and IPC
                                  └──> session daemon client
```

The GTK frontend and CLI use one Rust application core and one GUI/CLI executable named `endfield-unix`. The only companion executable is `endfield-unixd`. There is no separate GUI binary or CLI binary. GTK widgets and GTK-facing state remain on the GUI main thread. Filesystem, process, network, telemetry, benchmark, and other potentially blocking work runs in workers; workers send results back to the GUI through Relm4 messages. The synchronous C planner is bounded, deterministic planning work and must not perform I/O.

The desktop stack is fixed: Rust 2024, GTK4, libadwaita, Relm4, GTK CSS, and SVG resources. C optimizer code targets C17. Rust owns the application lifecycle, CLI argument handling, UI integration, game and runtime management, platform adapters, telemetry ingestion, daemon lifecycle and IPC, SQLite/TOML persistence, safe FFI wrappers, plan validation/execution, and recovery.

## P002 workspace and entry point

The Cargo workspace uses resolver version 3 and contains one application package at `apps/endfield-unix`. The package is Rust 2024 with MSRV 1.85 and produces exactly one application binary, `endfield-unix`. Its library contains argument dispatch and startup routing so those behaviors can be tested without a GUI. P004 adds Relm4 0.9.1, GTK4-rs 0.9.6, and libadwaita-rs 0.7.2; these bindings use the matching GTK4 0.9 / GLib 0.20 generation.

`main` obtains arguments as OS strings, skips the executable name, and delegates process work to the shared library. Dispatch recognizes only `--help`, `--version`, and no arguments. Help, version, and argument errors are resolved before GTK initialization. No arguments call `start_gui`; P004 initializes GTK/libadwaita on the main thread and starts the Relm4 application. Display initialization errors return status 69 with a diagnostic rather than panicking.

P002 performs no hardware, filesystem, process, network, Wine, Proton, or game probing. Invalid input and output failures return explicit nonzero statuses and diagnostics.

P003 adds a workspace library package, `optimizer-ffi`, for the C17 build integration and safe FFI wrapper without changing the executable entry point. The workspace still has one application binary, `endfield-unix`; the FFI package is a library and adds no executable. P004 implements the GTK shell in that application package. There is no second GUI/CLI executable.

## P004 native application shell

The stable GTK application ID is `io.github.nghianguyen150612.endfieldunix`; GLib validates it before creating the application. `ui::app::App` is the top-level Relm4 component. Its model owns the active page, compact-navigation state, and window lifecycle. `AppMessage` carries navigation, responsive-layout, back-navigation, and close requests. GTK widgets are created and mutated only on the GUI main thread.

`ui::navigation` defines the six page identities and builds an `AdwNavigationSplitView` with a `GtkStackSidebar` and content stack. libadwaita collapses the split view at narrower widths; the header exposes a back affordance to return to the sidebar. `ui::pages` owns each page's view subtree. Page builders display shell status and send only navigation requests; they do not access services, system probes, or the C planner. Home keeps Play disabled until a launch service exists. Optimize identifies the C17 empty plan set and does not invoke an optimization workflow.

`ui::resources` embeds GTK CSS and the original SVG mark into the executable. CSS is installed only after graphics initialization. SVG decoding has a symbolic GTK icon fallback. CLI help and version do not initialize GTK or load GUI resources. No GResource compiler or second binary is introduced.

The application window starts at 1100 × 720 logical pixels and remains resizable. AdwNavigationSplitView and scrollable page content adapt to narrower windows. GTK's platform typography, system font scaling, keyboard traversal, accessible widget roles, and native focus indication are retained. The stack uses no transition animation. The interface contains no network-loaded resource or continuously animated content.

The page model is presentation state, not a service layer. Future platform probing, runtime, library, benchmark, planner, and diagnostics work must arrive through service boundaries and Relm4 messages/workers; those integrations are not implemented in P004. No GTK widget or display-server interface enters the C17 planner or optimizer FFI. The selected `v1_4` libadwaita bindings require system libadwaita 1.4 or newer. macOS and BSD behavior remains unverified.

Define interfaces when implementation reaches the relevant feature; keep OS-specific behavior behind adapters. The following responsibilities anchor that design:

| Boundary | Responsibility |
| --- | --- |
| `PlatformProbe` | Discover platform identity and, more importantly, available capabilities, versions, devices, and diagnostics. |
| `RuntimeProvider` | Detect, verify, install, select, pin, and remove user-managed Wine/Proton-compatible runners under the runtime policy. |
| `HardwareTelemetryProvider` | Read available hardware, utilization, FPS/frametime, thermal, and power observations with source and availability metadata. |
| `SessionSupervisor` | Start, observe, and end game sessions; preserve a running game if the GUI closes. |
| `OptimizationPlanner` | Convert normalized snapshots and user goals into one or more proposed plans through the C17 engine. |
| `OptimizationExecutor` | Revalidate and apply only approved, supported actions; report partial failure and rollback state. |
| `BenchmarkProvider` | Run or ingest repeatable observations with configuration and provenance. |
| `ServiceManager` | Install, start, stop, and inspect the unprivileged per-user `endfield-unixd` service where supported. |

These are responsibilities, not pre-existing Rust trait signatures. Prefer capability checks over OS-name conditionals. Keep Linux sysfs, procfs, systemd, process-control, Vulkan, display-server, and driver-specific details in Linux adapters. Future macOS and FreeBSD adapters implement the same domain-facing responsibilities only for capabilities they can actually provide.

## Rust and C17 ownership

Rust gathers system data, maps unavailable or unknown data explicitly, normalizes units and identifiers, and calls the C planner. C17 evaluates capabilities, interprets goals and intensity, evaluates constraints, generates multiple candidates, rejects unsupported or unsafe combinations, scores and ranks plans, and reports benefits and trade-offs. C17 does not read sysfs, inspect Wine, call GTK, access the network, execute commands, change system state, or decide user approval. Rust validates each returned action against current capabilities and policy before presenting or executing it.

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

Runtime acquisition is verified, user-scoped, integrity-checked, and pin-aware. A runtime pin records provider, exact build/version, integrity identity where available, and association with the game/profile/prefix. Known-good versions are preserved. Wine prefix creation, update, backup, verification, and recovery are Rust-owned workflows with atomic metadata updates and explicit handling of user data. The presence of Proton does not imply that a separate system Wine package is needed.

SQLite is suitable for structured local session/benchmark records; TOML is suitable for human-editable application configuration and profile definitions. Schema and config versions must support safe migrations. Telemetry remains local by default. Do not store passwords or platform credentials when a supported system authentication flow can avoid it.

## Daemon and IPC

`endfield-unixd` is the sole companion executable. It tracks game sessions, gathers available hardware/FPS/frametime/thermal/power data, records benchmark observations, and provides diagnostic reports to GUI and CLI clients. It is lightweight, unprivileged by default, and session-scoped where practical. Closing the GUI does not terminate a game session. Shutdown is graceful; the daemon may stop when no client or supervised session requires it.

Use authenticated local IPC with a versioned, bounded message protocol. On Linux, the initial transport should be a per-user Unix-domain socket in a user-owned runtime directory, with filesystem permissions and peer-credential checks tied to the current user. Do not listen on a network interface. Validate message size, version, operation, and caller identity. Keep future transport choices behind the IPC adapter so other platforms can use their native local transport while preserving authorization and protocol semantics. Service installation and control use the user service manager; root is not required by default.

## Compatibility policy

Linux is the only initial implementation target. macOS and FreeBSD remain experimental until platform-specific functionality and game/runtime behavior have been validated on real systems. Do not advertise game compatibility, optimization availability, or equivalent telemetry merely because a platform adapter can identify the OS.
