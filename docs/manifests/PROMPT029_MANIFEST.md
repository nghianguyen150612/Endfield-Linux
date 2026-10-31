# PROMPT029 implementation and qualification manifest

Status: **DRAFT / NOT ACCEPTED**. P030 is untouched. No runtime or game process was launched. Real Wine-prefix, physical Arch desktop, Dell Vostro and Endfield gameplay qualification are **UNVERIFIED**.

## Baseline and review

Fetched remote branches before editing. Main was `223bcd4`; P028 remote head was exactly `4789d64f3c8ff745bd3c235ec1f040a6a9c7e0c2`. GitHub PR #1 was OPEN with mergedAt null. Supplied P028 [run 41083711942](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41083711942) completed successfully at that head.

Branch `feat/p029-prefix-backup-restore-recovery` descends from that verified P028 head. One [Draft PR #2](https://github.com/nghianguyen150612/Endfield-Linux/pull/2) targets `feat/p028-isolated-prefix-management`. Nothing was merged or committed to main. The final full HEAD and latest run are reported in the completion message; this committed document cannot contain its own commit hash.

## Implementation inventory

- `crates/prefix-management/src/backup.rs`: typed backup IDs/manifests/integrity/plans, bounded copy and verification, exact-scope restoration, journaled exchange/CAS recovery, explicit deletion, history archival and separately approved P027 pin CAS recovery.
- `crates/prefix-management/src/backup_tests.rs`: process-exit, pin-recovery, corruption, quota-accounting, readonly, deletion and archival regressions; additional core snapshot tests live in backup.rs.
- `crates/prefix-management/src/linux_fs.rs` and lib.rs: reuse Linux safeguards and block P028 mutations while restore evidence occupies its active slot; no P028 schema or ABI change.
- `apps/endfield-unix/src/cli/prefix.rs`: nested backup/restore/recovery commands and approval parsing. Explicit serde dependency and corresponding lockfile entry support typed JSON reports.
- `apps/endfield-unix/tests/runtime_install_cli.rs`: actual binary backup/verify/delete smoke or fail-closed /proc result, exact pin-byte preservation and read-only empty-XDG tests.
- `.github/workflows/ci.yml`: stacked-PR branch coverage and focused snapshot/fault tests without removing any existing Rust, GTK, CLI, metadata or GCC/Clang gates.
- README, CHANGELOG, ROADMAP, PREFIX-MANAGEMENT, RUNTIME-PINNING, STORAGE, SECURITY, ARCHITECTURE, CLI and TESTING updated; [BACKUP-RECOVERY.md](../BACKUP-RECOVERY.md) is the detailed format and operating contract.

## Data and runtime guarantees

Schema-1 snapshots live only under P006 app data/backups. Owner metadata binds UID and root/parent inode identities. Private staging is verified before no-overwrite publication; directory names are the snapshot index. Each manifest records the validated P028 source record, exact P027 association and provenance/revisions, lifecycle, timestamp, bounded inventory, modes/mtime/sizes and SHA-256 digests. Restoration always re-verifies current payload bytes and exact authorized scope. SHA-256 is not authentication.

Restore uses same-mount Linux exchange after staging validation; original data remains in the transaction payload. Prefix-registry CAS is a subsequent journaled step, not an atomic cross-registry transaction. Recovery distinguishes original/staged inode positions and expected registry revisions. Repeated completion is idempotent. Fully staged backups can be explicitly republished. Ambiguous/missing/corrupt evidence stays blocked and preserved. Completed restore release archives originals without deleting them.

Prefix-associated runtime restoration and P027 pin restoration are separate. Prefix-only operations have no PinStore mutation path. A separately confirmed pin operation revalidates the saved exact reference through existing managed P025/P026 or external P022 CLI adapters, checks adapter availability and invokes public P027 CAS. It neither installs nor substitutes runtimes, imports other profiles, or invents P021 compatibility evidence. Pin journaling recognizes the exact expected successor revision after interruption.

## Local evidence

Toolchain: genuine Rust 1.85.0 installed in the environment. Synthetic fixtures only.

| Check | Observed result |
| --- | --- |
| cargo +1.85.0 fmt --all -- --check | Passed |
| cargo +1.85.0 test --locked -p endfield-unix-prefix-management --all-targets | Passed; final count recorded in completion report |
| Focused prefix-management Clippy, all targets, -D warnings | Passed |
| cargo +1.85.0 test --locked --workspace --exclude endfield-unix | Passed, including P006/P019/P021–P028 libraries, IPC/daemon and optimizer contracts |
| cargo +1.85.0 metadata --locked --no-deps --format-version 1 | Passed |
| git diff --check | Passed |
| GCC C17 ABI script | Passed |
| Full workspace test, Clippy and build | Attempted; failed at missing native development packages (including glib-2.0); GTK4/libadwaita pkg-config probes are unavailable |
| Local Clang C17 | Unavailable: clang is not installed |
| Actual application CLI integration and GTK smoke | Pending genuine hosted gates; not claimed from local harness |

A temporary external test crate typechecked the real prefix.rs and runtime_pin.rs CLI modules against the real headless libraries and exercised Clap parsing plus backup list, observing exit 0 and `[]`. This is **not** execution of the canonical application binary and does not replace the hosted CLI/GTK tests. No fake pkg-config files or native libraries were created.

Core tests cover empty/nested/hidden/binary roundtrip, stable distinct IDs, readonly verification, corruption/unexpected entries, unknown/active use, links/sparse-file refusal, quota arithmetic, confirmation/CAS conflicts, lifecycle/no-false-initialization, exact runtime metadata, original preservation, independently journaled pin restoration, and explicit deletion/archive behavior. Actual child processes exit with code 91 at backup intent/staged/published and restore intent/staged/published/registry boundaries. Recovery uses a fresh process's released OS locks and durable evidence. Test-only usage observations are injectable; production has no bypass.

Initial hosted [run 41148857178](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41148857178) targets `b6b5e90d41357479bb54c60b69fbaf584502dede`; [run 41149086346](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41149086346) targets `9fd9709d1856575e6564ca8a9b17f3b43ed8121f`. Both were queued when inspected. Later commits trigger new runs; only a successful run on final HEAD can qualify acceptance.

## Remaining acceptance risks

Required hosted Rust/GTK/CLI/GCC/Clang jobs have not completed. Full actual-binary integration results are not locally available. No pass is inferred from queueing a workflow.

The supported snapshot subset excludes Initialized prefixes, all symlinks (including internal Wine drive links), hardlinks, detected sparse files, xattrs/ACLs, special files and cross-mount operations. Full POSIX metadata and full P027 historical registry restoration are unsupported. Restore planning exposes saved metadata but does not evaluate current P027 selection status. Per-operation quotas do not provide retention management.

Incomplete copy, missing/corrupt journal and partial deletion remain explicitly blocked for manual inspection. Automated deletion-resume, automatic rollback of publication, deletion of preserved originals and a CLI inventory of all historical/deleting/pin-recovery journals are not implemented. Safe completion recovery is supported; rollback is not mislabeled as completion.

The wider requested matrix is not fully qualified: adversarial rename/mount races, alternate UID ownership, physical ENOSPC, arbitrary mid-copy/mid-unlink crashes, every power-loss ordering, all runtime-family/profile/UMU combinations and real prefixes need further coverage. Hashing and advisory locks assume cooperative offline same-user access. These limitations and unresolved required gates keep P029 Draft and prevent an acceptance or P030-readiness claim.
