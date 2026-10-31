# Changelog

## Unreleased

### Added

- P028 isolated prefix management: explicit machine/profile-scoped managed
  directories, P027 runtime association, read-only preflight and inspection,
  recovery, registry-only removal, and confirmed managed-data deletion.
- `prefix init` returns `initialization_unsupported` until a Wine/Proton/UMU
  initialization adapter can be validated. No runtime or game process is
  started, and Endfield compatibility remains unverified.

## P029 — hosted qualified backup/recovery

Added managed-prefix snapshot inventories and SHA-256 verification, private staging, durable restore evidence and original-data preservation, explicit snapshot removal, completed-restore archival and separate P027 CAS pin recovery. Restricted to Created prefixes and ordinary files/directories. The merged P029 implementation passed hosted Rust/GTK, actual CLI, GCC, and Clang gates in [run 41186534556](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41186534556). Historical test and fault evidence remains in [the P029 manifests](docs/manifests/PROMPT029_MANIFEST.md).

### P029A remediation

Fixed the actual executable's P028-only parser by sharing the prefix operation enum with dispatch. Added complete nested parser/help coverage and real-executable backup/restore/recovery tests. Restore planning now assesses current P027 state through a new strictly observational API. Incomplete deletions have a read-only inspection command; corrupt and premature completion evidence is rejected before mutations. Added copy/unlink process-exit and staging-replacement regressions. This is P029 hardening, not a new milestone; unsupported filesystem objects and real-prefix qualification limits remain.


P029B adds bounded, journaled deletion replay for verified empty and nonempty snapshots, with per-entry identity and hash checks, explicit confirmation, revision checking and interruption recovery at each unlink boundary. It adds read-only deletion-journal/history/pin-journal inspection and limited explicit rollback for published, uncommitted schema-2 restores. The original and displaced trees remain preserved by rollback, and P027 pins stay unchanged. Legacy deletion remnants, legacy restore rollback and post-registry-commit rollback stay blocked. Final qualification is tracked in the P029B manifest.

## P030 — draft compatibility diagnostics

Adds a portable typed assessment core and headless `diagnose compatibility` command. The report combines exact P027 selection and P022/P025/P026 revalidation, P013 PCI/DRM/Vulkan observations, P028 lifecycle and runtime association, and P029 metadata-only recovery state. It never executes a runtime or game, changes a pin, initializes a prefix, or claims Endfield compatibility. Final hosted qualification is pending; see [the P030 manifest](docs/manifests/PROMPT030_MANIFEST.md).
