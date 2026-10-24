# Local IPC protocol

P010 implements a Linux-only local channel between `endfield-unix` and the
independently launched `endfield-unixd`. The shared library is
`endfield-unix-ipc` in `crates/ipc`; it is library-only, has no GTK dependency,
and contains portable protocol types separately from Linux socket and
credential handling. P010 does not validate a macOS or FreeBSD transport.

## Protocol 1.0

The IPC version is `1.0`. It is independent of package version `0.1.0`, the
C17 ABI, configuration schema, SQLite schema, domain snapshot schema, and
diagnostics event schema. Each connection carries one request and one response
at most. The daemon closes the connection after responding.

Every JSON envelope is a strict object with these required fields:

| Field | Type | Rule |
| --- | --- | --- |
| `protocol_major` | unsigned 16-bit integer | Must equal `1`. |
| `protocol_minor` | unsigned 16-bit integer | P010 supports `0`; a newer requested minor is rejected. |
| `request_id` | string | 1–64 ASCII letters, digits, `.`, `_`, or `-`; correlation only, never authentication. |
| `message_type` | string | A known request or response operation. |
| `payload` | typed JSON | Operation-specific and strict; unknown fields are rejected. |

Ping request and response:

```json
{"protocol_major":1,"protocol_minor":0,"request_id":"0000000000000001","message_type":"ping","payload":{}}
```

```json
{"protocol_major":1,"protocol_minor":0,"request_id":"0000000000000001","message_type":"pong","payload":"pong"}
```

Status uses `message_type: "get_status"` with an empty object payload. Its
response has `message_type: "status"` and a typed `payload.status` object with
the service name, daemon and application package versions, lifecycle state,
IPC availability, status code, implemented capabilities (`local_ipc`, `ping`,
`status`), and explicit unimplemented capabilities (`telemetry`,
`game_session_supervision`). Status data comes from the daemon's P009 lifecycle
coordinator; a successful CLI status requires a validated `Running` response.

An error response uses `message_type: "error"` and a `payload.error.code` enum:
`malformed_request`, `unsupported_protocol_version`,
`unsupported_operation`, `authentication_failure`, `server_busy`,
`request_timeout`, `service_shutting_down`, or `internal_failure`. A peer that
fails OS authentication is closed before its application message is parsed,
so the daemon does not send that peer an error response.

Major versions must match. P010 negotiates only the common v1.0 feature set;
it rejects a newer minor version instead of guessing how newer fields behave.
Unknown operations, missing required fields, invalid request IDs, and unknown
schema fields are rejected. The JSON parser retains its recursion limit, and
typed deserialization rejects extra fields. A client verifies the response
version, request ID, response type, payload shape, and status values.

## Framing and time limits

Each message uses a 4-byte unsigned length in network byte order followed by
exactly that many UTF-8 JSON bytes. The maximum request or response body is
16 KiB. Zero lengths and larger declared lengths are rejected before memory
allocation. Truncated bodies, invalid UTF-8, malformed JSON, and oversized
payloads are rejected. Read and write loops handle partial I/O.

The model accepts one request per connection. There is no request pipeline or
unbounded message stream. The client uses one 2-second deadline across connect,
write, and response read. Each server request has a 1.5-second overall deadline
from accept, including time spent waiting in the bounded dispatch queue. A
client that trickles bytes cannot renew that deadline one read at a time.

## Linux socket and ownership

The daemon requires an existing, absolute `XDG_RUNTIME_DIR`. It does not fall
back to `/tmp`, derive a path from `HOME`, or create the user's runtime root.
The runtime root must be a real directory owned by the current effective UID,
with no group or other permissions. The application creates and validates:

```text
$XDG_RUNTIME_DIR/endfield-unix/endfield-unixd.sock
```

The `endfield-unix` namespace directory uses mode `0700`; the socket and
`endfield-unixd.lock` use mode `0600`. The full socket path must fit Linux's
`sockaddr_un.sun_path` limit, including the terminating NUL. The CLI validates
the runtime directory, app directory, and existing socket type, owner, and
permissions before connecting. Static help, version, and about do not resolve
or create runtime paths.

