# systemd user service

P020 optionally runs the existing foreground `endfield-unixd` process under a
Linux `systemd --user` manager. It does not add a helper executable, root
service, game launcher, process-control IPC, or GUI-to-daemon auto-start. The
service remains disabled until the user explicitly enables it.

## Installation and commands

Build both application binaries with the supported toolchain. Install uses
only the daemon executable; the GUI/CLI remains an ordinary user-launched
program.

```bash
cargo +1.85.0 build --locked --release -p endfield-unix -p endfield-unixd
target/release/endfield-unix service systemd install
target/release/endfield-unix service systemd enable
target/release/endfield-unix service systemd start
target/release/endfield-unix service ping
target/release/endfield-unix service status
```

`install` selects the adjacent `endfield-unixd` release executable when
available. Otherwise provide an explicit absolute path with
`service systemd install --daemon /absolute/path/to/endfield-unixd`. Sources
inside `target/debug` are rejected. Installation copies the executable to
`~/.local/bin/endfield-unixd` and installs
`endfield-unixd.service` under
`${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/`. The ownership manifest and
installer lock live under
`${XDG_STATE_HOME:-$HOME/.local/state}/endfield-unix/systemd/`.

Installation is an explicit unprivileged operation. Running these management
commands as root is rejected. It creates private app-owned directories and
files, checks each existing path component without following symlinks, checks
file ownership and type, stages files in the destination directory, and uses
atomic creation/replacement. An unknown existing unit or executable is a
conflict unless `--replace` is supplied. Replacement requires an available
user manager, an inactive unit, and no authenticated daemon IPC listener. The
prior file is kept at a `.p020-backup` sibling path. Review such a backup
before removing it manually.

The unit contains only generated, fixed content. It executes the installed
daemon with `--foreground`, uses `Type=notify`, and has bounded startup and
stop timeouts. The installer runs the trusted `systemd-analyze verify` against
the exact unit after the daemon is present at its target path. It reloads the
user manager when reachable, but does not enable or start the service. If the
manager is unavailable, installation may still be staged and reports that a
user-session `systemctl --user daemon-reload` is needed before management
operations.

The service commands are:

```text
endfield-unix service systemd install [--daemon PATH] [--replace]
endfield-unix service systemd uninstall
endfield-unix service systemd enable
endfield-unix service systemd disable
endfield-unix service systemd start
endfield-unix service systemd stop
endfield-unix service systemd restart
endfield-unix service systemd status
```

Enable and disable change autostart only; neither starts nor stops a running
service. Start and restart wait for systemd's readiness handshake and then
perform a real authenticated IPC Ping. Uninstall refuses an active service,
disables it, and removes only the exact unit and daemon identified by the
installer ownership record. A modified or unverified daemon executable is
retained. Uninstall never recursively removes the user's config, state, game,
prefix, or recording directories.

`service status` remains the authenticated daemon IPC status command.
`service systemd status` separately reports manager reachability, the unit-file
ownership state, load/enabled/active/substate, main PID and last result, plus a
new live authenticated IPC Ping. An active unit without healthy IPC is
reported as a discrepancy. IPC that works while the unit is inactive may
belong to a manually started daemon; P020 neither adopts nor terminates it.

## Readiness, runtime paths, and shutdown

The daemon reads only the service-manager-provided `NOTIFY_SOCKET` and sends
fixed `READY=1` and `STOPPING=1` messages. Filesystem destinations must be the
user manager's `$XDG_RUNTIME_DIR/systemd/notify` socket beneath a private,
owner-controlled runtime tree; abstract destinations must use systemd's
canonical notification name. Readiness is sent only after signal
handling, the P019 supervisor worker, validated P010 runtime directory,
exclusive socket ownership and bind, bounded IPC worker startup, the Running
lifecycle transition, listener activation, and a successful authenticated
Ping. A bind conflict, missing runtime directory, failed Ping, worker failure,
interrupted startup, or failed READY notification produces no successful
readiness notification. A notification send error is typed and makes startup
fail; a STOPPING send error emits one sanitized warning and cleanup continues.
When `NOTIFY_SOCKET` is absent, ordinary foreground execution remains
available and no notification socket is created. Filesystem and abstract
Linux Unix-datagram addresses are handled; only fixed protocol messages are
sent. Notifications contain no private state.

