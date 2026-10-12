#!/bin/sh
set -eu

# Issue #91 Bluetooth A2DP-only output preflight on an ALREADY booted Google
# Pixel 9a (tegu / GXQ96 / MP1.0) Ferrix dev guest, reached over the USB
# CDC-ACM bring-up shell (tools/pixel-9a/acm-probe.py).
#
#   a2dp-output-preflight.sh [preflight]   read-only; the only mode
#
# One session sends the exact bytes of a2dp-output-preflight-remote.sh, whose
# first action is the exact tegu device-tree gate. The guest reports the
# BlueZ, PipeWire/WirePlumber and package state an A2DP-only playback proof
# would depend on, as counts, classes, versions, and yes/no matches. The host
# validates every line and lists each fail-closed blocker:
#
#   candidate-observed  exactly one powered controller, exactly one paired and
#                       connected Audio Sink device, exactly one bluez A2DP
#                       sink whose node name and BlueZ address both name that
#                       device, no ALSA/other/headset/unreadable sink, no
#                       audio stream or filter, both internal speaker-path
#                       controls `off`, no open ALSA playback PCM, pw-cat with
#                       --target and --properties, and the reviewed services,
#                       packages, kernel, and root
#   blocked             anything else, with the reasons in
#                       a2dp_preflight_problems
#
# Neither result opens the active proof. No playback mode exists: whether a
# pw-cat stream aimed at that sink can fall back to another sink, and how the
# on-image WirePlumber honours an explicit target, are not verified on the
# target (audio-route-decision.md, "Bluetooth A2DP output preflight").
# The guest never plays or records audio, pairs, connects, trusts, powers,
# scans, selects a profile or default, changes a volume, writes a mixer
# control, loads modules, or starts or stops a service. capabilities.toml is
# never edited and audio stays unsupported.
#
# Exit status: 0 candidate-observed, 2 usage, 3 transport or guest failure,
# 4 not the exact tegu board, 5 malformed or incomplete probe output,
# 6 blocked.
#
# FERRIX_SHELL selects the host CDC-ACM node (default
# /dev/cu.usbmodemferrix_tegu3); it is never printed or retained. Set
# FERRIX_PIXEL9A_A2DP_EVIDENCE=mock for stubbed runs. Transcripts go to
# ignored out/pixel-9a/a2dp-output-preflight/ and hold no Bluetooth address,
# device name, node name, serial, port name, or audio. Never commit them.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target_id=google-pixel-9a-tegu-gxq96-mp1-0
probe="$root/tools/pixel-9a/a2dp-output-preflight-remote.sh"
transport="$root/tools/pixel-9a/acm-probe.py"
out="${FERRIX_OUT_DIR:-$root/out}/pixel-9a/a2dp-output-preflight"
dt_model='ZUMA PRO TEGU MP on ZUMA PRO'
dt_compatible='google,ZUMA PRO TEGU'
pinned_kernel=6.1.124-android14-11-ga4662a84a15f
active_gate_reason=pipewire-explicit-target-fallback-unverified-on-target
max_output=65536
acm_timeout=${FERRIX_PIXEL9A_ACM_TIMEOUT:-120}

usage() {
	printf 'usage: tools/pixel-9a/a2dp-output-preflight.sh [preflight]\n' >&2
	printf 'environment: FERRIX_SHELL (CDC-ACM node) FERRIX_PIXEL9A_ACM_TIMEOUT (seconds, 5-600, default 120)\n' >&2
	printf '             FERRIX_PIXEL9A_A2DP_EVIDENCE (physical|mock) FERRIX_OUT_DIR\n' >&2
	exit 2
}

case "$#:${1:-}" in
	0:|1:preflight) ;;
	*) usage ;;
esac
case "${FERRIX_PIXEL9A_A2DP_EVIDENCE:-physical}" in
	physical) evidence="physical:$target_id" ;;
	mock) evidence=mock ;;
	*) usage ;;
esac
case "$acm_timeout" in ''|*[!0-9]*) usage ;; esac
[ "$acm_timeout" -ge 5 ] && [ "$acm_timeout" -le 600 ] || usage

transcript=''
die() {
	code=${2:-1}
	[ -z "$transcript" ] || printf 'result=refused\nreason=%s\n' "$1" >>"$transcript"
	printf 'REFUSING pixel-9a A2DP output preflight: %s\n' "$1" >&2
	printf '{"event":"pixel-9a-a2dp-output-preflight","evidence":"%s","status":"failed","reason":"%s","exit":%s}\n' \
		"$evidence" "$(printf '%s' "$1" | tr '"\\' "''")" "$code" >&2
	exit "$code"
}

for tool in python3 awk sed; do
	command -v "$tool" >/dev/null 2>&1 || die "missing host tool '$tool'; install it explicitly"
done
[ -f "$probe" ] && [ ! -L "$probe" ] || die 'guest probe script is missing or linked'
[ -f "$transport" ] && [ ! -L "$transport" ] || die 'CDC-ACM transport is missing or linked'

sha256() { (shasum -a 256 "$1" 2>/dev/null || sha256sum "$1") | cut -c1-64; }

