# Structured diagnostics and support reports

P008 adds the reusable library crate `endfield-unix-diagnostics` at
`crates/diagnostics`. It records a small, typed set of application events and
generates a versioned support report from static metadata and validated local
events. It does not inspect hardware, configuration, SQLite contents, game
files, environment dumps, process arguments, or network services. It does not
upload data. This describes the diagnostics implementation; it is not a
security audit of the application.

## Architecture

The diagnostics crate depends on the P007 domain crate for the existing
`Diagnostic`, `DiagnosticCategory`, `ProbeError`, and `ProbeErrorKind` contracts.
It has no GTK4, libadwaita, Relm4, Wine/Proton, C17, or executable dependency.
It is library-only and forbids unsafe Rust. The application and P009's
`endfield-unixd` process use the same event types and storage policy.

`LogStore` is injectable through an explicit directory and retention policy.
`LogWorker` owns filesystem work on a bounded background thread. The GTK
lifecycle uses a queue with 256 event slots; event submission is nonblocking.
After the GTK loop returns, a flush waits up to 250 ms for queued lifecycle
events to finish. A timeout does not block the GTK event loop. `shutdown()`
waits for the worker thread to exit and is appropriate when the caller accepts
that join behavior. `shutdown_bounded()` waits for queue drain and a shutdown
acknowledgement, then detaches the worker's final return path so it never makes
an unbounded thread join. If filesystem I/O blocks beyond the timeout, the
method reports a timeout; the operation itself is not cancellable.

The global tracing subscriber is installed only on GUI startup. It accepts
events only for the private `endfield_unix::lifecycle` target and the five
compiled-in lifecycle names. The subscriber maps those names to fixed event
codes and messages. It never visits or formats arbitrary tracing fields.
Other tracing targets and names are ignored by the persistent event layer.
Services can record a `DiagnosticEvent` directly through `LogHandle` when they
have safe typed fields.

## Event schema and severity

Each JSON Lines record has `schema_version`, `timestamp_unix_nanos`,
`severity`, `component`, `code`, and a fixed `message`. Optional fields are an
operation outcome, P007 diagnostic category, registered capability ID,
allowlisted provider ID, and a bounded opaque correlation ID. Event schema
version 1 uses stable uppercase event codes and snake-case component/status
values. Deserialization rejects unknown keys, unsupported schema versions,
unapproved messages, invalid capability IDs, and malformed correlation IDs.

Severity values are `ERROR`, `WARN`, `INFO`, `DEBUG`, and `TRACE`. Persistent
logging defaults to `INFO`; `DEBUG` and `TRACE` are filtered. Lowering the
threshold cannot add arbitrary fields: events still use the typed model, and
the tracing layer still discards all field values. P008 lifecycle events are
application starting, GUI initialized, GUI startup failed, normal window
closure, and diagnostics unavailable. P009 adds the fixed daemon codes
`DAEMON_STARTING`, `DAEMON_RUNNING`, `DAEMON_SHUTDOWN_REQUESTED`,
`DAEMON_STOPPED`, `DAEMON_STARTUP_FAILED`, and `DAEMON_RUNTIME_FAILED`. They
require `component: daemon`, carry no operation fields, and use compiled-in
messages. Existing event codes, field meanings, and schema version 1 are
preserved.

## Domain diagnostic mapping

The adapter exports only category, optional known capability ID, and provider
identifiers from a fixed allowlist. It never exports `Diagnostic.message` or
`ProbeError.detail()`. Domain category and operation outcome remain separate:

| P007 category | Event outcome | Interpretation |
| --- | --- | --- |
| Unsupported capability | Unavailable | The provider reports this capability as unsupported. |
| Missing provider | Unavailable | No provider is available; this is not a hardware incompatibility result. |
| Permission denied | Failed | An attempted operation could not access its source. |
| Temporary failure | Failed | An attempted operation failed temporarily. |
| Malformed source data | Failed | An attempted operation rejected malformed data. |
| Stale observation | Incomplete | An observation exists but is stale. |
| Not yet probed | Not attempted | P007 supplied a model without testing the capability. |

Successful operations use `succeeded`; known gaps use `not_implemented`.
These values do not imply a capability was tested. Safe provider IDs currently
include `linux-procfs`, `linux-sysfs`, `linux-uname`, `linux-os-release`,
`linux-environment`, `linux-xdg-runtime-dir`, `drm`, `vulkan-loader`, `wayland`,
`x11`, `systemd-user`, `wine`, and `proton`. Unknown provider names are omitted.

## Local log paths and retention

On Linux, logs use:

```text
${XDG_STATE_HOME:-$HOME/.local/state}/endfield-unix/logs/
```

An `XDG_STATE_HOME` override must be absolute and contain no `..` component;
otherwise the resolver uses `$HOME/.local/state`. Configuration and database
paths in `endfield-unix-storage` are unchanged. The diagnostics path resolver is
a bounded extension for logs only and follows the same absolute-path and
final-component safety rules.

The active file is `current.jsonl`. Rotated files have the
`segment-<time>-<process>-<sequence>.jsonl` form. Retention enforces:

- 5 MiB maximum per segment.
- 5 retained segments total, including the current file.
- 7 days maximum age.
- 4 KiB maximum per JSON event line.
- 4096 directory-entry scan bound.

Rotation copies and synchronizes the current bounded segment to a new
create-only archive before removing the current path. It rotates before the
next line would exceed the segment limit. Events that exceed the configured
line bound are rejected; private content is never truncated into a log.
Cleanup only considers `current.jsonl` and correctly shaped app archive names.
Unrecognized files are preserved. Recognized symlinks, hard-linked files, and
special files are refused.

