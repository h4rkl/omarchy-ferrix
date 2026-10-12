#!/bin/sh
set -eu

# Issue #2 internal-microphone capture proof on an ALREADY booted Google
# Pixel 9a (tegu / GXQ96 / MP1.0) Ferrix dev guest, reached over the USB
# CDC-ACM bring-up shell (tools/pixel-9a/acm-probe.py).
#
#   mic-capture-proof.sh [preflight]   read-only; the default
#   mic-capture-proof.sh capture       one gated 2 s capture from hw:0,8
#   mic-capture-proof.sh acoustic      one gated 2 s capture while the host
#                                      plays its gated reference tone (#80)
#   mic-capture-proof.sh restore       gated recovery of the one mixer control
#
# preflight checks the exact device tree, the pinned Kleaf kernel, root,
# alsa-utils amixer/arecord, `od` byte order, one `aoc-snd-card`, that PCM 8
# is the single `00-08: EP1 capture (*)` capture line of /proc/asound/pcm
# with one substream, that `arecord -l` names the same PCM with `Subdevices:
# 1/1`, that any per-PCM procfs files agree and read `closed` (a kernel
# without them reports `not-exposed`, which is never read as closed), that
# no /proc/*/fd names /dev/snd/pcmC0D8c, that `EP1 TX Mixer INTERNAL_MIC_TX`
# is a single rw boolean reading `off`, and that the watched speaker controls
# `TDM_0_RX Mixer EP1` and `TAS25XX ASI Switch` read `off`. It prints the
# observed PCM id/name and the exact capture confirmation, which embeds a
# binding hash of the card driver, PCM 8 identity, mixer numid, and kernel.
# It changes nothing.
#
# capture needs that per-run typed confirmation
#   FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM='record 2s from hw:0,8 with EP1 TX Mixer INTERNAL_MIC_TX binding <12 hex> on google-pixel-9a-tegu-gxq96-mp1-0'
# Review the preflight's pcm_id/pcm_name (the source expectation is the EP1
# capture front end, audio-route-decision.md) before typing it. It runs a fresh
# preflight session, refuses unless the binding and every check still hold,
# then sends one capture session. That session sets only the microphone route
# on, records raw S16_LE 48 kHz stereo for 2 s (bounded by `timeout 8`) to
# /run/ferrix-mic-capture-proof/capture.raw, sets the route back to its
# recorded original `off` and verifies it, measures the PCM bytes, and
# deletes the file; a guest trap does the same on HUP/INT/TERM/exit. No
# speaker route, amplifier, TAS, or volume control is written and nothing is
# played. Only aggregate statistics are returned or kept.
#
# acoustic (issue #80) needs its own per-run typed confirmation
#   FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM='play the gated 1180 Hz host tone and record 2s from hw:0,8 with EP1 TX Mixer INTERNAL_MIC_TX binding <12 hex> on google-pixel-9a-tegu-gxq96-mp1-0'
# and the physical setup in audio-route-decision.md (host built-in speaker,
# modest volume set by the operator, fixed distance, quiet room). It refuses
# exactly as capture does, then writes the deterministic reference tone from
# mic-acoustic-tone.py (1180 Hz, -20 dBFS peak, 200 ms on / 200 ms off) to
# ignored out/, starts the host player (afplay, pw-play, or paplay, at the
# default device and existing volume; nothing selects a device or changes a
# volume), checks it is still playing after a 1 s lead-in, and only then
# sends one capture session in mode acoustic. The guest does everything
# capture does, and before deleting the file it also folds per-window
# Goertzel amplitudes at 1180 Hz and at control bins 1060/1280 Hz over the
# gate period. The host stops the player, deletes the tone file, and
# classifies each channel from those aggregates: the tone-off windows of the
# same recording are the baseline and the control bins are the spectral
# control. acoustic-tone-detected needs a gated (>= 10 dB on/off), narrowband
# (>= 10 dB over the control bins), cycle-consistent tone whose control bins
# are not gated with it, and a player that was still playing when the
# session returned. Anything else is inconclusive or a capture failure;
# never a pass. Nothing is played on the phone.
#
# If the capture session's transport fails, its own cleanup output is lost;
# the workflow then opens one read-only preflight session to observe the
# control and the proof directory and reports them. restore needs
#   FERRIX_PIXEL9A_MIC_RESTORE_CONFIRM='restore EP1 TX Mixer INTERNAL_MIC_TX off on google-pixel-9a-tegu-gxq96-mp1-0'
# and writes `off` to that one control only if it reads `on`, and removes the
# fixed proof directory.
#
# The result is a classification of the PCM bytes (digital-silence,
# constant-level, saturated, non-silent-low-level, non-silent-signal, or a
# capture failure). A non-silent result shows that the AoC path delivered
# varying samples; it does not prove acoustic pickup, which needs a controlled
# stimulus. capabilities.toml is never edited.
#
# Exit status: 0 completed (capture: non-silent PCM measured), 2 usage or
# missing/mismatched confirmation, 3 transport or guest failure, 4 not the
# exact tegu board, 5 malformed probe output, 6 refused before any state
# change, 7 cleanup not verified (route not restored or file not removed),
# 8 capture completed and cleaned up but no non-silent PCM was measured
# (acoustic: completed and cleaned up but inconclusive), 9 acoustic only:
# cleaned up, but the capture itself failed (no usable PCM to analyse).
# acoustic exits 0 only for acoustic-tone-detected.
#
# FERRIX_SHELL selects the host CDC-ACM node (default
# /dev/cu.usbmodemferrix_tegu3); it is never printed or retained. Set
# FERRIX_PIXEL9A_MIC_CAPTURE_EVIDENCE=mock for stubbed runs. Transcripts go
# to ignored out/pixel-9a/mic-capture-proof/ and hold no serials, port names,
# samples, or audio. Never commit them.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target_id=google-pixel-9a-tegu-gxq96-mp1-0
probe="$root/tools/pixel-9a/mic-capture-proof-remote.sh"
transport="$root/tools/pixel-9a/acm-probe.py"
out="${FERRIX_OUT_DIR:-$root/out}/pixel-9a/mic-capture-proof"
dt_model='ZUMA PRO TEGU MP on ZUMA PRO'
dt_compatible='google,ZUMA PRO TEGU'
pinned_kernel=6.1.124-android14-11-ga4662a84a15f
mic_control='EP1 TX Mixer INTERNAL_MIC_TX'
pcm_expected_id='EP1 capture (*)'
expected_bytes=384000
restore_phrase="restore $mic_control off on $target_id"
tone_tool="$root/tools/pixel-9a/mic-acoustic-tone.py"
tone_hz=1180
control_hz='1060 1280'
tone_window_frames=2400
tone_period_windows=8
tone_level_dbfs=-20
# Pre-registered acoustic decision thresholds (see audio-route-decision.md).
tone_min_lsb=0.5
tone_gate_db=10
tone_over_control_db=10
control_gate_db=6
baseline_tone_db=10
tone_min_cycles=4
max_output=65536
acm_timeout=${FERRIX_PIXEL9A_ACM_TIMEOUT:-120}

