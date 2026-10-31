# PROMPT030 implementation and qualification manifest

## Baseline and branch

- Verified remote `main`: `43171ac19bc3e4bc51dc25d3e87f730c1ab17778`.
- P028 PR [#1](https://github.com/nghianguyen150612/Endfield-Linux/pull/1) and P029 PR [#2](https://github.com/nghianguyen150612/Endfield-Linux/pull/2) are merged.
- P030 branch: `feat/p030-compatibility-diagnostics`.
- Draft PR: [#3](https://github.com/nghianguyen150612/Endfield-Linux/pull/3). The implementation source head qualified by hosted CI is `861058f889a5e7438b1dbf7e42981e1d35e8e0dd`. This follow-up manifest update is documentation-only; the resulting PR head is checked separately.
- Merge status: intentionally unmerged. P031 has not started.

## Implementation

P030 adds the portable `endfield-unix-compatibility-diagnostics` assessment crate, the existing `endfield-unix diagnose compatibility` CLI route, a versioned JSON schema, and a metadata-only P029 recovery observation. P013 retains its ash-based Vulkan provider and now reports bounded device-extension names, core Vulkan 1.0 feature observations, source-provided Vulkan driver identity, and DRM-to-PCI BDF evidence.

Runtime selection uses the exact P027 profile/machine resolution and existing P022/P025/P026 revalidation. A stale profile reference blocks fallback. P028 inspects only one explicitly selected registered prefix. P029 restore, rollback, snapshot-deletion, and P027 pin-recovery markers remain read-only; the observer does not inspect prefix or snapshot payload contents. Findings are deterministic and bounded. Translation-layer readiness is separately reported as unknown, and game compatibility is always `not_tested`.

The actual-binary integration suite is [compatibility_cli.rs](../../apps/endfield-unix/tests/compatibility_cli.rs). It covers an empty isolated XDG host, JSON and human output, help and parser errors, no XDG creation, stale profile pin precedence, missing prefixes, and a Created/uninitialized managed prefix. The CI workflow runs this integration suite with the real application binary.

## Local qualification

Final local results for the current implementation:

- `cargo +1.85.0 fmt --all -- --check`: passed.
- `cargo +1.85.0 test --locked --workspace --exclude endfield-unix`: passed, including P021–P029 library and daemon/IPC regressions.
- `cargo +1.85.0 clippy --locked --workspace --exclude endfield-unix --all-targets -- -D warnings`: passed.
- `cargo +1.85.0 build --locked --workspace --exclude endfield-unix`: passed.
- `cargo +1.85.0 test -p endfield-unix-compatibility-diagnostics --locked`: 13 passed.
- `cargo +1.85.0 test -p endfield-unix-prefix-management --locked --lib`: 64 passed; the metadata-only compatibility observer's four focused tests passed, including pending deletion and pin recovery.
- A temporary native-free harness compiled the CLI adapter and actual-binary integration test source, and warning-denied Clippy passed for that source. The harness has a stub executable; it is not used as actual-binary smoke evidence.
- A populated report serialized by the portable Rust crate validated against `docs/schema/compatibility-diagnostics-v1.schema.json` with JSON Schema Draft 2020-12 validation.
- `cargo +1.85.0 metadata --locked --no-deps --format-version 1` and `git diff --check`: passed.
- `CC=gcc native/optimizer/tests/run-c-tests.sh` and `CC=gcc native/optimizer/tests/run-sanitizers.sh`: passed.

The full `cargo +1.85.0 check --locked --workspace`, `clippy --locked --workspace --all-targets -- -D warnings`, `test --locked --workspace`, and `build --locked --workspace` commands stop in `glib-sys`/`gobject-sys` before compiling the GTK application because this environment lacks `glib-2.0.pc` and `gobject-2.0.pc`; GTK4 and libadwaita development files are also absent. No fake pkg-config or native-library installation was introduced. Clang is not installed. Therefore local actual-binary integration and GTK screenshot smoke are unverified.

## Hosted and physical qualification

- Hosted CI for implementation head `861058f889a5e7438b1dbf7e42981e1d35e8e0dd` passed in [Rust CI run 41206762591](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41206762591). The run qualified workspace check, Clippy, build, tests, actual P030 CLI integration, P021–P029 regression gates, GTK smoke, metadata, and GCC/Clang C17 ABI jobs.
- Actual-binary compatibility CLI suite and GTK screenshot smoke: passed on the hosted runner with genuine GTK dependencies.
- GCC and Clang C17 ABI hosted gates: passed. Clang is unavailable locally.
- The current PR head includes only this qualification-manifest update after the qualified implementation commit. Check the latest PR run for final-head status.
- RTX 5070 desktop and Dell Vostro 3510 physical observations: unverified. Optional read-only procedures are documented in [COMPATIBILITY-DIAGNOSTICS.md](../COMPATIBILITY-DIAGNOSTICS.md).
- Wine/Proton execution, prefix initialization, DXVK/VKD3D-Proton requirements, Endfield launch, rendering, and performance: not tested and not claimed.
