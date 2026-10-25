# Linux platform and runtime discovery

P011 adds read-only Linux platform identity discovery and P012 extends the same
adapter with CPU, memory, cgroup, scheduling, cpufreq, and power-interface
discovery in `crates/platform-linux`, published as the library crate
`endfield-unix-platform-linux`. P011's hosted Linux qualification passed in
[CI run 40087592204](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40087592204),
and P012's complete hosted qualification passed in [CI run
40161140586](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40161140586).
Linux CI does not establish support on Arch Linux, CachyOS, or the user's
physical devices.

## Boundary and report shape

The adapter depends on `endfield-unix-domain` and reuses P007's
`PlatformIdentity`, `KernelIdentity`, `DistributionIdentity`, `Architecture`,
`SystemSnapshot`, `PlatformProbe`, `ProbeError`, `Diagnostic`, and
`Provenance`. The domain crate remains dependency-free and contains no Linux
APIs. `LinuxPlatformReport` wraps a validated P007 snapshot with typed
distribution details, runtime-environment facts, user runtime-directory
assessment, a completeness state, and report schema version 1. This extension
does not change P007 snapshot schema 1, the P003 C ABI, or P010 IPC protocol
1.0.

The source boundary is injectable and read-only. It permits bounded reads of
the two fixed os-release sources and the one fixed systemd container marker,
native kernel identity, checks of three allowlisted marker paths, and metadata
inspection of the current `XDG_RUNTIME_DIR`. It does not provide arbitrary
file browsing, directory traversal, process inspection, writes, network
access, command execution, or privileged operations.

## Distribution metadata

The source order is:

1. `/etc/os-release`
2. `/usr/lib/os-release`, only when `/etc/os-release` is missing

The two files are never merged. If the preferred source exists but is
unreadable, malformed, oversized, or an unexpected type, the probe reports a
partial result instead of using the fallback. `/etc/os-release` may be a
symlink only when its lexical target resolves exactly to `/usr/lib/os-release`
(including the conventional relative target `../usr/lib/os-release`). The
target must be a regular file opened with no-follow behavior. Other symlink
targets and symlink chains are rejected. The probe is unprivileged; the fixed
target rule also avoids following an arbitrary path if reused in a more
privileged process.

The parser:

- Caps the file at 64 KiB, each line at 4 KiB, every parsed value at 1 KiB,
  and selected domain fields at 128 bytes.
- Requires valid UTF-8, uppercase assignment keys, and one assignment per
  non-comment line. It handles CRLF and surrounding line whitespace.
- Supports unquoted values without unescaped whitespace, single-quoted
  literals, double-quoted values, and the documented backslash escapes. It
  never invokes a shell, expands variables, or evaluates command syntax.
- Rejects malformed assignments, invalid escapes, unterminated quotes,
  trailing characters after quotes, control characters, and invalid selected
  fields. Unknown well-formed fields are discarded.
- Uses last-valid-assignment-wins for duplicate keys.

The selected fields are `ID`, `NAME`, `PRETTY_NAME`, `VERSION_ID`, `VERSION`,
`ID_LIKE`, and optional `VARIANT_ID`. P007 receives normalized `ID`,
`VERSION_ID` (falling back to the actual `VERSION` field when present), and
`PRETTY_NAME` (falling back to `NAME`). Missing version fields remain `None`;
the adapter never invents a version for rolling releases. The Linux-specific
report retains validated `ID_LIKE`, `VARIANT_ID`, and source selection.

## Running kernel and architecture

The running kernel is queried through `nix::sys::utsname::uname`, not by
starting the external `uname` program. The adapter validates and preserves the
kernel system name and full release string, including distro or custom kernel
suffixes. The machine field reported by the running kernel is normalized
through P007 `Architecture`, including x86, x86_64, ARM, AArch64, RISC-V,
PowerPC, s390x, and preserved unknown names.

Architecture is kernel-reported. The adapter does not use
`std::env::consts::ARCH` as host evidence and does not conflate a 32-bit
executable with the architecture reported by a 64-bit kernel. The executable's
build-target architecture is not included in this report.

## Runtime environment classification

The result is evidence-oriented and includes classification, identified or
uncertain status, confidence, safe source IDs, a bounded fixed diagnostic,
and explicit uncertainty. It checks only:

- `/.dockerenv` and `/run/.containerenv` as marker files.
- `/run/systemd/container` against a fixed allowlist of known container names.
- The kernel release returned by `uname` for recognizable `Microsoft` or
  `WSL` markers.

