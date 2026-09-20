#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
runtime=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64/agent
[ -S "$runtime/qmp.sock" ] || { echo "QEMU agent session is not running; run: just qemu-agent-start" >&2; exit 2; }
[ "$#" -ge 2 ] || { echo "usage: qemu-input.sh tap X Y | swipe X1 Y1 X2 Y2 | key QCODE" >&2; exit 2; }
if [ "$1" = tap ] || [ "$1" = swipe ]; then
  if [ "$1" = tap ]; then
    [ "$#" -eq 3 ] || { echo "usage: qemu-input.sh tap X Y" >&2; exit 2; }
  else
    [ "$#" -eq 5 ] || { echo "usage: qemu-input.sh swipe X1 Y1 X2 Y2" >&2; exit 2; }
  fi
  profile=${FERRIX_QEMU_PROFILE:-"$root/targets/qemu-aarch64/profile.toml"}
  width=$(awk -F= '/^[[:space:]]*width[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
  height=$(awk -F= '/^[[:space:]]*height[[:space:]]*=/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$profile")
  [ -n "$width" ] && [ -n "$height" ] || { echo "unable to read display dimensions from $profile" >&2; exit 2; }
  if [ "$1" = tap ]; then
    exec python3 "$root/targets/qemu-aarch64/scripts/qmp.py" --socket "$runtime/qmp.sock" tap "$2" "$3" --width "$width" --height "$height"
  fi
  exec python3 "$root/targets/qemu-aarch64/scripts/qmp.py" --socket "$runtime/qmp.sock" swipe "$2" "$3" "$4" "$5" --width "$width" --height "$height"
fi
exec python3 "$root/targets/qemu-aarch64/scripts/qmp.py" --socket "$runtime/qmp.sock" "$@"
