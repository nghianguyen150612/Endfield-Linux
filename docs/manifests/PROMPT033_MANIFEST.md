# P033 partial implementation checkpoint (not accepted)

P033 now has an initial portable assessment engine, CLI, preliminary schema and
hosted initial-HEAD CI evidence. It remains **incomplete and unqualified for acceptance**.
This manifest records source observations and ongoing implementation; it is not
proof of complete dependency assessment or physical/game compatibility.

## Baseline

- Repository: `nghianguyen150612/Endfield-Linux`
- Fetched `main` on 2026-11-03: `d66270a543cc0dc2b579694e6cc7bc7891438717`.
- GitHub reported PR #3 merged as `6f383c6390462cd4da64a97d372fee739b20fd2c`,
  PR #4 as `846d9c867bc49946b2dffe15f2d4632cd6b412df`, and PR #5 as
  `d66270a543cc0dc2b579694e6cc7bc7891438717`.
- The P033 branch was created from that fetched main; the fresh checkout was clean.

## Official requirements observed 2026-11-03

The [Steam listing](https://store.steampowered.com/app/4732690/), via its
app-details API for app 4732690, reported the following Windows requirements:

| Tier | OS | CPU | Memory | GPU | DirectX | Storage | Network |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Minimum | Windows 10 64-bit | Intel Core i5-9400F | 16 GB RAM | NVIDIA GeForce GTX 1060 | 11 | 60 GB available | Broadband |
| Recommended | Windows 10 64-bit | Intel Core i7-10700K | 16 GB RAM | NVIDIA GeForce RTX 2060 | 11 | 60 GB available | Broadband |

The [Epic listing](https://store.epicgames.com/p/arknightsendfield-f0f167)
reported minimum Windows 10 64-bit, Intel Core i5-9400F or equivalent, 16GB,
NVIDIA GeForce GTX 1060 or equivalent, DirectX 11, and 50GB free space or
higher with an additional 40GB free for unpacking. Its recommended section
reported Windows 10/11 64-bit, Intel Core i7-10700K or equivalent or higher,
16GB or higher, NVIDIA GeForce RTX 2060 or equivalent or higher, DirectX 11,
and the same free-space and unpacking guidance, with SSD recommended.

The source's displayed GB unit must not silently be assigned a binary or decimal
byte threshold without recording and justifying the interpretation. The listed
CPU and GPU models do not establish a machine-comparable performance threshold;
DirectX 11 does not establish Vulkan or translation-layer compatibility. Steam
and Epic storage figures are distinct channel guidance, not additive universal
requirements. No GRYPHLINK requirements were established here.

## Remaining work and qualification

P033 remains unfinished: source-specific local Steam evidence, richer typed dependency findings,
recommendations, adversarial cases and final-head qualification remain outstanding. In particular, reuse
P030's existing observation adapter and P027 selection precedence; integrate
P031/P032 and P028/P029 without mutation; maintain explicit unknown states and
privacy bounds; and keep game compatibility untested. Validate official source
evidence again if it is stale when implementation resumes.

The initial checkpoint environment did not expose `cargo`, `rustc`, or `rustup`, and
`pkg-config` could not find GTK4 or libadwaita. No Rust, GTK, actual-executable,
physical-device, or final-head hosted CI qualification was performed for P033.
Do not interpret this checkpoint as completing P033 or authorizing P034.

## Resume progress (still incomplete)

A portable requirement catalog and initial memory/storage comparison have been
added along with a provisional CLI adapter that reuses P030, P012, P031 and the
P032 offline catalog. The command initially lacked a storage target, and the requirements and
recommendation engine still do not cover all P033 cases. The
preliminary schema and executable test do not establish acceptance. See
[dependency diagnostics](../DEPENDENCY-DIAGNOSTICS.md) for current gaps.

Resume validation on 2026-11-03 used Rust 1.85.0 and a disposable Ubuntu
24.04 container with genuine GTK4/libadwaita development libraries. The full
`cargo test --locked --workspace` passed; P033 actual-executable tests included
isolated XDG roots, source selection, denied network/child creation, and unchanged
synthetic game, runtime-pin, prefix, and recovery metadata. The P027–P032
portable regression crates also passed. A generated report validated against
the preliminary JSON schema with P030's embedded schema. These checks do not
cover every requested adversarial case and do not establish P033 completion.

## Stage 2: explicit destination observation (not accepted)

The continuing P033 branch now has an optional `--storage-path` argument for one
user-selected existing directory. The Linux adapter opens each directory
component descriptor-relatively with `O_DIRECTORY | O_NOFOLLOW` and obtains
available bytes via `fstatvfs`. This produces a bounded, path-redacted JSON
storage observation and supplies the existing P033 storage assessor. No
filesystem is selected implicitly and no directories or metadata are written.
The CLI regression covers valid directory observation, a symlink refusal,
absolute-path parsing and no XDG side effects. Epic unpack space remains
unknown without proof of allocation and unpack target. These changes and the
revised schema require hosted CI qualification at their own final HEAD.
P033 remains Draft and unmerged; P034 has not begun.

## Resume 2026-11-04: Steam and storage work in progress

The P033 adapter now accepts one explicit Steam library or appmanifest path with
`--source-id steam`, using P032's bounded observer. A selected P031 game is
compared through the shared P032 path and device/inode matching logic after
registry revision checks. Local observations are redacted, ephemeral and
untrusted. No account credentials or payload files are inspected. Explicit
storage errors distinguish missing and permission-denied from other blocked
paths; Epic unpack readiness remains unknown. Concurrent renames are not
excluded by the descriptor-based snapshot. These changes require final-head
qualification and do not complete the remaining P033 acceptance matrix.
