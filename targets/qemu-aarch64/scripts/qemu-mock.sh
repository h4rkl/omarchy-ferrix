#!/bin/sh
# Send a mock-control command to a daemon in the running guest.
#   just qemu-mock set-power charging=true                      # ferrix-core (default)
#   just qemu-mock --service telephony incoming-call number=+15550100042
#   just qemu-mock --service network set-radio enabled=false
#   just qemu-mock --service core '{"cmd":"set-wifi","state":"disconnected"}'
# Channels (ADR-006): core -> control.sock (virtio port ferrix.control),
# telephony -> telephony-control.sock (ferrix.telephony), network ->
# network-control.sock (ferrix.network). Targets the agent session's sockets
# when they exist, otherwise the plain qemu-run/qemu-check sockets.
# FERRIX_QEMU_CONTROL_SOCKET overrides the resolved path for any service.
# This drives *mock* state in the virtual phone; never a product interface.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64
command -v python3 >/dev/null || { echo "missing python3; needed for the mock-control client" >&2; exit 2; }
usage() { echo "usage: qemu-mock.sh [--service core|telephony|network] NAME key=value ... | qemu-mock.sh [--service ...] '{...json...}'" >&2; exit 2; }
service=core
while [ "$#" -gt 0 ]; do
  case "$1" in
    --service) [ "$#" -ge 2 ] || usage; service=$2; shift 2 ;;
    --service=*) service=${1#--service=}; shift ;;
    --) shift; break ;;
    *) break ;;
  esac
done
[ "$#" -ge 1 ] || usage
case "$service" in
  core) name=control.sock ;;
  telephony) name=telephony-control.sock ;;
  network) name=network-control.sock ;;
  *) echo "unknown --service $service (core|telephony|network)" >&2; exit 2 ;;
esac
if [ -n "${FERRIX_QEMU_CONTROL_SOCKET:-}" ]; then
  socket=$FERRIX_QEMU_CONTROL_SOCKET
elif [ -S "$out/agent/$name" ]; then
  socket="$out/agent/$name"
else
  socket="$out/$name"
fi
[ -S "$socket" ] || { echo "no $service mock-control socket at $socket; start the guest first (just qemu-agent-start or just qemu-run)" >&2; exit 2; }
exec python3 "$root/targets/qemu-aarch64/scripts/mock_control.py" --socket "$socket" "$@"
