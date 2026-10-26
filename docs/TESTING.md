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

## P010 validation record — 2026-10-24

- Rust toolchain — pinned Rust 1.85.0; workspace edition 2024. Locked direct IPC dependencies include `fs2` 0.4.3, `libc` 0.2.191, `nix` 0.29.0, `serde` 1.0.229, `serde_json` 1.0.145, and `thiserror` 2.0.11.
- `cargo +1.85.0 fmt --all -- --check` and `git diff --check` — passed.
- `cargo +1.85.0 test --locked -p endfield-unix-ipc -p endfield-unixd --all-targets` — passed: 28 IPC unit/contract tests and 21 daemon, CLI, and lifecycle tests. Coverage includes typed v1.0 requests and errors, unknown and extra fields, identifier and integer bounds, partial and malformed frames, peer UID policy, actual same-UID kernel credentials, path permissions and ownership, active-instance refusal, safe stale-socket recovery, Ping/Status, mismatched responses, slow byte-trickle deadline enforcement, client timeouts, SIGTERM/SIGINT lifecycle, and owned socket cleanup.
- `cargo +1.85.0 clippy --locked -p endfield-unix-ipc -p endfield-unixd --all-targets -- -D warnings` — passed.
- `cargo +1.85.0 test --locked -p endfield-unix-diagnostics -p endfield-unix-domain -p endfield-unix-storage -p optimizer-ffi --all-targets` — passed: 30 diagnostics, 22 domain, 26 storage, and 13 Rust FFI tests. Corresponding Clippy with `-D warnings` passed.
- `CC=gcc native/optimizer/tests/run-c-tests.sh` and `CC=gcc native/optimizer/tests/run-sanitizers.sh` — passed. Clang is not installed locally.
- `cargo +1.85.0 metadata --locked --no-deps --format-version 1` — passed; exactly two application binaries remain, the IPC crate is library-only, and neither the IPC crate nor daemon directly depends on GTK, libadwaita, or Relm4.
- Workspace `cargo check` and `cargo clippy` (all targets) — passed using temporary pkg-config metadata only to type-check Rust code. The end-to-end test target also type-checks. The temporary metadata does not verify native GTK linking or runtime. A native GTK check/build, workspace test/link, graphical smoke test, and real daemon/CLI subprocess IPC gate cannot run in this container because GLib/GTK4/libadwaita development libraries and Xvfb are absent. The dedicated Linux subprocess gate is configured in hosted CI; it exercises Ping/Status, malformed input, over-capacity clients, slow/disconnected clients, SIGTERM/SIGINT, shutdown cleanup, and child reaping.
- Cross-UID kernel authentication was not exercised because no second UID is available; the deterministic UID policy test and actual same-UID `SO_PEERCRED` path are separate checks. No manual Arch Linux runtime session was run.
- CI fixture correction — the initial run found that IPC test runtime roots inherited the hosted runner's broader umask. The fixtures now explicitly set mode `0700`, preserving the production runtime-directory check.
- GitHub Actions — [Rust CI run for commit `2de9b88`](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40083552910) completed successfully. Native workspace check, Clippy, application binary build, full workspace tests, P009 lifecycle gate, real daemon/CLI IPC subprocess gate, GTK smoke and screenshot upload, workspace build, exact two-binary/GTK-dependency metadata assertions, and GCC/Clang C17 ABI jobs all passed.

## P011 validation record — 2026-10-24

P011 is complete after the hosted checks passed in [Rust CI run
40087592204](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40087592204).
The local execution environment does not include native GTK4, libadwaita, or
GLib development packages, and it is not an Arch Linux device.

