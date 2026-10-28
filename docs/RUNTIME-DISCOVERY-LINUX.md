# P022 Linux Wine and Proton discovery

P022 adds a library-only Linux adapter, `endfield-unix-runtime-linux`, over
the portable P021 `endfield-unix-runtime` provider contract. It performs
bounded, read-only inspection and does not initialize graphics, GTK, the
daemon, or storage. It makes no network requests and never executes Wine,
Proton, a launcher, or a manifest.

## Sources and roots

The adapter registers fixed source classes: system Wine executables, PATH Wine
executables, native Steam, Flatpak Steam, Heroic, Lutris, and explicit roots.
Each root attempt records its source, type, expected ownership, availability,
attempt/completion state, and a safe issue category. Absent conventional
locations are ordinary empty results. Permission errors,
wrong root types, malformed Steam library metadata, unsafe aliases, and limit
hits make the P021 report partial while preserving candidates from other roots.

System Wine checks `/usr/bin/wine`, `/usr/bin/wine64`, `/usr/local/bin/wine`,
and `/usr/local/bin/wine64`. PATH is only a hint: it is bounded to 16 KiB and
64 entries; empty, relative, and current-directory entries are ignored. The
scanner never uses a shell or runs `wine --version`.

Native Steam checks the conventional `~/.steam/root`, `~/.steam/steam`, and
`~/.local/share/Steam` compatibility-tools and `steamapps/common` locations.
It reads a bounded `steamapps/libraryfolders.vdf` subset where present: only
balanced UTF-8 quoted-string VDF with absolute paths and no parent traversal
is accepted. `steamapps/common` is inspected one level deep with the same
structural checks: a Proton launcher and supported local metadata are both
required, so unrelated game directories are not reported as runtimes. The
directory is not recursively searched. Proton labels or Proton-specific
launcher/manifest files select folders for inspection; an unrelated game's
`version` file alone is not a Proton hint.
Flatpak Steam checks
`~/.var/app/com.valvesoftware.Steam/data/Steam/compatibilitytools.d` and its
Steam-managed `steamapps/common`; a host-visible path does not prove Flatpak
sandbox access.

Heroic checks native `~/.config/heroic/tools/{wine,proton}`, a validated
`XDG_CONFIG_HOME`, and supported Flatpak config/data layouts. Lutris checks
`~/.local/share/lutris/runners/wine` and an absolute
`XDG_DATA_HOME/lutris/runners/wine`. Steam's `XDG_DATA_HOME/Steam` layout is
also considered when the override is valid. A conventional Flatpak Lutris
runner path is checked when present. The adapter reads no
launcher credentials, game configuration, scripts, or account files.

`--root /absolute/path` adds up to eight roots for that invocation only. Roots
must be absolute, bounded, and free of parent components. A root may itself be
a manager directory containing candidate directories or a runtime directory.
Custom roots are not persisted and are not treated as trusted origins.

## Candidate rules and identity

Wine executable candidates must resolve to regular executable files. Manager
Wine candidates need a recognized `bin/wine`, `bin/wine64`, `wine`, or
`wine64` executable in the candidate directory. No version is inferred from
an executable name. A bounded local `version` file may supply the version and
explicit Wine Staging marker; a directory name alone never establishes that
lineage. Invalid version metadata is reported and does not become a version.

Proton candidates need a regular executable `proton` launcher plus at least
one supported local metadata file (`compatibilitytool.vdf`, `toolmanifest.vdf`,
or `version`). The launcher is never executed, and its presence does not prove
that it can launch. VDF is treated as data: UTF-8 only, quoted strings and
braces only, balanced scopes, bounded nesting/tokens, and recognized Proton
manifest roots. Version metadata is read independently; malformed or
unsupported metadata is reported and not used as identity evidence. GE-Proton,
DWProton, and Valve Proton are reported as claims from local metadata or, as a
weaker hint, a directory label. The CLI names which evidence supplied the
claim. Conflicting local lineage or version claims are reported as partial
discovery and the conflicting field remains unknown. A name is not proof of
release authenticity.

