#!/bin/sh
# Send a mock-control command to the running guest's mobile-core.
#   just qemu-mock set-power charging=true
#   just qemu-mock '{"cmd":"set-wifi","state":"disconnected"}'
# Targets the agent session's socket when it exists, otherwise the plain
# qemu-run/qemu-check socket. OMARCHY_QEMU_CONTROL_SOCKET overrides.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${OMARCHY_OUT_DIR:-"$root/out"}/qemu-aarch64
command -v python3 >/dev/null || { echo "missing python3; needed for the mock-control client" >&2; exit 2; }
[ "$#" -ge 1 ] || { echo "usage: qemu-mock.sh NAME key=value ... | qemu-mock.sh '{...json...}'" >&2; exit 2; }
if [ -n "${OMARCHY_QEMU_CONTROL_SOCKET:-}" ]; then
  socket=$OMARCHY_QEMU_CONTROL_SOCKET
elif [ -S "$out/agent/control.sock" ]; then
  socket="$out/agent/control.sock"
else
  socket="$out/control.sock"
fi
[ -S "$socket" ] || { echo "no mock-control socket at $socket; start the guest first (just qemu-agent-start or just qemu-run)" >&2; exit 2; }
exec python3 "$root/targets/qemu-aarch64/scripts/mock_control.py" --socket "$socket" "$@"
