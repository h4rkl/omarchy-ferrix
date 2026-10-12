#!/bin/sh
# Offline proof for the issue #91 bounded A2DP playback proof. No device is
# contacted, nothing is played or recorded, and no real BlueZ, PipeWire, or
# mixer exists. Layers:
#
#   0. static policy of the host and guest bodies;
#   1. the exact composed guest session (only path constants redirected) run
#      by the host /bin/sh against fake bluetoothctl, wpctl, amixer,
#      rc-service, pw-mon and pw-cat. The fake pw-mon replays a scripted
#      registry stream; the fake pw-cat reads the tone, emits its own client,
#      node, port and link, and can inject a misroute, race, disconnect,
#      kernel-side opener or control change while "playing";
#   2. the host workflow over the real CDC-ACM transport and a fake guest that
#      answers with layer-1 output or a variant of it (decision lifecycle,
#      classification, malformed and identifier-bearing output);
#   3. end to end: host, transport, fake guest running the composed session.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
host="$root/tools/pixel-9a/a2dp-active-proof.sh"
body="$root/tools/pixel-9a/a2dp-active-proof-remote.sh"
probe="$root/tools/pixel-9a/a2dp-output-preflight-remote.sh"
pre_host="$root/tools/pixel-9a/a2dp-output-preflight.sh"
target_id=google-pixel-9a-tegu-gxq96-mp1-0
fails=0
command -v python3 >/dev/null 2>&1 || { echo 'python3 is required for the CDC-ACM transport test' >&2; exit 1; }
# The evidence record is docs/status.md plus every docs/status-log/ month.
status_sum() { cat "$root/docs/status.md" "$root"/docs/status-log/*.md | cksum; }
status_before=$(status_sum)
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

# --- layer 0: static policy -----------------------------------------------------
check "static: host POSIX syntax" sh -n "$host"
check "static: guest body POSIX syntax" sh -n "$body"
code=$(grep -vE '^[[:space:]]*#' "$body")
host_code=$(grep -vE '^[[:space:]]*(#|printf )' "$host")
offending=$(printf '%s\n' "$code" | grep -nwE 'aplay|arecord|speaker-test|pw-play|pw-record|paplay|parecord|pactl|alsactl|alsaucm|pw-cli|pw-link|pw-metadata|pw-loopback|modprobe|insmod|rmmod|mount|umount|dd|btmgmt|hciconfig|rfkill|rc-service|rc-update' || true)
check "static: the guest body never records, links, edits metadata, or touches modules, mounts, radios, or services" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
check "static: amixer is only cget on the two watched controls" \
	sh -c '! printf "%s\n" "$1" | grep -E "amixer.*(cset|sset|set |-s |events)" &&
		[ "$(printf "%s\n" "$1" | grep -c "amixer -c")" -eq 1 ] && printf "%s\n" "$1" | grep -Fq "amixer -c \"\$card\" cget" &&
		grep -Fxq "watched_controls='"'"'TDM_0_RX Mixer EP1|TAS25XX ASI Switch'"'"'" "$2"' sh "$code" "$body"
check "static: wpctl is only status, inspect and get-volume (never a volume, mute or default write)" \
	sh -c '[ "$(printf "%s\n" "$1" | grep -oE "(wp|wpctl) (status|inspect|get-[a-z]+|set-[a-z]+|settings|clear-default|load|edit)" | sort -u | tr "\n" "|")" = "wp get-volume|wp inspect|wp status|" ] &&
		! printf "%s\n" "$1" | grep -Eq "set-volume|set-mute|set-default|set-profile|set-route"' sh "$code"
check "static: the volume cap is one named constant, 0.50 on the wpctl scale, the same on host and guest" \
	sh -c 'grep -Fxq volume_cap=50 "$1" && grep -Fxq volume_cap=50 "$2" && [ "$(grep -c "^volume_cap=" "$1")" -eq 1 ]' sh "$body" "$host"
check "static: no fixed sleep stands in for the stream removal" sh -c '! grep -Eq "^sleep 0\.[0-9]+$" "$1" && grep -Fq "own-removed" "$1"' sh "$body"
check "static: bluetoothctl is only devices Paired and info" \
	[ "$(printf '%s\n' "$code" | grep -oE 'bluetoothctl [a-z]+( [A-Z][a-z]+)?' | sort -u | tr '\n' '|')" = 'bluetoothctl devices Paired|bluetoothctl info|' ]
pw_lines=$(printf '%s\n' "$code" | grep -E 'pw-cat( |$)' | grep -vE 'for tool in')
check "static: pw-cat runs once, raw playback from the tone FIFO, with --target and --properties naming the sink serial" \
	sh -c '[ "$(printf "%s\n" "$1" | grep -c "pw-cat --playback")" -eq 1 ] &&
		printf "%s\n" "$2" | grep -Fq "pw-cat --playback --raw --rate 48000 --channels 1 --format u8 \\" &&
		printf "%s\n" "$2" | grep -Fq "	--target \"\$id_serial\" --properties \"\$props\" - <\"\$proof_dir/tone.fifo\"" &&
		[ "$(printf "%s\n" "$2" | grep -c -- "--volume")" -eq 0 ]' sh "$pw_lines" "$code"
for prop in 'target.object = \"$id_serial\"' 'node.dont-fallback = true' 'node.dont-reconnect = true' 'node.dont-move = true' \
	'node.linger = false' 'state.restore-props = false' 'state.restore-target = false'; do
	check "static: the stream sets $prop" sh -c 'grep "^props=" "$1" | grep -Fq "$2"' sh "$body" "$prop"
done
check "static: pw-mon is the only monitor, run read-only with no colours, no params, and separators" \
	sh -c '[ "$(printf "%s\n" "$1" | grep -c "pw-mon -N -a -p ")" -eq 1 ] && [ "$(printf "%s\n" "$1" | grep -cE "pw-mon( |\$)" )" -eq 2 ]' sh "$code"
check "static: the tone is at most 1.5 s of u8 mono 48 kHz, bounded by head -c and a 4 s watchdog" \
	sh -c 'grep -Fxq tone_bytes=72000 "$1" && grep -Fxq tone_cycles=1125 "$1" && grep -Fxq play_wall=4 "$1" &&
		grep -Fq "| head -c \"\$tone_bytes\" >\"\$proof_dir/tone.fifo\"" "$1" && [ $((72000 / 48000)) -lt 2 ]' sh "$body"
want_cycle=$(awk 'BEGIN { pi = atan2(0, -1); s = ""; for (i = 0; i < 64; i++) { x = 13 * sin(2 * pi * i / 64); v = 128 + int(x + (x >= 0 ? 0.5 : -0.5)); s = s sprintf("\\%03o", v) } print s }')
check "static: the cycle table is one 750 Hz cycle at 48 kHz, peak 13/128" grep -Fxq "tone_cycle='$want_cycle'" "$body"
# Identity values reach only the guard FIFO (>&3) and the binding hash input.
leaks=$(printf '%s\n' "$code" | grep -E '(out|printf)[^#]*[$](id_addr|id_name|id_serial|id_dev|id_sink|own_tok|own_name|ctl_tok|proof_salt|address)' |
	grep -vE '>&3$|binding-v2|address=%s' || true)
check "static: the guest never prints the address, node name, serial, device id, salt or tokens" [ -z "$leaks" ]
[ -z "$leaks" ] || printf '%s\n' "$leaks"
check "static: the guest body gates on the exact tegu device tree" \
	sh -c 'grep -Fq "ZUMA PRO TEGU MP on ZUMA PRO" "$1" && grep -Fq "google,ZUMA PRO TEGU" "$1"' sh "$body"
verbs='adb|fastboot|ssh|scp|curl|dd|mount|umount|flash|erase|reboot|modprobe|insmod|rmmod|rc-service|amixer|aplay|arecord|bluetoothctl|wpctl|pw-cat|pw-mon|pw-play|paplay|afplay'
offending=$(printf '%s\n' "$host_code" | grep -nE "(^|[;|&(]|then|do|else)[[:space:]]*($verbs)([[:space:]]|$)" || true)
check "static: the host runs no Android, storage, network, Bluetooth, or audio command" [ -z "$offending" ]
check "static: the host never edits capabilities.toml" sh -c '! grep -nE "capabilities\.toml\"|>.*capabilities" "$1"' sh "$host"
check "static: the host always reports audio unsupported and owner-pending audibility" \
	sh -c 'grep -Fq "emit audio_capability unsupported" "$1" && grep -Fq "emit audible_confirmation owner-pending" "$1"' sh "$host"
check "static: the v3 preflight still has no playback mode" \
	sh -c 'grep -Fxq "active_gate_reason=active-playback-workflow-not-implemented" "$1" && ! grep -n "CONFIRM" "$1"' sh "$pre_host"

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-a2dp-active-proof-test.XXXXXX")
rundir=$(mktemp -d /tmp/a2dp-ap.XXXXXX)
# The host keeps its decision salt only inside the repository's ignored out/,
# so every host run here writes below this directory.
mkdir -p "$root/out"
outroot=$(mktemp -d "$root/out/a2dp-active-proof-test.XXXXXX")
. "$root/tools/pixel-9a/tests/acm-fake-guest.sh"
sock_pid=''
trap 'acm_guest_stop; [ -z "$sock_pid" ] || { kill "$sock_pid" 2>/dev/null; wait "$sock_pid" 2>/dev/null; } || :; rm -rf "$work" "$rundir" "$outroot"' EXIT
unset FERRIX_PIXEL9A_ACM_TIMEOUT FERRIX_PIXEL9A_A2DP_EVIDENCE FERRIX_PIXEL9A_A2DP_PLAY_CONFIRM 2>/dev/null || true

# The host's library extraction is the code both sides judge the preflight by.
awk '/^library\(\) \{/,/^}/' "$host" >"$work/library-fn.sh"
sh -c '. "$1"; preflight_host=$2; library' sh "$work/library-fn.sh" "$pre_host" >"$work/library.sh"
check "library: the extracted v3 functions parse and are defined once" \
	sh -c 'sh -n "$1" && for f in validate get is_n preflight_problems; do [ "$(grep -c "^$f() {" "$1")" -eq 1 ] || exit 1; done' sh "$work/library.sh"
fn_text() { awk -v f="$1() {" 'index($0, f) == 1 { on = 1 } on { print } on && /^}/ { exit }' "$2"; }
for fn in validate preflight_problems; do
	check "library: $fn is byte-identical to the v3 host's" [ "$(fn_text "$fn" "$work/library.sh")" = "$(fn_text "$fn" "$pre_host")" ]
done

# --- layer 0b: the graph guard awk, fed recorded pw-mon text directly ----------
# PipeWire Module objects print a quoted args value over several lines, and
# some (protocol-native) hold blank and "#" lines inside it. Seen on
# physical:google-pixel-9a-tegu-gxq96-mp1-0 (2026-10-12): the guard took the
# blank line for the end of the block and tripped on normal startup output.
sed -n "/^guard_awk='\$/,/^'\$/p" "$body" >"$work/guard-awk.sh"
check "guard unit: the guard awk is extracted from the guest body" sh -c 'sh -n "$1" && [ "$(wc -l <"$1")" -gt 100 ]' sh "$work/guard-awk.sh"
gdir="$work/guard-unit"
gtok=feedfacefeedface
# guard_run INPUT: the guard over INPUT to end of file; results in $gdir.
guard_run() {
	rm -rf "$gdir"
	mkdir -p "$gdir"
	sh -c '. "$1"; awk -v tok="$2" -v dir="$3" "$guard_awk" <"$4"' sh "$work/guard-awk.sh" "$gtok" "$gdir" "$1" 2>/dev/null
}
gfile() { if [ -s "$gdir/$1" ]; then sed -n '1p' "$gdir/$1"; else echo none; fi; }
ctl() { printf 'FERRIXCTL %s %s\n' "$gtok" "$*"; }
pw_core() { printf '\ttype: PipeWire:Interface:Core\n\tcookie: 1\n\tversion: "1.6.8"\n\tname: "pipewire-0"\n \tproperties:\n \t\tcore.daemon = "true"\n\n'; }
# The redacted module-rt block exactly as pw-mon printed it on the device.
pw_module_rt() {
	printf 'added:\n\tid: 1\n\tpermissions: r--m-\n\ttype: PipeWire:Interface:Module (version 3)\n\tname: "libpipewire-module-rt"\n'
	printf '\tfilename: "/usr/lib/pipewire-0.3/libpipewire-module-rt.so"\n\targs: "{\n            nice.level    = -11\n'
	printf '            rt.prio       = 88\n            #rt.time.soft = -1\n        }"\n'
	printf ' \tproperties:\n \t\tmodule.name = "libpipewire-module-rt"\n \t\tobject.id = "1"\n \t\tobject.serial = "1"\n\n'
}
# protocol-native shape: blank and "#" lines in args, plus lines shaped like
# every rule the guard has (property, marked property, id, type, header-free
# garbage); none of them may reach p[], pchg, bad, id or type.
pw_module_native() {
	printf 'added:\n\tid: 2\n\tpermissions: r--m-\n\ttype: PipeWire:Interface:Module (version 3)\n\tname: "libpipewire-module-protocol-native"\n'
	printf '\tfilename: "/usr/lib/pipewire-0.3/libpipewire-module-protocol-native.so"\n\targs: "{\n\n'
	printf '            # Deprecated legacy mode (not socket-based),\n            #access.legacy = true\n\n'
	printf ' \t\tobject.serial = "999"\n*\t\tnode.name = "forged"\n\tid: 77\n\ttype: PipeWire:Interface:Node (version 3)\n'
	printf 'garbage that pw-mon would never print\n        }"\n'
	printf ' \tproperties:\n \t\tmodule.name = "libpipewire-module-protocol-native"\n \t\tobject.id = "2"\n \t\tobject.serial = "2"\n\n'
}
pw_registry() {
	printf 'added:\n\tid: 33\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Client (version 3)\n \tproperties:\n \t\tobject.serial = "10"\n \t\tpipewire.sec.pid = "900"\n\n'
	printf 'added:\n\tid: 43\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Device (version 3)\n \tproperties:\n \t\tobject.serial = "20"\n \t\tdevice.api = "bluez5"\n\n'
	printf 'added:\n\tid: 46\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n \tstate: "suspended"\n \tproperties:\n \t\tobject.serial = "40"\n \t\tnode.name = "bluez_output.x.1"\n \t\tmedia.class = "Audio/Sink"\n\n'
	printf 'added:\n\tid: 35\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Client (version 3)\n \tproperties:\n \t\tobject.serial = "500"\n \t\tpipewire.sec.pid = "4242"\n\n'
}
# guard_session ENFORCE_EVENTS: startup with both modules, arm, review, then
# ENFORCE_EVENTS (a shell function printing events) and stop.
guard_session() {
	pw_core; pw_module_rt; pw_module_native; pw_registry
	ctl monitor 4242; ctl preflight; ctl bind 46 43 own-node; ctl review
	"$1"
	ctl stop
}
no_events() { :; }
guard_session no_events >"$work/guard-modules.txt"
guard_run "$work/guard-modules.txt"
check "guard unit: Module args with blank, \"#\" and \"= \" lines parse; the guard arms and reviews clean" \
	sh -c '[ "$1" = armed ] && [ "$2" = ok ] && [ "$3" = none ] && [ "$4" = "no 0 no no none" ]' sh \
	"$(gfile armed)" "$(gfile review)" "$(gfile trip.graph)" "$(gfile final)"
alsa_after() { printf 'added:\n\tid: 80\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n \tstate: "suspended"\n \tproperties:\n \t\tobject.serial = "610"\n \t\tnode.name = "alsa_output.x"\n \t\tmedia.class = "Audio/Sink"\n\n'; }
guard_session alsa_after >"$work/guard-modules-sink.txt"
guard_run "$work/guard-modules-sink.txt"
check "guard unit: a sink added after the modules still parses and trips new-sink-node, nothing else" \
	sh -c '[ "$1" = ok ] && [ "$2" = new-sink-node ]' sh "$(gfile review)" "$(gfile trip.graph)"
stream_after() {
	printf 'added:\n\tid: 70\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Client (version 3)\n \tproperties:\n \t\tobject.serial = "600"\n \t\tpipewire.sec.pid = "5000"\n\n'
	printf 'added:\n\tid: 71\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n \tstate: "idle"\n \tproperties:\n \t\tobject.serial = "601"\n \t\tnode.name = "own-node"\n \t\tmedia.class = "Stream/Output/Audio"\n\n'
	printf 'added:\n\tid: 72\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Port (version 3)\n \tproperties:\n \t\tobject.serial = "602"\n \t\tnode.id = "71"\n\n'
	printf 'added:\n\tid: 73\n\tpermissions: r-x--\n\ttype: PipeWire:Interface:Link (version 3)\n\toutput-node-id: 71\n\tinput-node-id: 46\n \tstate: "active"\n \tproperties:\n \t\tobject.serial = "603"\n\n'
	printf 'removed:\n\tid: 73\n\nremoved:\n\tid: 72\n\nremoved:\n\tid: 71\n\nremoved:\n\tid: 70\n\n'
}
guard_session stream_after >"$work/guard-modules-stream.txt"
guard_run "$work/guard-modules-stream.txt"
check "guard unit: our stream after the modules is enforced as before (seen, linked active, removed)" \
	sh -c '[ "$1" = none ] && [ "$2" = "yes 1 yes yes none" ]' sh "$(gfile trip.graph)" "$(gfile final)"
# Lines inside args never mark a property change: a marked property there on
# the bound sink is not a props change, a real one after it still is.
args_on_sink() {
	printf 'changed:\n\tid: 46\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n\targs: "{\n*\t\tapi.bluez5.codec = "sbc"\n*\tproperties:\n\n}"\n'
	printf ' \tproperties:\n \t\tobject.serial = "40"\n \t\tnode.name = "bluez_output.x.1"\n \t\tmedia.class = "Audio/Sink"\n\n'
}
guard_session args_on_sink >"$work/guard-args-mark.txt"
guard_run "$work/guard-args-mark.txt"
check "guard unit: args lines never set the property-change mark" [ "$(gfile trip.graph)" = none ]
real_mark() { args_on_sink; printf 'changed:\n\tid: 46\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n*\tproperties:\n*\t\tobject.serial = "40"\n*\t\tapi.bluez5.codec = "sbc"\n\n'; }
guard_session real_mark >"$work/guard-real-mark.txt"
guard_run "$work/guard-real-mark.txt"
check "guard unit: a real marked property change after it still trips" [ "$(gfile trip.graph)" = bound-sink-props-changed ]
open_args() { printf 'added:\n\tid: 3\n\tpermissions: r--m-\n\ttype: PipeWire:Interface:Module (version 3)\n\targs: "{\n            nice.level    = -11\n\n'; }
{ pw_core; pw_module_rt; open_args; } >"$work/guard-open-eof.txt"
guard_run "$work/guard-open-eof.txt"
check "guard unit: a quote still open when pw-mon ends trips graph-observation-malformed" \
	sh -c 'printf "%s\n" "$1" | tr , "\n" | grep -Fxq graph-observation-malformed && printf "%s\n" "$1" | tr , "\n" | grep -Fxq graph-observer-ended' sh "$(gfile trip.graph)"
{ pw_core; open_args; pw_registry; ctl monitor 4242; ctl preflight; ctl stop; } >"$work/guard-open-header.txt"
guard_run "$work/guard-open-header.txt"
check "guard unit: a quote still open at the next event header trips graph-observation-malformed" \
	sh -c 'printf "%s\n" "$1" | tr , "\n" | grep -Fxq graph-observation-malformed' sh "$(gfile trip.graph)"
{ pw_core; printf 'added:\n\tid: 3\n\tpermissions: r--m-\n\ttype: PipeWire:Interface:Module (version 3)\n \tproperties:\n \t\tobject.serial = "3"\n\nstray line outside any block\n\n'; pw_registry; ctl monitor 4242; ctl stop; } >"$work/guard-stray.txt"
guard_run "$work/guard-stray.txt"
check "guard unit: a line outside any block, with no quote open, still trips" [ "$(gfile trip.graph)" = graph-observation-malformed ]

# --- layer 1: fake guest ------------------------------------------------------------
fake="$work/fake-guest"
fbin="$fake/bin"
state="$fake/state"
mkdir -p "$fbin" "$state" "$fake/dt" "$fake/proc" "$fake/spa" "$fake/etc"
printf 'ZUMA PRO TEGU MP on ZUMA PRO\000' >"$fake/dt/model"
printf 'google,ZUMA PRO TEGU\000google,ZUMA PRO\000' >"$fake/dt/compatible"
printf 'FAKEDTSERIAL01\000' >"$fake/dt/serial-number"
proofdir="$work/run-proof"
sed -e "s#^dt_root=/proc/device-tree\$#dt_root=$fake/dt#" -e "s#^proc_root=/proc\$#proc_root=$fake/proc#" \
	-e "s#^runtime_dir=/run/user/1000\$#runtime_dir=$rundir#" -e "s#^apk_db=/lib/apk/db/installed\$#apk_db=$fake/installed#" \
	-e "s#^spa_bluez_plugin=/usr/lib/spa-0.2/bluez5/libspa-bluez5.so\$#spa_bluez_plugin=$fake/spa/libspa-bluez5.so#" \
	-e "s#^passwd_file=/etc/passwd\$#passwd_file=$fake/etc/passwd#" \
	-e "s#^wp_data_dir=/usr/share/wireplumber\$#wp_data_dir=$fake/share/wireplumber#" \
	-e "s#^wp_override_dirs=.*#wp_override_dirs='$fake/etc/xdg/wireplumber $fake/etc/wireplumber $fake/local/wireplumber'#" \
	-e "s#^wp_data_sha256=[0-9a-f]*\$#wp_data_sha256=FAKE_TREE_DIGEST#" \
	"$probe" >"$fake/probe.sh.in"
sed -e "s#^proof_dir=/run/ferrix-a2dp-proof\$#proof_dir=$proofdir#" -e "s#^dt_root=/proc/device-tree\$#dt_root=$fake/dt#" \
	-e "s#^proc_root=/proc\$#proc_root=$fake/proc#" -e "s#^runtime_dir=/run/user/1000\$#runtime_dir=$rundir#" \
	"$body" >"$fake/body.sh"
check "fake guest: only the four root constants differ from the guest body" \
	sh -c '[ "$(diff -u "$1" "$2" | grep -c "^+[^+]")" -eq 4 ]' sh "$body" "$fake/body.sh"

# Every fake tool logs its argv; a verb outside the reviewed set is a violation.
# Each PipeWire client also appears in the fake registry stream, as it would.
cat >"$fbin/registry-client" <<'EOF'
#!/bin/sh
# registry-client add|remove PID: a short-lived client in the fake registry.
s=$FAKE_A2DP_STATE
if [ "$1" = add ]; then
	c=$(cat "$s/next-id" 2>/dev/null || echo 200); echo $((c + 1)) >"$s/next-id"
	echo "$c" >"$s/client.$2"
	printf 'added:\n\tid: %s\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Client (version 3)\n \tproperties:\n \t\tobject.serial = "%s"\n \t\tpipewire.sec.pid = "%s"\n \t\tapplication.name = "tool"\n\n' \
		"$c" "$((c + 1000))" "$2" >>"$s/mon-events"
else
	printf 'removed:\n\tid: %s\n\n' "$(cat "$s/client.$2")" >>"$s/mon-events"
fi
EOF
cat >"$fbin/bluetoothctl" <<'EOF'
#!/bin/sh
s=$FAKE_A2DP_STATE
printf 'bluetoothctl %s\n' "$*" >>"$s/calls"
case "$*" in
	list) cat "$s/bt-list" ;;
	show) cat "$s/bt-show" ;;
	'devices Paired') cat "$s/bt-paired" ;;
	'info '??:??:??:??:??:??)
		f="$s/bt-info.$2"; [ -f "$f" ] || exit 1; cat "$f"
		# Optional: a change between the in-session preflight and the tone.
		n=$(($(cat "$s/info-count" 2>/dev/null || echo 0) + 1)); echo "$n" >"$s/info-count"
		if [ "$n" -eq 2 ] && [ -f "$s/inject-before" ]; then sh "$s/inject-before"; sleep 0.3; fi ;;
	*) printf 'bluetoothctl %s\n' "$*" >>"$s/violations"; exit 1 ;;
esac
EOF
cat >"$fbin/wpctl" <<'EOF'
#!/bin/sh
s=$FAKE_A2DP_STATE
printf 'wpctl %s runtime=%s\n' "$*" "${XDG_RUNTIME_DIR:-unset}" >>"$s/calls"
registry-client add $$
trap 'registry-client remove $$' EXIT
case "$*" in
	status) if [ -f "$s/played" ] && [ -f "$s/wp-status-after" ]; then cat "$s/wp-status-after"; else cat "$s/wp-status"; fi ;;
	'inspect '[0-9]*) f="$s/wp-inspect.$2"; [ -f "$f" ] || exit 1; cat "$f" ;;
	'get-volume '[0-9]*)
		printf '%s\n' "$2" >>"$s/volume-reads"
		if [ -f "$s/wp-volume" ]; then cat "$s/wp-volume"; else echo 'Volume: 0.40'; fi ;;
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
[ -f "$file" ] || exit 1
printf "numid=120,iface=MIXER,name='%s'\n  ; type=BOOLEAN,access=rw------,values=1\n  : values=%s\n" "$name" "$(cat "$file")"
EOF
cat >"$fbin/rc-service" <<'EOF'
#!/bin/sh
s=$FAKE_A2DP_STATE
printf 'rc-service %s\n' "$*" >>"$s/calls"
[ "$#" -eq 2 ] && [ "$2" = status ] || { printf 'rc-service %s\n' "$*" >>"$s/violations"; exit 1; }
EOF
# Fake pw-mon: the pinned 1.6.8 output shape. It prints the core info, the
# scripted initial registry (its own client carries its pid), then follows
# the shared event file until killed.
cat >"$fbin/pw-mon" <<'EOF'
#!/bin/sh
s=$FAKE_A2DP_STATE
printf 'pw-mon %s\n' "$*" >>"$s/calls"
[ "$*" = '-N -a -p' ] || { printf 'pw-mon %s\n' "$*" >>"$s/violations"; exit 1; }
[ ! -f "$s/mon-fail" ] || exit 1
printf '\ttype: PipeWire:Interface:Core\n\tcookie: 1\n\tversion: "1.6.8"\n\tname: "pipewire-0"\n \tproperties:\n \t\tcore.daemon = "true"\n\n'
sed "s/@MONPID@/$$/" "$s/mon-initial"
n=0
while :; do
	total=$(wc -l <"$s/mon-events" | tr -d ' ')
	# A stalled monitor: alive, but it forwards nothing more.
	if [ -f "$s/mon-stall" ]; then sleep 0.02; continue; fi
	if [ "$total" -gt "$n" ]; then sed -n "$((n + 1)),${total}p" "$s/mon-events"; n=$total; fi
	[ ! -f "$s/mon-exit-now" ] || exit 0
	sleep 0.02
done
EOF
# Fake pw-cat: --help, or one playback. It checks the reviewed argv, measures
# the tone (byte count and range only; nothing is kept), adds its client,
# node, port and link to the registry, runs an optional scenario mid-tone,
# then "plays" for 1.5 s. Like the PipeWire server, a reaper removes its
# objects once the process is gone, however it ended (exit or SIGKILL).
cat >"$fbin/pw-cat" <<'EOF'
#!/bin/sh
s=$FAKE_A2DP_STATE
printf 'pw-cat %s\n' "$*" >>"$s/calls"
if [ "$*" = --help ]; then cat "$s/pw-help"; exit 0; fi
[ "$1 $2 $3 $4 $5 $6 $7 $8 $9" = '--playback --raw --rate 48000 --channels 1 --format u8 --target' ] && [ "${11}" = --properties ] &&
	[ "${13}" = - ] && [ "$#" -eq 13 ] || { printf 'pw-cat %s\n' "$*" >>"$s/violations"; exit 1; }
serial=${10}
props=${12}
name=$(printf '%s\n' "$props" | sed -n 's/.*node\.name = "\([^"]*\)".*/\1/p')
printf 'serial=%s\nname=%s\n' "$serial" "$name" >"$s/pw-cat-args"
printf '%s\n' "$props" >"$s/pw-cat-props"
od -An -v -tu1 | awk '{ for (i = 1; i <= NF; i++) { n++; if (min == "" || $i < min) min = $i; if ($i > max) max = $i } }
	END { printf "bytes=%d min=%d max=%d\n", n, min, max }' >"$s/tone-measure"
scenario=$(cat "$s/scenario" 2>/dev/null || echo good)
[ "$scenario" != no-link ] || { sleep 0.3; exit 1; }
printf 'added:\n\tid: 70\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Client (version 3)\n \tproperties:\n \t\tobject.serial = "600"\n \t\tpipewire.sec.pid = "%s"\n\nadded:\n\tid: 71\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n \tinput ports: 0/0\n \toutput ports: 1/1\n \tstate: "idle"\n \tproperties:\n \t\tobject.serial = "601"\n \t\tnode.name = "%s"\n \t\tmedia.class = "Stream/Output/Audio"\n\nadded:\n\tid: 72\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Port (version 3)\n\tdirection: "output"\n \tproperties:\n \t\tobject.serial = "602"\n \t\tnode.id = "71"\n\n' \
	"$$" "$name" >>"$s/mon-events"
me=$$
(
	while kill -0 "$me" 2>/dev/null; do sleep 0.05; done
	[ "$scenario" != late-removal ] || sleep 1.5
	[ "$scenario" = stream-left ] || printf 'removed:\n\tid: 73\n\nremoved:\n\tid: 72\n\nremoved:\n\tid: 71\n\nremoved:\n\tid: 70\n\n' >>"$s/mon-events"
) </dev/null >/dev/null 2>&1 &
in_node=46
[ "$scenario" != link-elsewhere ] || in_node=30
printf 'added:\n\tid: 73\n\tpermissions: r-x--\n\ttype: PipeWire:Interface:Link (version 3)\n\toutput-node-id: 71\n\toutput-port-id: 72\n\tinput-node-id: %s\n\tinput-port-id: 47\n \tstate: "init"\n \tformat:\n\t\tnone\n \tproperties:\n \t\tobject.serial = "603"\n\nchanged:\n\tid: 73\n\tpermissions: r-x--\n\ttype: PipeWire:Interface:Link (version 3)\n\toutput-node-id: 71\n\toutput-port-id: 72\n\tinput-node-id: %s\n\tinput-port-id: 47\n*\tstate: "active"\n*\tformat:\n\t\t  Audio:U8\n \tproperties:\n \t\tobject.serial = "603"\n\nchanged:\n\tid: 46\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n \tinput ports: 2/65\n \toutput ports: 0/0\n*\tstate: "running"\n \tproperties:\n \t\tobject.serial = "40"\n \t\tnode.name = "bluez_output.66_77_88_99_AA_BB.1"\n \t\tmedia.class = "Audio/Sink"\n\n' \
	"$in_node" "$in_node" >>"$s/mon-events"
sleep 0.3
case "$scenario" in
	alsa-sink) printf 'added:\n\tid: 80\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n \tstate: "suspended"\n \tproperties:\n \t\tobject.serial = "610"\n \t\tnode.name = "alsa_output.platform-aoc.HiFi__Speaker__sink"\n \t\tmedia.class = "Audio/Sink"\n\n' >>"$s/mon-events" ;;
	transient-sink) printf 'removed:\n\tid: 81\n\n' >>"$s/mon-events" ;;
	disconnect) printf 'removed:\n\tid: 46\n\n' >>"$s/mon-events" ;;
	device-gone) printf 'removed:\n\tid: 43\n\n' >>"$s/mon-events" ;;
	device-added) printf 'added:\n\tid: 82\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Device (version 3)\n \tproperties:\n \t\tobject.serial = "611"\n \t\tdevice.api = "alsa"\n\n' >>"$s/mon-events" ;;
	foreign-stream) printf 'added:\n\tid: 83\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n \tproperties:\n \t\tobject.serial = "612"\n \t\tnode.name = "other-player"\n \t\tmedia.class = "Stream/Output/Audio"\n\n' >>"$s/mon-events" ;;
	alsa-device-changed) printf 'changed:\n\tid: 42\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Device (version 3)\n \tproperties:\n \t\tobject.serial = "15"\n \t\tdevice.api = "alsa"\n\n' >>"$s/mon-events" ;;
	malformed) printf 'garbage that pw-mon would never print\n\n' >>"$s/mon-events" ;;
	forged-property) printf 'added:\n\tid: 84\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Client (version 3)\n \tproperties:\n \t\tobject.serial = "613"\n \t\tobject.serial = "1"\n\n' >>"$s/mon-events" ;;
	monitor-dies) : >"$s/mon-exit-now" ;;
	pcm-open) mkdir -p "$FAKE_PROC/3/fd"; ln -s /dev/snd/pcmC0D0p "$FAKE_PROC/3/fd/9" ;;
	speaker-on) printf 'on\n' >"$s/ctl.TDM_0_RX_Mixer_EP1" ;;
	amp-unreadable) rm -f "$s/ctl.TAS25XX_ASI_Switch" ;;
	stream-left) : >"$s/stream-left" ;;
	sink-props-changed) printf 'changed:\n\tid: 46\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n*\tproperties:\n*\t\tobject.serial = "40"\n*\t\tnode.name = "bluez_output.66_77_88_99_AA_BB.1"\n*\t\tmedia.class = "Audio/Sink"\n*\t\tapi.bluez5.codec = "sbc"\n\n' >>"$s/mon-events" ;;
