#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64
accel=${FERRIX_QEMU_ACCEL:-auto}
graphics=${FERRIX_QEMU_GRAPHICS:-off}
mode=${FERRIX_QEMU_MODE:-run}
agent=${FERRIX_QEMU_AGENT:-0}
profile=${FERRIX_QEMU_PROFILE:-"$root/targets/qemu-aarch64/profile.toml"}
case "$accel" in
  auto) [ "$(uname -s)" = Darwin ] && accel=hvf || accel=tcg ;;
  hvf|kvm|tcg) ;;
  *) echo "invalid FERRIX_QEMU_ACCEL: $accel" >&2; exit 2 ;;
esac
cpu=cortex-a57
case "$accel" in
  hvf|kvm) cpu=host ;;
esac
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
[ -x "$(command -v qemu-system-aarch64)" ] || { echo "missing qemu-system-aarch64; install with: brew install qemu" >&2; exit 2; }
[ -f "$out/Image" ] && [ -f "$out/initramfs.cpio.gz" ] || { echo "missing guest artifacts; run: just qemu-build" >&2; exit 2; }
[ -f "$profile" ] || { echo "missing QEMU profile: $profile" >&2; exit 2; }
set -- qemu-system-aarch64 -machine virt -cpu "$cpu" -m 1024 -smp 2 -accel "$accel" -kernel "$out/Image" -initrd "$out/initramfs.cpio.gz"
# Mock-control channel: a virtio-serial port named ferrix.control, exposed on
# the host as a local Unix socket. ferrix-core reads line-delimited JSON
# commands from it (see scripts/mock_control.py / `just qemu-mock`). Agent
# sessions keep it beside their other runtime files.
if [ "$agent" = 1 ]; then
  control_socket=${FERRIX_QEMU_CONTROL_SOCKET:-"$out/agent/control.sock"}
else
  control_socket=${FERRIX_QEMU_CONTROL_SOCKET:-"$out/control.sock"}
fi
# Per-daemon mock-control channels (ADR-006): ferrix-telephonyd reads
# ferrix.telephony, ferrix-networkd reads ferrix.network. Same virtio-serial
# bus, one host socket each, no cross-daemon forwarding. `just qemu-mock
# --service telephony|network ...` and `ferrix-scenario` drive them.
session_dir=$(dirname "$control_socket")
telephony_socket=${FERRIX_QEMU_TELEPHONY_SOCKET:-"$session_dir/telephony-control.sock"}
network_socket=${FERRIX_QEMU_NETWORK_SOCKET:-"$session_dir/network-control.sock"}
mkdir -p "$session_dir" "$(dirname "$telephony_socket")" "$(dirname "$network_socket")"
rm -f "$control_socket" "$telephony_socket" "$network_socket"
set -- "$@" -device virtio-serial-pci \
  -chardev "socket,id=omctl,path=$control_socket,server=on,wait=off" -device virtserialport,chardev=omctl,name=ferrix.control \
  -chardev "socket,id=omtel,path=$telephony_socket,server=on,wait=off" -device virtserialport,chardev=omtel,name=ferrix.telephony \
  -chardev "socket,id=omnet,path=$network_socket,server=on,wait=off" -device virtserialport,chardev=omnet,name=ferrix.network
printf 'mock-control socket: %s\n' "$control_socket"
printf 'telephony mock-control socket: %s\n' "$telephony_socket"
printf 'network mock-control socket: %s\n' "$network_socket"
if [ "$agent" = 1 ] || [ "$graphics" != off ]; then
  width=$(awk -F= '/^[[:space:]]*width[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
  height=$(awk -F= '/^[[:space:]]*height[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
  case "$width:$height" in
    *[!0-9:]*|:*|*:) echo "invalid display geometry in $profile" >&2; exit 2 ;;
  esac
  # Guest panel geometry. Headless sessions keep the full profile resolution
  # so framebuffer evidence stays at the mock panel geometry. Windowed
  # backends open the host window at the guest size, and the profile's phone
  # panel is taller than a desktop, so the panel is scaled (aspect ratio
  # preserved, never upscaled) to the host's visible screen height in
  # physical pixels: logical points times the backing scale (2 on Retina).
  # The guest pins Weston to exactly this mode via the kernel command line;
  # otherwise QEMU's Cocoa UI pushes its own window size into virtio-gpu and
  # the guest surface ends up at an arbitrary size. Tap coordinates are
  # normalized to the display, so input is unaffected.
  gpu_w=$width
  gpu_h=$height
  case "$graphics" in
    off|none) ;;
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
  gpu="virtio-gpu-pci,xres=$gpu_w,yres=$gpu_h"
  graphics_args="ferrix.graphics=on ferrix.display=${gpu_w}x${gpu_h}"
  # Guest compositor: ferrix-shell (Rust product compositor) or weston
  # (reference). Unset leaves the guest init's default in charge.
  case "${FERRIX_QEMU_SHELL:-}" in
    '') ;;
    ferrix-shell|weston) graphics_args="$graphics_args ferrix.shell=$FERRIX_QEMU_SHELL" ;;
    *) echo "invalid FERRIX_QEMU_SHELL: $FERRIX_QEMU_SHELL (ferrix-shell|weston)" >&2; exit 2 ;;
  esac
fi
if [ "$agent" = 1 ]; then
  runtime="$out/agent"
  qmp_socket=${FERRIX_QEMU_QMP_SOCKET:-"$runtime/qmp.sock"}
  serial_log=${FERRIX_QEMU_SERIAL_LOG:-"$runtime/serial.log"}
  [ "$graphics" = off ] && graphics=none
  mkdir -p "$runtime"
  rm -f "$qmp_socket"
  set -- "$@" -append "console=ttyAMA0 panic=-1 ferrix.mode=$mode $graphics_args" -device "$gpu" -device virtio-tablet-pci -device virtio-keyboard-pci -display "$(display_for "$graphics")" -serial "file:$serial_log" -qmp "unix:$qmp_socket,server=on,wait=off"
  if [ "${FERRIX_QEMU_DAEMONIZE:-0}" = 1 ]; then
    case "$graphics" in
      none|off) ;;
      *) echo "FERRIX_QEMU_DAEMONIZE=1 requires headless graphics (none|off)" >&2; exit 2 ;;
    esac
    set -- "$@" -daemonize -pidfile "${FERRIX_QEMU_PID_FILE:-$runtime/qemu.pid}"
  fi
elif [ "$graphics" = off ]; then
  set -- "$@" -append "console=ttyAMA0 panic=-1 ferrix.mode=$mode" -nographic
else
  set -- "$@" -append "console=ttyAMA0 panic=-1 ferrix.mode=$mode $graphics_args" -device "$gpu" -device virtio-tablet-pci -device virtio-keyboard-pci -display "$(display_for "$graphics")" -serial mon:stdio
fi
printf 'QEMU command:'; printf ' %s' "$@"; printf '\n'
exec "$@"
