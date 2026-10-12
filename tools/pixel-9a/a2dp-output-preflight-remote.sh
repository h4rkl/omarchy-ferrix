#!/bin/sh
# Guest body of the issue #91 Bluetooth A2DP output preflight. Sent unchanged
# over the CDC-ACM shell (acm-probe.py, piped to the guest `sh`) by
# a2dp-output-preflight.sh. Read-only.
#
# It checks the exact device tree, then reports, as fixed key=value lines:
#
#   - the kernel release, uid, and tool presence (bluetoothctl, wpctl,
#     amixer, timeout, pw-cat) plus whether `pw-cat --help` lists
#     `--target` and `--properties`;
#   - installed versions of bluez, pipewire, wireplumber, pipewire-spa-bluez
#     and pipewire-tools, read from the apk database file, and whether the
#     SPA bluez5 plugin file exists;
#   - OpenRC status of bluetooth, ferrix-bluetooth-uart, ferrix-pipewire and
#     ferrix-audio;
#   - the BlueZ controller count and Powered state, and counts of paired,
#     paired-with-Audio-Sink-UUID and connected Audio Sink devices
#     (`bluetoothctl list|show|devices Paired|info`);
#   - the PipeWire audio graph of the uid-1000 session (`wpctl status` and
#     `wpctl inspect` of each sink): sinks by class (bluez A2DP, other bluez,
#     ALSA, null, other), streams, filters, the default sink's class, the
#     A2DP sink's codec, and whether that sink's node name and BlueZ address
#     both name the one connected Audio Sink device;
#   - two internal speaker-path controls read with `amixer cget` only
#     (`TDM_0_RX Mixer EP1`, `TAS25XX ASI Switch`) and the number of open
#     ALSA playback PCM file descriptors, with whether that fd scan was
#     complete.
#
# No address, device name, alias, node name, or serial is printed: they stay
# in shell variables and only counts, classes, and yes/no matches leave the
# guest. Nothing here plays or records audio, pairs, connects, trusts,
# powers, scans, selects a profile or default, changes a volume, writes a
# mixer control, loads modules, or starts or stops a service.
set -u

dt_root=/proc/device-tree
proc_root=/proc
runtime_dir=/run/user/1000
apk_db=/lib/apk/db/installed
spa_bluez_plugin=/usr/lib/spa-0.2/bluez5/libspa-bluez5.so
card=0
audio_sink_uuid=0000110b-0000-1000-8000-00805f9b34fb
watched_controls='TDM_0_RX Mixer EP1|TAS25XX ASI Switch'

model=$(tr '\000' '\n' <"$dt_root/model" 2>/dev/null | sed -n '1p')
compatible=$(tr '\000' '\n' <"$dt_root/compatible" 2>/dev/null)
[ "$model" = 'ZUMA PRO TEGU MP on ZUMA PRO' ] || exit 4
printf '%s\n' "$compatible" | grep -Fxq 'google,ZUMA PRO TEGU' || exit 4

# Every reported value stays inside the host's reviewed character set.
clean() { tr '_' '-' | tr -cd 'A-Za-z0-9.+-' | cut -c1-48; }
have() { command -v "$1" >/dev/null 2>&1; }
run_t() { if have timeout; then timeout "$@" </dev/null; else shift; "$@" </dev/null; fi; }
count_of() { if [ -z "$1" ]; then echo 0; else printf '%s\n' "$1" | grep -c .; fi; }

printf 'probe=a2dp-output-preflight\nprobe_version=2\n'
printf 'dt_model=%s\ndt_compatible=google,ZUMA PRO TEGU\n' "$model"
printf 'kernel_release=%s\nuid=%s\n' "$(uname -r | clean)" "$(id -u | tr -cd '0-9')"

for tool in bluetoothctl wpctl amixer timeout pw-cat; do
	key=$(printf '%s' "$tool" | tr '-' '_')
	if have "$tool"; then printf 'tool.%s=present\n' "$key"; else printf 'tool.%s=missing\n' "$key"; fi
done
# `--help` prints usage and exits; it opens no stream. Only option presence
# is recorded, for the active-gate review (audio-route-decision.md).
pw_target=unavailable
pw_props=unavailable
if have pw-cat; then
	pw_help=$(run_t 5 pw-cat --help 2>&1)
	pw_target=absent
	pw_props=absent
	printf '%s\n' "$pw_help" | grep -Eq '(^|[[:space:]])--target([=[:space:]]|$)' && pw_target=present
	printf '%s\n' "$pw_help" | grep -Eq '(^|[[:space:]])--properties([=[:space:]]|$)' && pw_props=present
