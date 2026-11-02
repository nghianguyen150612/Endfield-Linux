# P031 existing game location import

P031 registers one directory that the user explicitly selects. It writes only
Endfield-Unix application metadata. It does not install, copy, repair, update,
rename, delete, or execute game files, and it does not establish game
authenticity, launch readiness, or compatibility.

## State and evidence

P031 persists `registered` to mean that Endfield-Unix saved a user-selected
path and the observations made at approval time. Revalidation returns
`registered` when the current root identity and bounded counts match;
`changed`, `missing`, `blocked`, or `unsupported` describe the corresponding
observed condition. Identity confidence is a separate field:
`user_declared_unverified` when the user confirms that the selected directory
contains Endfield. This declaration is not independent evidence.
There is no authoritative Endfield directory layout, marker, manifest, build
field, or publisher signature in the repository evidence, so P031 does not
invent or infer any of them. Build and publisher authenticity remain unknown;
compatibility is not evaluated. The contract contains future
`structure_observed` and `identity_verified` state values, but P031 never emits
them because no authoritative game format or identity proof is available.

The Linux observer records the selected root's device, inode, and current
mount ID, the reported filesystem type, and counts of top-level directories,
regular files, links, and special entries. It examines at most 512 top-level
entries. It does not recurse, read file contents, hash files, inspect arbitrary
documents, or identify executable candidates. Root symlinks and special
entries block registration. A link in a path component also blocks selection.
All path components are opened descriptor-relatively with no-follow flags.

Revalidation opens only the exact saved path and compares its root identity and
top-level entry counts. A missing path is `missing`; a different root object
or changed entry-type counts are `changed`; permission or safe-observation
failures are `blocked`; and unsupported filesystem or inspection conditions
are `unsupported`. It never searches for a replacement. Mount IDs can change
across remounts, so P031 does not claim identity continuity after remounting.
Same-count filename changes, nested changes, and file-byte changes may not be
detected. Registration is not an integrity audit.

## Linux path and filesystem behavior

Selection requires an absolute UTF-8 path no longer than 4096 bytes, no more
than 256 components, and no component longer than 255 bytes. `..`, symlink
components, missing directories, non-directories, unreadable paths, empty
roots, and roots exceeding the 512-entry limit return typed failures. Spaces
and Unicode names are supported. No home or mounted-volume scan occurs.

P031 recognizes ext-family, XFS, Btrfs, F2FS, kernel NTFS3, generic FUSE/FUSEBLK,
exFAT, VFAT, CIFS, NFS, tmpfs, and overlay filesystem reports when device,
inode, and current mount identity are observable. NTFS and exFAT can therefore
be imported when the mounted filesystem supplies those observations. An
unknown filesystem type is unsupported. Current-mount comparisons do not
promise persistence across remounts. Unix ownership semantics are reported
only for ext-family, XFS, Btrfs, and F2FS; UID/GID values on other filesystems
may be mount-mapped or synthetic. A tmpfs game location is flagged as volatile.

Application metadata has separate storage constraints. The registry writer
requires private P006 application data storage on ext-family, XFS, Btrfs,
F2FS, tmpfs, or overlay. If the XDG application data directory is volatile,
the registration may not survive shutdown. The game directory is never used
for application metadata.

## Planning and approval

Planning is read-only. It does not create XDG directories, SQLite, lock files,
or files in the selected directory. The plan includes the proposed ID, exact
path, root identity, scope, filesystem observations, declaration confidence,
collisions, registry revision, expected metadata effects, warnings, and an
exact confirmation string. Apply requires the same path and declaration,
expected revision, and exact token. It reopens the path and recomputes the
plan before committing under the registry's exclusive lock.

`can_register` covers the selected-location checks. A plan cannot create or
lock XDG storage to prove that a later metadata write will succeed, so apply
also checks P006 directory permissions and registry-filesystem support and can
return a storage error without touching the game directory.

```sh
endfield-unix game list
endfield-unix game import plan --path "/mnt/Games/Arknights Endfield" --id endfield-main --declare-endfield
endfield-unix game import apply --path "/mnt/Games/Arknights Endfield" --id endfield-main --declare-endfield --expected-revision 0 --confirm 'TOKEN_FROM_PLAN'
endfield-unix game show --id endfield-main
endfield-unix game validate --id endfield-main
endfield-unix game remove-plan --id endfield-main
endfield-unix game remove --id endfield-main --expected-revision 1 --confirm 'TOKEN_FROM_REMOVE_PLAN'
```

Use the token printed by the current plan; do not reuse a token after a
revision or observed-root change. `game list` omits absolute paths. Plan, show,
validate, and remove-plan are explicit local inspection operations and display
the selected path. Their output is not automatically logged or included in
support reports.

## Registry and recovery

The sole P031 authority is the versioned JSON registry at
`$XDG_DATA_HOME/endfield-unix/game-installations/registry.json` (or the P006
default under `~/.local/share`). Schema version 1 has a monotonic revision and
up to 1024 records. Application directories are mode `0700`; registry and
lock files are owner-only mode `0600`. Registry input is bounded to 1 MiB,
strictly validated, and rejects unknown fields, duplicate IDs/locations,
corrupt JSON, invalid records, checksum mismatches, and future schema
versions. The SHA-256 checksum detects accidental valid-looking edits; it is
not a MAC and does not authenticate data against the same user. There is no
SQLite copy of this registry.

Mutation uses an advisory exclusive lock, revision compare-and-swap, a
same-directory private temporary file, `fsync`, and Linux atomic `renameat2`
publication. Failed or ambiguous publication never falls back to resetting
the registry. Interrupted temporary files are reconciled only during an
approved mutation while holding the lock; malformed or unexpected temporary
metadata blocks the operation with a recovery error.

If the registry is corrupt, uses a future schema, or reports recovery
required, stop mutations and preserve the complete `game-installations`
directory for inspection or backup. Do not delete or hand-edit the registry
or temporary files to make the command proceed. The game directory remains
untouched; the CLI does not provide a game-file deletion or repair command.

Unregister has a read-only plan, exact ID, current revision, and exact
confirmation. It removes only that registry record. P027 runtime pins, P028
prefix records, P029 recovery journals, P030 diagnostic contracts, daemon IPC,
and the C17 optimizer are independent and are not changed or associated by
P031. No runtime or prefix is selected or initialized.

## GUI status

The current GTK shell has no service integration point or worker-backed
directory chooser and preview flow. P031 is headless-first: the library and
canonical CLI are implemented, and the GUI does not offer game import. No GTK
main-thread filesystem work or duplicate GUI import engine was added. A later
GUI flow must call the same contracts through a background worker and Relm4
messages.