- Rust toolchain — pinned Rust 1.85.0.
- `cargo +1.85.0 fmt --all -- --check` — passed.
- `cargo +1.85.0 test --locked -p endfield-unix-platform-linux --all-targets` — passed: 44 tests across parser contracts, source precedence and symlink policy, kernel/domain integration, runtime-environment fixtures, and the live Linux kernel/architecture smoke.
- `cargo +1.85.0 clippy --locked -p endfield-unix-platform-linux --all-targets -- -D warnings` — passed.
- `cargo +1.85.0 test --locked -p endfield-unix-diagnostics --all-targets` — passed: 31 tests, including the new P011 provider allowlist/redaction contract; matching Clippy passed.
- `cargo +1.85.0 test --locked -p endfield-unix-ipc -p endfield-unixd -p endfield-unix-domain -p endfield-unix-storage -p optimizer-ffi --all-targets` — passed. P010 IPC, daemon lifecycle, portable domain, storage/recovery, and Rust FFI tests all passed.
- Workspace `cargo check --locked --workspace --all-targets` and Clippy with `-D warnings` — passed using temporary pkg-config metadata. App Rust targets and all integration-test targets type-checked; the metadata does not verify native GTK linking.
- Cargo metadata — passed; exactly two application binaries remain, `endfield-unix` and `endfield-unixd`; the Linux adapter is library-only with no GTK, libadwaita, or Relm4 dependency; P007 domain still has no dependencies.
- `CC=gcc native/optimizer/tests/run-c-tests.sh` and `CC=gcc native/optimizer/tests/run-sanitizers.sh` — passed. Clang is not installed in this local environment; hosted CI runs the existing GCC/Clang matrix.
- A native GTK workspace build, app test linking, GTK smoke, and GTK screenshot are not claimed locally because GLib, GTK4, and libadwaita development packages are absent. CI now runs a real `diagnose platform` CLI command, the adapter tests, existing GUI smoke/screenshot, and the full workspace gates on Ubuntu 24.04.
- CI metadata verifies exactly the two application binaries (`endfield-unix` and `endfield-unixd`), verifies that the platform adapter is library-only, and checks that it has no GUI dependency.
- P003 C17 files and the P010 IPC protocol are unchanged. No Arch Linux physical-device validation has been performed; use the manual comparison commands in [PLATFORM-LINUX.md](PLATFORM-LINUX.md).
- Hosted GitHub Actions — [Rust CI run for commit `3a2ebdb`](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40087592204) completed successfully. Workspace check and Clippy, app/daemon builds, the real headless platform CLI, all 44 adapter tests and live smoke, full workspace tests, daemon lifecycle, authenticated IPC integration, GTK smoke and screenshot artifact, workspace build, and exact two-binary/library-only metadata assertions passed.
- Hosted C17 matrix — GCC and Clang jobs both passed in the same workflow.
- Actual hosted `diagnose platform` output from the Ubuntu runner:

  ```text
  Platform: Linux
  Distribution: Ubuntu 24.04.5 LTS (id: ubuntu; version: 24.04)
  Kernel: Linux 6.17.0-1022-azure
  Architecture: x86_64 (kernel-reported)
  Environment: native Linux process (low confidence; sources: linux-environment)
  Environment uncertainty: no recognized container or WSL marker was found; bare-metal status is not established
  User runtime directory: available and valid (linux-xdg-runtime-dir)
  Probe result: partial
  ```

- The hosted output is a real Ubuntu CI runner observation. It does not substitute for manual Arch Linux, CachyOS, or physical-device validation.

## P012 validation record — 2026-10-25

P012 implementation and hosted qualification are complete. The hosted workflow
for commit `6d01cb41bb3d495a0b6b3f4f19215feee34e49da` passed in [CI run
40161140586](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40161140586).
This workspace does not include native GTK4/libadwaita development packages or
Xvfb, and it is not an Arch Linux device.

- Rust toolchain — pinned Rust 1.85.0; P012 directly pins `libc` 0.2.191 and
  `nix` 0.29.0, consistent with the existing locked workspace dependencies.
- `cargo +1.85.0 fmt --all -- --check` and `git diff --check` — passed.
- `cargo +1.85.0 test --locked -p endfield-unix-platform-linux --all-targets`
  — passed: 68 tests total, including 24 P012 deterministic hardware
  contracts, the P012 live Linux CPU/memory/snapshot smoke, and all P011
  parser, platform, environment, and live-kernel contracts.
