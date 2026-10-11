#!/bin/sh
# Sent over the CDC-ACM shell (tools/pixel-9a/acm-probe.py, piped to the guest
# `sh`) by tools/pixel-9a/audio-inventory.sh. Read-only audio inventory of the
# exact tegu guest. The device-tree gate runs before any audio probe, inside
# the same shell session that collects the inventory.
#
# This probe only reads /proc, /sys, /dev listings, logs, and query-only
# command output. It never opens a PCM, changes a mixer control or the default
# sink, loads modules, mounts filesystems, writes sysfs, or starts services.
# Playback, capture, and mixer tools are only checked with `command -v`.
#
# Output: `key=value` summary lines plus raw sections between
# `## begin <name>` and `## end <name> exit=<n>`, every raw line prefixed with
# `| ` so it can never be parsed as a summary field.
set -u

expected_model='ZUMA PRO TEGU MP on ZUMA PRO'
expected_compatible='google,ZUMA PRO TEGU'

model=$(tr '\000' '\n' 2>/dev/null </proc/device-tree/model | sed -n '1p')
compatible=$(tr '\000' '\n' 2>/dev/null </proc/device-tree/compatible)
if printf '%s\n%s\n' "$model" "$compatible" | grep -Fxq 'linux,dummy-virt'; then
  echo identity=refused-qemu
  exit 5
fi
if [ "$model" != "$expected_model" ] ||
  ! printf '%s\n' "$compatible" | grep -Fxq "$expected_compatible"; then
  echo identity=mismatch
  printf 'observed_dt_model=%s\n' "${model:-unavailable}"
  exit 4
fi

echo probe=read-only
echo probe_version=1
echo identity=verified
printf 'dt_model=%s\n' "$model"
printf 'dt_compatible=%s\n' "$expected_compatible"
printf 'kernel_release=%s\n' "$(uname -r)"
os_id=$(sed -n 's/^ID=//p' /etc/os-release 2>/dev/null | sed -n '1p' | tr -d '"')
printf 'os_id=%s\n' "${os_id:-unknown}"

have() { command -v "$1" >/dev/null 2>&1; }

# Bound every query so a wedged daemon cannot hang the shell session.
run_t() {
  if have timeout; then timeout 10 "$@"; else "$@"; fi
}

raw() {
  raw_name=$1
  shift
  raw_out=$("$@" 2>&1)
  raw_rc=$?
  printf '## begin %s\n' "$raw_name"
  [ -z "$raw_out" ] || printf '%s\n' "$raw_out" | sed 's/^/| /'
  printf '## end %s exit=%s\n' "$raw_name" "$raw_rc"
}

raw_text() {
  printf '## begin %s\n' "$1"
  [ -z "$2" ] || printf '%s\n' "$2" | sed 's/^/| /'
  printf '## end %s exit=0\n' "$1"
}

count_lines() {
  if [ -z "$1" ]; then echo 0; else printf '%s\n' "$1" | wc -l | tr -d ' '; fi
}

uid=$(id -u)
priv=none
if [ "$uid" = 0 ]; then
  priv=root
elif have doas && doas -n true >/dev/null 2>&1; then
  priv=doas
elif have sudo && sudo -n true >/dev/null 2>&1; then
  priv=sudo
fi
printf 'uid=%s\npriv=%s\n' "$uid" "$priv"

as_root() {
  case "$priv" in
    root) "$@" ;;
    doas) doas -n "$@" ;;
    sudo) sudo -n "$@" ;;
    *) return 126 ;;
  esac
}

# --- tool availability (never executed) ------------------------------------

audio_tools='aplay arecord amixer alsactl alsaucm speaker-test wpctl pw-cli pw-dump pactl pipewire wireplumber pipewire-pulse socat'
for tool in $audio_tools; do
  key=$(printf '%s' "$tool" | tr '-' '_')
  if have "$tool"; then echo "tool_$key=present"; else echo "tool_$key=unavailable"; fi
done

# --- ALSA ------------------------------------------------------------------

if [ -r /proc/asound/cards ]; then
  echo alsa_proc=present
  cards_text=$(cat /proc/asound/cards 2>/dev/null)
  printf 'alsa_cards=%s\n' "$(printf '%s\n' "$cards_text" | grep -cE '^ *[0-9]+ \[')"
  raw_text proc-asound-cards "$cards_text"
else
  echo alsa_proc=absent
  echo alsa_cards=unknown
