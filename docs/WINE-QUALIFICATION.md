# Managed Wine qualification (P034)

Authentic Wine execution is **NOT_RUN**. No independently authenticated Wine 10.0 package with a matching qualification profile was available for this continuation. The disposable harness can reach the descriptor-bound executor when its gates pass. Production prefix initialization and game launch have separate eligibility and approval requirements and remain blocked.

`endfield-unix diagnose wine-runtime --profile desktop --json` inspects the selected P027 pin, P025 ownership, P026 bytes, artifact profile, host ABI and qualification blockers. It creates no XDG storage and starts no subprocess. Set `ENDFIELD_AUTHENTIC_WINE_TEST_ROOT` to consult one preserved, matching test journal. The recorded status reports an absent, inconclusive, stale, unreadable or recorded outcome. A diagnostic record cannot grant an execution capability. GTK performs the same inspection on its existing worker. Its Play action stays disabled. [Diagnostic schema](schema/wine-runtime-qualification-v1.schema.json)

## Authenticated package contract

The supported profile format describes a relocatable, headless upstream Wine 10.0 Linux x86-64 WoW64 build without a native preloader. The build uses `bin`, a configured relative library directory, and `share` under one configured prefix. The profile declares the exact source tag and commit, loader configuration, minimum host libraries and required initialization roles. A publisher or independently authenticated artifact source must supply the reviewed build and inventory. Repository test fixtures use synthetic payloads and keys.

P024 manifest schema 2 adds `wine_qualification_profile_sha256` to an artifact entry. The digest covers the exact detached profile JSON bytes. The P025 record and exact P027 pin already bind the authenticated manifest digest. Supply the original manifest and profile as `manifest.json` and `profile.json` in `$XDG_DATA_HOME/endfield-unix/wine-qualification/<installation-id>/`. Keep those sidecars outside the installed payload. The installer still requires a genuine P025-managed Wine tarball with its required `version` file. The regular CLI's user-pinned trust tier does not meet this qualification's independently authenticated publisher requirement.

The profile uses these version 1 fields.

| Field | Value or contract |
| --- | --- |
| `schema_version` | `1` |
| `build`, `source_ref` | `wine-10.0` |
| `source_commit` | `b073859675060c9211fcbccfd90e4e87520dc2c2` |
| `architecture` | `x86_64` |
| `build_configuration` | `linux-x86_64-wow64-no-preloader-headless` |
| `invocation` | `wineboot.exe --init` |
| `configure_prefix`, `libdir` | Exact compiled common absolute prefix and relative library directory |
| `interpreter` | `/lib64/ld-linux-x86-64.so.2` or `/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2` |
| `host_libraries` | Extra required host SONAMEs involved in dynamic initialization |
| `inventory_sha256` | P026 inventory digest over the normalized P025-installed archive contents |
| `components` | Exact objects with `role`, `path`, `kind`, and `usage`. Kinds are `native_executable`, `unix_module`, `windows_module`, and `resource`. Usage is `initialization`, `host_dependent`, `optional`, or `gameplay`. |

The core role paths are `launcher=bin/wine`, `wineserver=bin/wineserver`, `ntdll_unix=<libdir>/wine/x86_64-unix/ntdll.so`, and `wine_inf=share/wine/wine.inf`. The `ntdll`, `wineboot`, `kernel32`, `kernelbase`, `services`, `rundll32`, `setupapi`, `advapi32`, `shell32`, `ws2_32`, and `newdev` roles map to their matching DLL or EXE under `<libdir>/wine/x86_64-windows/`. The `wow64_rundll32` and `wow64_setupapi` roles map to the corresponding files under `<libdir>/wine/i386-windows/`. All are required. The signed profile must enumerate any additional exact initialization modules and helpers dictated by the build, including required PE imports, delay imports, and runtime loads. Optional and gameplay roles contribute only to the signed package inventory. Native host-dependent roles are included in the ABI check.

