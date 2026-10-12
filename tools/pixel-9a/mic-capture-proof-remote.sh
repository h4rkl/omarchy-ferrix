#!/bin/sh
# Guest body of the issue #2 internal-microphone capture proof. Sent over the
# CDC-ACM shell (acm-probe.py, piped to the guest `sh`) by
# mic-capture-proof.sh, which prepends exactly two validated assignments:
#
#   proof_mode=preflight|capture|acoustic|restore
#   proof_binding=        (capture: the 64-hex binding the preflight printed)
#
# preflight is read-only. It checks the exact device tree, the pinned Kleaf
# kernel, root, alsa-utils `amixer`/`arecord`, `od` on a known two-sample
# input, and the single `aoc-snd-card`. PCM 8 is identified from its one
# `00-08` line in /proc/asound/pcm (direction, substream count, id `EP1
# capture (*)`), cross-checked with `arecord -l`, and with the per-PCM
# procfs files only when the kernel exposes them; a missing pcm8c directory
# is reported as `not-exposed`, never as closed. Openers are counted from
# /proc/*/fd and from the kernel's unopened-substream count. It reads three
# mixer controls with `amixer cget`: the microphone route
# `EP1 TX Mixer INTERNAL_MIC_TX` (shape and value) and, as watched read-only
# speaker controls, `TDM_0_RX Mixer EP1` and `TAS25XX ASI Switch`. It prints
# a binding (SHA-256 of the card driver, PCM 8's listing key, id, name,
# direction, substream count and procfs exposure, the microphone control
# numid, and the kernel release) for the host's typed confirmation.
#
# capture (only after the host's typed confirmation) repeats every preflight
# check and refuses unless the binding is unchanged. It then writes exactly
# one control, `EP1 TX Mixer INTERNAL_MIC_TX`, from its recorded original
# value `off` to `on`, records at most 2 s of raw S16_LE 48 kHz stereo from
# hw:0,8 into /run/ferrix-mic-capture-proof/capture.raw, writes the original
# value back and verifies it, measures the PCM bytes (raw, so there is no WAV
# header to skip), and deletes the file. An EXIT/HUP/INT/TERM trap restores
# the control and deletes the file if the session ends early. Only aggregate
# statistics leave the guest; no sample, byte, or audio content is printed.
#
# acoustic (issue #80; only after the host's own typed confirmation, while the
# host plays its gated reference tone) is capture with the same checks,
# write, recording, restore, and cleanup, plus one more aggregate pass over
# the same transient file before it is deleted: per 50 ms window, the
# Goertzel amplitude at the stimulus frequency and at two control
# frequencies, folded at the stimulus gating period (see tone_measure). Only
# the folded per-channel means and cycle counts leave the guest; the
# per-window values stay inside awk.
#
# restore (only after the host's typed confirmation) writes `off` to the same
# single control if it reads `on` and removes the fixed proof directory. It
# exists for a capture session whose transport failed before its cleanup
# could be observed.
#
# Nothing here plays audio, writes a speaker, amplifier, TAS, volume, or any
# other control, loads modules, or starts or stops a service.
set -u

dt_root=/proc/device-tree
asound_root=/proc/asound
proc_root=/proc
proof_dir=/run/ferrix-mic-capture-proof
pinned_kernel=6.1.124-android14-11-ga4662a84a15f
mic_control='EP1 TX Mixer INTERNAL_MIC_TX'
pcm_expected_id='EP1 capture (*)'
speaker_route='TDM_0_RX Mixer EP1'
amp_switch='TAS25XX ASI Switch'
card=0
device=8
rate=48000
channels=2
seconds=2
expected_bytes=384000
# acoustic: the host's reference tone (mic-acoustic-tone.py) is gated on for
# 200 ms and off for 200 ms. A 2400-frame (50 ms) window holds whole cycles
# of every analysed frequency, so the Goertzel bins are exact and DC-free.
tone_hz=1180
control_hz='1060 1280'
tone_window_frames=2400
tone_period_windows=8

model=$(tr '\000' '\n' <"$dt_root/model" 2>/dev/null | sed -n '1p')
compatible=$(tr '\000' '\n' <"$dt_root/compatible" 2>/dev/null)
[ "$model" = 'ZUMA PRO TEGU MP on ZUMA PRO' ] || exit 4
printf '%s\n' "$compatible" | grep -Fxq 'google,ZUMA PRO TEGU' || exit 4

