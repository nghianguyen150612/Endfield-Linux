# P029B hardening and qualification record

Status: **Draft; final qualification pending**. P028 PR #1 is still unmerged. P029 PR #2 remains Draft and targets `feat/p028-isolated-prefix-management`. No merge, P030 work, Wine/Proton/UMU launch, game launch, or physical-device test has occurred. Real initialized Wine-prefix compatibility, Arch/Dell qualification and Endfield gameplay are **UNVERIFIED**.

## Baseline and branch

The verified P028 base is `4789d64f3c8ff745bd3c235ec1f040a6a9c7e0c2`; `main` was `223bcd404a422a54c56c06e7bc5afe273e85320b`. P028 PR #1 is open. Work continues on `feat/p029-prefix-backup-restore-recovery`, the head branch for Draft PR #2. The current final SHA and hosted run must be added after committing and pushing this qualification state. Earlier runs qualify only their named commits.

## Implemented recovery behavior

Schema-2 restore transactions retain a bounded inventory of the original managed prefix before publication. Each original entry records path, type, mode, mtime, size and SHA-256; the P028 record carries exact scope, lifecycle, registry generation and directory identity. Restore rechecks source content before exchange and checks preserved original content afterward. Old schema-1 transactions are not given fabricated content evidence; they remain eligible only for the existing forward-recovery path and are not eligible for rollback.

Explicit rollback applies only to a schema-2 restore that reached filesystem publication while the P028 registry still contains the exact original record at the expected revision. The plan revalidates scope, process-use, runtime backup, both directory identities and both complete inventories. Approval is bound to prefix, backup and both directory inodes. A synced rollback intent precedes same-mount `RENAME_EXCHANGE`; the completion marker follows revalidation. The original registry is still current in this supported state, so rollback checks it under the registry lock without writing it. Both the original and displaced restored tree remain on disk. A crash before/after exchange is replayed idempotently. P027 pins are unchanged. Schema-1, registry-committed, changed, ambiguous and pin-conflicted transactions stay blocked. No automatic rollback or cross-registry atomicity is claimed.

Snapshot deletion uses an independent schema-1 intent at `backups/deletion-journals/<backup-id>/intent.json`. Before retirement, the journal records the complete verified schema-1 manifest, exact source record and current P028 registry revision, deletion-journals/snapshots/deleting parent identities, Linux mount ID, snapshot root/payload identities, original metadata file identities and SHA-256 hashes, and every payload entry's device/inode identity. The bounded envelope is checksummed for accidental corruption, not authenticated against same-user modification. If it exceeds the 32 MiB metadata limit, deletion stops before creating the transaction directory or moving the snapshot.

The explicit `deletion-plan` validates journal schema and scope, registry revision, parent/root/payload identity, mount, metadata bytes, inventory structure, ownership and content. `deletion-resume` requires the current expected revision and the exact token from the plan. A synced `clearing.json` precedes permission changes or unlink. After that marker, every remaining file must be an original inventory path with original device/inode, owner, type, mode, mtime, size and SHA-256; every remaining directory must match its original identity and permitted permissions. Only the original mode or the owner-access bits added for deletion are accepted. Missing entries are accepted only in Clearing. Each unlink fsyncs its parent. Removing the payload, original metadata, retired root and writing completion are separate durable boundaries. Completed journals remain as evidence and prevent backup-ID reuse. Repeated explicit replay is idempotent.

Legacy partial deletion without this independent intent remains inspection-only and blocked. An extra entry, symlink, hardlink, changed file identity/content, corrupt/missing journal, registry revision change, mount change, unexpected transaction or ambiguous marker blocks replay. The lock serializes cooperative application operations. Linux does not provide conditional unlink-by-inode; a malicious same-user process racing after the last name check remains outside the offline cooperative-use boundary. Mount-ID changes across reboots/namespaces can make a previously valid journal blocked for inspection.

Read-only CLI inventory now includes pending backups, legacy deletion remnants, new deletion journals, restore status, archived restore/rollback history and pending/completed pin journal markers. History validation is explicit: restore/rollback archives report directory identity only, not full payload verification; pin history reports journal state only, not current P027 state. Neither history command creates XDG storage. Prefix-only restore, rollback and deletion do not call P027 mutation APIs. P027 pin recovery remains a separate exact-runtime revalidation and expected-revision CAS operation through the public PinStore API.

## Runtime and prefix boundaries

Current P027 machine/profile selection is assessed read-only during restore planning. Blocking profile pins remain blocking; missing, changed or unverifiable saved runtimes stay associated with the prefix and do not cause fallback or family conversion. Explicit pin recovery is separate, revision-checked, revalidates managed runtimes through P025/P026 and external references at their exact P022 location, and does not import another profile or machine. P021 compatibility evidence is not fabricated.