esac
if [ "$scenario" = watchdog ]; then sleep 6; fi
sleep 1.2
: >"$s/played"
[ "$scenario" != mon-stall ] || : >"$s/mon-stall"
: >"$s/pw-cat-finished"
EOF
printf '#!/bin/sh\nshift\n[ "$1" != ls ] || [ ! -f "$FAKE_A2DP_STATE/ls-timeout" ] || exit 124\nexec "$@"\n' >"$fbin/timeout"
real_ls=$(command -v ls)
cat >"$fbin/ls" <<EOF
#!/bin/sh
s=\$FAKE_A2DP_STATE
[ ! -f "\$s/ls-denied" ] || echo 'ls: cannot open directory /proc/77/fd/: Permission denied' >&2
exec $real_ls "\$@"
EOF
printf '#!/bin/sh\necho 6.1.124-android14-11-ga4662a84a15f\n' >"$fbin/uname"
printf '#!/bin/sh\nif command -v shasum >/dev/null 2>&1; then exec shasum -a 256 "$@"; fi\nexec /usr/bin/sha256sum "$@"\n' >"$fbin/sha256sum"
printf '#!/bin/sh\necho 0\n' >"$fbin/id"
# The graph guard needs an awk that acts on each FIFO line as it arrives
# (BSD awk and gawk do). mawk, Ubuntu's default awk, fills its input
# buffer first unless run -W interactive, so the guard would never arm and
# every play would refuse; that refusal is itself checked below. The fake
# guest therefore runs mawk interactive. A buffering awk is simulated by
# awk-buffered, which hands the guard its input only at end of file.
real_awk=$(command -v awk)
mawk_bin=''
if awk -W version </dev/null 2>&1 | grep -q '^mawk'; then
	mawk_bin=$real_awk
	printf '#!/bin/sh\nexec %s -W interactive "$@"\n' "$real_awk" >"$fbin/awk"
	real_awk=$fbin/awk