- `cargo +1.85.0 clippy --locked -p endfield-unix-platform-linux
  --all-targets -- -D warnings` — passed.
- `cargo +1.85.0 check --locked --workspace --all-targets` and workspace
  Clippy with `-D warnings` — passed using temporary pkg-config metadata for
  Rust type checking. App, CLI, and test targets type-checked; the metadata
  does not verify native GTK linking or runtime.
- Domain, diagnostics, storage, IPC, daemon, and Rust FFI package tests —
  passed. This includes the P012 provider allowlist/redaction test, P010 IPC,
  P009 lifecycle, storage migrations/recovery, P007 model contracts, and the
  Rust C17 FFI contracts.
- `CC=gcc native/optimizer/tests/run-c-tests.sh` and
  `CC=gcc native/optimizer/tests/run-sanitizers.sh` — passed. Clang is not
  installed locally.
- Locked Cargo metadata — passed; exactly two application binaries remain:
  `endfield-unix` and `endfield-unixd`. The Linux adapter is library-only and
  its dependencies contain no GTK, libadwaita, or Relm4.
- Hosted workflow gates — passed: format, native GTK workspace check,
  workspace Clippy, application binaries, `diagnose platform`,
  `diagnose hardware`, platform adapter tests, full workspace tests, P009
  lifecycle, P010 IPC end-to-end, GTK smoke and screenshot artifact,
  workspace build, metadata assertions, and both GCC and Clang C17 ABI jobs.
- Actual hosted `diagnose hardware` output from the Ubuntu runner:

  ```text
  Hardware probe: Linux
  CPU:
    Model: AMD EPYC 7763 64-Core Processor
    Logical CPUs online: 4
    Physical cores: unknown
    CPUs present: 4
    CPUs possible: 4
    CPUs offline: 0
    CPUs allowed to process: 4
  CPU scheduling:
    Affinity query: available and queryable
    Scheduler policy query: available and queryable
    Current policy: other
    Current static priority: 0
    Scheduling control permission: not assessed; no settings were changed
  CPU frequency/power:
    cpufreq interface: unavailable
    Policies: none detected
    Platform profile: unknown
    Power interface: unavailable
    Control write permission: not assessed
    Momentary frequency: not sampled
  Memory:
    Host-visible total RAM: 15.61 GiB
    Host-visible available RAM: 14.14 GiB
    Swap: 2.99 GiB total, 2.99 GiB free
    Cgroup status: v2
    Cgroup CPU-time quota: unlimited
    Cgroup effective CPU set: 4 CPUs
    Cgroup memory limit: unlimited
    Cgroup memory usage: 2.82 GiB
    Cgroup swap limit: unlimited
    Cgroup ancestor limits: not assessed; leaf limits are shown separately from host RAM
  Probe result: partial
  ```

- The hosted output is an Ubuntu CI runner observation and does not substitute
  for testing on an Arch Linux machine or physical devices.
- No manual Arch Linux, CachyOS, or physical-device validation has been
  performed: real Arch device validation is **UNVERIFIED**. Synthetic desktop
  and laptop fixtures are test inputs, not measurements from user hardware.

## P013 validation record — 2026-10-25

P013 is implemented on `main` and hosted qualification passed in [Rust CI run
40170150212](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40170150212)
for commit `438b58aa14fc74ce09b09cc4de758dddfeacb744`. Local validation used
Rust 1.85.0.

- `cargo +1.85.0 fmt --all` and `git diff --check` — passed.
- `cargo +1.85.0 test --locked -p endfield-unix-platform-linux --all-targets`
  — passed: 92 adapter tests, including deterministic PCI/DRM limit and mapping
  tests, hybrid multi-GPU correlation, software Vulkan, missing-loader,
  initialization failure, zero-device, malformed metadata, and live Linux
  smoke tests.
- `cargo +1.85.0 check --locked --workspace --all-targets` and workspace
  Clippy with `-D warnings` — passed using temporary pkg-config metadata stubs
  because this workspace has no GTK4/libadwaita development libraries. These
  checks type-check application and test targets, but do not link or run the
  native GUI.
