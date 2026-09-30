#!/bin/sh
# Sent over SSH by guest-evidence.py. Read-only, exact tegu guest inventory.
set -eu

model=$(tr '\000' '\n' </proc/device-tree/model 2>/dev/null | sed -n '1p')
compatible=$(tr '\000' '\n' </proc/device-tree/compatible 2>/dev/null)
[ "$model" = 'ZUMA PRO TEGU MP on ZUMA PRO' ] || exit 4
printf '%s\n' "$compatible" | grep -Fxq 'google,ZUMA PRO TEGU' || exit 4

printf 'dt_model=%s\n' "$model"
printf 'dt_compatible=google,ZUMA PRO TEGU\n'
printf 'kernel_release=%s\n' "$(uname -r)"
os_id=$(sed -n 's/^ID=//p' /etc/os-release 2>/dev/null | sed -n '1p' | tr -d '"')
printf 'os_id=%s\n' "${os_id:-unknown}"

slot_cmdline=$(sed -n 's/.*androidboot\.slot_suffix=\(_[ab]\).*/\1/p' /proc/cmdline 2>/dev/null | sed -n '1p')
slot_bootconfig=$(sed -n 's/^[[:space:]]*androidboot\.slot_suffix[[:space:]]*=[[:space:]]*"\{0,1\}\(_[ab]\).*/\1/p' /proc/bootconfig 2>/dev/null | sed -n '1p')
slot=unknown
if [ -n "$slot_cmdline" ] && [ -n "$slot_bootconfig" ] && [ "$slot_cmdline" != "$slot_bootconfig" ]; then
  slot=conflict
elif [ -n "$slot_bootconfig" ]; then
  slot=$slot_bootconfig
elif [ -n "$slot_cmdline" ]; then
  slot=$slot_cmdline
fi
printf 'slot_suffix=%s\n' "$slot"

for service in ferrix-core ferrix-networkd ferrix-telephonyd ferrix-audio ferrix-product; do
  state=unknown
  if command -v rc-service >/dev/null 2>&1; then
    state=stopped
    rc-service "$service" status >/dev/null 2>&1 && state=running
  fi
  printf 'service_%s=%s\n' "$(printf '%s' "$service" | tr '-' '_')" "$state"
done

for item in core:/run/ferrix/core.sock appd:/run/ferrix/ferrix-appd.sock wayland:/run/user/1000/wayland-0; do
  name=${item%%:*}
  path=${item#*:}
  state=absent
  [ -S "$path" ] && state=present
  printf 'socket_%s=%s\n' "$name" "$state"
done

for item in shell:/run/ferrix/ferrix-shell.pid appd:/run/ferrix/ferrix-appd.pid; do
  name=${item%%:*}
  path=${item#*:}
  state=absent
  if [ -r "$path" ]; then
    pid=$(cat "$path" 2>/dev/null || true)
    case "$pid" in
      ''|*[!0-9]*) state=invalid ;;
      *) state=stale; kill -0 "$pid" 2>/dev/null && state=alive ;;
    esac
  fi
  printf 'pid_%s=%s\n' "$name" "$state"
done

drm_connected=0
drm_modes=0
for status_path in /sys/class/drm/card*-*/status; do
  [ -r "$status_path" ] || continue
  [ "$(cat "$status_path" 2>/dev/null)" = connected ] || continue
  drm_connected=$((drm_connected + 1))
  modes_path=${status_path%/status}/modes
  if [ -r "$modes_path" ]; then
    count=$(wc -l < "$modes_path" | tr -d ' ')
    drm_modes=$((drm_modes + count))
  fi
done
printf 'drm_connected_count=%s\ndrm_mode_count=%s\n' "$drm_connected" "$drm_modes"

framebuffers=0
for device in /sys/class/graphics/fb[0-9]*; do
  [ -e "$device" ] && framebuffers=$((framebuffers + 1))
done
printf 'framebuffer_count=%s\n' "$framebuffers"

input_events=0
touch_candidates=0
for event in /sys/class/input/event[0-9]*; do
  [ -e "$event" ] || continue
  input_events=$((input_events + 1))
  name=$(cat "$event/device/name" 2>/dev/null || true)
  if printf '%s\n' "$name" | grep -Eiq 'touch|synaptics|goodix'; then
    touch_candidates=$((touch_candidates + 1))
  fi
done
printf 'input_event_count=%s\ntouch_candidate_count=%s\n' "$input_events" "$touch_candidates"