usage() {
	printf 'usage: tools/pixel-9a/mic-capture-proof.sh [preflight | capture | acoustic | restore]\n' >&2
	printf 'environment: FERRIX_SHELL (CDC-ACM node) FERRIX_PIXEL9A_ACM_TIMEOUT (seconds, 5-600, default 120)\n' >&2
	printf '             FERRIX_PIXEL9A_MIC_CAPTURE_EVIDENCE (physical|mock) FERRIX_OUT_DIR\n' >&2
	printf '             capture: FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM  restore: FERRIX_PIXEL9A_MIC_RESTORE_CONFIRM\n' >&2
	printf '             acoustic: FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM (FERRIX_PIXEL9A_ACM_TIMEOUT at most 120)\n' >&2
	exit 2
}

case "$#:${1:-}" in
	0:|1:preflight) mode=preflight ;;
	1:capture|1:acoustic|1:restore) mode=$1 ;;
	*) usage ;;
esac
case "${FERRIX_PIXEL9A_MIC_CAPTURE_EVIDENCE:-physical}" in
	physical) evidence="physical:$target_id" ;;
	mock) evidence=mock ;;
	*) usage ;;
esac
case "$acm_timeout" in ''|*[!0-9]*) usage ;; esac
[ "$acm_timeout" -ge 5 ] && [ "$acm_timeout" -le 600 ] || usage
# The reference tone must outlast the whole capture session.
[ "$mode" != acoustic ] || [ "$acm_timeout" -le 120 ] || usage

transcript=''
die() {
	code=${2:-1}
	[ -z "$transcript" ] || printf 'result=refused\nreason=%s\n' "$1" >>"$transcript"
	printf 'REFUSING pixel-9a mic capture proof: %s\n' "$1" >&2
	printf '{"event":"pixel-9a-mic-capture-proof","mode":"%s","evidence":"%s","status":"failed","reason":"%s","exit":%s}\n' \
		"$mode" "$evidence" "$(printf '%s' "$1" | tr '"\\' "''")" "$code" >&2
	exit "$code"
}

# The gates that need no guest state are checked before a file is written or
# the port is opened.
case "$mode" in
	capture)
		[ -n "${FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM:-}" ] ||
			die 'set FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM to the exact phrase printed by a preflight run' 2
		;;
	acoustic)
		[ -n "${FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM:-}" ] ||
			die 'set FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM to the exact acoustic phrase printed by a preflight run' 2
		;;
	restore)
		[ "${FERRIX_PIXEL9A_MIC_RESTORE_CONFIRM:-}" = "$restore_phrase" ] ||
			die "set FERRIX_PIXEL9A_MIC_RESTORE_CONFIRM='$restore_phrase' to restore the route once" 2
		;;
esac

for tool in python3 awk sed; do
	command -v "$tool" >/dev/null 2>&1 || die "missing host tool '$tool'; install it explicitly"
