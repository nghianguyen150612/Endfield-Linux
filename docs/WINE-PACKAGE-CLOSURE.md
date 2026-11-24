# P034 complete Wine package candidate

This continuation completes the three Wine package payloads and produces a real,
repeatable managed-package candidate. **Execution remains blocked.** Wine,
package maintainer scripts and Endfield were not executed. PR #7 stays Draft and
unmerged; P034 remains active and P035 has not begun.

## Recovery and independent revalidation, 2026-11-24

Recovery started at `164e3c9a974f1f9ec926cdb21672ad6edb7d4298`, three commits
after the requested checkpoint. The branch was clean and matched the fetched
remote; PR #7 was open, Draft and unmerged. Its exact-head CI run
[44883220442](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/44883220442)
was independently checked as successful. All newer implementation was preserved.

A fresh acquisition through `acquire-winehq-candidate.py` verified the live
InRelease signature, both signed index digests and all three downloaded DEBs.
Offline inspection and packaging reproduced the committed source inventory,
transformed inventory and packaging record exactly, including the archive digest.
The actual archive is now also available at
`/workspace/p034-verification/acquired/managed/winehq-noble-10.0-dualarch-candidate.tar.gz`.
See the bounded [revalidation receipt](artifact-evidence/winehq-dualarch-revalidation-2026-11-24.json).

Combined-inventory validation additionally rejects any entry beneath another
package's regular file or symlink. Per-DEB validation alone cannot exclude these
cross-package conflicts. Tests cover both input orders and both ancestor types,
require rejection before candidate output creation, and preserve legitimate
shared directories and architecture-specific descendants. Package-set fixtures
now contain eight tests. This check does not change the genuine candidate bytes,
inventory identities, provenance or execution eligibility.

## Packages and integrity

All packages are WineHQ Ubuntu Noble version `10.0.0.0~noble-1`:

| Package | Architecture | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| wine-stable-amd64 | amd64 | 117345946 | `393d545040fe5c029eadeb31fcea52126714473e6c11b3cd851447b2dcbf1f72` |
| wine-stable-i386 | i386 | 110481570 | `5ba523409f013b12953f577bc3322aeae599bb16afac66c59ebe2dde7b337bfa` |
| wine-stable | amd64/common | 2816424 | `31e6eb2e15ae2e38ef72c9865c0d01da4bc587e554e7db46fe1a75b4420878e1` |

Newly retrieved [InRelease](artifact-evidence/winehq-noble-closure-InRelease.asc)
verifies for `D43F640145369C51D786DDEA76F1A20FF987672F`. The uncompressed AMD64 and
i386 indices match its SHA256 section. Their locally compressed copies are
preserved as `winehq-noble-closure-Packages-{amd64,i386}.gz`; decompress before
checking the signed index digests. Exact name/version/architecture records,
package SHA-256 and the DEB control fields agree. The old AMD64/common DEB bytes
were rechecked against this repository metadata; the missing i386 DEB was acquired
from its verified index record, without guessing a URL or digest.

Repository integrity, key identity corroboration, accepted binary publisher
policy and P024 manifest authentication remain separate. The existing valid
certification from the independently corroborated Wine source signer supports
repository-key identity. It does not automatically enroll a binary publisher or
convert OpenPGP into a P024 Ed25519 signature. No local signing key was generated.

## Inventory and dependency closure

The [combined source evidence](artifact-evidence/winehq-dualarch-source-inventory.json)
contains **2,847 normalized paths**, with package ownership, original archive paths,
original modes/UID/GID, normalized file types, executable classification, sizes
and computed SHA-256. Shared directories retain all owners; duplicate regular-file
ownership and conflicting paths are rejected. Source inventory digest:

`d73e7e769fc0d19ad7fb958264987550648d4f6491762a6df520094431e5b90c`

The common package's exact AMD64 and i386 requirements are now present. This is
**complete closure of the three Wine packages**, not full runtime dependency
closure. The graph separately records Depends, Pre-Depends, alternative groups,
external system requirements and optional Recommends. Native/media/graphics
libraries remain host-dependent. [Installed-package observations](artifact-evidence/winehq-dualarch-system-dependencies.json)
check package architecture and version constraints with read-only dpkg tools;
missing matching names do not prove absence of a virtual provider. No dependency
solver transaction or system installation was performed.

The inspector now also rejects non-root archive ownership and preserves original
mode observations separately from normalized P026 entries. Links are inventoried,
not materialized in inspection staging. Setid, special files, traversal, absolute
paths, duplicate entries, link parents and excessive decoded streams remain
rejected. Acquisition checks distribution, architecture, signature fingerprint,
index digest, package size/hash and control/index agreement.

## Managed candidate and transformation

Actual private candidate:

`/workspace/wine-closure/managed/winehq-noble-10.0-dualarch-candidate.tar.gz`

Size: **437,830,035 bytes**. SHA-256:

`391fe86b400d2cf7a8a58c72866c7a8752d4cc613f5536003a7f45f40f7851ec`

