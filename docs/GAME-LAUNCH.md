# Game launch planning (P034)

P034 remains in progress. It can prepare and inspect an exact launch plan, but it cannot create a production process. GTK Play stays disabled, `game launch` has no `apply` operation, and every prepared invocation reports `process_creation_supported: false`.

The portable launch plan is schema v2. Its deterministic ID binds the selected installation and registry revision, the P027 runtime pin and reference digest, managed runtime generation/ownership/artifact and P026 evidence, the selected adapter and invocation-plan ID, the runtime executable device/inode/size, the P028 prefix record and filesystem identity/association, the initialization-plan ID, P031 game-root identity, P032 executable identity, policy, blockers, warnings, and expected side effects. Plan approval cannot override blockers, and the production execution gate remains closed.

The canonical read-only command is:

```bash
endfield-unix game launch plan \
  --id endfield-main \
  --prefix-id prefix-main \
  --profile desktop \
  --adapter managed-wine \
  --executable-relative 'SelectedExecutable.exe' \
  --json
```

The profile may be omitted for machine selection. The adapter must be explicit: `managed-wine`, `external-wine`, `umu-proton`, or `direct-proton`. The command never creates application storage merely to plan. It returns status 78 for a blocked plan, 2 for an invalid request, and 4 when required state cannot be observed. Output is bounded and contains no absolute game, runtime, prefix, or argument values.

| Adapter | Current P034 boundary |
| --- | --- |
| Managed Wine | The preparation-only adapter builds native argv as `bin/wine`, the selected game executable, then literal arguments. It requires the exact managed P027 pin, P025 ownership and P026 complete matching audit, a regular executable `bin/wine`, the matching P028 association, and exact prefix/game filesystem identities. The invocation plan has its own digest and reports its expected executable identity. No process is created. If the expected file is absent, a symlink, or changed, preparation is unsupported. |
| External Wine | Unsupported for launch. P022 revalidates structural identity, but its stored external reference does not bind an execution-approved exact Wine binary path or external trust policy. P034 does not guess among `wine`, `wine64`, or other entries. |
| UMU-Proton | P023 can create a non-executable P021 plan. P034 does not yet bind a discovered UMU executable and production process boundary to an approved request. `UMU_RUNTIME_UPDATE=0` disables one update behavior; it does not guarantee offline execution or prevent initial Steam Linux Runtime acquisition. |
| Direct Proton | Unsupported. P027 has no direct Proton preference and P034 has no verified game identifier or invocation flags for this path. It does not substitute another runtime. |

The managed Wine argv follows Wine's documented `wine program [arguments]` model and never joins values into shell text. Its preview specifies an exact executable, working directory, native arguments, `WINEPREFIX`, a clear-and-desktop-session-allowlist environment policy, runtime/prefix/game identity bindings, foreground direct-child lifetime policy, approval requirement, and expected effects. P019 daemon ownership is not connected, so this lifetime policy is descriptive and cannot be used to start a game. See the [Wine manual](https://manpages.debian.org/bookworm/wine/wine.1.en.html).