fi
if [ -r /proc/asound/pcm ]; then
  pcm_text=$(cat /proc/asound/pcm 2>/dev/null)
  printf 'alsa_pcm_total=%s\n' "$(count_lines "$pcm_text")"
  printf 'alsa_pcm_playback=%s\n' "$(printf '%s\n' "$pcm_text" | grep -c 'playback')"
  printf 'alsa_pcm_capture=%s\n' "$(printf '%s\n' "$pcm_text" | grep -c 'capture')"
  raw_text proc-asound-pcm "$pcm_text"
else
  echo alsa_pcm_total=unknown
  echo alsa_pcm_playback=unknown
  echo alsa_pcm_capture=unknown
fi
[ ! -r /proc/asound/devices ] || raw proc-asound-devices cat /proc/asound/devices
if [ -d /dev/snd ]; then
  snd_nodes=$(ls /dev/snd 2>/dev/null)
  printf 'dev_snd_nodes=%s\n' "$(count_lines "$snd_nodes")"
  printf 'dev_snd_control=%s\n' "$(printf '%s\n' "$snd_nodes" | grep -c '^controlC')"
  printf 'dev_snd_pcm_playback=%s\n' "$(printf '%s\n' "$snd_nodes" | grep -c '^pcmC.*p$')"
  printf 'dev_snd_pcm_capture=%s\n' "$(printf '%s\n' "$snd_nodes" | grep -c '^pcmC.*c$')"
  raw_text dev-snd "$snd_nodes"
else
  echo dev_snd_nodes=absent
fi

# --- kernel: config, modules, device-tree bindings, logs --------------------

audio_name_re='snd|asoc|alsa|aoc|audio|sound|codec|cs35l|cs40l|tas2|max98'
if [ -r /proc/config.gz ] && have zcat; then
  snd_config=$(zcat /proc/config.gz 2>/dev/null | grep -E '^CONFIG_SND(_SOC)?=' | tr '\n' ' ')
  printf 'kernel_config_snd=%s\n' "${snd_config:-not-set}"
else
  echo kernel_config_snd=unknown
fi
if [ -r /proc/modules ]; then
  audio_modules=$(awk '{ print $1 }' /proc/modules | grep -iE "$audio_name_re")
  printf 'audio_modules=%s\n' "$(count_lines "$audio_modules")"
  raw_text audio-modules "$audio_modules"
else
  echo audio_modules=unknown
fi

dt_nodes=0
dt_bound=0
dt_unbound=0
dt_lines=""
for entry in /sys/bus/platform/devices/* /sys/bus/i2c/devices/* /sys/bus/spi/devices/*; do
  [ -r "$entry/of_node/compatible" ] || continue
  node_compatible=$(tr '\000' ' ' <"$entry/of_node/compatible" 2>/dev/null)
  printf '%s\n' "$node_compatible" | grep -iqE "$audio_name_re" || continue
  dt_nodes=$((dt_nodes + 1))
  if [ -L "$entry/driver" ]; then
    dt_bound=$((dt_bound + 1))
    state=bound
  else
    dt_unbound=$((dt_unbound + 1))
    state=unbound
  fi
  dt_lines="$dt_lines$state ${entry##*/} $node_compatible
