# CLI commands

`endfield-unix` is one executable with native GTK4 GUI mode and headless command mode. Static CLI commands parse and format output without initializing GTK, creating a libadwaita application, connecting to a display server, creating config/database directories, opening SQLite, starting processes, contacting the network, or launching background workers. `diagnose platform`, `diagnose hardware`, `diagnose gpu`, `diagnose display`, `diagnose runtimes`, `diagnose umu`, `diagnose telemetry cpu-memory`, and `diagnose telemetry frames` are separate read-only Linux commands; they avoid GTK, the daemon, and application storage. Runtime discovery never executes UMU, Wine, or Proton and prints no absolute runtime paths. Frame import reads one explicitly named regular file and never searches for recordings. Hardware, display, telemetry, and runtime output is printed only for an explicit command and is not automatically persisted or exported. `service ping` and `service status` make bounded local Unix-socket requests and never start the daemon. P020's explicit `service systemd` operations manage the user service through fixed `systemctl --user` commands. P028's `prefix` command plans, creates, inspects, recovers, unregisters, or explicitly deletes Endfield-Unix-owned directories without Wine execution. See [IPC.md](IPC.md), [SYSTEMD-USER.md](SYSTEMD-USER.md), [RUNTIME-DISCOVERY-LINUX.md](RUNTIME-DISCOVERY-LINUX.md), and [PREFIX-MANAGEMENT.md](PREFIX-MANAGEMENT.md).

## Available commands

| Invocation | Behavior | Needs a graphical session |
| --- | --- | --- |
| `endfield-unix` | Start the GTK4 desktop application | Yes |
| `endfield-unix gui` | Start the same GTK4 application through the same startup path | Yes |
| `endfield-unix --help`, `-h`, or `help` | Show available commands and options | No |
| `endfield-unix help gui` | Show GUI command help | No |
| `endfield-unix help about` | Show about command help | No |
| `endfield-unix --version` | Print the package version | No |
| `endfield-unix about` | Print static application and repository metadata | No |
| `endfield-unix diagnose report --output PATH` | Write a versioned, privacy-safe JSON report to the explicit path | No |
| `endfield-unix diagnose platform` | Inspect validated Linux distribution, running kernel, architecture, environment hints, and runtime-directory availability | No |
| `endfield-unix diagnose hardware` | Inspect Linux CPU topology, process affinity, memory, cgroup v2 limits, scheduling queries, and readable frequency/power interfaces | No |
| `endfield-unix diagnose gpu` | Inspect PCI graphics devices, DRM nodes, kernel drivers, and Vulkan physical devices | No |
| `endfield-unix diagnose compatibility [--profile ID] [--prefix-id ID] [--json]` | Explain selected runtime, driver, Vulkan, and one registered managed-prefix evidence | No |
| `endfield-unix diagnose display` | Inspect local Wayland/X11 sessions, output modes, and DRM connectors | No |
| `endfield-unix diagnose runtimes [--family wine|proton] [--root /absolute/path]` | Discover installed Wine/Proton files under documented roots and explicit additional directories | No |
| `endfield-unix diagnose umu` | Inspect local UMU availability and Proton candidate count without execution | No |
| `endfield-unix diagnose telemetry cpu-memory [--interval-ms MS]` | Sample CPU utilization and collect RAM, swap, cgroup memory, and optional current frequency | No |
| `endfield-unix diagnose telemetry frames --input PATH [--format frame-events-v1|mangohud-csv]` | Import one selected frame recording and report only source-supported P015 metrics | No |
| `endfield-unix prefix list` | List managed prefixes and revalidate stored runtime associations | No |
| `endfield-unix prefix show --id ID` | Inspect one managed prefix without mutation | No |
| `endfield-unix prefix plan --id ID --scope machine|profile [--profile ID] [--expected-revision REV]` | Preflight managed storage and exact P027 runtime association | No |
| `endfield-unix prefix create --id ID --scope machine|profile [--profile ID] --expected-revision REV` | Create an empty managed directory and metadata; does not initialize Wine | No |
| `endfield-unix prefix validate --id ID` | Revalidate lifecycle, ownership, and associated runtime without mutation | No |
| `endfield-unix prefix init --id ID` | Return explicit initialization_unsupported status; no process starts | No |
| `endfield-unix prefix recover --id ID --expected-revision REV` | Reconcile a matching interrupted directory-only create | No |
| `endfield-unix prefix remove --id ID --expected-revision REV` | Remove only registry ownership and preserve data | No |
| `endfield-unix prefix delete --id ID --expected-revision REV --confirm-path PATH` | Delete only the exact verified managed path after safety checks | No |
| `endfield-unix service ping` | Complete authenticated Ping/Pong over the local daemon socket | No |
| `endfield-unix service status` | Print validated lifecycle, protocol, version, and capability data | No |
| `endfield-unix service systemd install [--daemon PATH] [--replace]` | Install the foreground daemon and generated unit; autostart remains disabled | No |
| `endfield-unix service systemd uninstall` | Remove verified installer-owned files after the unit is inactive | No |
| `endfield-unix service systemd enable/disable/start/stop/restart/status` | Manage autostart/lifecycle or inspect unit state plus a separate live IPC Ping | No |

