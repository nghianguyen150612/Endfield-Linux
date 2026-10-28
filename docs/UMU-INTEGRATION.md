# P023 local UMU integration and launch-plan preparation

P023 adds a Linux-only, library-based UMU adapter to
`endfield-unix-runtime-linux`. It discovers local `umu-run` candidates,
prepares typed UMU environment changes, rechecks a selected P022 Proton
identity, and can construct a P021 `LaunchPlan`. It never invokes UMU, Wine,
Proton, a game, a package manager, or a network client. It does not create or
change a prefix. A plan is data and grants no launch authorization.

## Upstream contract revision

The contract was reviewed against Open-Wine-Components/umu-launcher commit
[`e2b203a1fdd2af9f35166f5713cb3f85d72587e2`](https://github.com/Open-Wine-Components/umu-launcher/commit/e2b203a1fdd2af9f35166f5713cb3f85d72587e2), including `README.md`,
`docs/umu.1.scd`, `docs/umu.5.scd`, `umu/umu_run.py`, and
`umu/umu_consts.py`. The current upstream FAQ wiki snapshot reviewed was
[`48253424c500ea1c99fdb3ad463edd7a2a7a7d3e`](https://github.com/Open-Wine-Components/umu-launcher/wiki/Frequently-asked-questions-(FAQ)).
The man page documents `GAMEID`, `PROTONPATH`,
`WINEPREFIX`, `STORE`, `PROTON_VERB`, and `UMU_RUNTIME_UPDATE`. The README
explains `GAMEID` and `STORE` protonfix database selection; man page 5 lists
`egs`, `gog`, `battlenet`, `amazon`, `humble`, `itchio`, and `ubisoft` store
values. `PROTON_VERB` must be an upstream Proton verb; `waitforexitandrun` is
the documented default. The upstream man page's example uses
`PROTONFIXES_DISABLE=1` to suppress fixes. `UMU_RUNTIME_UPDATE=0` disables Steam Linux Runtime
automatic updates. This is not an offline or read-only guarantee.

The docs permit missing `PROTONPATH`, which selects UMU-Proton by default, and
also document `GE-Proton` as an automatic download token. P023 always requires
an explicit, absolute, inspected Proton directory. Missing `WINEPREFIX` and
`GAMEID` select defaults that UMU may create or initialize, so P023 requires an
explicit existing prefix directory and writes the neutral upstream ID
`umu-default` when no verified game ID is selected. Unset `STORE` is retained
as a neutral policy. No Endfield-specific ID or storefront is inferred.

## Local discovery and identity

Discovery checks `/usr/bin/umu-run`, `/usr/local/bin/umu-run`,
`$HOME/.local/bin/umu-run`, and up to 128 validated absolute PATH directories.
It accepts an optional caller-selected absolute executable path. Relative and
empty PATH entries are ignored; the current directory is never searched.
Candidates are deduplicated, path lengths and candidate counts are bounded,
and symlink traversal is limited to eight hops. Symlink aliases resolving to a
regular target are accepted. A cycle, non-regular target, non-executable
file, special file, overlong path, or changed file identity is rejected.
The report uses P022A's `NotPresent`, `Inspected`, `PartiallyInspected`,
`Failed`, and `Truncated` outcomes. A PATH or candidate limit hit is reported
as truncated, not as proof that UMU is absent. Incomplete inspection keeps
provider availability unknown unless a structurally valid candidate was
actually found.

Structural inspection is limited to filesystem type, execute bits, resolved
path, and stable local file identity. It does not read or execute the program
contents to prove that the candidate is an authentic UMU distribution. UMU's
version is therefore `unknown`; no stable machine-readable version interface
was assumed and no `--version`/`--help` call is made. Package database version
integration is not implemented. A structurally valid executable is not
`VerifiedUsable`, and actual UMU execution readiness remains unverified.
Filesystem identity is a point-in-time observation; P034 must revalidate the
UMU executable and all runtime references immediately before any future launch.

P022 candidates remain the authority for Proton structure and provenance
classification. Selection is explicit by P022 runtime ID. The selected family
must be Proton and P022's identity inspection is repeated to detect changed
metadata. No enumeration index, marketing version, latest label, first result,
or system Wine fallback is used. Proton source authenticity, artifact
integrity, and compatibility evidence remain exactly as P022 observed them.

## Environment and prefix policy

The typed environment uses P021 `EnvironmentName`, `EnvironmentChange`, and
`ValueSensitivity`. It sets exact absolute `PROTONPATH` and `WINEPREFIX`,
`GAMEID=umu-default` unless a bounded control-free explicit identifier is
provided, `PROTON_VERB=waitforexitandrun`, `PROTONFIXES_DISABLE=1`, and
`UMU_RUNTIME_UPDATE=0`. `STORE`
is either one of the upstream documented values or explicitly removed from
the inherited environment. Known UMU behavior-control variables are removed;
no arbitrary inherited override is accepted. Environment keys are unique and
deterministically sorted. Prefix and runtime paths are marked sensitive and
redacted by P021 dry-run JSON. Game arguments are retained in their original
native order but marked sensitive in dry-run output.
The UMU plan summary identifies `linux-umu-adapter` separately from the
selected Proton provider and includes the unknown UMU version, Proton source
state, compatibility state, and `executable_now=false` without exposing paths
or arguments.

Prefix selection requires an absolute, bounded, traversal-free path with an
explicit non-unknown ownership class. Preparation checks that it already
exists as a directory. P023 never creates, initializes, migrates, repairs,
backs up, or deletes it. A path does not prove prefix compatibility with a
Proton build; P028 and P030 own fuller management and diagnostics.

## Side effects and readiness boundary

The future execution of UMU can acquire Steam Linux Runtime components,
download Proton in automatic configurations, update runtime caches, initialize
compatibility layers, create or mutate a Wine prefix, and run protonfixes that
change user-owned files. `UMU_RUNTIME_UPDATE=0` only disables the documented
automatic SLR update path. Explicit `PROTONPATH` avoids UMU's default or
automatic Proton selection, but does not prove UMU will make no network request
or filesystem change. P023 performs none of these operations.

The reviewed upstream FAQ explains that Proton builds may need to be compiled
for the Steam Linux Runtime and that UMU only searches a limited location for
its automatic-latest modes. P023 avoids those modes. Structural checks cannot
establish that a selected Proton build is suitable for pressure-vessel/SLR;
compatibility remains unassessed. The environment plan removes inherited
`STEAM_COMPAT_LIBRARY_PATHS`, `PRESSURE_VESSEL_FILESYSTEMS_RO/RW`, and
`SteamGameId`, so it does not silently request extra container mounts or
invent a Steam app ID.

P021 plan validation describes structural compatibility of the adapter and
runtime identity. A P023 plan records unverified source state and
`not-evaluated` compatibility; readiness remains blocked by missing integrity,
compatibility, or authorization evidence. An incomplete Proton identity is a
typed incomplete preflight result. Missing UMU, Proton selection, prefix, or
game file is also reported as a missing prerequisite. A successful plan is
still `executable_now: false` and does not authorize execution. P034 owns
execution; it must revalidate TOCTOU-sensitive references.

The UMU provider advertises implemented local plan preparation when a
structurally valid candidate exists. It does not advertise verified usability,
source or artifact verification, compatibility assessment, downloads, or
process execution. Capability negotiation continues to require the P021
verification, compatibility, and approval evidence.

## CLI and validation

`endfield-unix diagnose umu` reports local UMU status and P022 Proton candidate
count without printing paths. It does not use GTK, the daemon, storage, network,
UMU, Wine, Proton, or a game. Absence is a successful truthful partial report.
Use `command -v umu-run` only as an optional comparison; do not invoke it during
P023 qualification.

Synthetic tests cover absent and user-local discovery, PATH hints and
symlinks, invalid file types, changed identity, bounded environment policy,
literal ordered arguments, redaction, P022 Proton selection, and an incomplete
preflight. The live Linux smoke is designed to pass when UMU is absent. Hosted
qualification and CI links will be recorded in `docs/TESTING.md` after the
workflow runs. No real device has been inspected as part of this change:

`Arch desktop UMU integration: UNVERIFIED`

`Dell Vostro UMU integration: UNVERIFIED`

`Real Endfield UMU launch: UNVERIFIED`

`Real Endfield compatibility: UNVERIFIED`

Manual read-only checks on Arch:

```sh
cargo +1.85.0 run --locked -p endfield-unix -- diagnose umu
cargo +1.85.0 run --locked -p endfield-unix -- diagnose runtimes
```

On the Arch RTX 5070 desktop, discover UMU and Proton independently and check
the existing GE-Proton 10-26 candidate if present. Any plan preview must use
explicit selected paths and must not replace that build. On the Dell Vostro
3510, repeat discovery independently; do not infer renderer selection, force
NVIDIA activation, start Proton, or run a benchmark just to check availability.

P024 can add authenticated source manifests and stronger runtime provenance.
P023 does not download, install,
pin, audit, launch, or assert game compatibility for a runtime.