fi
printf 'pw_cat_target_option=%s\npw_cat_properties_option=%s\n' "$pw_target" "$pw_props"

# --- packages: the apk database file, read only ----------------------------
for package in bluez pipewire wireplumber pipewire-spa-bluez pipewire-tools; do
	key=$(printf '%s' "$package" | tr '-' '_')
	if [ -r "$apk_db" ]; then
		version=$(awk -v want="$package" '
			/^$/ { p = "" }
			/^P:/ { p = substr($0, 3) }
			/^V:/ && p == want { print substr($0, 3); exit }' "$apk_db" 2>/dev/null | clean)
		printf 'pkg.%s=%s\n' "$key" "${version:-absent}"
	else
		printf 'pkg.%s=unavailable\n' "$key"
	fi
done
if [ -f "$spa_bluez_plugin" ]; then echo spa_bluez_plugin=present; else echo spa_bluez_plugin=absent; fi

for service in bluetooth ferrix-bluetooth-uart ferrix-pipewire ferrix-audio; do
	key=$(printf '%s' "$service" | tr '-' '_')
	if have rc-service; then
		rc-service "$service" status >/dev/null 2>&1 </dev/null
		case "$?" in
			0) echo "service.$key=started" ;;
			3) echo "service.$key=stopped" ;;
			*) echo "service.$key=not-started" ;;
		esac
	else
		echo "service.$key=unavailable"
	fi
done

# --- BlueZ: query verbs only ------------------------------------------------
# Spelled out: some awk builds have no {n} interval support.
hex2='[0-9A-F][0-9A-F]'
mac_re="^$hex2:$hex2:$hex2:$hex2:$hex2:$hex2\$"
bt_controllers=unavailable
bt_powered=unavailable
bt_paired_query=unavailable
bt_paired=unavailable
bt_paired_audio=unavailable
bt_connected_audio=unavailable
connected_sinks=''
if have bluetoothctl; then
	list=$(run_t 10 bluetoothctl list 2>/dev/null) && bt_controllers=$(printf '%s\n' "$list" |
		awk -v re="$mac_re" '$1 == "Controller" && toupper($2) ~ re { n++ } END { print n + 0 }')
	show=$(run_t 10 bluetoothctl show 2>/dev/null)
	case "$(printf '%s\n' "$show" | awk '$1 == "Powered:" { print $2; exit }')" in
		yes) bt_powered=yes ;;
		no) bt_powered=no ;;
	esac
	if paired=$(run_t 10 bluetoothctl devices Paired 2>/dev/null); then
		bt_paired_query=ok
		addresses=$(printf '%s\n' "$paired" | awk -v re="$mac_re" '$1 == "Device" && toupper($2) ~ re { print toupper($2) }' | sort -u)
		bt_paired=$(count_of "$addresses")
		bt_paired_audio=0
		bt_connected_audio=0
		for address in $addresses; do
			info=$(run_t 10 bluetoothctl info "$address" 2>/dev/null) || { bt_paired_query=info-failed; continue; }
			flags=$(printf '%s\n' "$info" | awk -v uuid="$audio_sink_uuid" '
				$1 == "Paired:" { p = $2 } $1 == "Connected:" { c = $2 }
				$1 == "UUID:" && index(tolower($0), "(" uuid ")") { s = 1 }
				END { printf "%s %s %s\n", p, c, s ? "sink" : "none" }')
			case "$flags" in
				'yes yes sink') bt_paired_audio=$((bt_paired_audio + 1)); bt_connected_audio=$((bt_connected_audio + 1))
					connected_sinks="$connected_sinks $address" ;;
				'yes '*' sink') bt_paired_audio=$((bt_paired_audio + 1)) ;;
			esac
		done
	else
		bt_paired_query=failed
	fi
fi
printf 'bt_controllers=%s\nbt_powered=%s\nbt_paired_query=%s\nbt_paired=%s\n' "$bt_controllers" "$bt_powered" "$bt_paired_query" "$bt_paired"
printf 'bt_paired_audio_sink=%s\nbt_connected_audio_sink=%s\n' "$bt_paired_audio" "$bt_connected_audio"

