# Security, privacy, and recovery

P001 sets the baseline requirements. P006 implements the storage protections below; this is not a broader security audit.

## Least privilege and trust boundaries

- Run the application, daemon, game, runtime setup, and telemetry collection as the logged-in user by default. No root daemon or privileged helper is part of the baseline.
- Do not change kernel parameters, global sysctls, system services, drivers, or other privileged system state without a specific action, clear explanation, and explicit user approval. Prefer scoped, reversible per-process or per-user changes.
- Treat downloaded runtimes, metadata, game files, prefixes, IPC requests, telemetry sources, and benchmark records as untrusted input. Validate paths, sizes, versions, identifiers, and integrity before use.
- Download runtimes only from verified trusted sources with integrity metadata. Do not invent game distribution endpoints or collect store credentials. Use supported authentication mechanisms and avoid retaining credentials.
- Never patch game binaries, disable or evade anti-cheat, or interfere with game integrity checks.

## Daemon and future IPC

P009's `endfield-unixd` runs as the logged-in user. It opens no socket, starts no process, reads no game data, and collects no telemetry. It uses `signal-hook`'s self-pipe based iterator to move SIGTERM/SIGINT handling out of asynchronous signal context; callbacks do not perform filesystem access or lock lifecycle state. The daemon has no root helper and installs no systemd unit.

The lifecycle coordinator synchronizes one typed state, makes repeated stop requests idempotent, and tracks registered worker completion. A timeout reports that work is still active; it does not cancel the operation. In particular, synchronous filesystem calls cannot be forcibly canceled. Before the lifecycle reaches Stopped, the signal iterator releases its registration and reports worker completion; shutdown avoids an unbounded thread join. Optional log failures emit only a sanitized, one-time notice and do not stop the daemon.

The GUI neither starts nor owns the daemon process. Closing the GUI does not stop an independently launched daemon. P009 does not supervise game sessions, so this process independence alone does not promise a running game will survive GUI closure.

P010 implements authenticated Linux local IPC at `$XDG_RUNTIME_DIR/endfield-unix/endfield-unixd.sock`. The runtime root and app directory are owner-checked; the app directory uses mode `0700`, and the socket and instance-lock file use `0600`. Linux `SO_PEERCRED` must match the daemon's effective UID before a request is parsed. Framing is capped at 16 KiB, each server request has a 1.5-second overall deadline, and the client uses a 2-second total deadline. The server uses four workers plus a queue bounded to twelve connections. An exclusive instance lock, same-UID socket validation, live-kernel-socket check, and inode recheck gate stale recovery. P010 has no TCP listener and only exposes read-only Ping and Status. Same-UID authentication does not isolate mutually untrusted processes under the same account; future state changes need additional authorization and user approval. IPC requests, UIDs, PIDs, socket paths, and successful pings are not persisted in diagnostics. Future platform transports require their own validation before support is claimed.

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
only the new fixed provider identifiers are allowlisted.

## Runtime and prefix safety

Verify runtime source and integrity before activation. Keep known-good versions pinned and do not automatically upgrade them. Stage a new runner separately, validate it, and retain the previous runner until the user elects to change or remove it. Back up prefixes before risky migrations or repair operations. Explain what is included in a backup and where it is stored. Never silently delete or overwrite a prefix, runtime, or user data. Provide rollback steps for any applied optimization or runtime change.

## Optimization approval

The C17 engine is a pure planner: it receives normalized inputs and returns proposed plans. It cannot execute commands or make privileged changes. Rust must revalidate current capabilities and safety policy, show plan effects and trade-offs, and obtain approval before execution. L4 experimental actions require explicit per-plan acknowledgement. No optimization intensity permits anti-cheat bypass, binary modification, or hidden persistence.

Do not claim a recommendation will increase FPS unless relevant benchmark evidence supports that estimate. Make uncertainty and missing hardware data visible. Software settings cannot be presented as a remedy for a possible hardware power-delivery fault.
