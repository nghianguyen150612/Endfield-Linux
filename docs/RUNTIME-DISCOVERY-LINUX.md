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
attempt/completion state, an explicit outcome, and a safe issue category.
`NotPresent` is a successfully checked absence; `Inspected` and
`PartiallyInspected` record completed reads; `Failed` is an unsafe or failed
inspection; `Truncated` records a resource limit. Missing conventional
locations and missing PATH executable hints are ordinary observations, not
source failures. Permission errors,
wrong root types, malformed Steam library metadata, unsafe aliases, and limit
hits make the P021 report partial while preserving candidates from other roots.

System Wine checks `/usr/bin/wine`, `/usr/bin/wine64`, `/usr/local/bin/wine`,
and `/usr/local/bin/wine64`. PATH is only a hint: it is bounded to 16 KiB and
64 entries; empty, relative, and current-directory entries are ignored. The
scanner never uses a shell or runs `wine --version`. It checks whether a PATH
hint exists before spending the 32-root scan budget. Missing PATH hints do not
consume that budget. Existing roots are ordered by source policy: system Wine,
explicit roots, managed Steam/Heroic/Lutris locations, then PATH Wine. An
existing-root overflow is reported as `Truncated` and makes discovery partial;
it does not silently discard an explicit root in favor of PATH candidates.

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
Exit status is 0 for full or partial discovery, 2 for invalid options, 70 only
when every applicable inspected source fails, 74 for output errors, and 78 on
unsupported platforms. A successful absence observation, a completed empty
directory, or a truncated source is not itself a fatal source failure. A
zero-candidate complete report means only that the selected local locations
contained no recognized runtimes; it says nothing about game compatibility.
Partial CLI output prints only fixed source labels and safe issue codes, never
root paths or raw metadata. If the provider's applicable roots all fail
inspection, the provider returns the typed `ProviderUnavailable` error.

The hosted Linux smoke runs the actual executable and accepts an empty result
when no Wine or Proton is installed. An isolated temporary HOME/PATH fixture
independently verifies an empty-host result even if the test machine happens
to have Wine installed. A passing smoke validates safe command execution and
report formatting; it does not validate any runtime. Synthetic
temporary-directory contracts cover VDF parsing, metadata, symlink boundaries,
Wine structure, ownership, root absence/failure outcomes, and P021 report
construction.

## P022A hosted smoke repair

The first hosted P022 qualification, [run
40619500314](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40619500314),
failed in the real CLI smoke with `all accessible runtime discovery sources
failed`; its GCC and Clang C17 jobs passed. The CLI treated missing
conventional roots as failures because the fatal check required an available
root even when a location had been correctly checked and found absent. In
addition, absent PATH `wine`/`wine64` hints were included in the root list
before applying the 32-root limit. This made a normal PATH capable of
truncating optional hints and turning an otherwise empty machine into an
error.

P022A classifies absence, completed inspection, partial inspection, failure,
and resource truncation separately. It deduplicates same-source root aliases,
checks candidate paths before applying the 32 existing-root budget, gives
explicit roots priority over PATH candidates, and preserves a typed partial
result when a limit is reached. Only a report where all applicable root
inspections failed is fatal. The new isolated contracts cover empty HOME and
PATH, multiple PATH directories without Wine, missing/empty/inaccessible and
malformed explicit roots, mixed outcomes, the present-root budget, no false
candidates, conservative trust, and no runtime execution or fixture writes.

The first P022A hosted attempt, [run
40647230072](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40647230072),
passed the isolated empty-host test and the real CLI smoke, which printed:

```text
Runtime discovery: Linux

Discovered runtimes: 0
Discovery result: complete
```

That overall run later failed an existing CLI help assertion because its
expected subcommand list did not yet include `runtimes`; the assertion is
updated in the follow-up.

The corrected hosted qualification, [run
40648593964](https://github.com/nghianguyen150612/Endfield-Linux/actions/runs/40648593964),
passed all three jobs: Rust/GTK and both GCC and Clang C17 ABI tests. The real
CLI smoke again printed:

```text
Runtime discovery: Linux

Discovered runtimes: 0
Discovery result: complete
```

The isolated empty-host fixture and runtime contracts passed as well. This is
successful discovery behavior on a runner with no applicable installed
runtime; it is not runtime, origin, integrity, launch, or compatibility
validation. Source limits remain bounded to 64 PATH directories, 32 existing
scan roots, and the documented per-directory and metadata limits; exceeding a
bound remains visible as partial discovery.

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