case "${proof_mode:-}" in preflight|capture|acoustic|restore) ;; *) exit 5 ;; esac
case "${proof_binding:-}" in '') ;; *[!0-9a-f]*) exit 5 ;; esac

# Keep every reported value inside the host's reviewed character set.
clean() { tr -cd 'A-Za-z0-9 ._:()*,+/=-' | cut -c1-96; }
clean_lines() { while IFS= read -r line; do printf '%s\n' "$(printf '%s' "$line" | clean)"; done; }
have() { command -v "$1" >/dev/null 2>&1; }
kernel_release=$(uname -r | tr -cd 'A-Za-z0-9._+~-')
uid=$(id -u)

printf 'probe=mic-capture-proof\nmode=%s\n' "$proof_mode"
printf 'dt_model=%s\ndt_compatible=google,ZUMA PRO TEGU\n' "$model"
printf 'kernel_release=%s\nuid=%s\n' "$kernel_release" "$uid"

for tool in amixer arecord od timeout awk sha256sum; do
	if have "$tool"; then printf 'tool.%s=present\n' "$tool"; else printf 'tool.%s=missing\n' "$tool"; fi
done
# busybox has no arecord applet, but alsa-utils is the only reviewed one.
arecord_impl=missing
if have arecord; then
	if arecord --version 2>&1 </dev/null | grep -q '^arecord: version'; then arecord_impl=alsa-utils; else arecord_impl=other; fi
fi
amixer_impl=missing
if have amixer; then
	if amixer --version 2>&1 </dev/null | grep -q '^amixer version'; then amixer_impl=alsa-utils; else amixer_impl=other; fi
fi
printf 'arecord_impl=%s\namixer_impl=%s\n' "$arecord_impl" "$amixer_impl"
# The measurement reads samples with `od -t d2`, which uses host byte order:
# bytes 01 00 ff ff must read as 1 and -1 (little-endian, as S16_LE).
od_selftest=failed
[ "$(printf '\001\000\377\377' | od -An -v -t d2 2>/dev/null | awk '{ $1 = $1; print }')" = '1 -1' ] && od_selftest=ok
printf 'od_selftest=%s\n' "$od_selftest"

run_t() { if have timeout; then timeout "$@" </dev/null; else shift; "$@" </dev/null; fi; }

# --- card and PCM 8, read from procfs and the control interface only -------------
cards=$(awk '/^ *[0-9]+ \[/ { n++ } END { print n + 0 }' "$asound_root/cards" 2>/dev/null)
card_driver=$(awk -v c="$card" '$1 == c && /^ *[0-9]+ \[/ { sub(/^[^]]*\]: /, ""); sub(/ - .*$/, ""); print; exit }' \
	"$asound_root/cards" 2>/dev/null | clean)
