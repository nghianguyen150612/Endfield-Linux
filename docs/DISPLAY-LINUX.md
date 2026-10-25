# Linux display and session discovery (P014)

P014 adds the read-only `endfield-unix diagnose display` probe to the
library-only `endfield-unix-platform-linux` adapter. It combines P011 platform
identity, P012 CPU and memory facts, and P013 GPU facts with local Wayland,
X11/RandR, and DRM connector observations. It does not change P007 snapshot
schema 1, P010 IPC 1.0, the C17 ABI, stored configuration, or display state.

## Evidence and session semantics

The adapter reads only these environment hints: `XDG_SESSION_TYPE`,
`WAYLAND_DISPLAY`, `DISPLAY`, `XDG_CURRENT_DESKTOP`, and
`XDG_SESSION_DESKTOP`. Each value is length-limited and used only to select
local endpoints or produce a sanitized, explicitly unverified desktop hint.
Environment metadata is not proof that a session or compositor is running.

`SessionAssessment` keeps the suggested session type separate from endpoint
status and the verified display protocol. Both `WAYLAND_DISPLAY` and `DISPLAY`
may be present in an XWayland session; when both connections work, both are
reported, but Wayland owns the selected output inventory so XWayland outputs
are not counted a second time. A failed or stale endpoint yields a partial
report. With no endpoint hints, the command succeeds with a headless/unknown
session and unknown display capability. A DRM connector observation remains
independent of display-server reachability.

Desktop names such as Hyprland, Niri, GNOME, KDE, or Sway are shown only as
unverified environment hints. P014 does not inspect process lists, private
configuration, or compositor-specific control sockets, and it does not claim
a verified compositor identity. Protocol support is reported separately from
compositor identity and says nothing about permission to change display state.

## Wayland

`SystemWaylandBackend` connects only to a local Unix socket named by a
validated `WAYLAND_DISPLAY` basename under a validated, private
`XDG_RUNTIME_DIR`. It checks socket type and ownership before connecting. It
uses `wayland-client` 0.31.11 to read the registry, bind at most 64 `wl_output`
globals, and collect up to 128 modes per output. The standard output mode event
provides physical pixel dimensions, current/preferred flags, and refresh in
millihertz; the report stores refresh as a reduced integer rational. Core
`wl_output.scale` is kept separate from physical dimensions. If advertised,
`xdg-output-unstable-v1` supplies logical position and size. No surface is
created and no output-management or state-changing request is sent.

The registry pass is followed by a second bounded sync so output events sent
after a bind are not mistaken for an empty inventory. Output removal or
topology changes during discovery, malformed mode dimensions, and count-limit
overflow produce an inconsistent/partial inventory. No `wl_output` globals
after a successful registry query is an enumerated zero, not a connection
failure. Standard Wayland does not identify a global primary output, HDR, VRR,
or a universally meaningful application scale; those values remain unknown.
The optional xdg-output protocol is used only when advertised. Fractional
scale is not inferred from physical and logical dimensions.

Each protocol roundtrip has a two-second deadline and the caller waits at most
five seconds for the worker. A blocked local socket connect or client-library
call cannot be forcibly cancelled; the worker is detached after the caller's
deadline and may finish later. The CLI caller remains bounded. Real compositor
integration is not established by the synthetic backend fixtures.

## X11 and RandR

`SystemX11Backend` accepts only local `:display[.screen]` or `unix/:display`
syntax. Host names and TCP-style `DISPLAY` values are rejected before
`x11rb` 0.13.2 is called. Normal Xauthority authentication is retained; an
authentication refusal is a separate status. The adapter reads RandR version,
screen geometry, output resources, output-to-CRTC state, primary output,
advertised modes, and current mode. Outputs without an active CRTC and
disconnected outputs remain distinct from active outputs.

Mode refresh is computed from RandR dot-clock and totals with checked integer
arithmetic; interlace and doublescan timing flags are accounted for. Invalid
or insufficient timing leaves refresh unknown. Duplicate or stale resource
relationships degrade the inventory rather than creating a confident result.
The adapter never grabs the server or sends RandR configuration requests.

RandR work runs in a worker and the caller waits at most two seconds. A blocked
server/library call cannot be forcibly cancelled, so a detached worker may
remain until the library call returns. The command itself does not wait
indefinitely. Xvfb output is an X server output and is not automatically
classified as a physical panel; its connector and physical identity remain
unknown unless independent evidence exists.

## Outputs, exact modes, and snapshot mapping