"
done
printf 'audio_dt_nodes=%s\naudio_dt_bound=%s\naudio_dt_unbound=%s\n' "$dt_nodes" "$dt_bound" "$dt_unbound"
raw_text audio-dt-nodes "$(printf '%s' "$dt_lines" | sed '/^$/d')"
sound_class=0
for entry in /sys/class/sound/*; do
  [ -e "$entry" ] && sound_class=$((sound_class + 1))
done
printf 'sys_class_sound=%s\n' "$sound_class"

dmesg_access=unavailable
dmesg_text=""
if have dmesg; then
  if dmesg_text=$(dmesg 2>/dev/null) && [ -n "$dmesg_text" ]; then
    dmesg_access=ok
  elif dmesg_text=$(as_root dmesg 2>/dev/null) && [ -n "$dmesg_text" ]; then
    dmesg_access=ok-privileged
  else
    dmesg_access=denied
    dmesg_text=""
  fi
fi
printf 'dmesg_access=%s\n' "$dmesg_access"
if [ -n "$dmesg_text" ]; then
  dmesg_audio=$(printf '%s\n' "$dmesg_text" | grep -iE "$audio_name_re")
  dmesg_failures=$(printf '%s\n' "$dmesg_audio" | grep -iE 'fail|error|defer|timed? ?out|not found|-517|-110|-22|-19|-2$')
  firmware_failures=$(printf '%s\n' "$dmesg_text" | grep -iE 'firmware' | grep -iE "$audio_name_re" | grep -iE 'fail|not found|error|-2$')
  printf 'dmesg_audio_lines=%s\n' "$(count_lines "$dmesg_audio")"
  printf 'dmesg_audio_failure_lines=%s\n' "$(count_lines "$dmesg_failures")"
  printf 'dmesg_audio_firmware_failures=%s\n' "$(count_lines "$firmware_failures")"
  raw_text dmesg-audio "$(printf '%s\n' "$dmesg_audio" | tail -n 80)"
else
  echo dmesg_audio_lines=unknown
  echo dmesg_audio_failure_lines=unknown
  echo dmesg_audio_firmware_failures=unknown
fi

# Only an already-mounted debugfs is read; this probe never mounts anything.
deferred_text=$(as_root cat /sys/kernel/debug/devices_deferred 2>/dev/null)
if [ "$?" -eq 0 ]; then
  deferred_audio=$(printf '%s\n' "$deferred_text" | grep -iE "$audio_name_re")
  printf 'deferred_audio_devices=%s\n' "$(count_lines "$deferred_audio")"
  raw_text devices-deferred-audio "$deferred_audio"
else
  echo deferred_audio_devices=unknown
fi

firmware_files=""
for dir in /lib/firmware /vendor/firmware; do
  [ -d "$dir" ] || continue
  found=$(find "$dir" -maxdepth 2 \( -iname '*aoc*' -o -iname '*audio*' -o -iname '*cs35l*' -o -iname '*cs40l*' -o -iname '*dsp*' \) 2>/dev/null | head -n 40)
  [ -z "$found" ] || firmware_files="$firmware_files$found
"
done
firmware_files=$(printf '%s' "$firmware_files" | sed '/^$/d')
printf 'audio_firmware_files=%s\n' "$(count_lines "$firmware_files")"
raw_text audio-firmware-files "$firmware_files"

# --- UCM -------------------------------------------------------------------

ucm_dir=unavailable
for dir in /usr/share/alsa/ucm2 /usr/share/alsa/ucm; do
  if [ -d "$dir" ]; then ucm_dir=$dir; break; fi
done
printf 'ucm_dir=%s\n' "$ucm_dir"
if [ "$ucm_dir" != unavailable ]; then
  printf 'ucm_conf_files=%s\n' "$(find "$ucm_dir" -name '*.conf' 2>/dev/null | wc -l | tr -d ' ')"
  matched=0
  drivers=$(sed -n 's/^ *[0-9][0-9]* \[[^]]*\]: \([^ ]*\) - .*/\1/p' /proc/asound/cards 2>/dev/null)
  for driver in $drivers; do
    if [ -e "$ucm_dir/conf.d/$driver" ] || [ -e "$ucm_dir/$driver" ]; then
      matched=$((matched + 1))
    fi
  done
  printf 'ucm_card_drivers=%s\nucm_matched_card_profiles=%s\n' "$(count_lines "$drivers")" "$matched"
else
  echo ucm_conf_files=unknown
  echo ucm_matched_card_profiles=unknown
fi
if have alsaucm; then
  raw alsaucm-listcards run_t alsaucm listcards
  printf 'alsaucm_listcards_exit=%s\n' "$raw_rc"
else
  echo alsaucm_listcards_exit=unavailable
fi

# --- PipeWire / WirePlumber graph -------------------------------------------

# The Ferrix session (ferrix-pipewire/ferrix-audio) runs as uid 1000.
if [ "$uid" = 0 ]; then runtime_dir=/run/user/1000; else runtime_dir=/run/user/$uid; fi
if [ -S "$runtime_dir/pipewire-0" ]; then echo pipewire_socket=present; else echo pipewire_socket=absent; fi
if [ -S "$runtime_dir/pulse/native" ]; then echo pulse_socket=present; else echo pulse_socket=absent; fi
for process in pipewire wireplumber pipewire-pulse; do
  key=$(printf '%s' "$process" | tr '-' '_')
  if ! have pgrep; then
    echo "process_$key=unknown"
  elif pgrep -x "$process" >/dev/null 2>&1; then
    echo "process_$key=running"
  else
    echo "process_$key=absent"
  fi