printf 'alsa_cards=%s\ncard_driver=%s\n' "${cards:-unavailable}" "${card_driver:-unavailable}"
# PCM 8's identity comes from /proc/asound/pcm, whose lines the kernel writes
# as "CC-DD: id : name[ : playback N][ : capture N]". The pinned kernel lists
# it but has no /proc/asound/card0/pcm8c directory (physical preflight,
# 2026-10-12). Every line must carry that prefix; exactly one may be 00-08;
# it must split into id, name, and stream fields, all inside the reported
# character set. Anything else is malformed or ambiguous, never guessed.
pcm_key=$(printf '%02d-%02d' "$card" "$device")
pcm_listing=$( [ -r "$asound_root/pcm" ] || { echo 'pcm_list=absent'; exit 0; }
	awk -v want="$pcm_key" -v okre='^[A-Za-z0-9 ._:()*,+/=-]*$' '
	!/^[0-9][0-9]+-[0-9][0-9]+: / { bad = 1; next }
	index($0, want ": ") == 1 { n++; k = split(substr($0, length(want) + 3), f, " : ") }
	END {
		st = "missing-entry"; dir = "unavailable"; subs = "unavailable"; id = "unavailable"; nm = "unavailable"
		if (bad) st = "malformed"
		else if (n > 1) st = "ambiguous"
		else if (n == 1) {
			st = "malformed"
			if (f[1] != "" && f[1] ~ okre && f[2] ~ okre && length(f[1]) <= 64 && length(f[2]) <= 64) {
				if (k == 3 && f[3] ~ /^capture [0-9]+$/) { st = "ok"; dir = "CAPTURE"; subs = substr(f[3], 9) }
				else if (k == 3 && f[3] ~ /^playback [0-9]+$/) { st = "ok"; dir = "PLAYBACK"; subs = substr(f[3], 10) }
				else if (k == 4 && f[3] ~ /^playback [0-9]+$/ && f[4] ~ /^capture [0-9]+$/) { st = "ok"; dir = "DUPLEX"; subs = "unavailable" }
				if (st == "ok") { id = f[1]; nm = f[2] }
			}
		}
		printf "pcm_list=%s\npcm_list_entries=%d\npcm_stream=%s\npcm_substreams=%s\npcm_id=%s\npcm_name=%s\n", st, n, dir, subs, id, nm
	}' "$asound_root/pcm" 2>/dev/null)
get_l() { printf '%s\n' "$pcm_listing" | sed -n "s/^$1=//p" | sed -n '1p' | clean; }
pcm_list=$(get_l pcm_list)
pcm_stream=$(get_l pcm_stream)
pcm_substreams=$(get_l pcm_substreams)
pcm_id=$(get_l pcm_id)
pcm_name=$(get_l pcm_name)
printf 'pcm_list=%s\npcm_list_entries=%s\npcm_stream=%s\npcm_substreams=%s\npcm_id=%s\npcm_name=%s\n' "${pcm_list:-absent}" \
	"$(get_l pcm_list_entries)" "${pcm_stream:-unavailable}" "${pcm_substreams:-unavailable}" "${pcm_id:-unavailable}" "$pcm_name"
# The per-PCM procfs directory is optional. When it exists it must be whole
# and agree with the listing; when it does not, nothing is inferred from it.
pcm_dir="$asound_root/card$card/pcm${device}c"
info="$pcm_dir/info"
status_file="$pcm_dir/sub0/status"
pcm_procfs=partial
if [ ! -e "$pcm_dir" ] && [ ! -L "$pcm_dir" ]; then
	pcm_procfs=not-exposed
elif [ -r "$info" ] && [ -r "$status_file" ]; then
	pcm_procfs=exposed
fi
info_field() {
	[ "$pcm_procfs" = exposed ] || { echo not-exposed; return; }
	v=$(sed -n "s/^$1: //p" "$info" 2>/dev/null | sed -n '1p')
	if [ "$(printf '%s' "$v" | clean)" = "$v" ]; then printf '%s\n' "$v"; else echo malformed; fi
}
pcm_info_stream=$(info_field stream)
pcm_info_id=$(info_field id)
pcm_info_name=$(info_field name)
pcm_info_count=$(info_field subdevices_count)
printf 'pcm_procfs=%s\npcm_info_stream=%s\npcm_info_id=%s\npcm_info_name=%s\npcm_info_subdevices_count=%s\n' "$pcm_procfs" \
	"$pcm_info_stream" "$pcm_info_id" "$pcm_info_name" "$pcm_info_count"
pcm_state() {
	[ -e "$pcm_dir" ] || [ -L "$pcm_dir" ] || { echo not-exposed; return; }
	state=$(sed -n '1p' "$status_file" 2>/dev/null | clean)
	case "$state" in closed) echo closed ;; '') echo unavailable ;; state:*) echo "open-${state#state: }" | tr -cd 'A-Za-z0-9-' ;; *) echo other ;; esac
}
# `arecord -l` lists capture devices through the control device without
# opening any PCM. Its card 0 device 8 line must be the only one and name the
# same id and name; "Subdevices: A/N" is the kernel's count of unopened
# substreams, which also sees openers that /proc/*/fd cannot name.
arecord_listing() {
	alist=$(run_t 5 arecord -l 2>/dev/null)
	arc=$?
	printf '%s\n' "$alist" | awk -v rc="$arc" -v pre="card $card: " -v mid=", device $device: " \
		-v okre='^[A-Za-z0-9 ._:()*,+/=-]*$' '
		want_sub {
			want_sub = 0; s = $0; sub(/^ +/, "", s)
			if (s ~ /^Subdevices: [0-9]+\/[0-9]+$/) subs = substr(s, 13); else bad = 1
			next
		}
		index($0, pre) == 1 && (p = index($0, mid)) {
			n++; want_sub = 1; r = substr($0, p + length(mid)); q = 0
			for (i = length(r); i > 1; i--) if (substr(r, i - 1, 2) == " [") { q = i - 1; break }
			if (q && substr(r, length(r)) == "]") { id = substr(r, 1, q - 1); nm = substr(r, q + 2, length(r) - q - 2) } else bad = 1
		}
		END {
			st = "ok"
			if (rc != 0) st = "failed"; else if (n == 0) st = "missing-entry"; else if (n > 1) st = "ambiguous"
			else if (bad || subs == "" || id == "" || id !~ okre || nm !~ okre || length(id) > 64 || length(nm) > 64) st = "malformed"
			if (st != "ok") { id = "unavailable"; nm = "unavailable"; subs = "unavailable" }
			printf "arecord_list_exit=%s\narecord_list=%s\narecord_list_entries=%d\n", rc, st, n
			printf "arecord_list_id=%s\narecord_list_name=%s\narecord_list_subdevices=%s\n", id, nm, subs
		}' | clean_lines
}
# opener_scan: count /proc/*/fd links whose target is named pcmC<card>D<device>c
# in any directory (a node seen through another mount namespace still counts).
# The scan is complete only if /proc has at least one fd table, ls neither
# timed out nor failed to run, and every ls complaint is a process that
# exited mid-scan; a permission error or any unknown line makes it
# incomplete. Sets openers, opener_scan, and opener_procs.
opener_scan() {
	opener_procs=0
	for fd_dir in "$proc_root"/[0-9]*/fd; do [ -d "$fd_dir" ] && opener_procs=$((opener_procs + 1)); done
	scan=$(run_t 20 ls -l "$proc_root"/[0-9]*/fd/ 2>&1)
	scan_rc=$?
	set -- $(printf '%s\n' "$scan" | awk -v node="pcmC${card}D${device}c" '
		/^ls: / { if ($0 !~ /No such file or directory$/) other++; next }
		/^l[-r][-w][-xsS]/ && / -> / {
			t = $0; sub(/^.* -> /, "", t); m = split(t, part, "/")
			if (part[m] == node || part[m] == node " (deleted)") hits++
			next
		}
		/^total [0-9]+$/ || /^$/ || /\/fd\/?:$/ { next }
		{ other++ }
		END { print hits + 0, other + 0 }')
	openers=${1:-unavailable}
	opener_scan=complete
	case "$scan_rc" in 124|126|127|137|143) opener_scan=incomplete ;; esac
	[ "${2:-1}" = 0 ] && [ "$opener_procs" -gt 0 ] || opener_scan=incomplete
}
opener_scan
printf 'pcm_state=%s\npcm_openers=%s\npcm_opener_scan=%s\npcm_opener_procs=%s\n' "$(pcm_state)" "$openers" "$opener_scan" "$opener_procs"
alisting=$(arecord_listing)
printf '%s\n' "$alisting"

