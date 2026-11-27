# Wine 10.0 real binary candidate (P034 continuation)

Historical two-package report: the sections below preserve the earlier AMD64/common
checkpoint. The current continuation recovered and verified the complete three-package
candidate, enrolled a disposable development-scoped P024 manifest, completed disposable
P025/P026/P027 state, built a byte-reproducible SquashFS image, and verified a private
read-only mount plus native preloader/helper handoff. The application still has no
owned mounted execution capability; production authority, complete i386 host ABI and
authentic qualification remain blocked. See [package closure](WINE-PACKAGE-CLOSURE.md),
[runtime image provider](RUNTIME-IMAGE-PROVIDER.md), and [qualification status](WINE-QUALIFICATION.md).

The following sections preserve the earlier two-package checkpoint.

Checkpoint status: **inspectable, limited-trust candidate; execution blocked**. This is the
first extracted binary candidate, not a trusted managed runtime. P034 is active;
P035 has not begun. Wine and Endfield were not executed. No package was installed.
PR #7 remains Draft and unmerged.

## Source-defined loader contract

The reviewed files match the source archive with SHA-256
`c5e0b3f5f7efafb30e9cd4d9c624b85c583171d33549d933cd3402f341ac3601`.
[File bindings](artifact-evidence/wine-10.0-loader-source-binding.json) connect
this review to the earlier independent Arch source-key policy. That policy does
not authenticate package binaries or their configure arguments.

At upstream commit `b073859675060c9211fcbccfd90e4e87520dc2c2`:

- `configure.ac:2033` chooses `wine` for x86-64 with i386 PE targets and `wine64`
  otherwise. Linux with a known host architecture enables HAVE_WINE_PRELOADER.
  `loader/Makefile.in` builds both launcher variants from main.o, and their
  preloaders from preloader.o/preloader_mac.o. No supported disable switch was
  identified. New WoW64 and classic paired builds have different loader names
  and architecture transitions; the package's AMD64 layout alone does not prove
  a new WoW64 build.
- `loader/main.c:163` uses `/proc/self/exe` on Linux, realpath and the compiled
  BINDIR/LIBDIR relationship to locate `wine/<architecture>-unix/ntdll.so`.
  WINEDLLPATH is another search input; the execution environment forbids it.
  The compiled LIBDIR fallback applies when self discovery is unavailable,
  not as a universal rescue after relocation.
- `dlls/ntdll/unix/loader.c:460` uses dladdr to locate Unix ntdll, discovers
  bin_dir from `/proc/self/exe`, constructs data_dir from BINDIR/DATADIR, and
  constructs wineloader from the invocation basename. Correct argv[0] matters.
  It establishes Unix/Windows module search paths from this layout.
- `__wine_main` checks WINELOADERNOEXEC. For initialization, absent that variable,
  pre_exec requests a re-exec when HAVE_WINE_PRELOADER is set. `preloader_exec`
  chooses the sibling `wine64-preloader` when the loader name ends in `64`,
  otherwise `wine-preloader`. It execv's that pathname and falls back to execv
  of the loader pathname if the preloader exec fails. The presence of a file
  alone does not establish this invocation; these source paths do.
- `loader/preloader.c:1435` reserves address ranges, opens/maps argv[1] as the
  main ELF, maps its interpreter, publishes preload reservation information,
  rewrites auxiliary values, removes its first argument, and transfers control
  to the interpreter. The preloader is not simply an optional wrapper whose
  descriptor transitively authenticates the launcher.
- `exec_wineloader` also uses the preloader path for child loaders and alternate
  architectures. `start_server:622` permits WINESERVER, then build/sibling
  discovery and finally compiled BINDIR wineserver. Environment filtering and
  absence of a fallback must remain verified, not assumed.

## Descriptor compatibility and preloader policy

Linux execveat(AT_EMPTY_PATH) executes the opened inode. `/proc/self/exe` refers
to its filesystem pathname, follows renames, and gains ` (deleted)` after unlink.
The native fixture verifies these observations, executable/parent replacement,
relative helper lookup, and substitution of an unbound helper. Existing executor
fixtures continue to cover held roots, generation changes and executable identity.
These fixtures do not establish authentic Wine behavior.

