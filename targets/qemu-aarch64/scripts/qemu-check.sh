#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${OMARCHY_OUT_DIR:-"$root/out"}/qemu-aarch64
log="$out/serial.log"
mkdir -p "$out"
OMARCHY_QEMU_ACCEL=tcg OMARCHY_QEMU_GRAPHICS=off OMARCHY_QEMU_MODE=check "$root/targets/qemu-aarch64/scripts/qemu-run.sh" >"$log" 2>&1 &
qemu_pid=$!
for _ in $(seq 1 450); do
  kill -0 "$qemu_pid" 2>/dev/null || break
  sleep 0.1
done
if kill -0 "$qemu_pid" 2>/dev/null; then
  kill "$qemu_pid" 2>/dev/null || true
  status=124
fi
wait "$qemu_pid" || status=$?
status=${status:-0}
cat "$log"
[ "$status" -eq 0 ] || { echo "QEMU smoke test failed (exit $status)" >&2; exit "$status"; }
grep -q '"event":"boot-health"' "$log" || { echo "missing QEMU boot-health record" >&2; exit 1; }
grep -q '"charging":true' "$log" || { echo "missing mock charging transition" >&2; exit 1; }
