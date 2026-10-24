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

## P006 validation record — 2026-10-24

- Rust toolchain — pinned Rust 1.85.0 (`rustc 1.85.0`). Direct storage dependencies are pinned; full Cargo metadata reported no declared dependency MSRV above 1.85.0. Twenty-three resolved packages do not publish a `rust-version` declaration.
- `cargo fmt --all -- --check` — passed after formatting the new crate.
- `cargo check --locked --workspace --all-targets` — passed using temporary pkg-config metadata because this environment has no GTK4/libadwaita development packages. This verifies Rust type checking but does not verify native GTK linking.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed with the same temporary pkg-config metadata.
- `cargo test --locked -p endfield-unix-storage` — passed: 26 tests across unit tests, configuration contracts, migration contracts, and recovery tests. Coverage includes injected pre-rename failure, simultaneous first writers, stale-write conflict, XDG paths, permission errors, app-directory and file symlink refusal, SQLite transaction rollback, migration rollback, future schemas, corruption preservation, foreign-key enforcement, metadata persistence, and bounded lock waits.
- `cargo test --locked -p optimizer-ffi` — passed: 13 unit and integration tests. The public C header and C17 ABI were not changed.
- `CC=gcc native/optimizer/tests/run-c-tests.sh` and `CC=gcc native/optimizer/tests/run-sanitizers.sh` — passed.
- `cargo metadata --locked --format-version 1` — passed; the workspace contains exactly one application binary, `endfield-unix`. Dependency metadata was checked for declared Rust versions through 1.85.0.
- The full Rust workspace test/build and graphical smoke test cannot be linked or run in this container because GTK4/libadwaita development libraries and Xvfb are absent. Clang is also unavailable locally. The hosted workflow completed those checks successfully on the native runner.
- `cargo test --locked --workspace` — passed in hosted CI, including existing CLI and FFI tests and the 26 storage tests.
- GTK regression — the Xvfb/D-Bus smoke test passed and uploaded the screenshot artifact.
- `cargo build --locked --workspace` and `cargo metadata --locked --no-deps --format-version 1` — passed in hosted CI; exactly one application executable remains.
- GCC and Clang C17 ABI jobs — passed in hosted CI. The C17 ABI and public header are unchanged.
- GitHub Actions — [Rust CI run for commit `3a345e6`](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40064013008) completed successfully.
- Power-loss recovery was not tested.

## P007 validation record — 2026-10-24

- Rust toolchain — installed and used the pinned Rust 1.85.0 toolchain.
- `cargo fmt --all -- --check` and `git diff --check` — passed.
- `cargo test --locked -p endfield-unix-domain` — passed: 22 synthetic contract tests covering identifier compatibility, availability and zero semantics, measurement conversion, snapshot invariants and normalization, multiple GPUs, partial provider results, total provider failure, and Linux/macOS/FreeBSD identities.
- `cargo clippy --locked -p endfield-unix-domain --all-targets -- -D warnings` — passed.
- `cargo test --locked -p endfield-unix-storage` — passed: 26 existing storage tests.
- `cargo test --locked -p optimizer-ffi` — passed: 13 existing Rust FFI tests. The C ABI header and optimizer FFI implementation are unchanged.
- `cargo clippy --locked -p endfield-unix-storage -p optimizer-ffi --all-targets -- -D warnings` — passed.
- `CC=gcc native/optimizer/tests/run-c-tests.sh` and `CC=gcc native/optimizer/tests/run-sanitizers.sh` — passed.
- `cargo metadata --locked --no-deps --format-version 1` — passed; the workspace has four library/application packages and exactly one application binary, `endfield-unix`. CI additionally asserts that the domain package has no dependencies or binary target.
- A native `cargo check --locked --workspace --all-targets` — blocked because this environment lacks the GLib/GTK4/libadwaita development packages; `pkg-config` could not find `glib-2.0`. With temporary pkg-config metadata used only for Rust type checking, workspace check and clippy passed locally; this did not verify native linking or GUI runtime. The hosted runner performed the real workspace build and tests.
- GitHub Actions — [P007 CI run for commit `9d43753`](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40067387932) completed successfully: formatting, workspace check, clippy, full workspace tests, GTK smoke and screenshot checks, Cargo metadata validation, and GCC/Clang C17 ABI jobs passed. Xvfb and Clang are not installed locally.
- No real hardware, display server, Vulkan library, or operating-system probe was used by the domain tests.

## P008 validation record — 2026-10-24