# --- mixer controls, read with `amixer cget` -----------------------------------
# ctl_read NAME PREFIX: print PREFIX.numid/iface/name/type/access/values/value.
ctl_read() {
	ctl_out=$(run_t 5 amixer -c "$card" cget "iface=MIXER,name='$1'" 2>/dev/null)
	ctl_rc=$?
	printf '%s\n' "$ctl_out" | awk -v p="$2" -v rc="$ctl_rc" -v q="'" '
		/^numid=/ {
			n = $0; sub(/^numid=/, "", n); sub(/,.*/, "", n)
			i = $0; sub(/^.*iface=/, "", i); sub(/,.*/, "", i)
			m = $0; k = index(m, "name=" q); m = k ? substr(m, k + 6) : ""
			if (substr(m, length(m)) == q) m = substr(m, 1, length(m) - 1)
		}
		/^ *; type=/ {
			t = $0; sub(/^.*type=/, "", t); sub(/,.*/, "", t)
			a = $0; sub(/^.*access=/, "", a); sub(/,.*/, "", a)
			c = $0; sub(/^.*values=/, "", c); sub(/,.*/, "", c)
		}
		/^ *: values=/ { v = $0; sub(/^ *: values=/, "", v) }
		END {
			printf "%s.exit=%s\n%s.numid=%s\n%s.iface=%s\n%s.name=%s\n", p, rc, p, n, p, i, p, m
			printf "%s.type=%s\n%s.access=%s\n%s.values=%s\n%s.value=%s\n", p, t, p, a, p, c, p, v
		}' | clean_lines
}
get() { printf '%s\n' "$1" | sed -n "s/^$2=//p" | sed -n '1p'; }

mic=$(ctl_read "$mic_control" mic)
spk=$(ctl_read "$speaker_route" speaker_route)
amp=$(ctl_read "$amp_switch" amp_switch)
printf '%s\n%s\n%s\n' "$mic" "$spk" "$amp"
mic_numid=$(get "$mic" mic.numid)
mic_value=$(get "$mic" mic.value)

