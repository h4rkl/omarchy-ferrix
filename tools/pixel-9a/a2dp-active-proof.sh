#!/bin/sh
set -eu

# Issue #91 bounded Bluetooth A2DP playback proof on an ALREADY booted Google
# Pixel 9a (tegu / GXQ96 / MP1.0) Ferrix dev guest, reached over the USB
# CDC-ACM bring-up shell (tools/pixel-9a/acm-probe.py).
#
#   a2dp-active-proof.sh [dry]   decision preflight; the default
#   a2dp-active-proof.sh play    one guarded 1.5 s tone to the bound A2DP sink
#
# Both modes send one session composed from a2dp-active-proof-remote.sh, the
# unchanged read-only v3 preflight body (a2dp-output-preflight-remote.sh), and
# the v3 host's pinned constants, key list, validator and blocker logic,
# copied from a2dp-output-preflight.sh. The guest runs that preflight in the
# session and re-derives the sink identity.
#
# dry plays nothing and writes no audio, Bluetooth, graph, mixer, or
# persistent state (it creates and removes a private tmpfs scratch
# directory). On a candidate-observed preflight it prints the decision
# context and one per-run phrase:
#
#   FERRIX_PIXEL9A_A2DP_PLAY_CONFIRM='play one 1.5 s 750 Hz tone on the bound A2DP sink binding <12 hex> decision <8 hex> on google-pixel-9a-tegu-gxq96-mp1-0'
#
# The binding is SHA-256, in the guest, over a secret random salt the host
# made for this decision, the sink's address, object.serial, device id, node
# name, profile and codec, the kernel, the five package versions, and the
# WirePlumber tree result. The salt never leaves ignored out/ (mode 0600) or
# the session, so the binding cannot be traced back to an address. Each dry
# run replaces the pending decision; a decision expires after 10 minutes.
#
# play needs that exact phrase. It consumes the pending decision (rename,
# then the salt is erased) before it opens the session, so a phrase works at
# most once, even if the session fails. The guest refuses unless, in the same
# session, the v3 preflight is candidate-observed again, the re-derived
# binding equals the decision's, and every guard is armed and the graph has
# not changed since the graph guard armed. The tone and the guards are
# described in a2dp-active-proof-remote.sh and audio-route-decision.md
# ("A2DP active proof workflow"). After the tone the guest runs the v3
# preflight again as the final observation.
#
# The host reports, separately:
#   route_safety          held | tripped (guard reasons) | unverified
#   tone_delivery         linked-active | not-delivered
#   cleanup               clean | anomaly (reasons)
#   audible_confirmation  owner-pending: only the owner, by ear, can confirm
#                         the tone was heard; code never claims it
#
# Exit status: 0 dry: decision ready / play: route held, tone delivered,
# cleanup clean; 2 usage or missing, stale, consumed or mismatched decision;
# 3 transport or guest failure; 4 not the exact tegu board; 5 malformed or
# incomplete probe output; 6 refused (nothing played); 7 a guard tripped, the
# route was not verified, or cleanup was not verified; 8 route held and
# cleanup clean but the tone was not delivered.
#
# Internal speaker and earpiece playback stays NO-GO; nothing here routes to
# them, and the guards stop the tone if anything could. capabilities.toml is
# never edited and audio stays unsupported, whatever the result.
#
# FERRIX_SHELL selects the host CDC-ACM node (default
# /dev/cu.usbmodemferrix_tegu3); it is never printed or retained. Set
# FERRIX_PIXEL9A_A2DP_EVIDENCE=mock for stubbed runs. Transcripts and the
# decision file go to ignored out/pixel-9a/a2dp-active-proof/ and hold no
# Bluetooth address, device or node name, serial, port name, salt, or audio.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target_id=google-pixel-9a-tegu-gxq96-mp1-0
body="$root/tools/pixel-9a/a2dp-active-proof-remote.sh"
preflight_body="$root/tools/pixel-9a/a2dp-output-preflight-remote.sh"
preflight_host="$root/tools/pixel-9a/a2dp-output-preflight.sh"
transport="$root/tools/pixel-9a/acm-probe.py"
out="${FERRIX_OUT_DIR:-$root/out}/pixel-9a/a2dp-active-proof"
decision="$out/decision-pending"
dt_model='ZUMA PRO TEGU MP on ZUMA PRO'
dt_compatible='google,ZUMA PRO TEGU'
decision_max_age=600
max_output=262144
acm_timeout=${FERRIX_PIXEL9A_ACM_TIMEOUT:-120}