**Compatibility: conditional for the initial ELF path discovery; incompatible
with a claim that the existing descriptor binds the complete Wine execution
chain. Authentic Wine qualification remains NOT_RUN.** An intact on-disk common
prefix and stable relative layout may permit initial discovery. The subsequent
preloader exec, its main-ELF open, child loaders and wineserver use pathnames;
the initial descriptor does not bind their identities. Deleted or moved layouts
can also invalidate lookup semantics. No production pathname fallback was added.

Schema 1 retains its existing conservative executable policy and remains
unsupported for these real binaries. Schema 2 represents exact observed roles,
source reference versus binary attestation, build identity/configuration,
installed prefix, module directories, initialization/resources/helpers,
preloader/invocation policy, compiler availability, host requirements, package
and inventory digests. Its validator is deliberately **non-authorizing**.
Production recognition returns `preloader_handoff_unverified`; a schema-v2
observation cannot upgrade a schema-v1 build or create an authorization.

A future execution design must bind all Wine-opened paths to an immutable,
verified runtime view, including dynamic modules and helper fallbacks, and define
ownership/quiescence for the whole helper lifecycle. Rechecking a mutable pathname
before exec is insufficient. This iteration enables no alternate strategy.

## Acquired binary evidence

| Package, version 10.0.0.0~noble-1 | Bytes | SHA-256 |
| --- | ---: | --- |
| wine-stable-amd64 (amd64) | 117345946 | `393d545040fe5c029eadeb31fcea52126714473e6c11b3cd851447b2dcbf1f72` |
| wine-stable (amd64/common) | 2816424 | `31e6eb2e15ae2e38ef72c9865c0d01da4bc587e554e7db46fe1a75b4420878e1` |

The newly retrieved Noble InRelease (dated 2026-11-14) has a valid OpenPGP
signature for `D43F640145369C51D786DDEA76F1A20FF987672F`. Its SHA256 section
matches the newly downloaded AMD64 Packages index; exact package records match
the downloaded DEB bytes. The key came from WineHQ itself. A valid generic UID certification on that repository key was additionally
verified from source signer `DA23579A74D4AD9AF9D3F945CEFAC8EAAF17519D`, independently
corroborated by the existing Arch source-key policy. This is independent
cryptographic corroboration of repository-key identity; see the [GPG certification
result](artifact-evidence/winehq-key-certification.txt) and preserved [source
public key](artifact-evidence/wine-source-signer-key.asc). The certification is
from 2018 and supplies no reproducible-build or package-content attestation.

The existing policy accepts the certifier for source releases, not as an automatic
authority to enroll a production binary publisher. Repository integrity is
verified and key identity is corroborated, while the candidate's production
publisher tier remains unverified/unpromoted under that policy. Neither this
certification nor OpenPGP metadata satisfies P024's Ed25519 manifest contract.

The [status and dependency records](artifact-evidence/winehq-noble-10.0-candidate-status.json),
[InRelease](artifact-evidence/winehq-noble-InRelease.asc),
[normalized index copy](artifact-evidence/winehq-noble-Packages.gz), and
[public key](artifact-evidence/winehq-repository-key.asc) preserve the chain for
review. The gzip index is a locally normalized copy; decompress it before checking
the signed uncompressed digest. The preserved public key is evidence, not a trust anchor.

The common package supplies wineserver and wine.inf. It requires both exact AMD64
and i386 packages. The i386 companion is explicitly unacquired, so the package set
is incomplete. The complete declared Depends, Pre-Depends and Recommends are in
the status records. They include Ubuntu t64 names and libc6 >= 2.38. Host package
installation and optional graphics/media libraries were not inferred from filenames.

## Inventory, profile and ABI

The [1,502-entry inventory](artifact-evidence/winehq-noble-10.0-candidate-inventory.json)
is derived from actual package bytes, sorted by normalized path with sizes,
SHA-256, original executable classification and P026-compatible entry encoding.
Its digest is `184405a6b524a3b5ae33d159a8b415d98c7b5548803c7da48531af3a976f5192`.
The [schema-v2 candidate profile](artifact-evidence/winehq-noble-10.0-candidate-profile.json)
contains exact regular-file roles. It passes structural byte-role validation.
This does not authenticate role semantics, prove complete module closure or attest
the binary's source commit. No profile signature was fabricated.

Staging is private at `/workspace/wine-candidate/combined/staging` on this disposable
host. All extracted regular files are mode 0600; directories are 0700. Symlinks are
recorded but never materialized, and maintainer scripts are never executed. Thus
staging modes differ from original payload classification. Candidate bytes and
full source/control inventories remain under `/workspace/wine-candidate/`.
The inspector refuses unsafe paths, link parents, special files, duplicates,
setid and expansion limits. It supports the actual zstd-compressed DEB members.

