# Official game sources (P032)

P032 provides an offline reviewed catalog, manual official installation handoff,
and bounded read-only Steam metadata observation. It does not acquire artifacts,
install launchers, update games, authenticate accounts, or launch anything.
Source availability, installed identity, artifact integrity, publisher
authentication, game build, runtime compatibility, and execution readiness are
separate. `game_compatibility` remains `not_tested`.

## Reviewed evidence

Primary distribution pages were observed on **2026-11-02 UTC**. Dates below are
source-displayed publication dates; an unknown publication date is not filled
from a game release date, copyright year, crawler date, or observation date.
Catalog revision `2026-11-02.1`, schema `1`, preserves these historical claims.

| Reference / provider | Exact document URL | Publication date observed | Supported claim |
| --- | --- | --- | --- |
| `publisher-home` / GRYPHLINE | <https://endfield.gryphline.com/> | Not displayed | Publisher game landing page; redirected to `/en-us` during review. It is not an artifact URL. |
| `gryphlink-update` / GRYPHLINE | <https://endfield.gryphline.com/en-us/news/0744> | 2026-10-13 12:00; timezone not displayed | “Sanctuary of Ink” pre-download/update notice directs PC users through GRYPHLINK. This establishes publisher-operated update guidance, not a public API. |
| `steam-announcement` / GRYPHLINE | <https://endfield.gryphline.com/vi-vn/news/3837> | 14-10-2026 20:00; timezone not displayed | Steam announcement links app **4732690**, names Asia and Americas / Europe servers, and Windows 10/11 64-bit requirements. Account-progress statements do not establish interchangeable files or updater state. |
| `steam-listing` / Steam | <https://store.steampowered.com/app/4732690/> | Not displayed | Official app listing; Hypergryph developer, GRYPHLINE publisher, Windows requirements, kernel-level ACE anti-cheat. Displayed release date Oct 14, 2026 is not the page publication date. |
| `epic-listing` / Epic Games Store | <https://store.epicgames.com/p/arknightsendfield-f0f167> | Not displayed | Official Epic listing; Hypergryph developer, GRYPHLINE publisher, Windows 64-bit requirements and Epic ID requirement. Displayed release date 01/22/26 is not publication evidence. |

The task supplied an external Steam Deck **Unsupported** observation. The text
extract available during revalidation did not expose the Deck badge, so P032
preserves this as supplied external platform information, not an independently
revalidated badge or an Endfield-Unix test result. Kernel-level ACE information
was visible on the official Steam page. Neither indicates that this application
can resolve anti-cheat or Linux compatibility.

No Chinese publishing channel, Chinese build/account equivalence, CDN allowlist,
latest-version API, game manifest endpoint, token service, Epic internal catalog
ID, or artifact digest authority was established. Global records do not authorize
region conversion. Language selection is not build identity.

## Catalog and capability policy

`crates/game-sources` is a library, not another application executable. Portable
Serde types describe channels, exact evidence claims, explicit verification
states, capabilities, supported platforms, restrictions, unsupported operations,
and manual guidance. A cfg-gated Linux module supplies local observation; no
GTK, network client, process, IPC, runtime, or storage API is used by this crate.

The three built-in records are ordered `epic`, `gryphlink`, `steam`. Stable IDs
are exact and case-sensitive. Only reviewed built-in data is accepted by catalog
validation, including its evidence and capability policy; it is not a mechanism
to enroll arbitrary URLs. Changes require code review and a catalog revision.
All three records support catalog and official handoff. Only Steam supports local
observation. Epic and GRYPHLINK local metadata inspection is unsupported.

- `catalog_only`: reviewed page/evidence is available offline; no download protocol implied.
- `official_handoff`: display official URLs and manual guidance; no browser, URI handler, launcher, or HTTP request is started.
- `local_installation_observation`: inspect explicitly selected, bounded local metadata; neither authenticate nor change its payload.
- `verified_artifact_acquisition`: reserved, disabled for every P032 source. A URL returning bytes would not enable it.

`verified_official_listing`, `verified_publisher_announcement`,
`observed_local_metadata`, `user_declared`, `unverifiable`, `unsupported`,
`changed`, and `unknown` are separate states, not an ordinal trust score.
Official listing evidence concerns the page, not installed files.
`check-catalog` validates known built-in identity/evidence offline. Live network
availability stays `unknown`; no failed request can erase historical evidence,
and no missing source triggers a mirror or channel substitution. P032 has no
network option or supported direct download/update API.