done
[ -f "$probe" ] && [ ! -L "$probe" ] || die 'guest probe script is missing or linked'
[ -f "$transport" ] && [ ! -L "$transport" ] || die 'CDC-ACM transport is missing or linked'
player=''
if [ "$mode" = acoustic ]; then
	[ -f "$tone_tool" ] && [ ! -L "$tone_tool" ] || die 'reference tone generator is missing or linked'
	for tool in afplay pw-play paplay; do
		command -v "$tool" >/dev/null 2>&1 && { player=$tool; break; }
	done
	[ -n "$player" ] || die 'no host tone player (afplay, pw-play, or paplay); install one explicitly'
fi

sha256() { (shasum -a 256 "$1" 2>/dev/null || sha256sum "$1") | cut -c1-64; }

mkdir -p "$out"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
transcript="$out/$mode-$stamp-$$.txt"
work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-mic-capture-proof.XXXXXX")
# acoustic: the reference tone lives under ignored out/ only while it plays.
stimulus_dir="$out/stimulus-$stamp-$$"
player_pid=''
# stop_player: stop the host tone if it still plays; sets player_alive.
stop_player() {
	player_alive=no
	[ -n "$player_pid" ] || return 0
	if kill -0 "$player_pid" 2>/dev/null; then
		player_alive=yes
		kill "$player_pid" 2>/dev/null || :
	fi
	wait "$player_pid" 2>/dev/null || :
	player_pid=''
}
host_cleanup() {
	stop_player
	rm -rf "$stimulus_dir" "$work"
}
trap host_cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
{
	printf '# Pixel 9a internal-microphone capture proof (%s), %s UTC\n' "$mode" "$stamp"
	printf '# evidence: %s\n' "$evidence"
	printf '# Aggregate statistics only: no serials, port names, samples, or audio. Never commit this file.\n'
	printf 'target_id=%s\nevidence=%s\nmode=%s\nsource=cdc-acm\n' "$target_id" "$evidence" "$mode"
	printf 'probe_body_sha256=%s\n' "$(sha256 "$probe")"
} >"$transcript"

emit() {
	printf '%s=%s\n' "$1" "$2" >>"$transcript"
	printf '%s=%s\n' "$1" "$2"
}

# session NAME MODE BINDING: send the two-line header and the exact probe body
# as the only content of one CDC-ACM session; output in $work/NAME.out. Sets
# session_status, and session_transport=failed on a transport failure.
session() {
	composed="$work/$1.sh"
	{
		printf 'proof_mode=%s\nproof_binding=%s\n' "$2" "$3"
		cat "$probe"
	} >"$composed"
	printf '%s_probe_sha256=%s\n' "$1" "$(sha256 "$composed")" >>"$transcript"
	session_status=0
	session_transport=ok
	python3 "$transport" --probe "$composed" --timeout "$acm_timeout" --max-bytes "$max_output" --stderr discard \
		>"$work/$1.out" 2>"$work/$1.log" || session_status=$?
	if [ "$session_status" -eq 255 ]; then
		session_transport=failed
		session_reason=$(sed -n 's/^ferrix-acm: .* reason=\([a-z-]*\)$/\1/p' "$work/$1.log" | sed -n '1p')
		return 0
	fi
	[ "$session_status" -ne 4 ] || die 'guest is not the exact tegu board (device-tree gate refused)' 4
	[ "$(wc -c <"$work/$1.out" | tr -d ' ')" -le "$max_output" ] || die 'probe output exceeds the reviewed size limit' 5
}

# validate FILE MODE: every line is a known, unique key with a value in the
# reviewed character set, and every required key is present.
validate() {
	awk -v mode="$2" '
		BEGIN {
			split("probe mode dt_model dt_compatible kernel_release uid tool.amixer tool.arecord tool.od tool.timeout tool.awk tool.sha256sum arecord_impl amixer_impl od_selftest alsa_cards card_driver pcm_list pcm_list_entries pcm_stream pcm_substreams pcm_id pcm_name pcm_procfs pcm_info_stream pcm_info_id pcm_info_name pcm_info_subdevices_count pcm_state pcm_openers pcm_opener_scan pcm_opener_procs arecord_list_exit arecord_list arecord_list_entries arecord_list_id arecord_list_name arecord_list_subdevices binding proof_dir probe_complete", g, " ")
			for (i in g) required[g[i]] = 1
			split("mic speaker_route amp_switch", c, " ")
			split("exit numid iface name type access values value", f, " ")
			for (i in c) for (j in f) required[c[i] "." f[j]] = 1
			split("refusal state_changed", o, " ")
			for (i in o) optional[o[i]] = 1
			if (mode == "restore") {
				split("restore_write mic_value_after proof_dir_removed", r, " ")
				for (i in r) optional[r[i]] = 1
			}
			if (mode == "capture" || mode == "acoustic") {
				split("enable_exit mic_value_enabled record_exit record_error restore_exit mic_value_after mixer_restored pcm_bytes pcm_container samples frames leading_zero_frames windows window_ac_rms_min window_ac_rms_max measure_exit pcm_bytes_expected raw_removed speaker_route_value_after amp_switch_value_after pcm_state_after pcm_openers_after pcm_opener_scan_after arecord_list_subdevices_after cleanup_restore_exit cleanup_mixer_restored cleanup_raw_removed", m, " ")
				for (i in m) optional[m[i]] = 1
				split("samples min max mean ac_rms zero clipped", s, " ")
				for (i in s) { optional["ch0." s[i]] = 1; optional["ch1." s[i]] = 1 }
			}
			if (mode == "acoustic") {
				split("tone_hz tone_control_hz tone_window_frames tone_period_windows tone_exit tone_windows", t, " ")
				for (i in t) optional[t[i]] = 1
				split("tone_phase tone_on tone_off control_on control_off rms_on rms_off tone_cycles_usable tone_cycles_gated", s, " ")
				for (i in s) { optional["ch0." s[i]] = 1; optional["ch1." s[i]] = 1 }
			}
		}
		{
			eq = index($0, "=")
			if (eq < 2) { bad = 1; next }
			key = substr($0, 1, eq - 1)
			value = substr($0, eq + 1)
			if (!(key in required) && !(key in optional)) bad = 1
			if (key in seen) bad = 1
			if (value !~ /^[A-Za-z0-9 ._:()*,+\/=-]*$/) bad = 1
			seen[key] = 1
		}
		END { if (bad) exit 1; for (k in required) if (!(k in seen)) exit 1 }
	' "$1"
}

