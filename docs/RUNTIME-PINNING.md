# Runtime pinning and explicit selection (P027)

P027 stores a user's explicit Wine or Proton preference for this machine or one machine-bound profile. It returns a non-executable selection preview. It does not start a runtime, prepare a Wine prefix, launch a game, or prove Endfield compatibility.

## Identity and scope

`endfield-unix-runtime-selection` is a library-only portable crate. Linux-specific discovery and installation revalidation stay at the CLI boundary through P022, P025, and P026 APIs. The selection core has no GTK, daemon, graphics, UMU, or process-execution dependency.

On Linux, machine scope uses SHA-256 over the domain string `endfield-unix:machine-identity:v1\0` and a validated, normalized 32-hex-character `/etc/machine-id`. The raw value is not persisted, returned, or printed. This is a pseudonymous identifier, not anonymous data. Missing, unreadable, symlinked, or malformed machine IDs produce an unavailable/invalid result; no hostname, random, or cross-host fallback is fabricated. Tests inject deterministic machine IDs.

Profile identifiers are caller supplied, 1–96 bytes, restricted to ASCII letters, digits, spaces, `.`, `_`, and `-`, and reject dot-prefixed or parent-like values. A profile scope is `(machine identity, profile ID)`. It does not assert that a game installation or imported P031 profile exists.

## Registry and operations

The strict JSON pin registry is `runtime-pins.json` beside P006 config in the XDG config directory. Schema v2 contains a monotonic revision, a SHA-256 corruption checksum, and at most 256 unique scope records. Schema v1 managed-only records migrate in memory on read and are written as v2 at the next explicit mutation. Reads do not rewrite data. Unknown fields, malformed JSON, unsupported versions, invalid IDs/digests, oversized files, and duplicate scopes fail closed without resetting the registry. The checksum detects accidental or inconsistent edits; it is not a MAC and does not authenticate same-user changes. P006 SQLite and its existing preference data are unchanged.

Writes use a private parent directory and mode-0600 lock and temporary files, an advisory writer lock, caller-supplied expected revision, atomic replacement, file synchronization, and directory synchronization. A stale revision returns a conflict. Setting the exact same reference and adapter is idempotent. Replacing a different reference requires `--replace` and the expected revision. Removing a pin removes only that record and leaves runtime files untouched. Same-user processes remain inside the trust boundary; file permissions are not cryptographic evidence.

Managed references bind the exact P025 installation ID, family, provider/project, build, artifact and manifest SHA-256, generation, provenance, and digest of the validated ownership record. Explicit set and resolve invoke P025 identity checks and the P026 bounded full baseline audit. List and show do not hash runtime payloads. Complete baseline mismatch, missing/unverifiable baseline, partial audit, audit failure, missing installation, or changed ownership identity blocks selection and preserves the pin.

External references begin with an explicit P022 candidate selection. The transient path-derived candidate ID is shown by `diagnose runtimes` and accepted only as an explicit selection handle; it is not persisted as the runtime identity. The stored reference uses P022's canonical observed location, provider, family, conservatively reported lineage and its evidence basis, optional build, source category, ownership classification, architectures, structural shape, and a SHA-256 fingerprint of bounded structural metadata and filesystem observations. The path is retained privately for exact reinspection and is not printed by pin list/show/resolve. Non-UTF-8 locations cannot be stored. The fingerprint is not an artifact digest or full integrity audit. Resolution asks P022 to inspect only the pinned location and blocks if it disappears, changes, or is no longer recognized. External files are never modified. P022 source labels describe discovery provenance and do not authenticate Steam, Heroic, Lutris, Wine, Proton, or a publisher.

## Selection, adapters, and evidence

Selection order is an explicit one-operation override through the library API, exact machine-plus-profile pin, machine pin, then no selection. The CLI does not currently expose an override flag. An invalid or unavailable profile pin blocks machine fallback. A missing or stale machine pin also produces no substitute. Discovery, installation, audit, and provider ordering never change stored pins. An override does not write the registry.

Adapter preferences are metadata and are checked separately from adapter availability. UMU-Proton requires Proton; managed Wine and external Wine preferences require the corresponding Wine reference. An unavailable UMU remains an unavailable adapter and does not select direct Proton or system Wine. Adapter preference does not run or prove availability of the adapter.

P025 provenance is preserved for managed artifacts: user-pinned artifacts remain `UserPinnedLimitedTrust`, and independent publisher authentication is retained only where P025 actually established it. External references preserve unverified P022 source provenance and unknown/external ownership information. P026 local baseline match is not publisher authentication. All pins remain `CompatibilityNotVerified`; P027 does not persist or invent P021 compatibility evidence. The library exposes `evaluate_pinned_compatibility` over P021's `RuntimeIdentity`, `CompatibilityScope`, and `CompatibilityEvidence` types. It accepts only an exact managed build and artifact digest, then assesses only evidence whose complete scope matches, including adapter, platform, architecture, game build, prefix configuration, driver requirements, and environment restrictions. P022's structural snapshot is not an artifact digest and cannot bind P021 integrity evidence. This evaluation does not update the stored pin or construct `KnownGoodRuntimeReference`; static capability and user-reported evidence retain their P021 strength. P027 has no Endfield compatibility evidence.