Duplicate roles or paths, unsupported layouts, symlinks in component paths, malformed paths, stale generations and changed P026 inventory are refused. The complete inventory digest binds all package entries, including additional payload files. Required role bytes are opened via the no-follow trusted root and must match that inventory. The qualification plan then binds the endpoint-specific installation ID, generation, P026 baseline identity, and P027 revision. A schema 1 manifest or candidate basename inventory supplies no authenticated role evidence.

Wine 10.0 uses `/proc/self/exe` to discover the launcher directory and dynamically loads `ntdll.so` using its configured BINDIR/LIBDIR relationship. The native loader tries the installed sibling wineserver first and has a compiled-in fallback. Qualification checks that the fallback is absent on the host and supplies no PATH or Wine loader overrides. `wineboot.exe --init` starts services and can exit zero even when `update_wineprefix` fails to find `wine.inf`. The source review defines the accepted contract and does not verify any installed bytes. [Launcher](https://github.com/wine-mirror/wine/blob/wine-10.0/loader/main.c) · [NTDLL loader](https://github.com/wine-mirror/wine/blob/wine-10.0/dlls/ntdll/unix/loader.c) · [Wineboot](https://github.com/wine-mirror/wine/blob/wine-10.0/programs/wineboot/wineboot.c)

## Host ABI and loader assumptions

The bounded inspector reads ELF64 little-endian program headers, PT_INTERP, DT_NEEDED, search paths, and GNU version requirements and definitions. For native qualification, each required version name, including GLIBC_* names, must occur in the observed provider's definition table. Native launchers and wineserver require the exact glibc interpreter. The ABI gate distinguishes supported under documented assumptions, missing dependency, incompatible ABI, unsupported loader configuration, and unverifiable. It never runs `ldd` or executes an inspected file.

Qualification refuses RPATH/RUNPATH, unsafe interpreter candidates, loader audit/filter configurations, global preloads and ambiguous default library candidates. It resolves dependencies from root-owned, non-writable symlink chains in `/usr/lib/x86_64-linux-gnu`, `/usr/lib64`, and `/usr/lib`. Observed system bytes, the loader cache, kernel release and boot identity enter the host fingerprint. The read-only diagnostic's legacy dependency graph can also describe contained managed `$ORIGIN` paths. That broader graph does not authorize an execution.

A passing host assessment assumes distribution-controlled loader cache and hwcaps entries are trusted and compatible with the inspected providers. It does not model cache choice, alternate hwcaps paths, individual relocations, NSS, all `dlopen` calls, or distribution package changes during execution. The signed layout and minimum initialization closure are inputs to the authentic observation. Trust extends to root/kernel/mount administration and excludes an adversary already holding the same UID.

## Private headless environment

Supply an absolute, new disposable root under an existing UID-owned, mode-0700 parent. The harness creates separate mode-0700 `home`, `config`, `cache`, `data`, `runtime`, `tmp`, `records`, `evidence`, and `logs` directories. Its P028 registry and single managed prefix live below the test data directory. It reads the original runtime, pins, and applicable recovery metadata from the source AppPaths during both stages. It never copies or mutates the real pin or prefix registry.

The descriptor-bound child receives only HOME, XDG_CONFIG_HOME, XDG_CACHE_HOME, XDG_DATA_HOME, XDG_RUNTIME_DIR, TMPDIR, WINEPREFIX, a fixed USER and LC_ALL. It receives no inherited display, authentication, session bus, Wine override or dynamic loader variables. The private runtime directory follows the XDG ownership and mode contract but supplies no session bus or compositor. Builds requiring desktop/session services fail the headless qualification contract. There is no fallback to the real session runtime directory.

This environment controls paths passed to the child. It does not impose a filesystem, process, or network sandbox. Wine 10.0 Linux uses `/tmp/.wine-UID/server-DEVICE-INODE` regardless of TMPDIR or XDG_RUNTIME_DIR. Qualification checks the existing base ownership and permissions and requires the exact server path to be unused. The evidence records bounded entries within the disposable tree and whether that exact server directory appeared. The test preserves the test root, server artifacts, and partial prefix on failure. Use a non-root disposable account or machine when the surrounding filesystem and network must also be isolated. [Wine server directory and lock source](https://github.com/wine-mirror/wine/blob/wine-10.0/server/request.c)

## Two-stage harness

Set the source installation's XDG environment and these selectors in both stages.

```text
ENDFIELD_AUTHENTIC_WINE_PIN=<exact-existing-managed-pin>
ENDFIELD_AUTHENTIC_WINE_REVISION=<exact-P027-registry-revision>
ENDFIELD_AUTHENTIC_WINE_ARTIFACT_SHA256=<artifact-digest>
ENDFIELD_AUTHENTIC_WINE_TEST_ROOT=<new-absolute-directory-under-private-parent>
```

Run preparation first.

```bash
ENDFIELD_AUTHENTIC_WINE_STAGE=prepare \
  cargo +1.85.0 test --locked -p endfield-unix-prefix-management \
  --test authentic_wine authentic_wine_initialization_opt_in -- --ignored --exact --nocapture
```

Preparation exits without spawning. It records preflight evidence in `evidence/preflight.json` and, when eligible, writes the exact plan in `records/plan.json`. Review the runtime digest, host fingerprint, prefix identity, environment, invocation, effects and plan ID. With separate approval of that exact attempt, run the same test with `ENDFIELD_AUTHENTIC_WINE_STAGE=execute`, `ENDFIELD_AUTHENTIC_WINE_OPT_IN=I_ACCEPT_DISPOSABLE_WINE_EXECUTION` and `ENDFIELD_AUTHENTIC_WINE_PLAN_APPROVAL=<reviewed-plan-id>`. Preparation refuses existing roots. Execution uses the original prepared root, recomputes the plan and refuses any change. This test is ignored by default and regular CI never enables it.

Execution consumes the approval with an exclusive, fsynced `records/attempt.json` before the existing P028 coordinator publishes Starting and spawns the verified descriptor. Running is published before exit observation. Observing zero exit moves the prefix to VerificationPending. Qualification checks the exact wineserver lock twice, requires its socket absent and an idle visible prefix, and then checks that Wine wrote a fresh `.update-timestamp` matching the verified `wine.inf` modification time. The stable P028 prefix structural verifier runs after those checks. The lock query is read-only. The harness invokes neither `wineserver --wait` nor `--kill`. Unknown or still-active helpers yield `QUIESCENCE_UNVERIFIED`, even with an otherwise plausible prefix. The check covers the source-defined server lock and visible same-UID processes carrying the exact WINEPREFIX. A same-UID process with unreadable environment can block preflight or completion. It cannot prove all descendant or external activity has stopped.

The harness bounds direct observation to 60 seconds and quiescence observation to 10 seconds. Its cancellation addresses only the directly owned initializer. Bounded logs and `evidence/result.json` record the observed status, exact plan, attempt, prefix verifier output, quiescence and side effects. An attempt marker with no valid final result is inconclusive even if the earlier preflight document says NOT_RUN. There is no automatic rerun, deletion, helper signaling or promotion after crash, timeout or failed publication. A successful disposable outcome is specific to the artifact, installation, pin, host and plan. Diagnostic labels cannot grant production authorization.

## Validation scope

Native fixtures exercise signed profile binding, host ABI inspection, exact approval, coordinator execution, verified and inconclusive completion, side effects and replay refusal. They use a test-only process table so unrelated same-UID processes cannot make fixture outcomes nondeterministic. Production qualification always scans the real process table. The authentic harness stays out of default tests. The matching independently authenticated binary package, explicit approval, authentic initialization and helper completion, production authorization and game integration remain outstanding. P034 remains partial and P035 has not started. [Draft PR #7](https://github.com/nghianguyen150612/Endfield-Linux/pull/7)

## Wine 10.0 source acquisition observation

The Wine 10.0 source archive and detached signature were obtained from the WineHQ URLs linked by the release announcement. The archive SHA-256 observed locally is `c5e0b3f5f7efafb30e9cd4d9c624b85c583171d33549d933cd3402f341ac3601`. Its detached signature is valid against the key retrieved from keys.openpgp.org with fingerprint `DA23579A74D4AD9AF9D3F945CEFAC8EAAF17519D`. The keyserver identifies the key as Alexandre Julliard. Independently administered Arch Linux packaging lists that same fingerprint as an accepted Wine source signing key in its current `PKGBUILD` and in its historical package records. Under the explicit assumption that the official Arch packaging repository is a trusted independent source for release-signing keys, the source archive is **SOURCE_AUTHENTICATED**. The Wine 10.0 tag also verifies with this key and resolves to commit `b073859675060c9211fcbccfd90e4e87520dc2c2`. The generic inspection tool reports `VALID_FOR_SUPPLIED_KEY` because it does not itself authenticate the caller-supplied key. The WineHQ package repository key has a different fingerprint and does not authenticate the source signer. [Release announcement](https://list.winehq.org/hyperkitty/list/wine-announce@list.winehq.org/thread/JN2HC7JA7DUGMRJBAD7RPYLE65K3MAFQ/) · [Source archive](https://dl.winehq.org/wine/source/10.0/wine-10.0.tar.xz) · [Detached signature](https://dl.winehq.org/wine/source/10.0/wine-10.0.tar.xz.sign) · [Arch Wine source-key policy at commit 721fbed](https://gitlab.archlinux.org/archlinux/packaging/packages/wine/-/blob/721fbed8cae2dbd8b2a59bca613317b6ace78ce5/PKGBUILD) · [Historical Arch package record](https://lists.archlinux.org/pipermail/arch-commits/2016-February/315850.html)

`scripts/inspect-wine-source.py` verifies a caller-supplied archive digest and detached signature against a caller-supplied key in a temporary GPG home. It inspects bounded source archive entries without extraction and refuses links, special entries, traversal, duplicates and oversized inputs. Its output explicitly records that signer identity is not independently authenticated. The separate `scripts/acquire-wine-source.py` explicitly downloads the fixed WineHQ source and signature into a new private directory, enforces size limits, and runs that inspector. It records results without promoting signer identity. The workflow does not install a runtime, generate a binary profile, or grant P025/P027 trust.

Wine 10.0's actual `configure.ac` enables a native preloader on Linux for known host architectures. No supported disable-preloader configure option was identified. This conflicts with the current no-preloader qualification profile, and a compatible build must be independently demonstrated before the profile can describe an authentic upstream build. The archive and signature do not authenticate locally built binaries. No source build, reproducibility comparison, managed installation, role profile, or authentic qualification plan has been produced from this source. [Wine 10.0 configure source](https://github.com/wine-mirror/wine/blob/wine-10.0/configure.ac) · [Loader build rules](https://github.com/wine-mirror/wine/blob/wine-10.0/loader/Makefile.in)

WineHQ's Ubuntu Noble package index lists `wine-stable-amd64` version `10.0.0.0~noble-1`. The observed package SHA-256 is `393d545040fe5c029eadeb31fcea52126714473e6c11b3cd851447b2dcbf1f72`, matching the index. The index SHA-256 matches the WineHQ `InRelease` metadata, whose signature verified against WineHQ's published package key `D43F640145369C51D786DDEA76F1A20FF987672F`. The package key's identity has not been independently authenticated in this investigation, so binary publisher authenticity remains **UNVERIFIED**. The package contents include `opt/wine-stable/bin/wine64`, `opt/wine-stable/bin/wine64-preloader`, `opt/wine-stable/lib64/wine/x86_64-unix/ntdll.so`, and `opt/wine-stable/lib64/wine/x86_64-windows/wineboot.exe`. The package therefore does not directly match the current managed `bin/wine`, no-preloader profile. WineHQ's `wine-stable` metapackage also depends on separate AMD64 and i386 packages. No package was installed or executed. [WineHQ repository metadata](https://dl.winehq.org/wine-builds/ubuntu/dists/noble/InRelease) · [Package index](https://dl.winehq.org/wine-builds/ubuntu/dists/noble/main/binary-amd64/Packages.gz) · [AMD64 package](https://dl.winehq.org/wine-builds/ubuntu/pool/main/w/wine/wine-stable-amd64_10.0.0.0~noble-1_amd64.deb)