# The binding ties the confirmation to PCM 8 as observed: its listing key,
# id, name, direction, substream count, and whether procfs exposes it.
binding=$(printf '%s\n' "$card_driver" "$pcm_key" "$pcm_id" "$pcm_name" "$pcm_stream" "$pcm_substreams" "$pcm_procfs" \
	"$mic_numid" "$kernel_release" |
	sha256sum 2>/dev/null | cut -c1-64)
printf 'binding=%s\n' "${binding:-unavailable}"
proof_dir_state=absent
[ ! -e "$proof_dir" ] && [ ! -L "$proof_dir" ] || proof_dir_state=present
printf 'proof_dir=%s\n' "$proof_dir_state"

if [ "$proof_mode" = preflight ]; then
	printf 'probe_complete=yes\n'
	exit 0
fi

# --- active modes: guest-side refusal before any state change -------------------
refusal=''
refuse() { refusal="$refusal${refusal:+,}$1"; }
ctl_ok() {
	# PREFIX NAME: a single boolean, readable/writable mixer control.
	[ "$(get "$2" "$1.exit")" = 0 ] && [ "$(get "$2" "$1.iface")" = MIXER ] &&
		[ "$(get "$2" "$1.type")" = BOOLEAN ] && [ "$(get "$2" "$1.values")" = 1 ] &&
		case "$(get "$2" "$1.access")" in rw*) true ;; *) false ;; esac
}
[ "$uid" = 0 ] || refuse not-root
[ "$amixer_impl" = alsa-utils ] || refuse amixer-not-alsa-utils
have timeout || refuse timeout-missing
[ "$(get "$mic" mic.name)" = "$mic_control" ] || refuse mic-control-name-mismatch
ctl_ok mic "$mic" || refuse mic-control-shape-unexpected
case "$mic_numid" in ''|*[!0-9]*) refuse mic-control-numid-malformed ;; esac
if [ "$proof_mode" = restore ]; then
	case "$mic_value" in on|off) ;; *) refuse mic-control-value-unexpected ;; esac
	if [ -n "$refusal" ]; then
		printf 'refusal=%s\nstate_changed=no\nprobe_complete=yes\n' "$refusal"
		exit 6
	fi
	restore_write=not-needed
	if [ "$mic_value" = on ]; then
		run_t 5 amixer -q -c "$card" cset "iface=MIXER,name='$mic_control'" off >/dev/null 2>&1
		restore_write=$?
	fi
	after=$(get "$(ctl_read "$mic_control" mic)" mic.value)
	[ ! -d "$proof_dir" ] || { rm -f "$proof_dir/capture.raw"; rmdir "$proof_dir" 2>/dev/null; }
	removed=yes
	[ ! -e "$proof_dir" ] || removed=no
	printf 'restore_write=%s\nmic_value_after=%s\nproof_dir_removed=%s\n' "$restore_write" "${after:-unavailable}" "$removed"
	printf 'probe_complete=yes\n'
	[ "$after" = off ] && [ "$removed" = yes ] || exit 7
	exit 0
fi

[ "$arecord_impl" = alsa-utils ] || refuse arecord-not-alsa-utils
[ "$od_selftest" = ok ] || refuse od-selftest-failed
[ "$kernel_release" = "$pinned_kernel" ] || refuse kernel-not-pinned
[ "$cards" = 1 ] || refuse alsa-card-count-not-one
[ "$card_driver" = aoc-snd-card ] || refuse card-driver-not-aoc-snd-card
[ "$pcm_list" = ok ] || refuse "pcm-listing-${pcm_list:-absent}"
[ "$pcm_stream" = CAPTURE ] || refuse pcm-not-capture
[ "$pcm_substreams" = 1 ] || refuse pcm-capture-count-not-one
[ "$pcm_id" = "$pcm_expected_id" ] || refuse pcm-id-not-ep1-capture
[ "$(get "$alisting" arecord_list)" = ok ] && [ "$(get "$alisting" arecord_list_id)" = "$pcm_id" ] &&
	[ "$(get "$alisting" arecord_list_name)" = "$pcm_name" ] || refuse pcm-arecord-listing-disagrees
case "$pcm_procfs:$(pcm_state)" in
	not-exposed:not-exposed) ;;
	exposed:closed)
		[ "$pcm_info_stream" = CAPTURE ] && [ "$pcm_info_id" = "$pcm_id" ] && [ "$pcm_info_name" = "$pcm_name" ] &&
			[ "$pcm_info_count" = "$pcm_substreams" ] || refuse pcm-info-listing-disagree
		;;
	exposed:*) refuse pcm-not-closed ;;
	*) refuse pcm-procfs-partial ;;
