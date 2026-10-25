# Linux CPU and memory telemetry (P016)

P016 adds one-shot, read-only CPU and memory collection to the existing
`endfield-unix-platform-linux` library. P012 remains the inventory pass: it
reports CPU topology, memory capacity, cgroup controls, and readable cpufreq
interfaces. P016 samples changing CPU counters and memory gauges. Neither pass
starts continuous monitoring, writes telemetry history, changes system state,
or requires `endfield-unixd`.

## Architecture and source bounds

`LinuxCpuMemoryTelemetryProvider` implements P015's `TelemetryProvider` and
declares only whole-machine CPU use, per-logical-CPU use, current CPU
frequency, RAM used and available, swap used, and cgroup memory used. It uses
P015 request, observation, target, temporal, provenance, quality, issue,
collection, and batch types directly. The Linux adapter depends on
`endfield-unix-telemetry`; the portable telemetry and P007 domain crates gain
no Linux dependencies. The adapter remains a library and introduces no
binary, GTK dependency, unsafe code, C17 dependency, daemon operation, or
network access.

| Observation | Fixed source | Bound / interpretation |
| --- | --- | --- |
| CPU counters | `/proc/stat` | 256 KiB total, 512 bytes per CPU record, 4096 logical CPU records; longer unrelated lines are ignored within the file bound |
| Online CPU set | `/sys/devices/system/cpu/online` via P012 | P012's bounded CPU-list parser |
| RAM and swap gauges | `/proc/meminfo` via P012 | 64 KiB total; exact `kB` values normalized to bytes |
| Cgroup memory | Current process's validated cgroup v2 `memory.current` | P012 mount and cgroup resolution; no arbitrary path input |
| Current CPU frequency (optional) | cpufreq policy `cpuinfo_cur_freq` | P012 policy enumeration and related/affected CPU lists |

Source APIs accept fixed families, not arbitrary paths. The provider does not
open process command lines, enumerate processes, execute utilities, write
sysfs controls, change affinity or governors, or persist raw source contents.
Errors map to typed P015 states and safe diagnostics; raw kernel errors and
cgroup paths are not exported.

## CPU accounting

The parser reads aggregate `cpu` and `cpuN` records keyed by actual numeric
Linux CPU IDs. IDs must be canonical unsigned decimal indexes. Duplicate or
malformed records become record-local errors, so one bad core does not erase a
valid aggregate or sibling records. A malformed aggregate, invalid UTF-8,
overlong line, oversized file, excess CPU records, missing aggregate, invalid
counter, or changed field count cannot become measured zero. Optional counters
are retained only when present and never padded with zero.

Linux reports user, nice, system, idle, iowait, irq, softirq, steal, guest,
and guest_nice. Guest time is already included in user/nice, so guest counters
are checked for decreases but are not added again. The first eight fields
form total CPU-time delta. `idle + iowait` is treated as idle; the remaining
included delta is busy. This common Linux convention does not represent
process use, cgroup quota use, available headroom, or benchmark performance.

For earlier and later records:

```text
total_delta = sum(delta(user, nice, system, idle, iowait, irq, softirq, steal))
idle_delta  = delta(idle) + delta(iowait)
busy_delta  = total_delta - idle_delta
utilization_basis_points = round_half_up(busy_delta * 10000 / total_delta)
```

Counters are `u64`; subtraction and summation are checked and scaling uses
`u128` intermediates. Exact-half rounding is upward. Zero total delta,
decreasing counters, idle greater than total, incompatible layouts, or a
zero-length clock window produce degraded/invalid observations, never
negative or fabricated percentages. Valid idle is measured zero. P015 stores
utilization in basis points, where 10000 is 100%.

## Clock, CPU identity, and hotplug

The provider starts one injectable monotonic clock session, reads
`/proc/stat`, waits the requested bounded interval, then reads it again.
Production uses `std::time::Instant`; deterministic tests inject timestamps
and do not sleep. All timestamps in the batch use that session's clock domain.
Timestamps are taken after bounded source reads. CPU utilization carries the
actual P015 window and completion time; equal timestamps cannot produce a
measured rate. Memory/cgroup gauges use monotonic instants close to their
reads. Values from one `/proc/meminfo` read share one observation instant.

Logical CPU targets require a validated P012 report and caller-supplied
`SnapshotRefId`. The context retains P012's sorted online kernel CPU IDs and
the snapshot's CPU `InstanceId`; those IDs map to snapshot logical indexes.
Raw kernel IDs are never P015 logical indexes. This mapping lasts only for
that snapshot. Both online sets and both `/proc/stat` ID sets must match it
before per-core output. Hotplug, missing/new/offline records, excess records,
invalid IDs, or an unavailable snapshot prevents invented associations. One
unassociated degraded/unknown observation reports unresolved per-core scope
while valid aggregate utilization remains. A reset or malformed counter on
one core affects that core only.

Per-core output is capped at 4080 observations, leaving room under P015's
4096-observation batch limit for other metrics. An online set above the cap
produces unresolved per-core scope as a whole; the provider never truncates
and emits an arbitrary subset.

## Metrics and source semantics

P016 reuses P012's bounded `/proc/meminfo` parser; it does not substitute
`MemFree` for `MemAvailable`.