Two independent packaging invocations on these same inspected inputs produced
identical archive bytes and metadata values. This verifies local package
repeatability; it is not a reproducible Wine source-build comparison.

The transformation preserves original payload paths, regular-file bytes and safe
relative symlink targets. It keeps `wine`, `wine64`, both preloaders, wineserver,
`lib/wine/i386-*`, `lib64/wine/x86_64-*`, and resources under `opt/wine-stable`.
No Wine executable was renamed, patched or replaced with a wrapper. Archive
ownership/mtime become zero, directory modes become 0700, and regular-file modes
become 0700/0600 according to original executable classification, matching P025's
existing extraction normalization. Local `version` and `endfield-layout-v1.json`
metadata are added. Timestamp normalization, including wine.inf, is explicit and
requires eventual authentic observation; it is not proof of successful Wineboot.

The [transformation record](artifact-evidence/winehq-dualarch-packaging.json)
binds source package digests, full source evidence, source inventory, artifact
hash, transformed inventory and local candidate generation. This local generation
is not an installed P025 generation or P027 pin. Source evidence and profile
changes invalidate the local receipt. The [profile](artifact-evidence/winehq-dualarch-profile.json)
uses `upstream_packages_locally_transformed`; it cannot impersonate independently
authenticated publisher provenance.

The [transformed inventory](artifact-evidence/winehq-dualarch-transformed-inventory.json)
has **2,849 entries**, digest:

`74bdf3b1215aace973ab7cc2292266194c072c99977836c09e989c7ff3247b24`

P025 adds a narrow `winehq-dualarch-v1` layout descriptor contract. It checks the
exact supported build, all three source package identities, version metadata,
required native executable paths/classes/architectures, both module directory
sets, initialization modules and wine.inf through existing no-follow directory
operations. An unknown or incomplete descriptor fails; it does not fall back to
legacy layout. Legacy Wine/Proton formats remain supported. The descriptor is
layout metadata inside the artifact, never an independent signature authority.

The actual archive passed the existing P025 extractor in a disposable temporary
tree, then layout validation, P026 shadow-baseline creation and full audit.
[Dry-run result](artifact-evidence/winehq-dualarch-p025-dry-run.json) records the
exact transformed inventory. No P025 installation record, managed runtime,
existing installation change, prefix or pin was created. The baseline uses the
existing limited-trust classification solely for inspection, not an actual user pin.

## Preloader-aware ABI and host assessment

Read-only ELF candidate inspection now supports ELF32 LE i386 and ELF64 LE x86-64.
The production executor's existing x86-64-only `inspect` boundary remains intact;
a new candidate parser does not promote architecture support for execution.

Ordinary dynamic ELF, static ELF, interpreter-free ET_DYN and unsupported files
are classified separately. A Wine-preloader assessment requires the exact
candidate role, inventory digest, executable entry and file hash, then verifies
ELF class/machine/type, bounded program headers, file-backed executable entry,
alignment and absence of writable-executable load segments, interpreter and
ordinary library imports. It does not recognize a preloader from its basename.

Actual i386 preloader: **static ET_EXEC**. Actual AMD64 preloader:
**interpreter-free ET_DYN**. Both pass the reviewed structural checks. Their state
is `preloader_abi_unverified`, because candidate roles are not authenticated
production roles and kernel/invocation/runtime-view qualification is incomplete.
They receive no execution permission. Ordinary native-executable glibc rules are
not globally relaxed for interpreter-free files.

The updated [host ABI report](artifact-evidence/winehq-dualarch-host-abi.json)
inspects **81 actual native ELF objects** on Debian 13 x86-64, both architectures,
51 architecture-qualified dependency names and 98 GNU version requirements.
51 requirement groups have their symbols in an observed provider; that does not
establish which provider the real loader will select. The i386 interpreter
`/lib/ld-linux.so.2` is absent. Kernel IA32 execution was not tested. Managed module
candidates, missing providers, ambiguous/static resolution limits and dynamic
loading unknowns remain explicit. No ldd or inspected ELF was executed. These
results do not qualify Arch Linux, other distributions, or complete runtime ABI.

## Execution boundary and runtime-view strategy

The verified Wine 10.0 source review remains applicable: the launcher derives
paths from `/proc/self/exe` and compiled BINDIR/LIBDIR/DATADIR relationships;
initialization re-execs a sibling preloader; the preloader opens the main ELF by
pathname and transfers control through its interpreter; later loaders, modules
and wineserver use derived pathnames. Invocation basename and relative layout
matter. Environment overrides and compiled fallbacks remain part of the closure.

A safe view must preserve the runtime root, both launchers/preloaders, wineserver,
module/resource bytes, search roots, layout/configuration, prefix association and
environment throughout process/helper completion. Retained descriptors, an
immediate pre-spawn hash and owner-controlled 0555 permissions do not establish
this boundary. A read-only bind can still expose writes through a mutable backing
alias. Ordinary user namespaces alone do not fix that problem.

