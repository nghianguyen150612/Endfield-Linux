# ADR 0002: Capability-oriented cross-platform boundaries

- **Status:** Accepted as the P001 architecture baseline
- **Date:** 2026-10-23

## Context

The implementation order is Linux-first through P060, experimental macOS in P061–P070, experimental FreeBSD in P071–P080, then cross-platform stabilization in P081–P090. Platform services, hardware telemetry, runtime behavior, and game compatibility differ and cannot be inferred from an OS label alone.

## Decision

Keep portable domain models and application workflows separate from platform-specific adapters. Define `PlatformProbe`, `RuntimeProvider`, `HardwareTelemetryProvider`, `SessionSupervisor`, `OptimizationPlanner`, `OptimizationExecutor`, `BenchmarkProvider`, and `ServiceManager` as responsibility boundaries; establish concrete APIs as implementation needs arise. Adapters report supported, unavailable, and unknown capabilities with provenance. Select behavior from reported capabilities, not OS identity alone.

The Linux adapter is first. macOS and FreeBSD support is experimental until separately validated on real systems. No game compatibility claim follows from compiling, probing, or launching the application on a platform.

## Consequences

Linux-specific mechanisms stay out of the portable optimizer and domain core. Platform support can be described precisely by capability and evidence. Adapter contracts and release gates must be tested independently for each supported platform.
