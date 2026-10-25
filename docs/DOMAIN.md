# Portable domain models and platform interfaces

P007 adds the reusable Rust domain library `endfield-unix-domain` at
`crates/domain`. It represents normalized facts that future adapters and
services can share. It does not inspect the host operating system, enumerate
hardware, detect Vulkan or a display server, launch processes, or call the C
planner.

The crate uses only the Rust standard library and has `#![forbid(unsafe_code)]`.
It has no dependency on GTK4, libadwaita, Relm4, the application binary, storage,
the optimizer FFI wrapper, or platform-specific bindings. It is a library-only
workspace member and adds no executable.

## Platform identity

`PlatformIdentity` combines a `PlatformFamily`, OS name and optional version,
typed `Architecture`, and optional kernel and distribution identities. A Linux
distribution is valid only with the Linux family. Optional versions remain
absent when the adapter does not know them; the domain layer does not infer a
version from a name.

P011 and P012 are implemented by the Linux adapter in `crates/platform-linux`. It maps the
running kernel's `uname` system name, release, and machine fields into these
existing P007 types, and maps validated os-release `ID`, version, and display
name into `DistributionIdentity`. The portable schema is unchanged. Linux-only
container/WSL evidence and user runtime-directory status live in the adapter's
separately versioned `LinuxPlatformReport`. P012 CPU, memory, and their
capability records are normalized into the same snapshot; details without a
portable P007 field (such as cgroup controls and frequency-policy metadata)
remain in `LinuxHardwareReport`.

The family identifiers are the existing P001/P003 C ABI values:

| ABI value | Rust variant |
| ---: | --- |
| 1 | `Linux` |
| 2 | `MacOs` |
| 3 | `FreeBsd` |
| 255 | `OtherUnknown` |

Conversions use checked `TryFrom<u32>` mappings. Other numeric values are
rejected. The architecture parser recognizes common x86, ARM, RISC-V, PowerPC,
and s390x names and preserves an unfamiliar normalized name as `Other`. An
unknown family, OS version, kernel, or architecture does not prove application,
runtime, driver, Vulkan, or game compatibility.

## Capability registry and availability

`CapabilityId` explicitly maps the P001 registry. Unlisted and reserved values
are rejected instead of being accepted through numeric casts.

| ID | Capability | Scalar measurement unit, when applicable |
| ---: | --- | --- |
| 1 | CPU topology | Count |
| 2 | CPU scheduling | No scalar |
| 3 | CPU frequency/power | Hertz, milliwatts, millivolts, joules, or millijoules |
| 10 | GPU device | Count |
| 11 | VRAM | Bytes |
| 12 | Hardware acceleration | No scalar |
| 13 | Graphics driver | No scalar |
| 14 | Vulkan device | Count |
| 20 | RAM | Bytes |
| 21 | Swap | Bytes |
| 30 | CPU thermal | Milli-Celsius |
| 31 | GPU thermal | Milli-Celsius |
| 32 | Power telemetry | Milliwatts, millivolts, joules, or millijoules |
| 40 | Display | Pixels |
| 41 | Compositor/session | No scalar |
| 50 | Runtime discovery | Count |
| 51 | Optimization-tool availability | Count |

Where the current generic scalar record can map a typed inventory value, RAM's
scalar is total capacity, while available RAM remains in `MemoryInfo`; display
resolution remains a width/height pair in `DisplayInfo`. P007 does not flatten
those composite fields for the C ABI.

`CapabilityRecord` carries the identity, availability, optional measurement,
optional snapshot-local instance, provenance reference, and diagnostic. Its
availability values match the C ABI:

| ABI value | State | Meaning |
| ---: | --- | --- |
| 0 | Unknown | Not queried, not understood, or not enough evidence |
| 1 | Unavailable | Evidence establishes the capability is absent or unsupported |
| 2 | Available | Evidence establishes the capability is present |
| 3 | Degraded | Partially available, stale, or impaired observation |

Unknown and unavailable records cannot carry measurements. An available record
may omit a scalar because many capabilities are qualitative. A degraded record
requires a diagnostic. Any non-unknown record requires provenance. A valid
numeric zero is an ordinary measurement and stays distinct from `None`.

## Measurements and conversion

`MeasurementUnit` uses the complete ABI unit registry: count, bytes, hertz,
pixels, milli-frames per second, milliseconds, microseconds, nanoseconds,
milli-Celsius, milliwatts, millivolts, joules, and millijoules. Capability
records validate unit/capability combinations. For example, RAM cannot use
hertz and a scheduling capability cannot carry an invented scalar.