`runtime_view` implements real sealed memfd backing preparation and revalidation,
bound to artifact/inventory/profile hashes and candidate generation. F_SEAL_WRITE,
GROW, SHRINK and SEAL prevent writes/truncation through aliases. Tests exercise
actual kernel denial, changed scope/generation, failed preparation, descriptor
cleanup and cleanup error classification. The actual candidate was sealed and
revalidated, then closed; see [runtime-view evidence](artifact-evidence/winehq-dualarch-runtime-view.json).
This seals the archive backing bytes; it does **not** mount or freeze an unpacked
Wine tree. No fake immutable flag or pathname execution fallback exists.

Capability detection invokes only fixed distribution utilities in a child user
and mount namespace, with a cleared environment and bounded timeout. A private
read-only tmpfs mount is attempted there; the parent namespace and sentinel are
checked unchanged. No sudo, normal-host mount or delegated service is used.
This host permits sealed memfd but fails the namespace/mount probe:
`namespace_mount_unavailable`. Even on a host where the probe succeeds, the
implementation returns `trusted_image_mount_authority_unavailable`: no qualified
filesystem-image mount provider is installed or implemented here. No execution
capability is issued in either case.

A future image provider must mount verified, stable backing bytes with Wine's
required path semantics, bind host libraries/fallbacks, own helper lifecycle and
cleanup, and address its own privilege and namespace attack surface. The present
seals do not defend process memory against ptrace or root/kernel administration.
No protection against all same-UID/equal-privilege attacks is claimed.

Native fixtures demonstrate descriptor execution, procfs discovery, pathname
preloader selection, opening/re-executing the launcher and relative helper lookup.
They also demonstrate unprotected helper/symlink substitution and in-place
modification of an owner-controlled executable despite an open descriptor.
Interrupted native children are reaped. These are deliberately harmless Linux
contract observations, not authentic Wine qualification. The runtime-view API
fails closed before claiming a secured handoff.

## Trust and eligibility

| State | Result |
| --- | --- |
| Inspectable candidate | Complete three-package Wine payload, real byte inventory |
| Package integrity | Repository signature/index/package/control chain verified for supplied key |
| Transformation integrity | Actual repeatable archive, exact derived inventory and receipt |
| P024 manifest | No enrolled authority/signature; no fabricated manifest emitted |
| P025 installation | Structural dry run passes; installation authority remains blocked |
| P026 | Full disposable shadow inventory/audit passes; no installed baseline claimed |
| P027 | Not installed or pinned; existing exact revision/generation checks preserved |
| Execution view | Backing seals pass; mounted runtime view unavailable |
| Host ABI | Static multiarch evidence only; i386/system/loader closure unqualified |
| Authentic test | Ineligible; no executable plan or approval fabricated |
| Production initialization | Blocked |
| Game launch | Blocked; Endfield not executed |

The existing two-stage harness and P028 coordinator are preserved. Any future
eligible plan must bind the completed packages/profile, installed generation,
P027 revision, host report, runtime view, private XDG/prefix paths, initialization
invocation and helper observation policy. Separate approval must precede that
exact execution attempt.

## Reproduction and validation

The explicit acquisition script now obtains both signed architecture indices and
all three exact packages. `package-wine-candidate.py` combines inspected evidence
and produces the tarball without running package scripts. `--i386` extends the
existing profile generator while preserving its earlier two-package mode. Large
payloads remain in the disposable workspace, not Git.

Default CI uses committed metadata and native/synthetic fixtures only. It neither
downloads large Wine packages nor executes Wine. Actual artifact ABI, sealed
backing and P025 dry-run observations are explicitly selected ignored tests.
New offline tests cover version/architecture/index/signer mismatch, missing
companions, control inconsistencies, ownership, conflicting paths, deterministic
packaging, changed staging/transform/source evidence, malformed preloader shape,
architecture non-promotion, changed generation, unavailable views and cleanup.
Existing P024 cryptographic, P025/P026/P027, coordinator and P019/P021–P033
regressions remain required. Exactly two application binaries and C17 ABI
0x00010000 remain unchanged.

Genuine Rust 1.85 formatting and locked metadata pass. Local full workspace
check/Clippy/test/build commands were attempted and are blocked by absent GLib/GTK
pkg-config development files. Non-GTK regressions and affected-crate checks run
locally; final-head hosted CI supplies GTK/CLI/IPC and GCC/Clang qualification.
GCC C17 ABI and ASan/UBSan tests pass locally. No system packages were installed.

P034 is still partial: production manifest authority, a qualified mounted image
view, complete multiarch host/library resolution, installed/pinned lifecycle
binding and separately approved authentic Wine qualification remain outstanding.
P035 is not ready and has not started.

Local non-GTK workspace regressions: **815 passed, nine ignored**. The ignored
actual-artifact P025/P026 dry run, 81-object ABI inspection and sealed candidate
backing observation were selected separately and passed. Offline package-set
fixtures: six passed; DEB fixtures: six passed; native descriptor/chain fixtures:
four passed; source fixtures: four passed. Affected-crate all-target Clippy passes
with warnings denied. Final-head hosted results are reported on Draft PR #7.
