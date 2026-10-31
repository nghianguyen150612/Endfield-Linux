# Prefix snapshots and recovery (P029 draft)

P029 extends the P028 headless library and `endfield-unix prefix` CLI. It never executes Wine, Proton, UMU, wineboot or a game. This implementation is restricted to registered **Created**, uninitialized prefixes on the same machine and exact profile scope. Initialized-prefix evidence restoration is unsupported and blocked, rather than fabricated. Physical-device and real-prefix qualification remain UNVERIFIED.

## Version 1 storage

P006 resolves the application data directory. Existing `prefixes/registry.json`, `prefixes/locks/` and `prefixes/data/<id>/` remain unchanged. The additional layout is:

```text
backups/
  owner.json
  staging/<backup-id>/{intent.json,manifest.json,payload/}
  snapshots/<backup-id>/{intent.json,manifest.json,payload/}
  restores/<prefix-id>/{intent.json,staged.json,payload/,completed.json}
  history/<prefix-id>-<restored-inode>/...
  pin-recovery/<backup-id>-<expected-pin-revision>/{intent.json,completed.json}
  deleting/<backup-id>/...
```

Files appear only at their relevant durable transition. Directories are initially 0700, metadata is 0600. The root ownership marker records schema 1, UID, and root and application-parent device/inode identities. An existing backup root without that marker is not adopted. A crash before marker persistence leaves an explicitly blocked root for inspection. No destination argument is accepted. Moving these directories or importing them across machines is unsupported.

There is deliberately no second mutable backup registry: the sorted, validated snapshot directory names are the publication index. Each snapshot carries its own versioned manifest. Thus no filesystem/backup-registry dual commit is required. Published snapshots without readable valid manifests are reported as errors, never silently reconstructed or discarded. Directory names accept only the P028 identifier alphabet. Reads never create storage, lock files or registries.

## Inventory and integrity

A schema-1 manifest records stable backup ID, the complete validated P028 source record (including machine/profile scope, lifecycle, filesystem identity, exact P027 association, pin revision and selection provenance), prefix registry revision, timestamp, completion state, observation limits, sorted traversal inventory and total logical bytes. Entries carry relative UTF-8 path, regular-file/directory type, mode, mtime with nanoseconds, size and regular-file SHA-256. Directory digests are empty; inventory equality verifies directory presence and metadata.

Only ordinary, owned regular files and directories are supported. All symlinks (including Wine drive links), hardlinked files, detected sparse files, devices, sockets, FIFOs, extended attributes/ACLs, special permission bits, non-UTF-8 names, mount crossings and unverifiable ownership are rejected. The sparse policy uses Linux allocated-block accounting; it is conservative, not an extent-preservation implementation. Atime, ctime, birthtime, inode numbers, extended metadata and exact allocation are not restored. UID remains the invoking owner. No full POSIX metadata preservation is claimed.

Bounds are 100,000 inventory entries, depth 64, relative path 4096 bytes, logical payload 8 GiB and JSON metadata 32 MiB. Operations require known free space plus a 64 MiB reserve; these are per-operation bounds, not automatic retention. Actual ENOSPC propagates as failure and staging remains available for inspection. There is no retention pruning.

Verification reads all supported payload entries again, compares types, metadata, lengths, hashes, exact inventory and totals, and checks manifest schema, ownership, source record and authorized scope. Unexpected snapshot-root entries are unsafe. The typed integrity vocabulary includes Verified, Corrupt, Incomplete, Unsupported, Missing, Unsafe and Unverifiable; lower-level ownership/I/O errors remain typed errors. Not every OS error maps to a single integrity label. A manifest's persisted Verified field alone never authorizes restore: restore performs current full verification.

SHA-256 detects accidental corruption. The manifest is editable by the same user and is not authentication, a signature or proof of publisher trust.

## Consistency and locks

Plans check P028 identity/lifecycle, registry revision, bounded inventory, space, exact scope and active-use observation. Mutations take the existing prefix registry lock followed by the prefix lock; this serializes P028 operations too. Unknown or unreadable `/proc` assessment fails closed, with no force bypass. The scan sees visible same-UID processes whose environment contains the exact WINEPREFIX. It does not prove absence of all writers.