Measurements record whether a number is raw, normalized, or derived. They use
integers only. `checked_convert_unsigned` rejects overflow, zero or invalid
scales, and conversions that would lose precision; it does not use floating
point or silently round. The domain can represent signed milli-Celsius values
because temperatures below zero are valid facts. C17 ABI v1 stores measurement
values as `uint64_t`, so `Measurement::to_abi` rejects a negative value instead
of wrapping it. The full snapshot-to-C marshalling pipeline remains P042.

## Hardware and display contracts

The inventory models are descriptive contracts; their optional values do not
trigger probing.

- `CpuInfo` can represent a model identifier, topology availability and counts,
  an observed frequency, scheduling availability, and power-management
  availability. Physical-core counts remain optional; the model makes no SMT
  or hybrid-core assumptions.
- `GpuInfo` has a nonzero snapshot-local instance identity, optional vendor and
  model, integrated/discrete/unknown classification, optional driver identity,
  VRAM availability/capacity, hardware-acceleration availability, and Vulkan
  device availability. Multiple devices are supported; list order does not
  select a preferred GPU.
- `MemoryInfo` keeps RAM availability, total and available bytes, and swap
  availability, presence, and size separate. A known absent swap device is
  represented by `present = Some(false)`; unknown swap remains `None`.
- `DisplayInfo` supports multiple displays, optional resolution and refresh
  rate, optional primary status, connection information, and session
  availability. Refresh rate is represented in integer hertz. Display labels
  should omit EDID serials and other private identifiers. No Wayland/X11
  detection is included.

Constructors and `validate` methods reject contradictory values such as zero
CPU counts, available RAM greater than total RAM, a nonzero swap size when
swap is absent, or a zero display dimension. Zero remains representable in
generic measured quantities where it is a valid observation, including an
observed zero frequency or thermal reading.

## Snapshots, provenance, and diagnostics

`SystemSnapshot` groups schema version, platform identity, optional CPU and
memory data, GPU and display inventories, capability records, provenance, and
diagnostics. Its current in-memory schema is version 1. It is independent of
application, TOML, SQLite, and C ABI versions. P007 adds no snapshot serializer
or SQLite tables.

`InstanceId` and `ProvenanceId` are nonzero IDs scoped to one snapshot. A
capability with no instance maps to ABI instance zero, meaning platform-wide.
Validation rejects duplicate device IDs, duplicate capability/instance pairs,
unresolved instance references, duplicate provenance IDs, and unresolved
provenance references. Typed inventory availability is cross-checked against
its corresponding capability record. Scalar values with a direct one-value
mapping, such as logical processor count, VRAM, total RAM, and swap size, are
cross-checked too. Composite fields such as available RAM and display
resolution stay in their typed models instead of being flattened by P007.
`normalize` sorts device inventories, capability records,
provenance, and diagnostics so semantic input ordering does not change the
normalized representation. Partial snapshots are valid. An unsupported schema
version is rejected.

`Provenance` stores a bounded provider name and optional Unix-epoch nanosecond
observation time. It avoids requiring a timestamp when the source cannot supply
one. `DiagnosticCategory` distinguishes unsupported capability, missing
provider, permission denied, temporary failure, malformed source data, stale
observation, and not yet probed. A diagnostic can also carry a stable provider
name; provider names are restricted to short identifier characters, not paths
or free-form command output. Optional diagnostic text is bounded; adapters must
not put usernames, home paths, full command arguments, or other private machine
identifiers in it. P008 defines the structured logging and support report
boundary for these values.

## P008 diagnostic conversion

The `endfield-unix-diagnostics` crate adapts these existing domain types rather
than defining replacement categories. It preserves all seven category IDs and
maps them to explicit outcomes: unsupported capability and missing provider
are `unavailable`; permission, temporary, and malformed-source failures are
`failed`; stale observations are `incomplete`; and not-yet-probed is
`not_attempted`. A missing provider never becomes a hardware incompatibility
claim. Successful operations and known unimplemented operations use separate
outcomes.

The adapter drops `Diagnostic.message` and `ProbeError.detail()` at the
collection boundary. It exports only a registered capability ID and provider
names from the diagnostics crate's fixed allowlist. Unknown provider names are
omitted. These adapters do not invoke `PlatformProbe`, `CapabilityProvider`,
or a hardware source. See [DIAGNOSTICS.md](DIAGNOSTICS.md) for event and report
schemas.

## Probe interfaces and partial failures

`PlatformProbe::probe` is a synchronous, portable contract. A complete failure
returns `ProbeError`; a valid partial report is a successful snapshot with
diagnostics. `CapabilityProvider` is the synchronous boundary for one
independent provider and a `ProbeReport`.

