# Changelog

P033 merged with outstanding acceptance debt; it adds explicit read-only Steam metadata observation and more precise selected-storage failure states.

P033 merged with outstanding acceptance debt; the preliminary static dependency report remains incomplete and unqualified. See [status](docs/DEPENDENCY-DIAGNOSTICS.md).

## Unreleased

### Added

- P034 adds schema-v2 runtime/prefix/game identity binding, a managed Wine
  native-argv preview with no shell or process creation, a deterministic
  P028 initialization assessment, P029 pending/unknown recovery reporting,
  and expanded CLI/GTK preflight. Managed Wine preparation remains
  non-executable; external Wine, UMU execution, direct Proton, prefix
  initialization, P033 dependency readiness, anti-cheat compatibility, and
  real gameplay remain unsupported or unknown.

- P028 isolated prefix management: explicit machine/profile-scoped managed
  directories, P027 runtime association, read-only preflight and inspection,
  recovery, registry-only removal, and confirmed managed-data deletion.
- `prefix init` returns `initialization_unsupported` until a Wine/Proton/UMU
  initialization adapter can be validated. No runtime or game process is
  started, and Endfield compatibility remains unverified.
- P031 adds explicit headless registration, inspection, bounded revalidation,
  and metadata-only unregistering for one user-selected existing game directory.
  Game identity remains user-declared and unverified; no game files are read,
  hashed, changed, or executed.

## P032 — verified official source integration

Adds a versioned offline Steam/Epic/GRYPHLINK catalog with exact evidence and
manual guidance, explicit bounded Steam app 4732690 observation/revalidation,
and headless read-only P031 association previews. No direct acquisition,
launcher execution, source persistence/migration, updates or Linux compatibility
claim is added. P031 PR [#4](https://github.com/nghianguyen150612/Endfield-Linux/pull/4)
is verified merged at `846d9c867bc49946b2dffe15f2d4632cd6b412df`. See
[official sources](docs/OFFICIAL-GAME-SOURCES.md) and the
[P032 qualification manifest](docs/manifests/PROMPT032_MANIFEST.md).

## P029 — hosted qualified backup/recovery

Added managed-prefix snapshot inventories and SHA-256 verification, private staging, durable restore evidence and original-data preservation, explicit snapshot removal, completed-restore archival and separate P027 CAS pin recovery. Restricted to Created prefixes and ordinary files/directories. The merged P029 implementation passed hosted Rust/GTK, actual CLI, GCC, and Clang gates in [run 41186534556](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41186534556). Historical test and fault evidence remains in [the P029 manifests](docs/manifests/PROMPT029_MANIFEST.md).

### P029A remediation

Fixed the actual executable's P028-only parser by sharing the prefix operation enum with dispatch. Added complete nested parser/help coverage and real-executable backup/restore/recovery tests. Restore planning now assesses current P027 state through a new strictly observational API. Incomplete deletions have a read-only inspection command; corrupt and premature completion evidence is rejected before mutations. Added copy/unlink process-exit and staging-replacement regressions. This is P029 hardening, not a new milestone; unsupported filesystem objects and real-prefix qualification limits remain.


P029B adds bounded, journaled deletion replay for verified empty and nonempty snapshots, with per-entry identity and hash checks, explicit confirmation, revision checking and interruption recovery at each unlink boundary. It adds read-only deletion-journal/history/pin-journal inspection and limited explicit rollback for published, uncommitted schema-2 restores. The original and displaced trees remain preserved by rollback, and P027 pins stay unchanged. Legacy deletion remnants, legacy restore rollback and post-registry-commit rollback stay blocked. Final qualification is tracked in the P029B manifest.

## P030 — hosted-qualified compatibility diagnostics

Adds a portable typed assessment core and headless `diagnose compatibility` command. The report combines exact P027 selection and P022/P025/P026 revalidation, P013 PCI/DRM/Vulkan observations, P028 lifecycle and runtime association, and P029 metadata-only recovery state. It never executes a runtime or game, changes a pin, initializes a prefix, or claims Endfield compatibility. PR #3 merged at `6f383c6390462cd4da64a97d372fee739b20fd2c`; final-head hosted qualification passed in [CI run 41207736864](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41207736864). See [the P030 manifest](docs/manifests/PROMPT030_MANIFEST.md).