- `cargo +1.85.0 test --locked --workspace --exclude endfield-unix` — passed,
  covering domain, diagnostics/redaction, storage/recovery, IPC crate,
  platform-adapter, daemon lifecycle, and Rust FFI tests. The hosted full
  workspace test passed, including app CLI and GPU formatting tests. The
  P010 authenticated IPC end-to-end test, including its unchanged over-capacity
  saturation and recovery check, passed. No IPC test was changed or weakened.
- Cargo metadata assertions — passed: exactly two application binaries, a
  library-only Linux adapter, no Linux/Vulkan dependency in the domain, and no
  GTK dependency in the adapter or daemon.
- `CC=gcc native/optimizer/tests/run-c-tests.sh` and the GCC sanitizer script
  — passed. Clang is not installed locally; both hosted GCC and Clang C17 jobs
  passed.
- Hosted workspace check, Clippy, both executable builds, `diagnose platform`,
  `diagnose hardware`, `diagnose gpu`, all 92 adapter tests, the full workspace
  tests, daemon lifecycle, IPC end-to-end, Xvfb/DBus GUI smoke and screenshot,
  workspace build, and Cargo metadata assertions all passed. Metadata retained
  exactly `endfield-unix` and `endfield-unixd`; the Linux adapter is
  library-only and the domain remains dependency-free.
- Actual hosted `diagnose gpu` output from Ubuntu 24.04.5 LTS:

  ```text
  GPU discovery: Linux
  PCI graphics devices: 0
  DRM nodes: 1 validated (partial)
  Vulkan:
    Loader: present, initialization failed
    Physical devices: unknown
    Software devices: unknown
    Unmatched devices: unknown
    Detail: Vulkan instance initialization failed
  Probe result: partial
  ```

  This runner observation had no PCI graphics-class candidates and one DRM
  node without a validated PCI relationship. Vulkan instance initialization
  failed, so device counts remain unknown. No physical GPU was fabricated.
- Actual local Linux GPU smoke: PCI source available with zero graphics-class
  devices, DRM sysfs source missing, and the Vulkan loader loaded but instance
  initialization failed before physical-device enumeration; P012 CPU and
  memory facts remained present. The valid combined report was partial. This
  is the current container observation, not a target desktop or laptop
  measurement.
- Real Arch desktop/NVIDIA RTX 5070 and Dell Vostro 3510/Intel Iris Xe plus
  NVIDIA MX350 checks — **UNVERIFIED**. Ubuntu CI is not a substitute. See
  [GPU-LINUX.md](GPU-LINUX.md) for manual comparisons and interpretation.

## P014 validation record — 2026-10-25

P014 is implemented on `main`; hosted qualification passed in
[Rust CI run 40176731426](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40176731426)
for implementation commit
`6efe0cd1b0feb43df22e80be93896c921cd9526e`. Local validation used Rust 1.85.0.

- `cargo +1.85.0 fmt --all -- --check` and `git diff --check` — passed.
- `cargo +1.85.0 test --locked -p endfield-unix-platform-linux --all-targets
  -- --nocapture` — passed: 44 unit tests, the live display smoke, and the
  existing GPU, hardware, and platform contract/live suites. Coverage includes
  mixed Wayland/XWayland selection, headless preservation of P011–P013 facts,
  exact fractional modes, duplicate/sanitized labels, unknown primary state,
  stale endpoints, X11 authentication failure, DRM connector parsing and GPU
  mapping, sysfs containment, and bounded mode lists.
- `cargo +1.85.0 check --locked --workspace --all-targets` and workspace
  Clippy with `-D warnings` — passed using temporary pkg-config metadata stubs
  because the worker lacks GLib/GTK/libadwaita development packages. These
  commands type-check application and test targets but do not link or run the
  native application. The app CLI, Xvfb, GTK screenshot, and full native
  workspace checks were exercised by hosted CI below.
- `cargo +1.85.0 test --locked --workspace --exclude endfield-unix` — passed,
  covering domain, diagnostics/redaction, storage/recovery, IPC, the Linux
  adapter, daemon lifecycle, and Rust FFI tests. App CLI tests, the IPC
  subprocess gate, and GTK smoke are covered by the hosted CI record below.
