# PROMPT034 implementation manifest

Status: partial implementation; production launch unsupported; PR #7 remains Draft and unmerged.

## Recovery and qualification history

The continuation started on `feat/p034-secure-game-launch` at `5514755c14bbba30d369a585f375b2fa72b14d30`, with a clean worktree and no commits beyond the checkpoint. The remote branch and open Draft [PR #7](https://github.com/nghianguyen150612/Endfield-Linux/pull/7) matched that HEAD; `main` was `fb743f209fc7935fa86f05b58cbd380d76f245d3`. The exact checkpoint passed all three required hosted jobs in [CI run 42490708196](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/42490708196). This is historical evidence for the checkpoint, not qualification of this continuation.

## Implemented in this continuation

- Added a managed Wine, preparation-only invocation adapter. It constructs direct native argv without shell concatenation and requires a checked `bin/wine`, runtime pin and generation, P025 ownership record, P026 complete audit report, prefix record/revision/device/inode/runtime pin association, P031 game root, and P032 executable device/inode. The invocation digest covers those identities, argv, working directory, environment policy, direct-child lifetime policy, and side effects.
- Added an explicit desktop environment allowlist policy, prefix binding, direct-child-only lifetime description, and expected filesystem/network/process side effects. The plan remains non-executable; P019 ownership is not claimed.
- Added P028 read-only initialization plans with deterministic IDs, exact plan-ID approval matching, stale-runtime and identity checks, empty-prefix and inactive-use requirements, and fail-closed P029 recovery states. The stable outcome remains `initialization_unsupported`; no initializer runs and no state is promoted.
- Added typed P028 status that treats stored completed evidence as an unverified claim until a runtime-specific completion verifier exists.
- Upgraded the deterministic P034 plan to schema v2. Its plan ID now binds runtime-reference and ownership/integrity identity, invocation-plan ID and executable identity, prefix record/filesystem/runtime association and initialization-plan ID, and game-root identity.
- Extended CLI/GTK preflight reporting for runtime invocation support, prefix lifecycle, initialization evidence, and P029 recovery. Play stays disabled.
- Added focused Rust tests for native Unicode/shell-metacharacter arguments, non-execution, runtime/prefix/game identity binding, unsupported prefix initialization, preservation, stale association, unknown/pending recovery, and forged completion claims.

## Runtime and lifecycle boundary

| Adapter | State |
| --- | --- |
| Managed Wine | Verified non-executable argv preparation when the exact `bin/wine` object and P025/P026 evidence pass. Process creation remains disabled. |
| External Wine | Unsupported because P022's external identity does not bind an execution-approved exact binary path or trust policy. |
| UMU-Proton | Existing P023 non-executable plan preparation only. This P034 path does not create a production invoker; `UMU_RUNTIME_UPDATE=0` is not an offline guarantee. |
| Direct Proton | Unsupported; no verified P027 preference, game ID, or invocation semantics. |

Prefix initialization is not implemented. The plan requires a Created, application-owned, unchanged, empty, inactive prefix; exact runtime association; known recovery state; and exact approval. It always contains `initializer_unverified`, never creates a process, and never changes P028 or P029 storage. P029 initialized-prefix backup/restore remains unsupported.

Managed Wine preparation does not authorize launch. P033 critical dependency readiness, Endfield compatibility, anti-cheat compatibility, prefix initialization verification, process execution policy, and P019 ownership are still blocking or unknown. No runtime fallback is added, and stale pins remain stale.

## Validation status for this continuation

The current working tree has not yet received final local or hosted qualification. Only JSON syntax validation for `docs/schema/game-launch-plan-v2.schema.json` has run so far. This environment lacks Rust/Cargo and native GTK development files, and has limited free disk space. The previous successful hosted run on `5514755c14bbba30d369a585f375b2fa72b14d30` does not qualify new commits. Final validation still must include Rust 1.85 workspace checks, tests, Clippy/build/metadata, actual CLI/schema smoke, GTK smoke, GCC and Clang C17 ABI, and final-head hosted CI.

## Physical qualification and acceptance

No observations were collected from the named desktop or laptop, and no real Endfield executable or runtime was launched. Physical qualification is **UNVERIFIED**. P034 is not ready for acceptance. Do not merge PR #7 or begin P035 until the production initializer, completion verification, process execution/approval boundary, and process ownership/recovery requirements are independently implemented and qualified.
