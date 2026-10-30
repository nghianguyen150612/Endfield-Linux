# Linux runtime installation (P025)

P025 adds offline installation of explicitly selected local tar.gz Wine/Proton artifacts into the P006 XDG data hierarchy. It does not discover an artifact automatically, execute the runtime, select it, update an existing version, modify an external runtime, or launch Endfield.

## Trust and approval

P024 verification distinguishes independently authenticated publisher evidence from user-pinned keys. The installer preserves that distinction in plan identity and the ownership record. The library accepts publisher evidence only when the manifest signature and scope verify, trusted time was checked, and a scoped rollback checkpoint was supplied. The P024 anchor and checkpoint must come from an independently established caller policy. No publisher keys are bundled, and the CLI does not allow a local key file to claim publisher authentication.

The working CLI path is deliberately the advanced local-trust path: the user provides an explicit raw Ed25519 public key with `--trust-key`; P024 validates its signature, scope, current manifest time, exact artifact ID, filename, build, byte count, and SHA-256. The CLI labels this as **user-pinned, limited trust**. Planning does not install. Applying requires two deliberate values bound to the reviewed plan: its printed plan SHA-256 and the exact signed artifact SHA-256. This path does not create P021 authenticated artifact evidence, claim publisher authenticity, mark a runtime known-good, or change runtime selection.

A plan file is input data, not authorization. Apply reparses it with an unknown-field rejecting schema, freshly verifies the manifest/signature/key/artifact, reconstructs the plan from current XDG paths and evidence, and compares its deterministic identity before accepting approval. Changing trust key, scope, signature, manifest bytes, artifact bytes, destination, family, build, generation or artifact identity invalidates the plan hash or reconstruction. Plan output uses create-new semantics and mode 0600; apply never overwrites an existing installation.

## Commands

Plan a local artifact. Required scope and identity flags are explicit:

```sh
endfield-unix runtime install plan \
  --manifest /absolute/path/manifest.json \
  --signature /absolute/path/manifest.sig \
  --artifact /absolute/path/runtime.tar.gz \
  --trust-key /absolute/path/pinned-ed25519.pub \
  --provider ge-proton --project ge-proton --channel stable \
  --family proton --lineage ge-proton --build GE-Proton10-26 \
  --artifact-id linux-x86_64 --key-id selected-key \
  --plan-out /absolute/path/reviewed-plan.json
```

Review the printed plan and digest, then apply using the exact values:

```sh
PLAN_SHA256_FROM_PLAN_OUTPUT='paste the Plan value printed above'
ARTIFACT_SHA256_FROM_PLAN_OUTPUT='paste the Artifact SHA-256 value printed above'
endfield-unix runtime install apply \
  --manifest /absolute/path/manifest.json \
  --signature /absolute/path/manifest.sig \
  --artifact /absolute/path/runtime.tar.gz \
  --trust-key /absolute/path/pinned-ed25519.pub \
  --provider ge-proton --project ge-proton --channel stable \
  --family proton --lineage ge-proton --build GE-Proton10-26 \
  --artifact-id linux-x86_64 --key-id selected-key \
  --plan /absolute/path/reviewed-plan.json \
  --approve-plan "$PLAN_SHA256_FROM_PLAN_OUTPUT" \
  --accept-user-pinned-sha256 "$ARTIFACT_SHA256_FROM_PLAN_OUTPUT"
```

Inspect only Endfield-Unix managed records and transaction state:

```sh
endfield-unix runtime install status
endfield-unix runtime list-managed
```

The actual CLI integration test uses a synthetic, test-generated signing key and a private temporary XDG root. It exercises plan, rejected approval, apply, duplicate rejection, changed-artifact rejection, record inventory, and inert synthetic files. It never invokes a runtime. Test anchors are not production trust defaults.

Automatic HTTPS acquisition is unavailable because P024 manifest v1 authenticates no download URL. The local CLI supports the explicit user-pinned acceptance path described above. Hosted Rust/GTK, GCC and Clang qualification passed in [run 40872573654](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40872573654). Publisher-authenticated library installation remains available only to a caller that supplies independently established P024 publisher evidence and a trusted checkpoint. The CLI has no publisher trust store or policy-loading interface, since no independently authenticated publisher policy is currently provisioned.

## Managed layout, staging and publication

The installer resolves `$XDG_DATA_HOME/endfield-unix/runtimes` through P006 (default `~/.local/share/endfield-unix/runtimes`). It uses private `runtimes/.staging`, a private lock file, and side-by-side `wine/<installation-id>` or `proton/<installation-id>` directories. Installation IDs bind family, source/project, build, artifact digest, exact manifest digest and generation; they contain no user name or absolute path. Existing roots are walked with Linux directory-relative `openat` and `O_NOFOLLOW`; owners and restrictive modes are checked. No arbitrary `/tmp` staging fallback is used.

The source is first verified through P024, copied into an exclusive private staging directory, synced, and then reopened and rehashed against the signed artifact metadata. Extraction begins only after that staged-byte check. The staging directory is on the same managed filesystem as the final family directory. A process advisory lock is acquired without waiting; it serializes all managed installations, and the kernel releases it if the process exits.

The application supports gzip-compressed tar only and verifies gzip magic before parsing. It does not invoke shell tools, install scripts, package managers, Wine, Proton or UMU. Raw tar iteration rejects PAX/GNU extensions and metadata entries. Hardlinks, devices, FIFOs, sockets and unknown entries are rejected. Paths are bounded UTF-8 relative paths; absolute paths, dot/parent components, backslashes, drive-colon forms, NULs and duplicates fail. Parent directories are opened relative to the pinned extraction root without following symlinks. Relative symlinks with no parent components are deferred until extraction ends and accepted only if their target is an existing regular file or directory. Symlink chains, loops, escapes and hardlinks are unsupported.

