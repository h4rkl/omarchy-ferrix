#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
# out, runtime (logs: $out/agent) and sock_agent (short socket dir).
. "$root/targets/qemu-aarch64/scripts/paths.sh"
pid_file="$runtime/qemu.pid"
launch_log="$runtime/launch.log"
serial="$runtime/serial.log"
qmp_socket="$sock_agent/qmp.sock"
profile=${FERRIX_QEMU_PROFILE:-"$root/targets/qemu-aarch64/profile.toml"}
# Wall-clock bound for boot to graphical readiness (TCG on a loaded host is
# slow; the old 450-iteration loop was really a ~60-110 s budget).
timeout=${FERRIX_QEMU_AGENT_TIMEOUT:-150}
case "$timeout" in
  ""|*[!0-9]*|0) echo "invalid FERRIX_QEMU_AGENT_TIMEOUT: $timeout (positive integer seconds)" >&2; exit 2 ;;
esac

[ -f "$out/Image" ] && [ -f "$out/initramfs.cpio.gz" ] || { echo "missing guest artifacts; run: just qemu-build" >&2; exit 2; }
command -v python3 >/dev/null || { echo "missing python3; needed for QMP framebuffer and input control" >&2; exit 2; }
mkdir -p "$runtime"
ferrix_socket_dirs
if [ -f "$pid_file" ] && kill -0 "$(cat "$pid_file")" 2>/dev/null; then
  echo "QEMU agent session already running (pid $(cat "$pid_file"))" >&2
  exit 2
fi
rm -f "$pid_file" "$qmp_socket" "$serial" "$launch_log"
# Where this session's sockets are, for tools that only know the out dir.
printf '%s\n' "$sock_agent" > "$runtime/socket-dir"
printf 'starting QEMU agent session\n'
printf 'serial log: %s\nQMP socket: %s\n' "$serial" "$qmp_socket"
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
start=$(date +%s)
elapsed() { echo $(( $(date +%s) - start )); }
# fail REASON: one machine-readable record naming what was still missing,
# then stop the half-started session.
fail() {
  missing=
  for record in wayland-ready input-ready-pointer cursor-ready home-ready; do
    have "$record" || missing="$missing${missing:+,}\"$record\""
  done
  printf '{"evidence":"QEMU","event":"agent-start-failed","reason":"%s","missing":[%s],"probes":%s,"seconds":%s,"serial_log":"%s"}\n' \
    "$1" "$missing" "$probes" "$(elapsed)" "$serial" >&2
  python3 "$root/targets/qemu-aarch64/scripts/qmp.py" --socket "$qmp_socket" quit 2>/dev/null || kill "$pid" 2>/dev/null || true
  rm -f "$pid_file" "$qmp_socket"
  echo "timed out or failed waiting for QEMU graphical readiness (wayland-ready + input-ready pointer + cursor-ready + home-ready)" >&2
  exit 1
}
# have NAME: the readiness record NAME is in the serial log. The patterns
# match the record's name plus a distinguishing field rather than an exact
# JSON prefix, so console noise in front of a record (a shell prompt, a
# kernel line) does not hide it.
have() {
  [ -f "$serial" ] || return 1
  case "$1" in
    wayland-ready) grep -q '"event":"wayland-ready"' "$serial" ;;
    input-ready-pointer) grep -q '"event":"input-ready".*"capability":"pointer".*"status":"ok"' "$serial" ;;
    cursor-ready) grep -q '"event":"cursor-ready"' "$serial" ;;
    home-ready) grep -qE '"event":"home-ready[^"]*".*"tiles":[0-9]+' "$serial" ;;
    wayland-failed) grep -q '"event":"wayland-failed"' "$serial" ;;
  esac
}
probes=0
while [ ! -S "$qmp_socket" ]; do
  kill -0 "$pid" 2>/dev/null || { cat "$launch_log" >&2; rm -f "$pid_file"; exit 1; }
  [ "$(elapsed)" -lt 10 ] || { echo "QMP socket did not become ready" >&2; fail qmp-socket-missing; }
  sleep 0.1
done
printf 'QEMU control ready (pid %s); waiting for rendered client and hit-testable launcher\n' "$pid"
width=$(awk -F= '/^[[:space:]]*width[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
height=$(awk -F= '/^[[:space:]]*height[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
# Readiness includes wayland-ready (compositor + home surface committed),
# input-ready pointer (the home surface bound its wl_pointer), cursor-ready
# (that pointer received wl_pointer.enter and installed the cursor), and
# home-ready (the launcher's catalog is loaded and hit-testable). Requiring
# cursor-ready prevents the first developer tap from being consumed while the
# client initializes its pointer surface.
#
# The probe starts only after input-ready pointer. wl_pointer.enter is sent
# when pointer focus changes; a probe motion that reaches the compositor
# before the client has bound its wl_pointer sets the focus with nobody to
# tell, every later motion stays on the same surface, no enter is ever sent,
# and the client (smithay-client-toolkit) drops motion without an entered
# surface. That was the cursor-ready start flake. The probe also alternates
# between two neighbouring pixels because QEMU drops an absolute move to the
# position it already reported.
while :; do
  if have wayland-ready && have input-ready-pointer; then
    python3 "$root/targets/qemu-aarch64/scripts/qmp.py" --socket "$qmp_socket" move "$((width / 2 + probes % 2))" "$((height / 3))" --width "$width" --height "$height" >/dev/null || true
    probes=$((probes + 1))
  fi
  if have wayland-ready && have input-ready-pointer && have cursor-ready && have home-ready; then
    printf 'QEMU graphical session ready (pid %s, %s s, %s probe moves): %s\n' "$pid" "$(elapsed)" "$probes" "$(grep -m1 -oE '\{"evidence":"[^"]*","event":"home-ready".*"tiles":[0-9]+[^}]*\}' "$serial" || grep -m1 'home-ready' "$serial")"
    exit 0
  fi
  if have wayland-failed; then
    cat "$serial" >&2
    echo "QEMU graphical client failed to connect" >&2
    fail wayland-failed
  fi
  kill -0 "$pid" 2>/dev/null || { cat "$launch_log" >&2; [ ! -f "$serial" ] || cat "$serial" >&2; echo "QEMU exited before graphical readiness" >&2; fail qemu-exited; }
  [ "$(elapsed)" -lt "$timeout" ] || fail timeout
  sleep 0.2
done
