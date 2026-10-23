# Product definition

## Mission

Build `endfield-unix`, a standalone launcher and goal-driven optimization application for Arknights: Endfield on Unix-like systems. Make setup, launch, diagnosis, and reversible tuning approachable for nontechnical players while retaining clear, advanced controls for experienced Linux users.

The product is intended to provide:

1. A native GTK4 desktop experience and one-click game launch.
2. Wine/Proton runtime setup, game installation or import, update, verification, repair, and prefix recovery workflows.
3. Hardware-aware performance guidance, including low-end GPU and laptop use cases.
4. Detection of platform, kernel, driver, Vulkan, display, and available optimization capabilities.
5. A portable C17 goal-driven planner, with Rust responsible for system data collection, plan validation, approval, and execution.
6. Session, FPS/frametime, thermal, power, and hardware observations, followed by benchmark-informed recommendations.
7. Understandable candidate plans, explicit trade-offs, and safe rollback to known-good state.
8. Interfaces that can support later macOS and BSD work without claiming compatibility before validation.

## P001 scope

P001 establishes product, architecture, security, testing, naming, runtime, and roadmap contracts. It does not implement a Rust workspace, GTK application, command-line behavior, daemon, C optimizer, game integration, telemetry, installer, or platform adapter. Those features must not be described as available until implemented and validated.

## Product behavior contracts

### First-run setup and launch

The intended guided flow is:

1. Detect the platform and enumerate available capabilities.
2. Diagnose graphics drivers and Vulkan devices where supported.
3. Detect installed Wine/Proton runners and compatibility helpers.
4. Select a compatible runtime using known compatibility evidence and any user or machine pin.
5. Download only from a verified, trusted source; verify integrity before installation.
6. Install the runner into user-managed storage and initialize a managed Wine prefix.
7. Import an existing game installation or use a verified, official installation source. Do not invent distribution endpoints.
8. Validate dependencies and offer actionable diagnostics.
9. Present a one-click Play action.

GE-Proton and DWProton are runtime providers to support where compatible. UMU integration should be used where appropriate. Runtime installation must not require installing a separate system Wine package merely because Proton contains Wine. The application must not interfere with anti-cheat or game binary integrity.

### Optimization and benchmarks

The C17 planner accepts normalized capabilities and user goals and proposes multiple machine-readable candidate plans. Each plan must expose supported actions, constraints, a rank, evidence, expected benefits, and trade-offs in terms that the UI can explain. Rust rechecks the live capabilities, rejects stale or unsafe plans, asks the user for approval, and then applies actions through platform adapters. Applying a plan may be followed by a benchmark and an optional rollback.

The planner must not invent FPS or frametime improvements. Quantitative recommendations require attributable measurements for the relevant machine, runtime, game settings, and test conditions. Otherwise, present qualitative trade-offs or mark the estimate unknown.

Optimization intensity is defined as:

| Level | Name | Contract |
| --- | --- | --- |
| L0 | Observation only | Collect and report data; propose no state-changing action. |
| L1 | Conservative | Propose low-risk, reversible, narrowly scoped actions. |
| L2 | Balanced | Consider supported reversible actions with explicit trade-offs. |
| L3 | Aggressive but reversible | Consider broader reversible changes and require clear impact and rollback details. |
| L4 | Experimental | Keep opt-in and require explicit approval for each experimental plan; explain uncertainty and recovery steps. |

User approval is required before any plan is applied. L4 requires an additional explicit acknowledgement of its experimental status. No level authorizes bypassing anti-cheat, altering game binaries, or silently changing privileged system state.

### Runtime pinning and recovery

Keep a per-machine, per-game known-good runtime pin, including provider, exact version/revision, integrity identity when available, and the associated profile or prefix. A newer runner must not replace a known-good runner automatically. Runtime changes are staged, validated, and reversible; retain the previous usable runner and prefix backup until the user chooses to retire them.

Known hardware observations are evidence for future validation, not compatibility guarantees:

- Desktop baseline: Intel Core i7-12700F, NVIDIA RTX 5070 12 GB, 32 GB RAM, Arch Linux, and Hyprland. GE-Proton 10-26 is reported to work well with Endfield and DLSS. A newer GE-Proton version was reported to have GPU detection problems, unavailable DLSS, and severe lag. The newer version and root cause are unverified; do not state a cause as fact or upgrade this machine's pin automatically.
- Laptop baseline: Dell Vostro 3510, Intel Core i5-1135G7, Intel Iris Xe plus NVIDIA MX350, and 16 GB RAM on Linux. Prioritize stable frametimes, low-end GPU behavior, thermals, and power. Unexpected shutdowns under dGPU gaming load were reported. Software tuning must not be represented as a fix for a hardware power-delivery problem.

## Scope and non-goals

The initial product is Linux-first through P060. macOS work in P061–P070 and FreeBSD work in P071–P080 is experimental. P081–P090 is for cross-platform stabilization. A platform or game is supported only after its relevant release gates and real validation pass.

The product does not patch game binaries, bypass or disable anti-cheat, promise compatibility based on OS detection alone, perform unapproved privileged changes, invent game download sources, silently upgrade known-good runtimes, irreversibly delete prefixes or user data, collect unnecessary credentials, or upload telemetry without explicit user consent.
