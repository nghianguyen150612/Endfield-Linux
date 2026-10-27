# P019 daemon-owned session supervision

P019 adds the library-only `endfield-unix-sessions` crate and one required
monitor worker to `endfield-unixd`. It establishes trusted process ownership
and session lifecycle mechanics; it does not start a game or provide a public
launch/stop API. There are still exactly two application binaries.

## Identity and registry

Each registration receives a 128-bit random `SessionId` from the operating
system random source (`getrandom` 0.2.17), encoded as 32 lowercase hexadecimal
characters. Construction validates that shape. IDs are opaque labels, not
authorization tokens. The registry checks every generated ID against active
and retained records; collisions are regenerated. A PID is recorded only as
the current process observation and is cleared when the owned child is reaped.

`SessionSupervisor` owns a synchronized in-memory registry. It accepts at most
64 active children and retains at most 128 completed summaries. The combined
history is therefore at most 192 records. Active sessions are never evicted;
the oldest completed summary is retired when the completed bound is exceeded.
Registration is rejected while shutdown is underway. Lists are bounded by
these limits and contain safe typed summaries only.

## Lifecycle and process ownership

The enforced state transitions are:

```text
Created -> Starting -> Running -> Exited
                    |          -> Failed
                    |          -> Stopping -> Stopped
                    |                       -> Failed
                    +-> Exited / Failed
```

The last line covers a process that exits or cannot be monitored during
startup. Invalid and duplicate terminal transitions are rejected. Session
state is separate from daemon lifecycle state: daemon `Stopping` does not
change a running session to `Stopped`.

The registration boundary consumes an owned `std::process::Child`; no API
accepts a PID, process name, path, or client assertion. Only trusted in-process
launcher code may call it after a future authorized launch operation. P019
does not provide process creation, shell execution, Wine/Proton invocation,
arbitrary executable paths, or IPC registration. The GUI and same-UID IPC
authentication do not establish process ownership.

One monitor worker owns all child handles. It blocks for commands with no
children and checks owned children with `Child::try_wait` every 100 ms while
any are active. It reaps each exited child through the owned child relationship
and never uses a stale PID to reattach. This protects against PID reuse without
pidfds: `try_wait` observes the specific child the daemon owns. Exit code 0,
other exit codes, Linux signal termination, and unavailable status are distinct
outcomes. A `try_wait` error becomes a typed monitor failure; it is not
reported as an observed process exit. Exit time is recorded only as the
monotonic time at which the supervisor observed the exit, not as the process's
unknowable exact exit instant. Arguments, paths, and raw process errors are
not kept in session summaries.

The production monitor adapter only delegates to `Child::try_wait`. A private
injectable monitor seam lets deterministic unit tests exercise a monitor error
and a blocked observation without exposing synthetic or PID-based production
registration.

Monitor status is `Active` while the child is monitored, `Complete` after a
confirmed/reaped exit, `Failed` when ownership or observation fails, and
`Cancelled` when daemon shutdown ends monitoring while the process remains
unconfirmed.

The worker and registry are bounded: one worker, one 65-slot command queue (64
registrations plus a shutdown slot), 64 active child records, and 128 completed
summaries. The worker owns its child map; the registry mutex protects only
short state updates and is never held during a child wait. No process-group
signal, PID signal, `/proc` sweep, or busy-spin is used.

## Shutdown and lifetime

Daemon shutdown first rejects new registrations, then closes the existing IPC
service to new work, and notifies the supervisor. The monitor performs a last
nonblocking exit check, marks remaining monitor status `Cancelled`, drops its
`Child` handles, and completes its P009 worker token. It does not send SIGTERM
or SIGKILL. The P009 bounded worker wait remains authoritative; a timeout is
reported as failure and is not treated as worker completion.

Dropping a Rust `Child` handle does not terminate its process. A game may
continue running after daemon shutdown, but monitoring ends when the daemon
exits. P019 does not guarantee survival across terminal, process-group,
logout, service-manager, cgroup, or system shutdown policies. It does not
persist session state or adopt processes after restart. P036 owns recovery.

The supervisor is daemon-owned, not GUI-owned. Closing a client/control handle
does not remove its registry entry or child handle. This boundary is covered by
a test using a harmless child process; it is not evidence that a real game
survives a physical GUI window closure.

## Telemetry, diagnostics, and support reports

Every session receives a `SamplingContextId` allocated separately from its
`SessionId`. The association is stable in that session summary and can scope
future `ObservationTarget::GameSession` batches. P019 starts no telemetry
provider and imports no recordings. P018 import-local MangoHud/frame contexts
are not joined to live sessions by PID or filename.

The allowlisted event codes are `SESSION_REGISTERED`, `SESSION_STARTED`,
`SESSION_EXITED`, `SESSION_MONITOR_FAILED`, and
`SESSION_SUPERVISOR_STOPPED`. Events have fixed messages; they contain no PID,
session ID, command line, executable or installation path, environment,
account, or arbitrary error string. No heartbeat or session history is
exported. Support reports truthfully indicate that supervisor infrastructure
exists, without asserting a live session, game launch, runtime, or FPS sample.

## Validation and remaining boundary

The session tests use synchronization for lifecycle transitions and controlled
helper children for actual exit/reap and shutdown behavior. The helper uses
only stdin control, has no network, GPU, or user-file access, and has an outer
cleanup deadline. The daemon has an integration test for worker readiness,
completion-token accounting, and session ownership. Existing Ping/Status
protocol v1.0 is unchanged; it does not expose session records or controls.

Safe manual daemon smoke (from the repository root):

```sh
cargo +1.85.0 build --locked -p endfield-unixd
cargo +1.85.0 test --locked -p endfield-unix-sessions
cargo +1.85.0 test --locked -p endfield-unixd
cargo +1.85.0 run --locked -p endfield-unixd -- --foreground
```

In another terminal, `cargo +1.85.0 run --locked -p endfield-unix -- service
ping` and `... service status` continue to exercise ordinary P010 behavior.
They do not register or display game sessions. A trusted test harness is used
for real child ownership tests.

`Real Endfield session supervision: UNVERIFIED`

`Arch desktop Endfield survival test: UNVERIFIED`

`Dell Vostro Endfield survival test: UNVERIFIED`

P020 can add Linux user-service-manager lifecycle integration. Real game launch
ownership belongs to P034/P035; interrupted launch and restart recovery belong
to P036.
