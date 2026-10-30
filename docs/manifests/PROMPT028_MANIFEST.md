# PROMPT028 implementation manifest

## Scope and source

- Repository: `https://github.com/nghianguyen150612/Endfield-Linux`
- Qualified base: `223bcd404a422a54c56c06e7bc5afe273e85320b` (P027)
- Branch: `feat/p028-isolated-prefix-management`
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
- [ ] Full workspace test, Clippy, and build — local runs stop at missing GLib development files; hosted qualification pending.
- [x] Focused prefix-management tests (20 passed) and warning-denied Clippy.
- [ ] Actual P027/P028 process-level CLI integration — included in CI; hosted run pending.
- [x] Non-GTK workspace tests, build, and warning-denied Clippy passed; full GTK/daemon/IPC regression run pending hosted CI.
- [x] Cargo metadata assertions: exactly two executable names; prefix crate is library-only and has no GTK/daemon dependency.
- [x] GCC C17 ABI validation; Clang is unavailable locally and remains pending hosted CI.
- [x] `git diff --check`
- [ ] Final-head GitHub Actions qualification.

## Known blockers and evidence status

- Wine/Proton/UMU prefix initialization is unsupported; no process adapter is validated.
- No game or Endfield launch was performed. Compatibility remains unverified.
- Physical Arch and Dell Vostro device checks remain unverified.
- Local full workspace build/test/clippy are blocked by missing GLib/GTK4/libadwaita development files. The local prefix library tests and clippy are runnable independently. No fake pkg-config metadata was used.
- The local shell has GCC but not Clang. Hosted GCC/Clang C17 validation is pending.
- Draft PR and hosted final-head CI evidence will be recorded here after push. Do not merge while required gates are pending.