# --- PipeWire graph of the Ferrix session (uid 1000), read only -------------
wp() { run_t 5 env XDG_RUNTIME_DIR="$runtime_dir" wpctl "$@"; }
graph=wpctl-unavailable
sinks=unavailable; streams=unavailable; filters=unavailable
n_a2dp=unavailable; n_bluez_other=unavailable; n_alsa=unavailable; n_null=unavailable; n_other=unavailable; n_failed=unavailable
default_class=unavailable; a2dp_codec=unavailable; a2dp_match=unavailable; a2dp_default=unavailable
if [ -S "$runtime_dir/pipewire-0" ]; then echo pipewire_socket=present; else echo pipewire_socket=absent; fi
if have wpctl && [ ! -S "$runtime_dir/pipewire-0" ]; then
	graph=socket-absent
elif have wpctl; then
	if status_text=$(wp status 2>/dev/null); then
		graph=reachable
		# "S ID" per Audio sink (S is * for the default), and section counts.
		parsed=$(printf '%s\n' "$status_text" | awk '
			/^Audio/ { audio = 1; next }
			/^[A-Z]/ { audio = 0 }
			!audio { next }
			/Devices:/ { sec = "devices"; next } /Sinks:/ { sec = "sinks"; next } /Sources:/ { sec = "sources"; next }
			/Filters:/ { sec = "filters"; next } /Streams:/ { sec = "streams"; next }
			/[A-Za-z]+:[[:space:]]*$/ { sec = ""; next }
			match($0, /[0-9]+\. /) {
				id = substr($0, RSTART, RLENGTH - 2)
				if (sec == "sinks") printf "sink %s %s\n", (substr($0, 1, RSTART - 1) ~ /\*/) ? "*" : "-", id
				# Stream ports and filter links are indented below their node.
				if (sec == "streams" || sec == "filters") { ind = RSTART; if (!(sec in top) || ind <= top[sec]) { top[sec] = ind; n[sec]++ } }
			}
			END { printf "streams %d\nfilters %d\n", n["streams"], n["filters"] }')
		sink_rows=$(printf '%s\n' "$parsed" | sed -n 's/^sink //p')
		streams=$(printf '%s\n' "$parsed" | sed -n 's/^streams //p')
		filters=$(printf '%s\n' "$parsed" | sed -n 's/^filters //p')
		sinks=$(count_of "$sink_rows")
		n_a2dp=0; n_bluez_other=0; n_alsa=0; n_null=0; n_other=0; n_failed=0
		default_class=none
		[ "$(printf '%s\n' "$sink_rows" | grep -c '^\*')" -le 1 ] || default_class=ambiguous
		a2dp_codec=none; a2dp_match=none; a2dp_default=none
		while read -r mark id; do
			[ -n "$id" ] || continue
			props=$(wp inspect "$id" 2>/dev/null | awk '
				{ line = $0; sub(/^[[:space:]*]+/, "", line) }
				match(line, /^[A-Za-z0-9._-]+ = "/) {
					k = substr(line, 1, RLENGTH - 4); v = substr(line, RLENGTH + 1); sub(/"[[:space:]]*$/, "", v)
					if (k == "media.class" || k == "node.name" || k == "device.api" || k == "factory.name" ||
						k == "api.bluez5.profile" || k == "api.bluez5.codec" || k == "api.bluez5.address") print k "=" v
				}') || props=''
			prop() { printf '%s\n' "$props" | sed -n "s/^$1=//p" | sed -n '1p'; }
			media_class=$(prop media.class)
			node_name=$(prop node.name)
			device_api=$(prop device.api)
			if [ -z "$props" ] || [ -z "$media_class" ] || [ -z "$node_name" ]; then
				class=inspect-failed
			elif [ "$media_class" != Audio/Sink ]; then
				class=other
			elif [ "$device_api" = bluez5 ] || [ "${node_name#bluez_}" != "$node_name" ]; then
				class=bluez-other
				case "$device_api:$node_name:$(prop api.bluez5.profile)" in
					bluez5:bluez_output.*:a2dp-sink*) class=bluez-a2dp ;;
				esac
			elif [ "$device_api" = alsa ] || [ "${node_name#alsa_}" != "$node_name" ]; then
				class=alsa
			elif [ "$node_name" = auto_null ] || [ "$(prop factory.name)" = support.null-audio-sink ]; then
				class=null
			else
				class=other
			fi
			case "$class" in
				bluez-a2dp) n_a2dp=$((n_a2dp + 1))
					a2dp_codec=$(prop api.bluez5.codec | clean); [ -n "$a2dp_codec" ] || a2dp_codec=unreported
					# The node name's address and the BlueZ address property must
					# agree and name the one connected Audio Sink device.
					from_name=$(printf '%s\n' "$node_name" | sed -n 's/^bluez_output\.\([0-9A-Fa-f_]\{17\}\)\..*$/\1/p' | tr '_a-f' ':A-F')
					from_prop=$(prop api.bluez5.address | tr 'a-f' 'A-F')
					a2dp_match=no
					if [ -n "$from_name" ] && [ "$from_name" = "$from_prop" ] && printf '%s\n' "$from_name" | grep -Eq "$mac_re"; then
						set -- $connected_sinks
						[ "$#" -eq 1 ] && [ "$1" = "$from_name" ] && a2dp_match=yes
					fi
					if [ "$mark" = '*' ]; then a2dp_default=yes; else a2dp_default=no; fi ;;
				bluez-other) n_bluez_other=$((n_bluez_other + 1)) ;;
				alsa) n_alsa=$((n_alsa + 1)) ;;
				null) n_null=$((n_null + 1)) ;;
				inspect-failed) n_failed=$((n_failed + 1)) ;;
				*) n_other=$((n_other + 1)) ;;
			esac
			[ "$mark" != '*' ] || [ "$default_class" = ambiguous ] || default_class=$class
		done <<EOF