Snapshots and restore are restricted to registered Created, uninitialized prefixes under the P028 managed root and exact machine/profile scope. All symlinks, including internal and external `dosdevices` links, hardlinks, detected sparse files, xattrs/ACLs, special files, non-UTF-8 paths, mount crossings, unsupported metadata and uncertain ownership are rejected. A synthetic Wine-style directory fixture with internal and external `dosdevices` symlinks was refused without changing either target. This is structural testing only and does not establish that an initialized Wine prefix can be backed up or restored.

## Tests and local evidence

Synthetic isolated-XDG fixtures only. Current focused tests include backup/restore bytes and lifecycle, current P027 selection assessment, separate pin CAS and exact pin-byte preservation, corruption and ownership refusal, process-use uncertainty, unsupported filesystem entries, deletion journal corruption/ambiguity, changed nested inode, symlink/hardlink replacement, unexpected entry, registry revision conflict, and idempotent process-exit recovery across deletion and rollback boundaries. Actual-executable tests cover canonical nested help, read-only no-XDG queries, backup/verify/delete, restore approvals/revisions, recovery/history commands, a synthetic published-before-registry rollback fixture and pin state preservation. CI now runs a dedicated `backup::recovery_tests` filter for P029B and retains the independent all-target P028 prefix lifecycle gate.

The IPC saturation test previously assumed that all client `connect()` calls had reached the daemon's bounded queue after a fixed delay. It now fills capacity one accepted connection at a time, detects rejected sockets, and waits until the daemon retains all sixteen worker-plus-queue connections through `/proc/<pid>/fd` before probing the seventeenth. This is a test synchronization correction; IPC production code and assertions are unchanged. The test fails clearly if its Linux `/proc` observation is unavailable. Five consecutive isolated real-daemon repetitions passed, and both default and serialized full-workspace runs passed.

Local validation used a genuine Ubuntu 24.04 container with Rust 1.85.0, GTK 4.14.5, libadwaita 1.5.0, GCC 13 and Clang 18. No synthetic native libraries or pkg-config files were used. The actual GTK smoke ran through Xvfb and D-Bus and produced `/tmp/p029b-gtk-smoke.png` in the validation container.

| Check | Result |
| --- | --- |
| `cargo +1.85.0 fmt --all -- --check` | Passed |
| `cargo +1.85.0 check --locked --workspace` | Passed |
| `cargo +1.85.0 clippy --locked --workspace --all-targets -- -D warnings` | Passed |
| `cargo +1.85.0 test --locked --workspace` | Passed; 784 passed, 0 failed, 5 ignored across workspace test binaries |
| `cargo +1.85.0 test --locked --workspace -- --test-threads=1` | Passed; 784 passed, 0 failed, 5 ignored |
| `cargo +1.85.0 build --locked --workspace` | Passed |
| `cargo +1.85.0 test --locked -p endfield-unix-prefix-management --all-targets` | Passed; 60 passed, including real child-process deletion/rollback interruption tests |
| Actual binary `runtime_install_cli` and `prefix_backup_cli` suites | Passed; 6 integration tests, including parser, backup, restore, rollback, pin CAS, deletion and read-only XDG behavior |
| Actual-daemon IPC saturation test | Passed in five consecutive isolated runs after deterministic socket-retention synchronization |
| GTK smoke under Xvfb/D-Bus | Passed; screenshot generated |
| GCC C17 ABI | Passed |
| Clang C17 ABI | Passed |
| `cargo +1.85.0 metadata --locked --no-deps --format-version 1` | Passed |
| `git diff --check` | Passed |

All tests used synthetic fixtures for prefix data. No actual Wine/Proton/UMU/game process was started. No synthetic result is evidence of physical-device behavior.

## Hosted CI and delivery

Latest hosted run before this hardening commit: [41163530758](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41163530758) passed all three jobs for `4aed60a937f635409cf4cfbece087b8d4e21a450`. That result predates the deletion replay changes and IPC test synchronization; it is not final qualification. A new hosted run is required for the pushed head. Its URL and exact job results will be recorded in Draft PR #2 and the completion report; no queued or superseded result is a pass for that head.

P029B is not accepted until final-head required CI passes and the remaining supported behaviors are reviewed. Legacy deletion remnants, legacy/post-commit rollback, full POSIX metadata, initialized Wine prefixes containing symlinks or other unsupported objects, reboot/mount-namespace recovery, non-cooperative same-user writers, physical devices and gameplay remain unsupported or unverified. Keep PR #2 Draft and do not start P030.
