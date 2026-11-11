# PROMPT034 implementation manifest

Status: partial implementation; production launch unsupported; PR #7 remains Draft and unmerged.

## Current continuation: root-bound executor and P026 artifact check

Recovered clean branch `feat/p034-secure-game-launch` at `97a80df95d5a86919a9774f0437a1664078ca635`; remote HEAD matched, PR #7 remained open and Draft, and that exact checkpoint passed all hosted jobs in [CI run 42973478828](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/42973478828). The current work adds a no-follow absolute `TrustedRoot` checked against an expected directory device/inode/UID, safe-ancestor permissions, and Linux mount IDs. Descendant files and the working directory are opened from held descriptors. The executor validates native ELF64 headers and exposes `PT_INTERP`, refuses scripts and non-native formats, uses `execveat(AT_EMPTY_PATH)`, and retains an `OwnedChild` with bounded captures, a unique in-process attempt identity, bounded wait, direct-child cancellation, and direct-child cleanup on drop.

Managed Wine preflight now reloads the exact P027 pin revision around P025/P026 validation, opens `bin/wine` under the held root descriptor, and compares device/inode/size/SHA-256 with the P026 baseline entry. The report distinguishes top-level executable integrity from dependency-closure status. It remains a read-only preflight observation; it is not retained as a future spawn authorization. P028 transition APIs remain durable and fail closed, but no production coordinator connects them to a Wine process. No authentic Wine runtime was executed.

## Continuation: descriptor-bound executor

The earlier checkpoint added the Linux executor and P028 state-machine fix. It opened relative components with `O_NOFOLLOW`, retained the ELF descriptor through `execveat(AT_EMPTY_PATH)`, and rejected scripts. Its initial `std::process::Child` API lacked the trusted-root checks and bounded supervision added in the current continuation.

The executor is still a low-level component and is not connected to P028's production approval transition, P034 launch approval, CLI apply, GTK Play, or P019 supervision. Preflight now binds the top-level `bin/wine` file to P026 evidence, but does not bind the dynamic loader, shared-library resolution, helper modules, or Wine descendant processes. Managed Wine initialization and production game launch remain blocked. The state transition matrix rejects exit results from `Starting`, permits only `SpawnFailed` there, accepts exits from `Running`, and records interruption from `Starting` or `Running` as recovery-required.

Current local Rust 1.85 focused validation passes formatting, game-launch crate tests (20 unit and 7 controlled execution tests), and application type checking using temporary pkg-config metadata. Native tests execute repository-owned C ELF fixtures and cover pathname replacement, bounded output, cancellation, permissions, environment rejection, and direct-child status. The application check did not link GTK because this container lacks GTK4/libadwaita development libraries. Implementation head `1e33a7d2e14083f3dbd2436fdc6f2a1e903b553b` passed all hosted jobs in [CI run 42992253983](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/42992253983). The exact current head's hosted result is linked from PR #7.

## Recovery and qualification history

