# Dependency diagnostics (P033, work in progress)

P033 is incomplete and is not qualified for acceptance. The experimental
headless command is `endfield-unix diagnose dependencies [--profile ID]
[--game-id ID] [--prefix-id ID] [--source-id steam|epic|gryphlink] [--json]`.
It composes the existing P030 compatibility collector with P012 hardware facts,
P031's read-only selected-location revalidation, and P032's offline source
catalog. It does not select a runtime or GPU, launch processes, install software,
or persist a report. Game compatibility remains `not_tested`.

The portable requirement catalog identifies official Steam and Epic Windows
listing evidence observed 2026-11-03. Steam's 60 GB storage and Epic's 50 GB plus
40 GB unpacking guidance are separate, and GRYPHLINK has no established
requirements. CPU and GPU model equivalence is not inferred. DirectX does not
prove Vulkan or translation-layer support. The stores do not specify the GB
multiplier; comparisons only classify a value when both decimal and binary
interpretations agree. Swap is not physical RAM, available RAM is separately
reported, and ancestor cgroup limits can remain unknown. No target filesystem is
currently selected by the CLI, so space remains unknown. The core has a separate
space evaluator for a supplied target, but no CLI adapter yet.

JSON v1 is described by [the preliminary schema](schema/dependency-diagnostics-v1.schema.json)
and embeds the existing P030 report. Reports are generated on demand, have output
limits, omit registered paths and raw machine IDs, and do not authenticate game
identity. An explicit game ID is revalidated using P031; the user declaration
and game payload remain unverified. A source label is not installation evidence.
The source catalog's guidance is an official handoff, not a download capability.

Outstanding work includes richer independent requirement findings and typed
error observations, explicit filesystem selection and storage observation,
source-specific local Steam evidence, comprehensive platform and cgroup states,
a complete anti-cheat evidence assessment, full recommendation and schema contracts, adversarial
no-mutation tests, and full application and final-head CI qualification. Neither
physical target machine has been tested. Static observations do not establish a
playable Linux configuration, and P034 is not ready on the basis of this work.