Help lists only implemented commands. Linux CPU and memory inventory is available through `diagnose hardware`; on-demand CPU and memory telemetry is available through `diagnose telemetry cpu-memory`; GPU discovery is available through `diagnose gpu`, display/session discovery through `diagnose display`, installed Wine/Proton discovery through `diagnose runtimes`, and UMU integration discovery through `diagnose umu`. Runtime output is ephemeral and labels trust and compatibility as unverified. Frame telemetry is an explicit file import with canonical timestamped events by default; select `mangohud-csv` for supported MangoHud schema variants. Optimization and game services remain under development. See [HARDWARE-LINUX.md](HARDWARE-LINUX.md), [TELEMETRY-LINUX.md](TELEMETRY-LINUX.md), [FRAME-TELEMETRY-LINUX.md](FRAME-TELEMETRY-LINUX.md), [GPU-LINUX.md](GPU-LINUX.md), [DISPLAY-LINUX.md](DISPLAY-LINUX.md), [RUNTIME-DISCOVERY-LINUX.md](RUNTIME-DISCOVERY-LINUX.md), and [SYSTEMD-USER.md](SYSTEMD-USER.md). The `about` output comes from package metadata and static project information; it does not claim detected hardware, installed runtimes, or game state.

## Companion daemon executable

The workspace also builds `endfield-unixd`, a separate foreground lifecycle
process. It does not require GTK or a display server. With no arguments it runs
in the foreground; `--foreground` is explicit. SIGTERM and SIGINT request a
graceful stop. The GUI does not start or stop it.

| Invocation | Behavior |
| --- | --- |
| `endfield-unixd --help`, `-h` | Show daemon purpose, stop signals, and current limitations |
| `endfield-unixd --version` | Print the daemon package version |
| `endfield-unixd [--foreground]` | Run the lifecycle daemon until SIGTERM or SIGINT |

Daemon exit statuses are `0` for successful signal-driven shutdown, `2` for
invalid arguments, `70` for signal, lifecycle, worker, worker-timeout, or
cleanup failure, and `74` for help/version output I/O failure. Optional
diagnostic write failures produce a sanitized warning but retain status `0` on
otherwise orderly shutdown. The daemon's P010 IPC provides only read-only
Ping and Status. P020 adds user-service-management commands to the separate
`endfield-unix` CLI; it does not add daemon service-control IPC. See
[SYSTEMD-USER.md](SYSTEMD-USER.md) for installation and lifecycle behavior.

Examples:

```sh
endfield-unix --help
endfield-unix --version
endfield-unix about
endfield-unix diagnose report --output ./support-report.json
endfield-unix diagnose report --output ./support-report.json --overwrite
endfield-unix diagnose platform
endfield-unix diagnose hardware
endfield-unix diagnose gpu
endfield-unix diagnose display
endfield-unix diagnose telemetry cpu-memory
endfield-unix diagnose telemetry cpu-memory --interval-ms 500
endfield-unix diagnose telemetry frames --input ./frames.csv
endfield-unix diagnose telemetry frames --format mangohud-csv --input ./recording.csv
endfield-unix help gui
endfield-unix prefix plan --id game --scope profile --profile default --expected-revision 0
endfield-unix prefix create --id game --scope profile --profile default --expected-revision 0
endfield-unix prefix validate --id game
endfield-unix prefix init --id game
endfield-unix prefix show --id game
endfield-unix prefix delete --id game --expected-revision REV --confirm-path /exact/path/from/plan
endfield-unix service ping
endfield-unix service status
endfield-unix service systemd install --daemon /absolute/path/to/endfield-unixd
endfield-unix service systemd enable
endfield-unix service systemd start
endfield-unix service systemd status
endfield-unix service systemd stop
endfield-unix service systemd disable
endfield-unix service systemd uninstall
endfield-unix gui
```