get() {
	awk -v key="$2=" 'index($0, key) == 1 { print substr($0, length(key) + 1); exit }' "$1"
}

# check_common FILE: identity and completeness of any session's output.
check_common() {
	validate "$1" "$2" || die "$2 output has a malformed, unknown, duplicate, or missing field" 5
	[ "$(get "$1" probe)" = mic-capture-proof ] && [ "$(get "$1" mode)" = "$2" ] && [ "$(get "$1" probe_complete)" = yes ] ||
		die "$2 probe did not complete" 5
	[ "$(get "$1" dt_model)" = "$dt_model" ] && [ "$(get "$1" dt_compatible)" = "$dt_compatible" ] ||
		die 'probe reported a device tree other than the exact tegu board' 4
}

# preflight_problems FILE: the host's own fail-closed reading of a preflight.
preflight_problems() {
	p=''
	add() { p="$p${p:+,}$1"; }
	f=$1
	[ "$(get "$f" kernel_release)" = "$pinned_kernel" ] || add kernel-not-pinned
	[ "$(get "$f" uid)" = 0 ] || add not-root
	for t in amixer arecord od timeout awk sha256sum; do
		[ "$(get "$f" "tool.$t")" = present ] || add "$t-missing"
	done
	[ "$(get "$f" arecord_impl)" = alsa-utils ] || add arecord-not-alsa-utils
	[ "$(get "$f" amixer_impl)" = alsa-utils ] || add amixer-not-alsa-utils
	[ "$(get "$f" od_selftest)" = ok ] || add od-selftest-failed
	[ "$(get "$f" alsa_cards)" = 1 ] || add alsa-card-count-not-one
	[ "$(get "$f" card_driver)" = aoc-snd-card ] || add card-driver-not-aoc-snd-card
	# PCM 8 identity: the one 00-08 line of /proc/asound/pcm, then the same
	# id and name in `arecord -l`, then the per-PCM procfs files if exposed.
	[ "$(get "$f" pcm_list)" = ok ] && [ "$(get "$f" pcm_list_entries)" = 1 ] || add "pcm8-listing-$(get "$f" pcm_list)"
	[ "$(get "$f" pcm_stream)" = CAPTURE ] || add pcm8-not-capture
	[ "$(get "$f" pcm_substreams)" = 1 ] || add pcm8-capture-count-not-one
	[ "$(get "$f" pcm_id)" = "$pcm_expected_id" ] || add pcm8-id-not-ep1-capture
	[ "$(get "$f" arecord_list)" = ok ] && [ "$(get "$f" arecord_list_entries)" = 1 ] &&
		[ "$(get "$f" arecord_list_id)" = "$(get "$f" pcm_id)" ] && [ "$(get "$f" arecord_list_name)" = "$(get "$f" pcm_name)" ] ||
		add pcm8-arecord-listing-disagrees
	case "$(get "$f" pcm_procfs)" in
		not-exposed)
			# Nothing is inferred from absent files; every info field and the
			# state must say so.
			for k in pcm_info_stream pcm_info_id pcm_info_name pcm_info_subdevices_count pcm_state; do
				[ "$(get "$f" "$k")" = not-exposed ] || { add pcm8-procfs-inconsistent; break; }
			done
			;;
		exposed)
			[ "$(get "$f" pcm_info_stream)" = CAPTURE ] && [ "$(get "$f" pcm_info_id)" = "$(get "$f" pcm_id)" ] &&
				[ "$(get "$f" pcm_info_name)" = "$(get "$f" pcm_name)" ] &&
				[ "$(get "$f" pcm_info_subdevices_count)" = "$(get "$f" pcm_substreams)" ] || add pcm8-info-listing-disagree
			[ "$(get "$f" pcm_state)" = closed ] || add "pcm8-$(get "$f" pcm_state)"
			;;
		*) add pcm8-procfs-partial ;;
	esac
	# Openers: none in /proc/*/fd over a complete scan, and every substream
	# unopened according to the kernel.
	[ "$(get "$f" pcm_opener_scan)" = complete ] || add pcm8-opener-scan-incomplete
	case "$(get "$f" pcm_opener_procs)" in ''|0|*[!0-9]*) add pcm8-opener-scan-empty ;; esac
	[ "$(get "$f" pcm_openers)" = 0 ] || add pcm8-has-openers
	[ "$(get "$f" arecord_list_subdevices)" = 1/1 ] || add pcm8-subdevice-not-available
	[ "$(get "$f" mic.name)" = "$mic_control" ] || add mic-control-absent
	for c in mic speaker_route amp_switch; do
		[ "$(get "$f" "$c.exit")" = 0 ] && [ "$(get "$f" "$c.iface")" = MIXER ] && [ "$(get "$f" "$c.type")" = BOOLEAN ] &&
			[ "$(get "$f" "$c.values")" = 1 ] && case "$(get "$f" "$c.access")" in rw*) true ;; *) false ;; esac ||
			add "$c-control-shape-unexpected"
		[ "$(get "$f" "$c.value")" = off ] || add "$c-not-off"
	done
	case "$(get "$f" mic.numid)" in ''|*[!0-9]*) add mic-numid-malformed ;; esac
	printf '%s\n' "$(get "$f" binding)" | grep -Eqx '[0-9a-f]{64}' || add binding-unavailable
	[ "$(get "$f" proof_dir)" = absent ] || add proof-dir-present
	printf '%s\n' "$p"
}