Before processing every connection, Linux `SO_PEERCRED` is read through the
safe `nix` socket API. The peer UID must equal the daemon's effective UID.
Credentials supplied in JSON are never used. The application-owned runtime
directory also blocks traversal by other UIDs, while peer credentials remain
the authorization check. A different UID, including root when the daemon runs
as a normal user, is not automatically authorized.

Same-UID peer authentication is not a sandbox boundary between processes
already running as that user. A malicious same-user process can connect to the
socket. P010 exposes only read-only Ping and GetStatus. Future state-changing
operations need additional validation, authorization, and user-approval
rules.

The daemon holds an exclusive advisory instance lock for the runtime
namespace. It will not replace a second instance's socket. Recovery considers
only a same-UID socket object while holding that lock; it attempts a bounded
connection, checks Linux `/proc/net/unix` for a live kernel socket, then checks
the path's device and inode again before unlinking. If the live-socket check is
unavailable or ownership is ambiguous, startup fails closed. Regular files,
symlinks, and other UIDs' socket objects are never removed. During shutdown,
the daemon removes only the socket whose recorded device and inode still match
the socket it created. The lock file remains as a private lock anchor; the OS
releases the advisory lock when the daemon exits.

## Bounded service and lifecycle

The listener blocks in `poll()` on the listening socket and a private wake
stream. It does not busy-spin. Four registered worker threads handle clients;
the dispatch queue holds at most twelve more accepted clients. At most sixteen
clients are held by the application at once. If the queue is full, the newly
accepted connection is closed without parsing it. The operating-system listen
backlog remains finite and OS-managed. There is no thread-per-connection path.

The daemon initializes signal handling before IPC setup. It validates the
runtime root, takes the instance lock, binds and permissions the socket,
starts the bounded workers, and waits for listener readiness before changing
the lifecycle to `Running`. The listener does not accept until that transition.
SIGTERM and SIGINT wake the listener, stop new acceptance, drain or cancel
active work within the request deadline, remove the owned socket, release the
instance lock, stop diagnostics, and complete registered worker tokens before
the coordinator reaches `Stopped`. A second signal does not repeat cleanup.
The daemon reports unexpected IPC worker failure through the lifecycle
coordinator.

The CLI does not start the daemon or wait indefinitely. It distinguishes
unavailable service, timeout/shutdown, protocol or response failure,
authentication denial, and unsupported operation. No request logs a peer UID,
PID, socket path, request body, or successful ping event.

## Manual Arch Linux check

Install Rust 1.85.0, GTK4/libadwaita development packages, `pkgconf`, and GCC
as listed in the repository README. In a normal logged-in user session, open
Terminal A:

```bash
cargo +1.85.0 run --locked -p endfield-unixd -- --foreground
```

In Terminal B, using the same user session and its `XDG_RUNTIME_DIR`:

```bash
cargo +1.85.0 run --locked -p endfield-unix -- service ping
cargo +1.85.0 run --locked -p endfield-unix -- service status
```

Stop Terminal A with Ctrl+C. `service ping` succeeds only after a valid
authenticated Pong. `service status` prints daemon data only after validating
the request ID, protocol, response type, lifecycle, and capability fields. If
the daemon is not running, these commands return a nonzero status and do not
print `Daemon: running`.

## Tests and portability

`crates/ipc/tests` covers the protocol contract, malformed and extra fields,
framing limits, partial reads/writes, same-UID authentication policy,
permissions, path safety, active-instance refusal, stale recovery, real local
Ping/Status, malformed requests, concurrent clients, reconnection, and an
overall timeout, including a slow byte-trickle check that proves server
deadlines do not reset with each partial read. The
daemon lifecycle tests exercise SIGTERM and SIGINT. A subprocess integration
test starts the actual daemon and CLI with an isolated runtime directory,
checks real Ping and Status, drives more than the four-worker plus twelve-queue
client limit with slow partial frames, verifies over-capacity connections are
closed and service health recovers, sends malformed and truncated requests,
and signals a daemon with active work before checking bounded socket cleanup
and child reaping. It also starts a fresh daemon and verifies SIGINT cleanup.
The deterministic UID mismatch policy test does not claim a cross-UID kernel
test; that requires another UID and is not run in unprivileged CI.

P010 implements and validates Linux only. Future macOS or FreeBSD transports
need their own peer-authentication and socket-ownership implementation and
test gates before either platform is reported as supported.