## Parsing and dispatch

Clap parses OS-string arguments through its non-terminating `try_get_matches_from` API. The typed command model, parser, output formatters, and dispatcher live in separate modules under `apps/endfield-unix/src/cli/`. `main.rs` continues to pass OS arguments to the library entry point. Parser failures become status 2 and are written to stderr; static command output goes to stdout.

Both default startup and the `gui` command invoke the existing `start_gui()` boundary. No CLI command constructs GTK widgets or accesses UI types. The report command calls the diagnostics library directly; it does not initialize GTK or the logger, create application state directories, run hardware probes, or contact the network. Reports contain static application/component metadata and validated allowlisted event history when available. The command writes only to the requested output file, refuses symlink destinations, uses mode `0600`, and refuses an existing file unless `--overwrite` is set. `diagnose platform`, `diagnose hardware`, `diagnose gpu`, and `diagnose display` call `endfield-unix-platform-linux` directly, require no daemon, and perform no writes or command execution. Hardware probing reads bounded fixed procfs/sysfs sources. Display probing validates local Wayland/X11 endpoints and reads bounded DRM connector sources; it distinguishes environment hints from verified protocol connections, does not guess compositor identity, and does not correlate outputs to connectors by name. See [HARDWARE-LINUX.md](HARDWARE-LINUX.md) and [DISPLAY-LINUX.md](DISPLAY-LINUX.md).

Output writers can be injected through `run_with_io` for deterministic dispatch tests. Output and diagnostic write failures are handled without panicking, including closed pipes. The service commands use the shared `endfield-unix-ipc` client, enforce a 2-second total deadline, validate the response envelope and status fields, and never initialize GTK or auto-start `endfield-unixd`.

## Exit statuses

| Status | Meaning |
| ---: | --- |
| `0` | Command succeeded |
| `2` | Invalid arguments, unknown command, unsupported argument combination, or extra argument |
| `69` | GUI startup failed, including unavailable graphical session |
| `74` | Selected input or command output I/O failed |
| `75` | Local daemon unavailable, connection/request timeout, shutdown, or saturation |
| `76` | Protocol mismatch or invalid daemon response |
| `77` | Socket permission or peer authentication denied |
| `65` | Daemon rejected an unsupported operation |
| `70` | Malformed frame source, fatal provider, required platform identity, or hardware-probe failure |
| `78` | Platform or explicitly selected frame format is unsupported |
| `79` | systemd user-service command or installation failed |
| `3` (prefix) | Prefix is missing or unregistered |
| `4` (prefix) | Prefix operation is stale, conflicting, unsafe, or unverifiable |
| `5` (prefix) | Wine initialization is unsupported by validated adapters |

Status `74` also covers report destination and file-write failures. Invalid
syntax or an unsupported report option remains status `2`. IPC failures use
the statuses above rather than the GUI startup status `69`. The platform command
returns `0` for complete or partial information, `70` if running-kernel identity
cannot be established, and `78` when the adapter is unsupported. Its
output-write failures use `74`; invalid arguments remain `2`. The hardware
command returns `0` for complete or partial hardware facts, `70` when required
Linux identity cannot be established, and `78` on an unsupported platform;
optional subsystem failures are reported as partial results. Output and
argument failures remain `74` and `2`.

P028 prefix commands are headless and perform no implicit mutation from
`list`, `show`, `plan`, or `validate`. `create` requires a valid P027 pin and a
current prefix registry revision. `delete` also requires the exact destination
shown by `plan` or `show`; `remove` unregisters while preserving all data.
`init` currently exits 5 with an explicit blocked result. Prefix-specific
statuses 3–5 may have separate meanings for other command families. See
[PREFIX-MANAGEMENT.md](PREFIX-MANAGEMENT.md) for lifecycle and deletion details.

## Planned command namespaces

The following namespaces are reserved for later work. The `diagnose`
namespace implements safe report export, Linux platform/hardware/GPU/display
inspection, and one-shot CPU/memory telemetry. `service ping` and
`service status` check authenticated IPC; `service systemd` explicitly manages
the optional Linux user service.