A recognized marker can identify a container or WSL. Conflicting WSL and
container indicators produce `Unknown`. Permission or source failures reduce
confidence and may produce `Unknown`. With no recognized markers and no source
failures, the result is `NativeLinux` with low confidence and an explicit note
that this does not prove a bare-metal host. WSL generation is not inferred.
The report does not claim GPU acceleration, Wine/Proton availability, or game
compatibility.

## User runtime directory

The adapter observes `XDG_RUNTIME_DIR` without creating or repairing it. It
uses the same policy as P010's IPC root: the path must be absolute, its final
component must be a real directory rather than a symlink, its owner must match
the current effective UID, group/other permission bits must be clear, and the
derived P010 socket path must fit Linux's `sockaddr_un.sun_path` bound. The
result distinguishes available, missing, invalid/unsafe, permission denied,
and unknown. The path itself is never shown or retained in the report.

## Partial failure, provenance, and privacy

Running-kernel identity is required for a successful probe. A kernel-source
failure returns a typed `ProbeError`. Missing or malformed distribution data,
uncertain environment evidence, and unsafe or missing runtime directories do
not erase a valid kernel snapshot; they produce partial results and safe
diagnostics. Unknown remains distinct from unavailable.

P007 provenance uses fixed provider IDs `linux-uname` and
`linux-os-release`, with the time the source was observed. Diagnostics use
fixed provider IDs and bounded messages. The report omits usernames, paths,
hostnames, machine IDs, container IDs, raw environment values, command lines,
and raw source text. P008 still discards arbitrary diagnostic messages before
persistent logging. Live P011/P012 facts are not automatically added to
support reports.

## P012 hardware discovery

The adapter also provides one-shot CPU, memory, scheduling, cgroup v2,
cpufreq, and platform power-profile discovery through `LinuxHardwareProbe`.
The probe composes with P011 identity into a validated P007 snapshot and a
versioned `LinuxHardwareReport`; independent failures become safe diagnostics
and partial facts. See [HARDWARE-LINUX.md](HARDWARE-LINUX.md) for the exact
source precedence, parsing rules, cgroup ancestor/v1 limits, and interface
semantics. The report is local to an explicitly requested CLI invocation and
is not saved to logs or support reports.

Run the headless command with:

```bash
endfield-unix diagnose hardware
```

It does not initialize GTK or application storage, start `endfield-unixd`,
write sysfs/procfs, or run external commands. The Linux adapter remains
library-only and does not depend on GTK. Rust 1.85.0 is the supported compiler;
the adapter uses the locked `libc` 0.2.191 and `nix` 0.29.0 crates.

## CLI

Run the local platform or hardware probe with:

```bash
endfield-unix diagnose platform
endfield-unix diagnose hardware
```

The command is headless, does not initialize GTK, does not read configuration
or SQLite, and does not need `endfield-unixd`. It prints validated facts and
labels partial or uncertain results. Complete and partial reports return
status `0`; unsupported platforms return `78`; fatal kernel identity failures
return `70`; output failures return `74`; invalid arguments retain status `2`.
Existing GUI and P010 IPC statuses remain unchanged.

## Manual Arch Linux validation

On a normal Arch Linux installation with the pinned Rust toolchain and GTK
build dependencies installed, run:

```bash
cargo +1.85.0 run --locked -p endfield-unix -- diagnose platform
cargo +1.85.0 run --locked -p endfield-unix -- diagnose hardware
cat /etc/os-release
uname -srm
cat /proc/meminfo
cat /sys/devices/system/cpu/online
```

Compare the reported distro, kernel release, and kernel machine with the local
system output. Omit private identifiers if sharing logs. The hosted Ubuntu
runner provides live Linux smoke coverage but does not substitute for a manual
Arch, CachyOS, or physical-device validation session. Real Arch device
validation: **UNVERIFIED**.

## Non-goals

P012 does not detect display, Wayland/X11,
refresh-rate, Wine, or Proton state. It does not create IPC requests, start the
daemon, change system configuration, install services, tune a kernel, or
launch a game. macOS and FreeBSD probing is not implemented.
## P013 GPU integration

P013 extends the existing Linux adapter with bounded PCI and DRM enumeration and dynamic Vulkan discovery. The GPU report composes the P011/P012 snapshot without changing platform schema, storage, IPC, or C ABI contracts. It does not expose GPU probing over IPC or add daemon work. See [GPU-LINUX.md](GPU-LINUX.md) for PCI classes, topology, correlation, memory semantics, and Arch validation.
