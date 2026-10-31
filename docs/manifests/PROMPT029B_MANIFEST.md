# P029B hardening investigation — incomplete

Status: **not implemented or qualified**. Continue on Draft PR #2; do not merge or start P030. This record does not claim P029B acceptance.

Baseline checked: local and remote P029 head `818e1fb549c0f672f37a5273cb3d675ee2905ba6`; P028 head `4789d64f3c8ff745bd3c235ec1f040a6a9c7e0c2` remained unmerged; main `223bcd4`. PR #2 remained Draft against P028. At inspection, [run 41160597072](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/41160597072) had a successful Rust/GTK job and queued GCC and Clang jobs; queued is not success.

## Evidence and outstanding implementation

A new isolated synthetic Wine-layout test constructs `drive_c`, registry fixtures and `dosdevices` links. Both a relative internal link and an absolute external link are rejected; the external target and link text remain unchanged. The focused Rust 1.85 test passed in a genuine Ubuntu environment. No Wine process ran. This is structural evidence only: real initialized Wine prefixes, physical devices and gameplay remain unverified. Existing rejection of symlinks, hardlinks, detected sparse files, xattrs/ACLs and special objects remains unchanged.

The existing schema-1 deletion retirement has no distinct durable deletion intent, and the original manifest is removed before the retired root. A crash can leave an incomplete tree or a missing manifest; treating this as authority to delete would be unsafe. Explicit deletion resume, its intent/completion journal, identity-bound approval, idempotent final-unlink recovery and tests remain **unimplemented**. Existing deletion inspection remains read-only and ambiguous states remain blocked.

The existing restore transaction records original directory identity but not an authenticated or verified original-content inventory. It cannot establish that preserved bytes were unchanged after publication. Explicit rollback planning, durable rollback intent, exchange/CAS recovery, preservation of displaced restored data and the fault-injection matrix remain **unimplemented**. No automatic or manual rollback is claimed. P027 pin behavior is unchanged.

History/pin-journal CLI inventory and the extended filesystem, concurrency, storage exhaustion and process-exit matrix remain **unimplemented**. The new fixture does not qualify those cases.

## IPC race investigation

`apps/endfield-unix/tests/ipc_e2e.rs` opens four worker clients and twelve queued clients, sleeps 50 ms only after all sixteen connects, then expects the seventeenth socket to close. The listener uses a twelve-slot `sync_channel` and `try_send`; if workers have not yet dequeued the first clients, earlier connections may be rejected while the queue is full. Later worker dequeues can free space for the nominal seventeenth connection, whose read then times out with `WouldBlock`. This is a scheduling-dependent test assumption consistent with the observed operation, not proof that the protocol itself has failed or proof that production is correct. A deterministic readiness barrier or independently justified production change requires separate design; no timeout, sleep, assertion or IPC code was changed. Prior local default and serialized failures and isolated pass are documented in the P029 manifest. Controlled reproduction was not repeated in this investigation.

## Qualification

Only the new focused synthetic fixture test and formatting were run for this change. Required full workspace checks, CLI integration, GTK, GCC/Clang ABI, deletion/rollback fault injection and final-head hosted CI remain outstanding. The subsequent commit must not be qualified by an older run. This is an incomplete investigation, not a completed P029B implementation; the acceptance conditions and P030 readiness are unmet.