The continuation started on `feat/p034-secure-game-launch` at `2b219c251fa0faa6a48e08e5f95da9fe233418db`, with a clean worktree and no commits beyond the checkpoint. The remote branch and open Draft [PR #7](https://github.com/nghianguyen150612/Endfield-Linux/pull/7) matched that HEAD; `main` was `fb743f209fc7935fa86f05b58cbd380d76f245d3`. The exact checkpoint passed all required hosted jobs in [CI run 42760639371](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/42760639371). This is historical evidence for the checkpoint, not qualification of this continuation.

The implementation head `384d178ac7a20bcc6ba224fd6d3b1efce16fad09` passed all three required jobs in [CI run 42790178867](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/42790178867). Workspace check, warning-denied Clippy, the actual P028 initialization-plan CLI assertions, P019/P021–P033 regressions, daemon/IPC, GTK smoke, build, metadata, and both GCC/Clang C17 ABI jobs passed. Optional systemd user-manager integration was unavailable and skipped. Earlier run 42789933240 failed only because the new CLI test expected `unsupported` while the runner correctly returned `blocked` for unknown `/proc` visibility; the assertion was corrected to accept either fail-closed state while requiring the adapter blocker, and the final implementation run passed.

## Implemented in this continuation

- Added a managed Wine, preparation-only invocation adapter. It constructs direct native argv without shell concatenation and requires a checked `bin/wine`, runtime pin and generation, P025 ownership record, P026 complete audit report, prefix record/revision/device/inode/runtime pin association, P031 game root, and P032 executable device/inode. The invocation digest covers those identities, argv, working directory, environment policy, direct-child lifetime policy, and side effects.
- Added an explicit desktop environment allowlist policy, prefix binding, direct-child-only lifetime description, and expected filesystem/network/process side effects. The plan remains non-executable; P019 ownership is not claimed.
- Added P028 schema-v2 durable initialization intent, exact plan/machine-profile-scope/pin/runtime/prefix binding, Starting/Running/VerificationPending/Failed/Interrupted transitions, revision checks, and refusal to retry, unregister, delete, or generically recover unresolved initialization.
- Added Wine 10.0 structural completion verification using no-follow directory/file descriptors, exact prefix inode, `drive_c`, `dosdevices/c:` target, stable Wine registry v2 headers, and bounded content digests. The completion method revalidates P027/P025/P026 and P029 state, and process exit zero alone never promotes `Initialized`.
- Added canonical `prefix init-plan --id ID [--json]` output. Existing `prefix init --id ID` syntax remains unchanged and blocked.
- Added an actual-binary CLI integration assertion for the blocked initialization plan: typed unsupported JSON and exit 78, with P027 pin bytes, P028 registry bytes, the empty prefix, and the runtime execution sentinel unchanged.
- Added versioned schemas for prefix registry v2 and initialization plan v2, including checksum-validated in-memory migration from registry v1.
- Upgraded the deterministic P034 plan to schema v2. Its plan ID now binds runtime-reference and ownership/integrity identity, invocation-plan ID and executable identity, prefix record/filesystem/runtime association and initialization-plan ID, and game-root identity.
- Extended CLI/GTK preflight reporting for runtime invocation support, prefix lifecycle, initialization evidence, and P029 recovery. Play stays disabled.
- Added focused Rust tests for native Unicode/shell-metacharacter arguments, non-execution, runtime/prefix/game identity binding, unsupported prefix initialization, preservation, stale association, unknown/pending recovery, and forged completion claims.

## Runtime and lifecycle boundary

| Adapter | State |
| --- | --- |
| Managed Wine game launch | Verified non-executable argv preparation when P027/P025/P026 evidence passes and opened `bin/wine` matches the exact P026 file identity/digest. Process creation remains disabled because the runtime dependency closure and exact pre-spawn authorization are not established. |
| Managed Wine initialization | Wine 10.0 source parses `--init`; the selected managed artifact's `wineboot.exe` layout and launcher invocation remain unqualified. Exit behavior is insufficient as a completion signal by itself. Execution remains unsupported because the dependency/helper closure, P028 process coordinator, and real runtime completion have not been qualified. |
| External Wine | Unsupported because P022's external identity does not bind an execution-approved exact binary path or trust policy. |
| UMU-Proton | Existing P023 non-executable plan preparation only. This P034 path does not create a production invoker; `UMU_RUNTIME_UPDATE=0` is not an offline guarantee. |
| Direct Proton | Unsupported; no verified P027 preference, game ID, or invocation semantics. |

Prefix initialization process creation is not implemented. A ready authorization cannot currently be produced by the read-only plan builder. The durable state machine and verifier are implemented and tested with synthetic fixtures; they are not evidence of authentic Wine runtime behavior. P029 initialized-prefix backup/restore remains unsupported. Wine 10.0 source says `wineboot --init` performs setup, but `update_wineprefix` logs `wine.inf` failures without reporting them through process exit status. Wine may start helper processes and write user-scoped cache/configuration outside the selected prefix.

Managed Wine preflight does not authorize launch. The P026-matched `bin/wine` descriptor is dropped after planning, and no production path consumes it through kernel exec. The executor can bind an ELF descriptor but does not bind its dynamic loader/library closure. The P034 child owner supervises only the direct process; it is not integrated with P019 or wineserver/helper descendants. P033 critical dependency readiness, Endfield compatibility, and anti-cheat compatibility are still blocking or unknown. No runtime fallback is added, and stale pins remain stale. `game launch apply` does not exist; GTK Play remains disabled.

## Validation status for this continuation

The documentation-only head `231d2e318e033cebcfdc24786a672409acc78d77` also passed all three required jobs in [CI run 42792298525](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/42792298525). Local genuine Rust 1.85 validation passes formatting, the 74-test prefix-management suite, warning-denied prefix-crate Clippy, P034 launch-plan/controlled-executor tests, P022 runtime and P027 selection tests, P019 session/supervisor tests, schema JSON parsing, GCC C17 ABI and GCC ASan/UBSan C17 tests, and `git diff --check`. A wider non-GTK workspace test attempt ran out of filesystem space; generated `target/debug` artifacts were removed before rerunning focused suites. This local environment lacks GTK4/libadwaita development metadata and Clang, so complete workspace and GUI validation comes from hosted CI.

## Physical qualification and acceptance

No observations were collected from the named desktop or laptop, and no real Endfield executable or Wine runtime was launched. Physical and real-runtime qualification are **UNVERIFIED**. P034 is not ready for acceptance. Do not merge PR #7 or begin P035 until production initialization process creation, final exec identity binding, process ownership, GTK/CLI execution authorization, and final-head hosted qualification are independently implemented and verified.
