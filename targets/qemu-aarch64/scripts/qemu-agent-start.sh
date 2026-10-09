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
graphics=${FERRIX_QEMU_GRAPHICS:-none}
case "$graphics" in
  none|off)
    # Ask QEMU itself to daemonize. A plain shell background job can receive
    # SIGHUP when this short-lived launcher exits (notably from automation),
    # leaving qemu-tap/qemu-swipe with a stale pid file and dead QMP socket.
    FERRIX_QEMU_AGENT=1 FERRIX_QEMU_MODE=agent FERRIX_QEMU_DAEMONIZE=1 \
      FERRIX_QEMU_PID_FILE="$pid_file" FERRIX_QEMU_ACCEL=${FERRIX_QEMU_ACCEL:-tcg} \
      FERRIX_QEMU_GRAPHICS="$graphics" \
      "$root/targets/qemu-aarch64/scripts/qemu-run.sh" >"$launch_log" 2>&1
    [ -s "$pid_file" ] || { cat "$launch_log" >&2; echo "QEMU did not write its pid file" >&2; exit 1; }
    pid=$(cat "$pid_file")
    ;;
  *)
    # Cocoa/GTK must retain their foreground UI process, so keep the original
    # background launch for explicitly requested host-window sessions.
    FERRIX_QEMU_AGENT=1 FERRIX_QEMU_MODE=agent FERRIX_QEMU_ACCEL=${FERRIX_QEMU_ACCEL:-tcg} \
      FERRIX_QEMU_GRAPHICS="$graphics" \
      "$root/targets/qemu-aarch64/scripts/qemu-run.sh" >"$launch_log" 2>&1 &
    pid=$!
    printf '%s\n' "$pid" > "$pid_file"
    ;;
esac
for _ in $(seq 1 100); do
  [ -S "$runtime/qmp.sock" ] && break
  kill -0 "$pid" 2>/dev/null || { cat "$launch_log" >&2; rm -f "$pid_file"; exit 1; }
  sleep 0.1
done
[ -S "$runtime/qmp.sock" ] || { echo "QMP socket did not become ready" >&2; exit 1; }
printf 'QEMU control ready (pid %s); waiting for rendered client and hit-testable launcher\n' "$pid"
# Weston advertises its pointer seat after the first tablet event. Move to a
# non-actionable launcher coordinate so input readiness is observable without
# activating UI.
width=$(awk -F= '/^[[:space:]]*width[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
height=$(awk -F= '/^[[:space:]]*height[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
# Readiness includes wayland-ready (compositor + home surface committed),
# input-ready pointer (seat announced), cursor-ready (a pointer event reached
# the compositor), and home-ready (the launcher's catalog is loaded and
# hit-testable). Requiring cursor-ready prevents the first developer tap from
# being consumed while the compositor initializes its pointer surface.
# The probe alternates between two neighbouring pixels: QEMU drops an
# absolute move to the position it already reported, so a single fixed
# point sent before the compositor's pointer was ready never produced a
# second event and readiness timed out.
for i in $(seq 1 450); do
  if [ -f "$runtime/serial.log" ] && grep -q '"event":"wayland-ready"' "$runtime/serial.log"; then
    python3 "$root/targets/qemu-aarch64/scripts/qmp.py" --socket "$runtime/qmp.sock" move "$((width / 2 + i % 2))" "$((height / 3))" --width "$width" --height "$height" >/dev/null
  fi
  if [ -f "$runtime/serial.log" ] \
    && grep -q '"event":"wayland-ready"' "$runtime/serial.log" \
    && grep -q '"event":"input-ready".*"capability":"pointer".*"status":"ok"' "$runtime/serial.log" \
    && grep -q '"event":"cursor-ready"' "$runtime/serial.log" \
    && grep -qE 'home-ready[^"]*".*"tiles":[0-9]+' "$runtime/serial.log"; then
    # Several guest processes write records to the serial console, so a line
    # can be interleaved with another writer's text (observed:
    # `"event":"home-ready/bin/sh: ","tiles":8,...`). Match the record's
    # name plus its `tiles` field rather than the exact JSON prefix.
    printf 'QEMU graphical session ready (pid %s): %s\n' "$pid" "$(grep -m1 -E 'home-ready[^"]*".*"tiles":[0-9]+' "$runtime/serial.log")"
    exit 0
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
echo "timed out waiting for QEMU graphical readiness (wayland-ready + input-ready pointer + cursor-ready + home-ready)" >&2
exit 1
