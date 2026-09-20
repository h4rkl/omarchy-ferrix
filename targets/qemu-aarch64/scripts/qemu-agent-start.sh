#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64
runtime="$out/agent"
pid_file="$runtime/qemu.pid"
launch_log="$runtime/launch.log"
profile=${FERRIX_QEMU_PROFILE:-"$root/targets/qemu-aarch64/profile.toml"}

[ -f "$out/Image" ] && [ -f "$out/initramfs.cpio.gz" ] || { echo "missing guest artifacts; run: just qemu-build" >&2; exit 2; }
command -v python3 >/dev/null || { echo "missing python3; needed for QMP framebuffer and input control" >&2; exit 2; }
mkdir -p "$runtime"
if [ -f "$pid_file" ] && kill -0 "$(cat "$pid_file")" 2>/dev/null; then
  echo "QEMU agent session already running (pid $(cat "$pid_file"))" >&2
  exit 2
fi
rm -f "$pid_file" "$runtime/qmp.sock" "$runtime/serial.log" "$launch_log"
printf 'starting QEMU agent session\n'
printf 'serial log: %s\nQMP socket: %s\n' "$runtime/serial.log" "$runtime/qmp.sock"
# TCG is the portable agent default. A host accelerator remains an explicit
# override because graphics/readback behavior varies across host backends.
FERRIX_QEMU_AGENT=1 FERRIX_QEMU_MODE=agent FERRIX_QEMU_ACCEL=${FERRIX_QEMU_ACCEL:-tcg} FERRIX_QEMU_GRAPHICS=${FERRIX_QEMU_GRAPHICS:-none} "$root/targets/qemu-aarch64/scripts/qemu-run.sh" >"$launch_log" 2>&1 &
pid=$!
printf '%s\n' "$pid" > "$pid_file"
for _ in $(seq 1 100); do
  [ -S "$runtime/qmp.sock" ] && break
  kill -0 "$pid" 2>/dev/null || { cat "$launch_log" >&2; rm -f "$pid_file"; exit 1; }
  sleep 0.1
done
[ -S "$runtime/qmp.sock" ] || { echo "QMP socket did not become ready" >&2; exit 1; }
printf 'QEMU control ready (pid %s); waiting for rendered client\n' "$pid"
# Weston advertises its pointer seat after the first tablet event. Move to a
# non-actionable launcher coordinate so input readiness is observable without
# activating UI.
width=$(awk -F= '/^[[:space:]]*width[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
height=$(awk -F= '/^[[:space:]]*height[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
for _ in $(seq 1 450); do
  if [ -f "$runtime/serial.log" ] \
    && grep -q '"event":"wayland-ready"' "$runtime/serial.log" \
    && grep -q '"event":"input-ready".*"capability":"pointer".*"status":"ok"' "$runtime/serial.log"; then
    printf 'QEMU graphical session ready (pid %s)\n' "$pid"
    exit 0
  fi
  if [ -f "$runtime/serial.log" ] && grep -q '"event":"wayland-ready"' "$runtime/serial.log"; then
    python3 "$root/targets/qemu-aarch64/scripts/qmp.py" --socket "$runtime/qmp.sock" move "$((width / 2))" "$((height / 3))" --width "$width" --height "$height" >/dev/null
  fi
  if [ -f "$runtime/serial.log" ] && grep -q '"event":"wayland-failed"' "$runtime/serial.log"; then
    cat "$runtime/serial.log" >&2
    echo "QEMU graphical client failed to connect" >&2
    break
  fi
  kill -0 "$pid" 2>/dev/null || { cat "$launch_log" >&2; [ ! -f "$runtime/serial.log" ] || cat "$runtime/serial.log" >&2; echo "QEMU exited before graphical readiness" >&2; break; }
  sleep 0.1
done
python3 "$root/targets/qemu-aarch64/scripts/qmp.py" --socket "$runtime/qmp.sock" quit 2>/dev/null || kill "$pid" 2>/dev/null || true
rm -f "$pid_file" "$runtime/qmp.sock"
echo "timed out waiting for QEMU graphical readiness" >&2
exit 1
