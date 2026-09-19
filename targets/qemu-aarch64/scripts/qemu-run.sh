#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${OMARCHY_OUT_DIR:-"$root/out"}/qemu-aarch64
accel=${OMARCHY_QEMU_ACCEL:-auto}
graphics=${OMARCHY_QEMU_GRAPHICS:-off}
mode=${OMARCHY_QEMU_MODE:-run}
agent=${OMARCHY_QEMU_AGENT:-0}
profile=${OMARCHY_QEMU_PROFILE:-"$root/targets/qemu-aarch64/profile.toml"}
case "$accel" in
  auto) [ "$(uname -s)" = Darwin ] && accel=hvf || accel=tcg ;;
  hvf|kvm|tcg) ;;
  *) echo "invalid OMARCHY_QEMU_ACCEL: $accel" >&2; exit 2 ;;
esac
cpu=cortex-a57
case "$accel" in
  hvf|kvm) cpu=host ;;
esac
# Windowed host backends scale the guest's portrait panel to a resizable,
# monitor-sized window; the guest framebuffer keeps the profile resolution.
display_for() {
  case "$1" in
    cocoa) printf 'cocoa,zoom-to-fit=on,zoom-interpolation=on' ;;
    gtk) printf 'gtk,zoom-to-fit=on' ;;
    *) printf '%s' "$1" ;;
  esac
}
[ -x "$(command -v qemu-system-aarch64)" ] || { echo "missing qemu-system-aarch64; install with: brew install qemu" >&2; exit 2; }
[ -f "$out/Image" ] && [ -f "$out/initramfs.cpio.gz" ] || { echo "missing guest artifacts; run: just qemu-build" >&2; exit 2; }
[ -f "$profile" ] || { echo "missing QEMU profile: $profile" >&2; exit 2; }
set -- qemu-system-aarch64 -machine virt -cpu "$cpu" -m 1024 -smp 2 -accel "$accel" -kernel "$out/Image" -initrd "$out/initramfs.cpio.gz"
if [ "$agent" = 1 ] || [ "$graphics" != off ]; then
  width=$(awk -F= '/^[[:space:]]*width[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
  height=$(awk -F= '/^[[:space:]]*height[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
  case "$width:$height" in
    *[!0-9:]*|:*|*:) echo "invalid display geometry in $profile" >&2; exit 2 ;;
  esac
  # On-screen host windows open at the guest resolution, and the profile's
  # phone panel is taller than most monitors. For windowed backends, scale the
  # panel to a monitor-friendly height (aspect ratio preserved). Headless
  # sessions keep the full profile resolution so framebuffer evidence stays
  # at the mock panel geometry. Taps are normalized, so they are unaffected.
  gpu_w=$width
  gpu_h=$height
  case "$graphics" in
    off|none) ;;
    *)
      max_h=${OMARCHY_QEMU_WINDOW_MAX_HEIGHT:-960}
      case "$max_h" in *[!0-9]*|'') echo "invalid OMARCHY_QEMU_WINDOW_MAX_HEIGHT: $max_h" >&2; exit 2 ;; esac
      if [ "$height" -gt "$max_h" ]; then
        gpu_h=$max_h
        gpu_w=$(( width * max_h / height ))
        gpu_w=$(( gpu_w - gpu_w % 2 ))
      fi
      ;;
  esac
  gpu="virtio-gpu-pci,xres=$gpu_w,yres=$gpu_h"
fi
if [ "$agent" = 1 ]; then
  runtime="$out/agent"
  qmp_socket=${OMARCHY_QEMU_QMP_SOCKET:-"$runtime/qmp.sock"}
  serial_log=${OMARCHY_QEMU_SERIAL_LOG:-"$runtime/serial.log"}
  [ "$graphics" = off ] && graphics=none
  mkdir -p "$runtime"
  rm -f "$qmp_socket"
  set -- "$@" -append "console=ttyAMA0 panic=-1 omarchy.mode=$mode omarchy.graphics=on" -device "$gpu" -device virtio-tablet-pci -display "$(display_for "$graphics")" -serial "file:$serial_log" -qmp "unix:$qmp_socket,server=on,wait=off"
elif [ "$graphics" = off ]; then
  set -- "$@" -append "console=ttyAMA0 panic=-1 omarchy.mode=$mode" -nographic
else
  set -- "$@" -append "console=ttyAMA0 panic=-1 omarchy.mode=$mode omarchy.graphics=on" -device "$gpu" -device virtio-tablet-pci -display "$(display_for "$graphics")" -serial mon:stdio
fi
printf 'QEMU command:'; printf ' %s' "$@"; printf '\n'
exec "$@"
