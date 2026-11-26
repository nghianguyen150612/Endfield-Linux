# P034 Wine runtime image provider

## Selected strategy

P034 selected **SquashFS built with `mksquashfs` 4.6.1 and mounted through `squashfuse` 0.5.2**. EROFS is enabled in the observed kernel, but the host has no loop-control device, and no unprivileged EROFS file mount was demonstrated. Squashfuse uses `/dev/fuse` and the host's existing `fusermount3`; it does not need a loop device. The implementation and tests run only in a child user and mount namespace with propagation made private. No sudo, system package installation, new setuid helper, or privileged daemon was used. The existing host `fusermount3` is root-owned and setuid; its availability and policy are explicit host prerequisites.

Observed host: Debian 13 x86-64, kernel `6.18.57`, `CONFIG_USER_NS=y`, `CONFIG_EROFS_FS=y`, `/dev/fuse` present, `CONFIG_FUSE_FS`/mount behavior demonstrated through a real FUSE mount, `/dev/loop-control` absent. A private tmpfs mount probe also succeeded, but it is not evidence for a filesystem image provider. The SquashFS utilities and libraries were unpacked under disposable workspace state and were not installed system-wide.

## Deterministic image construction

`scripts/build-wine-squashfs-image.py` accepts only the existing exact candidate plus its P025-installed runtime tree, P026 baseline, P027 pin, profile, package receipt and transformed inventory. It validates those identities, compares every installed entry with the transformed inventory, stages a sanitized runtime-only tree, preserves exact file bytes/modes and safe relative symlinks, and rejects special files, set-id files, path escapes and changed entries. It excludes P025/P026 management sidecars from the image.

The image command is:

```text
mksquashfs ROOT IMAGE -noappend -reproducible -all-root -no-xattrs -no-exports -no-progress -no-duplicates -mkfs-time 0 -all-time 0 -processors 1 -comp zstd -Xcompression-level 15
```

The exact candidate was built twice from the same installed generation. Both outputs were byte-identical:

- Candidate SHA-256: `391fe86b400d2cf7a8a58c72866c7a8752d4cc613f5536003a7f45f40f7851ec` (437,830,035 bytes)
- Image SHA-256: `e3b4a6d61313f1f93ebbbf933a1078771a5ba1062af24d30c42aeba1d28ff15b` (387,833,856 bytes)
- Logical inventory: 2,849 entries, `74bdf3b1215aace973ab7cc2292266194c072c99977836c09e989c7ff3247b24`
- Artifact profile SHA-256: `c2d687c1885f5ca343a362f955c221e6589b8a1fb2f44655b4064032fba42947`
- Candidate generation identity: `4b8cba966f1c4bbf51cd4c12c67d458d924ffafc662a08840db3ea9568639ba3`
- Image builder SHA-256: `ea9ac9b6697186082d68b88cb20a747bc981bfbb6cf383233eb22532bf53d0ec`

Full builder configuration and the complete logical inventory are in [image evidence](artifact-evidence/winehq-dualarch-squashfs-image.json). The original tarball was not changed.

The scoped manifest was `winehq-noble-dualarch-dev-20261125`, key ID
`dev-p034-ephemeral-20261125`, manifest version 1, with exact manifest SHA-256
`28a9ccc9ba07c46e64ae7aa1e0058fb43f7eafa80c2f2625ca1b4b6feec856d5`. P024
verified it as `DevelopmentOnly`; presenting the same key as an independently
authenticated publisher was rejected. The installation and pin identities were:

- P025 installation ID: `wine-02b53c1f12f48c147799cf30a04ad10e6e22341bed8a10ee25e8535092bdabb3-1`, generation 1
- P025 ownership record SHA-256: `689e2269e3916413652192e86b34b29e584feb3b479add754cd3194504bdb808`
- P026 baseline SHA-256: `79092c5e2eb728d4dcbd6437dace19dbf7c557fdb8f449a413ca3bb0bbdf4940`
- P027 pin ID: `pin-1873fb0eb33d90d28a415bb7009de791f58d58c93b9a337b4b5320164c4771a4`, registry revision 1

The public manifest receipt records the exact manifest, signature, public key,
candidate, profile, source-package and inventory hashes. It contains no private
key bytes.

## Mounted view evidence

`scripts/inspect-wine-squashfs-image.py` verifies the image digest, copies it to a sealed memfd, then invokes squashfuse in a private user/mount namespace. It checked the actual Wine image's mounted inventory against all 2,849 P026 entries and the transformed inventory digest. Mount attributes were `ro,nosuid,nodev,relatime`. Attempts to append, replace, and symlink-replace runtime paths failed. The sealed backing rejected an in-place write. The parent mount namespace was unchanged, the FUSE mount was unmounted before the namespace owner exited, and `wine_spawned` is false. See [mounted-view evidence](artifact-evidence/winehq-dualarch-squashfs-mounted-view.json).

A separate harmless native fixture test mounted a small SquashFS image through the same provider and exercised descriptor-bound launcher invocation, `/proc/self/exe`, sibling preloader selection, preloader pathname open of the main executable, relative module lookup, helper exec, process completion, and teardown. Backing mutation and runtime write/replacement/symlink attempts were rejected. See [fixture evidence](artifact-evidence/winehq-dualarch-squashfs-native-fixture.json). This qualifies the host FUSE pathname behavior; it does not execute Wine.

## Application boundary and threat model

`PreparedRuntimeImage` in `prefix-management::runtime_view` now verifies the image digest and binds it to candidate digest, image format/provider identity, transformed inventory digest, profile digest, and candidate generation. It represents verified immutable backing bytes only. It intentionally returns `TrustedImageMountAuthorityUnavailable` for execution: the mount-and-inspect workflow is not yet wired into an application-owned process capability. CLI/GTK read-only qualification diagnostics now report manifest authority scope, recorded signature provenance, the selected provider, image-digest availability, mounted-view eligibility, and separate i386 ABI state in JSON schema v2; diagnostics do not mount images or create state.

This distinction is required because the current executor owns only its direct child. It does not establish completion/termination ownership for wineserver and every helper, nor bind the mounted namespace's lifetime to that descendant lifecycle. Authentic qualification and production initialization therefore remain blocked. The view protects image-owned Wine files while mounted; host shared libraries stay outside the image and remain trusted under the host distribution policy. The work does not claim protection against malicious root, kernel compromise, unrestricted ptrace, equal-UID process-memory attacks, or changes to host libraries during execution. Wine was not spawned.

## Acceptance state

| State | Result |
| --- | --- |
| Development manifest | Verified through P024 in disposable development scope |
| Production manifest authority | Unavailable; operator enrollment is not done |
| Image construction | Built from the real P025 installation |
| Image reproducibility | Two builds byte-identical |
| Squashfuse provider operation | Implemented in bounded host tooling and mounted successfully |
| Native fixture handoff | Qualified in private namespace |
| Actual Wine image mounted inventory | Qualified; no Wine process started |
| Application execution view/capability | Blocked pending process and descendant lifecycle ownership integration |
| Host ABI | Not qualified; `/lib/ld-linux.so.2` absent, no i386 shared libraries in ldconfig, and a static ELF32 fixture receives `ENOEXEC` |
| Authentic qualification / Wine | Not ready / not run |
| Production initialization / game launch | Blocked / disabled |

The development manifest, installed runtime, baseline, and pin remain classified `development_only`; they cannot authorize production initialization or game launch. PR #7 stays Draft and unmerged. P035 has not begun.