```text
endfield-unix game ...
endfield-unix runtime ...
endfield-unix optimize ...
endfield-unix benchmark ...
endfield-unix profile ...
```

Each future command must call the corresponding shared application service. Adding a command name before its service exists must not imply that the operation works.
## GPU diagnosis

`endfield-unix diagnose gpu` performs headless local PCI, DRM, kernel-driver, and Vulkan discovery without GTK, daemon, or storage access. It returns status 0 for complete or partial reports, 70 for a required probe failure, 74 for output failure, 78 for unsupported platform, and 2 for invalid arguments. See [GPU-LINUX.md](GPU-LINUX.md) for interpretation and limitations.

## Display diagnosis

`endfield-unix diagnose display` performs one-shot Wayland, X11/RandR, and
DRM connector discovery without GTK, daemon, or storage access. It returns
status 0 for complete, partial, or headless reports; 70 for required Linux
identity failure; 74 for output failure; 78 for an unsupported platform; and
2 for invalid arguments. Stale session variables and inaccessible display
servers remain partial provider observations. Exact fractional refresh stays
in the Linux-specific report rather than being rounded into P007 integer Hz.
See [DISPLAY-LINUX.md](DISPLAY-LINUX.md) for interpretation and limitations.

## CPU and memory telemetry

`endfield-unix diagnose telemetry cpu-memory` performs one bounded Linux
collection without GTK, the daemon, configuration, SQLite, or network access.
The default CPU counter interval is 250 ms; `--interval-ms` accepts 100–2000
ms. The command obtains a P012 CPU snapshot for identity mapping when
available, then asks the P016 provider for one pair of `/proc/stat` readings
and the requested memory, cgroup, and optional current-frequency observations.
It prints actual elapsed time, and preserves `partial` status when a source or
identity is unavailable. CPU and memory values are not automatically logged
or added to support reports. Because the provider waits synchronously for the
bounded interval, a future GUI caller must invoke it from an owned worker.

The command returns 0 for complete or partial valid data, 2 for invalid
arguments, 70 for fatal provider failure, 74 for output failure, and 78 for an
unsupported platform. See [TELEMETRY-LINUX.md](TELEMETRY-LINUX.md) for source
limits, metric semantics, and failure states.

## P017 GPU telemetry

Read-only Linux GPU telemetry is implemented through PCI-correlated optional
AMDGPU sysfs/hwmon, Intel i915 current frequency, and dynamically loaded NVIDIA
NVML gauges. `endfield-unix diagnose telemetry gpu --interval-ms 250` runs
headlessly, without Vulkan/display initialization or the daemon. Unsupported
utilization windows and missing sensors remain explicit partial data. Suspended
and unknown-power devices are skipped conservatively. No live samples are
persisted or included in default support reports. See [GPU-TELEMETRY-LINUX.md](GPU-TELEMETRY-LINUX.md) for source semantics, limitations, tests, and manual physical-device checks.

## P018 frame telemetry import

`diagnose telemetry frames` defaults to the versioned `frame-events-v1`
schema. `--format mangohud-csv` explicitly selects the supported MangoHud
legacy or versioned-v1 CSV structures. The command opens only the supplied
regular file, does not print its path or raw headers, and requires no GTK,
daemon, game, or storage. Valid partial results exit `0`; invalid CLI arguments
exit `2`; malformed data/provider failures exit `70`; inaccessible input and
output failures exit `74`; unsupported source variants exit `78`.

MangoHud rows preserve their latest frame-interval and instantaneous reciprocal
FPS semantics. P018 does not infer `log_interval`, treat every row as a frame,
or derive a recording-wide FPS window from the sampled FPS column. See
[FRAME-TELEMETRY-LINUX.md](FRAME-TELEMETRY-LINUX.md) for details.

## P024 runtime manifest verification

The library-only offline verifier and headless `diagnose runtime-integrity` command
verify exact-byte Ed25519 signatures, scoped explicit anchors, version/expiry policy,
and actual local artifact size/SHA-256. User-pinned matches remain limited trust
(exit 3), without official publisher authentication. Discovery and UMU remain
unverified; no download, extraction, installation or runtime execution is added.
See [runtime trust manifests](RUNTIME-TRUST-MANIFESTS.md) for schema, CLI flags, evidence mapping,
file-race and rollback limits, synthetic tests, and P025/P026 boundaries.