- `cargo metadata --locked --no-deps --format-version 1` plus the CI metadata
  assertions — passed: exactly two executables remain, the Linux adapter is
  library-only, the domain crate has no dependencies, GTK stays out of the
  adapter and daemon, and display protocol dependencies stay out of the C17
  optimizer.
- `CC=gcc native/optimizer/tests/run-c-tests.sh` and
  `CC=gcc native/optimizer/tests/run-sanitizers.sh` — passed. Clang is not
  installed locally; hosted CI passed both GCC and Clang jobs.
- The live display smoke produced this valid local partial/headless result:

  ```text
  live display smoke: session_hint=Headless, verified_protocol=None, wayland=NotAdvertised/NotAttempted, x11=NotAdvertised/NotAttempted, selected_source=None, outputs=0, drm_status=Missing, drm_connectors=0, completeness=Partial, snapshot_valid=true
  ```

- Hosted qualification passed for all three jobs in
  [CI run 40176731426](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40176731426):
  the Rust/GTK/CLI/IPC job, C17 ABI tests with GCC, and C17 ABI tests with Clang.
  The Rust job passed formatting, workspace check and Clippy, app binary builds,
  headless platform/hardware/GPU/display commands, Xvfb/RandR, Linux adapter
  tests, workspace and contract tests, daemon lifecycle and authenticated IPC,
  graphical smoke and screenshot upload, workspace build, and Cargo metadata.
  The IPC gate completed without the saturation-timeout recurrence.
- The hosted headless display command correctly reported no connected display protocol while separately discovering a virtual DRM connector:

  ```text
  Display discovery: Linux
  Session:
    Type hint: headless
    Verified display protocol: none connected
    Compositor: unknown
    Wayland: not advertised (output inventory: not attempted)
    X11/RandR: not advertised (output inventory: not attempted)
  Display output source: unknown (no protocol inventory verified)
  Displays: none verified
  DRM connectors: 1
    Virtual-1: connected, enabled, virtual; GPU mapping: unknown
  Probe result: partial
  ```

- Hosted Xvfb/RandR verified a live X11 inventory with one output and 1600×900
  screen geometry. The output stayed classified as virtual/unknown physical
  connection, as expected for Xvfb:

  ```text
  Type hint: X11 (hint)
  Verified display protocol: X11
  X11/RandR: connected (output inventory: enumerated)
    Protocol version: 1.6
    Output records: 1
    Screen geometry: 1600×900 pixels
  Display output source: X11 RandR
  Display 1:
    Name: screen
    Connection: unknown
    State: active
    Mode resolution: 1600×900 pixels
  ```

- These hosted observations validate the headless and X11/RandR paths. They do
  not establish real Wayland compositor integration or physical display
  behavior.

- Real Wayland compositor, Arch desktop, Dell Vostro, Hyprland, and Niri
  validation remains **UNVERIFIED**. Deterministic fixtures do not establish
  real compositor integration.

## P015 validation record — 2026-10-25

P015 is implemented on `main` in commit
`4e6fb09bfc079b8dc74e6f72e2aac70c2de05198`. Hosted qualification passed after
rerunning the failed Rust job in [CI run 40184991675](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40184991675).

- Rust 1.85.0 — used locally and pinned in CI. `cargo fmt --all -- --check`,
  workspace `cargo check --locked --workspace --all-targets`, and workspace
  Clippy with `-D warnings` passed locally. Workspace check and Clippy used
  temporary pkg-config metadata only to type-check Rust targets because this
  container lacks GTK4, GLib, and libadwaita development files; native linking
  was verified on the hosted runner.
- `cargo test --locked -p endfield-unix-telemetry --all-targets` — passed
  locally: 50 deterministic tests across metric/unit, timestamp, freshness,
  provenance, missing-data, quality/counter, synthetic-example, and batch
  contracts.
