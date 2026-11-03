# P032 implementation and qualification manifest

## Verified baseline and workflow

- Repository: <https://github.com/nghianguyen150612/Endfield-Linux>, target `main`.
- GitHub PR #3: merged 2026-11-02 11:23:32 UTC, merge `6f383c6390462cd4da64a97d372fee739b20fd2c`.
- GitHub PR #4: merged 2026-11-02 14:07:24 UTC, merge `846d9c867bc49946b2dffe15f2d4632cd6b412df`.
- Clone, `git log`, clean status and `git ls-remote origin HEAD refs/heads/main` agreed on supplied baseline `846d9c867bc49946b2dffe15f2d4632cd6b412df`; P031 ancestry is present. Empty workspace contained no work to overwrite.
- Feature branch: `feat/p032-official-game-sources`. No main push, auto-merge, third application executable, ABI change or P033 implementation.
- Implementation/test commit: `a320f6f` (full final HEAD is reported by the Draft PR and completion report; this document cannot embed its own commit hash).
- Exactly one Draft PR targets `main`; hosted final-head evidence is recorded below as it becomes available. Do not merge automatically.

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
- Non-GTK workspace Clippy/build, metadata and GCC C17 checks: tracked below after completion.
- Local Clang and GTK smoke: unavailable (no Clang/native GTK stack); required hosted gates retained.
- `git diff --check`: passed.

## Hosted qualification

Pending initial Draft PR run. Required final-head CI includes Rust 1.85 format,
workspace all-target check and Clippy, actual app build, 10 P032 core tests,
`game_sources_cli` actual executable under seccomp network/child denial, original
P031 real CLI and registry preservation tests, P027–P030 regressions, full
workspace tests/build, daemon/IPC gates, GTK smoke/screenshot, metadata, and
independent GCC and Clang C17 ABI jobs. Optional isolated systemd user-manager
qualification remains unavailable unless explicitly enabled by repository CI.

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
