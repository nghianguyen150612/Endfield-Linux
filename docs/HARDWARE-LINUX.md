# Linux CPU and memory discovery

P012 adds the headless `endfield-unix diagnose hardware` command and extends
the existing `endfield-unix-platform-linux` library. It performs one bounded,
read-only discovery pass. It does not monitor workload, estimate performance,
recommend a power mode, or change system settings.

## Source and domain boundary

The probe composes a validated P011 `LinuxPlatformReport` with P012 facts in a
versioned `LinuxHardwareReport`. P007's portable `SystemSnapshot` contains
validated CPU and memory models, scalar capabilities, provenance, and safe
diagnostics. Linux-only cgroup controls, CPU set details, cpufreq policies, and
platform profile facts stay in `LinuxHardwareReport`, since the portable
snapshot has no corresponding detailed fields. Snapshot schema 1, C17 ABI
1.0, TOML/SQLite schema v1, and P010 IPC v1.0 are unchanged.

The source trait names fixed source families; it does not accept arbitrary
paths. `/proc/cpuinfo` is capped at 256 KiB, `/proc/meminfo` at 64 KiB, CPU
lists at 16 KiB, individual metadata and cgroup control files at 4 KiB, and
policy enumeration at 256 entries. CPU-list IDs are capped at 1,000,000;
affinity discovery has its own 1 MiB mask bound. The source opens regular files
with no-follow behavior. Known sysfs directory
links are canonicalized and accepted only when they stay within their fixed
system CPU or firmware boundary. Cgroup paths come from bounded process and
mount metadata, are checked for traversal, and must resolve under the selected
cgroup mount. No shell, external command, network, root access, directory-tree
scan, or write is used.

## CPU inventory and topology

The adapter reads `/sys/devices/system/cpu/online`, `present`, and `possible`.
Online CPUs provide the reported logical processor count and the logical IDs
used for topology. Present-but-offline IDs are listed separately where both
sets are available. Possible and present are optional observations: a missing
list is unknown, not zero. Inconsistent online/present/possible sets produce a
diagnostic rather than silently substituting counts.

Physical cores are counted only when every online CPU has valid
`physical_package_id` and `core_id` metadata. When every online CPU also
reports `die_id`, the identity tuple is
`(physical_package_id, die_id, core_id)`; when `die_id` is absent for every
CPU, the tuple is `(physical_package_id, core_id)`. Mixed or malformed die
metadata makes the physical-core count unknown. This avoids treating a core
number as globally unique and avoids deriving core count from SMT ratios. No
Intel P-core/E-core classification is inferred.

Process affinity is queried independently through `sched_getaffinity(0, ...)`.
The implementation starts with a 128-byte mask and grows it on `EINVAL` up to
1 MiB, then returns the set CPU IDs, including noncontiguous and large indexes.
An empty or invalid mask is not reported as zero CPUs. The affinity set is
distinct from online CPUs, cgroup `cpuset.cpus.effective`, and CPU-time quota.

The optional model string comes from bounded `/proc/cpuinfo` text. The parser
prefers `model name`, then architecture-specific `Hardware`, `Model`, and
`Processor` fields. It validates UTF-8 and field size, rejects control
characters, ignores numeric processor identifiers, and returns unknown when
the highest-priority model field is ambiguous. It never invokes `lscpu`,
looks up marketing specifications, or retains processor serial numbers.

## RAM and swap

The parser reads `MemTotal`, optional `MemAvailable`, `SwapTotal`, and optional
`SwapFree` in any order. Every selected value must be an unsigned integer
followed by the exact kernel unit `kB`; conversion uses checked multiplication
by 1024 bytes. Oversized data, invalid UTF-8, overlong lines, duplicate
selected fields, invalid units, negatives, and overflow become safe partial
results. `MemTotal` is required and must be nonzero. `MemAvailable` is not
replaced with `MemFree` or an estimate; if absent it remains unknown. An
available value greater than total is discarded and degrades the RAM record.

A valid `SwapTotal: 0 kB` means no swap capacity is configured in this
observation. Positive `SwapTotal` supplies configured capacity; `SwapFree` is
shown only when valid and not greater than total. Missing or malformed
`SwapTotal` means unknown, not absent. These values reflect `/proc/meminfo`;
they are not substituted with a cgroup limit.

## Cgroup resource controls

The source resolves the current process's unified cgroup entry from
`/proc/self/cgroup` and its cgroup2 mount from `/proc/self/mountinfo`. It maps
the process path relative to the mount's declared root, selects the most
specific compatible mount, validates the canonical target, and reads only
fixed control names:

- `cpu.max`
- `cpuset.cpus.effective`
- `memory.max`
- `memory.current`
- `memory.swap.max`