usage() {
	printf 'usage: tools/pixel-9a/a2dp-active-proof.sh [dry|play]\n' >&2
	printf 'environment: FERRIX_SHELL (CDC-ACM node) FERRIX_PIXEL9A_ACM_TIMEOUT (seconds, 30-300, default 120)\n' >&2
	printf '             FERRIX_PIXEL9A_A2DP_EVIDENCE (physical|mock) FERRIX_OUT_DIR\n' >&2
	printf '             play: FERRIX_PIXEL9A_A2DP_PLAY_CONFIRM (the phrase printed by the latest dry run)\n' >&2
	exit 2
}

case "$#:${1:-}" in
	0:|1:dry) mode=dry ;;
	1:play) mode=play ;;
	*) usage ;;
esac
case "${FERRIX_PIXEL9A_A2DP_EVIDENCE:-physical}" in
	physical) evidence="physical:$target_id" ;;
	mock) evidence=mock ;;
	*) usage ;;
esac
case "$acm_timeout" in ''|*[!0-9]*) usage ;; esac
[ "$acm_timeout" -ge 30 ] && [ "$acm_timeout" -le 300 ] || usage

transcript=''
die() {
	code=${2:-1}
	[ -z "$transcript" ] || printf 'result=refused\nreason=%s\n' "$1" >>"$transcript"
	printf 'REFUSING pixel-9a A2DP active proof (%s): %s\n' "$mode" "$1" >&2
	printf '{"event":"pixel-9a-a2dp-active-proof","evidence":"%s","mode":"%s","status":"failed","reason":"%s","audio_capability":"unsupported","exit":%s}\n' \
		"$evidence" "$mode" "$(printf '%s' "$1" | tr '"\\' "''")" "$code" >&2
	exit "$code"
}

for tool in python3 awk sed od mv; do
	command -v "$tool" >/dev/null 2>&1 || die "missing host tool '$tool'; install it explicitly" 2
done
for file in "$body" "$preflight_body" "$preflight_host" "$transport"; do
	[ -f "$file" ] && [ ! -L "$file" ] || die "$(basename "$file") is missing or linked" 2
done
[ "$mode" != play ] || [ -n "${FERRIX_PIXEL9A_A2DP_PLAY_CONFIRM:-}" ] ||
	die 'set FERRIX_PIXEL9A_A2DP_PLAY_CONFIRM to the exact phrase printed by the latest dry run' 2

sha256() { (shasum -a 256 "$1" 2>/dev/null || sha256sum "$1") | cut -c1-64; }
hex() { od -An -N"$1" -tx1 /dev/urandom | tr -cd '0-9a-f'; }
now() { date -u +%s; }

# library: the v3 host's pinned constants, key list, validator and blocker
# logic, unchanged. Both the guest (before the tone) and this host (after
# the session) judge the preflight with exactly this code.
library() {
	grep -E '^(pinned_kernel|reviewed_pipewire|reviewed_wireplumber)=' "$preflight_host"
	awk '/^expected_keys=/,/probe_complete.$/' "$preflight_host"
	awk '/^validate\(\) \{/,/^}/' "$preflight_host"
	grep -E '^(get|is_n)\(\) \{.*\}$' "$preflight_host"
	awk '/^preflight_problems\(\) \{/,/^}/' "$preflight_host"
}