report_preflight() {
	f=$1
	for key in kernel_release uid arecord_impl amixer_impl od_selftest alsa_cards card_driver pcm_list pcm_stream pcm_substreams \
		pcm_id pcm_name pcm_procfs pcm_state arecord_list arecord_list_subdevices pcm_openers pcm_opener_scan mic.numid mic.type mic.access mic.value speaker_route.value amp_switch.value proof_dir; do
		emit "$key" "$(get "$f" "$key")"
	done
	emit kernel_pinned "$([ "$(get "$f" kernel_release)" = "$pinned_kernel" ] && echo yes || echo no)"
}

# --- preflight session (always first; read-only) ---------------------------------

session preflight preflight ''
[ "$session_transport" = ok ] || die "guest CDC-ACM shell transport failed (${session_reason:-unknown})" 3
[ "$session_status" -eq 0 ] || die "guest preflight probe failed (exit $session_status)" 3
pre="$work/preflight.out"
check_common "$pre" preflight
{ printf '## raw preflight output\n'; cat "$pre"; } >>"$transcript"
report_preflight "$pre"
problems=$(preflight_problems "$pre")
binding=$(get "$pre" binding)
capture_phrase="record 2s from hw:0,8 with $mic_control binding $(printf '%s' "$binding" | cut -c1-12) on $target_id"
acoustic_phrase="play the gated $tone_hz Hz host tone and $capture_phrase"

if [ "$mode" = preflight ]; then
	emit preflight_problems "${problems:-none}"
	emit preflight_status "$([ -z "$problems" ] && echo ready || echo blocked)"
	emit result preflight-only
	cp "$transcript" "$out/preflight-latest.txt"
	printf 'transcript=%s\n' "$transcript"
	if [ -z "$problems" ]; then
		printf 'Preflight changed nothing. After reviewing pcm_id/pcm_name above, one capture is:\n'
		printf "  FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM='%s' tools/pixel-9a/mic-capture-proof.sh capture\n" "$capture_phrase"
		printf 'One controlled-tone acoustic capture (separate owner approval; set up the host speaker as audio-route-decision.md says) is:\n'
		printf "  FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM='%s' tools/pixel-9a/mic-capture-proof.sh acoustic\n" "$acoustic_phrase"
	else
		printf 'Preflight changed nothing and is blocked; capture will refuse.\n'
	fi
	exit 0
fi

# --- gated restore ---------------------------------------------------------------

if [ "$mode" = restore ]; then
	case "$(get "$pre" mic.value)" in
		on) ;;
		off)
			[ "$(get "$pre" proof_dir)" = present ] || {
				emit restore_needed no
				emit result nothing-to-restore
				printf 'transcript=%s\n' "$transcript"
				exit 0
			}
			;;
		*) die 'microphone route value is neither on nor off; restore refuses' 6 ;;
	esac
	session restore restore ''
	[ "$session_transport" = ok ] || die "guest CDC-ACM shell transport failed during restore (${session_reason:-unknown}); run preflight" 3
	r="$work/restore.out"
	case "$session_status" in 0|6|7) ;; *) die "guest restore probe failed (exit $session_status)" 3 ;; esac
	check_common "$r" restore
	{ printf '## raw restore output\n'; cat "$r"; } >>"$transcript"
	[ "$session_status" -ne 6 ] || die "guest refused restore before changing any state ($(get "$r" refusal))" 6
	emit restore_write "$(get "$r" restore_write)"
	emit mic_value_after "$(get "$r" mic_value_after)"
	emit proof_dir_removed "$(get "$r" proof_dir_removed)"
	emit result "$([ "$session_status" -eq 0 ] && echo restored || echo restore-unverified)"
	cp "$transcript" "$out/restore-latest.txt"
	printf 'transcript=%s\n' "$transcript"
	[ "$session_status" -eq 0 ] || exit 7
	exit 0
