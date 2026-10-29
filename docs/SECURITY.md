# Security, privacy, and recovery

P001 sets the baseline requirements. P006 implements the storage protections below; this is not a broader security audit.

## Least privilege and trust boundaries

- Run the application, daemon, game, runtime setup, and telemetry collection as the logged-in user by default. No root daemon or privileged helper is part of the baseline.
- Do not change kernel parameters, global sysctls, system services, drivers, or other privileged system state without a specific action, clear explanation, and explicit user approval. Prefer scoped, reversible per-process or per-user changes.
- Treat downloaded runtimes, metadata, game files, prefixes, IPC requests, telemetry sources, and benchmark records as untrusted input. Validate paths, sizes, versions, identifiers, and integrity before use.
- Download runtimes only from verified trusted sources with integrity metadata. Do not invent game distribution endpoints or collect store credentials. Use supported authentication mechanisms and avoid retaining credentials.
- Never patch game binaries, disable or evade anti-cheat, or interfere with game integrity checks.

## Daemon and future IPC

`endfield-unixd` runs as the logged-in user. It uses `signal-hook`'s self-pipe based iterator to move SIGTERM/SIGINT handling out of asynchronous signal context; callbacks do not perform filesystem access or lock lifecycle state. P019 adds one bounded worker that monitors only explicitly transferred owned `Child` handles. P020 adds optional unprivileged user-service management; it rejects root invocation and adds no privileged helper or system-level unit.

The lifecycle coordinator synchronizes one typed state, makes repeated stop requests idempotent, and tracks registered worker completion. A timeout reports that work is still active; it does not cancel the operation. In particular, synchronous filesystem calls cannot be forcibly canceled. Before the lifecycle reaches Stopped, the signal iterator releases its registration and reports worker completion; shutdown avoids an unbounded thread join. Optional log failures emit only a sanitized, one-time notice and do not stop the daemon.

The GUI neither starts nor owns the daemon process. Closing the GUI does not stop an independently launched daemon. P019's in-memory registry and process handles belong to the daemon, not a GUI client. Registration is in-process only; same-UID IPC authentication does not establish that an arbitrary process belongs to Endfield. P019 accepts no PID, shell command, executable path, process name, or IPC launch/kill request. Actual game launch ownership is deferred to P034/P035.

Normal daemon shutdown rejects registrations and stops monitoring without
signaling child processes. A still-running child may continue after the daemon
exits, but it is no longer supervised. P019 does not promise survival across
terminal, process-group, logout, cgroup, or machine shutdown behavior, and it
does not adopt processes after daemon restart.

P010 implements authenticated Linux local IPC at `$XDG_RUNTIME_DIR/endfield-unix/endfield-unixd.sock`. The runtime root and app directory are owner-checked; the app directory uses mode `0700`, and the socket and instance-lock file use `0600`. Linux `SO_PEERCRED` must match the daemon's effective UID before a request is parsed. Framing is capped at 16 KiB, each server request has a 1.5-second overall deadline, and the client uses a 2-second total deadline. The server uses four workers plus a queue bounded to twelve connections. An exclusive instance lock, same-UID socket validation, live-kernel-socket check, and inode recheck gate stale recovery. P010 has no TCP listener and only exposes read-only Ping and Status. Same-UID authentication does not isolate mutually untrusted processes under the same account; future state changes need additional authorization and user approval. IPC requests, UIDs, PIDs, socket paths, and successful pings are not persisted in diagnostics. Future platform transports require their own validation before support is claimed.

P020's generated systemd user unit executes the installed daemon directly and
uses `Type=notify`. The daemon sends fixed readiness/stopping protocol messages
to the manager-provided notification socket; it does not log the destination
or any private state. The CLI invokes only a root-owned `systemctl` or
`systemd-analyze` executable from a fixed path, with fixed subcommands, a
minimal allowlisted environment, bounded output, and deadlines. It never uses
a shell or sudo. Installation validates user-owned, non-symlink directory
chains, executable and unit types, modes, and ownership; writes are staged in
the destination directory and recorded in a private ownership manifest.
Replacement requires explicit `--replace`, a reachable manager, an inactive
unit, and unavailable authenticated IPC, and saves a hard-link backup. Removal
checks the generated unit and recorded file identity; it does not recursively
delete directories and retains a daemon that no longer matches the recorded
inode identity. Installation neither enables the unit nor starts it. The unit
retains `KillMode=control-group`; a child that P019 does not signal may still
be cleaned up by systemd on service stop. P020 therefore makes no game-process
survival claim.

