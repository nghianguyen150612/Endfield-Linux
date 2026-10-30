# Runtime pinning and explicit selection (P027)

P027 stores a user's explicit choice of an Endfield-managed Wine or Proton installation. It returns a non-executable selection report. It does not start a runtime, prepare a Wine prefix, launch a game, or prove Endfield compatibility.

## Identity and scopes

The `endfield-unix-runtime-selection` crate is a portable library and depends only on the P021 runtime contracts and P006 storage paths plus small serialization, hashing, locking, and error dependencies. It does not depend on GTK, the daemon, graphics, UMU, or runtime execution.

Machine identity is derived from a validated 32 hexadecimal character Linux `/etc/machine-id` using SHA-256 over the domain string `endfield-unix:machine-identity:v1\0` and the normalized machine-id. The raw value is not returned, persisted, or printed. The result is pseudonymous, not anonymous. Containers or systems without a valid machine-id receive a typed unavailable error; no hostname or random fallback is fabricated. Tests inject deterministic identities.

Profile IDs are caller supplied, 1–96 bytes, restricted to ASCII letters, digits, spaces, `.`, `_`, and `-`, and reject dot-prefixed and parent-like values. A profile scope includes both the machine identity and the profile ID. It does not assert an imported game installation exists.

## Registry and operations

The strict schema-v1 JSON registry lives at `runtime-pins.json` beside the P006 application config under XDG config. It contains a monotonic revision, an unkeyed SHA-256 corruption checksum, and at most 256 unique scope records. The checksum detects accidental/inconsistent edits; it is not a MAC and does not authenticate same-user changes. Unknown fields, malformed JSON, unsupported schemas, invalid IDs/digests, oversized registries, and duplicate scopes fail closed. The registry is separate from the P006 SQLite database, so no database migration or existing preference data change is required.

Updates hold an advisory writer lock, compare the caller's expected revision, write a mode-0600 temporary file, synchronize it, atomically rename it, and synchronize the containing directory. A conflict returns an error. Exact same-scope/runtime/adapter set is idempotent. Replacing another identity requires `--replace` plus `--expected-revision`. Removing a pin preserves runtime files. The lock coordinates cooperating Endfield-Unix writers; same-user programs can still modify home-directory metadata and permissions do not provide cryptographic authenticity.

Managed references bind installation ID, family, provider, project, build, artifact and manifest SHA-256, generation, provenance classification, and SHA-256 of the validated P025 ownership record. P025 checks the record and P026 performs a bounded full baseline audit when a managed identity is explicitly pinned or resolved. List and show do not hash runtime payloads. Unverifiable, partial, failed, or mismatching audit evidence does not establish integrity. Resolution rechecks the exact stored identity; an old digest or matching version label is insufficient.

P027's CLI currently accepts explicitly selected P025-managed installations. P022 external candidates are not yet persisted as pin references: a discovery candidate ID alone is not a security identity, and no stable external structural binding is implemented in this milestone. Arbitrary paths are not accepted.

## Selection, adapters, and evidence

Selection order is explicit operation override (library API), exact machine-plus-profile pin, machine pin, then no selection. A present invalid profile pin blocks machine fallback. The CLI resolve operation uses stored pins and reports missing or stale records without choosing another version. Installing, discovering, auditing, or removing another runtime never changes a pin. Overrides do not write the registry.

Optional adapter preferences are metadata, not evidence of availability. UMU-Proton is valid only for Proton and Managed-Wine only for Wine. Resolution checks UMU presence when UMU-Proton is selected and reports unavailable/unknown status without substituting another adapter. Verified-direct Proton is omitted until the caller can supply qualifying P021 evidence.

Source provenance is copied from P025. User-pinned artifacts stay `UserPinnedLimitedTrust`; a local P026 baseline match is not publisher authentication. P027 records compatibility as not verified. Pinned, integrity-assessed, and known-good are separate concepts. No Endfield compatibility evidence or benchmark is created here.

## CLI

```sh
endfield-unix runtime list-managed
endfield-unix runtime pin list
endfield-unix runtime pin show --scope machine
endfield-unix runtime pin set --scope machine --installation-id INSTALLATION_ID --expected-revision 0
endfield-unix runtime pin set --scope profile --profile test-profile --installation-id INSTALLATION_ID --expected-revision 1
endfield-unix runtime pin resolve --profile test-profile
endfield-unix runtime pin remove --scope profile --profile test-profile --expected-revision 2
```

Use `--replace` with `set` to replace an existing scope after inspecting its current revision. Profile IDs are synthetic/user-defined; they are not game installation claims. List/show are metadata-only. Resolve hashes the managed installation via P026, so it may be expensive for large runtimes. No CLI path launches Wine, Proton, UMU, or a game.

## Limits and qualification

The registry is user-owned and cannot defend against hostile processes running as the same user. The selection crate's resolver accepts a validation callback so callers must revalidate; a successful report is not a P034 execution authorization. Full external candidate pinning, broad cross-platform machine identity, compatibility evidence matching to future imported profiles, and richer audit outcome display remain future work. P028 can consume pin records as preferences but must perform its own identity and integrity checks.

Machine-specific Arch desktop and Dell Vostro validation was not performed. Real known-good Endfield compatibility remains **UNVERIFIED**.