fi

# --- gated capture ---------------------------------------------------------------

[ -z "$problems" ] || die "preflight refused the capture ($problems)" 6
if [ "$mode" = acoustic ]; then
	[ "${FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM}" = "$acoustic_phrase" ] ||
		die "confirmation does not match this preflight; expected FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM='$acoustic_phrase'" 2
else
	[ "${FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM}" = "$capture_phrase" ] ||
		die "confirmation does not match this preflight; expected FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM='$capture_phrase'" 2
fi
printf 'About to capture once on %s (%s):\n' "$target_id" "$evidence"
printf '  kernel %s; card driver %s; PCM hw:0,8 %s [%s], stream %s, procfs %s\n' "$(get "$pre" kernel_release)" \
	"$(get "$pre" card_driver)" "$(get "$pre" pcm_id)" "$(get "$pre" pcm_name)" "$(get "$pre" pcm_stream)" "$(get "$pre" pcm_procfs)"
printf '  write only %s (numid %s) off -> on -> off; speaker route and TAS switch stay off and unwritten\n' \
	"$mic_control" "$(get "$pre" mic.numid)"
printf '  arecord -t raw -f S16_LE -r 48000 -c 2 -d 2 into /run/ferrix-mic-capture-proof, measured, then deleted\n'
emit binding "$binding"

if [ "$mode" = acoustic ]; then
	stimulus_seconds=$((acm_timeout + 15))
	printf '  host plays %s Hz at %s dBFS peak, 200 ms on / 200 ms off, for at most %s s through %s at the current volume\n' \
		"$tone_hz" "$tone_level_dbfs" "$stimulus_seconds" "$player"
	mkdir -m 0700 "$stimulus_dir" || die 'cannot create the host stimulus directory' 6
	python3 "$tone_tool" --out "$stimulus_dir/tone.wav" --seconds "$stimulus_seconds" ||
		die 'reference tone generation failed' 6
	emit stimulus_player "$player"
	emit stimulus_tone_hz "$tone_hz"
	emit stimulus_level_dbfs "$tone_level_dbfs"
	emit stimulus_seconds "$stimulus_seconds"
	emit stimulus_sha256 "$(sha256 "$stimulus_dir/tone.wav")"
	"$player" "$stimulus_dir/tone.wav" </dev/null >/dev/null 2>&1 &
	player_pid=$!
	sleep 1
	kill -0 "$player_pid" 2>/dev/null || {
		stop_player
		rm -rf "$stimulus_dir"
		die "host tone player $player did not keep playing; nothing was changed on the guest" 6
	}
	emit stimulus_started yes
fi

session capture "$mode" "$binding"
c="$work/capture.out"
if [ "$mode" = acoustic ]; then
	stop_player
	rm -rf "$stimulus_dir"
	emit stimulus_alive_at_session_end "$player_alive"
	emit stimulus_removed "$([ ! -e "$stimulus_dir" ] && echo yes || echo no)"
fi
if [ "$session_transport" = failed ]; then
	# The capture session's own cleanup output is lost. Observe the state
	# read-only and report it; never retry the capture.
	emit capture_transport "failed-${session_reason:-unknown}"
	session verify preflight ''
	v="$work/verify.out"
	if [ "$session_transport" = ok ] && [ "$session_status" -eq 0 ] && validate "$v" preflight; then
		emit verify_mic_value "$(get "$v" mic.value)"
		emit verify_proof_dir "$(get "$v" proof_dir)"
		emit verify_pcm_state "$(get "$v" pcm_state)"
		emit verify_pcm_openers "$(get "$v" pcm_openers)"
		{ printf '## raw post-failure preflight output\n'; cat "$v"; } >>"$transcript"
		if [ "$(get "$v" mic.value)" = off ] && [ "$(get "$v" proof_dir)" = absent ]; then
			die "capture transport failed (${session_reason:-unknown}); a read-only check found the route off and no proof file" 3
		fi
		die "capture transport failed and the route or proof file was left behind; run: FERRIX_PIXEL9A_MIC_RESTORE_CONFIRM='$restore_phrase' tools/pixel-9a/mic-capture-proof.sh restore" 7
	fi
	die "capture transport failed and the post-failure check could not run; the route state is unknown; run preflight, then restore if needed" 7
fi
case "$session_status" in 0|6|7) ;; *) die "guest capture probe failed (exit $session_status)" 3 ;; esac
check_common "$c" "$mode"
{ printf '## raw capture-session output\n'; cat "$c"; } >>"$transcript"
if [ "$session_status" -eq 6 ]; then
	emit state_changed "$(get "$c" state_changed)"
	die "guest refused before changing any state ($(get "$c" refusal))" 6
fi

anomalies=''
anomaly() { anomalies="$anomalies${anomalies:+,}$1"; }
for key in enable_exit mic_value_enabled record_exit record_error restore_exit mic_value_after mixer_restored raw_removed \
	speaker_route_value_after amp_switch_value_after pcm_state_after pcm_openers_after pcm_opener_scan_after \
	arecord_list_subdevices_after measure_exit pcm_container pcm_bytes pcm_bytes_expected \
	samples frames leading_zero_frames windows window_ac_rms_min window_ac_rms_max; do
	emit "$key" "$(get "$c" "$key")"