done

if have wpctl && [ -S "$runtime_dir/pipewire-0" ]; then
  raw wpctl-status run_t env XDG_RUNTIME_DIR="$runtime_dir" wpctl status
  if [ "$raw_rc" -eq 0 ]; then
    echo pipewire_graph=reachable
    printf '%s\n' "$raw_out" | awk '
      /^Audio/ { audio = 1; next }
      /^[A-Z]/ { audio = 0 }
      audio && /Devices:/ { section = "devices"; next }
      audio && /Sinks:/ { section = "sinks"; next }
      audio && /Sources:/ { section = "sources"; next }
      audio && /[A-Za-z]+:[[:space:]]*$/ { section = ""; next }
      audio && /[0-9]+\./ {
        count[section]++
        if (section == "sinks" && /\*/) default_sink = 1
        if (section == "sources" && /\*/) default_source = 1
      }
      END {
        printf "graph_devices=%d\ngraph_sinks=%d\ngraph_sources=%d\n", count["devices"], count["sinks"], count["sources"]
        printf "graph_default_sink=%s\ngraph_default_source=%s\n", default_sink ? "marked" : "none", default_source ? "marked" : "none"
      }'
    raw wpctl-list-audio-sinks run_t env XDG_RUNTIME_DIR="$runtime_dir" wpctl list audio sinks
    printf 'adapter_list_sinks_exit=%s\n' "$raw_rc"
    raw wpctl-inspect-default-sink run_t env XDG_RUNTIME_DIR="$runtime_dir" wpctl inspect @DEFAULT_AUDIO_SINK@
    raw wpctl-inspect-default-source run_t env XDG_RUNTIME_DIR="$runtime_dir" wpctl inspect @DEFAULT_AUDIO_SOURCE@
  else
    echo pipewire_graph=unreachable
  fi
elif have wpctl; then
  echo pipewire_graph=socket-absent
else
  echo pipewire_graph=wpctl-unavailable
fi
if have pactl && [ -S "$runtime_dir/pulse/native" ]; then
  raw pactl-info run_t env XDG_RUNTIME_DIR="$runtime_dir" pactl info
  raw pactl-short-sinks run_t env XDG_RUNTIME_DIR="$runtime_dir" pactl list short sinks
  raw pactl-short-sources run_t env XDG_RUNTIME_DIR="$runtime_dir" pactl list short sources
fi

# --- Ferrix audio adapter ----------------------------------------------------

backend=$(sed -n 's/^FERRIX_AUDIO_BACKEND=//p' /etc/ferrix/backends.env 2>/dev/null | sed -n '1p')
printf 'ferrix_audio_backend=%s\n' "${backend:-unknown}"
for service in ferrix-pipewire ferrix-audio ferrix-core; do
  key=$(printf '%s' "$service" | tr '-' '_')
  if have rc-service; then
    rc-service "$service" status >/dev/null 2>&1
    case "$?" in
      0) echo "service_$key=started" ;;
      3) echo "service_$key=stopped" ;;
      *) echo "service_$key=not-started" ;;
    esac
  else
    echo "service_$key=unknown"
  fi
done
if [ -S /run/ferrix/core.sock ]; then
  echo core_socket=present
  if have socat; then
    raw ferrix-core-state sh -c 'printf "{\"request\":\"state\"}\n" | timeout 5 socat -t 2 - UNIX-CONNECT:/run/ferrix/core.sock'
    case "$raw_out" in
      *'"reply":"state"'*) echo core_state=observed ;;
      *) echo core_state=not-observed ;;
    esac
    capability=$(printf '%s\n' "$raw_out" | grep -o '"audio":"[a-z-]*"' | sed -n '1p' | cut -d'"' -f4)
    route=$(printf '%s\n' "$raw_out" | grep -o '"audio":{"route":"[a-z-]*"' | sed -n '1p' | cut -d'"' -f6)
    printf 'core_audio_capability=%s\ncore_audio_route=%s\n' "${capability:-absent}" "${route:-absent}"
  else
    echo core_state=socket-client-unavailable
  fi
else
  echo core_socket=absent
  echo core_state=socket-absent
fi
log_text=$(as_root sh -c 'grep -hiE "audio|wireplumber|pipewire" /var/log/ferrix-core.log 2>/dev/null | tail -n 20')
raw_text ferrix-core-audio-log "$log_text"
echo probe_complete=yes