mkdir -p "$out"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
transcript="$out/preflight-$stamp-$$.txt"
work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-a2dp-output-preflight.XXXXXX")
trap 'rm -rf "$work"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
{
	printf '# Pixel 9a Bluetooth A2DP output preflight, %s UTC\n' "$stamp"
	printf '# evidence: %s\n' "$evidence"
	printf '# Read-only counts and classes: no address, device or node name, serial, port name, or audio. Never commit this file.\n'
	printf 'target_id=%s\nevidence=%s\nmode=preflight\nsource=cdc-acm\n' "$target_id" "$evidence"
	printf 'probe_body_sha256=%s\n' "$(sha256 "$probe")"
} >"$transcript"

emit() {
	printf '%s=%s\n' "$1" "$2" >>"$transcript"
	printf '%s=%s\n' "$1" "$2"
}

session_status=0
python3 "$transport" --probe "$probe" --timeout "$acm_timeout" --max-bytes "$max_output" --stderr discard \
	>"$work/guest.out" 2>"$work/transport.log" || session_status=$?
case "$session_status" in
	0) ;;
	255)
		reason=$(sed -n 's/^ferrix-acm: .* reason=\([a-z-]*\)$/\1/p' "$work/transport.log" | sed -n '1p')
		die "guest CDC-ACM shell transport failed (${reason:-unknown})" 3 ;;
	4) die 'guest is not the exact tegu board (device-tree gate refused)' 4 ;;
	*) die "guest preflight probe failed (exit $session_status)" 3 ;;
esac
[ "$(wc -c <"$work/guest.out" | tr -d ' ')" -le "$max_output" ] || die 'probe output exceeds the reviewed size limit' 5

# The exact reviewed key set; every key is required once and nothing else may
# appear. Values may not carry ':' or '_' (the two spellings of a Bluetooth
# address) or a run of twelve hex digits.
expected_keys='probe probe_version dt_model dt_compatible kernel_release uid
tool.bluetoothctl tool.wpctl tool.amixer tool.timeout tool.pw_cat pw_cat_target_option pw_cat_properties_option
pkg.bluez pkg.pipewire pkg.wireplumber pkg.pipewire_spa_bluez pkg.pipewire_tools spa_bluez_plugin
service.bluetooth service.ferrix_bluetooth_uart service.ferrix_pipewire service.ferrix_audio
bt_controllers bt_powered bt_paired_query bt_paired bt_paired_audio_sink bt_connected_audio_sink
pipewire_socket pipewire_graph graph_sinks graph_streams graph_filters
graph_sink_bluez_a2dp graph_sink_bluez_other graph_sink_alsa graph_sink_null graph_sink_other graph_sink_inspect_failed
default_sink_class a2dp_sink_codec a2dp_sink_device_match a2dp_sink_default
watch.tdm_0_rx_mixer_ep1 watch.tas25xx_asi_switch pcm_scan_procs pcm_opener_scan playback_pcm_openers probe_complete'

# validate FILE: prints nothing and succeeds only for the exact key set.
validate() {
	printf '%s\n' $expected_keys | awk '
		NR == FNR { want[$0] = 1; total++; next }
		{
			eq = index($0, "=")
			if (eq < 2) { print "line without a key"; bad = 1; next }
			k = substr($0, 1, eq - 1); v = substr($0, eq + 1)
			if (!(k in want)) { print "unexpected key " k; bad = 1; next }
			if (k in seen) { print "duplicate key " k; bad = 1; next }
			seen[k] = 1; n++
			if (v !~ /^[A-Za-z0-9 .,+\/-]*$/) { print "value outside the reviewed character set for " k; bad = 1 }
			if (v ~ /[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]/ && k != "kernel_release") {
				print "identifier-shaped value for " k; bad = 1
			}
			last = k
		}
		END {
			if (n != total) print "missing keys"
			else if (last != "probe_complete") print "probe_complete is not last"
			exit (bad || n != total || last != "probe_complete")
		}' - "$1"
}

if ! problems=$(validate "$work/guest.out"); then
	die "malformed or incomplete probe output ($(printf '%s\n' "$problems" | sed -n '1p'))" 5
fi
# Only validated lines reach the transcript.
cat "$work/guest.out" >>"$transcript"

get() { sed -n "s/^$1=//p" "$2" | sed -n '1p'; }
g() { get "$1" "$work/guest.out"; }
[ "$(g probe)" = a2dp-output-preflight ] && [ "$(g probe_version)" = 2 ] && [ "$(g probe_complete)" = yes ] ||
	die 'guest did not report the reviewed preflight probe' 5
[ "$(g dt_model)" = "$dt_model" ] && [ "$(g dt_compatible)" = "$dt_compatible" ] ||
	die 'guest output does not carry the exact tegu identity' 4