## CLI

Inspect external candidates first. Candidate IDs are local discovery handles and may change if the observed location changes:

```sh
endfield-unix diagnose runtimes
endfield-unix diagnose runtimes --root /absolute/path/to/candidate-parent
```

Manage managed or external pins using the exact current registry revision:

```sh
endfield-unix runtime pin list
endfield-unix runtime pin show --scope machine
endfield-unix runtime pin set --scope machine --installation-id INSTALLATION_ID --expected-revision 0
endfield-unix runtime pin set --scope profile --profile test-profile --installation-id INSTALLATION_ID --expected-revision 1
endfield-unix runtime pin set-external --candidate-id CANDIDATE_ID --scope profile --profile test-profile --expected-revision 2
endfield-unix runtime pin set-external --candidate-id CANDIDATE_ID --scope machine --root /absolute/path/to/candidate-parent --expected-revision 0
endfield-unix runtime pin resolve --profile test-profile
endfield-unix runtime pin remove --scope profile --profile test-profile --expected-revision 3
```

Repeat the same `--root` used during discovery for custom candidates. Use `--replace` only after inspecting the pin and current revision. `show` reports stored metadata without revalidation. `resolve` fully audits a managed runtime with P026; for external runtimes it performs bounded P022 structural identity revalidation and reports integrity as not assessed. Neither operation is launch authorization. No command executes Wine, Proton, UMU, or a game.

## Tests and limits

Deterministic tests cover machine/profile scope, schema migration, strict serialization, checksums, duplicate detection, idempotency, replacement/removal, compare-and-swap conflicts, concurrent writers, and corruption/symlink rejection. P021 evidence tests accept an exact managed build and full target scope, reject different runtime builds, host platforms, architectures, and game builds, retain user reports and static checks below verified-test strength, and reject matching evidence for external structural-only references. Synthetic P022 tests cover exact external location and structural identity change. The process-level CLI test covers managed side-by-side preservation and P026 tamper detection; it also creates an external P022 fixture, explicitly pins it, resolves it in another process, detects an in-place change, removes only the pin, and confirms the runtime files remain.

The registry is user-owned and cannot defend against hostile processes running as the same user. Selection callbacks must revalidate before use; a successful report is not P034 execution authorization. Future launch code must perform its own identity and integrity checks immediately before execution. P028 consumes these preferences while preserving explicit selection and revalidating independently.

Managed and external pinning, typed P021 evidence evaluation, and the actual CLI integration passed all hosted jobs in [run 41022089042](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41022089042) at `20c7640c7101a142e1855162a26fe7b0ec94789b`. Detailed gates and limitations are recorded in [TESTING.md](TESTING.md). No physical Arch desktop or Dell Vostro runtime checks were performed. Real known-good Endfield compatibility remains **UNVERIFIED**.

## P028 prefix association

P028 consumes the explicit machine/profile selection without changing this registry. Each created prefix records its exact P027 runtime reference, pin ID/revision, scope source, and adapter preference. Plan/create revalidate managed identity and the P026 baseline or revalidate an external reference through P022. A changed pin does not migrate an existing prefix, and a blocking profile pin does not fall back to the machine pin. Prefix deletion never removes the runtime installation or pin. See [isolated prefix management](PREFIX-MANAGEMENT.md).

## P029 association recovery

Prefix snapshot restoration preserves its exact historical association and never calls PinStore mutation APIs. Separately approved recovery reconstructs only that association’s original pin scope through public P027 CAS APIs after exact P025/P026/P022 revalidation. It is not a full registry import. Independent journals handle interrupted pin commits; there is no cross-registry atomicity. See [backup/recovery](BACKUP-RECOVERY.md).

## P029A observational pin snapshots

`PinStore::load_read_only` observes one bounded, checksummed registry using no-follow descriptor-relative access on Unix. It creates no parent directories or locks, changes no permissions and does not persist migration. Unsafe ownership, mode, aliases and malformed content are rejected. A missing registry returns the empty typed snapshot. Restore planning uses this API and P027 resolution with existing exact P025/P026/P022 validation; a present blocking profile pin never falls back. The observed revision does not reserve state: mutation still requires P027 CAS. Existing `load` and mutation contracts are unchanged.


P029B limited rollback checks the original prefix record while holding its registry lock and does not write the P027 registry. The saved association is retained exactly; separate pin recovery remains independently approved.

P029B deletion replay is scoped to backup snapshots and never mutates runtime installations or P027 pins. Deletion journals that reference a snapshot conflict with restore and pin recovery, and any observed conflict blocks replay. Read-only `prefix recovery pins` reports journal markers only; it does not infer that the current pin still matches saved state.