fi
cat >"$fake/awk-buffered" <<EOF
#!/bin/sh
case " \$* " in
*" dir="*) d=\$(cat); printf '%s\n' "\$d" | exec $real_awk "\$@" ;;
esac
exec $real_awk "\$@"
EOF
chmod 0755 "$fbin"/* "$fake/awk-buffered"
export FAKE_A2DP_STATE="$state" FAKE_PROC="$fake/proc"
mkdir -p "$fake/share/wireplumber/wireplumber.conf.d"
printf 'context.modules = []\n' >"$fake/share/wireplumber/wireplumber.conf"
printf 'monitor.bluez.properties = {}\n' >"$fake/share/wireplumber/wireplumber.conf.d/50-bluetooth.conf"
tree_digest=$(cd "$fake/share/wireplumber" && find . ! -type d | LC_ALL=C sort | while IFS= read -r f; do "$fbin/sha256sum" "$f"; done |
	"$fbin/sha256sum" | cut -c1-64)
sed "s#^wp_data_sha256=FAKE_TREE_DIGEST\$#wp_data_sha256=$tree_digest#" "$fake/probe.sh.in" >"$fake/probe.sh"

python3 -c 'import socket, sys, time
s = socket.socket(socket.AF_UNIX); s.bind(sys.argv[1]); s.listen(1); time.sleep(900)' "$rundir/pipewire-0" &
sock_pid=$!
tries=0
while [ ! -S "$rundir/pipewire-0" ]; do
	tries=$((tries + 1))
	[ "$tries" -le 50 ] || { echo 'fake pipewire socket did not appear' >&2; exit 1; }
	sleep 0.1
done

addr=66:77:88:99:AA:BB
addr_us=66_77_88_99_AA_BB
sinks_headset=' │      30. Dummy Output                        [vol: 1.00]
 │  *   46. Synthetic Headphones                [vol: 0.50]'
filters_pair=' │    - loopback-900-12
 │       51. Bluetooth internal capture stream for Synthetic Headphones [Stream/Input/Audio/Internal]
 │       52. Synthetic Headphones                                       [Audio/Source]'
# wp_status FILE SINK_LINES [STREAM_LINES]
wp_status() {
	printf 'PipeWire '"'"'pipewire-0'"'"' [1.6.8, user@ferrix-synth, cookie:12345]\n\nAudio\n ├─ Devices:\n │      42. aoc-snd-card                        [alsa]\n │      43. Synthetic Headphones                [bluez5]\n │\n ├─ Sinks:\n%s\n │\n ├─ Sources:\n │\n ├─ Filters:\n%s\n │\n └─ Streams:\n%s\n\nVideo\n\nSettings\n └─ Default Configured Devices:\n         0. Audio/Sink    bluez_output.%s.1\n' \
		"$2" "$filters_pair" "${3:-}" "$addr_us" >"$1"
}
# inspect_bluez SERIAL [PROFILE]
inspect_bluez() {
	{
		printf 'id 46, type PipeWire:Interface:Node\n    api.bluez5.address = "%s"\n    api.bluez5.codec = "ldac"\n    api.bluez5.profile = "%s"\n' "$addr" "${2:-a2dp-sink}"
		printf '  * device.api = "bluez5"\n    device.id = "43"\n    device.serial = "SYNSERIAL46"\n  * media.class = "Audio/Sink"\n'
		printf '  * node.description = "Synthetic Headphones"\n  * node.name = "bluez_output.%s.1"\n    object.serial = "%s"\n' "$addr_us" "$1"
	} >"$state/wp-inspect.46"
}
# reset_guest: one paired, connected LDAC headset, its bound BlueZ loopback
# pair, no ALSA sink, speaker path off, and the matching fake registry.
reset_guest() {
	rm -rf "${state:?}"/* "$fake/proc"/* "$fake/home" "$fake/etc"/* "$proofdir"
	mkdir -p "$fake/proc/1/fd" "$fake/proc/2/fd" "$fake/home/user" "$fake/etc/wireplumber"
	ln -s /dev/null "$fake/proc/1/fd/0"
	ln -s /dev/snd/pcmC0D8c "$fake/proc/2/fd/7"
	printf 'wireplumber\n' >"$fake/proc/1/comm"
	printf '/usr/bin/wireplumber\000' >"$fake/proc/1/cmdline"
	printf 'XDG_RUNTIME_DIR=/run/user/1000\000HOME=%s\000' "$fake/home/user" >"$fake/proc/1/environ"
	printf 'Name:\twireplumber\nUid:\t1000\t1000\t1000\t1000\n' >"$fake/proc/1/status"
	printf 'sh\n' >"$fake/proc/2/comm"
	printf 'root:x:0:0:root:/root:/bin/sh\nuser:x:1000:1000::%s:/bin/sh\n' "$fake/home/user" >"$fake/etc/passwd"
	: >"$fake/spa/libspa-bluez5.so"
	printf 'P:bluez\nV:5.86-r2\n\nP:pipewire\nV:1.6.8-r4\n\nP:wireplumber\nV:0.5.15-r0\n\nP:pipewire-spa-bluez\nV:1.6.8-r4\n\nP:pipewire-tools\nV:1.6.8-r4\n' >"$fake/installed"
	printf 'Controller 0A:0B:0C:0D:0E:0F ferrix [default]\n' >"$state/bt-list"
	printf 'Controller 0A:0B:0C:0D:0E:0F (public)\n\tPowered: yes\n' >"$state/bt-show"
	printf 'Device %s Synthetic Headphones\n' "$addr" >"$state/bt-paired"
	{
		printf 'Device %s (public)\n\tName: Synthetic Headphones\n\tPaired: yes\n\tConnected: yes\n' "$addr"
		printf '\tUUID: Audio Sink                (0000110b-0000-1000-8000-00805f9b34fb)\n'
	} >"$state/bt-info.$addr"
	printf '      --target                          Set node target\n  -P  --properties                      Set node properties\n' >"$state/pw-help"
	wp_status "$state/wp-status" "$sinks_headset"
	wp_status "$state/wp-status-after" "$sinks_headset"
	printf 'id 30, type PipeWire:Interface:Node\n  * factory.name = "support.null-audio-sink"\n  * media.class = "Audio/Sink"\n  * node.name = "auto_null"\n' >"$state/wp-inspect.30"
	inspect_bluez 40
	{
		printf 'id 51, type PipeWire:Interface:Node\n    bluez5.loopback = "true"\n  * media.class = "Stream/Input/Audio/Internal"\n'
		printf '    node.dont-fallback = "true"\n    node.link-group = "loopback-900-12"\n  * node.name = "bluez_capture_internal.%s"\n' "$addr"
		printf '    node.passive = "true"\n    target.object = "bluez_input.%s.0"\n' "$addr_us"
	} >"$state/wp-inspect.51"
	{
		printf 'id 52, type PipeWire:Interface:Node\n    bluez5.loopback = "true"\n    device.id = "43"\n  * media.class = "Audio/Source"\n'
		printf '    node.link-group = "loopback-900-12"\n  * node.name = "bluez_input.%s"\n' "$addr"
	} >"$state/wp-inspect.52"
	printf 'off\n' >"$state/ctl.TDM_0_RX_Mixer_EP1"
	printf 'off\n' >"$state/ctl.TAS25XX_ASI_Switch"
	: >"$state/mon-events"
	# Initial registry, as pw-mon prints it: the monitor's own client (serial
	# 500) is not first, and one object newer than it comes before it.
	{
		printf 'added:\n\tid: 3\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Metadata (version 3)\n \t\tobject.serial = "5"\n \t\tmetadata.name = "default"\n\n'
		printf 'added:\n\tid: 33\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Client (version 3)\n \tproperties:\n \t\tobject.serial = "10"\n \t\tpipewire.sec.pid = "900"\n\n'
		printf 'added:\n\tid: 42\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Device (version 3)\n \tproperties:\n \t\tobject.serial = "15"\n \t\tdevice.api = "alsa"\n\n'
		printf 'added:\n\tid: 43\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Device (version 3)\n \tproperties:\n \t\tobject.serial = "20"\n \t\tdevice.api = "bluez5"\n\n'
		printf 'added:\n\tid: 30\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n \tstate: "suspended"\n \tproperties:\n \t\tobject.serial = "30"\n \t\tnode.name = "auto_null"\n \t\tmedia.class = "Audio/Sink"\n\n'
		printf 'added:\n\tid: 35\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Client (version 3)\n \tproperties:\n \t\tobject.serial = "500"\n \t\tpipewire.sec.pid = "@MONPID@"\n \t\tremote.intention = "manager"\n\n'
		printf 'added:\n\tid: 46\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n \tinput ports: 2/65\n \tstate: "suspended"\n \tproperties:\n \t\tobject.serial = "40"\n \t\tnode.name = "bluez_output.%s.1"\n \t\tmedia.class = "Audio/Sink"\n\n' "$addr_us"
		printf 'added:\n\tid: 47\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Port (version 3)\n\tdirection: "input"\n \tproperties:\n \t\tobject.serial = "41"\n \t\tnode.id = "46"\n\n'
		printf 'added:\n\tid: 51\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n \tproperties:\n \t\tobject.serial = "60"\n \t\tnode.name = "bluez_capture_internal.%s"\n \t\tmedia.class = "Stream/Input/Audio/Internal"\n\n' "$addr"
		printf 'added:\n\tid: 52\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n \tproperties:\n \t\tobject.serial = "61"\n \t\tnode.name = "bluez_input.%s"\n \t\tmedia.class = "Audio/Source"\n\n' "$addr"
		printf 'changed:\n\tid: 30\n\tpermissions: rwxm-\n\ttype: PipeWire:Interface:Node (version 3)\n*\tstate: "idle"\n \tproperties:\n \t\tobject.serial = "30"\n \t\tnode.name = "auto_null"\n \t\tmedia.class = "Audio/Sink"\n\n'
	} >"$state/mon-initial"
}
# compose MODE SALT EXPECT BODY > FILE: exactly what the host sends.
compose() {
	printf 'proof_mode=%s\nproof_salt=%s\nproof_expect=%s\n' "$1" "$2" "$3"
	cat "$work/library.sh"
	printf 'preflight_body() {\n'
	cat "$fake/probe.sh"
	printf '}\n'
	cat "$4"
}
salt=0123456789abcdef0123456789abcdef
gout=''
gstatus=0
guest() {
	compose "$1" "$salt" "${2:-none}" "$fake/body.sh" >"$work/session.sh"
	gstatus=0
	gout=$(cd "$work" && PATH="$fbin:$PATH" sh "$work/session.sh" 2>"$work/guest.err") || gstatus=$?
	[ -z "${A2DP_TEST_DEBUG:-}" ] || { n=$(($(cat "$A2DP_TEST_DEBUG.n" 2>/dev/null || echo 0) + 1)); echo "$n" >"$A2DP_TEST_DEBUG.n"
		{ printf '%s\n' "$gout"; echo "== stderr"; cat "$work/guest.err"; echo "== calls"; cat "$state/calls" 2>/dev/null || :; echo "== violations"; cat "$state/violations" 2>/dev/null || :; } >"$A2DP_TEST_DEBUG.$n"; }
}
gget() { printf '%s\n' "$gout" | sed -n "s/^$1=//p" | sed -n "${2:-1}p"; }
no_violation() { [ ! -s "$state/violations" ]; }
no_identifier() {
	! printf '%s\n' "$gout" | grep -Eiq "66[:_]77|0A[:_]0B|FAKEDTSERIAL|Synthetic|bluez_output|bluez_input|bluez_capture|loopback-|SYNSERIAL|auto_null|ferrix-a2dp-proof-|$salt|$fake|serial=40|=40\$"
}
played() { [ -f "$state/pw-cat-finished" ]; }
pw_cat_ran() { grep -q '^pw-cat --playback' "$state/calls" 2>/dev/null; }
no_scratch() { [ ! -e "$proofdir" ]; }

reset_guest
guest dry
binding=$(gget binding)
check "guest dry: completes with a binding and the scratch removed" \
	sh -c '[ "$1" -eq 0 ] && [ "$2" = yes ] && printf "%s\n" "$3" | grep -Eqx "[0-9a-f]{64}" && [ "$4" = yes ] && [ "$5" = no ]' sh \
	"$gstatus" "$(gget active_complete)" "$binding" "$(gget scratch_removed)" "$(gget tone_started)"
check "guest dry: the in-session v3 preflight is candidate-observed" \
	sh -c '[ "$1" = candidate-observed ] && [ "$2" = ok ] && [ "$3" = a2dp-sink ] && [ "$4" = ldac ]' sh \
	"$(gget preflight)" "$(gget identity)" "$(gget sink_profile_class)" "$(gget sink_codec)"
check "guest dry: no pw-mon, no playback, no violation, nothing left" sh -c '! grep -Eq "^pw-mon|^pw-cat --playback" "$1" && [ ! -s "$2" ] && [ ! -e "$3" ]' sh \
	"$state/calls" "$state/violations" "$proofdir"
check "guest dry: no address, name, node name, serial, salt, or token leaves the guest" no_identifier
printf '%s\n' "$gout" >"$work/fx-dry"
guest dry
check "guest dry: the binding is stable for the same salt and graph" [ "$(gget binding)" = "$binding" ]
salt=ffffffffffffffffffffffffffffffff
guest dry
check "guest dry: another decision salt gives an unrelated binding" [ "$(gget binding)" != "$binding" ]
salt=0123456789abcdef0123456789abcdef
printf 'FAKEDTSERIAL02\000' >"$fake/dt/serial-number"
guest dry
check "guest dry: another phone (device-tree serial) gives another binding" [ "$(gget binding)" != "$binding" ]
printf 'FAKEDTSERIAL01\000' >"$fake/dt/serial-number"
check "guest dry: the bound sink volume is read once, reported, and under the cap" \
	sh -c '[ "$1" = 0.40 ] && [ "$2" = no ] && [ "$(grep -c . "$3")" -ge 1 ]' sh "$(sed -n 's/^sink_volume=//p' "$work/fx-dry")" \
	"$(sed -n 's/^sink_mute=//p' "$work/fx-dry")" "$state/volume-reads"
reset_guest
printf 'Volume: 0.50\n' >"$state/wp-volume"
guest dry
check "guest dry: a volume exactly at the cap is accepted" sh -c '[ "$1" -eq 0 ] && [ "$2" = 0.50 ]' sh "$gstatus" "$(gget sink_volume)"
reset_guest
printf 'Volume: 0.51\n' >"$state/wp-volume"
guest dry
check "guest dry: a volume over the cap refuses before any decision" \
	sh -c '[ "$1" -eq 6 ] && [ "$2" = sink-volume-over-cap ] && [ "$3" = 0.51 ] && [ -z "$4" ]' sh "$gstatus" "$(gget active_refusal)" "$(gget sink_volume)" "$(gget binding)"

reset_guest
guest play "$binding"
check "guest play: completes after one tone" sh -c '[ "$1" -eq 0 ] && [ "$2" = yes ] && [ "$3" = yes ]' sh "$gstatus" "$(gget active_complete)" "$(gget tone_started)"
check "guest play: every gate held before the tone" \
	sh -c '[ "$1" = yes ] && [ "$2" = yes ] && [ "$3" = yes ] && [ "$4" = ok ]' sh \
	"$(gget binding_match)" "$(gget guard_armed)" "$(gget kernel_guards_armed)" "$(gget guard_review)"
check "guest play: pw-cat played to the end, linked only to the bound sink, link active" \
	sh -c '[ "$1" = 0 ] && [ "$2" = idle ] && [ "$3" = complete ] && [ "$4" = yes ] && [ "$5" = 1 ] && [ "$6" = yes ] && [ "$7" = yes ]' sh \
	"$(gget tone_exit)" "$(gget tone_watchdog)" "$(gget graph_final)" "$(gget graph_own_node)" "$(gget graph_own_links)" "$(gget graph_link_active)" \
	"$(gget graph_own_removed)"
check "guest play: the volume was read in the play session, after the review and before pw-cat" \
	sh -c '[ "$1" = 0.40 ] && [ "$2" = no ] && grep -n "" "$3" | awk -F: "/^[0-9]+:wpctl get-volume/ { v = \$1 } /^[0-9]+:pw-cat --playback/ { p = \$1 } END { exit !(v && p && v < p) }"' sh \
	"$(gget sink_volume)" "$(gget sink_mute)" "$state/calls"
check "guest play: no guard tripped" \
	sh -c '[ "$1" = none ] && [ "$2" = none ] && [ "$3" = none ] && [ "$4" = none ]' sh \
	"$(gget guard_trip_graph)" "$(gget guard_trip_pcm)" "$(gget guard_trip_ctl)" "$(gget guard_trip_live)"
check "guest play: the tone was 72000 bytes of u8 between 115 and 141" grep -Fxq 'bytes=72000 min=115 max=141' "$state/tone-measure"
check "guest play: pw-cat targeted the bound sink serial with the reviewed properties" \
	sh -c 'grep -Fxq serial=40 "$1" && grep -Eq "^name=ferrix-a2dp-proof-[0-9a-f]{16}$" "$1" &&
		grep -Fq "target.object = \"40\" node.dont-fallback = true node.dont-reconnect = true node.dont-move = true node.linger = false state.restore-props = false state.restore-target = false" "$2"' \
	sh "$state/pw-cat-args" "$state/pw-cat-props"
check "guest play: a final v3 observation ran and the scratch is gone" \
	sh -c '[ "$1" = 0 ] && [ "$2" = yes ] && [ "$(printf "%s\n" "$3" | grep -c "^probe=a2dp-output-preflight$")" -eq 2 ] && [ ! -e "$4" ]' sh \
	"$(gget post_exit)" "$(gget scratch_removed)" "$gout" "$proofdir"
check "guest play: only reviewed verbs ran" no_violation
check "guest play: no address, name, node name, serial, salt, or token leaves the guest" no_identifier
check "guest play: amixer only read the watched controls" sh -c '[ "$(grep "^amixer " "$1" | grep -vc " cget ")" -eq 0 ]' sh "$state/calls"
printf '%s\n' "$gout" >"$work/fx-play-good"

# play_case NAME WANT_REFUSAL SETUP...: a refusal before any tone.
refusal_case() {
	label=$1 want=$2
	shift 2
	reset_guest
	"$@"
	guest play "${expect:-$binding}"
	check "guest refuses ($label): exit 6, $want, nothing played" \
		sh -c '[ "$1" -eq 6 ] && [ "$2" = "$3" ] && [ "$4" = refused ] && [ "$5" = no ]' sh \
		"$gstatus" "$(gget active_refusal)" "$want" "$(gget active_complete)" "$(gget tone_started)"
	check "guest refuses ($label): pw-cat never played, no scratch of its own left, no violation" \
		sh -c '! grep -qs "^pw-cat --playback" "$1" && { [ "$4" = scratch-left-over ] || [ ! -e "$2" ]; } && [ ! -s "$3" ]' sh \
		"$state/calls" "$proofdir" "$state/violations" "$label"
	check "guest refuses ($label): no identifier leaves the guest" no_identifier
	printf '%s\n' "$gout" >"$work/fx-refuse-$label"
	expect=''
}
expect=0000000000000000000000000000000000000000000000000000000000000000
refusal_case binding-mismatch binding-mismatch :
check "guest refuses (binding-mismatch): every guard reason travels with the refusal, none tripped" \
	sh -c '[ "$1" = none ] && [ "$2" = none ] && [ "$3" = none ] && [ "$4" = none ]' sh \
	"$(gget guard_trip_graph)" "$(gget guard_trip_pcm)" "$(gget guard_trip_ctl)" "$(gget guard_trip_live)"
reserial() { inspect_bluez 41; }
refusal_case reconnected-sink-skew binding-mismatch reserial
codec() { sed -i.bak 's/"ldac"/"sbc"/' "$state/wp-inspect.46"; }
refusal_case codec-skew binding-mismatch codec
kernel() { printf '#!/bin/sh\necho 6.1.124-android14-11-other\n' >"$fbin/uname"; }
refusal_case kernel-skew preflight-blocked kernel
printf '#!/bin/sh\necho 6.1.124-android14-11-ga4662a84a15f\n' >"$fbin/uname"
version() { sed -i.bak 's/^V:0.5.15-r0$/V:0.5.16-r0/' "$fake/installed"; }
refusal_case version-skew preflight-blocked version
alsa_present() {
	wp_status "$state/wp-status" "$sinks_headset
 │      50. Speaker                             [vol: 0.40]"
	printf 'id 50, type PipeWire:Interface:Node\n  * device.api = "alsa"\n  * media.class = "Audio/Sink"\n  * node.name = "alsa_output.platform-aoc.HiFi__Speaker__sink"\n' >"$state/wp-inspect.50"
}
refusal_case alsa-sink-at-start preflight-blocked alsa_present
speaker() { printf 'on\n' >"$state/ctl.TDM_0_RX_Mixer_EP1"; }
refusal_case speaker-on-at-start preflight-blocked speaker
leftover() { mkdir -p "$proofdir"; }
refusal_case scratch-left-over proof-dir-left-over leftover
check "guest refuses (scratch-left-over): no guard ran, so no guard reason is read from a directory it did not make" \
	sh -c '! printf "%s\n" "$1" | grep -q "^guard_trip_"' sh "$gout"
check "guest refuses (scratch-left-over): an existing scratch directory is not removed" [ -d "$proofdir" ]
rm -rf "$proofdir"
monfail() { : >"$state/mon-fail"; }
refusal_case graph-guard-cannot-arm graph-guard-not-armed monfail
cp "$fbin/awk" "$work/awk.saved" 2>/dev/null || rm -f "$work/awk.saved"
restore_awk() { if [ -f "$work/awk.saved" ]; then cp "$work/awk.saved" "$fbin/awk"; else rm -f "$fbin/awk"; fi; }
buffered_awk() { cp "$fake/awk-buffered" "$fbin/awk"; }
refusal_case buffering-awk graph-guard-not-armed buffered_awk
restore_awk
if [ -n "$mawk_bin" ]; then
	plain_mawk() { printf '#!/bin/sh\nexec %s "$@"\n' "$mawk_bin" >"$fbin/awk"; }
	refusal_case non-interactive-mawk graph-guard-not-armed plain_mawk
	restore_awk
fi
inject_alsa() {
	printf 'printf '"'"'added:\\n\\tid: 80\\n\\tpermissions: rwxm-\\n\\ttype: PipeWire:Interface:Node (version 3)\\n \\tproperties:\\n \\t\\tobject.serial = "700"\\n \\t\\tnode.name = "alsa_output.x"\\n \\t\\tmedia.class = "Audio/Sink"\\n\\n'"'"' >>"$FAKE_A2DP_STATE/mon-events"\n' >"$state/inject-before"
}
refusal_case sink-added-after-preflight graph-changed-before-tone inject_alsa
inject_transient() { printf 'printf "removed:\\n\\tid: 99\\n\\n" >>"$FAKE_A2DP_STATE/mon-events"\n' >"$state/inject-before"; }
refusal_case transient-before-tone graph-changed-before-tone inject_transient
inject_pcm() { printf 'mkdir -p "$FAKE_PROC/4/fd"; ln -s /dev/snd/pcmC0D0p "$FAKE_PROC/4/fd/3"\n' >"$state/inject-before"; }
refusal_case pcm-opened-after-preflight kernel-guard-tripped-before-tone inject_pcm
check "guest refuses (pcm-opened-after-preflight): the transcript carries the pcm guard reason" \
	sh -c '[ "$1" = none ] && [ "$2" = playback-pcm-opener ] && [ "$3" = none ] && [ "$4" = none ]' sh \
	"$(gget guard_trip_graph)" "$(gget guard_trip_pcm)" "$(gget guard_trip_ctl)" "$(gget guard_trip_live)"
inject_garbage() { printf 'printf "garbage that pw-mon would never print\\n\\n" >>"$FAKE_A2DP_STATE/mon-events"\n' >"$state/inject-before"; }
refusal_case graph-malformed-before-tone graph-guard-tripped-before-tone inject_garbage
check "guest refuses (graph-malformed-before-tone): named for the graph guard, which records the parse failure" \
	sh -c '[ "$1" = graph-observation-malformed ] && [ "$2" = none ] && [ "$3" = none ]' sh \
	"$(gget guard_trip_graph)" "$(gget guard_trip_pcm)" "$(gget guard_trip_ctl)"
two_sinks() {
	printf 'Device %s A\nDevice 00:11:22:33:44:55 B\n' "$addr" >"$state/bt-paired"
	sed "s/$addr/00:11:22:33:44:55/" "$state/bt-info.$addr" >"$state/bt-info.00:11:22:33:44:55"
}
refusal_case second-headset preflight-blocked two_sinks
loud() { printf 'Volume: 0.80\n' >"$state/wp-volume"; }
refusal_case volume-over-cap sink-volume-over-cap loud
muted() { printf 'Volume: 0.30 [MUTED]\n' >"$state/wp-volume"; }
refusal_case volume-muted sink-muted muted
vol_percent() { printf 'Volume: 40%%\n' >"$state/wp-volume"; }
refusal_case volume-unparseable sink-volume-unreadable vol_percent
vol_two() { printf 'Volume: 0.10\nVolume: 0.90\n' >"$state/wp-volume"; }
refusal_case volume-two-lines sink-volume-unreadable vol_two
vol_none() { : >"$state/wp-volume"; }
refusal_case volume-unsupported sink-volume-unreadable vol_none
vol_big() { printf 'Volume: 10.00\n' >"$state/wp-volume"; }
refusal_case volume-out-of-range sink-volume-unreadable vol_big

# trip_case NAME SCENARIO GUARD WANT: a tone that a guard must stop.
trip_case() {
	label=$1 scenario=$2 guard=$3 want=$4
	reset_guest
	printf '%s\n' "$scenario" >"$state/scenario"
	guest play "$binding"
	got=$(gget "guard_trip_$guard")
	check "guest trips ($label): the $guard guard records $want and kills pw-cat" \
		sh -c '[ "$1" -eq 0 ] && [ "$2" = yes ] && printf "%s\n" "$3" | tr , "\n" | grep -Fxq "$4" && [ "$5" = 137 ] && [ ! -f "$6" ]' sh \
		"$gstatus" "$(gget tone_started)" "$got" "$want" "$(gget tone_exit)" "$state/pw-cat-finished"
	[ "$(printf '%s\n' "$got" | tr , '\n' | grep -Fxc "$want")" -eq 1 ] || printf '     got %s=%s tone_exit=%s\n' "$guard" "$got" "$(gget tone_exit)"
	check "guest trips ($label): final observation ran, scratch removed, no identifier" \
		sh -c '[ "$1" = 0 ] && [ "$2" = yes ]' sh "$(gget post_exit)" "$(gget scratch_removed)"
	check "guest trips ($label): no identifier leaves the guest" no_identifier
	printf '%s\n' "$gout" >"$work/fx-trip-$label"
}
trip_case alsa-sink-appears alsa-sink graph new-sink-node
trip_case transient-object-never-seen transient-sink graph unobserved-object-removed
trip_case link-to-other-node link-elsewhere graph link-outside-bound-sink
trip_case headset-disconnect disconnect graph bound-sink-removed
trip_case bluez-device-removed device-gone graph bound-device-removed
trip_case device-added device-added graph device-added
trip_case foreign-stream foreign-stream graph foreign-node
trip_case alsa-device-profile-change alsa-device-changed graph graph-object-changed
trip_case malformed-monitor-line malformed graph graph-observation-malformed
trip_case forged-duplicate-property forged-property graph graph-observation-malformed
trip_case monitor-exits monitor-dies live guard-exited
trip_case playback-pcm-opened pcm-open pcm playback-pcm-opener
trip_case speaker-route-on speaker-on ctl speaker-control-not-off
trip_case amp-switch-unreadable amp-unreadable ctl speaker-control-read-failed
trip_case bound-sink-props-changed sink-props-changed graph bound-sink-props-changed

reset_guest
{ pw_module_rt; pw_module_native; } >>"$state/mon-initial"
guest play "$binding"
check "guest play (startup registry holds Modules with multi-line args): arms, reviews clean, route held" \
	sh -c '[ "$1" -eq 0 ] && [ "$2" = ok ] && [ "$3" = none ] && [ "$4" = yes ] && [ "$5" = 0 ]' sh \
	"$gstatus" "$(gget guard_review)" "$(gget guard_trip_graph)" "$(gget graph_own_removed)" "$(gget tone_exit)"
reset_guest
printf 'no-link\n' >"$state/scenario"
guest play "$binding"
check "guest play (target never linked): no trip, but no own node seen added or removed" \
	sh -c '[ "$1" -eq 0 ] && [ "$2" = 1 ] && [ "$3" = no ] && [ "$4" = 0 ] && [ "$5" = none ] && [ "$6" = no ]' sh \
	"$gstatus" "$(gget tone_exit)" "$(gget graph_own_node)" "$(gget graph_own_links)" "$(gget guard_trip_graph)" "$(gget graph_own_removed)"
printf '%s\n' "$gout" >"$work/fx-play-no-link"
reset_guest
printf 'late-removal\n' >"$state/scenario"
guest play "$binding"
check "guest play (removal events 1.5 s late): the bounded wait still sees the stream removed" \
	sh -c '[ "$1" -eq 0 ] && [ "$2" = 0 ] && [ "$3" = yes ] && [ "$4" = yes ] && [ "$5" = none ]' sh \
	"$gstatus" "$(gget tone_exit)" "$(gget graph_own_node)" "$(gget graph_own_removed)" "$(gget guard_trip_graph)"
printf '%s\n' "$gout" >"$work/fx-play-late-removal"
reset_guest
printf 'mon-stall\n' >"$state/scenario"
guest play "$binding"
check "guest play (pw-mon stalls before the removal): no trip, but the removal is reported unseen" \
	sh -c '[ "$1" -eq 0 ] && [ "$2" = 0 ] && [ "$3" = yes ] && [ "$4" = no ] && [ "$5" = none ] && [ "$6" = complete ]' sh \
	"$gstatus" "$(gget tone_exit)" "$(gget graph_own_node)" "$(gget graph_own_removed)" "$(gget guard_trip_graph)" "$(gget graph_final)"
printf '%s\n' "$gout" >"$work/fx-play-mon-stall"
reset_guest
printf 'watchdog\n' >"$state/scenario"
guest play "$binding"
check "guest play (player hangs): the watchdog kills it within the wall bound" \
	sh -c '[ "$1" = fired ] && [ "$2" = 137 ] && [ "$3" = none ]' sh "$(gget tone_watchdog)" "$(gget tone_exit)" "$(gget guard_trip_graph)"
printf '%s\n' "$gout" >"$work/fx-play-watchdog"
reset_guest
printf 'stream-left\n' >"$state/scenario"
wp_status "$state/wp-status-after" "$sinks_headset" '        70. ferrix
             71. output_MONO     > Synthetic Headphones:playback_FL	[active]'
guest play "$binding"
check "guest play (stream left behind): the final observation sees it" \
	sh -c '[ "$(printf "%s\n" "$1" | sed -n "/^active_section=post-begin$/,/^active_section=post-end$/p" | grep -c "^graph_streams=1$")" -eq 1 ]' sh "$gout"
printf '%s\n' "$gout" >"$work/fx-play-stream-left"

# --- layer 2: host workflow against layer-1 output ----------------------------------
compose_body() { cat "$work/library.sh"; printf 'preflight_body() {\n'; cat "$probe"; printf '}\n'; cat "$body"; }
compose_body >"$work/expected-body.sh"
acm_guest_init "$root" "$work" "$work/expected-body.sh"
mkdir -p "$work/fx"
for f in "$work"/fx-*; do cp "$f" "$work/fx/${f##*/fx-}"; done
derive() { src=$1 name=$2; shift 2; sed "$@" "$work/fx/$src" >"$work/fx/$name"; }
derive play-good leak-address -e "s/^sink_codec=.*/sink_codec=$addr/"
derive play-good leak-hex -e 's/^sink_codec=.*/sink_codec=66778899AABB/'
derive play-good unexpected-key -e 's/^active_complete=yes$/sink_serial=40\nactive_complete=yes/'
derive play-good duplicate-key -e 's/^tone_exit=0$/tone_exit=0\ntone_exit=0/'
derive play-good truncated -e '/^active_complete=/d'
derive play-good no-post -e '/^active_section=post-begin$/,/^active_section=post-end$/d'
derive play-good post-speaker-on -e '/^active_section=post-begin$/,/^active_section=post-end$/s/^watch.tdm_0_rx_mixer_ep1=off$/watch.tdm_0_rx_mixer_ep1=on/'
first() { awk -v k="$3" -v v="$4" '!done && index($0, k "=") == 1 { print v; done = 1; next } { print }' "$work/fx/$1" >"$work/fx/$2"; }
first play-good pre-blocked graph_sink_alsa graph_sink_alsa=1
first play-good pre-malformed graph_sinks 'graph_sinks=2\ngraph_sinks=2'
derive play-good skipped-review -e 's/^guard_review=ok$/guard_review=foreign-node/'
derive play-good loud-claimed -e 's/^sink_volume=.*/sink_volume=0.90/'
derive play-good muted-claimed -e 's/^sink_mute=.*/sink_mute=yes/'
derive play-good no-removal -e 's/^graph_own_removed=yes$/graph_own_removed=no/'
derive dry dry-loud -e 's/^sink_volume=.*/sink_volume=0.75/'
derive dry dry-bad-binding -e 's/^binding=.*/binding=abc/'
first dry dry-wrong-board dt_model 'dt_model=ZUMA PRO COMET MP on ZUMA PRO'

