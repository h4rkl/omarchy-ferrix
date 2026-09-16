#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${OMARCHY_OUT_DIR:-"$root/out"}/qemu-aarch64
accel=${OMARCHY_QEMU_ACCEL:-auto}
graphics=${OMARCHY_QEMU_GRAPHICS:-off}
mode=${OMARCHY_QEMU_MODE:-run}
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
set -- qemu-system-aarch64 -machine virt -cpu "$cpu" -m 1024 -smp 2 -accel "$accel" -kernel "$out/Image" -initrd "$out/initramfs.cpio.gz" -append "console=ttyAMA0 panic=-1 omarchy.mode=$mode" -nographic
[ "$graphics" = off ] || set -- "$@" -display "$graphics"
printf 'QEMU command:'; printf ' %s' "$@"; printf '\n'
exec "$@"