## Steam observation format and limits

The app identity comes from the publisher announcement and official store.
The **local format**, separately, was checked against the implementation author's
published [steamlocate 2.1.1 app source](https://docs.rs/steamlocate/2.1.1/src/steamlocate/app.rs.html)
and [library source](https://docs.rs/steamlocate/2.1.1/src/steamlocate/library.rs.html),
observed 2026-11-03; docs.rs lists release publication 2026-08-13. These are primary
implementation references for a community format reader, **not Valve approval,
a supported download API, or game authenticity evidence**. They demonstrate the
`AppState`/`appid`/`installdir` fields and library `steamapps/common` resolution.
Valve Developer Wiki KeyValues access returned 403 during this review; it was
not used as fetched evidence. No community page establishes first-party trust.
No third-party code or additional parser dependency was copied or installed.

P022's existing VDF helper is private, permits a larger bound, and flattens
pairs for runtime hints. P032 needs exact root-level app identity and duplicate
rejection, so it uses a narrow independent tree parser with the same quoted
string/braces approach rather than weakening or exporting that helper.

Select either one **library root** (the directory containing `steamapps`) or
its exact `steamapps/appmanifest_4732690.acf`. Selecting `steamapps` itself is not
a library-root selection. No `libraryfolders.vdf`, mounted libraries, account
databases, authentication tokens, Proton settings, executables, depot manifests,
or game payloads are read. Unknown nested metadata is syntax-checked but not
used as identity or emitted. Display name never establishes identity.

Accepted subset: UTF-8 quoted strings and brace objects, one exact `AppState`
root; case-insensitive field keys; duplicate keys (including case variants),
truncated/unbalanced trees, additional roots, includes, conditionals, comments,
and unquoted variants are rejected. Only escaped quote/backslash are supported.
Limits: 64 KiB manifest, 4096 string tokens, 4096 bytes per string, depth 0–8,
4096-byte selected absolute UTF-8 path, 256 components, 255 bytes per component.
App ID must be the exact string `4732690` at root level. `installdir` must be a
single nonempty literal child name without separators, colon, dot traversal or
control characters. Folder names are not hardcoded.

Every path component uses descriptor-relative no-follow opens. `O_PATH`
validates object type before opening anything for I/O; only a held regular
single-link manifest inode is reopened through the kernel's `/proc/self/fd/N`
link. No user-provided symlink is followed. Systems without usable procfs fail
closed. Manifest and installation objects are reopened for replacement checks.
Symlinks, hardlinked manifests, devices and special metadata files fail closed.
No directory enumeration occurs. Missing installation directories are preserved
as partial observations; missing manifests are typed errors. Build/state numeric
strings are reported as **untrusted launcher claims**; game build and installation
completeness stay unknown even if StateFlags says 4. No downloaded-bytes totals
or names prove a complete installation.

`revalidate` takes the exact prior selection and 64-character observation token.
The token binds normalized manifest location, bounded bytes, device/inode,
manifest timestamps/size, and installation-directory device/inode or absence.
A different location, metadata edit, replacement, or appearance/disappearance
changes the token. Missing or unsafe evidence is a typed failure; changed valid
evidence retains the current observation with `status: changed` and exit 4.
Token comparison neither rewrites a record nor searches for another location.
It is an unkeyed local change detector, not trusted digest authority. File-byte,
same-inode game-content changes, mount continuity, malicious same-user races,
and full snapshot isolation are outside its guarantee. No game files are hashed.

Steam alone owns installation and updates of its edition. GRYPHLINK state,
patches, or updater guidance must not be applied to a Steam installation.

## Explicit P031 integration and persistence

P031 PR [#4](https://github.com/nghianguyen150612/Endfield-Linux/pull/4) merged
2026-11-02 at `846d9c867bc49946b2dffe15f2d4632cd6b412df`. P030 PR
[#3](https://github.com/nghianguyen150612/Endfield-Linux/pull/3) merged at
`6f383c6390462cd4da64a97d372fee739b20fd2c`. GitHub metadata and Git ancestry
were checked before editing. Remote main matched the supplied baseline.

P031's strict schema-v1 registry has no source field. P032 deliberately keeps
observations separate and ephemeral: there is **no migration, association writer,
sidecar registry, apply token, or cross-registry atomicity claim**. Adding a
second authority for the same installation would need a separately specified
transaction/recovery contract. Existing v1 registrations remain unchanged,
including corrupt/future-schema fail-closed handling and original import/remove
revision CAS. Nothing silently resets or rewrites records.

`plan-association` reads an exact game ID and source ID, reports the current
registry revision, revalidates the registered location, and optionally inspects
one explicit Steam path. A matching observed installation path and device/inode
plus successful P031 root revalidation yields `observed_local_metadata`; an absent observation,
mismatch, or stale location leaves the proposed label `user_declared`. The
report exposes the match boolean, actual observation and P031 validation.
No metadata label promotes P031's identity confidence. A revision is an
observation, not a reserved CAS revision; concurrent changes are possible.
`source validate` reports **no persisted source association** and revalidates
only the P031 location. To recheck a Steam observation, use `source revalidate`
with the prior path/token. No source, runtime or prefix is selected automatically.

## Actual CLI

```sh
endfield-unix game sources
endfield-unix game sources --json
endfield-unix game source show --id steam
endfield-unix game source show --id epic --json
endfield-unix game source show --id gryphlink
endfield-unix game source check-catalog --json
endfield-unix game source inspect --id steam --path '/selected/SteamLibrary' --json
endfield-unix game source revalidate --id steam --path '/selected/SteamLibrary' --expected-observation TOKEN_FROM_INSPECT --json
endfield-unix game source plan-association --game-id endfield-main --source-id steam --json
endfield-unix game source plan-association --game-id endfield-main --source-id steam --path '/selected/SteamLibrary' --json
endfield-unix game source validate --game-id endfield-main --json
```

After reviewing an observation, explicitly select the reported installation path
for `game import plan --path PATH --id ID --declare-endfield`. Continue P031's
existing exact-token apply only if desired. P032 does not register automatically.
Manual handoff leaves the user responsible for using the provider's supported
platform and client; the source page is never presented as Linux support.

All source commands run before GTK initialization and without daemon, network,
child processes, or XDG creation. JSON uses schema 1; human reports display a
short notice plus escaped, formatted report fields. Both are bounded to 64 KiB.
Explicit inspection and association output includes selected local paths; it is
not logged or added to support reports. Raw manifest/account fields are omitted.
Failures use schema-1 `error.code` and `error.message`. Exit statuses: 0 valid
report (possibly partial); 2 invalid path/source/app identity or CLI syntax;
3 missing evidence; 4 changed evidence or stale P031 location; 70 catalog or
P031 storage/schema failure; 74 I/O; 78 unsupported operation/metadata variant.
P031 registry errors retain their existing codes/status mapping. Output errors
use the canonical dispatch's I/O status. Nested `--help` exposes only operations
implemented here; there is no source download/install/apply command.

## Qualification and next boundaries

Deterministic fixtures cover catalog identity/order/evidence/capability policy,
unsafe Steam metadata, partial/missing state, unknown build, wrong identities,
explicit scope, local change detection, and absence of authenticity promotion.
The real executable suite denies socket/connect and child creation using Linux
seccomp, removes display/session variables and executable search paths, checks
storage-free reads, actual nested help, P031 schema-v1 registration, label versus
observation previews, mismatched locations, unchanged registry/game bytes, stale
CAS rejection, removal and corrupt/future registry preservation. Existing P027–P031
regressions remain required. Final outcomes are recorded in the
[P032 manifest](manifests/PROMPT032_MANIFEST.md).

No real Endfield/Steam installation, physical NTFS/exFAT device, RTX 5070 or Dell
Vostro 3510 was used. Physical-device and gameplay qualification remain
**unverified**. No C17 ABI, P027 pin, P028 prefix, P029 recovery, or P030
compatibility contract is changed. P033 can consume these separate source
states when implementing dependency diagnostics; it has not started. P034 owns
execution and P038 owns supported update workflows. Direct acquisition remains
unsupported until protocol, artifact integrity, independent publication
authentication, storage and installation semantics have grounded contracts.