The user manager normally supplies `XDG_RUNTIME_DIR`. P010 remains the owner
of `$XDG_RUNTIME_DIR/endfield-unix/endfield-unixd.sock`, its 0700 application
directory, 0600 socket and lock, instance lock, peer-UID checks, stale-socket
rules, and inode-safe removal. The service does not use `RuntimeDirectory=`
because systemd-managed removal could race P010 cleanup or manual daemon use.
There is no `/tmp` fallback. A daemon without a valid runtime directory fails
startup rather than advertising a service that cannot accept IPC.

systemd sends SIGTERM on stop. The daemon sends STOPPING near the beginning of
shutdown, rejects new session registrations, closes IPC, stops P019 monitoring,
releases its owned socket, completes worker tokens, drains optional diagnostics
with a 500 ms bound, and exits. Owned worker waits have a two-second bound;
timeouts do not claim to cancel synchronous operations. The unit's
`TimeoutStopSec=10s` leaves a margin over the bounded cleanup sequence. Clean
SIGTERM is not a failure restart. Unexpected failures may restart after three
seconds, with three starts allowed in a 60-second interval. No session state is
restored after a restart.

## Process scope, GUI, and logout

The GUI neither owns nor controls the daemon. Closing the GUI window or a
terminal does not stop an independently running user service. Stopping or
restarting the daemon is different: the unit keeps `KillMode=control-group`,
so systemd can clean up remaining processes in the service cgroup after the
main process exits. P019 not sending a signal to a child does not override
that cgroup policy. P020 creates no production game process and promises no
game survival across service stop or restart. `KillMode=process` and
`KillMode=none` are not used as workarounds. P034–P036 must evaluate a separate
user-managed game scope and test restart, GUI closure, logout, process-group
signals, cleanup ownership, and recovery before launching games.

The daemon follows the user's systemd manager lifecycle. GUI closure,
terminal closure, service stop, user-manager stop, logout, and reboot are
separate events. No default promise is made that the daemon or user manager
survives logout. Lingering can keep a user manager running without a login
session, but P020 never enables lingering, writes system-wide login policy,
changes `/var/lib/systemd/linger`, or requires sudo. Non-systemd Linux keeps
the existing manual `endfield-unixd --foreground` path and all current GUI,
CLI, discovery, telemetry, IPC, and daemon functions.

The service hardening is deliberately limited to `UMask=0077`,
`NoNewPrivileges=true`, fixed execution, and explicit timeouts/restart limits.
`ProtectHome`, `DynamicUser`, `PrivateDevices`, memory execution restrictions,
and broad filesystem isolation are deferred because P008 diagnostics and P010
IPC need normal per-user state and runtime paths.

## Availability and manual Arch validation

The CLI probes `systemctl --user show --property=Version`; the presence of a
systemd PID 1 or `/run/systemd/system` alone is not treated as a working user
manager. Status distinguishes non-Linux, another init, missing `systemctl`, a
container without a manager, manager/D-Bus unavailability, permission denial,
and other probe failures. None of those states disables manual foreground
daemon use on Linux.

On a disposable or intended Arch Linux user account, build and inspect the
release artifacts before installing:

```bash
cargo +1.85.0 build --locked --release -p endfield-unix -p endfield-unixd
target/release/endfield-unix service systemd install --daemon "$PWD/target/release/endfield-unixd"
target/release/endfield-unix service systemd enable
target/release/endfield-unix service systemd start
systemctl --user status endfield-unixd.service
systemctl --user is-enabled endfield-unixd.service
systemctl --user is-active endfield-unixd.service
journalctl --user -u endfield-unixd.service -n 50 --no-pager
target/release/endfield-unix service ping
target/release/endfield-unix service status
systemctl --user restart endfield-unixd.service
target/release/endfield-unix service ping
systemctl --user stop endfield-unixd.service
target/release/endfield-unix service systemd disable
target/release/endfield-unix service systemd uninstall
```

The optional [`scripts/systemd-user-integration.sh`](../scripts/systemd-user-integration.sh)
automates these checks only after explicit opt-in and a disposable user-account
assertion. It refuses existing service files, installation records, binaries,
active units, and a live daemon. It does not run against a developer's login
configuration by default.

Hosted CI validates the unit syntax and notification/service contracts. If an
isolated real user manager is not available, qualification must state exactly:

`Real systemd --user integration: UNVERIFIED`

The Arch procedure and real game/runtime compatibility have not been run by
unit or hosted CI tests.
