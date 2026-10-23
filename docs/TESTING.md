# Testing and release gates

## P001 baseline

P001 added documentation only. At that baseline there was no Rust or C implementation, test suite, or application build/lint command. Rust/C tests, GUI checks, runtime checks, and platform validation were **not yet applicable**, not passed. P002 preserves the P001 requirements and adds the initial Rust checks below; it does not implement the future GUI, C optimizer, daemon, runtime, or platform checks.

P001 validation is limited to repository/document checks: file and link inspection; naming and required-policy search; roadmap sequence and range checks; and cross-checking that architecture, FFI, runtime, security, and product contracts agree. The task completion report records the actual commands and outcomes.

### P001 validation record — 2026-10-23

- `git diff --cached --check` — passed with no whitespace errors.
- Inline Python documentation contract check — passed: all 11 expected files were present, all relative Markdown links resolved, P001–P090 appeared exactly once and in order, required product/framework/platform/goal terms were present, and the ABI, naming, and security requirements were represented.
- Naming review — passed: application names are `endfield-unix` and `endfield-unixd`; the disallowed aliases appear only in the naming rule that prohibits them, and the temporary GitHub repository identity is retained only as repository metadata.
- Rust/C builds, tests, and lints — not run; not applicable because P001 contains no Rust/C implementation or build manifests.

## Required checks as implementation arrives

### Rust and C core

- Build with the Rust 2024 toolchain and supported GTK4/libadwaita/Relm4 development dependencies.
- Run Rust formatting, linting, unit, integration, and documentation tests available in the workspace.
- Compile the planner as C17 with strict warnings on each target compiler; run C unit tests and memory/undefined-behavior instrumentation where available.
- Exercise ABI version negotiation, malformed structures, unknown identifiers, boundary values, pointer/length validation, plan-handle lifetime, destruction, and error propagation.
- Check Rust/C behavior with sanitizers and dynamic analysis where toolchains support them; verify no allocator ownership crosses the ABI.
- Test that candidate generation is deterministic for identical snapshots, goals, and engine versions, and that L0 produces no state-changing action.

### GTK, CLI, daemon, and platform adapters

- Verify GTK access stays on the GUI main thread and slow operations do not block it; test cancellation, progress, worker failure, and window closure.
- Test GUI and CLI use the same executable and application services.
- Test daemon authentication, message bounds, version mismatches, malformed input, socket permissions, reconnects, graceful shutdown, and game survival after GUI exit.
- Test Linux capability detection against missing tools, permissions, unsupported devices, and partial sensor data. Test capability-driven behavior instead of relying only on OS-name branches.
- Test installer source verification, interrupted downloads, rollback, pin preservation, prefix backup/restore, and user-data retention without contacting invented endpoints.
- Test benchmark provenance and ensure unsupported numeric performance predictions are omitted.

### Release gates

- A platform is supported only after real-system build, install, update, launch, session, telemetry, and recovery validation for the declared capabilities.
- A game/runtime combination is listed as compatible only after reproducible testing with exact game, runner, driver, prefix, and relevant system details.
- macOS and FreeBSD remain experimental until their own gates pass; passing Linux tests does not validate them.
- Security and privacy release review must confirm least privilege, authenticated IPC, integrity verification, consent behavior, and non-destructive recovery.
- Record the commands, environment, results, and limitations for each release. Do not describe unrun checks as passing.

## P002 validation record — 2026-10-23

- `rustc --version` — `rustc 1.85.0 (4d91de4e4 2025-02-17)`.
- `cargo --version` — `cargo 1.85.0 (d73d2caf9 2024-12-31)`.
- `cargo fmt --all -- --check` — passed, exit 0, no output.
- `cargo build --locked --workspace` — passed, exit 0; `Finished dev profile`.
- `cargo check --locked --workspace --all-targets` — passed, exit 0; checked `endfield-unix v0.1.0`.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed, exit 0 with no warnings.
- `cargo test --locked --workspace` — passed: 7 library unit tests, 6 compiled-executable integration tests, 0 failures. Binary and doc test targets contained 0 tests.
- `cargo metadata --locked --no-deps --format-version 1` — passed; metadata reports package `endfield-unix` at version `0.1.0`, edition `2024`, Rust version `1.85`, no dependencies, and one application binary target named `endfield-unix`.
- `cargo run --locked --quiet -p endfield-unix -- --help` — exit 0; printed the application name, active-development status, usage/options, and P004 GUI boundary.
- `cargo run --locked --quiet -p endfield-unix -- --version` — exit 0; output `endfield-unix 0.1.0`.
- `cargo run --locked --quiet -p endfield-unix` — exit 69; output `endfield-unix: GUI mode is unavailable: the GTK4 application shell is planned for P004.`
- `cargo run --locked --quiet -p endfield-unix -- --unknown` — exit 2; output `endfield-unix: unrecognized argument "--unknown"`.
- Compiled-executable startup integration test — passed with inherited environment cleared. The workspace has no external Rust dependencies, and startup performs no GPU, Wine, Proton, game, filesystem, or network access.
- GTK window tests — not run; no GTK window exists in P002.
- `git diff --check && git diff --cached --check` — passed with no whitespace errors.
- `.github/workflows/ci.yml` — added for pushes to `main` and pull requests targeting `main`; it runs the same Rust checks on `ubuntu-latest` without GPU or desktop requirements.
