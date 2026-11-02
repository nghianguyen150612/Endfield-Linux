# PROMPT031 implementation and qualification manifest

## Baseline and branch

- Verified starting `main`: `6f383c6390462cd4da64a97d372fee739b20fd2c`.
- P030 PR [#3](https://github.com/nghianguyen150612/Endfield-Linux/pull/3) is merged at that exact commit. Its final-head hosted run [41207736864](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41207736864) passed.
- Feature branch: `feat/p031-existing-game-import`.
- P031 changes are limited to explicit existing-location import and management. P032 has not started.

## Implementation

P031 adds the GTK-independent `endfield-unix-game-import` crate, Linux
descriptor-relative filesystem observation and private application-owned
versioned registry, and the canonical headless `endfield-unix game` CLI family.
The import requires an explicit user declaration and exact plan approval.
Registration stores root identity and bounded top-level entry counts only.
No authoritative Endfield layout or marker exists in the repository evidence;
identity remains user-declared and unverified, with build, publisher, and
compatibility evidence absent.

The actual-executable suite is
[`game_import_cli.rs`](../../apps/endfield-unix/tests/game_import_cli.rs). It
covers empty XDG state, help, planning, approval refusal, registration,
listing, inspection, revalidation, stale revision refusal, unregister, and
preservation of game bytes/metadata and synthetic P027/P028/P029 state. An
inert synthetic executable has a sentinel side effect if run; the test confirms
that the sentinel is never created.

## Local qualification

- `cargo +1.85.0 fmt --all -- --check`: passed.
- `cargo +1.85.0 test --locked -p endfield-unix-game-import --all-targets -- --test-threads=1`: passed, 31 tests.
- `cargo +1.85.0 clippy --locked -p endfield-unix-game-import --all-targets -- -D warnings`: passed.
- Serialized non-GTK workspace regression, `cargo +1.85.0 test --locked --workspace --exclude endfield-unix -- --test-threads=1`: passed on final rerun with 723 passed, 0 failed, and 4 ignored. P006 storage and P021–P030 library/daemon regressions were included. The first attempt had one P009 daemon lifecycle test failure; the isolated lifecycle suite and two complete serialized reruns passed.
- Matching non-GTK workspace Clippy with `-D warnings` and build, both excluding the GTK `endfield-unix` package: passed.
- A temporary GTK-free harness using the workspace's Clap 4.5.53 type-checked `cli/game_import.rs` and compiled the actual `game_import_cli.rs` integration-test source with `--no-run`. It does not run a real `endfield-unix` binary.
- `cargo +1.85.0 metadata --locked --no-deps --format-version 1`: passed; metadata reports exactly two application binaries, `endfield-unix` and `endfield-unixd`.
- `CC=gcc native/optimizer/tests/run-c-tests.sh` and `CC=gcc native/optimizer/tests/run-sanitizers.sh`: passed; no C17 ABI files changed. Clang is not installed locally.
- Full workspace check, Clippy, test, and build commands stop in `glib-sys` before compiling the GTK app because this environment has no `glib-2.0.pc`; GTK4/libadwaita development libraries and a local GTK smoke run are unavailable. No fake pkg-config metadata was used.
- Actual P031 executable integration and GTK smoke are therefore pending hosted CI. P031 fixtures are synthetic; no physical game installation or device was used.

## Hosted and physical qualification

Record exact final-head GitHub Actions run IDs and outcomes here after the
Draft PR is created. The final hosted run must qualify GTK smoke and actual
executable integration with genuine native libraries. Physical Endfield
import, Wine/Proton/UMU execution, game launch, rendering, and compatibility
remain unverified.