## P025 runtime installation

P025 provides qualified local tar.gz installation with P024 artifact verification, a deliberate user-pinned limited-trust path, exact-plan approval, private staging, bounded extraction, no-overwrite publication and transaction recovery. Hosted Rust/GTK, GCC and Clang qualification passed in [CI run 40872573654](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40872573654). It does not establish publisher authenticity or support HTTPS acquisition. See [Linux runtime installation](RUNTIME-INSTALLATION-LINUX.md) for commands and limits.


P026 installed runtime auditing is documented in [RUNTIME-INTEGRITY-AUDIT-LINUX.md](RUNTIME-INTEGRITY-AUDIT-LINUX.md). It compares new-install local snapshots and leaves P025 schema-v1 installations without baselines Unverifiable. P026 supports optional P024 source-backed reconstruction with user-pinned CLI evidence; hosted qualification remains pending, and no game compatibility claim is made.

## P027 runtime pin commands

`runtime pin list`, `show`, `set`, `set-external`, `resolve`, and `remove` manage explicit managed-runtime and P022-discovered external preferences without GTK or daemon access. Mutations require the current registry revision; replacement additionally requires `--replace`. Profile scopes require `--profile`. External selection uses a candidate ID printed by `diagnose runtimes`; a custom candidate parent must be supplied again with `--root`. The candidate ID is a temporary selection handle. The registry stores the exact canonical observed location and bounded structural identity, not the candidate ID. Managed `resolve` revalidates the P025 record and P026 baseline; external `resolve` revalidates the exact P022 candidate structure and does not claim file integrity. Neither path starts Wine, Proton, UMU, or a game. See [runtime pinning](RUNTIME-PINNING.md) for examples and status semantics.

## P029 backup/recovery commands

The existing prefix family gains nested backup, restore and recovery commands, documented with exact approval strings in [backup/recovery](BACKUP-RECOVERY.md). backup create and recovery run require --confirm; restore apply requires restore:ID:BACKUP; recovery pin requires pin:BACKUP and an expected P027 revision. backup deletion requires delete:BACKUP. P029 refusals return 4. These operations do not initialize GTK, start the daemon or run runtimes.

## P029A canonical parser repair

The executable now parses the same typed prefix operation tree used by dispatch. `prefix recover` remains the P028 creation-reconciliation command; `prefix recovery <status|run|release|pin>` handles P029 snapshots. Top-level, topic and nested help expose the new groups. `prefix backup deletions` reports incomplete retirement evidence without unlinking anything. Restore-plan JSON now includes current P027 selection and saved-reference assessment; it never mutates pins. See [backup/recovery](BACKUP-RECOVERY.md) for approval strings and exit codes.


P029B adds `prefix recovery rollback-plan --id ID` and `prefix recovery rollback --id ID --expected-revision REV --confirm TOKEN`. Use the exact token printed by the plan. Only published, uncommitted schema-2 restores qualify; unsupported states return a blocked result. `recovery release` archives completed rollback evidence and displaced data.

P029B also adds read-only `prefix backup deletion-plan --backup-id ID` and `prefix backup deletion-journals`. `prefix backup deletion-resume --backup-id ID --expected-revision REV --confirm TOKEN` resumes a verified interrupted deletion; use the exact token from the plan. `prefix recovery history --id ID` lists archived restore/rollback journals, and `prefix recovery pins` lists pin-journal markers. These history reports state their evidence limits and do not claim current payload or pin integrity unless separately verified. Legacy partial deletion remains blocked.
## P030 compatibility diagnostics

`endfield-unix diagnose compatibility [--profile PROFILE_ID] [--prefix-id PREFIX_ID] [--json]` emits a bounded read-only assessment using the actual CLI parser and dispatch. It runs headless without the daemon. `--profile` follows P027 machine/profile precedence; a stale profile pin blocks fallback. `--prefix-id` inspects exactly one registered managed prefix. `--json` selects schema v1; successful partial reports exit 0, invalid arguments exit 2, output errors exit 74, and serialization/limit failures exit 70. Static findings are not game-compatibility results. See [COMPATIBILITY-DIAGNOSTICS.md](COMPATIBILITY-DIAGNOSTICS.md) for the report and privacy contract.
