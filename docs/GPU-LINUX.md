# Linux GPU discovery (P013)

P013 adds bounded, read-only GPU inventory to the existing `endfield-unix-platform-linux` adapter. `endfield-unix diagnose gpu` is headless and does not initialize GTK, contact `endfield-unixd`, modify configuration, or use a helper process.

## Separate observations

P013 reports PCI display hardware, kernel driver binding, DRM primary/render nodes, Vulkan loader and physical-device observations, and evidence-backed cross-provider mappings separately. It does not verify game compatibility or successful rendering. No game, graphical window, Vulkan surface, logical device, queue, workload, or benchmark is started.

## Sources and bounds

PCI candidates come from `/sys/bus/pci/devices/`. Only class `0x03` display controllers with VGA (`00`), 3D (`02`), or other display (`80`) subclasses enter the GPU inventory. Strict BDF values preserve domain, bus, device, and function. Enumeration is capped at 512 entries. Known vendor IDs are Intel (`8086`), NVIDIA (`10de`), and AMD (`1002`); unknown IDs remain numeric. No marketing-name table is bundled. A model is shown only when a uniquely correlated Vulkan device reports its physical-device name.

DRM discovery uses `/sys/class/drm/` and `/dev/dri/`. Only primary `cardN` and render `renderDN` nodes are considered; connector and control nodes are ignored. Sysfs `device` relationships are canonicalized and accepted only under `/sys/devices`. BDF, not node number, associates a node with PCI hardware. Device nodes are checked for character-device type and read access without opening them. Existence, denied access, missing `/dev` nodes, and unknown access remain separate. Enumeration is capped at 128 nodes. DRM without validated PCI identity remains unassociated.

The bound kernel driver is read from the PCI sysfs `driver` link. No version is inferred from the kernel release or Vulkan driver-version encoding. A missing link means no bound driver was observed; inaccessible or malformed data remains unknown and does not erase PCI hardware.

Vulkan is queried through ash `0.38.0`, dynamically loading the system loader. The probe does not depend on `vulkaninfo`, infer availability from ICD manifests, or create a surface/device/workload. Up to 64 physical devices and 16 heaps per device are retained. Loader load failure, missing loader entry points, initialization failure, zero devices, and successful enumeration are separate states. When advertised, `VK_KHR_driver_properties` contributes its raw driver ID and bounded driver name; these remain separate from kernel driver names and Vulkan API versions. Vendor ICD code runs in-process and may block; bounded results do not mean a blocked driver call can be forcibly cancelled.

Vulkan/PCI correlation uses `VK_EXT_pci_bus_info` only when the physical device advertises it and the Vulkan properties query returns a valid BDF. A Vulkan device without that extension or with an invalid/unavailable BDF remains unmatched; vendor/device IDs are checked for consistency after a BDF match but never used as physical identity. Duplicate BDF observations are ambiguous, and conflicting IDs remain unmatched with a diagnostic. PCI BDF maps DRM nodes. Enumeration order, node index, vendor/model IDs, `boot_vga`, and user expectations are never identity evidence.

A successful Vulkan enumeration with zero physical devices marks each PCI GPU's Vulkan-device availability `Unavailable`. Missing-loader, initialization-failure, degraded, and uncorrelated observations remain `Unknown`; a missing loader does not establish absent GPU support.

## Snapshot and memory semantics

P013 reuses P007 `GpuInfo` and capability IDs 10–14. GPU instance IDs are deterministic within normalized input and start at 2; P012 CPU retains ID 1. P014 can use the reserved display range starting at 1,000,000. Provenance IDs 10–12 identify PCI, DRM, and Vulkan facts after existing P011/P012 IDs. Typed fields and capability records are validated together. GPU probe failures preserve P012 CPU and memory observations.

Integrated/discrete classification comes only from Vulkan physical-device type after an exact `VK_EXT_pci_bus_info` BDF correlation. Other types and uncorrelated PCI GPUs remain unknown. Vulkan CPU/software implementations are counted and labeled separately; they are not dedicated graphics hardware.

Vulkan device-local heap sizes retain their Vulkan semantics in the Linux-specific report. They are not mapped to dedicated VRAM. Normalized dedicated VRAM remains unknown because P013 has no source proving dedicated capacity. Hardware acceleration is available only when an exactly correlated integrated/discrete Vulkan physical device is enumerated. This does not establish application readiness or game rendering.

Missing optional sources, permission failures, malformed records, and loader failures produce diagnostics and partial results when Linux platform identity remains available. Unknown is not treated as hardware absence.

## Security and limits

The adapter uses fixed source-family APIs, strict identifiers, bounded reads/enumeration, and sysfs link containment. It does not run commands, require root, load modules, configure PRIME, modify power, open DRM nodes, perform DRM master operations, or change system state. `ash` unsafe FFI is isolated in `vulkan.rs`; optional PCI properties are queried only after checking the corresponding extension, and the Vulkan instance is destroyed after every successful creation. The portable domain and C17 ABI gain no Linux or Vulkan dependency.

Synthetic tests are not measurements of a user's system. Hosted Ubuntu smoke coverage accepts no physical GPU and does not qualify target Arch hardware.

## Hosted CI validation

The hosted workflow passed for commit `438b58aa14fc74ce09b09cc4de758dddfeacb744` in [Rust CI run 40170150212](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40170150212), including the real headless CLI, 92 platform-adapter tests, full workspace tests, IPC/daemon gates, GTK smoke/screenshot, metadata, and GCC/Clang C17 jobs. The Ubuntu 24.04.5 runner reported zero PCI graphics-class devices, one DRM node without a validated PCI relationship, and Vulkan instance initialization failure; it returned a valid partial report with unknown Vulkan device counts. This is a CI environment observation, not target hardware validation.

## Manual Arch Linux comparison

```sh
cargo +1.85.0 run --locked -p endfield-unix -- diagnose gpu
lspci -nnk | grep -A4 -Ei 'VGA|3D|Display'
ls -l /dev/dri/
find /sys/class/drm -maxdepth 2 -name device -print
vulkaninfo --summary # only when already installed
```

The reference desktop check must verify NVIDIA RTX 5070 identification, real driver binding, DRM mapping, Vulkan identity, and avoid VRAM assumptions. The Dell Vostro 3510 check must independently verify Intel Iris Xe and NVIDIA MX350 and their mappings. No active game GPU or solved power-delivery issue is implied. Both physical-machine checks remain **UNVERIFIED** until run on those machines.

P014 may add display/compositor observations. P017 owns dynamic GPU utilization, thermal, power, and memory usage. Optimizer work must not interpret Vulkan enumeration as benchmark, compatibility, or default GPU-selection evidence.
