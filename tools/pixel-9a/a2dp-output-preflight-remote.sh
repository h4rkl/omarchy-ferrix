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
#     `wpctl inspect` of each sink and filter node): sinks by class (bluez
#     A2DP, other bluez, ALSA, null, other), streams, the default sink's
#     class, the A2DP sink's codec, and whether that sink's node name and
#     BlueZ address both name the one connected Audio Sink device;
#   - filter nodes by class: only the two halves of WirePlumber 0.5.15's
#     BlueZ source loopback (`monitors/bluez.lua`
#     CreateDeviceLoopbackSource) bound to that same device are recognised;
#     anything else is counted as other, unbound, or uninspectable;
#   - the WirePlumber daemon: one process, its argv and config-path
#     environment variable names, override files in every directory
#     WirePlumber 0.5.15 searches before /usr/share, and a digest of the
#     /usr/share/wireplumber tree compared with the reviewed packages;
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
passwd_file=/etc/passwd
wp_data_dir=/usr/share/wireplumber
wp_override_dirs='/etc/xdg/wireplumber /etc/wireplumber /usr/local/share/wireplumber'
# sha256 over the sorted `sha256sum ./path` lines of /usr/share/wireplumber
# as installed by Alpine v3.24 wireplumber-0.5.15-r0 plus postmarketOS v26.06
# postmarketos-base-ui-audio-backend-pipewire-51-r0 (75 files; see
# audio-route-decision.md, "A2DP route source review").
wp_data_sha256=4be643c4ed1f2ba088dcd7e33dbce325131f1c6aef2a1d1aed7ec64e7c335b79
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