esac
# Fresh opener checks just before the first write: no /proc/*/fd link and
# every substream unopened according to the kernel.
opener_scan
[ "$opener_scan" = complete ] || refuse pcm-opener-scan-incomplete
[ "$openers" = 0 ] || refuse pcm-has-openers
[ "$(get "$(arecord_listing)" arecord_list_subdevices)" = 1/1 ] || refuse pcm-subdevice-not-available
[ "$mic_value" = off ] || refuse mic-control-not-off
ctl_ok speaker_route "$spk" && [ "$(get "$spk" speaker_route.value)" = off ] || refuse speaker-route-not-off-or-unreadable
ctl_ok amp_switch "$amp" && [ "$(get "$amp" amp_switch.value)" = off ] || refuse amp-switch-not-off-or-unreadable
[ "$proof_dir_state" = absent ] || refuse proof-dir-present
[ -n "$binding" ] && [ "$binding" = "${proof_binding:-}" ] || refuse binding-changed
if [ -n "$refusal" ]; then
	printf 'refusal=%s\nstate_changed=no\nprobe_complete=yes\n' "$refusal"
	exit 6
fi

# --- capture: the only state change, undone on every exit path ------------------
original=$mic_value
raw="$proof_dir/capture.raw"
mixer_changed=no
mixer_restored=no
cleanup() {
	trap '' HUP INT TERM
	if [ "$mixer_changed" = yes ] && [ "$mixer_restored" != yes ]; then
		run_t 5 amixer -q -c "$card" cset "iface=MIXER,name='$mic_control'" "$original" >/dev/null 2>&1
		printf 'cleanup_restore_exit=%s\n' "$?"
		[ "$(get "$(ctl_read "$mic_control" mic)" mic.value)" = "$original" ] && mixer_restored=yes
		printf 'cleanup_mixer_restored=%s\n' "$mixer_restored"
	fi
	if [ -e "$proof_dir" ]; then
		rm -f "$raw"
		rmdir "$proof_dir" 2>/dev/null
		[ -e "$proof_dir" ] && printf 'cleanup_raw_removed=no\n' || printf 'cleanup_raw_removed=yes\n'
	fi
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

mkdir -m 0700 "$proof_dir" 2>/dev/null || { printf 'refusal=proof-dir-create-failed\nstate_changed=no\nprobe_complete=yes\n'; exit 6; }
printf 'state_changed=proof-dir\n'
mixer_changed=yes
run_t 5 amixer -q -c "$card" cset "iface=MIXER,name='$mic_control'" on >/dev/null 2>&1
printf 'enable_exit=%s\n' "$?"
mic_on=$(get "$(ctl_read "$mic_control" mic)" mic.value)
printf 'mic_value_enabled=%s\n' "${mic_on:-unavailable}"

record_exit=not-run
record_error=none
if [ "$mic_on" = on ]; then
	err=$(run_t 8 arecord -q -D "hw:$card,$device" -t raw -f S16_LE -r "$rate" -c "$channels" -d "$seconds" "$raw" 2>&1)
	record_exit=$?
	case "$err" in
		'') ;;
		*'Device or resource busy'*) record_error=busy ;;
		*'Invalid argument'*|*'Broken configuration'*|*'non available'*|*'set_params'*|*'hw params'*) record_error=params-or-prepare ;;
		*'Input/output error'*) record_error=io ;;
		*overrun*) record_error=overrun ;;
		*) record_error=other ;;
	esac
	case "$record_exit" in 124|137|143) record_error=timeout ;; esac
fi
printf 'record_exit=%s\nrecord_error=%s\n' "$record_exit" "$record_error"

run_t 5 amixer -q -c "$card" cset "iface=MIXER,name='$mic_control'" "$original" >/dev/null 2>&1
printf 'restore_exit=%s\n' "$?"
after=$(get "$(ctl_read "$mic_control" mic)" mic.value)
[ "$after" = "$original" ] && mixer_restored=yes
printf 'mic_value_after=%s\nmixer_restored=%s\n' "${after:-unavailable}" "$mixer_restored"