- Rust toolchain — installed and used pinned Rust 1.85.0. Locked Cargo metadata resolved 145 packages; none declares an MSRV above 1.85.0, and 24 packages omit `rust-version` metadata.
- `cargo fmt --all -- --check` and `git diff --check` — passed.
- `cargo test --locked -p endfield-unix-diagnostics --all-targets` — passed: 29 tests total (2 unit, 12 logging contract, 7 redaction contract, and 8 report contract tests). Coverage includes strict event schema checks, dropped domain details, all seven category mappings, bounded worker behavior, concurrent thread writers, permissions, retention, malformed and oversized records, symlink refusal, report export and overwrite policy, and incomplete-export cleanup. Cross-process coordination is implemented with the advisory lock but was not separately exercised in a process-level test.
- `cargo clippy --locked -p endfield-unix-diagnostics --all-targets -- -D warnings` — passed.
- `cargo test --locked -p endfield-unix-domain -p endfield-unix-storage -p optimizer-ffi --all-targets` — passed: 22 domain, 26 storage, and 13 FFI tests. The domain tests use synthetic fixtures; no host hardware was queried.
- `cargo clippy --locked -p endfield-unix-domain -p endfield-unix-storage -p optimizer-ffi --all-targets -- -D warnings` — passed.
- `cargo check --locked --workspace --all-targets` and `cargo clippy --locked --workspace --all-targets -- -D warnings` — passed using temporary pkg-config metadata for Rust type checking. A native GTK build is unavailable in this container: GTK4/libadwaita and GLib development packages are absent. The metadata stubs do not validate native linking or GUI runtime.
- `cargo metadata --locked --no-deps --format-version 1` — passed; exactly one application binary remains: `endfield-unix`.
- `CC=gcc native/optimizer/tests/run-c-tests.sh` and `CC=gcc native/optimizer/tests/run-sanitizers.sh` — passed. No C17 header, ABI, or optimizer FFI implementation was changed.
- The local full-workspace test/build and GTK smoke test were not run because GTK4/libadwaita development libraries and Xvfb are unavailable. Clang is also unavailable locally. The GitHub workflow retains the real GTK build, full workspace tests/build, GTK smoke and screenshot, and GCC/Clang ABI jobs.
- GitHub Actions — [Rust CI run for commit `c863739`](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40071954339) completed successfully: formatting, native workspace check, Clippy, full workspace tests, GTK smoke and screenshot upload, workspace build, Cargo metadata assertions, and GCC/Clang C17 ABI jobs passed.
- No credentials, environment dumps, or real hardware were read by the diagnostics and redaction tests.

## P009 validation record — 2026-10-24

- Rust toolchain — pinned Rust 1.85.0. The daemon's locked dependency tree resolves `signal-hook` 0.3.18, `signal-hook-registry` 1.4.8, `libc` 0.2.191, and `thiserror` 2.0.11; the daemon package and all its targets compile with Rust 1.85.0.
- `cargo +1.85.0 fmt --all -- --check` — passed.
- `cargo +1.85.0 check --locked -p endfield-unixd --all-targets` — passed.
- `cargo +1.85.0 build --locked -p endfield-unixd` — passed.
- `cargo +1.85.0 clippy --locked -p endfield-unixd -p endfield-unix-diagnostics --all-targets -- -D warnings` — passed.
- `cargo +1.85.0 test --locked -p endfield-unixd --all-targets` — passed: 5 lifecycle/diagnostics unit tests, 5 CLI tests, and 11 lifecycle tests. Linux subprocess coverage started the actual executable, observed `DAEMON_RUNNING`, sent SIGTERM and SIGINT, checked orderly status 0 and one cleanup event, exercised repeated signals and optional diagnostics failure, reaped each child, and confirmed unchanged CPU ticks during a 300 ms idle observation. Controlled-worker tests verified typed failure and timeout behavior without cancellation claims.
- `cargo +1.85.0 test --locked -p endfield-unix-diagnostics --all-targets` — passed: 30 tests, including bounded logger shutdown, P008 event compatibility, schema-version-1 daemon events, and partial daemon support-report metadata.
- `cargo +1.85.0 test --locked -p endfield-unix-domain -p endfield-unix-storage -p optimizer-ffi --all-targets` — passed: 61 domain, storage, and Rust FFI tests.
- `CC=gcc native/optimizer/tests/run-c-tests.sh` and `CC=gcc native/optimizer/tests/run-sanitizers.sh` — passed; ABI 1.0 and the public C header are unchanged.
- `cargo metadata --locked --no-deps --format-version 1` — passed; exactly two application binaries are reported, `endfield-unix` and `endfield-unixd`. The daemon's direct dependencies contain no GTK4, libadwaita, or Relm4.
- Full local `cargo check --locked --workspace --all-targets` — blocked because this container lacks GLib, GTK4, and libadwaita development files. Xvfb and Clang are also absent, so the full GUI smoke and Clang C17 jobs ran in hosted CI.
- GitHub Actions — [Rust CI run for commit `915e48f`](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40079025273) completed successfully. Native workspace check, full Clippy, workspace tests (including the bounded daemon gate and existing GUI CLI/FFI/storage/domain/diagnostics regressions), GTK smoke and screenshot upload, workspace build, exact two-binary metadata assertion, and GCC/Clang C17 ABI jobs all passed.
- `git diff --check` — passed.