## Data and consent

Keep telemetry, sessions, benchmark results, and configuration local by default. Do not upload telemetry without explicit informed consent. Collect only data needed for a displayed feature. Never request unrelated credentials. Provide a user-visible way to inspect and remove application-owned records; cleanup must not remove game data or prefixes without explicit selection and confirmation.

Configuration and state updates should be versioned and recoverable. Use atomic writes where practical, validate migrations, and preserve a recoverable copy before destructive transformations. Protect files with user-only permissions when they contain sensitive local information.

P006 creates Linux app-owned config/data directories with mode `0700` and TOML, lock, temporary, and database files with mode `0600`. It refuses final-component symlinks and non-regular config/database paths, uses same-directory atomic TOML replacement, and does not automatically delete or recreate unreadable, unsupported, or corrupt data. Its SQLite v1 migration is transactional. P006 has no database backup feature; copying an active WAL database file alone is not a consistent backup.

P008 stores only typed allowlisted event fields under
`${XDG_STATE_HOME:-$HOME/.local/state}/endfield-unix/logs/`. Its application
namespace and log directory use mode `0700`; files use mode `0600`. It refuses
final-component symlinks and hard-linked log files, coordinates library writers
with an advisory lock, and bounds event size, segment size, age, and retained
file count. Arbitrary tracing fields and P007 free-form diagnostic messages
are not persisted. The support-report schema excludes raw paths, credentials,
environment values, command arguments, device IDs, and correlation IDs. The
report is written only to an explicit destination, with mode `0600`; an existing
file requires an overwrite option and symlink destinations are refused.
See [DIAGNOSTICS.md](DIAGNOSTICS.md) for the data flow and limits. These P008
controls do not constitute a security audit of the whole application.

## P011 read-only platform discovery

`diagnose platform` runs unprivileged and makes no filesystem changes. The
Linux adapter reads only bounded `/etc/os-release`, `/usr/lib/os-release`, and
fixed container-indicator sources, plus the native `uname` interface. It does
not execute a shell or external command, scan processes or cgroups, read
arbitrary environment values, access the network, or collect hardware data.
For os-release fallback, `/etc/os-release` takes precedence and
`/usr/lib/os-release` is used only if the preferred path is missing. The only
accepted symlink target is the fixed `/usr/lib/os-release` target; file opens
reject unexpected final symlinks and non-regular files.

Parser reads are capped at 64 KiB, lines at 4 KiB, and selected values at 128
bytes after parsing. Invalid UTF-8, malformed assignments, invalid escapes,
unexpected file types, and overlong sources become bounded diagnostics. Raw
source text and paths never enter the report. Container evidence uses only
allowlisted marker paths and recognized systemd container values. WSL
classification uses recognizable kernel release markers and does not guess
WSL generation. Missing markers do not establish bare-metal execution; the
report marks that result low-confidence or unknown. Runtime-directory
assessment follows P010's absolute path, no-symlink, effective-owner, private
mode, and socket-path length policy. It never creates or repairs the directory.

The live report is printed locally and is not automatically added to P008 logs
or support reports. P008 continues to discard free-form diagnostic messages;
only fixed provider identifiers are allowlisted.

## P012 read-only hardware discovery

`diagnose hardware` reads only bounded, named kernel source families for CPU
topology, model, memory, process affinity/scheduler state, cgroup v2 controls,
cpufreq policy, and platform profile. The source interface has no arbitrary
path-taking method or write operation. CPU and policy enumeration, CPU lists,
and each source file have explicit bounds. The process affinity query uses
`sched_getaffinity`; scheduler reads use `sched_getscheduler` and
`sched_getparam`. The adapter does not call `sched_setaffinity`,
`sched_setscheduler`, `setpriority`, or any sysfs/procfs write.

The cpufreq policy and platform profile sources are fixed kernel paths. A
cpufreq policy link is canonicalized and accepted only while its target remains
under `/sys/devices/system/cpu`. The cgroup v2 directory is resolved from the
current process's cgroup and mount metadata with traversal checks; raw paths,
container IDs, model serials, and source contents are not included in the
report or diagnostics. Cgroup v1 resource limits are not interpreted as v2.
The probe does not assess writable permissions because it performs no control
write and does not need root.

Hardware facts are displayed locally only after an explicit CLI request. They
are not collected in the GUI, serialized to support reports, or added to
persistent event logs. The fixed P012 provider identifiers are allowlisted in
P008; provider IDs do not carry source contents.