# --- measure begin ---------------------------------------------------------------
# measure FILE: aggregate statistics of interleaved stereo S16_LE samples.
# Every byte of FILE is PCM (arecord -t raw); a RIFF header is refused rather
# than counted. Window values are the AC RMS of 100 ms (4800-frame) windows.
measure() {
	bytes=$(wc -c <"$1" 2>/dev/null | tr -d ' ')
	printf 'pcm_bytes=%s\n' "${bytes:-0}"
	if [ "$(head -c 4 "$1" 2>/dev/null)" = RIFF ]; then
		printf 'pcm_container=riff-refused\n'
		return 1
	fi
	printf 'pcm_container=raw\n'
	run_t 60 od -An -v -t d2 "$1" | awk -v ch=2 -v win=4800 '
		function emit_window() {
			if (wn == win * ch) {
				m = ws / wn; r = sqrt(ws2 / wn - m * m)
				if (wcount == 0 || r < wmin) wmin = r
				if (wcount == 0 || r > wmax) wmax = r
				wcount++
			}
			ws = 0; ws2 = 0; wn = 0
		}
		{
			for (f = 1; f <= NF; f++) {
				v = $f + 0; c = n % ch
				if (cnt[c] == 0 || v < lo[c]) lo[c] = v
				if (cnt[c] == 0 || v > hi[c]) hi[c] = v
				cnt[c]++; s[c] += v; s2[c] += v * v
				if (v == 0) z[c]++
				if (v >= 32767 || v <= -32768) clip[c]++
				if (v != 0) nonzero = 1
				if (!nonzero && c == ch - 1) lead++
				ws += v; ws2 += v * v; wn++
				if (wn == win * ch) emit_window()
				n++
			}
		}
		END {
			printf "samples=%d\nframes=%d\nleading_zero_frames=%d\n", n, int(n / ch), lead
			for (c = 0; c < ch; c++) {
				if (cnt[c] == 0) { printf "ch%d.samples=0\n", c; continue }
				m = s[c] / cnt[c]; r = s2[c] / cnt[c] - m * m; if (r < 0) r = 0
				printf "ch%d.samples=%d\nch%d.min=%d\nch%d.max=%d\n", c, cnt[c], c, lo[c], c, hi[c]
				printf "ch%d.mean=%.1f\nch%d.ac_rms=%.2f\n", c, m, c, sqrt(r)
				printf "ch%d.zero=%d\nch%d.clipped=%d\n", c, z[c] + 0, c, clip[c] + 0
			}
			printf "windows=%d\n", wcount
			if (wcount) printf "window_ac_rms_min=%.2f\nwindow_ac_rms_max=%.2f\n", wmin, wmax
		}'
}
# --- measure end -----------------------------------------------------------------