- `cargo test --locked --workspace --exclude endfield-unix` — passed locally:
  315 tests covering the telemetry crate and existing domain, diagnostics,
  storage, platform adapter, daemon, IPC library, and Rust FFI targets. The
  Linux application IPC subprocess test is part of the GTK-linked application
  package and was run by hosted CI.
- Local `cargo build --locked --workspace --exclude endfield-unix`, metadata
  assertions, and `git diff --check` — passed. Metadata retains exactly the
  `endfield-unix` and `endfield-unixd` binaries; telemetry is library-only and
  depends directly only on the dependency-free domain crate.
- GCC C17 ABI tests and sanitizer tests — passed locally. Clang, GTK/Xvfb,
  graphical CLI package tests, and the complete workspace build/test require
  native dependencies absent from this container and were run in hosted CI.
- Hosted run `40184991675` passed formatting, native workspace check, Clippy,
  both executable builds, all four headless platform/hardware/GPU/display CLI
  commands, Xvfb/RandR display smoke, Linux platform-adapter tests, all 50
  telemetry tests, full workspace tests, the bounded daemon lifecycle gate,
  authenticated IPC subprocess gate, graphical smoke and screenshot upload,
  workspace build, and Cargo metadata assertions. GCC and Clang C17 ABI jobs
  also passed.
- The first hosted attempt failed once in the existing IPC saturation test at
  `apps/endfield-unix/tests/ipc_e2e.rs:144`: its one-second read observed
  `WouldBlock` instead of EOF on the over-capacity socket. The failed Rust job
  was rerun without changing or weakening IPC code or tests; the full workspace
  test and dedicated bounded IPC gate both passed on retry. This timing failure
  is recorded separately from P015 telemetry validation.
- The hosted platform/hardware/GPU/display commands exercised existing Linux
  discovery probes. P015's telemetry examples remain synthetic; P016 adds the
  first real Linux CPU and memory provider. GPU/FPS/thermal collection remains
  future work.

## P016 validation record — 2026-10-25

P016 was implemented on `main` from baseline
`667f2f1b79e2741e3effd4262a6ad7dc403dd3fe` and committed as
`7e1b186257243e88218a321b7d79014f3a9d7c7d`. Hosted qualification passed in
[CI run 40205107552](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40205107552)
after rerunning the failed Rust job without code or test changes.

- `cargo +1.85.0 fmt --all -- --check`, `git diff --check`, and
  `cargo +1.85.0 metadata --locked --no-deps --format-version 1` — passed
  locally. GCC C17 ABI tests passed locally; Clang is not installed locally.
- In an Ubuntu 24.04 validation container, Rust 1.85.0 workspace check,
  Clippy with `-D warnings`, and the application/daemon build passed. The live
  `diagnose telemetry cpu-memory --interval-ms 250` command returned measured
  CPU and memory observations and a partial result because current frequency
  was unavailable. The hosted CLI reported:

  ```text
  Aggregate utilization: 1.00%
  Sampling interval: 250.322 ms
  Logical CPUs sampled: 4/4
  Per-core utilization: available
  Current frequency: unknown
  RAM available: 14.57 GiB
  Estimated RAM used: 1.04 GiB
  Swap used: 0.00 MiB
  Cgroup memory usage: 3.29 GiB
  CPU source: measured
  Memory source: measured
  Clock: monotonic
  Probe result: partial
  ```

  Runner values vary and are not hardware baselines.
- Linux adapter all-target tests passed, including 62 adapter unit tests, the
  existing platform/hardware/GPU/display live and contract suites, the new live
  telemetry smoke, and 13 new telemetry provider contract tests. The dedicated
  P015 telemetry all-target tests also passed. The application unit and CLI
  integration tests passed after updating the expected help synopsis for the
  new subcommand.
- The full non-GUI workspace suite passed as the unprivileged workspace user
  using `cargo +1.85.0 test --locked --workspace --exclude endfield-unix`. It
  included the storage permission test, daemon lifecycle suite, IPC library,
  Rust FFI, diagnostics privacy, P015, and Linux adapter tests. The app unit
  and CLI integration tests passed in the native-dependency container.