Backup takes repeated source inventories and hashes around copying, verifies staging, repeats process assessment and rechecks source identity before durable manifest publication. This supports offline cooperative use, not a kernel filesystem snapshot or exclusion of arbitrary same-user writers. Descriptor-relative no-follow opens, mount checks, pre/post file metadata, reopened entry identities and checked counters detect supported classes of replacement/change. Malicious same-user modifications remain outside the authenticity boundary.

## Restore transaction

`restore plan` verifies the entire snapshot and existing destination; it reports existing-data replacement, bytes, revision, source metadata and the exact confirmation string. The associated runtime is restored even if it later becomes stale; the P028 `validate` command reports readiness with the existing P025/P026/P022 revalidation contracts. P029A assesses the current P027 machine/profile selection through a read-only, no-follow registry observation. JSON reports the observed pin revision, selection source/state, whether its exact runtime/pin/adapter association matches the saved one, independent validation of the saved reference, and saved-adapter availability. A corrupt or unsafe pin registry reports Unverifiable. A blocking profile pin stays blocked without machine fallback. This is an observation, not launch authorization or a reservation of that revision. Prefix-only restoration still leaves every P027 pin unchanged.

Apply checks the prefix revision and exact `restore:<prefix-id>:<backup-id>` confirmation. It durably writes original-record intent, copies into private transaction payload, fully verifies it and records staged device/inode identity. After process-use, registry and destination checks, Linux renameat2 RENAME_EXCHANGE swaps the verified payload and live directory. This requires the same mount and kernel support. No cross-filesystem fallback is attempted. The original directory now remains at `restores/<id>/payload`; it was never deleted.

Publication and prefix-registry CAS are separate durable steps. Recovery compares the live and preserved identities with the journal, verifies the restored payload, and commits the original snapshot lifecycle/runtime association into P028. The expected revision, exact original record and staged inode distinguish pre-publication, published-before-registry, registry-committed and completed states. Recovery refuses ambiguous or changed evidence. Repeated successful recovery is idempotent. Registry revisions must not have advanced through unrelated operations; an unexpected revision blocks inspection rather than guessing.

`recovery release` archives a completed transaction into history and preserves all original bytes. Until release, even completed restore evidence blocks further P028 mutations and further restores for that prefix. There is no original-data deletion operation. Archive names use the preserved journal's restored inode; collisions block rather than overwrite. No automatic rollback is offered. Forward recovery completes publication; the separately approved limited rollback described below is available only before the registry commit. Preserved originals remain available for inspection.

## Runtime-pin recovery

Prefix-only restore has no call to P027 mutation APIs and does not change pins. It preserves the original exact association, family, pin ID/revision, adapter preference and selection source. Stale references remain stale without fallback or runtime migration.

`recovery pin` is separate approval, using `pin:<backup-id>` and the expected **P027** revision. It reconstructs the saved association's original machine/profile pin scope; a machine pin inherited by a profile remains a machine pin. It validates the exact runtime through existing P025/P026 or exact-location P022 checks before invoking P027's public `PinStore::set`. It never rewrites P027 registry bytes directly. UMU availability must be established for UMU-Proton. No runtime installation, family conversion, system Wine substitution or compatibility evidence is created.

A separate private intent and completion marker journal the P027 CAS. Repeating the same operation after interruption recognizes only the exact expected successor revision and saved pin identity. Changed intervening state blocks. This restores association metadata, not a full historical P027 registry or historical pin timestamps. There is no atomic transaction across prefix and pin registries; the user must approve each operation independently.

## CLI and recovery procedure

```sh
endfield-unix prefix backup list
endfield-unix prefix backup pending
endfield-unix prefix backup deletions
endfield-unix prefix backup plan --id game
endfield-unix prefix backup create --id game --expected-revision REV --confirm
endfield-unix prefix backup show --backup-id BACKUP
endfield-unix prefix backup verify --backup-id BACKUP
endfield-unix prefix backup recover --id game --backup-id BACKUP --confirm
endfield-unix prefix restore plan --id game --backup-id BACKUP
endfield-unix prefix restore apply --id game --backup-id BACKUP --expected-revision REV --confirm restore:game:BACKUP
endfield-unix prefix recovery status --id game
endfield-unix prefix recovery run --id game --expected-revision REV --confirm
endfield-unix prefix recovery release --id game --expected-revision REV --confirm release:game
endfield-unix prefix recovery pin --backup-id BACKUP --expected-pin-revision PIN_REV --confirm pin:BACKUP
endfield-unix prefix backup delete --backup-id BACKUP --confirm delete:BACKUP
```

