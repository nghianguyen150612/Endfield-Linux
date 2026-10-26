# Linux GPU telemetry (P017)

`endfield-unix diagnose telemetry gpu [--interval-ms 250]` performs one read-only,
headless collection. It needs neither the daemon nor a display connection. The
interval accepts P016's 100–2000 ms policy; current GPU gauges do not sleep.
There is no background sampling, persistence, support-report inclusion, network
upload, rendering, GPU configuration, module loading, or external command source.

## Identity and batches

`GpuIdentityContext::discover` reuses P013's PCI parser, graphics-class filter,
validated driver binding, sorted canonical PCI BDF ordering, and snapshot-local
ID allocator. It uses a P011 platform snapshot, without P012 CPU collection,
Vulkan enumeration, or P014 display probing. The caller owns `SnapshotRefId`.
`from_report` accepts an existing P013 report only after checking snapshot
validation, sorted unique BDFs, expected instance IDs, vendor and driver agreement.
The report remains a trusted inventory contract; it is not a signed hardware
attestation. No matching uses marketing name, DRM card number, or NVML index.

`LinuxGpuTelemetryProvider` accepts explicit P015 requests and returns validated
schema-v1 batches. Six existing GPU metrics are declared, with independent
missing-data states. Engine utilization is deferred. Batches retain exact integer
values, monotonic timestamps, fixed source provider IDs, normalized origin, and
complete/partial quality. Gauge timestamps bracket read completion, not a claim
about hardware sensor freshness. Unsupported utilization has no invented window.
A missing identity context produces an empty partial batch and a typed issue;
no arbitrary GPU target is assigned. No unassociated raw values are retained.
No GPU means an empty partial batch. The maximum device count is bounded by the
4096-observation batch budget and the PCI enumeration limit.

## Sources and semantics

- AMDGPU: only AMD PCI devices actually bound to `amdgpu` use
  `mem_info_vram_used` and `mem_info_vram_total`, in bytes. Zero usage is valid;
  zero total and usage greater than total are rejected. These counters describe
  the driver's VRAM pool, not GTT allocations, free memory, Vulkan heaps, or
  model-derived capacity. On APUs, firmware-reserved system memory can back this
  pool; physical separate VRAM cannot be established from these counters alone.
  Therefore normalized AMD dedicated capacity requires an existing correlated
  P013 report with a verified discrete classification. The fresh PCI-only path
  leaves AMD dedicated capacity Unsupported, while retaining driver VRAM-pool
  usage. It never initializes Vulkan to obtain that classification.
- AMD hwmon: scan at most 32 directories under the verified PCI device; require
  containment and the `amdgpu` hwmon name. The documented `temp1_input` edge
  sensor is selected, not hotspot or memory junction; signed milli-Celsius is
  preserved. `power1_input` is GPU power consumption in microwatts, converted
  by integer division by 1000, rounding down by less than 1 mW. It is not a
  total-system reading or a guaranteed total-board rail sum. Power caps,
  maximums, and TDP are never substituted. Multiple matching hwmon devices are
  ambiguous; they are not averaged or silently selected. AMD frequency is
  unsupported rather than choosing among clock domains.
- Intel `i915`: read `gt_act_freq_mhz` only through a contained, unique primary
  DRM node of the PCI device. Checked multiplication converts MHz to Hz.
  `xe` sources, multi-GT clocks, Intel hwmon package/rail power, and temperature
  are deferred where a reliable GPU scope/selection rule is unavailable.
  Intel shared RAM, apertures, and Vulkan heaps are never dedicated VRAM.
