# `endfield-unixd` lifecycle daemon

P009 adds the second application executable, `endfield-unixd`. It is a
foreground Linux service process that establishes the lifecycle, shutdown, and
diagnostics boundaries for later daemon work. The workspace still has only two
application binaries: `endfield-unix` (GTK GUI and CLI) and `endfield-unixd`.

## Run it

```sh
cargo +1.85.0 run --locked -p endfield-unixd -- --help
cargo +1.85.0 run --locked -p endfield-unixd -- --version
cargo +1.85.0 run --locked -p endfield-unixd -- --foreground
```

No arguments are equivalent to `--foreground`. The process remains attached
to its parent or terminal; it does not fork, detach, or install a service.
SIGTERM and SIGINT request graceful shutdown. Help and version return before
signal registration, diagnostics initialization, or lifecycle startup. The
daemon needs no GTK library, display server, GPU, game, Wine, or Proton.

Exit statuses are `0` for a successful shutdown (including SIGTERM/SIGINT),
`2` for invalid arguments, `70` for signal, lifecycle, worker, worker-timeout,
or resource-cleanup failure, and `74` when help/version output cannot be
written.
Errors use fixed safe text; OS error details and filesystem paths are not
printed.

## Lifecycle and ownership

`ShutdownCoordinator` owns the synchronized state and the registered worker
count:

```text
Starting -> Running -> Stopping -> Stopped
    |          |           |
    +----------+-----------+----> Failed(typed reason)
```

Every transition is checked. Stopped and Failed are terminal. A stop during
Starting moves directly to Stopping and skips Running. The first stop request
sets the shutdown reason and owns cleanup; repeated requests return an
idempotent result. Concurrent requests are serialized by one mutex. No GTK
main loop or global mutable state is involved.

The runtime owns the signal listener and optional P008 `LogWorker`. The signal
listener is an explicit registered worker. Its iterator releases the signal
registration and sends completion before the coordinator enters Stopped; the
runtime detaches only its final thread return instead of joining indefinitely.
Other future workers receive a completion token:
they must report completion or a typed failure, and dropping an unfinished
token marks the lifecycle failed. The runtime waits for registered workers
using the configured shutdown policy.

## Startup, signals, and shutdown

Startup parses and validates arguments before any runtime setup. The daemon
pins `signal-hook` 0.3.18; Cargo resolves `signal-hook-registry` 1.4.8 and
`libc` 0.2.191, alongside `thiserror` 2.0.11. These dependencies compile with
the repository's Rust 1.85.0 toolchain. Foreground startup registers SIGTERM
and SIGINT through the signal iterator, which uses a self-pipe so raw
asynchronous handlers do not perform filesystem work, allocate, or lock the
lifecycle. It then starts optional diagnostics and
records `DAEMON_STARTING`. Only after signal setup succeeds does the lifecycle
enter Running and record `DAEMON_RUNNING`. A signal received during startup
produces a clean Starting → Stopping path without reporting Running.

While idle, the main thread waits on a condition variable and the signal
listener blocks on its signal pipe. There is no polling loop, periodic
filesystem read, child process, hardware query, network activity, or heartbeat
event.

The first SIGTERM or SIGINT changes Starting or Running to Stopping. The
runtime records one `DAEMON_SHUTDOWN_REQUESTED`, stops accepting future work,
queues `DAEMON_STOPPED`, drains and stops optional diagnostics, closes the
signal listener, waits for its completion notification and registered workers,
then transitions to Stopped.
Repeated signals during cleanup do not repeat cleanup. Successful handling
returns status 0.

`ShutdownPolicy` provides configurable diagnostic-drain and worker-wait
durations. A worker-wait timeout reports that work remains active; it does not
cancel that operation or assert a hard deadline. P008's `shutdown_bounded`
waits for queued log writes and an acknowledgement without an unbounded thread
join. If filesystem I/O itself blocks past the timeout, the daemon emits one
sanitized warning and exits without persistent diagnostics; that optional
worker is not forcibly cancellable. The signal listener's only blocking wait
is its signal pipe; closing the pipe wakes it, and its completion notification
confirms the iterator and worker token have been released.

## Diagnostics and failure behavior

The daemon reuses `endfield-unix-diagnostics`, `LogWorker`, and `LogHandle`. It
uses the existing private XDG state path, bounded queue, JSON Lines schema,
advisory cross-process lock, rotation, and retention. Fixed schema-version-1
events use `Component::Daemon`: `DAEMON_STARTING`, `DAEMON_RUNNING`,
`DAEMON_SHUTDOWN_REQUESTED`, `DAEMON_STOPPED`, `DAEMON_STARTUP_FAILED`, and
`DAEMON_RUNTIME_FAILED`. Each event has a compiled-in message. Signal values,
OS error strings, paths, and arbitrary details are not persisted. Logging
failure is optional, reported once with sanitized text, and never recursively
logged.

Support reports now mark the daemon `partial`, meaning lifecycle-only daemon
support exists while IPC, telemetry, and session tracking remain unavailable.
The report keeps schema version 1 and does not claim that a daemon process is
currently running; no live status check exists.

Signal registration, worker startup, invalid lifecycle transitions, worker
failure, shutdown timeout, and resource cleanup have typed failure paths. A
startup failure is reported without exposing source strings. A failure is
terminal and cannot accidentally restart the daemon.

## Boundaries and P010 readiness

P009 does not start game processes, collect hardware telemetry, initialize
networking, open a Unix socket, implement IPC, initialize SQLite, supervise
game sessions, or install/enable systemd. It does not create daemon settings,
database tables, or migrations. The GTK application neither starts nor owns
the daemon; closing the GUI does not stop an independently supervised daemon.
This process independence alone does not guarantee that a game survives GUI
closure; that behavior belongs to P019 session supervision.

P010 can attach an authenticated local IPC service to the foreground runtime's
startup and shutdown ownership. P010 still needs a versioned local protocol,
peer-credential validation, permissions, message-size limits, and bounded
connection handling; none is implied by P009.

## Validation

P009 adds deterministic lifecycle and CLI tests plus Linux subprocess tests
that start the real executable in isolated XDG directories, observe its
`DAEMON_RUNNING` event, send SIGTERM/SIGINT, verify status 0, check duplicate
cleanup events, and reap the child on every test path. The idle test compares
process CPU ticks during a quiet interval. Timeout tests show that a timed-out
worker remains active until it explicitly completes; they do not claim
cancellation.

The qualified commands and hosted GTK, C17, and CI results are recorded in
[TESTING.md](TESTING.md).
