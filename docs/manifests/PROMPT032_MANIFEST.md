# P032 implementation and qualification manifest

## Verified baseline and workflow

- Repository: <https://github.com/nghianguyen150612/Endfield-Linux>, target `main`.
- GitHub PR #3: merged 2026-11-02 11:23:32 UTC, merge `6f383c6390462cd4da64a97d372fee739b20fd2c`.
- GitHub PR #4: merged 2026-11-02 14:07:24 UTC, merge `846d9c867bc49946b2dffe15f2d4632cd6b412df`.
- Clone, `git log`, clean status and `git ls-remote origin HEAD refs/heads/main` agreed on supplied baseline `846d9c867bc49946b2dffe15f2d4632cd6b412df`; P031 ancestry is present. Empty workspace contained no work to overwrite.
- Feature branch: `feat/p032-official-game-sources`. No main push, auto-merge, third application executable, ABI change or P033 implementation.
- Initial implementation/test commit: `a320f6f`. Follow-ups `9e588b4` retain typed CLI observations and preservation sentinels; `830d819` binds source previews to P031 directory device/inode as well as path. The full final HEAD is reported by the Draft PR and completion report; this document cannot embed its own commit hash.
- Exactly one Draft PR [#5](https://github.com/nghianguyen150612/Endfield-Linux/pull/5) targets `main`. It remains Draft and unmerged; final-head evidence is also recorded in its description and completion report. Do not merge automatically.

## Delivered scope

Portable typed channel/source/evidence/capability catalog, schema 1 / revision
`2026-11-02.1`; deterministic offline ordering; exact official URLs and manual
provider guidance. Linux-only bounded strict Steam app 4732690 metadata observer,
no-follow/type-before-I/O path handling, explicit partial and unknown states,
local metadata/object token revalidation, headless canonical CLI, ephemeral P031
association previews, focused offline and real-executable CI tests.

P031 schema 1 remains unchanged. No migration or association mutation exists;
original plan/apply/remove CAS stays authoritative. Source labels remain user
assertions unless a matching local Steam metadata observation is supplied. That
observation still does not authenticate binaries, establish game build, or change
compatibility from `not_tested`. No cross-registry atomicity is claimed.

## Evidence and integration limits

Distribution documents observed 2026-11-02, exact publication dates and claim
limits are recorded in [OFFICIAL-GAME-SOURCES.md](../OFFICIAL-GAME-SOURCES.md).
GRYPHLINE update notice: 2026-10-13 12:00; Steam announcement: 2026-10-14 20:00;
timezones not displayed. Landing/store publication dates unknown. Do not replace
these with game release dates. Steam app ID 4732690 is backed by both publisher
announcement and store URL. Epic's storefront slug is not an internal catalog ID.
Local ACF shape was independently checked against steamlocate 2.1.1 author's
published source on 2026-11-03; this establishes format observations only, not
Valve authorization or first-party distribution trust. Developer Wiki access
returned 403 and was not treated as fetched evidence.

All channels support reviewed catalog/manual handoff. Steam additionally supports
one explicit local library or appmanifest selection. Epic/GRYPHLINK metadata
inspection remains unsupported. Every direct acquisition, game update, unknown
API/manifest protocol, account endpoint, launcher installation and execution is
unsupported. Steam owns Steam updates; no GRYPHLINK cross-channel operations.
Chinese and global versions/accounts/CDNs are not equated. Kernel ACE is external
Steam listing information. The supplied Steam Deck Unsupported badge was not
visible in fetched text, so it remains supplied external information rather than
independently revalidated evidence. No Linux gameplay claim follows.

## Local validation

Genuine Rust **1.85.0** was installed for this environment. No fake GTK packages,
substitute binary, or test harness replaces the canonical executable.

- `cargo +1.85.0 fmt --all -- --check`: passed.
- `cargo +1.85.0 check --locked --workspace`: blocked by missing native `glib-2.0.pc` / `gobject-2.0.pc`; not a workspace pass.
- Full workspace Clippy/test/build: attempted; native GTK prerequisites unavailable. Hosted CI must exercise the real app.
- `cargo +1.85.0 test --locked --workspace --exclude endfield-unix`: **733 passed, 0 failed, 4 ignored**, including P027–P031, new 10 source tests, daemon and IPC library regressions.
- `cargo +1.85.0 clippy --locked -p endfield-unix-game-sources --all-targets -- -D warnings`: passed.
- `cargo +1.85.0 clippy --locked --workspace --exclude endfield-unix --all-targets -- -D warnings`: passed.
- `cargo +1.85.0 build --locked --workspace --exclude endfield-unix`: passed.
- `cargo +1.85.0 metadata --locked --no-deps --format-version 1`: passed; exactly `endfield-unix` and `endfield-unixd` are application binaries. Source crate has no GUI, network, IPC or storage dependency.
- `CC=gcc native/optimizer/tests/run-c-tests.sh`: passed including the script's sanitizer/ABI gates.
- `CC=clang native/optimizer/tests/run-c-tests.sh`: attempted, unavailable (`clang: not found`).
- Local Clang and GTK smoke: unavailable (no Clang/native GTK stack); required hosted gates retained.
- `git diff --check`: passed.

## Hosted qualification

[Rust CI run 41550527363](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41550527363)
passed **all three jobs** on initial implementation/documentation head
`06d38507c7497ae5e0355616f7d1f55742d9cda1`. There were no failing checks in that run.
It passed Rust 1.85 formatting, workspace all-target check and Clippy, actual
application build, 10 P032 core tests, all 4 `game_sources_cli` real-executable
tests under seccomp network/child denial, original P031 registry and real CLI
tests, P027–P030 regressions, full workspace tests/build, daemon/IPC gates,
GTK smoke with screenshot upload, Cargo metadata, and independent GCC/Clang
C17 ABI jobs. This is executable CI evidence, not physical qualification.

The follow-up preserves independent pin/prefix/recovery sentinel bytes, retains
typed source observations through the CLI, and additionally requires matching
installation device/inode for P031 previews. The exact **final HEAD and its own
hosted run**, including this documentation commit, are recorded in Draft
[PR #5](https://github.com/nghianguyen150612/Endfield-Linux/pull/5) and the completion
report after checking the run. The earlier successful run above is not a
substitute for that final-head gate. Optional isolated systemd user-manager
qualification remains unavailable because its repository gate is not enabled.

## Changed files and CLI results

The 24 changed files comprise the new `crates/game-sources` Cargo manifest,
portable `src/lib.rs`, Linux `src/steam.rs` and `tests/sources.rs`; workspace
Cargo manifest/lock; application dependency manifest; canonical CLI
`game_sources.rs`, `game_import.rs`, `mod.rs`, `output.rs`; real
`game_sources_cli.rs` tests; `.github/workflows/ci.yml`; README, CHANGELOG,
ROADMAP, GAME-IMPORT, ARCHITECTURE, CLI, STORAGE, SECURITY, TESTING, this manifest
and OFFICIAL-GAME-SOURCES documentation. No P027–P031 implementation crate,
daemon, native C header/library, prefix or runtime persistence format changed.

Hosted real-binary results: `game sources`/`--json`, all three `source show`
variants, nested help, `source check-catalog`, explicit Steam `inspect` and
unchanged `revalidate` exited 0; modified valid metadata returned `changed`
and exit 4; unknown source returned `unknown_source` / 2; Epic/GRYPHLINK
inspection returned `unsupported_source_operation` / 78 without opening the
supplied path. Existing schema-v1 import/apply, source validation and source
label/Steam observation previews succeeded; no source was persisted. Wrong
P031 removal revision returned 4; approved current-revision removal preserved
game bytes. Corrupt/future P031 registries returned 70 and were preserved.
Read-only operations left absent XDG storage absent. No launcher or game was
run. Exact command syntax and evidence URLs are in the source guide.

## Physical qualification and later work

No physical Steam/Endfield installation, RTX 5070, Dell Vostro 3510, mounted
NTFS/exFAT device or actual Endfield payload was inspected. These remain
**UNVERIFIED**. Synthetic/hosted qualification cannot prove launcher format
coverage, game authenticity, Linux support, anti-cheat readiness, rendering,
performance or execution. Observation tokens do not cover payload contents,
remount continuity or adversarial same-user snapshot races.

P033 can consume the separate source evidence and unknown readiness states;
P033 has not started. P034 owns execution, P038 supported game updates. Future
artifact acquisition needs verified protocol, trusted integrity reference,
independent publication authentication, storage policy and installation
semantics. P032 deliberately supplies no speculative endpoint.