UMU and Proton are separate adapters. UMU uses its documented environment model and may write prefix/cache data or access the network to acquire Steam Linux Runtime components. `PROTON_VERB=waitforexitandrun` and the existing P023 environment are not interpreted as proof of safe initialization, offline behavior, or process ownership. See the [UMU manual](https://github.com/Open-Wine-Components/umu-launcher/blob/main/docs/umu.1.scd), the [UMU README](https://github.com/Open-Wine-Components/umu-launcher), and the [Proton FAQ](https://github.com/ValveSoftware/Proton/wiki/Proton-FAQ).

P028 now exposes a deterministic initialization plan for the exact Created prefix, machine/profile scope, and selected runtime association. It requires the registry revision, private owner UID, unchanged directory device/inode, an empty directory, no active use, a valid runtime association, and known-clear P029 recovery. The scope is included in the deterministic plan ID and durable intent. The only source-qualified initializer semantics in this continuation are upstream Wine 10.0 `wineboot.exe --init`. The plan still reports unsupported because the current process API does not bind the audited launcher inode through exec and Wine helper-process ownership is not integrated. `prefix init` and GTK Play do not start Wine.

Wine's 10.0 `wineboot` man page says it performs initial WINEPREFIX setup and documents `--init`; the [Wine 10.0 source](https://github.com/wine-mirror/wine/blob/wine-10.0/programs/wineboot/wineboot.c#L1801-L1922) parses that option and calls `update_wineprefix`. That helper logs a missing `wine.inf` and returns without propagating the error to `main`, so exit code zero is not sufficient completion evidence. The [module build file](https://github.com/wine-mirror/wine/blob/wine-10.0/programs/wineboot/Makefile.in) identifies the Windows module as `wineboot.exe`. This source evidence applies only to upstream Wine 10.0 semantics; it does not qualify a local managed build or imply game compatibility.

P034 persists a schema-v2 intent with the exact plan, machine/profile scope, pin revision, runtime-reference digest, prefix generation, and directory device/inode before any future spawn. It records Starting, Running, VerificationPending, Failed, or Interrupted and never retries an unresolved attempt. A zero exit only reaches VerificationPending. The descriptor-relative Wine 10.0 verifier checks the same prefix inode, `drive_c`, `dosdevices/c:` resolving to `../drive_c`, and stable `system.reg` and `user.reg` files with a Wine registry v2 header. It does not follow the `c:` symlink. This evidence supports a structural initialization claim only. P029 recovery, unknown process use, or a changed runtime/prefix binding prevents promotion.

P029 recovery is presented separately as `clear`, `pending`, or `unknown`. Pending or unreadable recovery metadata blocks execution. P029's current backup/recovery support covers Created, uninitialized prefixes only; P034 does not assume that an initialized prefix can be backed up or restored. Initialization cannot overwrite an initialized or externally owned prefix.

Preflight revalidates P031's game root and opens the selected executable beneath the held root descriptor with `O_NOFOLLOW`; it records device/inode/size and checks a bounded PE/COFF signature. That signature does not authenticate the publisher. P027's exact profile or machine pin is resolved without fallback, then P025/P026 managed identity/integrity or P022 external structure is revalidated. Adapter mismatch, stale pin, missing identity, changed prefix, pending/unknown recovery, incomplete dependency evidence, and unsupported initialization remain visible blockers. Compatibility and anti-cheat evidence remain separate and unknown; P033 acceptance debt keeps critical dependency readiness blocked.

The GTK worker reports the selected runtime adapter, invocation preparation state, prefix lifecycle, initialization evidence state, P029 recovery state, launch blockers, and approval requirement. It does not wait synchronously for runtime or game work. Play is an insensitive explanatory control and never reports a successful launch.

No P034 production process executor exists. The synthetic executor remains isolated to tests. Runtime/game writes, child lifetime, GUI-close behavior, cancellation after spawn, and descendant cleanup are not implemented. Planning and preflight do not mutate runtime pins, prefix registries, game registries, game files, or recovery journals. Do not run the real game in CI.

The previous verified checkpoint at `2b219c251fa0faa6a48e08e5f95da9fe233418db` passed all required hosted jobs in [run 42760639371](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/42760639371). The continuation implementation at `384d178ac7a20bcc6ba224fd6d3b1efce16fad09` passed all three hosted jobs in [run 42790178867](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/42790178867), including Rust/GTK workspace gates, P019/P021–P033 and P034 tests, daemon/IPC, GTK smoke, workspace build, metadata, and GCC/Clang C17. The following documentation-only head `231d2e318e033cebcfdc24786a672409acc78d77` also passed all three jobs in [run 42792298525](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/42792298525). No physical desktop/laptop observation or Endfield gameplay test is claimed. Anti-cheat compatibility remains unknown; P034 does not bypass anti-cheat, patch game files, or disable integrity checks.

P034 is not ready for acceptance, and P035 must not begin until a safe initializer and complete production execution/ownership boundary are independently qualified.