out=''
status=0
# run CASE FIXTURE EXIT MODE [CONFIRM]
run() {
	case_name=$1 fx=$2 st=$3 mode=$4
	case_dir="$outroot/run-$case_name"
	mkdir -p "$case_dir"
	acm_guest_start
	acm_guest_ctl header_lines 3
	acm_guest_ctl fixture "$work/fx/$fx"
	acm_guest_ctl exit "$st"
	status=0
	out=$(FERRIX_OUT_DIR="${out_override:-$case_dir}" FERRIX_PIXEL9A_A2DP_EVIDENCE="${run_evidence:-mock}" FERRIX_PIXEL9A_A2DP_PLAY_CONFIRM="${5:-}" \
		sh "$host" "$mode" 2>&1) || status=$?
	runs=$(acm_guest_runs)
	acm_guest_stop
	results="$case_dir/pixel-9a/a2dp-active-proof"
}
has() { printf '%s\n' "$out" | grep -Fxq "$1"; }
phrase_of() { printf '%s\n' "$1" | sed -n "s/^  FERRIX_PIXEL9A_A2DP_PLAY_CONFIRM='\\(.*\\)' tools.*/\\1/p"; }
leak_re='66[:_]77|Synthetic|bluez_(output|input|capture)|SYNSERIAL|ferrix-a2dp-proof-[0-9a-f]'
private() {
	if printf '%s\n' "$out" | grep -Eiq "$leak_re" || grep -rEiq "$leak_re" "$1"; then
		{ printf '%s\n' "$out"; cat "$1"/*; } | grep -Ei "$leak_re" | sed -n '1,3p'
		return 1
	fi
}

run dry dry 0 dry
phrase=$(phrase_of "$out")
salt_used=$(sed -n 's/^proof_salt=//p' "$ACM_GUEST_CTL/header.1")
check "host dry: exit 0 after one session, decision ready, nothing played" \
	sh -c '[ "$1" -eq 0 ] && [ "$2" -eq 1 ] && printf "%s\n" "$3" | grep -Fxq decision_status=ready && printf "%s\n" "$3" | grep -Fxq tone_started=no' sh "$status" "$runs" "$out"
check "host dry: the guest got the exact composed body after a dry header" \
	sh -c 'cmp -s "$1" "$2" && grep -Fxq proof_mode=dry "$3" && grep -Fxq proof_expect=none "$3"' sh "$ACM_GUEST_CTL/body.1" "$work/expected-body.sh" "$ACM_GUEST_CTL/header.1"
check "host dry: the phrase binds the binding prefix and a decision id" \
	sh -c 'printf "%s\n" "$1" | grep -Eqx "play one 1.5 s 750 Hz tone on the bound A2DP sink binding $2 decision [0-9a-f]{8} on google-pixel-9a-tegu-gxq96-mp1-0"' sh \
	"$phrase" "$(printf '%s' "$binding" | cut -c1-12)"
check "host dry: the decision file is private and holds the salt; no transcript or output does" \
	sh -c '[ "$(ls -l "$1/decision-pending" | cut -c1-10)" = "-rw-------" ] && grep -Fxq "salt=$2" "$1/decision-pending" &&
		! printf "%s\n" "$3" | grep -Fq "$2" && ! grep -rFq "$2" "$1"/dry-*' sh "$results" "$salt_used" "$out"
check "host dry: privacy of output and transcripts" private "$results"
check "host dry: the decision records its mock origin; the summary carries the volume" \
	sh -c 'grep -Fxq evidence=mock "$1/decision-pending" && printf "%s\n" "$2" | grep -Fxq sink_volume=0.40 &&
		printf "%s\n" "$2" | grep -Fq "sink volume 0.40 (read only; play refuses muted or above 0.50)"' sh "$results" "$out"
check "host dry: prints the decision context" \
	sh -c 'printf "%s\n" "$1" | grep -Fq "Decision context (dry run; nothing was played" && printf "%s\n" "$1" | grep -Fq "node.dont-move" &&
		printf "%s\n" "$1" | grep -Fq "Internal speaker and earpiece stay NO-GO"' sh "$out"

# Lifecycle: play consumes the decision exactly once.
play_dir="$outroot/run-play-good"
mkdir -p "$play_dir/pixel-9a"
cp -R "$results" "$play_dir/pixel-9a/"
run play-good play-good 0 play "$phrase"
check "host play: route held, tone delivered, cleanup clean, audibility owner-pending, exit 0" \
	sh -c '[ "$1" -eq 0 ] && printf "%s\n" "$2" | grep -Fxq route_safety=held && printf "%s\n" "$2" | grep -Fxq tone_delivery=linked-active &&
		printf "%s\n" "$2" | grep -Fxq cleanup=clean && printf "%s\n" "$2" | grep -Fxq audible_confirmation=owner-pending &&
		printf "%s\n" "$2" | grep -Fxq result=route-held-tone-delivered && printf "%s\n" "$2" | grep -Fxq audio_capability=unsupported' sh "$status" "$out"
check "host play: the observed sink volume is in the redacted summary" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq sink_volume=0.40 && printf "%s\n" "$1" | grep -Fxq sink_mute=no && grep -Fxq sink_volume=0.40 "$2"/play-latest.txt' sh "$out" "$results"
check "host play: the session carried the decision's salt and binding" \
	sh -c 'grep -Fxq proof_mode=play "$1" && grep -Fxq "proof_salt=$2" "$1" && grep -Fxq "proof_expect=$3" "$1" && cmp -s "$4" "$5"' sh \
	"$ACM_GUEST_CTL/header.1" "$salt_used" "$binding" "$ACM_GUEST_CTL/body.1" "$work/expected-body.sh"
check "host play: the decision is consumed and its salt erased" \
	sh -c '[ ! -e "$1/decision-pending" ] && ls "$1"/decision-consumed-* >/dev/null && ! grep -rFq "$2" "$1"' sh "$results" "$salt_used"
check "host play: privacy of output and transcripts" private "$results"
run play-good play-good 0 play "$phrase"
check "host play again: the same phrase is refused without a session" \
	sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 0 ] && printf "%s\n" "$3" | grep -Fq "no pending decision"' sh "$status" "$runs" "$out"
run play-without-dry play-good 0 play "$phrase"
check "host play without a dry run: refused, exit 2, no session" sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 0 ]' sh "$status" "$runs"
run play-no-phrase play-good 0 play ''
check "host play without a phrase: usage, exit 2, no session" sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 0 ]' sh "$status" "$runs"

# fresh_decision CASE: a dry run in CASE's out dir; sets phrase.
fresh_decision() {
	run "$1" dry 0 dry
	phrase=$(phrase_of "$out")
}
fresh_decision wrong-phrase
run wrong-phrase play-good 0 play "$(printf '%s' "$phrase" | sed 's/decision [0-9a-f]*/decision 00000000/')"
check "host play with another decision's phrase: refused, exit 2, not consumed" \
	sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 0 ] && [ -f "$3/decision-pending" ]' sh "$status" "$runs" "$results"
fresh_decision stale
sed -i.bak 's/^created=.*/created=1000/' "$results/decision-pending"
run stale play-good 0 play "$phrase"
check "host play with a stale decision: refused, exit 2, consumed" \
	sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 0 ] && [ ! -e "$3/decision-pending" ] && printf "%s\n" "$4" | grep -Fq "is now consumed"' sh "$status" "$runs" "$results" "$out"