# --- tone measure begin -----------------------------------------------------------
# tone_measure FILE: aggregate stimulus-locked statistics of the same raw
# stereo S16_LE file. Per channel and 2400-frame window it computes the
# Goertzel amplitude (LSB) at tone_hz and at each control_hz, and the window
# AC RMS. The host tone is a square on/off gate of tone_period_windows
# windows (half on), whose phase relative to the recording is unknown, so
# the phase that maximises the on-minus-off tone amplitude is chosen. Means
# are then taken over the two core windows of each half (positions 1-2 on,
# 5-6 off), which lie wholly inside a half for any sub-window offset. A
# cycle is usable when it holds all four core windows and gated when its
# on-core tone mean exceeds twice its off-core mean. Per-window values
# never leave awk.
tone_measure() {
	run_t 60 od -An -v -t d2 "$1" | awk -v ch=2 -v win="$tone_window_frames" -v per="$tone_period_windows" \
		-v rate="$rate" -v f0="$tone_hz" -v fc="$control_hz" '
		BEGIN {
			nf = 1 + split(fc, cf, " "); fr[0] = f0; for (k = 1; k < nf; k++) fr[k] = cf[k]
			pi = atan2(0, -1)
			for (k = 0; k < nf; k++) co[k] = 2 * cos(2 * pi * fr[k] / rate)
		}
		function close_window(   c, k, p, m, r) {
			for (c = 0; c < ch; c++) {
				for (k = 0; k < nf; k++) {
					p = q1[c, k] * q1[c, k] + q2[c, k] * q2[c, k] - co[k] * q1[c, k] * q2[c, k]
					if (p < 0) p = 0
					amp[c, k, nw] = 2 * sqrt(p) / win
					q1[c, k] = 0; q2[c, k] = 0
				}
				m = ws[c] / win; r = ws2[c] / win - m * m; if (r < 0) r = 0
				rms[c, nw] = sqrt(r); ws[c] = 0; ws2[c] = 0
			}
			nw++; fn = 0
		}
		{
			for (i = 1; i <= NF; i++) {
				v = $i + 0; c = n % ch; n++
				for (k = 0; k < nf; k++) { q0 = v + co[k] * q1[c, k] - q2[c, k]; q2[c, k] = q1[c, k]; q1[c, k] = q0 }
				ws[c] += v; ws2[c] += v * v
				if (c == ch - 1 && ++fn == win) close_window()
			}
		}
		function core(p) { return p == 1 || p == 2 ? 1 : (p == 5 || p == 6 ? -1 : 0) }
		END {
			half = per / 2
			printf "tone_windows=%d\n", nw
			for (c = 0; c < ch; c++) {
				best = -1; bscore = 0
				for (ph = 0; ph < per; ph++) {
					sc = 0
					for (w = 0; w < nw; w++) sc += ((w + ph) % per < half ? 1 : -1) * amp[c, 0, w]
					if (best < 0 || sc > bscore) { best = ph; bscore = sc }
				}
				on = 0; off = 0; con = 0; coff = 0; ron = 0; roff = 0; non = 0; noff = 0
				delete cy_on; delete cy_off; delete cy_non; delete cy_noff
				for (w = 0; w < nw; w++) {
					p = (w + best) % per; y = int((w + best) / per); s = core(p)
					cs = 0; for (k = 1; k < nf; k++) cs += amp[c, k, w]; cs /= (nf - 1)
					if (s > 0) { on += amp[c, 0, w]; con += cs; ron += rms[c, w]; non++; cy_on[y] += amp[c, 0, w]; cy_non[y]++ }
					if (s < 0) { off += amp[c, 0, w]; coff += cs; roff += rms[c, w]; noff++; cy_off[y] += amp[c, 0, w]; cy_noff[y]++ }
				}
				usable = 0; gated = 0
				for (y in cy_non) if (cy_non[y] == 2 && cy_noff[y] == 2) {
					usable++
					if (cy_on[y] > 2 * cy_off[y]) gated++
				}
				if (!non) non = 1
				if (!noff) noff = 1
				printf "ch%d.tone_phase=%d\nch%d.tone_on=%.3f\nch%d.tone_off=%.3f\n", c, best, c, on / non, c, off / noff
				printf "ch%d.control_on=%.3f\nch%d.control_off=%.3f\n", c, con / non, c, coff / noff
				printf "ch%d.rms_on=%.2f\nch%d.rms_off=%.2f\n", c, ron / non, c, roff / noff
				printf "ch%d.tone_cycles_usable=%d\nch%d.tone_cycles_gated=%d\n", c, usable, c, gated
			}
		}'
}
# --- tone measure end -------------------------------------------------------------

measure_exit=not-run
if [ -f "$raw" ]; then
	measured=$(measure "$raw")
	measure_exit=$?
	printf '%s\n' "$measured"
else
	printf 'pcm_bytes=0\n'
fi
printf 'measure_exit=%s\npcm_bytes_expected=%s\n' "$measure_exit" "$expected_bytes"
if [ "$proof_mode" = acoustic ]; then
	printf 'tone_hz=%s\ntone_control_hz=%s\ntone_window_frames=%s\ntone_period_windows=%s\n' "$tone_hz" \
		"$(printf '%s' "$control_hz" | tr ' ' ',')" "$tone_window_frames" "$tone_period_windows"
	tone_exit=not-run
	if [ "$measure_exit" = 0 ]; then
		toned=$(tone_measure "$raw")
		tone_exit=$?
		printf '%s\n' "$toned"
	fi
	printf 'tone_exit=%s\n' "$tone_exit"
fi
rm -f "$raw"
rmdir "$proof_dir" 2>/dev/null
raw_removed=yes
[ ! -e "$proof_dir" ] || raw_removed=no
printf 'raw_removed=%s\n' "$raw_removed"

# Post-state: the watched speaker controls must be unchanged and PCM 8 closed.
printf 'speaker_route_value_after=%s\n' "$(get "$(ctl_read "$speaker_route" speaker_route)" speaker_route.value)"
printf 'amp_switch_value_after=%s\n' "$(get "$(ctl_read "$amp_switch" amp_switch)" amp_switch.value)"
printf 'pcm_state_after=%s\n' "$(pcm_state)"
opener_scan
printf 'pcm_openers_after=%s\npcm_opener_scan_after=%s\n' "$openers" "$opener_scan"
printf 'arecord_list_subdevices_after=%s\n' "$(get "$(arecord_listing)" arecord_list_subdevices)"
printf 'probe_complete=yes\n'
[ "$mixer_restored" = yes ] && [ "$raw_removed" = yes ] || exit 7
exit 0
