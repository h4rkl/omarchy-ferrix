#!/bin/sh
# Boot the postmarketOS guest built by pmos-build.sh on generic AArch64 QEMU
# `virt` with a mandatory serial console. The pristine rootfs image is never
# written: QEMU boots a qcow2 overlay so manifest hashes stay valid.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64/pmos
# OPT-IN Android runtime image lives beside the default one (pmos-build.sh).
android=${FERRIX_PMOS_ANDROID:-0}
case "$android" in 0|1) ;; *) echo "invalid FERRIX_PMOS_ANDROID: $android (0|1)" >&2; exit 2 ;; esac
[ "$android" != 1 ] || out="$out-android"
accel=${FERRIX_QEMU_ACCEL:-auto}
graphics=${FERRIX_QEMU_GRAPHICS:-off}
mode=${FERRIX_QEMU_MODE:-run}
memory=${FERRIX_PMOS_MEMORY:-2048}
smp=${FERRIX_PMOS_SMP:-2}
ssh_port=${FERRIX_PMOS_SSH_PORT:-}
profile="$root/targets/qemu-aarch64/profile.toml"
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

# Host window metrics for windowed backends: prints "<visible height in
# logical points> <backing scale>" or nothing when the host cannot report them
# (no GUI session, unsupported platform).
host_window_metrics() {
  case "$(uname -s)" in
    Darwin)
      osascript -l JavaScript -e 'ObjC.import("AppKit"); var s = $.NSScreen.mainScreen; s.isNil() ? "" : Math.floor(s.visibleFrame.size.height) + " " + Math.floor(s.backingScaleFactor)' 2>/dev/null || true
      ;;
  esac
}

display_for() {
  case "$1" in
    # The window is fixed at the guest panel size (1 guest pixel = 1 host
    # pixel); QEMU's View menu can still toggle Zoom To Fit for resizing.
    cocoa) printf 'cocoa' ;;
    # Unverified on this host: GTK is only available on Linux hosts.
    gtk) printf 'gtk,zoom-to-fit=on' ;;
    *) printf '%s' "$1" ;;
  esac
}

command -v qemu-system-aarch64 >/dev/null || { echo "missing qemu-system-aarch64; install with: brew install qemu" >&2; exit 2; }
command -v qemu-img >/dev/null || { echo "missing qemu-img (part of the qemu package)" >&2; exit 2; }
[ -f "$profile" ] || { echo "missing QEMU profile: $profile" >&2; exit 2; }
for f in vmlinuz initramfs qemu-aarch64.img cmdline.txt manifest.json; do
  [ -f "$out/$f" ] || { echo "missing pmOS guest artifact $out/$f; run: just pmos-build" >&2; exit 2; }
done
cmdline=$(tr '\n' ' ' < "$out/cmdline.txt" | sed 's/ *$//')
base_sha=$(python3 - "$out/manifest.json" <<'PY'
import json, sys

try:
    manifest = json.load(open(sys.argv[1]))
    value = manifest["artifacts"]["qemu-aarch64.img"]["sha256"]
except (OSError, KeyError, TypeError, ValueError) as error:
    raise SystemExit(f"invalid pmOS manifest image hash: {error}")
if not isinstance(value, str) or len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
    raise SystemExit("invalid pmOS manifest image hash: expected 64 lowercase hex characters")
print(value)
PY
)
# panic=-1 reboots on a kernel panic; in check mode -no-reboot turns that into
# a QEMU exit so a hung guest never waits out the timeout.
append="$cmdline panic=-1 ferrix.mode=$mode"
if [ "$graphics" != off ]; then
  width=$(awk -F= '/^[[:space:]]*width[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
  height=$(awk -F= '/^[[:space:]]*height[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
  case "$width:$height" in
    *[!0-9:]*|:*|*:) echo "invalid display geometry in $profile" >&2; exit 2 ;;
  esac

  # Keep the guest's phone aspect ratio while fitting a windowed backend to
  # the host desktop. Cocoa opens at one guest pixel per host pixel, so on a
  # Retina display the desired guest height is the logical window height
  # multiplied by the backing scale. ferrix-shell reads ferrix.display= and
  # pins the DRM mode, preventing Cocoa from choosing an arbitrary mode.
  gpu_w=$width
  gpu_h=$height
  case "$graphics" in
    none) ;;
    *)
      window_h=${FERRIX_QEMU_WINDOW_HEIGHT:-}
      scale=1
      metrics=$(host_window_metrics)
      if [ -n "$metrics" ]; then
        visible_h=${metrics% *}
        scale=${metrics#* }
        # Leave room for the title bar and a little breathing space.
        [ -n "$window_h" ] || window_h=$(( visible_h - 60 ))
      else
        [ -n "$window_h" ] || window_h=800
      fi
      case "$window_h" in *[!0-9]*|'') echo "invalid FERRIX_QEMU_WINDOW_HEIGHT: $window_h" >&2; exit 2 ;; esac
      case "$scale" in *[!0-9]*|''|0) scale=1 ;; esac
      [ "$window_h" -ge 200 ] || { echo "host window height too small: $window_h points" >&2; exit 2; }
      target_h=$(( window_h * scale ))
      if [ "$height" -gt "$target_h" ]; then
        gpu_h=$(( target_h - target_h % 2 ))
        gpu_w=$(( width * gpu_h / height ))
        gpu_w=$(( gpu_w - gpu_w % 2 ))
      fi
      printf 'host window: %sx%s points at %sx backing scale; guest panel %sx%s\n' "$(( gpu_w / scale ))" "$(( gpu_h / scale ))" "$scale" "$gpu_w" "$gpu_h"
      ;;
  esac
  append="$append ferrix.graphics=on ferrix.display=${gpu_w}x${gpu_h}"
fi

# Copy-on-write overlay. Check mode always boots a fresh overlay (first boot,
# deterministic); run mode keeps its overlay unless FERRIX_PMOS_FRESH=1.
overlay_dir="$out/$mode"
overlay="$overlay_dir/disk.qcow2"
backing_marker="$overlay_dir/backing-image.sha256"
mkdir -p "$overlay_dir"
recreate=0
if [ "$mode" = check ] || [ "${FERRIX_PMOS_FRESH:-0}" = 1 ] || [ ! -f "$overlay" ]; then
  recreate=1
elif [ ! -f "$backing_marker" ] || [ "$(cat "$backing_marker")" != "$base_sha" ]; then
  echo "pmOS backing image changed; refreshing $overlay"
  recreate=1
fi
if [ "$recreate" = 1 ]; then
  rm -f "$overlay" "$backing_marker"
  qemu-img create -q -f qcow2 -b "$out/qemu-aarch64.img" -F raw "$overlay"
  printf '%s\n' "$base_sha" > "$backing_marker"
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
  set -- "$@" -append "$append" -device "virtio-gpu-pci,xres=$gpu_w,yres=$gpu_h" -device virtio-tablet-pci -device virtio-keyboard-pci -display "$(display_for "$graphics")" -serial mon:stdio
fi
printf 'QEMU command:'; printf ' %s' "$@"; printf '\n'
exec "$@"