done
for ch in ch0 ch1; do
	for stat in samples min max mean ac_rms zero clipped; do
		emit "$ch.$stat" "$(get "$c" "$ch.$stat")"
	done
done
[ "$(get "$c" mixer_restored)" = yes ] && [ "$(get "$c" mic_value_after)" = off ] || anomaly mic-route-not-restored
[ "$(get "$c" raw_removed)" = yes ] || anomaly raw-file-not-removed
[ "$(get "$c" speaker_route_value_after)" = off ] || anomaly speaker-route-not-off-after
[ "$(get "$c" amp_switch_value_after)" = off ] || anomaly amp-switch-not-off-after
# PCM 8 must be back as the preflight saw it: closed (or still not exposed),
# with no opener in /proc/*/fd or in the kernel's substream count.
case "$(get "$c" pcm_state_after)" in
	closed|not-exposed) [ "$(get "$c" pcm_state_after)" = "$(get "$pre" pcm_state)" ] || anomaly pcm8-state-changed-after ;;
	*) anomaly pcm8-not-closed-after ;;
esac
[ "$(get "$c" pcm_opener_scan_after)" = complete ] && [ "$(get "$c" pcm_openers_after)" = 0 ] &&
	[ "$(get "$c" arecord_list_subdevices_after)" = 1/1 ] || anomaly pcm8-opener-after
[ "$(get "$c" pcm_bytes_expected)" = "$expected_bytes" ] || die 'probe reported another expected byte count' 5
[ "$mode" != acoustic ] || [ ! -e "$stimulus_dir" ] || anomaly host-stimulus-not-removed

# --- classification of the PCM bytes ---------------------------------------------
bytes=$(get "$c" pcm_bytes)
case "$bytes" in ''|*[!0-9]*) bytes=0 ;; esac
if [ "$(get "$c" mic_value_enabled)" != on ]; then
	result=capture-failed-route-not-enabled
elif [ "$(get "$c" record_exit)" != 0 ]; then
	case "$(get "$c" record_error)" in
		none|'') result="capture-failed-exit-$(get "$c" record_exit)" ;;
		*) result="capture-failed-$(get "$c" record_error)" ;;
	esac
elif [ "$(get "$c" pcm_container)" != raw ]; then
	result=capture-failed-unexpected-container
elif [ "$bytes" -eq 0 ]; then
	result=no-pcm-data
else
	result=$(awk -v b="$bytes" -v want="$expected_bytes" \
		-v r0="$(get "$c" ch0.ac_rms)" -v r1="$(get "$c" ch1.ac_rms)" \
		-v z0="$(get "$c" ch0.zero)" -v z1="$(get "$c" ch1.zero)" \
		-v k0="$(get "$c" ch0.clipped)" -v k1="$(get "$c" ch1.clipped)" \
		-v n="$(get "$c" samples)" 'BEGIN {
			r = (r0 + 0 > r1 + 0) ? r0 + 0 : r1 + 0
			if (n + 0 == 0) { print "no-pcm-data"; exit }
			if (z0 + z1 == n + 0) cls = "digital-silence"
			else if (r < 0.5) cls = "constant-level"
			else if ((k0 + k1) / n > 0.01) cls = "saturated"
			else if (r < 32) cls = "non-silent-low-level"
			else cls = "non-silent-signal"
			if (b + 0 != want + 0) cls = "short-capture-" cls
			print cls
		}')
	peak_dbfs=$(awk -v r0="$(get "$c" ch0.ac_rms)" -v r1="$(get "$c" ch1.ac_rms)" 'BEGIN {
		r = (r0 + 0 > r1 + 0) ? r0 + 0 : r1 + 0
		if (r <= 0) print "-inf"; else printf "%.1f\n", 20 * log(r / 32768) / log(10)
	}')
	emit max_channel_ac_rms_dbfs "$peak_dbfs"
fi
emit mic_capture_result "$result"
if [ "$mode" = capture ]; then
	emit anomalies "${anomalies:-none}"
	emit acoustic_pickup unproven-without-controlled-stimulus
	emit capability_status 'unsupported (capabilities.toml not edited)'
	cp "$transcript" "$out/capture-latest.txt"
	printf 'transcript=%s\n' "$transcript"
	[ -z "$anomalies" ] && [ "$session_status" -eq 0 ] || exit 7
	case "$result" in non-silent-low-level|non-silent-signal) exit 0 ;; esac
	exit 8
fi

# --- acoustic classification (issue #80) -------------------------------------------
# The guest must have analysed the exact reviewed stimulus layout.
[ "$(get "$c" tone_hz)" = "$tone_hz" ] && [ "$(get "$c" tone_control_hz)" = "$(printf '%s' "$control_hz" | tr ' ' ',')" ] &&
	[ "$(get "$c" tone_window_frames)" = "$tone_window_frames" ] && [ "$(get "$c" tone_period_windows)" = "$tone_period_windows" ] ||
	die 'probe analysed another stimulus layout' 5