`cpu.max` is kept as a positive quota/period rational. The text `max` is
unlimited for that control; no fractional quota is rounded to a physical core
count. `cpuset.cpus.effective` is another CPU-set constraint and does not
replace process affinity. `memory.max` and `memory.swap.max` preserve the
kernel's `max` as unlimited; finite limits remain distinct from
`/proc/meminfo`. `memory.current` is the cgroup accounting value. The report
does not calculate application-available memory from the difference between
limit and current usage.

Only the resolved cgroup leaf is queried. Ancestor limits are not assessed, so
the leaf values are not represented as the complete effective limits. Cgroup
v1 is explicitly reported as unsupported/partial; v2 parsing is never applied
to v1 files. If cgroup resolution is unsafe or unavailable, its state remains
unknown without erasing other facts. Raw cgroup paths and container IDs are
never displayed, persisted, or included in diagnostics.

## Scheduling, frequency, and power interfaces

The adapter queries process affinity, `sched_getscheduler(0)`, and
`sched_getparam(0)`. It reports the current scheduler policy name and static
priority only when those read-only queries succeed. Query support does not
establish permission to modify policy. Mutation permission remains
unassessed/unknown, and the adapter never calls scheduler or affinity setters
or changes priority.

Cpufreq policies are enumerated under
`/sys/devices/system/cpu/cpufreq/policyN`, with a 256-policy bound and
fixed-name optional reads for driver, current/available governors, hardware
and policy frequency limits, related/affected CPU sets, and energy-performance
preference. Frequency values in kHz are converted to Hz with checked
multiplication. Hardware (`cpuinfo_*`) limits are kept separate from selected
policy (`scaling_*`) limits. Inconsistent ranges become unknown with a safe
diagnostic. A readable governor or preference does not prove a file is
writable or that the user can change it.

Where available, the fixed ACPI platform-profile interface reports the current
and offered profiles. This is only interface/state discovery. The probe does
not sample instantaneous CPU frequency, infer thermal limits, diagnose power
throttling, or recommend performance mode. `CpuInfo.observed_frequency`
remains unknown because this is a one-shot probe. `CpuInfo.power_management`
remains unknown because the portable field does not distinguish readable
interfaces from permission to change them.

## Normalization, provenance, and partial failures

P012 uses P007 `CpuInfo`, `CpuTopology`, `MemoryInfo`, `MemoryCapacity`,
`SwapInfo`, `CapabilityRecord`, and the stable capability IDs: CPU topology
`1`, scheduling `2`, CPU frequency/power interfaces `3`, RAM `20`, and swap
`21`. CPU logical count means online CPUs. Process-allowed IDs, cpuset,
quota, physical cores, and host RAM stay distinct. Bytes and frequency values
remain exact integers internally; capability measurements carry `Count`,
`Bytes`, or `Hertz` units. Each successful source receives a fixed provider
ID: `linux-cpu-sysfs`, `linux-cpuinfo`, `linux-meminfo`, `linux-cgroup`,
`linux-cpufreq`, `linux-scheduler`, or `linux-power`.

P011 identity remains present when an optional P012 subsystem fails. A failed
CPU model read does not invalidate topology; missing memory does not erase CPU
facts; cgroup and cpufreq failures do not erase either. Unknown values remain
unknown, and bounded diagnostics include only safe provider IDs and fixed
messages. The snapshot is normalized and validated after merge, with unique
snapshot-local CPU and provenance IDs. No GPU or display information is
produced.

Hardware data appears locally only in response to `diagnose hardware`. It is
not sent to `endfield-unixd`, stored in the support-report event history, or
added to persistent logs.

## CLI and verification

Run:

```bash
endfield-unix diagnose hardware
```

Complete and partial reports return status `0`. A required platform identity
failure returns `70`, unsupported platforms return `78`, output failures
return `74`, and invalid arguments return `2`. Optional-source failures are
shown as unknown/partial facts.

The deterministic P012 tests include the synthetic reference configurations:

- Desktop fixture: Intel Core i7-12700F, 12 physical cores, 20 logical CPUs,
  hybrid-style topology without P/E-core labels, and 32 GiB RAM.
- Laptop fixture: Intel Core i5-1135G7, 4 physical cores, 8 logical CPUs, and
  16 GiB RAM.

These are test-only inputs, not measurements from actual machines or
production defaults. Hosted Linux CI runs parser/contract tests, a live
read-only probe and the actual CLI command against its Ubuntu runner; all P012
gates passed in [CI run 40161140586](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40161140586).
Ubuntu CI does not establish Arch Linux or physical-device support. Real Arch
device validation: **UNVERIFIED**. For manual comparison commands, see
[PLATFORM-LINUX.md](PLATFORM-LINUX.md).
