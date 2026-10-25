# CLI commands

`endfield-unix` is one executable with native GTK4 GUI mode and headless command mode. Static CLI commands parse and format output without initializing GTK, creating a libadwaita application, connecting to a display server, probing hardware, creating config/database directories, opening SQLite, starting processes, contacting the network, or launching background workers. `diagnose platform` and `diagnose hardware` are separate read-only Linux probe commands; they avoid GTK, the daemon, and application storage. Hardware output is printed only for this explicit command and is not automatically persisted or exported. The two `service` commands make one bounded local Unix-socket request and never start the daemon. See [IPC.md](IPC.md) for protocol, socket ownership, and authentication details.

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
| `endfield-unix service ping` | Complete authenticated Ping/Pong over the local daemon socket | No |
| `endfield-unix service status` | Print validated lifecycle, protocol, version, and capability data | No |

Help lists only implemented commands. Linux CPU and memory hardware probing is available through `diagnose hardware`; GPU discovery, runtime discovery, optimization, and game services are under development. The `about` output comes from package metadata and static project information; it does not claim detected hardware, installed runtimes, or game state.

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
Ping and Status; telemetry, session tracking, service-management commands,
and systemd installation remain unimplemented.

Examples:

```sh
endfield-unix --help
endfield-unix --version
endfield-unix about
endfield-unix diagnose report --output ./support-report.json
endfield-unix diagnose report --output ./support-report.json --overwrite
endfield-unix diagnose platform
endfield-unix diagnose hardware
endfield-unix help gui
endfield-unix service ping
endfield-unix service status
endfield-unix gui
```

## Parsing and dispatch

Clap parses OS-string arguments through its non-terminating `try_get_matches_from` API. The typed command model, parser, output formatters, and dispatcher live in separate modules under `apps/endfield-unix/src/cli/`. `main.rs` continues to pass OS arguments to the library entry point. Parser failures become status 2 and are written to stderr; static command output goes to stdout.

Both default startup and the `gui` command invoke the existing `start_gui()` boundary. No CLI command constructs GTK widgets or accesses UI types. The report command calls the diagnostics library directly; it does not initialize GTK or the logger, create application state directories, run hardware probes, or contact the network. Reports contain static application/component metadata and validated allowlisted event history when available. The command writes only to the requested output file, refuses symlink destinations, uses mode `0600`, and refuses an existing file unless `--overwrite` is set. `diagnose platform` and `diagnose hardware` call `endfield-unix-platform-linux` directly, require no daemon, and perform no writes or command execution. Hardware probing reads bounded fixed procfs/sysfs sources. The full or partial output labels unknown and unavailable facts explicitly; cgroup quotas remain separate from affinity and physical topology, and host RAM remains separate from cgroup memory controls. See [HARDWARE-LINUX.md](HARDWARE-LINUX.md).

Output writers can be injected through `run_with_io` for deterministic dispatch tests. Output and diagnostic write failures are handled without panicking, including closed pipes. The service commands use the shared `endfield-unix-ipc` client, enforce a 2-second total deadline, validate the response envelope and status fields, and never initialize GTK or auto-start `endfield-unixd`.

## Exit statuses

| Status | Meaning |
| ---: | --- |
| `0` | Command succeeded |
| `2` | Invalid arguments, unknown command, unsupported argument combination, or extra argument |
| `69` | GUI startup failed, including unavailable graphical session |
| `74` | Standard output or standard error write failed |
| `75` | Local daemon unavailable, connection/request timeout, shutdown, or saturation |
| `76` | Protocol mismatch or invalid daemon response |
| `77` | Socket permission or peer authentication denied |
| `65` | Daemon rejected an unsupported operation |
| `70` | Fatal platform identity or required hardware-probe failure |
| `78` | Platform probe is unsupported on this operating system |

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

## Planned command namespaces

The following namespaces are reserved for later work. The `diagnose`
namespace implements safe report export, Linux platform inspection, and
read-only Linux CPU/memory hardware inspection. `service` currently implements
only `ping` and `status`.

```text
endfield-unix game ...
endfield-unix runtime ...
endfield-unix optimize ...
endfield-unix benchmark ...
endfield-unix profile ...
```

Each future command must call the corresponding shared application service. Adding a command name before its service exists must not imply that the operation works.