$sink_rows
EOF
	else
		graph=unreachable
	fi
fi
printf 'pipewire_graph=%s\ngraph_sinks=%s\ngraph_streams=%s\ngraph_filters=%s\n' "$graph" "$sinks" "$streams" "$filters"
printf 'graph_sink_bluez_a2dp=%s\ngraph_sink_bluez_other=%s\ngraph_sink_alsa=%s\n' "$n_a2dp" "$n_bluez_other" "$n_alsa"
printf 'graph_sink_null=%s\ngraph_sink_other=%s\ngraph_sink_inspect_failed=%s\n' "$n_null" "$n_other" "$n_failed"
printf 'default_sink_class=%s\na2dp_sink_codec=%s\na2dp_sink_device_match=%s\na2dp_sink_default=%s\n' \
	"$default_class" "$a2dp_codec" "$a2dp_match" "$a2dp_default"

# --- internal speaker path: read-only control values and PCM openers ---------
old_ifs=$IFS
IFS='|'
for control in $watched_controls; do
	IFS=$old_ifs
	key=$(printf '%s' "$control" | tr 'A-Z ' 'a-z_' | tr -cd 'a-z0-9_')
	value=unavailable
	if have amixer; then
		got=$(run_t 5 amixer -c "$card" cget "iface=MIXER,name='$control'" 2>/dev/null) &&
			value=$(printf '%s\n' "$got" | sed -n 's/^[[:space:]]*: values=//p' | sed -n '1p' | clean)
		[ -n "$value" ] || value=unavailable
	fi
	printf 'watch.%s=%s\n' "$key" "$value"
done
IFS=$old_ifs
# Opener scan, the mic-capture-proof-remote.sh opener_scan contract: count
# /proc/*/fd links whose target is named pcmC<card>D<device>p for any card and
# device, in any directory (a node seen through another mount namespace still
# counts). The scan is complete only if /proc has at least one fd table, ls
# neither timed out nor failed to run, and every ls complaint is a process
# that exited mid-scan; a permission error or any unknown line makes it
# incomplete, and an incomplete scan never reads as zero openers.
procs=0
for fd_dir in "$proc_root"/[0-9]*/fd; do [ -d "$fd_dir" ] && procs=$((procs + 1)); done
scan=$(run_t 20 ls -l "$proc_root"/[0-9]*/fd/ 2>&1)
scan_rc=$?
set -- $(printf '%s\n' "$scan" | awk '
	/^ls: / { if ($0 !~ /No such file or directory$/) other++; next }
	/^l[-r][-w][-xsS]/ && / -> / {
		t = $0; sub(/^.* -> /, "", t); sub(/ \(deleted\)$/, "", t); m = split(t, part, "/")
		if (part[m] ~ /^pcmC[0-9]+D[0-9]+p$/) hits++
		next
	}
	/^total [0-9]+$/ || /^$/ || /\/fd\/?:$/ { next }
	{ other++ }
	END { print hits + 0, other + 0 }')
openers=${1:-unavailable}
opener_scan=complete
case "$scan_rc" in 124|126|127|137|143) opener_scan=incomplete ;; esac
[ "${2:-1}" = 0 ] && [ "$procs" -gt 0 ] || opener_scan=incomplete
[ "$opener_scan" = complete ] || openers=unavailable
printf 'pcm_scan_procs=%s\npcm_opener_scan=%s\nplayback_pcm_openers=%s\n' "$procs" "$opener_scan" "$openers"
echo probe_complete=yes