Runtime architecture is preserved only when a supported manifest explicitly
states a recognized architecture value. Wine binary names, Proton packaging,
and host architecture do not fill missing runtime architecture metadata.

The P021 `RuntimeCandidate` holds family, opaque local ID, provider ID,
ownership, and an optional version hint. The Linux report separately carries
lineage, the evidence basis, local structure classification, sanitized display
label, fixed source references, and safe issue categories. Candidate IDs are
deterministic two-part FNV-1a hashes of resolved local paths and are local
lookup labels, not cryptographic identities; moving a runtime changes its ID.
Private paths
are kept only in provider memory for same-session identity inspection, omitted
from `Debug` output, and never printed or persisted. P021 `RuntimeIdentity`
requires a version. If the version is unknown, the candidate remains with no
version hint and identity inspection returns the typed `MissingMetadata`
provider error instead of inventing a version value.

Steam runtimes are classified externally user-managed; Wine under `/usr/bin`
or `/usr/lib` is distribution-managed; Heroic and Lutris runtimes retain their
other-launcher owner; explicit roots remain unknown. If aliases from sources
with differing ownership resolve to the same candidate, ownership is
conservative. Same-path aliases are merged while retaining source references;
different paths are distinct even when their labels or versions match.

## Filesystem bounds and trust

Discovery caps roots at 32, explicit roots at 8, PATH bytes at 16 KiB and
entries at 64, entries per directory at 512, metadata files at 256 KiB, VDF
nesting at 15 levels and tokens at 8,192, path length at 4,096 bytes,
symlink resolution at 8 hops, candidate count at the P021 limit of 512, and
reported safe issues at 128. Intermediate issue accumulation is bounded by
the root and directory-entry limits. PATH candidates are checked separately
under the 64-entry bound. This implementation inspects only a root and
its immediate child directories; it does not recursively walk arbitrary
trees.

Validated system and manager aliases may resolve through bounded symlink
chains. Explicit-root child aliases must remain under that root. Special files
are rejected for metadata and launchers. Metadata reads use a nonblocking,
no-follow regular-file handle, size bound, and before/after file identity
checks; filesystem changes can still produce partial discovery because there
is no installation lock.
Missing files are skipped; a denied or malformed source is not presented as
an empty successful source.

Local structure and metadata are untrusted. P022 records no source
authenticity, complete artifact integrity, compatibility, or launch-readiness
proof. Familiar Steam, Heroic, Lutris, GE-Proton, or DWProton names do not
change this. No inventory is written to SQLite or TOML, added to P008 support
reports, or uploaded.

## CLI and validation

`endfield-unix diagnose runtimes` is headless and has optional `--family wine`,
`--family proton`, and repeatable `--root /absolute/path` filters. Output omits
absolute paths and labels lineage evidence, source authenticity, artifact
integrity, compatibility, and launch readiness as unverified/not evaluated.
Exit status is 0 for full or partial discovery, 2 for invalid options, 70 when
all accessible sources fail, 74 for output errors, and 78 on unsupported
platforms.

The hosted Linux smoke runs the actual executable and accepts an empty result
when no Wine or Proton is installed. A passing smoke validates safe command
execution and report formatting; it does not validate any runtime. Synthetic
temporary-directory contracts cover VDF parsing, metadata, symlink boundaries,
Wine structure, ownership, and P021 report construction.

`Arch desktop runtime discovery: UNVERIFIED`

`Dell Vostro runtime discovery: UNVERIFIED`

`Actual Endfield runtime compatibility: UNVERIFIED`

Manual read-only comparison on an available Arch target:

```sh
cargo +1.85.0 run --locked -p endfield-unix -- diagnose runtimes
command -v wine
command -v wine64
```

For the desktop's reference GE-Proton 10-26 installation, inspect its real
location and local metadata manually when the device is available. Do not
replace it based on discovery output. P022 output alone is not a game
compatibility result. The Dell Vostro runner inventory and renderer remain
separate questions; no game is launched during validation.

The P021 boundary is ready for a future P023 adapter to consume discovered
candidate and identity facts. P022 does not add UMU support or launch plans.
