# ADR 0003: Verified runtimes, known-good pins, and rollback

- **Status:** Accepted as the P001 architecture baseline
- **Date:** 2026-10-23

## Context

Wine/Proton setup, game launch, and optimization must not put an existing working installation at risk. A newer GE-Proton build has been reported to regress GPU detection, DLSS availability, and performance on a known desktop, but its version and root cause have not been verified. Runtime artifacts and game sources must be trusted and verifiable.

## Decision

Manage compatible runners in user-owned storage. Support GE-Proton and DWProton where compatible and integrate UMU where appropriate. Verify source and integrity before use. Pin known-good runner identity per machine/game/profile and never upgrade such a pin automatically. Stage version changes and retain the previous runner and prefix state until the user elects to remove them. Back up prefixes before risky operations and provide restore/rollback workflows.

Import existing game files or install only from a verified official source; do not invent distribution endpoints. Preserve anti-cheat and binary integrity. Proton's bundled Wine does not by itself require installing a separate system Wine package. Root is not required by default.

## Consequences

Runtime and prefix storage requires explicit versioned metadata and lifecycle management. Disk use may temporarily increase to preserve rollback state. Compatibility reports must identify the tested runner and system; the reported newer-runner regression remains unverified and must not be generalized.