- The first hosted Rust attempt, job `121204258845`, failed in the existing IPC
  saturation assertion at `apps/endfield-unix/tests/ipc_e2e.rs:144`: the
  over-capacity connection returned `WouldBlock` instead of EOF after one
  second. The job was rerun unchanged as `121205326149`; the full workspace
  tests and dedicated authenticated IPC subprocess gate passed. P016 changes no
  IPC internals or tests, and the initial timing failure is retained here as a
  separate known flake.
- Hosted run `40205107552` passed formatting, workspace check and Clippy, both
  executable builds, all five headless Linux diagnostics including telemetry,
  Xvfb/RandR, all Linux adapter and P015 telemetry tests, workspace/CLI/FFI/
  storage/domain/diagnostics regressions, daemon lifecycle, authenticated IPC,
  GTK smoke, screenshot upload, workspace build, and Cargo metadata assertions.
  GCC and Clang C17 ABI jobs passed. The GTK screenshot artifact is available as
  [`endfield-unix-gtk-smoke`](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40205107552/artifacts/12603782558).
- Real device status remains **Arch desktop validation: UNVERIFIED** and
  **Dell Vostro 3510 validation: UNVERIFIED**. Ubuntu CI and container smoke do
  not replace those checks.

## P017 GPU telemetry

Read-only Linux GPU telemetry is implemented through PCI-correlated optional
AMDGPU sysfs/hwmon, Intel i915 current frequency, and dynamically loaded NVIDIA
NVML gauges. `endfield-unix diagnose telemetry gpu --interval-ms 250` runs
headlessly, without Vulkan/display initialization or the daemon. Unsupported
utilization windows and missing sensors remain explicit partial data. Suspended
and unknown-power devices are skipped conservatively. No live samples are
persisted or included in default support reports. See [GPU-TELEMETRY-LINUX.md](GPU-TELEMETRY-LINUX.md) for source semantics, limitations, tests, and manual physical-device checks.

P017 local qualification uses Rust 1.85.0 and a temporary Ubuntu 24.04 container
with native GTK development packages. The 14 deterministic GPU contracts and
headless live smoke pass; the local live host has zero verified GPUs and produces
an empty valid partial P015 batch. GCC and Clang C17 ABI and sanitizer scripts
pass. A parallel full-workspace run encountered the unchanged CLI test
`default_and_explicit_gui_use_the_same_startup_behavior`: an asynchronous
`state_path_unavailable` diagnostics message differed between the two GUI startup
stderr streams. No GUI/diagnostics or IPC implementation/test is changed to mask
this result. Hosted qualification and exact workflow evidence will be recorded
following the implementation push.

The local serial workspace run passed app unit/CLI tests, then reproduced the
pre-existing IPC capacity race at `apps/endfield-unix/tests/ipc_e2e.rs:144`: the
read returned `WouldBlock` instead of EOF. The test took 1.61 seconds. See
[retained failure excerpt](validation/p017-local-ipc-failure.txt). No hosted
workflow/job ID applies to this local container. IPC was not retried locally;
qualification continues through the normal hosted workflow as requested. A
separate focused IPC reliability repair remains recommended.

P017 implementation commit `8ea541b17160fd29a729dd55470a242c31869b02`
passed hosted qualification on the first attempt in [run 40316047680](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40316047680).
Rust/GTK/Linux/IPC job `121548786536`, Clang `121548787049`, and GCC
`121548787129` all succeeded. Hosted workspace tests, both IPC gates, 170 Linux
adapter tests, the no-GPU telemetry CLI/live smoke, CPU/memory telemetry,
Xvfb/RandR, GTK screenshot, builds and metadata gates passed. The local capacity
race did not recur in this hosted attempt. See the [full P017 report](P017-IMPLEMENTATION-REPORT.md)
for output, source limitations, file/dependency details and physical validation
status. A follow-up preserves required platform failures as CLI 70/78 while
optional GPU source failures remain partial results; native workspace check,
Clippy, 48 application unit tests and the GPU CLI integration test passed for
that correction.
