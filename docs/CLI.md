# CLI commands

`endfield-unix` is one executable with native GTK4 GUI mode and headless command mode. Static CLI commands parse and format output without initializing GTK, creating a libadwaita application, connecting to a display server, probing hardware, creating config/database directories, opening SQLite, starting processes, contacting the network, or launching background workers.

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

Help lists only implemented commands. Game, runtime, optimization, benchmarking, daemon, and hardware-probe services are under development. The `about` output comes from package metadata and static project information; it does not claim detected hardware, installed runtimes, or game state.

Examples:

```sh
endfield-unix --help
endfield-unix --version
endfield-unix about
endfield-unix diagnose report --output ./support-report.json
endfield-unix diagnose report --output ./support-report.json --overwrite
endfield-unix help gui
endfield-unix gui
```

## Parsing and dispatch

Clap parses OS-string arguments through its non-terminating `try_get_matches_from` API. The typed command model, parser, output formatters, and dispatcher live in separate modules under `apps/endfield-unix/src/cli/`. `main.rs` continues to pass OS arguments to the library entry point. Parser failures become status 2 and are written to stderr; static command output goes to stdout.

Both default startup and the `gui` command invoke the existing `start_gui()` boundary. No CLI command constructs GTK widgets or accesses UI types. The report command calls the diagnostics library directly; it does not initialize GTK or the logger, create application state directories, run hardware probes, or contact the network. Reports contain static application/component metadata and validated allowlisted event history when available. The command writes only to the requested output file, refuses symlink destinations, uses mode `0600`, and refuses an existing file unless `--overwrite` is set.

Output writers can be injected through `run_with_io` for deterministic dispatch tests. Output and diagnostic write failures are handled without panicking, including closed pipes.

## Exit statuses

| Status | Meaning |
| ---: | --- |
| `0` | Command succeeded |
| `2` | Invalid arguments, unknown command, unsupported argument combination, or extra argument |
| `69` | GUI startup failed, including unavailable graphical session |
| `74` | Standard output or standard error write failed |

Status `74` also covers report destination and file-write failures. Invalid
syntax or an unsupported report option remains status `2`.

## Planned command namespaces

The following service namespaces are reserved for later work. The `diagnose`
namespace currently implements only the safe report export described above;
hardware diagnostics remain unimplemented.

```text
endfield-unix game ...
endfield-unix runtime ...
endfield-unix optimize ...
endfield-unix benchmark ...
endfield-unix service ...
endfield-unix profile ...
```

Each future command must call the corresponding shared application service. Adding a command name before its service exists must not imply that the operation works.
