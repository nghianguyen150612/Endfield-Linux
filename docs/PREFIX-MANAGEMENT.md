# Isolated prefix management (P028)

P028 adds a headless, library-backed foundation for explicitly managed Wine/Proton prefix directories. It creates and inspects empty directories only. It does not run Wine, Proton, UMU, `wineboot`, a launcher, or a game, and it does not establish Endfield compatibility.

## Ownership and identity

`endfield-unix-prefix-management` is a library-only crate with a mockable filesystem adapter. `PrefixId` accepts 1–96 ASCII letters, digits, `_`, and `-`. Machine and profile scopes reuse P027's `MachineId` and `ProfileKey`; machine IDs are derived by P027 from validated Linux `/etc/machine-id`, while profile IDs remain caller-supplied identifiers and do not claim to be full P031 profiles. Multiple independent IDs are supported.

Endfield-Unix owns only entries below the P006 data directory:

```text
$XDG_DATA_HOME/endfield-unix/prefixes/
  registry.json
  locks/registry.lock
  locks/prefix-<id>.lock
  data/<id>/
```

When `XDG_DATA_HOME` is unset, P006's XDG default applies. The path is resolved by the existing storage contract, not by username-specific assumptions. Managed directories use mode `0700`; registry, lock, and temporary metadata files use mode `0600`. Linux access uses directory file descriptors, no-follow opens, mount identity checks, filesystem ownership checks, and bounded registry reads. Unsafe symlinks, unexpected filesystem aliases, non-owned app storage, and collisions with known Steam, Bottles, Lutris, Heroic, or conventional Wine roots fail closed. No external prefix is imported, adopted, or modified.

The registry is a separate schema-v1 JSON file. Each record preserves ID, machine/profile scope, relative path identity, ownership, invoking UID, exact P027 runtime reference, selection source and pin revision, adapter preference, lifecycle, filesystem device/inode identity, timestamps, generation, and optional structured initialization evidence (adapter ID, exact runtime-reference digest, evidence digest, outcome, and observation time). Partial evidence can remain attached to incomplete/recovery records; only the `CompletedAndVerified` outcome can accompany `Initialized`. The bounded registry has an accidental-corruption SHA-256 checksum; it is not a MAC and does not authenticate same-user edits. Unknown fields, malformed or corrupt content, and unsupported schema versions are preserved and reported instead of reset. There is no automatic schema migration in P028.

## Lifecycle and operations

- **Planned** is a read-only operation result. It validates scope, runtime pin, storage location and permissions, destination collisions, registry revision, and available free-space information. It does not create directories or metadata.
- **Created** means the private managed directory and registry record exist. It does not mean the Wine prefix is initialized.
- **Initialized** is reserved for verified future evidence. P028 never writes this state.
- **Incomplete/RecoveryRequired** records journal an interrupted mutation. `recover` only accepts a matching, empty directory; it will not adopt or clear non-empty data.
- **Missing** and **Blocked** are observed states. They do not authorize automatic recreation or overwrite.

Create is idempotent only for the same registered identity, scope, and runtime association. Per-registry and per-prefix advisory locks serialize mutations, and each mutation checks an expected registry revision. Creation first persists a recovery journal, creates the directory with restrictive permissions, then commits the created record. A failure leaves evidence for inspection or recovery.

`remove` removes only the registry entry and preserves the directory bytes. The resulting unregistered directory is not automatically adopted by a later create; that create reports a destination collision. P028 does not provide a data-deletion command for an unregistered orphan, so users must choose `delete` before unregistering when they want managed data removed through the app. `remove` prints the preserved path for reference. `delete` is a separate destructive operation. It requires the exact path printed by `plan` or `show`, checks the registry revision, ownership, device/inode and mount identity, symlink boundaries, locks, and process use before deletion. Linux process-use inspection looks for an exact `WINEPREFIX` in visible same-UID `/proc/*/environ`; an unreadable or unverifiable scan blocks deletion. This is a conservative signal, not a complete process handle scan. If an unlink is interrupted, the registry remains marked for recovery and the directory is not silently recreated. Runtime installations and P027 pins are never deleted by prefix operations.

## P027 runtime association

Plan and create resolve the explicit machine or machine-bound profile selection through P027. A blocking or stale profile pin does not fall back to a machine pin. Managed references are revalidated with P025 ownership identity and the P026 full baseline audit. External references are revalidated at the exact P022 location using P022's structural fingerprint; that fingerprint is not a full artifact integrity check. The record preserves the exact runtime reference and pin metadata used for creation.

Inspection and validation recheck the stored reference. They report when the current pin changed while preserving the prefix's original association; they never migrate a prefix or change a pin. A stale or removed runtime remains visible as stale/unavailable and does not cause a substitute selection. Runtime family and adapter-preference mismatches are rejected. Compatibility remains unverified.

## Wine initialization boundary

`prefix init` is exposed as an explicit operation but returns exit status 5 and `initialization_unsupported`. No validated Wine, Proton, or UMU initialization adapter exists in this milestone. No process is started, no `WINEPREFIX` is exported, and no directory is presented as initialized. A future initializer must have validated invocation semantics, argument-vector and environment isolation, bounded execution/cancellation, sanitized diagnostics, and verifiable completion evidence before the lifecycle can advance.

## CLI

These commands do not initialize GTK, require the daemon, run runtime processes, or launch a game:

```sh
endfield-unix prefix list
endfield-unix prefix show --id game
endfield-unix prefix plan --id game --scope profile --profile test-profile --expected-revision 0
endfield-unix prefix create --id game --scope profile --profile test-profile --expected-revision 0
endfield-unix prefix validate --id game
endfield-unix prefix init --id game
endfield-unix prefix recover --id game --expected-revision REV
endfield-unix prefix remove --id game --expected-revision REV
endfield-unix prefix delete --id game --expected-revision REV --confirm-path /exact/path/from/plan
```

`create` and `plan` require a valid P027 runtime pin. Profile selection is explicit. `plan` prints the prefix registry revision, managed destination, P027 pin identity/revision, runtime family/build, available bytes (or `unknown`), and whether mutation is planned. Use that exact target and current revision for deletion. `remove` preserves all bytes; `delete` removes only a verified managed directory. Exit 3 indicates a missing/unregistered prefix, exit 4 an unsafe, stale, conflicting, or unverifiable condition, and exit 5 unsupported initialization. Parser errors remain exit 2; output I/O failures remain exit 74.

## Qualification boundary

Synthetic tests cover read-only plans, lifecycle transitions, idempotence, concurrent creates, registry conflicts/corruption/schema handling, permission and symlink failures, interrupted creation, stale runtime identity, external-data preservation, active-use detection, and no fake initialization. The process-level CLI integration uses synthetic inert managed and external runtimes in isolated XDG roots. No real Wine, Proton, UMU, game, or hardware compatibility execution is part of P028. Physical Arch and Dell Vostro verification remains **UNVERIFIED**.

## P029 snapshot extension

The same library now exposes backup/recovery types and Linux operations. P028 mutations refuse outstanding restore evidence, including completed evidence until explicit release archives it without removing original bytes. Existing commands and initialization_unsupported remain. See [backup/recovery](BACKUP-RECOVERY.md).