`CompositePlatformProbe` runs its configured providers independently. It
retains valid results if another provider fails and records the failed
subsystem's diagnostic category. Malformed or conflicting provider data is
discarded without erasing earlier valid reports. No providers and all providers
failing are typed total errors. These traits contain no GTK types, OS-specific
paths, global mutable state, privileged access, network operation, or hardware
probe. P011 and P012 implement Linux discovery through the same adapter. A
valid running-kernel identity is required; distribution metadata, environment
classification, runtime-directory assessment, CPU, memory, cgroup, and
frequency/power observations fail independently and remain diagnostics or
unknown/partial facts. P012 adds typed CPU and memory values and capability
records without changing snapshot schema version 1 or the C ABI.

## P012 Linux hardware mapping

P012 maps online host-visible logical processor count and a physical-core
count only when package/die/core topology is complete and internally
consistent. Process-allowed CPU IDs, possible/present/offline CPU sets, CPU-time
quota, and physical topology remain distinct facts. `CpuInfo.observed_frequency`
stays unknown because this one-shot discovery probe does not sample frequency.
Scheduling capability records describe queryable interfaces; they do not claim
permission to change policy. cpufreq and platform-profile interface details
remain in the Linux-specific report, and readable controls do not prove
writability.

RAM and swap are mapped from bounded `/proc/meminfo` data. Kernel kB values use
checked multiplication by 1024; missing `MemAvailable` or `SwapTotal` remain
unknown, while a valid zero `SwapTotal` represents absent configured swap.
Host-visible memory is not replaced by a cgroup limit. Cgroup v2 CPU quota and
memory controls are reported separately from topology and host memory; ancestor
limits are not resolved, and cgroup v1 resource discovery is unsupported or
partial. The Linux adapter assigns fresh snapshot-local CPU instance and
provenance IDs when merging P011 and P012, then runs normal P007 snapshot
validation and ordering.

Future macOS and FreeBSD adapters can implement the same contracts. P011 only
implements Linux; identifying a platform does not establish compatibility or
imply that every capability exists on that platform.

## C17 boundary

Domain platform, availability, capability, and unit conversions preserve the
numeric identifiers in `native/optimizer/include/endfield_optimizer.h` and
`docs/ABI.md`. Capability instance identity is snapshot-local and maps to the
ABI's `instance_id`; `None` maps to zero. A capability's provenance reference
resolves to the snapshot provenance catalog. No provenance string or snapshot
marshalling call is sent to C by P007. The existing C17 ABI, safe FFI wrapper,
and successful empty plan-set behavior are unchanged.

## Synthetic test fixtures

The integration-test support module contains three in-memory fixtures:

1. A Linux desktop example with an Intel Core i7-12700F, NVIDIA RTX 5070,
   32 GB RAM, one example display, and partial runtime/session observations.
2. A Linux laptop example with an Intel Core i5-1135G7, Intel Iris Xe, NVIDIA
   MX350, 16 GB RAM, multiple GPUs, and incomplete thermal/power observations.
3. An incomplete unknown system with no usable GPU or display inventory,
   unknown memory/sensor information, and recoverable diagnostics.

These values are synthetic contract data, not measured or verified user
hardware. The incomplete laptop power data says nothing about the reported
power shutdown, and the fixtures are never used as production defaults or shown
by the application.

## Later service boundaries

P007 does not implement `RuntimeProvider`, `HardwareTelemetryProvider`,
`SessionSupervisor`, `OptimizationPlanner`, `OptimizationExecutor`,
`BenchmarkProvider`, or `ServiceManager`. Later milestones will define each
specialized API when its behavior is scheduled. P042 will define complete
snapshot-to-C marshalling; P015 will refine telemetry units, timestamps, and
missing-data behavior. The domain crate provides the shared foundation without
preempting those contracts.
## P013 Linux GPU facts

The portable P007 schema remains v1 and gains no Linux or Vulkan dependencies. P013 maps validated PCI GPUs to existing `GpuInfo` fields and capability IDs 10–14, allocates snapshot-local IDs after the P012 CPU, and validates typed values against scalar capability records. PCI/DRM/Vulkan-specific details and unmatched devices stay in `LinuxGpuReport`; unknown VRAM, classification, driver, or Vulkan mappings are preserved.

## P014 Linux display facts

The portable P007 schema remains v1 and gains no Linux display-protocol dependencies. P014 reuses `DisplayInfo`, `Resolution`, `Hertz`, `Availability`, `InstanceId`, `CapabilityRecord`, `Provenance`, `Diagnostic`, and `SystemSnapshot`. Capability IDs 40 (display) and 41 (compositor/session) retain their existing meanings. Display instances use IDs from the reserved 1,000,001 range and do not collide with CPU or GPU instances. Fractional refresh rates stay in the Linux-specific `LinuxDisplayReport`; P007 `Hertz` is populated only for an exact whole-Hz current mode. Session hints, successful protocol connections, compositor identity, current output state, connector state, and mode enumeration remain independent observations. No FPS, frame pacing, or display-to-DRM mapping is added to the portable snapshot.