printf 'probe=a2dp-output-preflight\nprobe_version=3\n'
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
# inspect_props ID: the reviewed properties of one node as key=value lines,
# or nothing if the inspect failed or printed a reviewed key twice (a value
# with an embedded newline could otherwise forge a later property line).
inspect_props() {
	wp inspect "$1" 2>/dev/null | awk '
		{ line = $0; sub(/^[[:space:]*]+/, "", line) }
		match(line, /^[A-Za-z0-9._-]+ = "/) {
			k = substr(line, 1, RLENGTH - 4); v = substr(line, RLENGTH + 1); sub(/"[[:space:]]*$/, "", v)
			if (k == "media.class" || k == "node.name" || k == "device.api" || k == "factory.name" ||
				k == "api.bluez5.profile" || k == "api.bluez5.codec" || k == "api.bluez5.address" ||
				k == "device.id" || k == "node.link-group" || k == "bluez5.loopback" || k == "node.dont-fallback" ||
				k == "node.passive" || k == "target.object" || k == "filter.smart" || k == "stream.capture.sink") {
				if (k in seen) dup = 1
				seen[k] = 1; out = out k "=" v "\n"
			}
		}
		END { if (!dup) printf "%s", out }' || :
}
prop() { printf '%s\n' "$props" | sed -n "s/^$1=//p" | sed -n '1p'; }
# addr_in PREFIX SUFFIX VALUE: the upper-cased text between PREFIX and
# SUFFIX of VALUE, or failure if VALUE does not have both.
addr_in() {
	case "$3" in "$1"*"$2") ;; *) return 1 ;; esac
	rest=${3#"$1"}
	printf '%s\n' "${rest%"$2"}" | tr 'a-f' 'A-F'
}
graph=wpctl-unavailable
sinks=unavailable; streams=unavailable; filters=unavailable
filter_groups=unavailable; f_capture=unavailable; f_source=unavailable; f_unbound=unavailable; f_other=unavailable; f_failed=unavailable
f_link=unavailable
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
			# Section headers are anchored to the tree prefix: a node, stream, or
			# device name that ends in "Sinks:" cannot switch sections.
			/^[^0-9A-Za-z]*Devices:[[:space:]]*$/ { sec = "devices"; next } /^[^0-9A-Za-z]*Sinks:[[:space:]]*$/ { sec = "sinks"; next }
			/^[^0-9A-Za-z]*Sources:[[:space:]]*$/ { sec = "sources"; next } /^[^0-9A-Za-z]*Filters:[[:space:]]*$/ { sec = "filters"; next }
			/^[^0-9A-Za-z]*Streams:[[:space:]]*$/ { sec = "streams"; next }
			/^[^0-9A-Za-z]*[A-Za-z][A-Za-z ]*:[[:space:]]*$/ { sec = ""; next }
			# wpctl prints one "- <node.link-group>" line per filter group.
			sec == "filters" && /^[^0-9A-Za-z]*- [^[:space:]]/ { groups++; next }
			match($0, /[0-9]+\. /) {
				id = substr($0, RSTART, RLENGTH - 2)
				if (sec == "sinks") printf "sink %s %s\n", (substr($0, 1, RSTART - 1) ~ /\*/) ? "*" : "-", id
				# Every filter-group member is its own node; none has sub-rows.
				if (sec == "filters") { printf "filter %s\n", id; n["filters"]++ }
				# Stream ports are indented below their node.
				if (sec == "streams") { ind = RSTART; if (!("streams" in top) || ind <= top["streams"]) { top["streams"] = ind; n["streams"]++ } }
			}
			END { printf "streams %d\nfilters %d\nfgroups %d\n", n["streams"], n["filters"], groups }')
		sink_rows=$(printf '%s\n' "$parsed" | sed -n 's/^sink //p')
		streams=$(printf '%s\n' "$parsed" | sed -n 's/^streams //p')
		filters=$(printf '%s\n' "$parsed" | sed -n 's/^filters //p')
		filter_groups=$(printf '%s\n' "$parsed" | sed -n 's/^fgroups //p')
		filter_rows=$(printf '%s\n' "$parsed" | sed -n 's/^filter //p')
		sinks=$(count_of "$sink_rows")
		n_a2dp=0; n_bluez_other=0; n_alsa=0; n_null=0; n_other=0; n_failed=0
		default_class=none
		[ "$(printf '%s\n' "$sink_rows" | grep -c '^\*')" -le 1 ] || default_class=ambiguous
		a2dp_codec=none; a2dp_match=none; a2dp_default=none
		sel_addr=''; sel_dev=''
		while read -r mark id; do
			[ -n "$id" ] || continue
			props=$(inspect_props "$id")
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
					# Only a single matched sink binds the loopback check below.
					if [ "$a2dp_match" = yes ] && [ "$n_a2dp" -eq 1 ]; then
						sel_addr=$from_name; sel_dev=$(prop device.id | tr -cd '0-9')
					else
						sel_addr=''; sel_dev=''
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
		# Filter nodes. WirePlumber 0.5.15 creates, per connected BlueZ device
		# with a headset profile, one libpipewire-module-loopback pair:
		# bluez_capture_internal.<address> (Stream/Input/Audio/Internal,
		# passive, dont-fallback, target bluez_input.<address_>.0) feeding
		# bluez_input.<address> (Audio/Source on the same device). Neither half
		# accepts a playback stream. Only that exact pair, bound to the one
		# matched A2DP sink and sharing one link group, is recognised.
		f_capture=0; f_source=0; f_unbound=0; f_other=0; f_failed=0; f_link=none
		lg_capture=''; lg_source=''
		while read -r id; do
			[ -n "$id" ] || continue
			props=$(inspect_props "$id")
			media_class=$(prop media.class)
			node_name=$(prop node.name)
			link_group=$(prop node.link-group)
			if [ -z "$props" ] || [ -z "$media_class" ] || [ -z "$node_name" ] || [ -z "$link_group" ]; then
				f_failed=$((f_failed + 1)); continue
			fi
			if [ "$(prop bluez5.loopback)" != true ] || [ -n "$(prop filter.smart)" ] || [ -n "$(prop stream.capture.sink)" ]; then
				f_other=$((f_other + 1)); continue
			fi
			sel_us=$(printf '%s' "$sel_addr" | tr ':' '_')
			if name_addr=$(addr_in bluez_capture_internal. '' "$node_name"); then
				if [ -z "$sel_addr" ] || [ "$name_addr" != "$sel_addr" ]; then
					f_unbound=$((f_unbound + 1))
				elif [ "$media_class" = Stream/Input/Audio/Internal ] && [ "$(prop node.dont-fallback)" = true ] &&
					[ "$(prop node.passive)" = true ] &&
					[ "$(addr_in bluez_input. .0 "$(prop target.object)")" = "$sel_us" ]; then
					f_capture=$((f_capture + 1)); lg_capture=$link_group
				else
					f_other=$((f_other + 1))
				fi
			elif name_addr=$(addr_in bluez_input. '' "$node_name"); then
				if [ -z "$sel_addr" ] || [ "$name_addr" != "$sel_addr" ]; then
					f_unbound=$((f_unbound + 1))
				elif [ "$media_class" = Audio/Source ] && [ -n "$sel_dev" ] && [ "$(prop device.id)" = "$sel_dev" ]; then
					f_source=$((f_source + 1)); lg_source=$link_group
				else
					f_other=$((f_other + 1))
				fi
			else
				f_other=$((f_other + 1))
			fi
		done <<EOF
$filter_rows
EOF
		if [ -n "$lg_capture" ] || [ -n "$lg_source" ]; then
			if [ "$lg_capture" = "$lg_source" ]; then f_link=yes; else f_link=no; fi
		fi
	else
		graph=unreachable
	fi
fi
printf 'pipewire_graph=%s\ngraph_sinks=%s\ngraph_streams=%s\ngraph_filters=%s\n' "$graph" "$sinks" "$streams" "$filters"
printf 'graph_sink_bluez_a2dp=%s\ngraph_sink_bluez_other=%s\ngraph_sink_alsa=%s\n' "$n_a2dp" "$n_bluez_other" "$n_alsa"
printf 'graph_sink_null=%s\ngraph_sink_other=%s\ngraph_sink_inspect_failed=%s\n' "$n_null" "$n_other" "$n_failed"
printf 'default_sink_class=%s\na2dp_sink_codec=%s\na2dp_sink_device_match=%s\na2dp_sink_default=%s\n' \
	"$default_class" "$a2dp_codec" "$a2dp_match" "$a2dp_default"