for key in tone_exit tone_windows; do emit "$key" "$(get "$c" "$key")"; done
for ch in ch0 ch1; do
	for stat in tone_phase tone_on tone_off control_on control_off rms_on rms_off tone_cycles_usable tone_cycles_gated; do
		emit "$ch.$stat" "$(get "$c" "$ch.$stat")"
	done
done
case "$result" in
	capture-failed-*) acoustic_result=$result ;;
	no-pcm-data|short-capture-*|digital-silence|constant-level) acoustic_result="capture-failed-$result" ;;
	*)
		if [ "$(get "$c" tone_exit)" != 0 ] || [ "$(get "$c" tone_windows)" != 40 ]; then
			acoustic_result=capture-failed-tone-analysis
		else
			acoustic_result=''
		fi
		;;
esac
if [ -z "$acoustic_result" ]; then
	# classify_channel CH: tone-detected, broadband-modulation,
	# inconsistent-gating (a gated narrowband tone in too few cycles),
	# contaminated-baseline, tone-not-detected, saturated, or digital-zero,
	# with the four ratios in dB. Amplitudes are floored at 0.001 LSB.
	classify_channel() {
		awk -v n="$(get "$c" "$1.samples")" -v z="$(get "$c" "$1.zero")" -v k="$(get "$c" "$1.clipped")" \
			-v on="$(get "$c" "$1.tone_on")" -v off="$(get "$c" "$1.tone_off")" \
			-v con="$(get "$c" "$1.control_on")" -v coff="$(get "$c" "$1.control_off")" \
			-v u="$(get "$c" "$1.tone_cycles_usable")" -v g="$(get "$c" "$1.tone_cycles_gated")" \
			-v minlsb="$tone_min_lsb" -v gate="$tone_gate_db" -v over="$tone_over_control_db" \
			-v cgate="$control_gate_db" -v base="$baseline_tone_db" -v mincy="$tone_min_cycles" '
			function db(a, b) { a += 0; b += 0; if (a < 0.001) a = 0.001; if (b < 0.001) b = 0.001; return 20 * log(a / b) / log(10) }
			BEGIN {
				snr = db(on, off); toc = db(on, con); cg = db(con, coff); bl = db(off, coff)
				if (n + 0 == 0 || z + 0 == n + 0) cls = "digital-zero"
				else if (k / n > 0.01) cls = "saturated"
				else if (on + 0 >= minlsb && snr >= gate && toc >= over && cg < cgate && u + 0 >= mincy && g + 0 >= u - 1) cls = "tone-detected"
				else if (snr >= gate - 4 && cg >= cgate) cls = "broadband-modulation"
				else if (on + 0 >= minlsb && snr >= gate && toc >= over) cls = "inconsistent-gating"
				else if (bl >= base) cls = "contaminated-baseline"
				else cls = "tone-not-detected"
				printf "%s %.1f %.1f %.1f %.1f\n", cls, snr, toc, cg, bl
			}'
	}
	classes=''
	for ch in ch0 ch1; do
		set -- $(classify_channel "$ch")
		emit "$ch.acoustic" "$1"
		emit "$ch.tone_gate_db" "$2"
		emit "$ch.tone_over_control_db" "$3"
		emit "$ch.control_gate_db" "$4"
		emit "$ch.baseline_tone_over_control_db" "$5"
		classes="$classes $1"
	done
	case "$classes" in
		*' tone-detected'*)
			if [ "$player_alive" = yes ]; then acoustic_result=acoustic-tone-detected
			else acoustic_result=inconclusive-stimulus-interrupted; fi
			;;
		' digital-zero digital-zero') acoustic_result=capture-failed-digital-silence ;;
		*' saturated'*) acoustic_result=inconclusive-saturated ;;
		*' broadband-modulation'*) acoustic_result=inconclusive-broadband-modulation ;;
		*' inconsistent-gating'*) acoustic_result=inconclusive-inconsistent-gating ;;
		*' contaminated-baseline'*) acoustic_result=inconclusive-contaminated-baseline ;;
		*)
			if [ "$player_alive" = yes ]; then acoustic_result=inconclusive-tone-not-detected
			else acoustic_result=inconclusive-stimulus-interrupted; fi
			;;
	esac
fi
case "$acoustic_result" in
	acoustic-tone-detected) acoustic_class=positive ;;
	inconclusive-*) acoustic_class=inconclusive ;;
	*) acoustic_class=failure ;;
esac
[ -z "$anomalies" ] && [ "$session_status" -eq 0 ] || acoustic_class=failure
emit acoustic_result "$acoustic_result"
emit acoustic_class "$acoustic_class"
emit anomalies "${anomalies:-none}"
if [ "$acoustic_class" = positive ]; then
	emit acoustic_pickup stimulus-locked-tone-under-documented-setup
else
	emit acoustic_pickup unproven
fi
emit capability_status 'unsupported (capabilities.toml not edited)'
cp "$transcript" "$out/acoustic-latest.txt"
printf 'transcript=%s\n' "$transcript"
[ -z "$anomalies" ] && [ "$session_status" -eq 0 ] || exit 7
case "$acoustic_class" in positive) exit 0 ;; inconclusive) exit 8 ;; esac
exit 9
