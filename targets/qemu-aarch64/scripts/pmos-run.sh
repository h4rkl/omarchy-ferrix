#!/bin/sh
# Boot the postmarketOS guest built by pmos-build.sh on generic AArch64 QEMU
# `virt` with a mandatory serial console. The pristine rootfs image is never
# written: QEMU boots a qcow2 overlay so manifest hashes stay valid.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64/pmos
accel=${FERRIX_QEMU_ACCEL:-auto}
graphics=${FERRIX_QEMU_GRAPHICS:-off}
mode=${FERRIX_QEMU_MODE:-run}
memory=${FERRIX_PMOS_MEMORY:-2048}
smp=${FERRIX_PMOS_SMP:-2}
ssh_port=${FERRIX_PMOS_SSH_PORT:-}
case "$accel" in
  auto) [ "$(uname -s)" = Darwin ] && accel=hvf || accel=tcg ;;
  hvf|kvm|tcg) ;;
  *) echo "invalid FERRIX_QEMU_ACCEL: $accel" >&2; exit 2 ;;
esac
case "$mode" in
  run|check) ;;
  *) echo "invalid FERRIX_QEMU_MODE: $mode" >&2; exit 2 ;;
esac
cpu=cortex-a57
case "$accel" in hvf|kvm) cpu=host ;; esac
command -v qemu-system-aarch64 >/dev/null || { echo "missing qemu-system-aarch64; install with: brew install qemu" >&2; exit 2; }
command -v qemu-img >/dev/null || { echo "missing qemu-img (part of the qemu package)" >&2; exit 2; }
for f in vmlinuz initramfs qemu-aarch64.img cmdline.txt; do
  [ -f "$out/$f" ] || { echo "missing pmOS guest artifact $out/$f; run: just pmos-build" >&2; exit 2; }
done
cmdline=$(tr '\n' ' ' < "$out/cmdline.txt" | sed 's/ *$//')
# panic=-1 reboots on a kernel panic; in check mode -no-reboot turns that into
# a QEMU exit so a hung guest never waits out the timeout.
append="$cmdline panic=-1 ferrix.mode=$mode"
[ "$graphics" = off ] || append="$append ferrix.graphics=on"

# Copy-on-write overlay. Check mode always boots a fresh overlay (first boot,
# deterministic); run mode keeps its overlay unless FERRIX_PMOS_FRESH=1.
overlay_dir="$out/$mode"
overlay="$overlay_dir/disk.qcow2"
mkdir -p "$overlay_dir"
if [ "$mode" = check ] || [ "${FERRIX_PMOS_FRESH:-0}" = 1 ] || [ ! -f "$overlay" ]; then
  rm -f "$overlay"
  qemu-img create -q -f qcow2 -b "$out/qemu-aarch64.img" -F raw "$overlay"
fi

netdev="user,id=net"
[ -n "$ssh_port" ] && netdev="$netdev,hostfwd=tcp:127.0.0.1:$ssh_port-:22"
set -- qemu-system-aarch64 -machine virt -cpu "$cpu" -m "$memory" -smp "$smp" -accel "$accel" \
  -kernel "$out/vmlinuz" -initrd "$out/initramfs" \
  -drive "file=$overlay,format=qcow2,if=virtio" \
  -netdev "$netdev" -device virtio-net-pci,netdev=net \
  -device virtio-rng-pci
[ "$mode" = check ] && set -- "$@" -no-reboot
if [ "$graphics" = off ]; then
  set -- "$@" -append "$append" -nographic
else
  set -- "$@" -append "$append" -device virtio-gpu-pci -device virtio-tablet-pci -device virtio-keyboard-pci -display "$graphics" -serial mon:stdio
fi
printf 'QEMU command:'; printf ' %s' "$@"; printf '\n'
exec "$@"
