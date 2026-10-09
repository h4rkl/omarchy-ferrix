#!/bin/sh
# Run the complete nested product session on a Linux host: persistent core,
# the application service (which starts the home surface and launches apps in
# their own namespaces), and the compositor in a host window. `mock` evidence.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
. "$root/targets/qemu-aarch64/scripts/paths.sh"
profile=${FERRIX_QEMU_PROFILE:-"$root/targets/qemu-aarch64/profile.toml"}
# Unix sockets go to the short per-out-dir socket dir (scripts/paths.sh).
mkdir -p "$sock_base/nested"
chmod 700 "$sock_base" "$sock_base/nested"
socket=${FERRIX_CORE_SOCKET:-"$sock_base/nested/core.sock"}
appd_socket=${FERRIX_APPD_SOCKET:-"$sock_base/nested/ferrix-appd.sock"}
# Nested compositor socket name; apps launched by ferrix-appd inherit it.
wayland_display=${FERRIX_NESTED_WAYLAND_DISPLAY:-ferrix-nested-0}
mkdir -p "$out"

cargo build --manifest-path "$root/Cargo.toml" \
  -p ferrix-core -p ferrix-shell -p ferrix-appd -p ferrix-runtime-native -p ferrix-basic-screen -p ferrix-terminal
FERRIX_CORE_SOCKET="$socket" "$root/target/debug/ferrix-core" "$profile" --control none &
core_pid=$!
appd_pid=
cleanup() {
  [ -n "$appd_pid" ] && kill "$appd_pid" 2>/dev/null || true
  kill "$core_pid" 2>/dev/null || true
  [ -n "$appd_pid" ] && wait "$appd_pid" 2>/dev/null || true
  wait "$core_pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

remaining=100
while [ ! -S "$socket" ] && [ "$remaining" -gt 0 ]; do
  sleep 0.05
  remaining=$((remaining - 1))
done
[ -S "$socket" ] || { echo "ferrix-core did not create $socket" >&2; exit 1; }

# The compositor connects to ferrix-appd lazily and retries, so ferrix-appd may start after
# it. Apps resolve their binaries through PATH: put the debug build first.
export PATH="$root/target/debug:$PATH"
export FERRIX_CORE_SOCKET="$socket"
export FERRIX_APPD_SOCKET="$appd_socket"
export FERRIX_QEMU_PROFILE="$profile"
export FERRIX_APP_DIR="$root/apps"
# Unified theming: the compositor and every app ferrix-appd launches read the
# checked-in palette (the guest reads /etc/ferrix/colors.toml), so this
# session reports theme-loaded with this path. Set FERRIX_COLORS to try
# another colors.toml.
export FERRIX_COLORS="${FERRIX_COLORS:-$root/themes/ferrix/colors.toml}"
# Only ferrix-appd (and therefore the apps) see the nested socket name; the nested
# compositor itself is a client of the host session's WAYLAND_DISPLAY.
WAYLAND_DISPLAY="$wayland_display" "$root/target/debug/ferrix-appd" --nested --socket "$appd_socket" --app-dir "$root/apps" &
appd_pid=$!

"$root/target/debug/ferrix-shell" --nested --core "$socket" --ferrix-appd "$appd_socket" --socket "$wayland_display" "$profile"
