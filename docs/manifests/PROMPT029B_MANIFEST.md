# P029B hardening investigation — incomplete

Status: **not implemented or qualified**. Continue on Draft PR #2; do not merge or start P030. This record does not claim P029B acceptance.

Baseline checked: local and remote P029 head `818e1fb549c0f672f37a5273cb3d675ee2905ba6`; P028 head `4789d64f3c8ff745bd3c235ec1f040a6a9c7e0c2` remained unmerged; main `223bcd4`. PR #2 remained Draft against P028. At inspection, [run 41160597072](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41160597072) had a successful Rust/GTK job and queued GCC and Clang jobs; queued is not success.

## Evidence and outstanding implementation

A new isolated synthetic Wine-layout test constructs `drive_c`, registry fixtures and `dosdevices` links. Both a relative internal link and an absolute external link are rejected; the external target and link text remain unchanged. The focused Rust 1.85 test passed in a genuine Ubuntu environment. No Wine process ran. This is structural evidence only: real initialized Wine prefixes, physical devices and gameplay remain unverified. Existing rejection of symlinks, hardlinks, detected sparse files, xattrs/ACLs and special objects remains unchanged.

The existing schema-1 deletion retirement has no distinct durable deletion intent, and the original manifest is removed before the retired root. A crash can leave an incomplete tree or a missing manifest; treating this as authority to delete would be unsafe. Explicit deletion resume, its intent/completion journal, identity-bound approval, idempotent final-unlink recovery and tests remain **unimplemented**. Existing deletion inspection remains read-only and ambiguous states remain blocked.

New restore transactions use journal schema 2 and retain a bounded, hashed inventory of the original tree. They check that inventory immediately before publication and during forward recovery; schema-1 journals remain usable for forward recovery. Checksums are not authentication against malicious same-user edits.

Explicit rollback is implemented only for schema-2 restores interrupted after filesystem publication and before the prefix registry commit. A read-only plan checks both inode identities, both inventories, scope, process use and the exact unchanged registry revision. Explicit confirmation is bound to the prefix, backup and both inode numbers. A durable intent precedes `RENAME_EXCHANGE`; the original registry record is still current, so the operation checks its revision and record under the registry lock without writing it. Any pin-recovery journal referencing the snapshot conservatively blocks rollback. The displaced restored tree is preserved in the transaction. Repeating rollback after an interruption completes it idempotently. Release can archive the completed transaction without deleting either tree. Rollback after registry commit, schema-1 rollback, automatic rollback and cross-registry atomicity are unsupported. Prefix and pin registries are not modified by this limited rollback.

Synthetic tests inject errors and real process exits after rollback intent, exchange and completion, and cover changed original bytes, ambiguous markers, incorrect approval/revision and schema-1 blocking. They do not establish power-loss durability or qualify the full adversarial matrix.

History/pin-journal CLI inventory and the extended filesystem, concurrency, storage exhaustion and process-exit matrix remain **unimplemented**. The new fixture does not qualify those cases.

## IPC race investigation

`apps/endfield-unix/tests/ipc_e2e.rs` opens four worker clients and twelve queued clients, sleeps 50 ms only after all sixteen connects, then expects the seventeenth socket to close. The listener uses a twelve-slot `sync_channel` and `try_send`; if workers have not yet dequeued the first clients, earlier connections may be rejected while the queue is full. Later worker dequeues can free space for the nominal seventeenth connection, whose read then times out with `WouldBlock`. This is a scheduling-dependent test assumption consistent with the observed operation, not proof that the protocol itself has failed or proof that production is correct. A deterministic readiness barrier or independently justified production change requires separate design; no timeout, sleep, assertion or IPC code was changed. Prior local default and serialized failures and isolated pass are documented in the P029 manifest. Controlled reproduction was not repeated in this investigation.

## Qualification

The focused prefix-management suite, formatting, focused Clippy and actual-binary CLI integration were run during this work; the final HEAD still needs complete qualification. Required full workspace checks, CLI integration, GTK, GCC/Clang ABI, deletion/rollback fault injection and final-head hosted CI remain outstanding. The subsequent commit must not be qualified by an older run. This is an incomplete investigation, not a completed P029B implementation; the acceptance conditions and P030 readiness are unmet.
