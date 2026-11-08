# Game launch planning (P034)

P034 adds a portable launch model and a headless, read-only preflight command. It does not yet provide production process execution, so the application reports production execution as unsupported and keeps GTK Play disabled.

The portable crate, endfield-unix-game-launch, validates explicit installation, prefix, profile, adapter, and installation-relative executable selections. Its typed LaunchPolicy records policy evidence, whether process creation is allowed, and the mandatory approval requirement. It generates a deterministic SHA-256 plan ID from the selected identities, registry revisions, filesystem observations, policy, warnings, blockers, and approval requirements. Approval is tied to that exact plan. Validation recomputes the ID, so changing plan fields while preserving the original ID invalidates approval.

The CLI command is:

    endfield-unix game launch plan --id INSTALLATION --prefix-id PREFIX --profile PROFILE --adapter ADAPTER --executable-relative PATH --json

The profile may be omitted for machine selection. The adapter must be explicit: managed-wine, external-wine, umu-proton, or direct-proton. Plans disclose the selected relative candidate and registry identities without serializing absolute game, runtime, or prefix paths. Blocked plans return status 78; malformed requests return status 2; an unavailable registry returns status 4.

Preflight reads P031's selected installation record and revalidates the registered root. It opens the candidate beneath that root by directory descriptor, rejects symlinks and traversal, records the file device/inode/size, and checks for a bounded PE/COFF signature. That signature does not authenticate the publisher or prove the file is Endfield. A later path-based execution would need fresh identity checks; this observation alone does not eliminate races.

Runtime preflight resolves the current machine/profile pin through P027 and invokes the existing P025/P026 managed identity and integrity check or P022 external structural revalidation. A stale profile pin remains blocking and does not fall back. The requested adapter must match the exact stored P027 adapter preference. P023 currently prepares non-executable UMU plans, and no production invocation adapter is verified, so runtime execution remains blocked.

Prefix preflight validates the exact P028 managed prefix, lifecycle, initialization evidence, runtime association, scope, and P029 recovery markers. Created or otherwise uninitialized prefixes produce prefix_initialization_unsupported. No initialization state is created or inferred. Pending or unreadable recovery evidence blocks. P033 is merged with outstanding acceptance debt; critical dependency readiness is reported as unknown and blocks execution.

The GTK home page accepts the same explicit selections and runs this preflight through a background worker. It presents the resulting plan state, blockers, and warnings. Play remains disabled because no supported production executor exists. The worker never waits for a child process.

Synthetic tests compile a harmless Rust fixture and run it directly with argument vectors. They cover literal spaces, Unicode, shell metacharacters, working directory, cleared environment, spawn failure, nonzero exit, bounded output, test-only approval mismatch, cancellation before spawn, and duplicate-launch exclusion. These tests demonstrate only controlled synthetic process behavior; they do not qualify a Wine/Proton/UMU adapter or gameplay.

Runtime execution, if later implemented, can modify prefix contents and caches and may access the network. P034 currently does not start runtimes or games, create application storage for planning, use a shell, or modify game, pin, prefix, or recovery data. P019 session supervision is not connected, process ownership across GUI closure is unverified, and restart recovery remains P036 work.

Anti-cheat and Linux gameplay compatibility remain unknown. P032's official Steam evidence reports kernel-level anti-cheat; P034 does not disable, bypass, or patch it. No physical machine or real game was tested. P035 is not ready until P034 has a verified production execution boundary and actual process ownership semantics.
