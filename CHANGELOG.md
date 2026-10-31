# Changelog

## Unreleased

### Added

- P028 isolated prefix management: explicit machine/profile-scoped managed
  directories, P027 runtime association, read-only preflight and inspection,
  recovery, registry-only removal, and confirmed managed-data deletion.
- `prefix init` returns `initialization_unsupported` until a Wine/Proton/UMU
  initialization adapter can be validated. No runtime or game process is
  started, and Endfield compatibility remains unverified.