fresh_decision tampered
sed -i.bak 's/^salt=.*/salt=zz/' "$results/decision-pending"
run tampered play-good 0 play "$phrase"
check "host play with a malformed decision: refused, exit 2" sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 0 ]' sh "$status" "$runs"
fresh_decision linked
mv "$results/decision-pending" "$results/real-decision"
ln -s real-decision "$results/decision-pending"
run linked play-good 0 play "$phrase"
check "host play with a linked decision: refused, exit 2" sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 0 ]' sh "$status" "$runs"
fresh_decision mock-origin
run_evidence=physical
run mock-origin play-good 0 play "$phrase"
run_evidence=''
check "host physical play with a mock-origin decision: refused, exit 2, no session, not consumed" \
	sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 0 ] && [ -f "$3/decision-pending" ] && printf "%s\n" "$4" | grep -Fq "came from a mock run"' sh \
	"$status" "$runs" "$results" "$out"
fresh_decision no-origin
sed -i.bak '/^evidence=/d' "$results/decision-pending"
run no-origin play-good 0 play "$phrase"
check "host play with a decision that records no origin: refused, exit 2, no session" sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 0 ]' sh "$status" "$runs"
mkdir -p "$work/outside"
out_override="$work/outside"
run outside dry 0 dry
out_override=''
check "host refuses a FERRIX_OUT_DIR outside the repository out/: exit 2, no session, nothing written" \
	sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 0 ] && [ -z "$(ls -A "$3")" ]' sh "$status" "$runs" "$work/outside"
