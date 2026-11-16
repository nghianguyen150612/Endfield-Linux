# Managed Wine qualification (P034)

## Current eligibility

Production initialization and game launch remain unsupported. Authentic Wine qualification is **NOT_RUN**. No Wine or Endfield process was executed for this iteration. No production artifact compatibility profile is shipped. The new report and harness make prerequisite evidence explicit; they do not establish authentic Wine success.

`endfield-unix diagnose wine-runtime --profile desktop --json` selects the existing P027 profile/machine pin without fallback to another installation. It independently revalidates P025 ownership and P026 inventory, opens the exact `bin/wine` descriptor, then inspects dependencies and helper candidates. It creates no XDG storage, downloads nothing, and starts no subprocess. Exit zero means a diagnostic report was emitted; inspect `production_initialization` and `blockers` for eligibility. JSON schema: `docs/schema/wine-runtime-qualification-v1.schema.json`. Private absolute paths and search paths are redacted. GTK's **Inspect Wine qualification** action performs this work off the main thread and reports selected runtime, evidence and blockers. Play remains disabled.

## ELF and loader policy

The memory-safe parser supports ELF64, little-endian, x86-64, ET_EXEC/ET_DYN. Other classes/architectures are unsupported. It bounds individual files to 64 MiB, program headers to 256, dynamic entries to 4096, imports to 128 and each dynamic string to 4096 bytes. The graph bounds distinct objects to 128, pending/output edges and total bytes to 256 MiB. Malformed headers, offsets, string tables, unterminated dynamic tables, audit/filter/auxiliary loading and NODEFLIB resolution patterns block inspection. No `ldd` or inspected program is executed.

The inspector reads PT_INTERP, DT_NEEDED, DT_RPATH and DT_RUNPATH. Only `$ORIGIN`/`${ORIGIN}` followed by normal contained components is modelled. Empty/current-directory components, traversal, absolute overrides, `$LIB`, `$PLATFORM`, slash-bearing imports and other tokens are unsupported. Managed symlinks and subordinate mounts are refused. The executor clears the environment and rejects PATH, LD_*, GLIBC_TUNABLES, GCONV_PATH, LOCPATH and unsupported WINE*/PROTON overrides; no ambient loader variable is inherited.

A graph node is one of `verified_managed`, `trusted_system`, `missing`, `untrusted`, `unverifiable`, `dynamic_not_established`, or `unsupported`:

- Managed bytes must match a regular non-setid P026 baseline entry through no-follow descriptor-relative reads, with ownership/mode/mount checks and bounded stable reads. This is installed-artifact integrity evidence, not new publisher authentication.
- System candidates use an explicit distribution trust assumption: canonical files under `/usr/lib` or `/usr/lib64`, root-owned and not group/world writable, with those checks on every canonical ancestor. Canonical system symlink aliases are allowed only inside that domain. Their hashes record observations; they are not publisher attestations. Root/kernel/mount administration is trusted.
- Default candidates are observed only in `/usr/lib/x86_64-linux-gnu`, `/usr/lib64` and `/usr/lib`. Loader cache, glibc-hwcaps, inherited RPATH and combinations of RPATH/RUNPATH are not simulated. The report always records this limitation. A found candidate is not a claim that the real loader must choose it.
- Static DT_NEEDED inspection never proves `dlopen`, NSS, graphics driver, Wine module or helper closure. This uncertainty always remains explicit and blocks production policy.

These checks do not protect against a fully adversarial process with the same UID. Hashing mutable files does not establish immutability across subsequent opens. A trusted descriptor protects the top-level ELF identity, not all future pathname-based loads. Filesystem and process observations are bounded evidence under the stated host assumptions, not absolute Linux userspace integrity.

## Wine 10.0 source contract and artifact gap

Reviewed source is the Wine upstream mirror's exact `wine-10.0` tag, over the connected GitHub source interface. Source review is not authentication or qualification of a locally installed build:

