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
  gpu="virtio-gpu-pci,xres=$width,yres=$height"
fi
if [ "$agent" = 1 ]; then
  runtime="$out/agent"
  qmp_socket=${OMARCHY_QEMU_QMP_SOCKET:-"$runtime/qmp.sock"}
  serial_log=${OMARCHY_QEMU_SERIAL_LOG:-"$runtime/serial.log"}
  [ "$graphics" = off ] && graphics=none
  mkdir -p "$runtime"
  rm -f "$qmp_socket"
  set -- "$@" -append "console=ttyAMA0 panic=-1 omarchy.mode=$mode omarchy.graphics=on" -device "$gpu" -device virtio-tablet-device -display "$graphics" -serial "file:$serial_log" -qmp "unix:$qmp_socket,server=on,wait=off"
elif [ "$graphics" = off ]; then
  set -- "$@" -append "console=ttyAMA0 panic=-1 omarchy.mode=$mode" -nographic
else
  set -- "$@" -append "console=ttyAMA0 panic=-1 omarchy.mode=$mode omarchy.graphics=on" -device "$gpu" -device virtio-tablet-device -display "$graphics" -serial mon:stdio
fi
printf 'QEMU command:'; printf ' %s' "$@"; printf '\n'
exec "$@"