The Linux report keeps physical mode dimensions, logical geometry, scale,
rotation, current/preferred state, and output state in separate typed fields.
`DisplayInfo` is the existing P007 portable record: it receives the current
resolution and an integer `Hertz` only when the exact rate is a whole number.
Fractional values remain unchanged in the Linux mode report, such as
`2997/50 Hz` for 59.94 Hz. No frame rate, presentation timing, FPS, or tearing
behavior is inferred.

Up to 64 outputs receive deterministic snapshot-local IDs beginning at
1,000,001, after sorting by sanitized output label and geometry. Labels are
restricted to simple ASCII connector-style names; unsafe or unusually long
names are replaced with a generic label. IDs are snapshot-local and are not
stable promises across hotplug or compositor restarts. The P007 display and
compositor/session capability IDs are 40 and 41. New provenance IDs 13–16
identify session hints, Wayland, X11, and DRM connector sources. The final
snapshot is normalized and validated after P014 records are appended.

P014 does not associate a Wayland or X11 output with a DRM connector by
matching names. DRM connectors are read independently from bounded
`/sys/class/drm` status, enabled, and mode files. Their sysfs links must resolve
under `/sys/devices`; a GPU association is recorded only when the validated
PCI BDF agrees with a P013 primary DRM node and GPU record. Connector
`connected`, `enabled`, and advertised modes do not prove that a display
server is using that connector or mode. EDID and serial values are not read.

P012 CPU/memory and P013 PCI/DRM/Vulkan observations remain in the combined
report when a display source fails. Failed enumeration is not converted to a
claim that there is no display hardware. A successful zero-output protocol
inventory is distinguished from an unavailable protocol or a failed
connection.

## Privacy and operational bounds

The adapter is unprivileged and uses no shell commands, external display
utilities, network connections, compositor configuration, DRM device opens,
EDID reads, mode changes, scaling changes, or backlight/gamma controls. It does
not persist output labels or include display data in P008 logs or support
reports. Fixed diagnostics omit endpoint paths and raw environment values.

Bounds are 64 display outputs, 128 modes per display output, 128 DRM connector
objects, 128 advertised mode lines per connector, 512 scanned DRM sysfs
directory entries, 4 KiB per connector attribute, and bounded Wayland/X11
caller waits. A display server or in-process graphics library may still leave
a detached provider worker running after the caller times out; this is an API
cancellation limitation, not a reason to claim a complete report.

The composite probe runs the existing P013 GPU inventory before display
discovery. As documented in [GPU-LINUX.md](GPU-LINUX.md), third-party Vulkan
ICD calls run in-process and cannot be forcibly cancelled if they block. The
Wayland and X11 caller waits do not place a hard deadline around that earlier
Vulkan call.

## CLI and validation

```sh
cargo +1.85.0 run --locked -p endfield-unix -- diagnose display
```

The command is headless, does not initialize GTK, does not contact
`endfield-unixd`, does not open application storage, and returns status 0 for
complete or partial observations. Fatal required-platform errors use 70,
output failures use 74, unsupported platforms use 78, and invalid arguments
use 2.

Deterministic fixtures cover headless Linux, independent DRM connectors,
Wayland plus XWayland, multiple outputs, an unverified Hyprland hint, exact
fractional mode rates, scaling/logical geometry, X11 primary and disconnected
outputs, stale Wayland endpoints, X11 authentication failure, snapshot
validation, and preservation of CPU/memory/GPU facts. Hosted CI runs the
headless CLI plus an Xvfb/RandR smoke and checks live X11 reachability and
screen geometry. A synthetic Wayland fixture is not evidence of a real
Hyprland, Niri, KWin, or wlroots session.

## Manual Arch Linux validation

On an Arch desktop or laptop, compare the probe with tools already installed
on that machine:

```sh
cargo +1.85.0 run --locked -p endfield-unix -- diagnose display
printf 'Session: %s\n' "${XDG_SESSION_TYPE:-unknown}"
printf 'Desktop: %s\n' "${XDG_CURRENT_DESKTOP:-unknown}"
wlr-randr                 # optional, Wayland/wlroots environments
xrandr --verbose          # optional, X11 environments
ls /sys/class/drm/
```

These utilities are for manual comparison only and are not runtime
dependencies. Check the desktop's actual active mode and the Dell Vostro
3510's internal eDP and any external output independently. Do not infer the
active display GPU from the NVIDIA MX350's presence.

Arch desktop display validation: **UNVERIFIED**

Dell Vostro 3510 display validation: **UNVERIFIED**

Real Hyprland/Niri session validation: **UNVERIFIED**

Hosted Xvfb validates the X11/RandR path only. P015 telemetry, P016–P020,
compositor control, mode switching, frame pacing, FPS, VRR, HDR, and display
optimization remain outside P014.