# is_n VALUE: a decimal count (never inferred from unavailable/unknown).
is_n() { case "$1" in ''|*[!0-9]*) return 1 ;; *) return 0 ;; esac; }
# preflight_problems FILE: every blocker, comma-separated, in a fixed order.
preflight_problems() {
	f=$1
	p=''
	add() { p="${p:+$p,}$1"; }
	n_is() { v=$(get "$1" "$f"); is_n "$v" && [ "$v" -eq "$2" ]; }
	[ "$(get uid "$f")" = 0 ] || add not-root
	[ "$(get kernel_release "$f")" = "$pinned_kernel" ] || add kernel-not-pinned
	for tool in bluetoothctl wpctl amixer timeout pw_cat; do
		[ "$(get "tool.$tool" "$f")" = present ] || add "$(printf '%s' "$tool" | tr '_' '-')-missing"
	done
	[ "$(get pw_cat_target_option "$f")" = present ] || add pw-cat-target-option-missing
	[ "$(get pw_cat_properties_option "$f")" = present ] || add pw-cat-properties-option-missing
	for package in bluez pipewire wireplumber pipewire_spa_bluez; do
		case "$(get "pkg.$package" "$f")" in absent|unavailable|'') add "package-$(printf '%s' "$package" | tr '_' '-')-not-installed" ;; esac
	done
	[ "$(get spa_bluez_plugin "$f")" = present ] || add spa-bluez-plugin-missing
	for service in bluetooth ferrix_bluetooth_uart ferrix_pipewire ferrix_audio; do
		[ "$(get "service.$service" "$f")" = started ] || add "service-$(printf '%s' "$service" | tr '_' '-')-not-started"
	done
	n_is bt_controllers 1 || add bt-controller-count-not-one
	[ "$(get bt_powered "$f")" = yes ] || add bt-not-powered
	[ "$(get bt_paired_query "$f")" = ok ] || add bt-paired-query-incomplete
	v=$(get bt_paired_audio_sink "$f")
	is_n "$v" && [ "$v" -ge 1 ] || add no-paired-audio-sink
	n_is bt_connected_audio_sink 1 || add connected-audio-sink-count-not-one
	if [ "$(get pipewire_graph "$f")" != reachable ]; then
		add "pipewire-graph-$(get pipewire_graph "$f")"
	else
		n_is graph_sink_bluez_a2dp 1 || add bluez-a2dp-sink-count-not-one
		n_is graph_sink_bluez_other 0 || add bluez-non-a2dp-sink-present
		n_is graph_sink_alsa 0 || add internal-alsa-sink-present
		n_is graph_sink_other 0 || add unclassified-sink-present
		n_is graph_sink_inspect_failed 0 || add sink-inspect-failed
		n_is graph_streams 0 || add audio-streams-present
		n_is graph_filters 0 || add audio-filters-present
		[ "$(get default_sink_class "$f")" != ambiguous ] || add default-sink-ambiguous
		if n_is graph_sink_bluez_a2dp 1; then
			[ "$(get a2dp_sink_device_match "$f")" = yes ] || add a2dp-sink-device-mismatch
		fi
	fi
	[ "$(get watch.tdm_0_rx_mixer_ep1 "$f")" = off ] || add internal-speaker-route-not-off
	[ "$(get watch.tas25xx_asi_switch "$f")" = off ] || add internal-amp-switch-not-off
	# Zero openers counts only from a complete scan of a non-empty /proc.
	[ "$(get pcm_opener_scan "$f")" = complete ] || add pcm-opener-scan-incomplete
	v=$(get pcm_scan_procs "$f")
	is_n "$v" && [ "$v" -gt 0 ] || add pcm-opener-scan-empty
	v=$(get playback_pcm_openers "$f")
	if ! is_n "$v"; then add playback-pcm-openers-unknown; elif [ "$v" -ne 0 ]; then add playback-pcm-open; fi
	printf '%s\n' "$p"
}

for key in pkg.bluez pkg.pipewire pkg.wireplumber pkg.pipewire_spa_bluez pkg.pipewire_tools \
	pw_cat_target_option pw_cat_properties_option bt_controllers bt_powered bt_paired bt_paired_audio_sink \
	bt_connected_audio_sink pipewire_graph graph_sinks graph_sink_bluez_a2dp graph_sink_alsa graph_sink_null \
	default_sink_class a2dp_sink_codec a2dp_sink_device_match a2dp_sink_default \
	watch.tdm_0_rx_mixer_ep1 watch.tas25xx_asi_switch pcm_opener_scan playback_pcm_openers; do
	printf '%s=%s\n' "$key" "$(g "$key")"
done
problems=$(preflight_problems "$work/guest.out")
if [ -z "$problems" ]; then
	emit a2dp_preflight candidate-observed
else
	emit a2dp_preflight blocked
	emit a2dp_preflight_problems "$problems"
fi
emit active_proof not-implemented
emit active_gate closed
emit active_gate_reason "$active_gate_reason"
emit audio_capability unsupported
cp "$transcript" "$out/preflight-latest.txt"
printf 'transcript=%s\n' "$transcript"
printf '{"event":"pixel-9a-a2dp-output-preflight","evidence":"%s","status":"%s","active_gate":"closed"}\n' \
	"$evidence" "$(if [ -z "$problems" ]; then echo candidate-observed; else echo blocked; fi)"
[ -z "$problems" ] || exit 6
