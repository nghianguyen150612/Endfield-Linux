# Security, privacy, and recovery

P001 sets requirements; it does not claim implementation or a security audit.

## Least privilege and trust boundaries

- Run the application, daemon, game, runtime setup, and telemetry collection as the logged-in user by default. No root daemon or privileged helper is part of the baseline.
- Do not change kernel parameters, global sysctls, system services, drivers, or other privileged system state without a specific action, clear explanation, and explicit user approval. Prefer scoped, reversible per-process or per-user changes.
- Treat downloaded runtimes, metadata, game files, prefixes, IPC requests, telemetry sources, and benchmark records as untrusted input. Validate paths, sizes, versions, identifiers, and integrity before use.
- Download runtimes only from verified trusted sources with integrity metadata. Do not invent game distribution endpoints or collect store credentials. Use supported authentication mechanisms and avoid retaining credentials.
- Never patch game binaries, disable or evade anti-cheat, or interfere with game integrity checks.

## IPC and daemon

`endfield-unixd` is unprivileged by default and communicates over authenticated local IPC. On Linux, place its socket in a user-owned runtime directory with restrictive permissions, verify peer credentials, enforce a protocol version and message-size limits, and validate every operation. Do not expose a listening network socket. Apply equivalent local-user authentication on future platforms.

The daemon must tolerate malformed or stale client messages, disconnects, GUI closure, interrupted shutdown, and partial telemetry availability. Diagnostics should redact user names, home paths, process arguments, tokens, and other sensitive values unless a user explicitly chooses to include them in a report.

## Data and consent

Keep telemetry, sessions, benchmark results, and configuration local by default. Do not upload telemetry without explicit informed consent. Collect only data needed for a displayed feature. Never request unrelated credentials. Provide a user-visible way to inspect and remove application-owned records; cleanup must not remove game data or prefixes without explicit selection and confirmation.

Configuration and state updates should be versioned and recoverable. Use atomic writes where practical, validate migrations, and preserve a recoverable copy before destructive transformations. Protect files with user-only permissions when they contain sensitive local information.

## Runtime and prefix safety

Verify runtime source and integrity before activation. Keep known-good versions pinned and do not automatically upgrade them. Stage a new runner separately, validate it, and retain the previous runner until the user elects to change or remove it. Back up prefixes before risky migrations or repair operations. Explain what is included in a backup and where it is stored. Never silently delete or overwrite a prefix, runtime, or user data. Provide rollback steps for any applied optimization or runtime change.

## Optimization approval

The C17 engine is a pure planner: it receives normalized inputs and returns proposed plans. It cannot execute commands or make privileged changes. Rust must revalidate current capabilities and safety policy, show plan effects and trade-offs, and obtain approval before execution. L4 experimental actions require explicit per-plan acknowledgement. No optimization intensity permits anti-cheat bypass, binary modification, or hidden persistence.

Do not claim a recommendation will increase FPS unless relevant benchmark evidence supports that estimate. Make uncertainty and missing hardware data visible. Software settings cannot be presented as a remedy for a possible hardware power-delivery fault.
