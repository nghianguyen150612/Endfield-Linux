# P033 implementation checkpoint (incomplete)

P033 has **not** been implemented or qualified. This file records the baseline and
source observations for resuming the work; it is not a dependency catalog or a
claim that the requested CLI, schema, or assessment exists.

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

All P033 implementation, integration, documentation, schemas, and requested
regression and actual-executable tests remain outstanding. In particular, reuse
P030's existing observation adapter and P027 selection precedence; integrate
P031/P032 and P028/P029 without mutation; maintain explicit unknown states and
privacy bounds; and keep game compatibility untested. Validate official source
evidence again if it is stale when implementation resumes.

The current environment did not expose `cargo`, `rustc`, or `rustup`, and
`pkg-config` could not find GTK4 or libadwaita. No Rust, GTK, actual-executable,
physical-device, or final-head hosted CI qualification was performed for P033.
Do not interpret this checkpoint as completing P033 or authorizing P034.
