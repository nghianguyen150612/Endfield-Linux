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

## P003 validation record — 2026-10-23

- Rust toolchain — pinned Rust 1.85.0, matching the repository MSRV.
- cargo fmt --all -- --check — passed.
- cargo check --locked --workspace --all-targets — passed.
- cargo clippy --locked --workspace --all-targets -- -D warnings — passed with no warnings.
- cargo test --locked --workspace — passed: 7 application unit tests, 6 application CLI integration tests, 4 FFI/layout unit tests, and 9 Rust FFI integration tests; 26 total, 0 failures.
- cargo metadata --locked --no-deps --format-version 1 — passed; packages are endfield-unix and optimizer-ffi, with exactly one application binary.
- cargo build --locked --workspace — passed.
- CC=gcc native/optimizer/tests/run-c-tests.sh — passed. The script compiles the ABI header and C17 implementation with -Wall -Wextra -Wpedantic -Werror, archives the native object, links the independent C test program against that static library, and executes it.
- CC=gcc native/optimizer/tests/run-sanitizers.sh — passed with AddressSanitizer and UndefinedBehaviorSanitizer enabled; no sanitizer report was emitted.
- C and Rust 64-bit ABI layout assertions — passed for sizes and natural alignment.
- Local Clang validation — not run because Clang is not installed in this environment. The GitHub workflow now has separate GCC and Clang C17 ABI jobs.
- The workflow includes all P002 Rust checks and adds standalone C17 header/library tests. GitHub-hosted job results are reported after the commit is pushed.
- GTK, Wine/Proton, game launch, daemon, telemetry collection, benchmark execution, and optimizer candidate generation remain unimplemented and were not tested.

## P004 validation and GTK build requirements

The supported Ubuntu CI runner is Ubuntu 24.04. The Rust check job installs `libgtk-4-dev`, `libadwaita-1-dev`, `pkg-config`, `dbus-daemon`, and `xvfb` before building. The application uses the Relm4 0.9.1 / GTK4-rs 0.9.6 / libadwaita-rs 0.7.2 bindings and the libadwaita 1.4 `NavigationSplitView` API. The graphical smoke test runs the actual Relm4 application under Xvfb and a private D-Bus session; it visits all six pages, checks their identities and header title, verifies the window's initial/resized sizes and SVG asset, and closes the window. CI also captures the application's initial Home page through GTK's renderer and publishes it as the `endfield-unix-gtk-smoke` workflow artifact.

Local GTK build prerequisites are GTK4 development files, libadwaita 1.4 or newer development files, `pkg-config`, and a C compiler. The app does not require GNOME Shell. On Arch Linux, install `gtk4`, `libadwaita`, `pkgconf`, and `gcc`; on the supported Ubuntu 24.04 runner, install `libgtk-4-dev`, `libadwaita-1-dev`, `pkg-config`, `dbus-daemon`, and `xvfb`.

P004 local validation uses Rust 1.85.0. The Rust sources and all target types were checked using temporary pkg-config metadata because this execution container has no GTK/GLib development packages. This type-check does not verify native linking or GUI runtime. GCC C17 ABI and Rust FFI checks remain separate; CI runs the graphical smoke test with the real GTK/libadwaita libraries. Do not treat an unavailable local display or missing GTK packages as a passing graphical test.

The CLI integration tests run without display variables and assert that help/version do not initialize GTK, no-argument startup attempts real GTK initialization and reports a missing display with status 69, and invalid arguments still fail with status 2. `run_smoke_test` is an ignored, display-dependent integration test executed only in the configured Xvfb job.

## P005 validation record — 2026-10-23

- Rust toolchain — pinned Rust 1.85.0.
- Clap compatibility — pinned Clap 4.5.53 and resolved `clap_derive` 4.5.49 both declare Rust 1.74 support.
- `cargo fmt --all -- --check` — passed locally and in GitHub Actions.
- `cargo check --locked --workspace --all-targets` — passed locally using temporary pkg-config metadata for Rust type checking and passed on the native GitHub runner with GTK4/libadwaita installed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed locally using the same type-check metadata and passed on the native GitHub runner.
- CLI source harness — passed 21 CLI unit tests and all 11 compiled-command integration assertions. The temporary harness compiles the repository's CLI modules and uses a GUI-startup stub.
- `cargo test --locked --workspace` — passed in GitHub Actions, including the compiled CLI tests and Rust FFI tests. The local attempt could not link because GTK4/libadwaita development libraries are absent from this container.
- GTK regression — the Xvfb/D-Bus GUI smoke test passed, visited the six existing navigation destinations, and uploaded the screenshot artifact.
- `cargo build --locked --workspace` and `cargo metadata --locked --no-deps --format-version 1` — passed in GitHub Actions; metadata reports exactly one application binary, `endfield-unix`.
- GCC and Clang C17 ABI workflow jobs — passed. Local GCC C17 ABI and sanitizer tests also passed; Clang is not installed locally.
- GitHub Actions — [Rust CI run for commit `cffa8a1`](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/39948830829) completed successfully.