printf 'graph_filter_groups=%s\ngraph_filter_bt_capture=%s\ngraph_filter_bt_source=%s\n' "$filter_groups" "$f_capture" "$f_source"
printf 'graph_filter_bt_unbound=%s\ngraph_filter_other=%s\ngraph_filter_inspect_failed=%s\nfilter_link_group_match=%s\n' \
	"$f_unbound" "$f_other" "$f_failed" "$f_link"

# --- WirePlumber daemon and configuration, read only -------------------------
# The linking behaviour reviewed for #91 is that of the packaged 0.5.15
# scripts and configuration. WirePlumber 0.5.15 (lib/wp/base-dirs.c) reads
# configuration and scripts from WIREPLUMBER_{CONFIG,DATA,MODULE}_DIR if set,
# else $XDG_CONFIG_HOME (~/.config), $XDG_DATA_HOME (~/.local/share),
# $XDG_CONFIG_DIRS (/etc/xdg), /etc, and $XDG_DATA_DIRS (/usr/local/share,
# /usr/share), each with a wireplumber/ subdirectory. The daemon's
# environment names and home path are read here and never printed.
wp_daemon=unavailable
wp_invocation=unavailable
wp_overrides=unavailable
wp_tree=unavailable
wp_pids=''
wp_seen=0
for proc_dir in "$proc_root"/[0-9]*; do
	[ -r "$proc_dir/comm" ] || continue
	wp_seen=$((wp_seen + 1))
	[ "$(cat "$proc_dir/comm" 2>/dev/null)" != wireplumber ] || wp_pids="$wp_pids ${proc_dir##*/}"
done
set -- $wp_pids
if [ "$wp_seen" -gt 0 ]; then
	case "$#" in 0) wp_daemon=none ;; 1) wp_daemon=one ;; *) wp_daemon=multiple ;; esac
fi
if [ "$wp_daemon" = one ]; then
	wp_proc="$proc_root/$1"
	wp_home=''
	if wp_argv=$(tr '\000' '\n' <"$wp_proc/cmdline" 2>/dev/null) && wp_env=$(tr '\000' '\n' <"$wp_proc/environ" 2>/dev/null) &&
		[ -n "$wp_argv" ]; then
		wp_invocation=reviewed
		case "$wp_argv" in /usr/bin/wireplumber|wireplumber) ;; *) wp_invocation=overridden ;; esac
		! printf '%s\n' "$wp_env" | sed -n 's/=.*//p' |
			grep -Eqx 'WIREPLUMBER_(CONFIG|DATA|MODULE)_DIR|XDG_(CONFIG|DATA)_(HOME|DIRS)' || wp_invocation=overridden
		wp_home=$(printf '%s\n' "$wp_env" | sed -n 's/^HOME=//p' | sed -n '1p')
	fi
	if [ -z "$wp_home" ]; then
		wp_uid=$(awk '$1 == "Uid:" { print $2; exit }' "$wp_proc/status" 2>/dev/null)
		[ -z "$wp_uid" ] || wp_home=$(awk -F: -v uid="$wp_uid" '$3 == uid { print $6; exit }' "$passwd_file" 2>/dev/null)
	fi
	case "$wp_home" in
		/*) if have find; then
				wp_overrides=0
				for dir in "$wp_home/.config/wireplumber" "$wp_home/.local/share/wireplumber" $wp_override_dirs; do
					[ -e "$dir" ] || [ -L "$dir" ] || continue
					if found=$(find "$dir" ! -type d 2>/dev/null); then
						wp_overrides=$((wp_overrides + $(count_of "$found")))
					else
						wp_overrides=unavailable; break
					fi
				done
			fi ;;
	esac
fi
if [ ! -d "$wp_data_dir" ]; then
	wp_tree=absent
elif have find && have sha256sum; then
	# Every entry must be a regular file; the digest covers names and contents.
	if wp_list=$(cd "$wp_data_dir" && find . ! -type d 2>/dev/null | LC_ALL=C sort) && [ -n "$wp_list" ]; then
		wp_tree=modified
		wp_sums=$(cd "$wp_data_dir" && while IFS= read -r f; do
			{ [ -f "$f" ] && [ ! -L "$f" ]; } || { echo irregular; continue; }
			sha256sum "$f" 2>/dev/null || echo unreadable
		done <<EOF
$wp_list
EOF
)
		[ "$(printf '%s\n' "$wp_sums" | sha256sum 2>/dev/null | cut -c1-64)" != "$wp_data_sha256" ] || wp_tree=reviewed
	fi
fi
printf 'wireplumber_daemon=%s\nwireplumber_invocation=%s\nwireplumber_config_overrides=%s\nwireplumber_data_tree=%s\n' \
	"$wp_daemon" "$wp_invocation" "$wp_overrides" "$wp_tree"

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
