# PROMPT028 implementation manifest

## Scope and source

- Repository: `https://github.com/nghianguyen150612/Endfield-Linux`
- Qualified base: `223bcd404a422a54c56c06e7bc5afe273e85320b` (P027)
- Branch: `feat/p028-isolated-prefix-management`
- Draft PR: [#1](https://github.com/nghianguyen150612/Endfield-Linux/pull/1), open and unmerged
- Qualified implementation head: `f24af7ba2b139db5f03adb9a58180e7ea9fa0d6e`
- Scope: P028 isolated, directory-only Wine prefix management
- Executables: unchanged; `endfield-unix` and `endfield-unixd` only
- C17 optimizer ABI: unchanged (`0x00010000`)

## Implementation

- Added library-only `endfield-unix-prefix-management` with typed IDs, P027 machine/profile scopes, exact runtime associations, lifecycle records with typed initialization evidence, versioned/checksummed metadata, mockable filesystem operations, Linux no-follow directory access, mount-boundary checks, locking, revision checks, recovery journaling, safe deletion checks, and active-use refusal.
- Added headless `endfield-unix prefix` list/show/validate/plan/create/init/recover/remove/delete commands. `init` is explicitly unsupported and never starts a process.
- Reused P027 resolution and P025/P026/P022 identity checks. Prefix associations do not update pins or migrate when current selection changes.
- Updated CLI, architecture, storage, security, runtime-provider, runtime-pinning, UMU, roadmap, README, and testing documentation. Added explicit note that the qualified P027 main branch already assigns P028 to prefix management; no historical roadmap item was rewritten.
- Added focused CI tests and metadata assertions for a GTK-free library crate, plus process-level managed/external P027/P028 CLI assertions.

## Changed files

- `.github/workflows/ci.yml`
- `CHANGELOG.md`, `Cargo.lock`, `Cargo.toml`, `README.md`
- `apps/endfield-unix/Cargo.toml`
- `apps/endfield-unix/src/cli/{commands,dispatch,mod,output,parser,prefix,runtime_pin}.rs`
- `apps/endfield-unix/tests/{cli,runtime_install_cli}.rs`
- `crates/prefix-management/{Cargo.toml,src/lib.rs,src/linux_fs.rs}`
- `docs/{ARCHITECTURE,CLI,DOMAIN,ROADMAP,RUNTIME-PINNING,RUNTIME-PROVIDERS,SECURITY,STORAGE,TESTING,UMU-INTEGRATION}.md`
- `docs/PREFIX-MANAGEMENT.md`
- `docs/manifests/PROMPT028_MANIFEST.md`

## Acceptance gates

- [x] `cargo +1.85.0 fmt --all -- --check`
- [x] Full workspace test, Clippy, and build passed in hosted CI; local GTK-dependent commands are blocked by missing GLib development files.
- [x] Focused prefix-management tests (20 passed) and warning-denied Clippy.
- [x] Actual P027/P028 process-level CLI integration passed in the isolated-XDG hosted run. On this runner, deletion returned `prefix_use_unknown`; the test verified refusal and preserved data.
- [x] Non-GTK workspace tests, build, and warning-denied Clippy passed locally; hosted P006/P021/P022/P025/P026/P027, daemon/IPC, and GTK regressions passed.
- [x] Cargo metadata assertions: exactly two executable names; prefix crate is library-only and has no GTK/daemon dependency.
- [x] GCC and Clang C17 ABI validation passed in hosted CI; GCC also passed locally.
- [x] `git diff --check`
- [x] All three GitHub Actions jobs passed in [run 41082558186](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41082558186) for implementation head `f24af7ba2b139db5f03adb9a58180e7ea9fa0d6e`.

## Known blockers and evidence status

- Wine/Proton/UMU prefix initialization is unsupported; no process adapter is validated.
- No game or Endfield launch was performed. Compatibility remains unverified.
- Physical Arch and Dell Vostro device checks remain unverified.
- Local full workspace build/test/clippy are blocked by missing GLib/GTK4/libadwaita development files. The local prefix library tests and clippy are runnable independently. No fake pkg-config metadata was used.
- The local shell has GCC but not Clang; both C17 compilers passed hosted CI. Optional real systemd user-manager integration was unavailable and skipped.
- Prefix deletion checks exact `WINEPREFIX` values in visible same-UID `/proc` environments. An unreadable scan returns `prefix_use_unknown` and blocks deletion; no process visibility is inferred.
- Earlier hosted runs exposed and fixed a runner-dependent `/proc` test assumption, a synthetic runtime mode restoration issue, and a missing test import. The final run above passed.