## Runtime and prefix safety

Verify runtime source and integrity before activation. Keep known-good versions pinned and do not automatically upgrade them. Stage a new runner separately, validate it, and retain the previous runner until the user elects to change or remove it. Back up prefixes before risky migrations or repair operations. Explain what is included in a backup and where it is stored. Never silently delete or overwrite a prefix, runtime, or user data. Provide rollback steps for any applied optimization or runtime change.

## Optimization approval

The C17 engine is a pure planner: it receives normalized inputs and returns proposed plans. It cannot execute commands or make privileged changes. Rust must revalidate current capabilities and safety policy, show plan effects and trade-offs, and obtain approval before execution. L4 experimental actions require explicit per-plan acknowledgement. No optimization intensity permits anti-cheat bypass, binary modification, or hidden persistence.

Do not claim a recommendation will increase FPS unless relevant benchmark evidence supports that estimate. Make uncertainty and missing hardware data visible. Software settings cannot be presented as a remedy for a possible hardware power-delivery fault.
## P013 graphics discovery

`diagnose gpu` reads only fixed bounded PCI/DRM sysfs families and dynamically loads Vulkan via ash. It does not open DRM nodes or execute commands. Vulkan vendor driver code runs in-process and may block; no hard cancellation guarantee is claimed. Sysfs topology links are constrained to `/sys/devices`. See [GPU-LINUX.md](GPU-LINUX.md) for bounds and correlation semantics.

## P014 display discovery

`diagnose display` reads five bounded session-hint variables, a validated
private local Wayland socket, a local-only X11 display string, Wayland
`wl_output`/optional xdg-output observations, read-only X11 RandR replies, and
bounded `/sys/class/drm` connector attributes. It rejects remote X11 hosts,
retains normal Xauthority authentication, does not create a Wayland surface,
and sends no display-changing request. It does not read EDID, open DRM device
nodes, run display utilities, inspect process lists, read compositor
configuration, or write display state. Output names are restricted to
sanitized local labels and are not added to support reports or persistent
logs.

Wayland roundtrips, X11 queries, and caller waits are bounded, but an OS
connect or protocol-library call that blocks cannot be forcibly cancelled.
Such a worker may finish after the CLI has returned a timeout result. See
[DISPLAY-LINUX.md](DISPLAY-LINUX.md) for exact bounds and source semantics.

## P015 telemetry contract privacy boundary

P015 defines in-memory contracts only. It performs no hardware collection,
filesystem or network access, background sampling, IPC, or persistent logging.
Provider IDs use P007's restricted identifier validation. Diagnostics contain
only fixed category IDs; metric values and raw provider error details are not
automatically sent to P008 logs or support reports. Device references are
snapshot-local, process/game targets use opaque caller-assigned contexts, and
the contract contains no usernames, paths, command lines, credentials,
serials, GPU UUIDs, container IDs, or environment maps. Future providers must
keep samples local by default and follow this policy before adding any storage
or reporting path. See [TELEMETRY.md](TELEMETRY.md).

## P016 Linux telemetry collection

The P016 provider reads only bounded fixed procfs/sysfs source families and
the P012-validated current cgroup source. It performs no shell execution,
process enumeration, privilege escalation, network access, affinity change,
governor change, or kernel write. CPU counters and memory values are held only
for the one caller-requested collection; the CLI does not persist raw or
normalized samples in P008 logs, support reports, SQLite, or a telemetry
history. Raw cgroup paths and container IDs are not included in observations
or output. See [TELEMETRY-LINUX.md](TELEMETRY-LINUX.md).

## P017 GPU telemetry

Read-only Linux GPU telemetry is implemented through PCI-correlated optional
AMDGPU sysfs/hwmon, Intel i915 current frequency, and dynamically loaded NVIDIA
NVML gauges. `endfield-unix diagnose telemetry gpu --interval-ms 250` runs
headlessly, without Vulkan/display initialization or the daemon. Unsupported
utilization windows and missing sensors remain explicit partial data. Suspended
and unknown-power devices are skipped conservatively. No live samples are
persisted or included in default support reports. See [GPU-TELEMETRY-LINUX.md](GPU-TELEMETRY-LINUX.md) for source semantics, limitations, tests, and manual physical-device checks.

## P018 frame recording import

