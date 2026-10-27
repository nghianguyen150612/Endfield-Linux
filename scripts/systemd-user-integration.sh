#!/usr/bin/env bash
set -euo pipefail

if [[ "${ENDFIELD_SYSTEMD_USER_INTEGRATION:-}" != "1" || "${ENDFIELD_DISPOSABLE_TEST_ACCOUNT:-}" != "1" ]]; then
  echo "Real systemd --user integration: UNVERIFIED (requires explicit opt-in from a disposable user account)."
  exit 77
fi

if [[ "${EUID}" == "0" ]]; then
  echo "Refusing to run the user-service integration gate as root." >&2
  exit 2
fi

cli="${ENDFIELD_CLI:-target/release/endfield-unix}"
daemon="${ENDFIELD_DAEMON:-target/release/endfield-unixd}"
if [[ ! -x "$cli" || ! -x "$daemon" ]]; then
  echo "Build both release executables or set ENDFIELD_CLI and ENDFIELD_DAEMON." >&2
  exit 2
fi
if [[ "$daemon" == *"/target/debug/"* ]]; then
  echo "The integration gate requires a release daemon executable." >&2
  exit 2
fi

if ! systemctl --user show --property=Version >/dev/null 2>&1; then
  echo "Real systemd --user integration: UNVERIFIED (no reachable user manager)."
  exit 77
fi

config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
state_home="${XDG_STATE_HOME:-$HOME/.local/state}"
unit="$config_home/systemd/user/endfield-unixd.service"
owner="$state_home/endfield-unix/systemd/ownership-v1"
installed_daemon="$HOME/.local/bin/endfield-unixd"
if [[ -e "$unit" || -L "$unit" || -e "$owner" || -L "$owner" || -e "$installed_daemon" || -L "$installed_daemon" ]]; then
  echo "Refusing to touch an existing unit, ownership record, or daemon executable." >&2
  exit 2
fi
if systemctl --user is-active --quiet endfield-unixd.service; then
  echo "Refusing to replace or stop an already active unit." >&2
  exit 2
fi
if "$cli" service ping >/dev/null 2>&1; then
  echo "Refusing to interact with an independently running daemon." >&2
  exit 2
fi

cleanup() {
  set +e
  "$cli" service systemd stop >/dev/null 2>&1
  "$cli" service systemd disable >/dev/null 2>&1
  "$cli" service systemd uninstall >/dev/null 2>&1
}
trap cleanup EXIT

"$cli" service systemd install --daemon "$daemon"
"$cli" service systemd enable
systemctl --user is-enabled --quiet endfield-unixd.service
"$cli" service systemd start
"$cli" service ping
"$cli" service status
"$cli" service systemd status
"$cli" service systemd restart
"$cli" service ping
"$cli" service systemd stop
if "$cli" service ping >/dev/null 2>&1; then
  echo "IPC remained reachable after stopping the test unit." >&2
  exit 1
fi
test ! -e "${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR must be provided by the user manager}/endfield-unix/endfield-unixd.sock"
"$cli" service systemd disable
"$cli" service systemd uninstall
trap - EXIT
echo "Real systemd --user integration: PASSED in the explicitly selected disposable account."
