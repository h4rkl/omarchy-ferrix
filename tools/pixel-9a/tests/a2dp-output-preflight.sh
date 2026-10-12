#!/bin/sh
# Offline proof for the issue #91 Bluetooth A2DP output preflight. No device
# is contacted, no audio is played or recorded, and no real BlueZ, PipeWire,
# or mixer exists. Three layers:
#
#   1. the exact guest probe, with only its path/root constants redirected
#      into a fake procfs/device tree/runtime dir/apk database, run by the
#      host /bin/sh against fake bluetoothctl, wpctl, amixer, rc-service and
#      pw-cat that log every call and refuse any verb outside the reviewed
#      query set;
#   2. the host workflow over the real CDC-ACM transport and a fake guest
#      (acm-fake-guest.sh) whose final `sh` answers with layer-1 output or a
#      variant of it;
#   3. end to end: host workflow, transport, fake guest running the
#      redirected probe, fake tools.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
host="$root/tools/pixel-9a/a2dp-output-preflight.sh"
probe="$root/tools/pixel-9a/a2dp-output-preflight-remote.sh"
target_id=google-pixel-9a-tegu-gxq96-mp1-0
fails=0
command -v python3 >/dev/null 2>&1 || { echo 'python3 is required for the CDC-ACM transport test' >&2; exit 1; }
status_before=$(cksum <"$root/docs/status.md")
plan_before=$(cksum <"$root/plans/pixel-9a-daily-phone.md")
capabilities_before=$(cksum <"$root/targets/boards/$target_id/capabilities.toml")

check() {
	name=$1
	shift
	if "$@"; then
		printf 'ok   %s\n' "$name"
	else
		printf 'FAIL %s\n' "$name"
		fails=$((fails + 1))
	fi
}

# --- static policy ---------------------------------------------------------------
check "static: host POSIX syntax" sh -n "$host"
check "static: probe POSIX syntax" sh -n "$probe"
code=$(grep -vE '^[[:space:]]*#' "$probe")
host_code=$(grep -vE '^[[:space:]]*(#|printf )' "$host")
verbs='adb|fastboot|ssh|scp|curl|dd|mount|umount|flash|erase|reboot|modprobe|insmod|rmmod|rc-service|rc-update|amixer|aplay|arecord|bluetoothctl|wpctl|pw-cat|pw-play|paplay|afplay'
offending=$(printf '%s\n' "$host_code" | grep -nE "(^|[;|&(]|then|do|else)[[:space:]]*($verbs)([[:space:]]|$)" || true)
check "static: the host runs no Android, storage, network, service, Bluetooth, or audio command" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
offending=$(printf '%s\n' "$code" | grep -nwE 'aplay|arecord|speaker-test|pw-play|pw-record|paplay|parecord|pactl|alsactl|alsaucm|pw-cli|pw-link|pw-metadata|modprobe|insmod|rmmod|mount|umount|dd|btmgmt|hciconfig|rfkill' || true)
check "static: the probe never plays, records, links, or touches modules, mounts, or radios" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
check "static: no playback, record, or media flag is passed to pw-cat" \
	sh -c '! printf "%s\n" "$1" | grep -E "pw-cat .*(-p|--playback|-r|--record|--target|--properties [^)])" | grep -v "grep -Eq"' sh "$code"
check "static: pw-cat is only ever run with --help" \
	sh -c '[ "$(printf "%s\n" "$1" | grep -E "run_t [0-9]+ pw-cat" | grep -vc "run_t 5 pw-cat --help")" -eq 0 ] &&
		[ "$(printf "%s\n" "$1" | grep -c "run_t 5 pw-cat --help")" -eq 1 ]' sh "$code"
bt_calls=$(printf '%s\n' "$code" | grep -oE 'bluetoothctl [a-z]+( [A-Z][a-z]+)?' | grep -v '^bluetoothctl wpctl$' | sort -u | tr '\n' '|')
bt_lines=$(printf '%s\n' "$code" | grep bluetoothctl | grep -vE 'for tool in bluetoothctl |have bluetoothctl;')
check "static: bluetoothctl is only list, show, devices Paired, and info" \
	sh -c '[ "$1" = "bluetoothctl devices Paired|bluetoothctl info|bluetoothctl list|bluetoothctl show|" ] &&
		[ "$(printf "%s\n" "$2" | grep -c .)" -eq 4 ] &&
		[ "$(printf "%s\n" "$2" | grep -cE "run_t 10 bluetoothctl (list|show|devices Paired|info) ")" -eq 4 ]' sh "$bt_calls" "$bt_lines"