P018 consumes only a regular file explicitly selected by the user. The Linux
adapter opens the final component without following symlinks, uses nonblocking
open flags to reject FIFOs safely, checks the opened descriptor, streams at
most 64 MiB, caps each CSV record at 64 KiB and 64 fields, and caps interval
records at 4,093. It uses a CSV parser for quoting rather than splitting on
commas. Invalid UTF-8, malformed rows, unsupported schemas, nonmonotonic
timestamps, and oversized input fail with fixed diagnostics.

The importer does not scan directories, execute fields, launch a game, inspect
processes, enable MangoHud, access the network, write application state, or
persist telemetry. Paths, MangoHud system metadata, and raw CSV contents are
not emitted or sent to diagnostics/support reports. Imported timestamps remain
recording-relative in a distinct caller-assigned P015 clock domain; file
modification time is ignored. See
[FRAME-TELEMETRY-LINUX.md](FRAME-TELEMETRY-LINUX.md).

## P021 runtime contracts

The portable runtime crate models untrusted provider, source, artifact,
compatibility, path, argument, and environment metadata. It performs no
directory search, network request, artifact read/hash, prefix creation,
filesystem modification, privilege escalation, or process launch. A source
URL or version tag is not authenticity evidence, and a matching digest from
untrusted metadata does not establish trusted origin.

Provider capability, provider implementation, provider availability, runtime
presence, source verification, artifact integrity, compatibility evidence,
risk, and user approval are independent states. Unknown and unsupported
results stay explicit. Wine, Proton, UMU, managed Wine, and external Wine are
not interchangeable; no Wine fallback is selected for Proton.

LaunchPlan holds executable, native arguments, working directory, environment
changes, prefix, mounts, output policy, process ownership, and risk as
validated data. It never builds a shell command. Its bounded JSON preview
preserves argument order, redacts private paths and classified secrets, and
always reports executable_now as false. Structural validation does not grant
authorization. External or unknown runtime ownership cannot be updated or
removed through the runtime management policy.

The portable runtime library has no platform dependencies. P022/P023's separate
Linux adapter is connected only to the headless `diagnose runtimes` and
`diagnose umu` CLI commands. It
reads bounded local paths, reports sanitized labels without absolute paths,
does not execute runtime files, and keeps its candidate path map in memory for
identity inspection in the same provider session. It writes no inventory to
storage or support reports. Explicit roots are opt-in, absolute, traversal
free, bounded, and ephemeral. Familiar directory names do not verify origin,
integrity, launch readiness, or compatibility. Source acquisition, trusted
manifests, prefix management, and launch remain later milestones. See
[RUNTIME-DISCOVERY-LINUX.md](RUNTIME-DISCOVERY-LINUX.md) and
[RUNTIME-PROVIDERS.md](RUNTIME-PROVIDERS.md).

## P023 UMU boundary

`diagnose umu` checks only bounded local filesystem metadata and reuses P022
for Proton discovery. It does not invoke `umu-run`, Wine, Proton, games,
package managers, or network clients. The UMU version remains unknown. A
regular executable file is structural local evidence only and is not a
verified UMU distribution or verified usable provider.

The prepared plan uses an explicit absolute Proton directory and existing
prefix path, and it removes selected inherited UMU behavior controls. Setting
`UMU_RUNTIME_UPDATE=0` disables the documented Steam Linux Runtime automatic
update path; it does not guarantee offline or read-only execution. Executing
UMU in the future may acquire runtimes, update caches, initialize compatibility
layers, apply protonfixes, and mutate a prefix. P023 performs none of these
effects. P034 must revalidate observed files because read-only inspection does
not close TOCTOU races. See [UMU-INTEGRATION.md](UMU-INTEGRATION.md).

## P024 runtime manifest verification

The library-only offline verifier and headless `diagnose runtime-integrity` command
verify exact-byte Ed25519 signatures, scoped explicit anchors, version/expiry policy,
and actual local artifact size/SHA-256. User-pinned matches remain limited trust
(exit 3), without official publisher authentication. Discovery and UMU remain
unverified; no download, extraction, installation or runtime execution is added.
See [runtime trust manifests](RUNTIME-TRUST-MANIFESTS.md) for schema, CLI flags, evidence mapping,
file-race and rollback limits, synthetic tests, and P025/P026 boundaries.

## P025 runtime installation

P025 now provides local tar.gz installation with P024 artifact verification, a deliberate user-pinned limited-trust path, exact-plan approval, private staging, bounded extraction, no-overwrite publication and transaction recovery. It does not establish publisher authenticity or support HTTPS acquisition. Hosted qualification is pending. See [Linux runtime installation](RUNTIME-INSTALLATION-LINUX.md) for the workflow and current evidence.
