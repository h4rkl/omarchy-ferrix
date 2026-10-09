#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
. "$root/targets/qemu-aarch64/scripts/paths.sh"
[ -S "$sock_agent/qmp.sock" ] || { echo "QEMU agent session is not running; run: just qemu-agent-start" >&2; exit 2; }
result="$runtime/frame-result.json"
for _ in $(seq 1 25); do
  if python3 "$root/targets/qemu-aarch64/scripts/qmp.py" --socket "$sock_agent/qmp.sock" screenshot --ppm "$runtime/frame.ppm" --png "$runtime/frame.png" >"$result"; then
    cat "$result"
    exit 0
  fi
  sleep 0.2
done
cat "$result" >&2
echo "QEMU framebuffer remained blank after graphical readiness" >&2
exit 1
