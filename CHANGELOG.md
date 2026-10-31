# Changelog

## Unreleased

### Added

- P028 isolated prefix management: explicit machine/profile-scoped managed
  directories, P027 runtime association, read-only preflight and inspection,
  recovery, registry-only removal, and confirmed managed-data deletion.
- `prefix init` returns `initialization_unsupported` until a Wine/Proton/UMU
  initialization adapter can be validated. No runtime or game process is
  started, and Endfield compatibility remains unverified.

## P029 — draft backup/recovery

Added managed-prefix snapshot inventories and SHA-256 verification, private staging, durable restore evidence and original-data preservation, explicit snapshot removal, completed-restore archival and separate P027 CAS pin recovery. Restricted to Created prefixes and ordinary files/directories. Hosted qualification is pending; see [the P029 manifest](docs/manifests/PROMPT029_MANIFEST.md).
