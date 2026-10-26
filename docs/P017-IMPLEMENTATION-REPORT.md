# P017 implementation and qualification

1. **Repository / branch:** `nghianguyen150612/Endfield-Linux`, `main`. No
   branches, pull requests, repository rename, force-push, or history rewrite.
2. **Original HEAD:** `d8602505e82bb693eadca26d924517c13e1fec9c` (verified P016).
3. **Files:** see the complete file list below. Existing CPU accounting, daemon,
   IPC, portable domain/telemetry schemas and native C17 source are unchanged.
4. **Dependencies:** added direct `libloading = 0.8.9`, already present in the
   lockfile through ash. Rust 2024 / Rust 1.85.0 retained. No mandatory NVML or
   other native vendor library. Existing pinned libc 0.2.191, ash 0.38.0 and
   nix 0.29.0 remain unchanged.
5. **Architecture:** `LinuxGpuTelemetryProvider` implements P015's explicit
   request/provider/validated batch contract with injectable sensor/native/clock
   boundaries. Identity discovery reuses P013 PCI parsing and ID allocation,
   without Vulkan, Wayland, X11, or a full display probe.
6. **hwmon:** bounded contained PCI-local scan; only documented AMDGPU edge
   temperature and consumption input selected. Multiple eligible hwmon sources
   are ambiguous. Power caps and unrelated CPU/package sensors are excluded.
7. **AMDGPU:** observed AMD vendor and `amdgpu` binding required; driver VRAM
   pool used/total counters validated; used greater than total is malformed;
   zero usage is valid. Edge temperature and GPU power are independent. Frequency
   is deferred. Dedicated capacity requires existing P013 discrete evidence.
8. **Intel:** observed Intel vendor / `i915` binding and contained unique primary
   DRM relationship required for `gt_act_freq_mhz`. `xe`, multi-GT ambiguity,
   per-client utilization, shared-memory capacity, and package sensors are
   explicitly unsupported where no truthful P015 source exists.
9. **NVIDIA:** dynamically loaded NVML with verified required symbols and
   initialization/shutdown lifetime. Physical handle lookup and reverse
   validation use PCI BDF; optional memory, temperature, power and graphics-clock
   functions fail independently. Missing library, initialization/version errors,
   denied access, and unsupported functions produce fixed typed categories.
10. **Identity:** validated canonical PCI domain/bus/device/function, sorted unique
    BDFs, P013 IDs, caller-owned snapshot reference and validated snapshot.
    Existing P013 reports are checked for schema, mapping, vendor, driver,
    classification and instance consistency. Current binding/PCI identifiers are
    rechecked before use.
11. **Multi-GPU:** independent snapshot-local targets; no association by model,
    vendor alone, DRM index, Vulkan order or NVML index. Conflicting native BDF
    handles are rejected; no source averaging or silent overwriting.
12. **Suspension:** only verified active devices get sensor reads. Suspended and
    unknown-power identities remain visible but degraded. NVML initialization
    is skipped if any discovered NVIDIA GPU is not verified active. Kernel races
    and vendor initialization prevent an absolute no-wake guarantee.
13. **Utilization:** normalized device utilization is currently unsupported.
    One-shot AMD/NVML busy percentages have no truthful P015 rate window;
    no interval, busy-time delta or measured zero is invented. No sleeping is
    needed for the implemented gauges. The CLI retains the 100–2000 ms interval
    policy, default 250 ms. Engine and per-client rates are deferred.
14. **Memory:** GPU used bytes preserve driver-specific accounting scope. AMD's
    pool can include reserved system memory on APUs; fresh PCI-only discovery
    therefore does not label its total as dedicated VRAM. Existing correlated
    P013 discrete classification can authorize that capacity. Intel system RAM,
    aperture and Vulkan heap sizes are never dedicated VRAM. NVML framebuffer
    used and total remain separate.
15. **Temperature:** signed milli-Celsius preserved for AMD edge sensors; NVML
    integer Celsius converts exactly using safe-width integer arithmetic.
16. **Power:** AMD microwatts divide by 1000, explicitly rounding down by less
    than 1 mW; NVML milliwatts stay exact. No cap, maximum, TDP or CPU rail is
    substituted for consumption. NVML averaging semantics vary with hardware.
17. **Frequency:** checked MHz × 1,000,000 gives exact Hz; current i915 and NVML
    graphics clocks only. Maximum/advertised clocks and AMD clock-domain choices
    are not substituted.
18. **Provenance / quality:** fixed allowlisted provider identifiers, normalized
    origin, operating-system sampling method, independent observation states and
    typed issues. Complete quality applies to valid gauges; missing/degraded
    sensors yield partial quality. No private metadata or raw native error text.
19. **P015 validation:** schema v1, existing GPU metric IDs, correct GPU targets,
    monotonic clock-domain validation, deterministic ordering, duplicate policy,
    and batch limits remain enforced. No P015, P010 IPC or P003 ABI changes.
20. **Actual headless CLI output:** Ubuntu hosted output is reproduced below.
    Exit 0 is valid for partial/no-GPU results; invalid intervals return 2;
    required platform/provider failures preserve 70/78, and output failures 74.
    No GTK startup, daemon, SQLite/configuration writes or telemetry persistence.