wp_calls=$(printf '%s\n' "$code" | grep -oE '(wp|wpctl) (status|inspect|set-[a-z]+|settings|clear-default|load|edit)' | sort -u | tr '\n' '|')
check "static: wpctl is only status and inspect" [ "$wp_calls" = 'wp inspect|wp status|' ]
check "static: amixer is only cget, and only on the two watched controls" \
	sh -c '! printf "%s\n" "$1" | grep -E "amixer.*(cset|sset|set |-s )" &&
		[ "$(printf "%s\n" "$1" | grep -c "amixer -c")" -eq 1 ] && printf "%s\n" "$1" | grep -Fq "amixer -c \"\$card\" cget" &&
		grep -Fxq "watched_controls='"'"'TDM_0_RX Mixer EP1|TAS25XX ASI Switch'"'"'" "$2"' sh "$code" "$probe"
check "static: rc-service is only queried with status" \
	sh -c '[ "$(printf "%s\n" "$1" | grep "rc-service \"" | grep -vc "rc-service \"\$service\" status ")" -eq 0 ]' sh "$code"
check "static: the probe creates, removes, and redirects into no file" \
	sh -c '! printf "%s\n" "$1" | grep -E ">[[:space:]]*[^&[:space:]=]" | grep -v ">/dev/null" | grep . &&
		! printf "%s\n" "$1" | grep -nE "mkdir|rm -|touch |tee |mv |cp |ln -|sed -i"' sh "$code"
check "static: the probe gates on the exact tegu device tree" \
	sh -c 'grep -Fq "ZUMA PRO TEGU MP on ZUMA PRO" "$1" && grep -Fq "google,ZUMA PRO TEGU" "$1"' sh "$probe"
check "static: the host has no playback mode, confirmation phrase, or capability edit" \
	sh -c '! grep -nE "CONFIRM|capabilities\.toml\"|>.*capabilities|1:(play|stimulus|restore|capture)" "$1"' sh "$host"
check "static: the host pins the reviewed Kleaf kernel" \
	grep -Fxq 'pinned_kernel=6.1.124-android14-11-ga4662a84a15f' "$host"
probe_status=0
sh "$probe" >/dev/null 2>&1 || probe_status=$?
check "probe: refuses a host that is not the tegu board" [ "$probe_status" -eq 4 ]

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-a2dp-output-preflight-test.XXXXXX")
# A Unix socket path must stay short (104 bytes on macOS).
rundir=$(mktemp -d /tmp/a2dp-rt.XXXXXX)
. "$root/tools/pixel-9a/tests/acm-fake-guest.sh"
sock_pid=''
trap 'acm_guest_stop; [ -z "$sock_pid" ] || { kill "$sock_pid" 2>/dev/null; wait "$sock_pid" 2>/dev/null; } || :; rm -rf "$work" "$rundir"' EXIT
unset FERRIX_PIXEL9A_ACM_TIMEOUT FERRIX_PIXEL9A_A2DP_EVIDENCE 2>/dev/null || true

# --- layer 1: the exact guest probe in a fake guest ---------------------------------
fake="$work/fake-guest"
fbin="$fake/bin"
state="$fake/state"
mkdir -p "$fbin" "$state" "$fake/dt" "$fake/proc" "$fake/spa"
printf 'ZUMA PRO TEGU MP on ZUMA PRO\000' >"$fake/dt/model"
printf 'google,ZUMA PRO TEGU\000google,ZUMA PRO\000' >"$fake/dt/compatible"
sed -e "s#^dt_root=/proc/device-tree\$#dt_root=$fake/dt#" -e "s#^proc_root=/proc\$#proc_root=$fake/proc#" \
	-e "s#^runtime_dir=/run/user/1000\$#runtime_dir=$rundir#" -e "s#^apk_db=/lib/apk/db/installed\$#apk_db=$fake/installed#" \
	-e "s#^spa_bluez_plugin=/usr/lib/spa-0.2/bluez5/libspa-bluez5.so\$#spa_bluez_plugin=$fake/spa/libspa-bluez5.so#" \
	"$probe" >"$fake/probe.sh"
check "fake guest: only the five root constants differ from the probe" \
	sh -c '[ "$(diff "$1" "$2" | grep -c "^>")" -eq 5 ]' sh "$probe" "$fake/probe.sh"

# Every fake tool logs its argv; a verb outside the reviewed set is recorded
# as a violation and fails.
cat >"$fbin/bluetoothctl" <<'EOF'
#!/bin/sh
s=$FAKE_A2DP_STATE
printf 'bluetoothctl %s\n' "$*" >>"$s/calls"
case "$*" in
	list) cat "$s/bt-list" ;;
	show) cat "$s/bt-show" ;;
	'devices Paired') cat "$s/bt-paired"; exit "$(cat "$s/bt-paired-exit" 2>/dev/null || echo 0)" ;;
	'info '??:??:??:??:??:??) f="$s/bt-info.$2"; [ -f "$f" ] || { echo "Device $2 not available"; exit 1; }; cat "$f" ;;
	*) printf 'bluetoothctl %s\n' "$*" >>"$s/violations"; exit 1 ;;