- [loader/main.c](https://github.com/wine-mirror/wine/blob/wine-10.0/loader/main.c), blob `ecb51efa136256b5d8a41aeda6f0a884104b118b`: Linux loader discovers `/proc/self/exe`, computes a configured BINDIR/LIBDIR relationship and dynamically loads `wine/x86_64-unix/ntdll.so`. Build-tree and WINEDLLPATH alternatives exist. This directly demonstrates why bin/wine DT_NEEDED is not the Wine closure. Descriptor execution/path discovery still needs authentic qualification.
- [loader/Makefile.in](https://github.com/wine-mirror/wine/blob/wine-10.0/loader/Makefile.in), blob `191ab81e1d234c22a78772330e975a02a03f2195`: loader/preloader selection and compiled installation directories depend on the build configuration.
- [wineboot Makefile](https://github.com/wine-mirror/wine/blob/wine-10.0/programs/wineboot/Makefile.in), blob `0983420a91f0992a36fbde74e1a0f4fae41d93e3`: module is `wineboot.exe`; imports and delay-imports include kernelbase, advapi32, ws2_32, shell32, setupapi and newdev.
- [wineboot.c](https://github.com/wine-mirror/wine/blob/wine-10.0/programs/wineboot/wineboot.c), blob `1b04fd3b202ee3b2fd0cd62b5b5bf1a2dc06ddec`: `wineboot.exe --init` starts services and calls update_wineprefix. That helper can return after failing to locate wine.inf while main ultimately returns zero. Other initialization work includes registry creation, run keys, rundll32 installations and user profile updates.
- [wineserver manual](https://github.com/wine-mirror/wine/blob/wine-10.0/server/wineserver.man.in), blob `5b9bf97e0f4d54218f21f08a6c8bd8af157f832e`: `--wait` waits for server termination; persistence can keep it alive after clients. WINEPREFIX device/inode participates in server socket selection. `--kill` is not used by this implementation.

The source-defined candidate invocation is the exact managed native launcher with native arguments `wineboot.exe`, `--init`, controlled WINEPREFIX and the exact managed prefix as cwd. This is source semantics, **not a verified installed-artifact invocation contract**. No flags are invented or external wineboot substituted.

P024 manifest schema 1 authenticates artifact ID, filename, size and digest, but has no role-to-path map, Wine configure settings, module/helper closure, host ABI constraints or quiescence contract. Its repository fixture is synthetic Proton metadata. P025/P026 bind installation and inventory; they do not fill these missing package semantics. The diagnostic checks candidate inventory bytes for wineserver, wineboot.exe, ntdll.so/ntdll.dll, kernel32/kernelbase, services.exe, rundll32.exe and wine.inf. Missing/ambiguous/replaced candidates are explicit. Matching basename plus matching inventory bytes never qualifies a role or package layout. No different system Wine is adopted.

A future production profile must bind an independently verified exact artifact/layout, configured loader paths, internal module/helper roles, host ABI and an observation mechanism for outstanding initialization activity. No generic Wine 10.x or distribution layout is accepted today.

## Authorization and durable completion

`authorize_production_initialization` accepts a real P028 plan, opaque approval, store and application paths, re-inspects current evidence, and returns `InitializationUnsupported` while policy is blocked. Reports cannot be deserialized into capabilities; production authorization fields remain private and there is no unsafe public constructor. Structural preparation, dependency observations, synthetic execution, authentic qualification and user-approved production initialization remain separate evidence levels.

The existing coordinator is reused. Production checks run before intent, after Starting, and through the runtime validator immediately before spawn and before verified completion. Synthetic tests use their existing test-only capability. Production never accepts synthetic success. The coordinator's durable revision checks, immediate-exit ordering, direct-child ownership and recovery-required paths are unchanged.

Observed exit zero and stable prefix registry/layout are separate facts. Background services or other Wine clients may remain after the direct child exits. No implemented policy establishes quiescence, and `wineserver --wait` is not automatically invoked merely because it is documented: its executable, prefix association, bounded wait and lifecycle effects need qualification. No helper PID/name is signaled. Partial prefix data is never silently deleted or retried; uncertain P028/P029 states remain blocked. Initialized-prefix restore is not newly enabled.

## Explicit opt-in harness

The integration test `crates/prefix-management/tests/authentic_wine.rs` is ignored by default. Default unit tests and hosted CI do not run authentic Wine. It requires all of:

```
ENDFIELD_AUTHENTIC_WINE_OPT_IN=I_ACCEPT_DISPOSABLE_WINE_EXECUTION
ENDFIELD_AUTHENTIC_WINE_PIN=<exact-existing-managed-pin>
ENDFIELD_AUTHENTIC_WINE_REVISION=<exact-registry-revision>
ENDFIELD_AUTHENTIC_WINE_ARTIFACT_SHA256=<exact-artifact-digest>
ENDFIELD_AUTHENTIC_WINE_TEST_ROOT=<new-absolute-directory-under-a-private-parent>
```

With the source installation's explicit XDG environment, invoke:

```
cargo +1.85.0 test --locked -p endfield-unix-prefix-management \
  --test authentic_wine authentic_wine_initialization_opt_in -- --ignored --exact --nocapture
```

**Current expected result is NOT_RUN at policy preflight.** The harness writes scoped evidence into the new private root and returns before creating a prefix or spawning Wine. Opt-in does not bypass any blocker. Do not read an ignored-test or preflight test PASS as authentic qualification.

The wired later stages create a separate managed test prefix and XDG tree, persist a plan for review, require `ENDFIELD_AUTHENTIC_WINE_PLAN_APPROVAL` containing that exact reviewed plan ID, obtain production authorization, use the existing coordinator, bound direct-child observation to 60 seconds, verify completion and persist scoped results. Resuming review requires `ENDFIELD_AUTHENTIC_WINE_RESUME=I_REVIEWED_THE_PRESERVED_TEST_DATA`, the same pin/digest/revision, an existing Created prefix with no intent, and no prior execution. There is no crash retry. Isolated XDG propagation and helper quiescence still need a supported production profile before those execution stages can become reachable.

Evidence records exact runtime/artifact/pin, host assumptions, invocation, lifecycle, process result, verifier result, timestamp, scope and limitations. Every test artifact and prefix is preserved. Inspect before manual cleanup. No user's gameplay prefix is used, no external prefix is adopted, and the Endfield game is never run. A container alone is not treated as complete isolation.

## Validation scope

Native ELF parsing, synthetic graph resolution, helper integrity rejection and existing synthetic coordinator tests establish only their respective code contracts. Authentic build qualification remains NOT_RUN. Exact final-head hosted qualification is recorded in Draft PR #7 and the completion report; historical checkpoint success does not qualify new code. P034 remains partial and P035 has not started.