| P015 metric | Value and target | Provenance and quality |
| --- | --- | --- |
| `CpuUtilizationWholeMachine` (`0x0101`) | Basis points, platform | `linux-proc-stat`; derived counter delta; actual window |
| `CpuUtilizationLogicalCpu` (`0x0102`) | Basis points, snapshot-local logical CPU or explicit unassociated scope | `linux-proc-stat`; derived delta; individual core can be partial |
| `CpuFrequency` (`0x0103`) | Hertz, validated CPU target or unassociated CPU | `linux-cpufreq-cpuinfo-current`; normalized from kHz |
| `RamAvailable` (`0x0202`) | Bytes, platform; `MemAvailable` | `linux-proc-meminfo`; normalized from kB |
| `RamUsed` (`0x0201`) | Bytes, platform; `MemTotal - MemAvailable` | `linux-proc-meminfo`; derived and flagged `Estimated` |
| `SwapUsed` (`0x0203`) | Bytes, platform; `SwapTotal - SwapFree` | `linux-proc-meminfo`; derived |
| `CgroupMemoryUsed` (`0x0205`) | Bytes, caller-owned cgroup sampling context; `memory.current` | `linux-cgroup-v2-memory-current`; raw OS interface |

RAM used estimates host-visible used RAM. It is not exact physical allocation,
process memory, or cgroup usage. `MemAvailable > MemTotal` is rejected. Swap
free greater than total is malformed; valid zero swap is measured zero, while
missing or malformed fields remain missing or malformed. kB-to-byte
conversion is checked.

Cgroup usage comes only from v2 `memory.current`, resolved for the current
process by P012's safe source. It is not `memory.max`, host RAM, or one
process's RSS. V1, no cgroup, missing usage, unsafe resolution, permission
denial, and malformed values retain distinct unsupported, unknown, permission,
or malformed states. The observation uses the request's caller-owned sampling
context as its target; no cgroup path or container ID is attached.

CPU frequency is optional. The provider reads `cpuinfo_cur_freq`, never a
minimum or maximum, converts kHz to Hz with checked multiplication, validates
all online CPUs against the P012 snapshot, and emits a CPU-scoped value only
when every relevant policy agrees. Missing cpufreq, incomplete coverage,
disagreement, or stale mapping stays unavailable/degraded. Utilization is
never inferred from frequency.

## Partial results and CLI

Each requested metric gets a P015 observation with provenance, typed state,
safe diagnostic category, and quality. Missing values are never measured
zero. A `/proc/stat` failure does not erase valid memory; a `/proc/meminfo`
failure does not erase valid CPU; optional cpufreq and cgroup failures do not
invalidate either. `ProviderCollection::partial` carries typed issues matching
affected observations. Fatal errors are reserved for failures such as
inability to establish a valid clock or batch contract. Unrequested metrics
are neither read nor emitted.

The headless command performs one bounded collection and needs no GTK, daemon,
configuration, database, or network:

```sh
endfield-unix diagnose telemetry cpu-memory
endfield-unix diagnose telemetry cpu-memory --interval-ms 500
```

The default interval is 250 ms; the accepted range is 100–2000 ms. The
provider waits synchronously, so a future GUI caller must use an owned worker.
The CLI shows actual elapsed interval, basis-point percentage, binary MiB/GiB,
per-core status, source quality, monotonic clock, and complete/partial status.
Complete and partial results return 0; invalid input returns 2; fatal provider
failures return 70; output failure returns 74; unsupported platform returns
78.

## Validation and device status

Deterministic tests inject procfs and cpufreq fixtures plus timestamps. They
cover bounds, malformed records, guest accounting, utilization math/resets,
sparse IDs, hotplug, per-core independence, memory underflow/missing/zero/
overflow, cgroup error mapping, policy disagreement, P015 batch validation,
and CLI interval/format behavior. The live Linux smoke reads actual
`/proc/stat` and `/proc/meminfo`, waits one bounded interval, and accepts
source-supported partial results without asserting fixed host values.
Hosted qualification passed in [CI run 40205107552](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40205107552),
including the bounded live CLI and smoke test.
Its variable runner values are recorded in [TESTING.md](TESTING.md); they do not validate physical Arch hardware.

Manual Arch Linux checks:

```sh
cargo +1.85.0 run --locked -p endfield-unix -- diagnose telemetry cpu-memory
cargo +1.85.0 run --locked -p endfield-unix -- diagnose telemetry cpu-memory --interval-ms 500
head -n 12 /proc/stat
grep -E '^(MemTotal|MemAvailable|SwapTotal|SwapFree):' /proc/meminfo
```

For cgroup v2, inspect only the current authorized cgroup's `memory.current`
with an appropriate local tool; do not publish its path or identifier. Compare
the Intel Core i7-12700F desktop's online CPU set and counters with P012
topology, then repeat on the Dell Vostro 3510 with Intel Core i5-1135G7.
Synthetic fixtures and Ubuntu CI do not replace these checks. Current status:
**Arch desktop validation: UNVERIFIED**; **Dell Vostro 3510 validation:
UNVERIFIED**.

P016 does not implement GPU utilization, GPU memory/thermal/power, FPS,
frametime, process monitoring, daemon sampling, telemetry IPC, persistent
history, GTK graphs, benchmarks, or optimization actions. P017 remains the
next GPU telemetry milestone.