umask 077
mkdir -p "$out"
work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-a2dp-active-proof.XXXXXX")
trap 'rm -rf "$work"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
library >"$work/library.sh"
for fn in validate get is_n preflight_problems; do
	[ "$(grep -c "^$fn() {" "$work/library.sh")" -eq 1 ] || die "cannot extract $fn from the v3 preflight host" 2
done
for var in pinned_kernel reviewed_pipewire reviewed_wireplumber expected_keys; do
	[ "$(grep -c "^$var=" "$work/library.sh")" -eq 1 ] || die "cannot extract $var from the v3 preflight host" 2
done
sh -n "$work/library.sh" || die 'the extracted v3 library does not parse' 2
. "$work/library.sh"

stamp=$(date -u +%Y%m%dT%H%M%SZ)
transcript="$out/$mode-$stamp-$$.txt"
{
	printf '# Pixel 9a Bluetooth A2DP active proof (%s), %s UTC\n' "$mode" "$stamp"
	printf '# evidence: %s\n' "$evidence"
	printf '# Counts, classes and a salted binding only: no address, name, serial, port name, salt, or audio. Never commit this file.\n'
	printf 'target_id=%s\nevidence=%s\nmode=%s\nsource=cdc-acm\n' "$target_id" "$evidence" "$mode"
	printf 'active_body_sha256=%s\npreflight_body_sha256=%s\nlibrary_sha256=%s\n' \
		"$(sha256 "$body")" "$(sha256 "$preflight_body")" "$(sha256 "$work/library.sh")"
} >"$transcript"
emit() {
	printf '%s=%s\n' "$1" "$2" >>"$transcript"
	printf '%s=%s\n' "$1" "$2"
}

# phrase BINDING DECISION_ID
phrase() {
	printf 'play one 1.5 s 750 Hz tone on the bound A2DP sink binding %s decision %s on %s' \
		"$(printf '%s' "$1" | cut -c1-12)" "$2" "$target_id"
}
dget() { sed -n "s/^$1=//p" "$2" | sed -n '1p'; }

# --- play: consume the pending decision before any session ---------------------
salt=''
expect=none
decision_id=''
if [ "$mode" = play ]; then
	[ -f "$decision" ] && [ ! -L "$decision" ] || die 'no pending decision; run a dry preflight first (a decision is used at most once)' 2
	salt=$(dget salt "$decision")
	expect=$(dget binding "$decision")
	decision_id=$(dget decision "$decision")
	created=$(dget created "$decision")
	printf '%s\n' "$salt" | grep -Eqx '[0-9a-f]{32}' && printf '%s\n' "$expect" | grep -Eqx '[0-9a-f]{64}' &&
		printf '%s\n' "$decision_id" | grep -Eqx '[0-9a-f]{8}' && printf '%s\n' "$created" | grep -Eqx '[0-9]+' ||
		die 'the pending decision is malformed; run a dry preflight again' 2
	[ "${FERRIX_PIXEL9A_A2DP_PLAY_CONFIRM}" = "$(phrase "$expect" "$decision_id")" ] ||
		die 'confirmation does not match the pending decision; run a dry preflight and review its context' 2
	consumed="$out/decision-consumed-$decision_id"
	[ ! -e "$consumed" ] || die 'this decision was already consumed' 2
	mv "$decision" "$consumed" || die 'cannot consume the pending decision' 2
	printf 'decision=%s\ncreated=%s\nconsumed=%s\n' "$decision_id" "$created" "$(now)" >"$consumed"
	age=$(($(now) - created))
	[ "$age" -ge 0 ] && [ "$age" -le "$decision_max_age" ] ||
		die "the decision is ${age}s old (limit ${decision_max_age}s) and is now consumed; run a dry preflight again" 2
	emit decision "$decision_id"
	emit decision_consumed yes
else
	rm -f "$decision"
	salt=$(hex 16)
	decision_id=$(hex 4)
	[ "${#salt}" -eq 32 ] && [ "${#decision_id}" -eq 8 ] || die 'host random source unavailable' 2
fi

# --- one session ------------------------------------------------------------------
composed="$work/session.sh"
{
	printf 'proof_mode=%s\nproof_salt=%s\nproof_expect=%s\n' "$mode" "$salt" "$expect"
	cat "$work/library.sh"
	printf 'preflight_body() {\n'
	cat "$preflight_body"
	printf '}\n'
	cat "$body"
} >"$composed"
session_status=0
python3 "$transport" --probe "$composed" --timeout "$acm_timeout" --max-bytes "$max_output" --stderr discard \
	>"$work/guest.out" 2>"$work/transport.log" || session_status=$?
rm -f "$composed"
[ "$mode" = dry ] || salt=''

# observe: after a lost play session, one read-only v3 preflight shows whether
# a stream, an opener or a speaker-path control was left behind.
observe() {
	o_status=0
	python3 "$transport" --probe "$preflight_body" --timeout 120 --max-bytes 65536 --stderr discard \
		>"$work/observe.out" 2>/dev/null || o_status=$?
	if [ "$o_status" -eq 0 ] && validate "$work/observe.out" >/dev/null; then
		{ printf '## post-failure read-only preflight\n'; cat "$work/observe.out"; } >>"$transcript"
		for key in graph_streams watch.tdm_0_rx_mixer_ep1 watch.tas25xx_asi_switch pcm_opener_scan playback_pcm_openers; do
			emit "observe.$key" "$(get "$key" "$work/observe.out")"
		done
		return 0
	fi
	emit observe unavailable
}
case "$session_status" in
	0|6) ;;
	255)
		reason=$(sed -n 's/^ferrix-acm: .* reason=\([a-z-]*\)$/\1/p' "$work/transport.log" | sed -n '1p')
		if [ "$mode" = play ]; then
			emit route_safety unverified
			observe
			die "play session transport failed (${reason:-unknown}); the tone may have started; check the observation above" 7
		fi
		die "guest CDC-ACM shell transport failed (${reason:-unknown})" 3 ;;
	4) die 'guest is not the exact tegu board (device-tree gate refused)' 4 ;;
	*)
		[ "$mode" != play ] || { emit route_safety unverified; observe; }
		die "guest active-proof probe failed (exit $session_status)" "$([ "$mode" = play ] && echo 7 || echo 3)" ;;
