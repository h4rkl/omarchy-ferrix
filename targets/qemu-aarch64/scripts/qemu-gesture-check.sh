#!/bin/sh
# Bounded QEMU/mock check of the product navigation gestures through the real
# compositor, application clients, ferrix-appd lifecycle, and framebuffer.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64
runtime="$out/agent"
serial="$runtime/serial.log"
profile=${FERRIX_QEMU_PROFILE:-"$root/targets/qemu-aarch64/profile.toml"}
evidence="$out/gesture-check"
timeout=${FERRIX_QEMU_GESTURE_TIMEOUT:-90}
case "$timeout" in
  ""|*[!0-9]*|0) echo "invalid FERRIX_QEMU_GESTURE_TIMEOUT: $timeout" >&2; exit 2 ;;
esac

start=$(date +%s)
elapsed() { echo $(( $(date +%s) - start )); }
started=0
cleanup() {
  [ "$started" = 0 ] || "$root/targets/qemu-aarch64/scripts/qemu-agent-stop.sh" >/dev/null 2>&1 || true
}
trap cleanup 0 1 2 15
fail() {
  printf '{"evidence":"QEMU","event":"gesture-check-failed","reason":"%s","seconds":%s,"serial_log":"%s"}\n' "$1" "$(elapsed)" "$serial" >&2
  [ ! -f "$serial" ] || tail -120 "$serial" >&2
  exit 1
}
wait_after() {
  line=$1
  pattern=$2
  label=$3
  while ! tail -n "+$line" "$serial" 2>/dev/null | grep -q -- "$pattern"; do
    [ -f "$runtime/qemu.pid" ] && kill -0 "$(cat "$runtime/qemu.pid")" 2>/dev/null || fail "qemu-exited-before-$label"
    [ "$(elapsed)" -lt "$timeout" ] || fail "timeout-waiting-for-$label"
    sleep 0.2
  done
}
mark() { echo $(( $(wc -l < "$serial") + 1 )); }
input="$root/targets/qemu-aarch64/scripts/qemu-input.sh"

[ -f "$out/Image" ] && [ -f "$out/initramfs.cpio.gz" ] || fail "missing-artifacts-run-qemu-build"
if [ -f "$runtime/qemu.pid" ] && kill -0 "$(cat "$runtime/qemu.pid")" 2>/dev/null; then
  fail "agent-session-already-running"
fi
width=$(awk -F= '/^[[:space:]]*width[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
height=$(awk -F= '/^[[:space:]]*height[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
case "$width:$height" in *[!0-9:]*|:*|*:) fail "invalid-profile-geometry" ;; esac

# Proportional points land on Messages and inside the compositor gesture zones
# for the versioned portrait profile while remaining valid for scaled profiles.
message_x=$((width * 77 / 100))
message_y=$((height * 53 / 100))
center_x=$((width / 2))
bottom_y=$((height * 98 / 100))
home_y=$((height * 81 / 100))
close_y=$((height * 64 / 100))
right_x=$((width * 98 / 100))
inward_x=$((width * 65 / 100))
left_x=$((width * 2 / 100))
left_inward_x=$((width * 35 / 100))
side_y=$((height * 38 / 100))

started=1
FERRIX_QEMU_ACCEL=tcg FERRIX_QEMU_GRAPHICS=none \
  "$root/targets/qemu-aarch64/scripts/qemu-agent-start.sh" >/dev/null || fail "agent-session-start-failed"

line=$(mark)
"$input" tap "$message_x" "$message_y"
wait_after "$line" '"event":"app-foreground","app":"org.ferrix.messages"' messages-launch

# Ordinary bottom swipe: Home is raised and Messages remains alive.
line=$(mark)
"$input" swipe "$center_x" "$bottom_y" "$center_x" "$home_y"
wait_after "$line" '"event":"home-gesture".*"kind":"swipe"' home-gesture
wait_after "$line" '"event":"app-background","app":"org.ferrix.messages"' messages-background
wait_after "$line" '"event":"app-foreground","app":"org.ferrix.home"' home-foreground
if tail -n "+$line" "$serial" | grep -q '"event":"app-terminating","app":"org.ferrix.messages"'; then
  fail "minimize-terminated-messages"
fi

# Resume the same instance, then yield from the iOS-style left edge.
line=$(mark)
"$input" tap "$message_x" "$message_y"
wait_after "$line" '"event":"app-foreground","app":"org.ferrix.messages"' messages-resume
line=$(mark)
"$input" swipe "$left_x" "$side_y" "$left_inward_x" "$side_y"
wait_after "$line" '"event":"messages-back","status":"yielded"' left-edge-yield
wait_after "$line" '"event":"app-foreground","app":"org.ferrix.home"' home-after-left-yield

# Resume again and verify Android's symmetric right-edge Back path.
line=$(mark)
"$input" tap "$message_x" "$message_y"
wait_after "$line" '"event":"app-foreground","app":"org.ferrix.messages"' messages-after-left-yield
line=$(mark)
"$input" swipe "$right_x" "$side_y" "$inward_x" "$side_y"
wait_after "$line" '"event":"messages-back","status":"yielded"' right-edge-yield
wait_after "$line" '"event":"app-foreground","app":"org.ferrix.home"' home-after-right-yield

# Resume once more, then dismiss with the deliberately long bottom swipe.
line=$(mark)
"$input" tap "$message_x" "$message_y"
wait_after "$line" '"event":"app-foreground","app":"org.ferrix.messages"' messages-second-resume
line=$(mark)
"$input" swipe "$center_x" "$bottom_y" "$center_x" "$close_y"
wait_after "$line" '"event":"close-gesture"' close-gesture
wait_after "$line" '"event":"app-terminating","app":"org.ferrix.messages"' messages-terminating
wait_after "$line" '"event":"app-exited","app":"org.ferrix.messages"' messages-exited
wait_after "$line" '"event":"app-foreground","app":"org.ferrix.home"' home-after-close

"$root/targets/qemu-aarch64/scripts/qemu-frame.sh" >/dev/null || fail "final-framebuffer-invalid"
mkdir -p "$evidence"
cp "$serial" "$evidence/serial.log"
cp "$runtime/frame.png" "$evidence/home-after-close.png"
"$root/targets/qemu-aarch64/scripts/qemu-agent-stop.sh" >/dev/null 2>&1 || true
started=0
printf '{"evidence":"QEMU","event":"gesture-check-passed","minimize":true,"left_edge_yield":true,"right_edge_yield":true,"close":true,"seconds":%s,"evidence_dir":"%s"}\n' "$(elapsed)" "$evidence"