Limits are: 64 GiB compressed artifact (P024), expanded stream at most 16 GiB and at most compressed bytes × 1000 + 1 MiB, 4 GiB per file, 200,000 entries, 4096 bytes per path, 32 path components, 8192 symlinks, 16 MiB cumulative archive path bytes, and 16 MiB cumulative symlink-target bytes. Extraction streams file data, uses checked counters, and preflights free space for the selected budget plus archive size. ENOSPC can still occur and fails before publication. Extracted files become 0600 or 0700 depending only on whether any executable bit was present; directories become 0700. UID/GID, setuid/setgid, ACLs, capabilities and extended attributes are not restored.

The synthetic structural contract checks Proton `proton`, `files/`, and a `version` file exactly matching the signed build. Wine checks `bin/wine`, `lib/`, and the same version convention. Launchers must be regular executable files; symlink launchers are not accepted. These layouts are intentionally narrow and do not establish that every upstream Wine/Proton archive is supported or that a runtime will execute successfully.

After structural validation, the installer writes a private versioned ownership record inside the staged tree with plan identity, source/build/artifact/manifest identities, trust classification, generation, and time. Files and directories are synced before Linux `renameat2(RENAME_NOREPLACE)` publishes the tree. Parent directories are synced after rename. If a post-rename sync fails, the call reports `ReconciliationRequired`; status checks the record rather than treating the visible tree as absent. Atomic visibility does not guarantee survival of every storage-device or power-loss failure. The record reports what Endfield-Unix installed and is not a P026 installed-file audit.

## Recovery and concurrency

Each private stage has a versioned transaction journal recording the plan and last durable phase. A new apply must pass fresh signature, artifact, plan, and user-approval checks and acquire the installer lock before reconciliation. It removes only staging directories with a private ownership check, a valid journal, and a plan identity exactly matching that newly approved apply. Cleanup uses directory-relative no-follow operations. Other incomplete or malformed entries remain in place and are reported as ambiguous; no arbitrary root is scanned and no unknown directory is recursively deleted.

`status` distinguishes recorded installations, incomplete staging, validated staging needing fresh approval, and ambiguous entries. Inventory reads are capped at 200,000 managed directories and recovery scans at 1,024 staged transactions; exceeding either bound returns a typed limit error rather than a partial success. It never treats extracted files as trust evidence or publishes a recovered stage. A crash after publication leaves the embedded ownership record in the visible tree; a crash before publication leaves the journal under `.staging`. Reapplying the same authorized plan can safely reconcile its owned stage and restage from freshly verified source bytes. A different plan requires new review and approval. A target collision leaves the existing installation untouched.

The current lock is process-level and global to the managed runtimes root. Concurrent requests return promptly with a busy or destination-collision error. There is no stale-lock-file deletion; advisory locks are released by the kernel when a process exits. Tests cover simultaneous same-destination attempts, source mutation after the initial verification, and deterministic failure hooks from lock acquisition through record creation and publication. Failure after publication returns a reconciliation-required result and status confirms the published record.

## Validation limits and future phases

Focused tests cover synthetic Wine/Proton trees, exact-plan and limited-trust approval, digest changes, extraction traversal/absolute paths, symlink escape and loops, hardlinks and special entries, duplicate entries, oversized headers, truncated/corrupt gzip, empty archive, side-by-side installs, collision preservation, interrupted staging reconciliation, publication failure injection, and concurrency. Hosted [run 40872573654](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40872573654) passed formatting, native workspace check, Clippy with warnings denied, P024 verification, P025 policy/extraction/recovery/concurrency tests, actual CLI plan/reject/apply/inventory smoke in an isolated XDG root, P021–P024 and broader discovery/telemetry contracts, full workspace/CLI/FFI/storage/domain/diagnostics tests, session/daemon/systemd checks, the dedicated IPC integration gate, GTK smoke/screenshot upload, workspace build, metadata, and GCC/Clang C17 ABI jobs. The optional real systemd user-manager gate was not enabled. The historical IPC capacity race did not recur in this run and remains unresolved.

Qualification history is retained: the first P025 hosted attempt caught a large CLI enum Clippy warning; the next attempt caught nested-help behavior in an existing CLI assertion; the following attempt caught a needless-borrow lint in the new parser test. Those issues were fixed in focused commits and the final hosted run above passed. The pre-existing dirty-tree CLI apply had failed closed with `TrustUnavailable` because P024 CLI evidence was user-pinned while the initial P025 apply path accepted only publisher-authenticated provenance. P025A completed the explicit, separately acknowledged user-pinned route; it did not weaken publisher checks or promote P021 `CandidateIdentified` to `ArtifactIntegrityVerified`.

P026 owns installed-file integrity auditing. P027 owns runtime pinning and selection. P025 never automatically switches a runtime. The synthetic GE-Proton10-26 fixture is not an authentic release.

Real GE-Proton installation: UNVERIFIED

Real Wine installation: UNVERIFIED

Official publisher artifact authenticity: UNVERIFIED

Arch desktop installation: UNVERIFIED

Dell Vostro installation: UNVERIFIED

Real Endfield compatibility: UNVERIFIED


P026 installed runtime auditing is documented in [RUNTIME-INTEGRITY-AUDIT-LINUX.md](RUNTIME-INTEGRITY-AUDIT-LINUX.md). It compares new-install local snapshots and leaves P025 schema-v1 installations without baselines Unverifiable. P026 supports optional P024 source-backed reconstruction with user-pinned CLI evidence; hosted qualification remains pending, and no game compatibility claim is made.