esac
EOF
cat >"$fbin/wpctl" <<'EOF'
#!/bin/sh
s=$FAKE_A2DP_STATE
printf 'wpctl %s runtime=%s\n' "$*" "${XDG_RUNTIME_DIR:-unset}" >>"$s/calls"
case "$*" in
	status) [ ! -f "$s/wp-down" ] || exit 1; cat "$s/wp-status" ;;
	'inspect '[0-9]*) f="$s/wp-inspect.$2"; [ -f "$f" ] || exit 1; cat "$f" ;;
	*) printf 'wpctl %s\n' "$*" >>"$s/violations"; exit 1 ;;
esac
EOF
cat >"$fbin/amixer" <<'EOF'
#!/bin/sh
s=$FAKE_A2DP_STATE
printf 'amixer %s\n' "$*" >>"$s/calls"
[ "$1" = -c ] && [ "$2" = 0 ] && [ "$3" = cget ] && [ "$#" -eq 4 ] || { printf 'amixer %s\n' "$*" >>"$s/violations"; exit 1; }
name=${4#iface=MIXER,name=\'}
name=${name%\'}
file="$s/ctl.$(printf '%s' "$name" | tr ' ' '_')"
[ -f "$file" ] || { echo "amixer: Cannot find the given element from control hw:0" >&2; exit 1; }
printf "numid=120,iface=MIXER,name='%s'\n  ; type=BOOLEAN,access=rw------,values=1\n  : values=%s\n" "$name" "$(cat "$file")"
EOF
cat >"$fbin/rc-service" <<'EOF'
#!/bin/sh
s=$FAKE_A2DP_STATE
printf 'rc-service %s\n' "$*" >>"$s/calls"
[ "$#" -eq 2 ] && [ "$2" = status ] || { printf 'rc-service %s\n' "$*" >>"$s/violations"; exit 1; }
exit "$(cat "$s/svc.$1" 2>/dev/null || echo 0)"
EOF
cat >"$fbin/pw-cat" <<'EOF'
#!/bin/sh
s=$FAKE_A2DP_STATE
printf 'pw-cat %s\n' "$*" >>"$s/calls"
[ "$*" = --help ] || { printf 'pw-cat %s\n' "$*" >>"$s/violations"; exit 1; }
cat "$s/pw-help"
EOF
printf '#!/bin/sh\nshift\nexec "$@"\n' >"$fbin/timeout"
printf '#!/bin/sh\necho 6.1.124-android14-11-ga4662a84a15f\n' >"$fbin/uname"
printf '#!/bin/sh\necho "$(cat "$FAKE_A2DP_STATE/uid" 2>/dev/null || echo 0)"\n' >"$fbin/id"
chmod 0755 "$fbin"/*
export FAKE_A2DP_STATE="$state"

# A real Unix socket stands in for the session's pipewire-0.
python3 -c 'import socket, sys, time
s = socket.socket(socket.AF_UNIX); s.bind(sys.argv[1]); s.listen(1); time.sleep(600)' "$rundir/pipewire-0" &
sock_pid=$!
tries=0
while [ ! -S "$rundir/pipewire-0" ]; do
	tries=$((tries + 1))
	[ "$tries" -le 50 ] || { echo 'fake pipewire socket did not appear' >&2; exit 1; }
	sleep 0.1
done

addr=66:77:88:99:AA:BB
addr2=00:11:22:33:44:55
status_head='PipeWire '"'"'pipewire-0'"'"' [1.4.9, user@ferrix-synth, cookie:12345]
 └─ Clients:
        33. WirePlumber                         [1.4.9, user@ferrix-synth, pid:900]

Audio
 ├─ Devices:
 │      42. aoc-snd-card                        [alsa]'
status_tail='Video
 ├─ Devices:
 │      60. synthetic camera                    [v4l2]

Settings
 └─ Default Configured Devices:
         0. Audio/Sink    bluez_output.66_77_88_99_AA_BB.1'
# wp_status SINK_LINES [STREAM_LINES [FILTER_LINES]]
wp_status() {
	printf '%s\n │      43. Synthetic Headphones                [bluez5]\n │\n ├─ Sinks:\n%s\n │\n ├─ Sources:\n │\n ├─ Filters:\n%s\n │\n └─ Streams:\n%s\n\n%s\n' \
		"$status_head" "$1" "${3:-}" "${2:-}" "$status_tail" >"$state/wp-status"
}
inspect_null() {
	printf 'id %s, type PipeWire:Interface:Node\n  * factory.name = "support.null-audio-sink"\n  * media.class = "Audio/Sink"\n  * node.description = "Dummy Output"\n  * node.name = "auto_null"\n' \
		"$1" >"$state/wp-inspect.$1"
}
# inspect_bluez ID NODE_ADDRESS PROPERTY_ADDRESS PROFILE
inspect_bluez() {
	printf 'id %s, type PipeWire:Interface:Node\n    api.bluez5.address = "%s"\n    api.bluez5.codec = "sbc_xq"\n    api.bluez5.profile = "%s"\n' \
		"$1" "$3" "$4" >"$state/wp-inspect.$1"
	printf '  * device.api = "bluez5"\n    device.serial = "SYNSERIAL46"\n  * media.class = "Audio/Sink"\n  * node.description = "Synthetic Headphones"\n' >>"$state/wp-inspect.$1"
	printf '  * node.name = "bluez_output.%s.1"\n' "$(printf '%s' "$2" | tr ':' '_')" >>"$state/wp-inspect.$1"
}
inspect_alsa() {
	printf 'id %s, type PipeWire:Interface:Node\n    alsa.card = "0"\n  * device.api = "alsa"\n  * media.class = "Audio/Sink"\n  * node.name = "alsa_output.platform-aoc.HiFi__Speaker__sink"\n' \
		"$1" >"$state/wp-inspect.$1"
}
bt_info() {
	# ADDRESS PAIRED CONNECTED SINK(yes|no)
	{
		printf 'Device %s (public)\n\tName: Synthetic Headphones\n\tAlias: Synthetic Headphones\n\tClass: 0x00240404 (2360324)\n' "$1"
		printf '\tIcon: audio-headset\n\tPaired: %s\n\tBonded: %s\n\tTrusted: yes\n\tBlocked: no\n\tConnected: %s\n\tLegacyPairing: no\n' "$2" "$2" "$3"
		[ "$4" != yes ] || printf '\tUUID: Audio Sink                (0000110b-0000-1000-8000-00805f9b34fb)\n'
		printf '\tUUID: A/V Remote Control Target (0000110c-0000-1000-8000-00805f9b34fb)\n'
	} >"$state/bt-info.$1"
}
# reset_guest: the inventoried physical state (#91): BlueZ powered, nothing
# paired, PipeWire with only Dummy Output, speaker path off.
reset_guest() {
	rm -rf "${state:?}"/* "$fake/proc"/*
	mkdir -p "$fake/proc/1/fd" "$fake/proc/2/fd"
	ln -s /dev/null "$fake/proc/1/fd/0"
	ln -s /dev/snd/pcmC0D8c "$fake/proc/2/fd/7"
	: >"$fake/spa/libspa-bluez5.so"
	printf 'C:Q1abc=\nP:bluez\nV:5.83-r0\n\nP:pipewire\nV:1.4.9-r0\n\nP:wireplumber\nV:0.5.12-r0\n\nP:pipewire-spa-bluez\nV:1.4.9-r0\n\nP:pipewire-tools\nV:1.4.9-r0\n' \
		>"$fake/installed"
	printf 'Controller 0A:0B:0C:0D:0E:0F ferrix [default]\n' >"$state/bt-list"
	printf 'Controller 0A:0B:0C:0D:0E:0F (public)\n\tName: ferrix\n\tPowered: yes\n\tDiscoverable: no\n' >"$state/bt-show"
	: >"$state/bt-paired"
	printf 'pw-cat [options] [<file>|-]\n  -h, --help                            Show this help\n      --target                          Set node target serial or name (default auto)\n  -P  --properties                      Set node properties\n' >"$state/pw-help"
	wp_status ' │  *   30. Dummy Output                        [vol: 1.00]'
	inspect_null 30
	printf 'off\n' >"$state/ctl.TDM_0_RX_Mixer_EP1"
	printf 'off\n' >"$state/ctl.TAS25XX_ASI_Switch"
}
# connect_headset: one paired, connected A2DP headset that WirePlumber made
# the default sink.
connect_headset() {
	printf 'Device %s Synthetic Headphones\n' "$addr" >"$state/bt-paired"
	bt_info "$addr" yes yes yes
	wp_status ' │      30. Dummy Output                        [vol: 1.00]
 │  *   46. Synthetic Headphones                [vol: 0.50]'
	inspect_bluez 46 "$addr" "$addr" a2dp-sink
}
gout=''
gstatus=0
guest() {
	gstatus=0
	gout=$(PATH="$fbin:$PATH" sh "$fake/probe.sh" 2>/dev/null) || gstatus=$?
}
gget() { printf '%s\n' "$gout" | sed -n "s/^$1=//p" | sed -n 1p; }
no_violation() { [ ! -s "$state/violations" ]; }
no_identifier() { ! printf '%s\n' "$gout" | grep -Eiq '66[:_]77|00[:_]11|0A[:_]0B|Synthetic Headphones|bluez_output|SYNSERIAL|auto_null'; }
awk '/^validate\(\) \{/,/^}/; /^expected_keys=/,/probe_complete.$/' "$host" >"$work/validate.sh"
{ grep -E '^(get|is_n)\(\) \{.*\}$' "$host"; awk '/^preflight_problems\(\) \{/,/^}/' "$host"; } >"$work/problems.sh"
host_accepts() { printf '%s\n' "$gout" >"$work/g.out"; sh -c '. "$1"; validate "$2"' sh "$work/validate.sh" "$work/g.out" >/dev/null; }
host_problems() { printf '%s\n' "$gout" >"$work/g.out"; sh -c '. "$1"; pinned_kernel=6.1.124-android14-11-ga4662a84a15f; preflight_problems "$2"' sh "$work/problems.sh" "$work/g.out"; }
current_problems='no-paired-audio-sink,connected-audio-sink-count-not-one,bluez-a2dp-sink-count-not-one'

reset_guest
guest
check "guest current state: completes on the fake tegu guest" sh -c '[ "$1" -eq 0 ] && [ "$2" = yes ]' sh "$gstatus" "$(gget probe_complete)"
check "guest current state: reads BlueZ and the graph as inventoried" \
	sh -c '[ "$1" = 1 ] && [ "$2" = yes ] && [ "$3" = ok ] && [ "$4" = 0 ] && [ "$5" = 0 ] && [ "$6" = 1 ] && [ "$7" = 1 ] && [ "$8" = 0 ] && [ "$9" = null ]' \
	sh "$(gget bt_controllers)" "$(gget bt_powered)" "$(gget bt_paired_query)" "$(gget bt_paired)" "$(gget graph_sink_bluez_a2dp)" \
	"$(gget graph_sinks)" "$(gget graph_sink_null)" "$(gget graph_streams)" "$(gget default_sink_class)"
check "guest current state: versions come from the apk database" \
	sh -c '[ "$1" = 5.83-r0 ] && [ "$2" = 1.4.9-r0 ] && [ "$3" = 0.5.12-r0 ] && [ "$4" = 1.4.9-r0 ] && [ "$5" = present ]' \
	sh "$(gget pkg.bluez)" "$(gget pkg.pipewire)" "$(gget pkg.wireplumber)" "$(gget pkg.pipewire_tools)" "$(gget spa_bluez_plugin)"
check "guest current state: a capture fd is not a playback opener" sh -c '[ "$1" = 0 ] && [ "$2" = 2 ]' sh "$(gget playback_pcm_openers)" "$(gget pcm_scan_procs)"
check "guest current state: speaker-path controls read off" sh -c '[ "$1" = off ] && [ "$2" = off ]' sh "$(gget watch.tdm_0_rx_mixer_ep1)" "$(gget watch.tas25xx_asi_switch)"
check "guest current state: wpctl reached the uid-1000 session runtime dir" \
	sh -c '[ "$(grep -c "^wpctl status runtime=$2\$" "$1")" -eq 1 ]' sh "$state/calls" "$rundir"
check "guest current state: only reviewed query verbs ran" no_violation
check "guest current state: the host validator accepts the real output" host_accepts
check "guest current state: the host blocks on the missing headset only" [ "$(host_problems)" = "$current_problems" ]
printf '%s\n' "$gout" >"$work/fx-current"

reset_guest
connect_headset
guest
check "guest headset: one A2DP sink whose node and address name the connected device" \
	sh -c '[ "$1" = 1 ] && [ "$2" = yes ] && [ "$3" = yes ] && [ "$4" = sbc-xq ] && [ "$5" = bluez-a2dp ] && [ "$6" = 1 ] && [ "$7" = 1 ]' \
	sh "$(gget graph_sink_bluez_a2dp)" "$(gget a2dp_sink_device_match)" "$(gget a2dp_sink_default)" "$(gget a2dp_sink_codec)" \
	"$(gget default_sink_class)" "$(gget bt_paired_audio_sink)" "$(gget bt_connected_audio_sink)"
check "guest headset: no address, device name, node name, or serial leaves the guest" no_identifier
check "guest headset: the host validator accepts it" host_accepts
check "guest headset: the host finds no blocker" [ -z "$(host_problems)" ]
check "guest headset: only reviewed query verbs ran" no_violation
check "guest headset: amixer only read the two watched controls" \
	sh -c '[ "$(grep -c "^amixer " "$1")" -eq 2 ] && [ "$(grep "^amixer " "$1" | grep -vc " cget ")" -eq 0 ]' sh "$state/calls"
printf '%s\n' "$gout" >"$work/fx-candidate"

# guest_case NAME WANT SETUP...: reset, connect, run SETUP, expect the host's
# exact blocker list (and no tool violation).
guest_case() {
	name=$1 want=$2
	shift 2
	reset_guest
	connect_headset
	"$@"
	guest
	got=$(host_problems)
	check "guest blocked ($name): $want" sh -c '[ "$1" = "$2" ] && [ ! -s "$3" ] && [ "$4" -eq 0 ]' sh "$got" "$want" "$state/violations" "$gstatus"
	[ "$got" = "$want" ] || printf '     got: %s\n' "$got"
	check "guest blocked ($name): no identifier leaves the guest" no_identifier
}
hfp() { inspect_bluez 46 "$addr" "$addr" headset-head-unit; }
guest_case headset-profile bluez-a2dp-sink-count-not-one,bluez-non-a2dp-sink-present hfp
mismatch() { inspect_bluez 46 "$addr2" "$addr2" a2dp-sink; }
guest_case other-device-sink a2dp-sink-device-mismatch mismatch
split() { inspect_bluez 46 "$addr" "$addr2" a2dp-sink; }
guest_case name-property-disagree a2dp-sink-device-mismatch split
two() {
	printf 'Device %s Synthetic Headphones\nDevice %s Other Headphones\n' "$addr" "$addr2" >"$state/bt-paired"
	bt_info "$addr2" yes yes yes
}
guest_case two-connected connected-audio-sink-count-not-one,a2dp-sink-device-mismatch two
disconnected() { bt_info "$addr" yes no yes; }
guest_case disconnected connected-audio-sink-count-not-one,a2dp-sink-device-mismatch disconnected
no_sink_uuid() { bt_info "$addr" yes yes no; }
guest_case no-sink-uuid no-paired-audio-sink,connected-audio-sink-count-not-one,a2dp-sink-device-mismatch no_sink_uuid
alsa() {
	wp_status ' │      30. Dummy Output                        [vol: 1.00]
 │  *   46. Synthetic Headphones                [vol: 0.50]
 │      50. Speaker                             [vol: 0.40]'
	inspect_alsa 50
}
guest_case alsa-sink internal-alsa-sink-present alsa
uninspectable() {
	wp_status ' │      30. Dummy Output                        [vol: 1.00]
 │  *   46. Synthetic Headphones                [vol: 0.50]
 │      51. Mystery                             [vol: 0.40]'
}
guest_case uninspectable-sink sink-inspect-failed uninspectable
stream() {
	wp_status ' │      30. Dummy Output                        [vol: 1.00]
 │  *   46. Synthetic Headphones                [vol: 0.50]' '        70. waydroid
             71. output_FL       > Synthetic Headphones:playback_FL	[active]
             72. output_FR       > Synthetic Headphones:playback_FR	[active]'
}
guest_case stream-present audio-streams-present stream
speaker_on() { printf 'on\n' >"$state/ctl.TDM_0_RX_Mixer_EP1"; }
guest_case speaker-route-on internal-speaker-route-not-off speaker_on
amp_gone() { rm -f "$state/ctl.TAS25XX_ASI_Switch"; }
guest_case amp-unreadable internal-amp-switch-not-off amp_gone
pcm_open() { ln -s /dev/snd/pcmC0D0p "$fake/proc/2/fd/9"; }
guest_case playback-pcm-open playback-pcm-open pcm_open
unpowered() { printf 'Controller 0A:0B:0C:0D:0E:0F (public)\n\tPowered: no\n' >"$state/bt-show"; }
guest_case unpowered bt-not-powered unpowered
paired_fails() { printf '1\n' >"$state/bt-paired-exit"; }
guest_case paired-query-fails bt-paired-query-incomplete,no-paired-audio-sink,connected-audio-sink-count-not-one,a2dp-sink-device-mismatch paired_fails
wp_down() { : >"$state/wp-down"; }
guest_case graph-unreachable pipewire-graph-unreachable wp_down
no_pwcat_opts() { printf 'pw-cat [options]\n  -h, --help   Show this help\n' >"$state/pw-help"; }
guest_case pw-cat-old pw-cat-target-option-missing,pw-cat-properties-option-missing no_pwcat_opts
bt_stopped() { printf '3\n' >"$state/svc.bluetooth"; }
guest_case bluetooth-stopped service-bluetooth-not-started bt_stopped
no_spa() { rm -f "$fake/spa/libspa-bluez5.so"; sed -i.bak '/^P:pipewire-spa-bluez$/,/^$/d' "$fake/installed"; }
guest_case no-spa-bluez package-pipewire-spa-bluez-not-installed,spa-bluez-plugin-missing no_spa
not_root() { printf '1000\n' >"$state/uid"; }
guest_case not-root not-root not_root

# --- layer 2: host workflow against layer-1 output ----------------------------------
acm_guest_init "$root" "$work" "$probe"
mkdir -p "$work/fx"
cp "$work/fx-current" "$work/fx/current"
cp "$work/fx-candidate" "$work/fx/candidate"
derive() {
	src=$1 name=$2
	shift 2
	sed "$@" "$work/fx/$src" >"$work/fx/$name"
}
derive candidate unexpected-key -e 's/^probe_complete=yes$/bt_address=redacted\nprobe_complete=yes/'
derive candidate duplicate-key -e 's/^probe_complete=yes$/graph_sinks=2\nprobe_complete=yes/'
derive candidate colon-mac -e "s/^a2dp_sink_codec=.*/a2dp_sink_codec=$addr/"
derive candidate underscore-mac -e 's/^a2dp_sink_codec=.*/a2dp_sink_codec=66_77_88_99_AA_BB/'
derive candidate hex-run -e 's/^pkg.bluez=.*/pkg.bluez=66778899AABB/'
derive candidate missing-key -e '/^graph_sink_alsa=/d'
derive candidate truncated -e '/^probe_complete=/d'
derive candidate wrong-probe -e 's/^probe_version=1$/probe_version=2/'
derive candidate wrong-identity -e 's/^dt_model=.*/dt_model=ZUMA PRO COMET MP on ZUMA PRO/'

out=''
status=0
run() {
	# CASE FIXTURE [EXIT] -- ARGS...
	case_name=$1 fx=$2 st=${3:-0}
	shift 3
	case_dir="$work/run-$case_name"
	mkdir -p "$case_dir"
	acm_guest_start
	acm_guest_ctl fixture "$work/fx/$fx"
	acm_guest_ctl exit "$st"
	status=0
	out=$(FERRIX_OUT_DIR="$case_dir" FERRIX_PIXEL9A_A2DP_EVIDENCE="${EVIDENCE:-mock}" sh "$host" "$@" 2>&1) || status=$?
	runs=$(acm_guest_runs)
	acm_guest_stop
	results="$case_dir/pixel-9a/a2dp-output-preflight"
	EVIDENCE=mock
}
has() { printf '%s\n' "$out" | grep -Fxq "$1"; }

run candidate candidate 0
check "host candidate: exit 0 after one session" sh -c '[ "$1" -eq 0 ] && [ "$2" -eq 1 ]' sh "$status" "$runs"
check "host candidate: the guest got the exact probe body" cmp -s "$ACM_GUEST_CTL/stdin.1" "$probe"
check "host candidate: reports candidate-observed with the active gate closed" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq a2dp_preflight=candidate-observed && printf "%s\n" "$1" | grep -Fxq active_proof=not-implemented &&
		printf "%s\n" "$1" | grep -Fxq active_gate=closed && printf "%s\n" "$1" | grep -Fxq audio_capability=unsupported &&
		printf "%s\n" "$1" | grep -Fxq active_gate_reason=pipewire-explicit-target-fallback-unverified-on-target' sh "$out"
check "host candidate: reports versions for the gate review" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq pkg.wireplumber=0.5.12-r0 && printf "%s\n" "$1" | grep -Fxq pkg.pipewire=1.4.9-r0' sh "$out"
check "host candidate: never offers a playback phrase" sh -c '! printf "%s\n" "$1" | grep -Eiq "confirm|phrase|pw-cat --target"' sh "$out"
check "host candidate: transcript keeps the probe hash and validated lines" \
	sh -c 'grep -Eq "^probe_body_sha256=[0-9a-f]{64}$" "$1" && grep -Fxq evidence=mock "$1" && grep -Fxq graph_sink_bluez_a2dp=1 "$1" &&
		grep -Fxq a2dp_preflight=candidate-observed "$1"' sh "$results/preflight-latest.txt"
EVIDENCE=physical run physical candidate 0
check "host physical: transcript names the exact target" grep -Fxq "evidence=physical:$target_id" "$results/preflight-latest.txt"
run current current 0
check "host current state: blocked, exit 6, with the exact blockers" \
	sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq a2dp_preflight=blocked && printf "%s\n" "$2" | grep -Fxq "a2dp_preflight_problems=$3" &&
		printf "%s\n" "$2" | grep -Fxq active_gate=closed' sh "$status" "$out" "$current_problems"
for item in unexpected-key:'unexpected key bt_address' duplicate-key:'duplicate key graph_sinks' \
	colon-mac:'value outside the reviewed character set for a2dp_sink_codec' \
	underscore-mac:'value outside the reviewed character set for a2dp_sink_codec' \
	hex-run:'identifier-shaped value for pkg.bluez' missing-key:'missing keys' truncated:'missing keys'; do
	fx=${item%%:*}
	want=${item#*:}
	run "$fx" "$fx" 0
	check "host refuses ($fx): exit 5, $want" sh -c '[ "$1" -eq 5 ] && printf "%s\n" "$2" | grep -Fq "($3)" &&
		! grep -rEq "66[:_]77|99AABB" "$4"' sh "$status" "$out" "$want" "$work/run-$fx"
done
run wrong-probe wrong-probe 0
check "host refuses (wrong-probe): exit 5" sh -c '[ "$1" -eq 5 ]' sh "$status"
run wrong-identity wrong-identity 0
check "host refuses (identity line): exit 4" sh -c '[ "$1" -eq 4 ]' sh "$status"
run guest-identity candidate 4
check "host refuses (guest device-tree gate): exit 4" sh -c '[ "$1" -eq 4 ] && printf "%s\n" "$2" | grep -Fq "not the exact tegu board"' sh "$status" "$out"
run guest-error candidate 1
check "host refuses (guest failure): exit 3" sh -c '[ "$1" -eq 3 ]' sh "$status"
run usage candidate 0 play
check "host usage: no play mode, exit 2 without a session" sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 0 ]' sh "$status" "$runs"
EVIDENCE=bogus run bad-evidence candidate 0
check "host usage: unknown evidence label is exit 2" sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 0 ]' sh "$status" "$runs"
mkdir -p "$work/run-unreachable"
acm_guest_start --silent
status=0
out=$(FERRIX_OUT_DIR="$work/run-unreachable" FERRIX_PIXEL9A_ACM_TIMEOUT=5 FERRIX_PIXEL9A_A2DP_EVIDENCE=mock sh "$host" 2>&1) || status=$?
silent_port=$FERRIX_SHELL
acm_guest_stop
check "host unreachable: exit 3 with sanitised reason" sh -c '[ "$1" -eq 3 ] && printf "%s\n" "$2" | grep -Fq "transport failed (no-shell-response)"' sh "$status" "$out"
check "privacy: the port path is neither printed nor retained" \
	sh -c '! printf "%s\n" "$1" | grep -Fq "$2" && ! grep -rFq "$2" "$3"' sh "$out" "$silent_port" "$work/run-unreachable"

# --- layer 3: end to end through the transport -----------------------------------
alt="$work/alt"
mkdir -p "$alt/tools/pixel-9a"
cp "$host" "$root/tools/pixel-9a/acm-probe.py" "$alt/tools/pixel-9a/"
cp "$fake/probe.sh" "$alt/tools/pixel-9a/a2dp-output-preflight-remote.sh"
cp "$fbin"/* "$work/acm-guest/bin/"
ACM_GUEST_PROBE="$alt/tools/pixel-9a/a2dp-output-preflight-remote.sh"
export ACM_GUEST_PROBE
e2e() {
	case_dir="$work/run-$1"
	mkdir -p "$case_dir"
	acm_guest_start
	acm_guest_ctl real 1
	status=0
	out=$(FERRIX_OUT_DIR="$case_dir" FERRIX_PIXEL9A_A2DP_EVIDENCE=mock sh "$alt/tools/pixel-9a/a2dp-output-preflight.sh" 2>&1) || status=$?
	acm_guest_stop
	results="$case_dir/pixel-9a/a2dp-output-preflight"
}
reset_guest
e2e e2e-current
check "e2e current state: blocked on the missing headset" \
	sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq "a2dp_preflight_problems=$3"' sh "$status" "$out" "$current_problems"
reset_guest
connect_headset
e2e e2e-candidate
check "e2e headset: candidate-observed with the gate closed" \
	sh -c '[ "$1" -eq 0 ] && printf "%s\n" "$2" | grep -Fxq a2dp_preflight=candidate-observed && printf "%s\n" "$2" | grep -Fxq active_gate=closed' sh "$status" "$out"
check "e2e headset: only reviewed query verbs ran" no_violation
check "e2e headset: no address, name, node name, or serial in output or transcripts" \
	sh -c '! printf "%s\n" "$1" | grep -Eiq "66[:_]77|0A[:_]0B|Synthetic|bluez_output|SYNSERIAL" &&
		! grep -rEiq "66[:_]77|0A[:_]0B|Synthetic|bluez_output|SYNSERIAL" "$2"' sh "$out" "$work/run-e2e-candidate"

check "no test edits docs/status.md" [ "$(cksum <"$root/docs/status.md")" = "$status_before" ]
check "no test edits the daily-phone plan" [ "$(cksum <"$root/plans/pixel-9a-daily-phone.md")" = "$plan_before" ]
check "no test edits capabilities.toml" [ "$(cksum <"$root/targets/boards/$target_id/capabilities.toml")" = "$capabilities_before" ]

if [ "$fails" -ne 0 ]; then
	printf '{"event":"pixel-9a-a2dp-output-preflight-test","status":"failed","failures":%s}\n' "$fails"
	exit 1
fi
printf '{"event":"pixel-9a-a2dp-output-preflight-test","status":"passed"}\n'