The [ABI report](artifact-evidence/winehq-noble-10.0-host-abi.json) applies the
existing read-only ELF and host ABI inspectors to **44 actual native ELF files**
on Debian 13 x86-64. The launcher uses `/lib64/ld-linux-x86-64.so.2` and requires
GLIBC through 2.34; wineserver requires GLIBC_2.38. The preloader is ET_DYN without
PT_INTERP or DT_NEEDED. The current native-executable ABI contract expects a glibc
interpreter, so its full-policy assessment is ABI_INCOMPATIBLE (policy mismatch,
not proof the preloader cannot run). Diagnostic exclusion of the preloader then
reports missing ntdll.so: the current host closure resolver does not qualify that
managed Unix-library dependency. No exclusion authorizes execution. Individual
interpreter, DT_NEEDED and GNU versions are recorded for each object. No ldd was
used. Full symbol resolution, dlopen closure, i386 ABI, host fingerprint and
cross-distribution compatibility remain unqualified.

## P024–P028 and qualification result

| Boundary | Result |
| --- | --- |
| P024 schema 2 | Can describe an exact digest/profile, but no independently authenticated Ed25519 manifest exists. Repository OpenPGP evidence is not a P024 signature. No fake manifest emitted. |
| P025 | Ineligible: DEB is unsupported; payload lacks root bin/wine, lib and version; package set incomplete; transformed publisher authentication absent. |
| Transformation | Not performed: required complete managed role set and invocation cannot be established. Exact inspected bytes are preserved; no renamed wrapper or fabricated upstream signature. |
| P026 | Compatible normalized source inventory encoding; no real installed baseline, generation or ownership claim. |
| P027 | Ineligible: no P025 installation or authenticated generation pin. Existing stale-pin rejection preserved. |
| P028/harness | Existing coordinator and two-stage harness preserved. No executable plan can pass current provenance, package, ABI and handoff gates. |

Candidate states are independently recorded: ACQUIRED, INTEGRITY_VERIFIED,
PUBLISHER_IDENTITY_UNVERIFIED, PROFILE_GENERATED, PROFILE_VALIDATED,
INCOMPLETE_PACKAGE_SET, HOST_ABI_BLOCKED. It is neither MANAGED_INSTALLATION_ELIGIBLE
nor AUTHENTIC_QUALIFICATION_READY. Production authorization remains blocked.

A source build was not attempted: the current host lacks bison, flex and Clang.
The machine began with about 7.5 GiB free, GCC and GPG; Rust 1.85 was installed
privately for project checks. No system packages were installed. Configure/new
WoW64 cross-toolchain requirements must be resolved before a controlled build.
Reproducibility is NOT_VERIFIED.

## Reproduction and tests

`scripts/acquire-winehq-candidate.py <new-private-destination>` explicitly downloads
bounded metadata and exact packages, checks the signer fingerprint and signed
index digest, and runs the offline inspector and role generator. It refuses
redirects and unknown signers, and never grants publisher identity. The preserved
candidate was acquired with equivalent explicit bounded download and GPG/index
verification steps; the orchestration script is provided for future fresh runs.

Default CI uses only offline malicious archive fixtures, the committed inventory
and profile, and harmless native descriptor fixtures. It never downloads or runs
Wine. Candidate ABI observation is an ignored, explicitly selected read-only unit
test; it cannot invoke the authentic execution harness. Existing P019/P021–P033
regressions, C17 ABI 0x00010000, and exactly two application binaries remain required.

This historical checkpoint delivered real binary evidence but **did not complete
P034 acceptance**. Its remaining-work list is superseded by the current continuation
at the top of this document and the linked package/provider evidence.

Local validation used genuine rustc 1.85.0 (4d91de4e4). Formatting and locked
metadata passed. The non-GTK workspace run passed 804 tests (six ignored) before
the final additive candidate checks; the final prefix-management checks are run
separately. Prefix-management check and all-target Clippy pass without warnings.
The full locked workspace check, Clippy, test and build commands were attempted
and blocked by absent GLib/GTK pkg-config development files. GCC C17 ABI tests
and GCC sanitizers passed. Clang and local GTK smoke are unavailable here;
final-head hosted CI is the required source of those results. No warning or test
was suppressed. The ABI remains 0x00010000 and no application binary was added.