The application namespace and log directory use mode `0700`; lock, active,
archive, and report files use mode `0600` on Unix. The writer opens files with
no-follow flags and rejects final-component symlinks. XDG parent directories
retain their normal filesystem resolution behavior, matching the existing
storage convention.

Each writer takes an exclusive advisory lock on `.writer.lock` around retention,
rotation, and append. Processes using this crate coordinate safely with one
another. A program that bypasses the lock can still race the final filesystem
operation; the lock is advisory. Segment names and types are checked before
cleanup, so retention does not remove unrelated files.

## Failure behavior

Logging is optional and cannot block configuration, SQLite, game, or GUI
functionality. The bounded queue drops an event when full and exposes a dropped
event count to the caller. A file initialization or write failure emits one
static safe warning to stderr and disables the persistent sink for that
process. It does not recursively log the logging error. The error code can
distinguish permission denial, disk full, unsafe paths, and other I/O failure
without printing a path or source message.

Readers bound each segment read. Truncated, oversized, malformed, or
unknown-schema records are omitted and counted. They are never copied into a
support report. The logger does not rewrite a corrupt segment. A write failure
does not alter storage configuration or SQLite state.

## Privacy model

The default report is allowlist-first. The redaction policy assigns fields to
these classes:

| Sensitivity | Examples | Default report behavior |
| --- | --- | --- |
| Safe static metadata | Application name, package version, build status | Included. |
| Safe structured diagnostic | Severity, fixed code/message, category, registered provider, capability ID | Included after schema validation. |
| Potentially identifying | Correlation IDs, device IDs | Omitted. |
| Secret or forbidden | Free-form text, home paths, environment values, command lines, credentials | Not accepted by the report schema. |

The P007 free-form diagnostic message and probe error detail are discarded at
the adapter boundary, before they enter persistent event records. Live P011
platform facts are not automatically copied into logs or support reports. Report event
messages come from fixed code strings. Correlation IDs are never copied to a
report. The exporter has no field for usernames, absolute paths, tokens,
passwords, authorization headers, cookies, account credentials, arbitrary
provider output, network addresses, device identifiers, command-line
arguments, environment dumps, or database content. It does not run regexes
over already-persisted arbitrary strings.

The report contains event timestamps and aggregate severity counts from valid
retained events; those can reveal when the application was used. Local files
are permission-restricted but not encrypted. Anyone able to read the user's
account files can read them. Review a generated report before sharing it.

## Support report schema

`SupportReport` serializes as UTF-8 JSON with schema version `1`. Its stable
top-level fields are:

- `schema_version`, `generated_at_unix_nanos`.
- `application`: fixed name, package version, and development/release status.
- `components`: truthful availability for the application, CLI, GUI,
  diagnostics, storage, P007 models, optimizer skeleton, hardware probes,
  telemetry, and daemon. The daemon status is `partial`: authenticated IPC,
  Ping, Status, and lifecycle support work, while telemetry and session
  tracking are unavailable. The report does not query whether a daemon is
  currently running.
- `log_history`: available/missing/unavailable state, omitted corrupt record
  count, and `ERROR`/`WARN`/`INFO`/`DEBUG`/`TRACE` counts.
- `diagnostics`: up to the most recent 128 validated, allowlisted events.

The JSON output is capped at 256 KiB. The report does not include P007's
synthetic fixtures as measured hardware, does not claim probe results, and does
not query CPU/GPU, temperature, FPS, drivers, Proton installations, or game
launch sessions. It does not read configuration or SQLite and has no network
client.

## Safe export workflow

The headless CLI writes only to an explicit path:

```sh
endfield-unix diagnose report --output ./support-report.json
```

An existing path is preserved unless `--overwrite` is provided. The exporter
refuses symlink and special-file destinations, writes a same-directory
temporary file with mode `0600`, synchronizes it, then publishes it atomically.
For a new destination it uses a create-only hard link, so a concurrent file
creation cannot be overwritten. Incomplete temporary files are removed. The
command does not initialize GTK, start logging, create an application state
directory, run a hardware probe, or contact the network. File and standard
stream failures use CLI exit status `74`; invalid command syntax remains `2`.

## GUI and daemon integration

The shared executable initializes the logger only after CLI parsing selects GUI
startup. Static help, version, about, and report generation do not create log
files. GTK startup and window lifecycle events are recorded from safe static
metadata. Filesystem writes run on the diagnostics worker, not the GTK main
loop. The Diagnostics page continues to state that hardware probes and
telemetry are unimplemented.

`endfield-unixd` depends on this library without GTK and does not install the
GUI's global tracing subscriber. It records only the fixed daemon lifecycle
events through `LogHandle`, sharing the same bounded worker, private paths,
retention, and cross-process lock. P010 deliberately adds no per-ping or
peer-identity events. At shutdown it uses `shutdown_bounded()`;
the queue-drain timeout is bounded, but a filesystem operation already in
progress cannot be forcibly canceled. Logging remains optional, and failure
emits one sanitized notice without failing daemon lifecycle cleanup. Neither
process writes periodic idle events. Future service adapters should emit stable
codes and registered IDs, preserve P007 category semantics, and omit
provider-controlled free-form details at collection time.

## Validation

P008 validation uses temporary directories and synthetic secrets. The
diagnostics tests cover event schema validation, allowlist redaction, category
mapping, operation outcomes, line-size rejection, rotation, age/count
retention, permissions, symlink refusal, concurrent writers, truncated and
oversized records, worker shutdown, report export, and temporary-file cleanup.
P009 adds daemon event serialization and component checks, support-report
availability compatibility, and real headless lifecycle integration. P010
retains the schema-version-1 support report while updating the meaning of the
daemon's `partial` availability. Workspace and hosted CI results are recorded
in [TESTING.md](TESTING.md).