esac
[ "$(wc -c <"$work/guest.out" | tr -d ' ')" -le "$max_output" ] || die 'probe output exceeds the reviewed size limit' 5

# --- validate: two v3 blocks by the v3 validator, the rest by this key set ---------
awk -v dir="$work" '
	$0 == "active_section=pre-begin" { f = dir "/pre.out"; printf "" > f; next }
	$0 == "active_section=post-begin" { f = dir "/post.out"; printf "" > f; next }
	$0 == "active_section=pre-end" || $0 == "active_section=post-end" { f = ""; next }
	f != "" { print > f; next }
	{ print > (dir "/active.out") }' "$work/guest.out"
[ -f "$work/active.out" ] || die 'malformed probe output (no active keys)' 5
active_keys='active_probe active_probe_version active_mode scratch guard_armed preflight_exit preflight identity
sink_profile_class sink_codec binding binding_match kernel_guards_armed guard_review tone_started tone_exit tone_watchdog
graph_final graph_own_node graph_own_links graph_link_active guard_trip_graph guard_trip_pcm guard_trip_ctl guard_trip_live
post_exit scratch_removed active_refusal active_complete'
problems=$(printf '%s\n' $active_keys | awk '
	NR == FNR { want[$0] = 1; next }
	{
		eq = index($0, "=")
		if (eq < 2) { print "line without a key"; bad = 1; next }
		k = substr($0, 1, eq - 1); v = substr($0, eq + 1)
		if (!(k in want)) { print "unexpected key " k; bad = 1; next }
		if (k in seen) { print "duplicate key " k; bad = 1; next }
		seen[k] = 1; last = k
		if (k == "binding") { if (v !~ /^[0-9a-f]+$/ || length(v) != 64) { print "binding is not a SHA-256"; bad = 1 }; next }
		if (v !~ /^[A-Za-z0-9 .,+\/-]*$/) { print "value outside the reviewed character set for " k; bad = 1 }
		if (v ~ /[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]/) {
			print "identifier-shaped value for " k; bad = 1
		}
	}
	END {
		if (last != "active_complete") print "active_complete is not last"
		exit (bad || last != "active_complete")
	}' - "$work/active.out") || die "malformed probe output ($(printf '%s\n' "$problems" | sed -n '1p'))" 5
a() { get "$1" "$work/active.out"; }
[ "$(a active_probe)" = a2dp-active-proof ] && [ "$(a active_probe_version)" = 1 ] && [ "$(a active_mode)" = "$mode" ] ||
	die 'guest did not report the reviewed active probe in this mode' 5
cat "$work/active.out" >>"$transcript"

# pre_check FILE NAME: a valid v3 block from the exact board, else exit 5/4.
pre_check() {
	p=$(validate "$1") || die "malformed $2 preflight block ($(printf '%s\n' "$p" | sed -n '1p'))" 5
	[ "$(get probe "$1")" = a2dp-output-preflight ] && [ "$(get probe_version "$1")" = 3 ] ||
		die "the $2 block is not the reviewed v3 preflight" 5
	[ "$(get dt_model "$1")" = "$dt_model" ] && [ "$(get dt_compatible "$1")" = "$dt_compatible" ] ||
		die "the $2 block does not carry the exact tegu identity" 4
	{ printf '## %s preflight block\n' "$2"; cat "$1"; } >>"$transcript"
}

# --- refusals ------------------------------------------------------------------------
if [ "$(a active_complete)" = refused ]; then
	[ "$session_status" -eq 6 ] || die 'guest refused but did not exit 6' 5
	[ "$(a tone_started)" = no ] || die 'guest refused after reporting a started tone' 5
	if [ -f "$work/pre.out" ]; then
		pre_check "$work/pre.out" in-session
		p=$(preflight_problems "$work/pre.out")
		[ -z "$p" ] || emit preflight_problems "$p"
	fi
	emit active_refusal "$(a active_refusal)"
	emit tone_started no
	emit audio_capability unsupported
	cp "$transcript" "$out/$mode-latest.txt"
	printf 'transcript=%s\n' "$transcript"
	die "guest refused before any tone ($(a active_refusal))" 6
fi
[ "$session_status" -eq 0 ] && [ "$(a active_complete)" = yes ] || die 'guest did not complete the active probe' 5
[ -f "$work/pre.out" ] || die 'malformed probe output (no in-session preflight block)' 5
pre_check "$work/pre.out" in-session
p=$(preflight_problems "$work/pre.out")
[ -z "$p" ] || die "guest went past a blocked in-session preflight ($p)" 5
[ "$(a preflight)" = candidate-observed ] && [ "$(a identity)" = ok ] || die 'guest went past an unconfirmed identity' 5
for key in kernel_release pkg.pipewire pkg.wireplumber a2dp_sink_codec a2dp_sink_default graph_filters \
	watch.tdm_0_rx_mixer_ep1 watch.tas25xx_asi_switch playback_pcm_openers; do
	emit "pre.$key" "$(get "$key" "$work/pre.out")"
done

# --- dry: record the decision and print its context ------------------------------------
if [ "$mode" = dry ]; then
	binding=$(a binding)
	[ -n "$binding" ] && [ "$(a tone_started)" = no ] && [ "$(a scratch_removed)" = yes ] ||
		die 'dry run did not return a binding with nothing started and the scratch removed' 5
	printf 'decision=%s\ncreated=%s\nsalt=%s\nbinding=%s\n' "$decision_id" "$(now)" "$salt" "$binding" >"$decision.tmp"
	mv -f "$decision.tmp" "$decision"
	salt=''
	emit sink_profile_class "$(a sink_profile_class)"
	emit sink_codec "$(a sink_codec)"
	emit decision "$decision_id"
	emit binding_prefix "$(printf '%s' "$binding" | cut -c1-12)"
	emit decision_status ready
	emit tone_started no
	emit audio_capability unsupported
	cp "$transcript" "$out/dry-latest.txt"
	printf 'transcript=%s\n' "$transcript"
	printf '\nDecision context (dry run; nothing was played or changed on %s, %s):\n' "$target_id" "$evidence"
	printf '  in-session v3 preflight candidate-observed: kernel %s, PipeWire %s, WirePlumber %s\n' \
		"$(get kernel_release "$work/pre.out")" "$(get pkg.pipewire "$work/pre.out")" "$(get pkg.wireplumber "$work/pre.out")"
	printf '  one paired, connected Audio Sink; one bluez A2DP sink (%s, codec %s, default %s) naming it; filters %s (bound BlueZ loopback only)\n' \
		"$(a sink_profile_class)" "$(a sink_codec)" "$(get a2dp_sink_default "$work/pre.out")" "$(get graph_filters "$work/pre.out")"
	printf '  no ALSA/other sink, no stream, speaker controls off, zero playback PCM openers (complete scan)\n'
	printf '  play would send 1.5 s of a generated 750 Hz sine, u8 mono 48 kHz, peak about -20 dBFS, no volume or mixer write,\n'
	printf '  through pw-cat --target <that sink serial> with target.object, node.dont-fallback, node.dont-reconnect, node.dont-move,\n'
	printf '  node.linger=false, state.restore-props=false and state.restore-target=false, after a fresh in-session preflight,\n'
	printf '  an identical binding, armed graph/PCM/control guards, and no graph change since the graph guard armed.\n'
	printf '  Any guard trip kills the tone. Internal speaker and earpiece stay NO-GO. Expires in %s s; usable once.\n' "$decision_max_age"
	printf 'Only after an explicit owner decision for this one run:\n'
	printf "  FERRIX_PIXEL9A_A2DP_PLAY_CONFIRM='%s' tools/pixel-9a/a2dp-active-proof.sh play\n" "$(phrase "$binding" "$decision_id")"
	printf '{"event":"pixel-9a-a2dp-active-proof","evidence":"%s","mode":"dry","status":"decision-ready","tone_started":false,"audio_capability":"unsupported"}\n' "$evidence"
	exit 0
fi

# --- play: classify ---------------------------------------------------------------------
[ "$(a binding_match)" = yes ] && [ "$(a guard_armed)" = yes ] && [ "$(a kernel_guards_armed)" = yes ] &&
	[ "$(a guard_review)" = ok ] && [ "$(a tone_started)" = yes ] ||
	die 'guest started a tone without reporting every gate' 5
for key in tone_exit tone_watchdog graph_final graph_own_node graph_own_links graph_link_active \
	guard_trip_graph guard_trip_pcm guard_trip_ctl guard_trip_live post_exit scratch_removed; do
	emit "$key" "$(a "$key")"
done
trips=''
for g in graph pcm ctl live; do
	v=$(a "guard_trip_$g")
	[ "$v" = none ] || trips="${trips:+$trips,}$g:$v"
done
if [ -n "$trips" ]; then
	route=tripped
elif [ "$(a graph_final)" = complete ]; then
	route=held
else
	route=unverified
fi
if [ "$(a tone_exit)" = 0 ] && [ "$(a tone_watchdog)" = idle ] && [ "$(a graph_own_node)" = yes ] &&
	is_n "$(a graph_own_links)" && [ "$(a graph_own_links)" -ge 1 ] && [ "$(a graph_link_active)" = yes ]; then
	delivery=linked-active
else
	delivery=not-delivered
fi
anomalies=''
anomaly() { anomalies="${anomalies:+$anomalies,}$1"; }
if [ -f "$work/post.out" ] && [ "$(a post_exit)" = 0 ]; then
	pre_check "$work/post.out" final
	post="$work/post.out"
	[ "$(get graph_streams "$post")" = 0 ] || anomaly stream-left
	[ "$(get watch.tdm_0_rx_mixer_ep1 "$post")" = off ] || anomaly speaker-route-not-off
	[ "$(get watch.tas25xx_asi_switch "$post")" = off ] || anomaly amp-switch-not-off
	[ "$(get pcm_opener_scan "$post")" = complete ] && [ "$(get playback_pcm_openers "$post")" = 0 ] || anomaly playback-pcm-opener
	n_alsa=$(get graph_sink_alsa "$post")
	[ "$n_alsa" = 0 ] || anomaly alsa-sink-present
	for key in graph_streams graph_sink_bluez_a2dp graph_sink_alsa watch.tdm_0_rx_mixer_ep1 watch.tas25xx_asi_switch \
		pcm_opener_scan playback_pcm_openers; do
		emit "post.$key" "$(get "$key" "$post")"
	done
	p=$(preflight_problems "$post")
	emit post_preflight "$([ -z "$p" ] && echo candidate-observed || echo blocked)"
	[ -z "$p" ] || emit post_preflight_problems "$p"
else
	anomaly final-observation-missing
fi
[ "$(a scratch_removed)" = yes ] || anomaly scratch-left
if [ -z "$anomalies" ]; then cleanup=clean; else cleanup=anomaly; fi
emit route_safety "$route"
[ -z "$trips" ] || emit route_trips "$trips"
emit tone_delivery "$delivery"
emit cleanup "$cleanup"
[ -z "$anomalies" ] || emit cleanup_anomalies "$anomalies"
emit audible_confirmation owner-pending
emit audio_capability unsupported
if [ "$route" = held ] && [ "$cleanup" = clean ]; then
	if [ "$delivery" = linked-active ]; then result=route-held-tone-delivered; code=0; else result=route-held-tone-not-delivered; code=8; fi
else
	result=route-not-verified; code=7
fi
emit result "$result"
cp "$transcript" "$out/play-latest.txt"
printf 'transcript=%s\n' "$transcript"
[ "$code" -ne 0 ] || printf 'Route safety held. Whether the tone was audible is for the owner to confirm by ear; record it with the transcript.\n'
[ "$code" -ne 7 ] || printf 'A guard tripped or the route or cleanup was not verified. Do not repeat; review the transcript with the owner.\n'
printf '{"event":"pixel-9a-a2dp-active-proof","evidence":"%s","mode":"play","result":"%s","route_safety":"%s","tone_delivery":"%s","cleanup":"%s","audible_confirmation":"owner-pending","audio_capability":"unsupported"}\n' \
	"$evidence" "$result" "$route" "$delivery" "$cleanup"
exit "$code"