ln -s "$work/outside" "$outroot/linked-out"
out_override="$outroot/linked-out"
run linked-out dry 0 dry
out_override=''
check "host refuses an out/ path that links outside the repository: exit 2, no session, nothing written" \
	sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 0 ] && [ -z "$(ls -A "$3")" ]' sh "$status" "$runs" "$work/outside"
fresh_decision redo
run redo dry 0 dry
check "host dry again: replaces the pending decision (the earlier phrase no longer works)" [ "$(phrase_of "$out")" != "$phrase" ]

# play_fixture CASE FIXTURE EXIT: a play session answered by FIXTURE.
play_fixture() {
	fresh_decision "$1"
	run "$1" "$2" "$3" play "$phrase"
}
for item in alsa-sink-appears:graph:new-sink-node headset-disconnect:graph:bound-sink-removed \
	link-to-other-node:graph:link-outside-bound-sink transient-object-never-seen:graph:unobserved-object-removed \
	monitor-exits:live:guard-exited playback-pcm-opened:pcm:playback-pcm-opener speaker-route-on:ctl:speaker-control-not-off; do
	label=${item%%:*}
	rest=${item#*:}
	play_fixture "trip-$label" "trip-$label" 0
	check "host play ($label): route tripped, exit 7, never audibly confirmed" \
		sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq route_safety=tripped && printf "%s\n" "$2" | grep -Eq "^route_trips=.*$3" &&
			printf "%s\n" "$2" | grep -Fxq result=route-not-verified && printf "%s\n" "$2" | grep -Fxq audible_confirmation=owner-pending' sh \
		"$status" "$out" "${rest%%:*}:[a-z,-]*${rest#*:}"
done
play_fixture speaker-cleanup trip-speaker-route-on 0
check "host play (speaker route on afterwards): cleanup anomaly reported" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq cleanup=anomaly && printf "%s\n" "$1" | grep -Fq speaker-route-not-off' sh "$out"
play_fixture no-link play-no-link 0
check "host play (never linked): the guard never saw our node, so route unverified, not delivered, exit 7" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq route_safety=unverified && printf "%s\n" "$2" | grep -Fxq tone_delivery=not-delivered &&
		printf "%s\n" "$2" | grep -Fxq graph_own_node=no' sh "$status" "$out"
play_fixture mon-stall play-mon-stall 0
check "host play (pw-mon stalled, removal unseen): route unverified, exit 7, never held" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq route_safety=unverified && ! printf "%s\n" "$2" | grep -Fxq route_safety=held &&
		printf "%s\n" "$2" | grep -Fxq graph_own_removed=no && printf "%s\n" "$2" | grep -Fxq result=route-not-verified' sh "$status" "$out"
play_fixture no-removal no-removal 0
check "host play (removal not reported, otherwise good): route unverified, exit 7" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq route_safety=unverified' sh "$status" "$out"
play_fixture late-removal play-late-removal 0
check "host play (late removal, seen within the bound): route held, delivered, exit 0" \
	sh -c '[ "$1" -eq 0 ] && printf "%s\n" "$2" | grep -Fxq route_safety=held' sh "$status" "$out"
play_fixture watchdog play-watchdog 0
check "host play (watchdog): not delivered, exit 8" sh -c '[ "$1" -eq 8 ] && printf "%s\n" "$2" | grep -Fxq tone_watchdog=fired' sh "$status" "$out"
play_fixture stream-left play-stream-left 0
check "host play (stream left): cleanup anomaly, exit 7" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq cleanup=anomaly && printf "%s\n" "$2" | grep -Fq stream-left' sh "$status" "$out"
play_fixture post-speaker-on post-speaker-on 0
check "host play (final observation shows the speaker route on): exit 7" sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fq speaker-route-not-off' sh "$status" "$out"
play_fixture no-post no-post 0
check "host play (final observation missing): exit 7" sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fq final-observation-missing' sh "$status" "$out"
for item in binding-mismatch:binding-mismatch sink-added-after-preflight:graph-changed-before-tone \
	alsa-sink-at-start:preflight-blocked graph-guard-cannot-arm:graph-guard-not-armed; do
	label=${item%%:*}
	play_fixture "refuse-$label" "refuse-$label" 6
	check "host play (guest refuses $label): exit 6, nothing played, decision still consumed" \
		sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq "active_refusal=$3" && printf "%s\n" "$2" | grep -Fxq tone_started=no && [ ! -e "$4/decision-pending" ]' sh \
		"$status" "$out" "${item#*:}" "$results"
done
for item in volume-over-cap:sink-volume-over-cap volume-muted:sink-muted volume-unparseable:sink-volume-unreadable; do
	label=${item%%:*}
	play_fixture "refuse-$label" "refuse-$label" 6
	check "host play (guest refuses $label): exit 6, nothing played, reason reported" \
		sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq "active_refusal=$3" && printf "%s\n" "$2" | grep -Fxq tone_started=no' sh \
		"$status" "$out" "${item#*:}"
done
for item in pcm-opened-after-preflight:kernel-guard-tripped-before-tone:pcm:playback-pcm-opener \
	graph-malformed-before-tone:graph-guard-tripped-before-tone:graph:graph-observation-malformed; do
	label=${item%%:*}
	rest=${item#*:}
	want_refusal=${rest%%:*}
	rest=${rest#*:}
	play_fixture "refuse-$label" "refuse-$label" 6
	check "host play (guest refuses $label): exit 6, the guard reason is in the output, the refusal and the transcript" \
		sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq "active_refusal=$3" && printf "%s\n" "$2" | grep -Fxq "guard_trip_$4=$5" &&
			printf "%s\n" "$2" | grep -Fxq "refusal_trips=$4:$5" && printf "%s\n" "$2" | grep -Fq "REFUSING pixel-9a A2DP active proof (play): guest refused before any tone ($3; guard trips $4:$5)" &&
			grep -Fxq "guard_trip_$4=$5" "$6/play-latest.txt" && grep -Fxq guard_trip_live=none "$6/play-latest.txt"' sh \
		"$status" "$out" "$want_refusal" "${rest%%:*}" "${rest#*:}" "$results"
done
derive refuse-pcm-opened-after-preflight refuse-partial-trips -e '/^guard_trip_ctl=/d'
play_fixture refuse-partial-trips refuse-partial-trips 6
check "host play (refusal with an incomplete set of guard reasons): malformed, route unverified, exit 7" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq route_safety=unverified && printf "%s\n" "$2" | grep -Fq "incomplete set of guard reasons"' sh "$status" "$out"
check "host play (guest refuses over-cap): the observed volume is reported" \
	grep -Fxq sink_volume=0.80 "$outroot/run-refuse-volume-over-cap/pixel-9a/a2dp-active-proof/play-latest.txt"
# Any failure once the play session has begun: route unverified, the
# read-only observation runs (a second session), exit 7.
play_fixture refusal-exit-mismatch refuse-binding-mismatch 0
check "host play (refusal reported with exit 0): route unverified, observed, exit 7" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq route_safety=unverified && [ "$3" -eq 2 ]' sh "$status" "$out" "$runs"
for fx in leak-address leak-hex unexpected-key duplicate-key truncated pre-blocked pre-malformed skipped-review loud-claimed muted-claimed; do
	play_fixture "bad-$fx" "$fx" 0
	check "host play rejects ($fx): route unverified, observed, exit 7, no identifier retained" \
		sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$4" | grep -Fxq route_safety=unverified && [ "$5" -eq 2 ] &&
			grep -Fxq route_safety=unverified "$3"/pixel-9a/a2dp-active-proof/play-*-*.txt && ! grep -rEq "66[:_]77|66778899AABB" "$3"' sh \
		"$status" "" "$outroot/run-bad-$fx" "$out" "$runs"
done
run dry-loud dry-loud 0 dry
check "host dry rejects a reported volume over the cap: exit 5, no decision" \
	sh -c '[ "$1" -eq 5 ] && [ ! -e "$2/decision-pending" ]' sh "$status" "$results"
derive dry dry-leak -e "s/^sink_codec=.*/sink_codec=$addr/"
run dry-leak dry-leak 0 dry
check "host dry keeps its codes: identifier-bearing output exits 5, one session, no decision" \
	sh -c '[ "$1" -eq 5 ] && [ "$2" -eq 1 ] && [ ! -e "$3/decision-pending" ] && ! printf "%s\n" "$4" | grep -Fq route_safety' sh \
	"$status" "$runs" "$results" "$out"
run dry-bad-binding dry-bad-binding 0 dry
check "host dry refuses a malformed binding: exit 5, no decision" \
	sh -c '[ "$1" -eq 5 ] && [ ! -e "$2/decision-pending" ]' sh "$status" "$results"
run dry-wrong-board dry-wrong-board 0 dry
check "host dry refuses a block from another board: exit 4" sh -c '[ "$1" -eq 4 ]' sh "$status"
derive refuse-alsa-sink-at-start dry-refused -e 's/^active_mode=play$/active_mode=dry/' -e '/^guard_trip_/d'
run dry-blocked dry-refused 6 dry
check "host dry on a blocked preflight: exit 6, problems listed, no decision" \
	sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq preflight_problems=internal-alsa-sink-present && [ ! -e "$3/decision-pending" ]' sh "$status" "$out" "$results"
run dry-board dry 4 dry
check "host dry (guest device-tree gate): exit 4" sh -c '[ "$1" -eq 4 ]' sh "$status"
fresh_decision play-guest-error
run play-guest-error play-good 1 play "$phrase"
check "host play (guest failure): route unverified, exit 7" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq route_safety=unverified' sh "$status" "$out"
run usage dry 0 bogus
check "host usage: unknown mode, exit 2, no session" sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 0 ]' sh "$status" "$runs"
mkdir -p "$outroot/run-unreachable"
acm_guest_start --silent
status=0
out=$(FERRIX_OUT_DIR="$outroot/run-unreachable" FERRIX_PIXEL9A_ACM_TIMEOUT=30 FERRIX_PIXEL9A_A2DP_EVIDENCE=mock sh "$host" dry 2>&1) || status=$?
silent_port=$FERRIX_SHELL
acm_guest_stop
check "host unreachable: exit 3, sanitised, port never retained" \
	sh -c '[ "$1" -eq 3 ] && printf "%s\n" "$2" | grep -Fq "transport failed (no-shell-response)" && ! printf "%s\n" "$2" | grep -Fq "$3" && ! grep -rFq "$3" "$4"' sh \
	"$status" "$out" "$silent_port" "$outroot/run-unreachable"

# --- layer 3: end to end through the transport -----------------------------------
alt="$work/alt"
mkdir -p "$alt/tools/pixel-9a"
cp "$host" "$pre_host" "$root/tools/pixel-9a/acm-probe.py" "$alt/tools/pixel-9a/"
cp "$fake/probe.sh" "$alt/tools/pixel-9a/a2dp-output-preflight-remote.sh"
cp "$fake/body.sh" "$alt/tools/pixel-9a/a2dp-active-proof-remote.sh"
cp "$fbin"/* "$work/acm-guest/bin/"
{ cat "$work/library.sh"; printf 'preflight_body() {\n'; cat "$fake/probe.sh"; printf '}\n'; cat "$fake/body.sh"; } >"$work/expected-alt-body.sh"
ACM_GUEST_PROBE="$work/expected-alt-body.sh"
export ACM_GUEST_PROBE
e2e() {
	case_dir="$alt/out/run-e2e"
	mkdir -p "$case_dir"
	acm_guest_start
	acm_guest_ctl header_lines 3
	acm_guest_ctl real 1
	status=0
	out=$(cd "$work" && FERRIX_OUT_DIR="$case_dir" FERRIX_PIXEL9A_A2DP_EVIDENCE=mock FERRIX_PIXEL9A_A2DP_PLAY_CONFIRM="${2:-}" \
		sh "$alt/tools/pixel-9a/a2dp-active-proof.sh" "$1" 2>&1) || status=$?
	acm_guest_stop
	results="$case_dir/pixel-9a/a2dp-active-proof"
}
reset_guest
e2e dry
phrase=$(phrase_of "$out")
check "e2e dry: decision ready" sh -c '[ "$1" -eq 0 ] && [ -n "$2" ]' sh "$status" "$phrase"
[ "$status" -eq 0 ] || printf '%s\n' "$out" | tail -n 5
reset_guest
e2e play "$phrase"
check "e2e play: route held, tone delivered, cleanup clean" \
	sh -c '[ "$1" -eq 0 ] && printf "%s\n" "$2" | grep -Fxq result=route-held-tone-delivered' sh "$status" "$out"
[ "$status" -eq 0 ] || printf '%s\n' "$out" | tail -n 5
check "e2e play: only reviewed verbs ran" no_violation
check "e2e: no identifier, salt, or token in output or transcripts" private "$results"
reset_guest
e2e dry
phrase=$(phrase_of "$out")
reset_guest
printf 'alsa-sink\n' >"$state/scenario"
e2e play "$phrase"
check "e2e play with an ALSA sink appearing mid-tone: killed, route tripped, exit 7" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq route_safety=tripped && [ ! -f "$3" ]' sh "$status" "$out" "$state/pw-cat-finished"

check "no test edits docs/status.md or docs/status-log/" [ "$(status_sum)" = "$status_before" ]
check "no test edits capabilities.toml" [ "$(cksum <"$root/targets/boards/$target_id/capabilities.toml")" = "$capabilities_before" ]

if [ "$fails" -ne 0 ]; then
	printf '{"event":"pixel-9a-a2dp-active-proof-test","status":"failed","failures":%s}\n' "$fails"
	exit 1
fi
printf '{"event":"pixel-9a-a2dp-active-proof-test","status":"passed"}\n'