21. **Synthetic tests:** 14 deterministic GPU contracts passed, covering synthetic
    RTX 5070, Iris Xe/MX350 suspension, AMDGPU, Intel xe/shared memory, headless,
    same-vendor/different-domain mappings, conflicts, rebinds, malformed values,
    overflow, unit conversion, source failure categories and sysfs containment.
    Synthetic NVML results are not physical NVIDIA validation.
22. **Hosted live smoke:** passed; no GPU required, empty valid partial batch on
    this runner, no invented utilization or GPU. All 170 Linux adapter tests
    passed, including the new live GPU telemetry smoke.
23. **P011–P016 regressions:** hosted platform/hardware/GPU/display diagnostics,
    Xvfb/RandR, CPU/memory telemetry, portable telemetry contracts and workspace
    tests passed. Local final non-GUI workspace suite also passed.
24. **GTK:** native local Xvfb smoke and screenshot passed; hosted smoke passed.
    [Hosted screenshot artifact](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40316047680/artifacts/12646489359).
25. **Daemon / IPC:** hosted lifecycle and authenticated subprocess gates passed
    on the first hosted attempt. Local serial workspace tests reproduced the
    pre-existing capacity race: `ipc_e2e.rs:144`, `WouldBlock`, 1.61 seconds;
    [failure excerpt retained](validation/p017-local-ipc-failure.txt). No local
    IPC retry and no IPC changes. A separate reliability repair is recommended.
    Local parallel CLI tests also encountered the unchanged asynchronous GUI
    startup stderr comparison race; hosted app CLI tests passed.
26. **Storage / domain / diagnostics:** hosted full workspace and local non-GUI
    regressions passed. No schema, logging privacy or default support-report
    change was introduced.
27. **C17:** GCC and Clang ABI jobs passed in hosted CI. Both compiler ABI and
    sanitizer scripts also passed in the local native Ubuntu container.
28. **Actions:** implementation [run 40316047680](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40316047680)
    passed on its first attempt: Rust/GTK/Linux/IPC job `121548786536`, Clang
    `121548787049`, GCC `121548787129`. No hosted rerun was required for that
    implementation run. Follow-up qualification is linked in the final delivery.
29. **Implementation commit:** `8ea541b17160fd29a729dd55470a242c31869b02`,
    `feat(p017): implement Linux GPU telemetry providers`.
30. **Final origin/main:** qualification follow-up adds this record and preserves
    required-platform CLI error propagation. The final delivery supplies the
    verified local/remote SHA and final workflow conclusion; no history is
    rewritten to embed a commit's own hash into its contents.
31. **Arch RTX 5070 validation: UNVERIFIED.**
32. **Dell Vostro Iris Xe validation: UNVERIFIED.**
33. **Dell Vostro MX350 validation: UNVERIFIED.**
34. **Limitations:** no measured utilization backend, no xe engine/per-client
    adaptation, conservative AMD dedicated-capacity eligibility, no guaranteed
    power-neutral NVML behavior, no force-cancellable native calls or strict
    wall-time bound, and no physical target validation. Optional missing sensors
    do not imply faulty GPUs. Real native NVML behavior remains unverified on
    the requested hardware.
35. **P018 readiness:** reusable validated provider/batch/identity boundaries are
    available. No FPS/frametime, sessions, systemd, streaming IPC, daemon polling,
    live GTK graphs, benchmark or GPU mutation was implemented.

## Hosted GPU CLI output

```text
GPU telemetry: Linux
GPUs: none verified

Sources:
  NVIDIA NVML: not queried
  AMDGPU sysfs/hwmon: not queried
  Intel interfaces: not queried
Utilization: unsupported normalized window; one-shot busy percentages are not interval averages
Probe result: Partial
```

This is an actual hosted no-GPU observation, not synthetic desktop/laptop data.
See [GPU-TELEMETRY-LINUX.md](GPU-TELEMETRY-LINUX.md) for exact interfaces and safe
manual Arch comparison commands.

## Changed files

- `.github/workflows/ci.yml`
- `Cargo.lock`
- `README.md`
- `apps/endfield-unix/src/cli/commands.rs`
- `apps/endfield-unix/src/cli/dispatch.rs`
- `apps/endfield-unix/src/cli/parser.rs`
- `apps/endfield-unix/tests/cli.rs`
- `crates/platform-linux/Cargo.toml`
- `crates/platform-linux/src/gpu_nvidia.rs`
- `crates/platform-linux/src/gpu_probe.rs`
- `crates/platform-linux/src/gpu_sources.rs`
- `crates/platform-linux/src/gpu_telemetry.rs`
- `crates/platform-linux/src/gpu_telemetry_sources.rs`
- `crates/platform-linux/src/lib.rs`
- `crates/platform-linux/tests/gpu_telemetry_contract.rs`
- `crates/platform-linux/tests/gpu_telemetry_live_smoke.rs`
- `docs/ARCHITECTURE.md`
- `docs/CLI.md`
- `docs/GPU-LINUX.md`
- `docs/GPU-TELEMETRY-LINUX.md`
- `docs/P017-IMPLEMENTATION-REPORT.md`
- `docs/ROADMAP.md`
- `docs/SECURITY.md`
- `docs/TELEMETRY-LINUX.md`
- `docs/TELEMETRY.md`
- `docs/TESTING.md`
- `docs/validation/p017-local-ipc-failure.txt`