- NVIDIA: optional NVML through `libloading` 0.8.9 (MSRV compatible with 1.85).
  The system loader resolves `libnvidia-ml.so.1`; there is no linked NVIDIA
  dependency and no vendor path override. Required v2 initialization/PCI-address
  physical-handle and v3 PCI symbols are verified before initialization.
  The allowed PCI lookup list is capped at 128; the full returned PCI domain, bus,
  device and function determine identity. No enumeration indices are used; matching
  uses full BDF. MIG instances and UUIDs are not enumerated or retained.
  Memory v1 reports used/total framebuffer bytes, temperature reports integer
  Celsius converted exactly to milli-Celsius, power reports NVML milliwatts,
  and graphics clock reports MHz converted with checked arithmetic. NVML power
  may be instantaneous or driver-averaged depending on hardware; no software
  sampling interval is asserted. Optional symbols and each metric fail
  independently. Initialization failure, missing library, unsupported function,
  permission denial and generic native failures become fixed typed categories.
  Successful initialization is paired with shutdown before unloading.

The common source boundary accepts fixed attribute enums and validated BDFs,
never caller-specified paths. File reads cap actual bytes at 128 and reject
symlink files, nonregular attributes, and unsafe canonical device relationships.
The PCI path must end in the same validated BDF. No sensor error becomes zero.
A successful driver binding is necessary evidence for vendor selection, not
proof of monitoring availability or hardware health.

## Utilization and power safety

AMDGPU `gpu_busy_percent` and NVML utilization describe vendor-internal sampling
periods. P017 does **not** normalize them as exact CLI-interval utilization.
All device utilization results are currently Unsupported, or degraded if power
state prevents access. Engine/per-client fdinfo data is deferred because P015
has no per-client GPU target. Frequency, CPU busy time, and display refresh never
stand in for GPU busy time. No two-point busy counter backend is shipped yet.

Only devices verified runtime-active are read. Suspended and unknown states
retain identity and produce degraded missing observations. If any discovered
NVIDIA device is suspended or unknown, the entire NVML library is skipped,
including initialization. This is conservative and can suppress otherwise valid
active-device gauges. Runtime status checks are subject to races with the kernel:
read-only NVML calls are **not guaranteed power-neutral**. The library may inspect
other physical devices during initialization. No-wake behavior cannot be
absolutely guaranteed. The application never explicitly activates or resets GPUs.

Native calls and sysfs reads are synchronous. Call counts/enumerations are bounded,
but vendor code can block; there is no strict wall-time cancellation guarantee.
No detached workers or abandoned timeout threads are created. UI callers must use
an owned worker, as for P016. The CLI is one on-demand collection.

## Validation and manual checks

Deterministic rooted sysfs and injected native fixtures exercise synthetic RTX
5070, Iris Xe/MX350 hybrid, AMD, Intel xe, headless, malformed and conflicting
sources. Synthetic NVML is not real NVIDIA hardware validation. Hosted CI adds
`gpu_telemetry_live_smoke` and the headless GPU telemetry CLI, accepting no GPU.
See [TESTING.md](TESTING.md) for actual qualification evidence and limitations.

Safe manual Arch commands (comparison commands are not runtime dependencies):

```sh
cargo +1.85.0 run --locked -p endfield-unix -- diagnose telemetry gpu
lspci -nnk | grep -A4 -Ei 'VGA|3D|Display'
ls /sys/class/drm/
ls /sys/class/hwmon/
```

On the RTX 5070 desktop compare verified PCI identity, observed kernel binding,
NVML availability, dedicated memory, temperature, power and current clock with
already-installed NVIDIA diagnostic facilities if safe. Utilization remains
unsupported by this normalized provider. On the Dell hybrid verify separate
Intel/NVIDIA targets, shared-memory semantics, and suspended NVIDIA skipping.
Missing optional sensors do not imply broken hardware. No benchmark, rendering,
stress workload, or reproduction of the MX350 shutdown is required.

Arch RTX 5070 validation: **UNVERIFIED**

Dell Vostro Iris Xe validation: **UNVERIFIED**

Dell Vostro MX350 validation: **UNVERIFIED**

P018 can consume the P015 contracts independently; no FPS, frametime, session,
IPC streaming, systemd, persistent telemetry, or live GUI graphs are added here.
