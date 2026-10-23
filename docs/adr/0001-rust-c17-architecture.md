# ADR 0001: Rust application core and C17 optimization planner

- **Status:** Accepted as the P001 architecture baseline
- **Date:** 2026-10-23

## Context

The product needs maintainable application, GUI, runtime, platform, storage, IPC, and recovery code, alongside a portable goal-driven optimizer whose decision engine is explicitly implemented in C17. The planner must remain independent of GUI, Linux interfaces, Wine, and privileged operations.

## Decision

Use Rust 2024 for the application core, single GUI/CLI executable, GTK4/libadwaita/Relm4 frontend, daemon, platform adapters, storage, telemetry ingestion, and plan validation/execution. Use C17 for capability evaluation, goal interpretation, constraint checking, candidate generation, intensity handling, ranking, trade-off analysis, and benchmark-informed recommendations.

Rust gathers and normalizes system snapshots and passes them through a documented C ABI. The C library returns multiple proposed plans and evidence/trade-off data. Rust validates the current system state, obtains user approval, then uses platform adapters to execute supported actions. The C engine has no I/O or side effects.

The ABI uses fixed-width values, explicit numeric identifiers, versioned size-tagged structs, borrowed inputs, and opaque C-owned plan handles with C destroy functions. Errors are returned as numeric status values; no panic or exception crosses the boundary. ABI major/minor compatibility and identifier stability are defined in `../ARCHITECTURE.md`.

## Consequences

The C boundary is explicit and auditable, and the optimizer can be tested without GTK or Linux. The Rust wrapper must carefully validate memory layouts and lifetimes. Plan execution and approval remain in Rust and cannot be delegated to the planner. No complete optimization algorithm is part of P001.