These commands are headless and do not start the daemon. Parser errors use exit 2, backup/recovery refusal uses exit 4, success uses exit 0, output failures retain exit 74. P028's commands and initialization_unsupported contract remain intact. Detailed show/plan JSON includes private source association metadata; do not publish it without review.

After interruption, inspect `backup pending` and `recovery status`. Fully completed backup staging can be verified and published with `backup recover`. Incomplete/corrupt staging remains preserved and blocked for manual inspection. Restore staging with a durable verified journal can resume; publication with an uncommitted registry can finalize. Missing/corrupt journals and altered identities block. Never replace registry metadata, move a payload over the live prefix, or recursively delete preserved data to bypass that result.

Snapshot deletion verifies the snapshot, refuses restore/pin-recovery evidence, durably retires it into `deleting/`, rechecks identity and inventory, then removes only no-follow verified application-owned entries. Partial deletion is retained in deleting and blocked for manual inspection; `backup deletions` now provides read-only inspection of retired snapshots: Incomplete for a safe remaining subset, Unsafe for changed/unexpected entries, and Unverifiable for missing/corrupt manifest evidence. It never follows manifest paths or resumes unlink. Automated deletion-resume and CLI listings of history/pin journals remain unsupported. Source prefixes, runtime installations and P027 pins are preserved. There is no automatic discard of incomplete staging.

## Qualification limits

Synthetic tests include binary/hidden/nested roundtrip, empty and distinct snapshots, read-only queries, corruption, scope/revision/confirmation failures, unsupported links/sparse files, quotas, staged publication recovery, original preservation, explicit pin CAS recovery, pin byte preservation, archival and deletion retirement. Test-only child processes call _exit at seven durable backup/restore boundaries and recover with OS-released locks. These are interruption tests, not power-loss/storage-device fault tests. Full mount-race, adversarial replacement, non-owned UID, real ENOSPC and every mid-copy/unlink boundary are not yet qualified. See the P029 manifest for current gate evidence and acceptance gaps.

## P029A parser and acceptance hardening

P029A is remediation within P029, not a new milestone. The canonical parser now directly consumes `prefix::Operation` rather than maintaining a second P028-only enum. The earlier temporary module harness did not cover that canonical tree; it did not qualify the actual executable. P029A adds canonical parser mappings/help tests and actual-binary integration covering read-only queries, backup create/verify/delete, restore approval/revisions, journal completion/release and separate P027 CAS recovery. Restricted /proc environments must assert the documented refusal, never bypass process-use assessment.

Recovery validates the original intent against the staged journal, checks allowed journal entries and completion-marker identity before publication or registry mutations, and rejects premature completion evidence. Additional child-process exits interrupt individual copy chunks and unlinks. Preserved originals and incomplete staging/deletion remain available for inspection. No automatic rollback is claimed.

## P029B investigation status

P029B remains incomplete. A synthetic Wine-style tree with internal and external `dosdevices` symlinks was rejected without following either link. Legacy transactions lack the durable deletion intent and preserved-original content evidence needed to safely infer deletion replay or rollback. Deletion replay remains unsupported. New schema-2 restore transactions support explicitly approved rollback only after filesystem publication and before the prefix registry commit. They retain and recheck an inventory of the preserved original; schema-1 and committed restore rollback are blocked. See the investigation record for the journal, retention and qualification limits. See [the P029B investigation record](manifests/PROMPT029B_MANIFEST.md).


### P029B limited rollback commands

```sh
endfield-unix prefix recovery rollback-plan --id game
endfield-unix prefix recovery rollback --id game --expected-revision REV --confirm EXACT_TOKEN_FROM_PLAN
```

Rollback is not automatic. The saved original and displaced restored payload are retained, and no P027 pin is changed. If interrupted after intent, run rollback again with the exact token and unchanged revision; forward recovery refuses the active rollback intent. `recovery release` archives a completed rollback while retaining the displaced tree. A valid schema-2 inventory and the exact original registry record are required. Any P027 pin-recovery journal referencing the snapshot blocks rollback, even if marked complete. Legacy schema-1 journals, completed/registry-committed restores and ambiguous evidence fail closed. Hashes detect changes, not malicious same-user tampering or publisher authenticity.
