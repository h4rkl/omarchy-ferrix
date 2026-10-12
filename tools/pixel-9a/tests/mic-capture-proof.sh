#!/bin/sh
# Offline proof for the issue #2 internal-microphone capture proof workflow.
# No device is contacted, no audio is played or recorded, and no real mixer
# exists. Three layers:
#
#   1. the host workflow over the real CDC-ACM transport and a fake guest
#      (acm-fake-guest.sh) whose final `sh` answers from synthetic fixtures;
#   2. the exact guest probe, with only its five path/root constants
#      redirected into a fake procfs/device tree, run by the host /bin/sh
#      against fake `amixer`/`arecord` that keep mixer state in files and
#      play back synthetic PCM, including an interrupted recording;
#   3. one end-to-end capture: host workflow, transport, fake guest running
#      that redirected probe, fake tools.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
proof="$root/tools/pixel-9a/mic-capture-proof.sh"
probe="$root/tools/pixel-9a/mic-capture-proof-remote.sh"
target_id=google-pixel-9a-tegu-gxq96-mp1-0
fails=0
command -v python3 >/dev/null 2>&1 || { echo 'python3 is required for the CDC-ACM transport test' >&2; exit 1; }
status_before=$(cksum <"$root/docs/status.md")
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
check "static: host POSIX syntax" sh -n "$proof"
check "static: probe POSIX syntax" sh -n "$probe"
code=$(grep -vE '^[[:space:]]*#' "$probe")
host_code=$(grep -vE '^[[:space:]]*(#|printf )' "$proof")
verbs='adb|fastboot|ssh|scp|curl|dd|mount|umount|flash|erase|reboot|modprobe|insmod|rmmod|rc-service|rc-update|amixer|arecord|aplay'
offending=$(printf '%s\n' "$host_code" | grep -nE "(^|[;|&(]|then|do|else)[[:space:]]*($verbs)([[:space:]]|$)" || true)
check "static: the host runs no Android, storage, network, service, or audio command" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
offending=$(printf '%s\n' "$code" | grep -nwE 'aplay|speaker-test|pw-play|pw-cat|paplay|alsactl|alsaucm|wpctl|pactl|modprobe|insmod|rmmod|rc-service|mount|umount|dd' || true)
check "static: probe never plays audio or touches services, modules, UCM, or mounts" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
csets=$(printf '%s\n' "$code" | grep -E 'amixer .*cset' || true)
mic_cset="cset \"iface=MIXER,name='\$mic_control'\" "
check "static: every mixer write names only the microphone route" \
	sh -c '[ "$(printf "%s\n" "$1" | grep -c .)" -eq 4 ] && [ "$(printf "%s\n" "$1" | grep -cF "$2")" -eq 4 ]' sh "$csets" "$mic_cset"
check "static: the microphone route constant is the exact control" grep -Fxq "mic_control='EP1 TX Mixer INTERNAL_MIC_TX'" "$probe"
check "static: speaker and amplifier controls appear only as read-only names" \
	sh -c '[ "$(printf "%s\n" "$1" | grep -cE "TDM_0_RX|TAS25XX")" -eq 2 ] && ! printf "%s\n" "$1" | grep -E "cset.*(speaker_route|amp_switch)"' sh "$code"
check "static: no TAS, volume, or profile control is named anywhere in the probe" \
	sh -c '! printf "%s\n" "$1" | grep -E "PLAYBACK_VOLUME|CODEC PROFILE|Playback Volume|Capture Volume|Limiter"' sh "$code"
check "static: exactly one recording, raw, bounded, from hw:0,8 at 48 kHz stereo S16_LE for 2 s" \
	sh -c '[ "$(printf "%s\n" "$1" | grep -c "arecord -q")" -eq 1 ] &&
		printf "%s\n" "$1" | grep -Fq "run_t 8 arecord -q -D \"hw:\$card,\$device\" -t raw -f S16_LE -r \"\$rate\" -c \"\$channels\" -d \"\$seconds\" \"\$raw\"" &&
		grep -Fxq card=0 "$2" && grep -Fxq device=8 "$2" && grep -Fxq rate=48000 "$2" && grep -Fxq channels=2 "$2" && grep -Fxq seconds=2 "$2" &&
		grep -Fxq expected_bytes=384000 "$2"' sh "$code" "$probe"
check "static: the raw file lives only under the fixed /run proof directory" \
	sh -c 'grep -Fxq "proof_dir=/run/ferrix-mic-capture-proof" "$1" && grep -Fxq "raw=\"\$proof_dir/capture.raw\"" "$1"' sh "$probe"
check "static: the guest trap covers EXIT, HUP, INT, and TERM before the first write" \
	awk '/^trap cleanup EXIT/ { e = NR } /^trap .exit 129. HUP/ { h = NR } /^trap .exit 130. INT/ { i = NR } /^trap .exit 143. TERM/ { t = NR }
		/^mkdir -m 0700/ { m = NR } /cset .* on >/ && !w { w = NR }
		END { exit !(e && h && i && t && m && w && e < m && h < m && i < m && t < m && m < w) }' "$probe"
check "static: preflight returns before any write or recording" \
	sh -c 'printf "%s\n" "$1" | sed -n "1,/proof_mode\" = preflight/p" | grep -Eq "cset|arecord -q|mkdir" && exit 1;
		printf "%s\n" "$1" | grep -A3 "proof_mode\" = preflight" | grep -q "exit 0"' sh "$code"
check "static: the route is restored before the PCM is measured" \
	awk '/^run_t 5 amixer .*"\$original"/ { r = NR } /^\tmeasured=\$\(measure/ { m = NR } END { exit !(r && m && r < m) }' "$probe"
check "static: the only other arecord call is the read-only device listing" \
	sh -c '[ "$(printf "%s\n" "$1" | grep -E "run_t [0-9]+ arecord " | grep -vcE "run_t (5 arecord -l|8 arecord -q) ")" -eq 0 ] &&
		[ "$(printf "%s\n" "$1" | grep -c "run_t 5 arecord -l ")" -eq 1 ]' sh "$code"
check "static: guest and host require the same PCM 8 id" \
	sh -c '[ "$(sed -n "s/^pcm_expected_id=//p" "$1")" = "'"'"'EP1 capture (*)'"'"'" ] &&
		[ "$(sed -n "s/^pcm_expected_id=//p" "$1")" = "$(sed -n "s/^pcm_expected_id=//p" "$2")" ]' sh "$probe" "$proof"
check "static: guest and host pin the same Kleaf kernel" \
	sh -c '[ "$(sed -n "s/^pinned_kernel=//p" "$1")" = "$(sed -n "s/^pinned_kernel=//p" "$2")" ]' sh "$probe" "$proof"
check "static: the probe gates on the exact tegu device tree" \
	sh -c 'grep -Fq "ZUMA PRO TEGU MP on ZUMA PRO" "$1" && grep -Fq "google,ZUMA PRO TEGU" "$1"' sh "$probe"
check "static: the host never edits capabilities or claims acoustic proof" \
	sh -c '! grep -nE "capabilities\.toml\"|>.*capabilities|acoustic_pickup (proven|yes)" "$1"' sh "$proof"
probe_status=0
{ printf 'proof_mode=preflight\nproof_binding=\n'; cat "$probe"; } | sh >/dev/null 2>&1 || probe_status=$?
check "probe: refuses a host that is not the tegu board" [ "$probe_status" -eq 4 ]

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-mic-capture-proof-test.XXXXXX")
. "$root/tools/pixel-9a/tests/acm-fake-guest.sh"
trap 'acm_guest_stop; rm -rf "$work"' EXIT
mkdir -p "$work/fx" "$work/pcm"
unset FERRIX_PIXEL9A_ACM_TIMEOUT FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM FERRIX_PIXEL9A_MIC_RESTORE_CONFIRM 2>/dev/null || true

# --- synthetic PCM: 96000 stereo S16_LE frames unless noted ---------------------
python3 - "$work/pcm" <<'PY'
import math, os, struct, sys
d = sys.argv[1]
def write(name, frames, fn, header=False):
    data = b"".join(struct.pack("<hh", *fn(i)) for i in range(frames))
    with open(os.path.join(d, name), "wb") as f:
        if header:
            f.write(b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVEfmt " + struct.pack("<IHHIIHH", 16, 1, 2, 48000, 192000, 4, 16) + b"data" + struct.pack("<I", len(data)))
        f.write(data)
tone = lambda a: (lambda i: (int(a * math.sin(2 * math.pi * 1000 * i / 48000)), int(a * math.sin(2 * math.pi * 1000 * i / 48000 + 1))))
write("tone", 96000, tone(3000))
write("quiet", 96000, lambda i: ((i * 7919) % 9 - 4, (i * 104729) % 7 - 3))
write("zero", 96000, lambda i: (0, 0))
write("dc", 96000, lambda i: (-12, -12))
write("short", 24000, tone(3000))
write("wav", 96000, tone(3000), header=True)
PY

# --- layer 1: host workflow against fixtures ---------------------------------------
acm_guest_init "$root" "$work" "$probe"
binding=$(printf '%064d' 0 | sed 's/^000000000000/0123456789ab/')
ctl() {
	# PREFIX NAME VALUE [NUMID]
	printf '%s.exit=0\n%s.numid=%s\n%s.iface=MIXER\n%s.name=%s\n' "$1" "$1" "${4:-45}" "$1" "$1" "$2"
	printf '%s.type=BOOLEAN\n%s.access=rw------\n%s.values=1\n%s.value=%s\n' "$1" "$1" "$1" "$1" "$3"
}
common() {
	printf 'probe=mic-capture-proof\nmode=%s\ndt_model=ZUMA PRO TEGU MP on ZUMA PRO\ndt_compatible=google,ZUMA PRO TEGU\n' "$1"
	printf 'kernel_release=6.1.124-android14-11-ga4662a84a15f\nuid=0\n'
	for t in amixer arecord od timeout awk sha256sum; do printf 'tool.%s=present\n' "$t"; done
	printf 'arecord_impl=alsa-utils\namixer_impl=alsa-utils\nod_selftest=ok\n'
	# The observed physical shape: one 00-08 listing line, no pcm8c procfs.
	printf 'alsa_cards=1\ncard_driver=aoc-snd-card\npcm_list=ok\npcm_list_entries=1\npcm_stream=CAPTURE\npcm_substreams=1\n'
	printf 'pcm_id=EP1 capture (*)\npcm_name=\npcm_procfs=not-exposed\npcm_info_stream=not-exposed\npcm_info_id=not-exposed\n'
	printf 'pcm_info_name=not-exposed\npcm_info_subdevices_count=not-exposed\n'
	printf 'pcm_state=not-exposed\npcm_openers=0\npcm_opener_scan=complete\npcm_opener_procs=212\n'
	printf 'arecord_list_exit=0\narecord_list=ok\narecord_list_entries=1\narecord_list_id=EP1 capture (*)\narecord_list_name=\n'
	printf 'arecord_list_subdevices=1/1\n'
	ctl mic 'EP1 TX Mixer INTERNAL_MIC_TX' "${2:-off}"
	ctl speaker_route 'TDM_0_RX Mixer EP1' off 120
	ctl amp_switch 'TAS25XX ASI Switch' off 130
	printf 'binding=%s\nproof_dir=%s\n' "$binding" "${3:-absent}"
}
{ common preflight; printf 'probe_complete=yes\n'; } >"$work/fx/preflight"
derive() {
	src=$1
	name=$2
	shift 2
	sed "$@" "$work/fx/$src" >"$work/fx/$name"
}
derive preflight pre-mic-on -e 's/^mic\.value=off$/mic.value=on/'
derive preflight pre-speaker-on -e 's/^speaker_route\.value=off$/speaker_route.value=on/'
derive preflight pre-amp-on -e 's/^amp_switch\.value=off$/amp_switch.value=on/'
# procfs-info-present: the same PCM with its per-PCM files exposed and agreeing.
derive preflight pre-exposed -e 's/^pcm_procfs=not-exposed$/pcm_procfs=exposed/' -e 's/^pcm_info_stream=.*/pcm_info_stream=CAPTURE/' \
	-e 's/^pcm_info_id=.*/pcm_info_id=EP1 capture (*)/' -e 's/^pcm_info_name=.*/pcm_info_name=/' \
	-e 's/^pcm_info_subdevices_count=.*/pcm_info_subdevices_count=1/' -e 's/^pcm_state=.*/pcm_state=closed/'
derive pre-exposed pre-pcm-open -e 's/^pcm_state=closed$/pcm_state=open-RUNNING/' -e 's/^pcm_openers=0$/pcm_openers=1/' \
	-e 's#^arecord_list_subdevices=1/1$#arecord_list_subdevices=0/1#'
derive pre-exposed pre-info-conflict -e 's/^pcm_info_id=.*/pcm_info_id=EP1 playback (*)/'
derive pre-exposed pre-info-playback -e 's/^pcm_info_stream=.*/pcm_info_stream=PLAYBACK/'
derive preflight pre-opener -e 's/^pcm_openers=0$/pcm_openers=1/' -e 's#^arecord_list_subdevices=1/1$#arecord_list_subdevices=0/1#'
derive preflight pre-kernel-opener -e 's#^arecord_list_subdevices=1/1$#arecord_list_subdevices=0/1#'
derive preflight pre-scan-incomplete -e 's/^pcm_opener_scan=complete$/pcm_opener_scan=incomplete/'
derive preflight pre-scan-empty -e 's/^pcm_opener_procs=.*/pcm_opener_procs=0/' -e 's/^pcm_opener_scan=complete$/pcm_opener_scan=incomplete/'
derive preflight pre-ambiguous -e 's/^pcm_list=ok$/pcm_list=ambiguous/' -e 's/^pcm_list_entries=1$/pcm_list_entries=2/' \
	-e 's/^pcm_stream=.*/pcm_stream=unavailable/' -e 's/^pcm_substreams=.*/pcm_substreams=unavailable/' \
	-e 's/^pcm_id=.*/pcm_id=unavailable/' -e 's/^pcm_name=.*/pcm_name=unavailable/'
derive pre-ambiguous pre-malformed-list -e 's/^pcm_list=ambiguous$/pcm_list=malformed/' -e 's/^pcm_list_entries=2$/pcm_list_entries=1/'
derive preflight pre-other-id -e 's/^pcm_id=.*/pcm_id=EP2 capture (*)/' -e 's/^arecord_list_id=.*/arecord_list_id=EP2 capture (*)/'
derive preflight pre-arecord-conflict -e 's/^arecord_list_name=.*/arecord_list_name=audio_capture1/'
derive preflight pre-arecord-failed -e 's/^arecord_list_exit=0$/arecord_list_exit=1/' -e 's/^arecord_list=ok$/arecord_list=failed/' \
	-e 's/^arecord_list_id=.*/arecord_list_id=unavailable/' -e 's/^arecord_list_name=.*/arecord_list_name=unavailable/' \
	-e 's#^arecord_list_subdevices=.*#arecord_list_subdevices=unavailable#'
derive preflight pre-two-substreams -e 's/^pcm_substreams=1$/pcm_substreams=2/'
derive preflight pre-procfs-partial -e 's/^pcm_procfs=not-exposed$/pcm_procfs=partial/' -e 's/^pcm_state=.*/pcm_state=unavailable/'
derive preflight pre-closed-inferred -e 's/^pcm_state=not-exposed$/pcm_state=closed/'
derive preflight pre-enum -e 's/^mic\.type=BOOLEAN$/mic.type=ENUMERATED/'
derive preflight pre-kernel -e 's/^kernel_release=.*/kernel_release=6.1.162-android14-11-gdeadbeef/'
derive preflight pre-no-arecord -e 's/^tool\.arecord=present$/tool.arecord=missing/' -e 's/^arecord_impl=alsa-utils$/arecord_impl=missing/'
derive preflight pre-two-cards -e 's/^alsa_cards=1$/alsa_cards=2/'
derive preflight pre-playback -e 's/^pcm_stream=CAPTURE$/pcm_stream=PLAYBACK/'
derive pre-ambiguous pre-no-pcm -e 's/^pcm_list=ambiguous$/pcm_list=absent/' -e 's/^pcm_list_entries=2$/pcm_list_entries=0/'
derive preflight pre-leftover -e 's/^proof_dir=absent$/proof_dir=present/'
derive preflight pre-qemu -e 's/^dt_model=.*/dt_model=linux,dummy-virt/'
derive preflight pre-unknown-key -e '/^probe_complete=/i\
serial=ABC123'
derive preflight pre-bad-chars -e 's/^pcm_name=.*/pcm_name=a;rm -rf/'
derive preflight pre-other-binding -e 's/^binding=.*/binding='"$(printf '%064d' 7)"'/'
derive preflight pre-no-mic -e 's/^mic\.exit=0$/mic.exit=1/' -e 's/^mic\.name=.*/mic.name=/' -e 's/^mic\.type=BOOLEAN$/mic.type=/'

# capture_fixture STATS-FILE [extra sed...]: a clean capture session.
capture_fixture() {
	common capture
	printf 'state_changed=proof-dir\nenable_exit=0\nmic_value_enabled=on\nrecord_exit=0\nrecord_error=none\n'
	printf 'restore_exit=0\nmic_value_after=off\nmixer_restored=yes\n'
	cat "$1"
	printf 'measure_exit=0\npcm_bytes_expected=384000\nraw_removed=yes\n'
	printf 'speaker_route_value_after=off\namp_switch_value_after=off\npcm_state_after=not-exposed\n'
	printf 'pcm_openers_after=0\npcm_opener_scan_after=complete\narecord_list_subdevices_after=1/1\nprobe_complete=yes\n'
}
stats() {
	# NAME BYTES SAMPLES RMS0 RMS1 ZERO0 ZERO1 CLIP
	{
		printf 'pcm_bytes=%s\npcm_container=raw\nsamples=%s\nframes=%s\nleading_zero_frames=0\n' "$2" "$3" "$(($3 / 2))"
		printf 'ch0.samples=%s\nch0.min=-3000\nch0.max=3000\nch0.mean=0.0\nch0.ac_rms=%s\nch0.zero=%s\nch0.clipped=%s\n' "$(($3 / 2))" "$4" "$6" "$8"
		printf 'ch1.samples=%s\nch1.min=-3000\nch1.max=3000\nch1.mean=0.0\nch1.ac_rms=%s\nch1.zero=%s\nch1.clipped=0\n' "$(($3 / 2))" "$5" "$7"
		printf 'windows=20\nwindow_ac_rms_min=%s\nwindow_ac_rms_max=%s\n' "$4" "$4"
	} >"$work/fx/stats-$1"
}
stats tone 384000 192000 2121.32 2121.30 200 190 0
stats quiet 384000 192000 2.58 2.00 21000 27000 0
stats zero 384000 192000 0.00 0.00 96000 96000 0
stats dc 384000 192000 0.00 0.00 0 0 0
stats clip 384000 192000 9000.0 9000.0 0 0 5000
stats short 96000 48000 2121.32 2121.30 50 40 0
capture_fixture "$work/fx/stats-tone" >"$work/fx/cap-tone"
for name in quiet zero dc clip short; do capture_fixture "$work/fx/stats-$name" >"$work/fx/cap-$name"; done
derive cap-tone cap-unrestored -e 's/^mic_value_after=off$/mic_value_after=on/' -e 's/^mixer_restored=yes$/mixer_restored=no/'
derive cap-tone cap-raw-left -e 's/^raw_removed=yes$/raw_removed=no/'
derive cap-tone cap-speaker-after -e 's/^speaker_route_value_after=off$/speaker_route_value_after=on/'
derive cap-tone cap-opener-after -e 's/^pcm_openers_after=0$/pcm_openers_after=1/'
derive cap-tone cap-state-after -e 's/^pcm_state_after=not-exposed$/pcm_state_after=closed/'
{
	common capture
	printf 'state_changed=proof-dir\nenable_exit=0\nmic_value_enabled=on\nrecord_exit=1\nrecord_error=params-or-prepare\n'
	printf 'restore_exit=0\nmic_value_after=off\nmixer_restored=yes\npcm_bytes=0\nmeasure_exit=not-run\npcm_bytes_expected=384000\nraw_removed=yes\n'
	printf 'speaker_route_value_after=off\namp_switch_value_after=off\npcm_state_after=not-exposed\n'
	printf 'pcm_openers_after=0\npcm_opener_scan_after=complete\narecord_list_subdevices_after=1/1\nprobe_complete=yes\n'
} >"$work/fx/cap-prepare"
{ common capture; printf 'refusal=binding-changed\nstate_changed=no\nprobe_complete=yes\n'; } >"$work/fx/cap-refused"
derive cap-tone cap-unknown-key -e '/^probe_complete=/i\
ch0.sample.17=1234'
{ common restore on; printf 'restore_write=0\nmic_value_after=off\nproof_dir_removed=yes\nprobe_complete=yes\n'; } >"$work/fx/restore-ok"
{ common preflight on present; printf 'probe_complete=yes\n'; } >"$work/fx/pre-left-on"

status=0
out=''
# run CASE FIXTURE.1 [FIXTURE.2 [EXIT.2 [FIXTURE.3]]] -- ARGS...
run() {
	case_name=$1
	shift
	fx1=$1 fx2=$2 st2=$3 fx3=$4
	shift 4
	case_dir="$work/run-$case_name"
	mkdir -p "$case_dir"
	acm_guest_start
	acm_guest_ctl header_lines 2
	acm_guest_ctl fixture.1 "$work/fx/$fx1"
	acm_guest_ctl exit.1 0
	if [ -n "$fx2" ]; then
		acm_guest_ctl fixture.2 "$work/fx/$fx2"
		acm_guest_ctl exit.2 "$st2"
	fi
	[ -z "$fx3" ] || { acm_guest_ctl fixture.3 "$work/fx/$fx3"; acm_guest_ctl exit.3 0; }
	status=0
	out=$(FERRIX_OUT_DIR="$case_dir" FERRIX_PIXEL9A_MIC_CAPTURE_EVIDENCE="${EVIDENCE:-mock}" sh "$proof" "$@" 2>&1) || status=$?
	runs=$(acm_guest_runs)
	acm_guest_stop
	ctl_dir=$ACM_GUEST_CTL
	results="$case_dir/pixel-9a/mic-capture-proof"
	EVIDENCE=mock
}
has() { printf '%s\n' "$out" | grep -Fxq "$1"; }
phrase="record 2s from hw:0,8 with EP1 TX Mixer INTERNAL_MIC_TX binding 0123456789ab on $target_id"
restore_phrase="restore EP1 TX Mixer INTERNAL_MIC_TX off on $target_id"

run preflight preflight '' 0 ''
check "preflight: completes ready in one session" sh -c '[ "$1" -eq 0 ] && [ "$2" -eq 1 ]' sh "$status" "$runs"
check "preflight: the guest got the exact body after the two-line header" cmp -s "$ctl_dir/body.1" "$probe"
check "preflight: header selects preflight and binds nothing" \
	sh -c '[ "$(cat "$1")" = "$(printf "proof_mode=preflight\nproof_binding=")" ]' sh "$ctl_dir/header.1"
check "preflight: ready" has 'preflight_status=ready'
check "preflight: reports the observed PCM identity for review" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq "pcm_id=EP1 capture (*)" && printf "%s\n" "$1" | grep -Fxq pcm_name= &&
		printf "%s\n" "$1" | grep -Fxq pcm_procfs=not-exposed && printf "%s\n" "$1" | grep -Fxq pcm_state=not-exposed &&
		printf "%s\n" "$1" | grep -Fxq arecord_list_subdevices=1/1' sh "$out"
check "preflight: prints the exact bound confirmation" sh -c 'printf "%s\n" "$1" | grep -Fq "FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM='"'"'$2'"'"'"' sh "$out" "$phrase"
check "preflight: transcript keeps hashes and raw output" \
	sh -c 'grep -Eq "^probe_body_sha256=[0-9a-f]{64}$" "$1" && grep -Eq "^preflight_probe_sha256=[0-9a-f]{64}$" "$1" && grep -Fxq "mic.value=off" "$1"' sh "$results/preflight-latest.txt"
EVIDENCE=physical run preflight-physical preflight '' 0 ''
check "preflight: physical transcript names the exact target" grep -Fxq "evidence=physical:$target_id" "$results/preflight-latest.txt"
run pre-exposed pre-exposed '' 0 ''
check "preflight: procfs-info-present PCM that agrees and is closed is ready" \
	sh -c '[ "$1" -eq 0 ] && printf "%s\n" "$2" | grep -Fxq preflight_status=ready && printf "%s\n" "$2" | grep -Fxq pcm_state=closed' sh "$status" "$out"

for item in pre-mic-on:mic-not-off pre-speaker-on:speaker_route-not-off pre-amp-on:amp_switch-not-off \
	pre-pcm-open:pcm8-open-RUNNING,pcm8-has-openers,pcm8-subdevice-not-available pre-enum:mic-control-shape-unexpected \
	pre-kernel:kernel-not-pinned pre-no-arecord:arecord-missing,arecord-not-alsa-utils pre-two-cards:alsa-card-count-not-one \
	pre-playback:pcm8-not-capture pre-leftover:proof-dir-present \
	pre-no-pcm:pcm8-listing-absent,pcm8-not-capture,pcm8-capture-count-not-one,pcm8-id-not-ep1-capture,pcm8-arecord-listing-disagrees \
	pre-ambiguous:pcm8-listing-ambiguous,pcm8-not-capture,pcm8-capture-count-not-one,pcm8-id-not-ep1-capture,pcm8-arecord-listing-disagrees \
	pre-malformed-list:pcm8-listing-malformed,pcm8-not-capture,pcm8-capture-count-not-one,pcm8-id-not-ep1-capture,pcm8-arecord-listing-disagrees \
	pre-other-id:pcm8-id-not-ep1-capture pre-arecord-conflict:pcm8-arecord-listing-disagrees \
	pre-arecord-failed:pcm8-arecord-listing-disagrees,pcm8-subdevice-not-available pre-two-substreams:pcm8-capture-count-not-one \
	pre-info-conflict:pcm8-info-listing-disagree pre-info-playback:pcm8-info-listing-disagree \
	pre-procfs-partial:pcm8-procfs-partial pre-closed-inferred:pcm8-procfs-inconsistent \
	pre-opener:pcm8-has-openers,pcm8-subdevice-not-available pre-kernel-opener:pcm8-subdevice-not-available \
	pre-scan-incomplete:pcm8-opener-scan-incomplete pre-scan-empty:pcm8-opener-scan-incomplete,pcm8-opener-scan-empty \
	pre-no-mic:mic-control-absent,mic-control-shape-unexpected; do
	fx=${item%%:*}
	want=${item#*:}
	run "$fx" "$fx" '' 0 ''
	check "preflight blocked ($fx): $want" sh -c '[ "$1" -eq 0 ] && printf "%s\n" "$2" | grep -Fxq "preflight_problems=$3" &&
		printf "%s\n" "$2" | grep -Fxq preflight_status=blocked && ! printf "%s\n" "$2" | grep -Fq FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM=' sh "$status" "$out" "$want"
done
run pre-qemu pre-qemu '' 0 ''
check "identity: a QEMU device tree is refused" [ "$status" -eq 4 ]
run pre-unknown-key pre-unknown-key '' 0 ''
check "malformed: an unknown field is refused and not echoed" sh -c '[ "$1" -eq 5 ] && ! printf "%s\n" "$2" | grep -Fq ABC123' sh "$status" "$out"
run pre-bad-chars pre-bad-chars '' 0 ''
check "malformed: a value outside the reviewed character set is refused" [ "$status" -eq 5 ]

# --- capture gating ------------------------------------------------------------
mkdir -p "$work/run-gate"
for args in 'capture now' 'record' 'restore off' 'preflight capture'; do
	status=0
	# shellcheck disable=SC2086
	FERRIX_OUT_DIR="$work/run-gate" sh "$proof" $args >/dev/null 2>&1 || status=$?
	check "usage: '$args' refused" [ "$status" -eq 2 ]
done
status=0
out=$(FERRIX_OUT_DIR="$work/run-gate" sh "$proof" capture 2>&1) || status=$?
check "gate: capture without a confirmation is refused before any session" sh -c '[ "$1" -eq 2 ] && [ ! -d "$2/pixel-9a" ]' sh "$status" "$work/run-gate"
status=0
out=$(FERRIX_OUT_DIR="$work/run-gate" sh "$proof" restore 2>&1) || status=$?
check "gate: restore without its phrase is refused and names it" \
	sh -c '[ "$1" -eq 2 ] && printf "%s\n" "$2" | grep -Fq "$3" && [ ! -d "$4/pixel-9a" ]' sh "$status" "$out" "$restore_phrase" "$work/run-gate"

export FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM="$phrase"
run cap-binding-moved pre-other-binding '' 0 '' capture
check "gate: a phrase for another binding is refused after only the preflight" \
	sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 1 ] && printf "%s\n" "$3" | grep -Fq "binding 000000000000 on"' sh "$status" "$runs" "$out"
# Assignments before a function call may persist in POSIX sh; set and reset.
FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM="record 2s from hw:0,8 binding 0123456789ab on $target_id"
run cap-short-phrase preflight '' 0 '' capture
FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM=$phrase
check "gate: a phrase that omits the control is refused" sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 1 ]' sh "$status" "$runs"
for fx in pre-mic-on pre-speaker-on pre-pcm-open pre-opener pre-kernel-opener pre-ambiguous pre-info-conflict pre-kernel pre-leftover; do
	run "cap-$fx" "$fx" '' 0 '' capture
	check "refusal: blocked preflight ($fx) never gets a capture session" sh -c '[ "$1" -eq 6 ] && [ "$2" -eq 1 ]' sh "$status" "$runs"
done

run cap-tone preflight cap-tone 0 '' capture
check "capture: tone completes, exit 0" sh -c '[ "$1" -eq 0 ] && [ "$2" -eq 2 ]' sh "$status" "$runs"
check "capture: both sessions carry the exact body" sh -c 'cmp -s "$1/body.1" "$2" && cmp -s "$1/body.2" "$2"' sh "$ctl_dir" "$probe"
check "capture: the capture header carries the full preflight binding" \
	sh -c '[ "$(cat "$1")" = "$(printf "proof_mode=capture\nproof_binding=%s" "$2")" ]' sh "$ctl_dir/header.2" "$binding"
check "capture: non-silent signal classified" has 'mic_capture_result=non-silent-signal'
check "capture: level reported in dBFS" has 'max_channel_ac_rms_dbfs=-23.8'
check "capture: acoustic pickup stays unproven" has 'acoustic_pickup=unproven-without-controlled-stimulus'
check "capture: no anomalies" has 'anomalies=none'
check "capture: the plan names only the microphone route" sh -c 'printf "%s\n" "$1" | grep -Fq "write only EP1 TX Mixer INTERNAL_MIC_TX (numid 45) off -> on -> off"' sh "$out"
run cap-quiet preflight cap-quiet 0 '' capture
check "capture: noise-floor PCM is non-silent-low-level, exit 0" sh -c '[ "$1" -eq 0 ] && printf "%s\n" "$2" | grep -Fxq mic_capture_result=non-silent-low-level' sh "$status" "$out"
for item in zero:digital-silence dc:constant-level clip:saturated short:short-capture-non-silent-signal prepare:capture-failed-params-or-prepare; do
	run "cap-${item%%:*}" preflight "cap-${item%%:*}" 0 '' capture
	check "capture: ${item%%:*} is ${item#*:}, exit 8" sh -c '[ "$1" -eq 8 ] && printf "%s\n" "$2" | grep -Fxq "mic_capture_result=$3"' sh "$status" "$out" "${item#*:}"
done
run cap-unrestored preflight cap-unrestored 7 '' capture
check "cleanup: an unrestored route is exit 7" sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq anomalies=mic-route-not-restored' sh "$status" "$out"
run cap-raw-left preflight cap-raw-left 7 '' capture
check "cleanup: a raw file left behind is exit 7" sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq anomalies=raw-file-not-removed' sh "$status" "$out"
run cap-speaker-after preflight cap-speaker-after 0 '' capture
check "cleanup: a speaker route that reads on afterwards is an anomaly" sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq anomalies=speaker-route-not-off-after' sh "$status" "$out"
run cap-opener-after preflight cap-opener-after 0 '' capture
check "cleanup: an opener seen after the capture is an anomaly" sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq anomalies=pcm8-opener-after' sh "$status" "$out"
run cap-state-after preflight cap-state-after 0 '' capture
check "cleanup: a PCM state other than the preflight's is an anomaly" sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq anomalies=pcm8-state-changed-after' sh "$status" "$out"
run cap-exposed pre-exposed cap-tone 0 '' capture
check "capture: a procfs-info-present preflight expects the same state afterwards" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq anomalies=pcm8-state-changed-after' sh "$status" "$out"
run cap-refused preflight cap-refused 6 '' capture
check "refusal: guest refusal is exit 6 before any state change" \
	sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq state_changed=no && printf "%s\n" "$2" | grep -Fq "(binding-changed)"' sh "$status" "$out"
run cap-unknown-key preflight cap-unknown-key 0 '' capture
check "malformed: an unknown capture field is refused" [ "$status" -eq 5 ]
check "privacy: transcripts hold only aggregates" sh -c '! cat "$1"/run-*/pixel-9a/mic-capture-proof/*.txt | grep -qE "^ch[01]\.sample\."' sh "$work"

# A capture session whose transport fails: the workflow checks state read-only.
transport_fail() {
	# CASE VERIFY-FIXTURE
	case_dir="$work/run-$1"
	mkdir -p "$case_dir"
	acm_guest_start
	acm_guest_ctl header_lines 2
	acm_guest_ctl fixture.1 "$work/fx/preflight"
	acm_guest_ctl fixture.2 "$work/fx/cap-tone"
	acm_guest_ctl flood.2 200000
	acm_guest_ctl fixture.3 "$work/fx/$2"
	status=0
	out=$(FERRIX_OUT_DIR="$case_dir" FERRIX_PIXEL9A_MIC_CAPTURE_EVIDENCE=mock sh "$proof" capture 2>&1) || status=$?
	runs=$(acm_guest_runs)
	acm_guest_stop
}
transport_fail cap-transport-clean preflight
check "transport: a failed capture session triggers one read-only check" \
	sh -c '[ "$1" -eq 3 ] && [ "$2" -eq 3 ] && printf "%s\n" "$3" | grep -Fxq capture_transport=failed-output-limit &&
		printf "%s\n" "$3" | grep -Fq "found the route off and no proof file"' sh "$status" "$runs" "$out"
transport_fail cap-transport-left pre-left-on
check "transport: a route left on is exit 7 and names the restore phrase" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq verify_mic_value=on && printf "%s\n" "$2" | grep -Fq "$3"' sh "$status" "$out" "$restore_phrase"
unset FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM

export FERRIX_PIXEL9A_MIC_RESTORE_CONFIRM="$restore_phrase"
run restore-noop preflight '' 0 '' restore
check "restore: nothing to restore needs no write session" sh -c '[ "$1" -eq 0 ] && [ "$2" -eq 1 ] && printf "%s\n" "$3" | grep -Fxq result=nothing-to-restore' sh "$status" "$runs" "$out"
run restore-on pre-left-on restore-ok 0 '' restore
check "restore: a route left on is restored in one write session" \
	sh -c '[ "$1" -eq 0 ] && [ "$2" -eq 2 ] && printf "%s\n" "$3" | grep -Fxq result=restored' sh "$status" "$runs" "$out"
check "restore: the restore header carries no binding" \
	sh -c '[ "$(cat "$1")" = "$(printf "proof_mode=restore\nproof_binding=")" ]' sh "$ctl_dir/header.2"
unset FERRIX_PIXEL9A_MIC_RESTORE_CONFIRM

mkdir -p "$work/run-unreachable"
acm_guest_start --silent
status=0
out=$(FERRIX_OUT_DIR="$work/run-unreachable" FERRIX_PIXEL9A_ACM_TIMEOUT=5 FERRIX_PIXEL9A_MIC_CAPTURE_EVIDENCE=mock sh "$proof" 2>&1) || status=$?
silent_port=$FERRIX_SHELL
acm_guest_stop
check "unreachable: exit 3 with sanitised reason" sh -c '[ "$1" -eq 3 ] && printf "%s\n" "$2" | grep -Fq "transport failed (no-shell-response)"' sh "$status" "$out"
check "privacy: the port path is neither printed nor retained" \
	sh -c '! printf "%s\n" "$1" | grep -Fq "$2" && ! grep -rFq "$2" "$3"' sh "$out" "$silent_port" "$work/run-unreachable"

# --- layer 2: the exact guest probe in a fake guest ---------------------------------
fake="$work/fake-guest"
fbin="$fake/bin"
state="$fake/state"
mkdir -p "$fbin" "$state" "$fake/dt" "$fake/asound/card0" "$fake/proc" "$fake/run"
printf 'ZUMA PRO TEGU MP on ZUMA PRO\000' >"$fake/dt/model"
printf 'google,ZUMA PRO TEGU\000google,ZUMA PRO\000' >"$fake/dt/compatible"
printf ' 0 [googleaocsndcar]: aoc-snd-card - google,aoc-snd-card\n                      google,aoc-snd-card\n' >"$fake/asound/cards"
# The 00-08 line is the exact physical text; the others are synthetic.
pcm_list_lines='00-00: EP1 playback (*) :  : playback 1
00-08: EP1 capture (*) :  : capture 1
00-09: EP2 capture (*) :  : capture 1
00-20: audio_incall (*) :  : playback 1 : capture 1'
arecord_list_text='**** List of CAPTURE Hardware Devices ****
card 0: googleaocsndcar [google,aoc-snd-card], device 8: EP1 capture (*) []
  Subdevices: 1/1
  Subdevice #0: subdevice #0
card 0: googleaocsndcar [google,aoc-snd-card], device 9: EP2 capture (*) []
  Subdevices: 1/1
  Subdevice #0: subdevice #0'
# expose_procfs [ID]: add the per-PCM files the pinned kernel does not have.
expose_procfs() {
	mkdir -p "$fake/asound/card0/pcm8c/sub0"
	printf 'card: 0\ndevice: 8\nsubdevice: 0\nstream: CAPTURE\nid: %s\nname: \nsubname: subdevice #0\nclass: 0\nsubclass: 0\nsubdevices_count: 1\nsubdevices_avail: 1\n' \
		"${1:-EP1 capture (*)}" >"$fake/asound/card0/pcm8c/info"
	printf 'closed\n' >"$fake/asound/card0/pcm8c/sub0/status"
}
sed -e "s#^dt_root=/proc/device-tree\$#dt_root=$fake/dt#" -e "s#^asound_root=/proc/asound\$#asound_root=$fake/asound#" \
	-e "s#^proc_root=/proc\$#proc_root=$fake/proc#" -e "s#^proof_dir=/run/ferrix-mic-capture-proof\$#proof_dir=$fake/run/ferrix-mic-capture-proof#" \
	"$probe" >"$fake/probe.sh"
check "fake guest: only the four root constants differ from the probe" \
	sh -c '[ "$(diff "$1" "$2" | grep -c "^>")" -eq 4 ]' sh "$probe" "$fake/probe.sh"
cat >"$fbin/amixer" <<'EOF'
#!/bin/sh
# Fake alsa-utils amixer: state/<control> holds on|off; every call is logged.
s=$FAKE_AUDIO_STATE
printf '%s\n' "$*" >>"$s/amixer.log"
[ "${1:-}" != --version ] || { echo 'amixer version 1.2.14'; exit 0; }
[ "${1:-}" != -q ] || shift
[ "$1" = -c ] && [ "$2" = 0 ] || exit 1
verb=$3 id=$4
name=${id#iface=MIXER,name=\'}
name=${name%\'}
file="$s/ctl.$(printf '%s' "$name" | tr ' ' '_')"
[ -f "$file" ] || { echo "amixer: Cannot find the given element from control hw:0" >&2; exit 1; }
if [ "$verb" = cset ]; then
	[ ! -f "$s/cset-fails" ] || exit 1
	printf '%s\n' "$5" >"$file"
	printf '%s=%s\n' "$name" "$5" >>"$s/writes"
fi
numid=$(sed -n 2p "$file" 2>/dev/null)
printf "numid=%s,iface=MIXER,name='%s'\n  ; type=BOOLEAN,access=rw------,values=1\n  : values=%s\n" "${numid:-45}" "$name" "$(sed -n 1p "$file")"
EOF
cat >"$fbin/arecord" <<'EOF'
#!/bin/sh
# Fake alsa-utils arecord: copies state/pcm to the last argument.
s=$FAKE_AUDIO_STATE
[ "${1:-}" != --version ] || { echo 'arecord: version 1.2.14 by Jaroslav Kysela <perex@perex.cz>'; exit 0; }
[ "${1:-}" != -l ] || { cat "$s/arecord-list"; exit 0; }
printf '%s\n' "$*" >>"$s/arecord.log"
cat "$s/ctl.EP1_TX_Mixer_INTERNAL_MIC_TX" | sed -n 1p >"$s/route-during-record"
for last; do :; done
case "$(cat "$s/arecord-mode" 2>/dev/null)" in
	fail) echo 'arecord: set_params:1416: Unable to install hw params:' >&2; exit 1 ;;
	busy) echo 'arecord: main:831: audio open error: Device or resource busy' >&2; exit 1 ;;
	hang) head -c 4096 "$s/pcm" >"$last"; : >"$s/recording"; sleep 3; exit 1 ;;
esac
cp "$s/pcm" "$last"
EOF
printf '#!/bin/sh\nshift\nexec "$@"\n' >"$fbin/timeout"
# Fake ls: optionally adds a permission error to the /proc/*/fd scan.
real_ls=$(command -v ls)
printf '#!/bin/sh\n[ ! -f "$FAKE_AUDIO_STATE/ls-denied" ] || echo "ls: cannot open directory %s: Permission denied" >&2\nexec %s "$@"\n' \
	"'/proc/77/fd/'" "$real_ls" >"$fbin/ls"
printf '#!/bin/sh\necho 6.1.124-android14-11-ga4662a84a15f\n' >"$fbin/uname"
printf '#!/bin/sh\necho 0\n' >"$fbin/id"
command -v sha256sum >/dev/null 2>&1 || printf '#!/bin/sh\nexec shasum -a 256 "$@"\n' >"$fbin/sha256sum"
chmod 0755 "$fbin"/*
export FAKE_AUDIO_STATE="$state"

reset_guest() {
	rm -rf "$state"/* "$fake/run/ferrix-mic-capture-proof" "$fake/proc"/* "$fake/asound/card0/pcm8c"
	printf '%s\n' "$pcm_list_lines" >"$fake/asound/pcm"
	printf '%s\n' "$arecord_list_text" >"$state/arecord-list"
	mkdir -p "$fake/proc/1/fd" "$fake/proc/2/fd"
	ln -s /dev/null "$fake/proc/1/fd/0"
	ln -s /dev/snd/pcmC0D0p "$fake/proc/1/fd/5"
	printf 'off\n45\n' >"$state/ctl.EP1_TX_Mixer_INTERNAL_MIC_TX"
	printf 'off\n120\n' >"$state/ctl.TDM_0_RX_Mixer_EP1"
	printf 'off\n130\n' >"$state/ctl.TAS25XX_ASI_Switch"
	cp "$work/pcm/${1:-tone}" "$state/pcm"
}
gout=''
gstatus=0
guest() {
	# MODE BINDING
	gstatus=0
	gout=$({ printf 'proof_mode=%s\nproof_binding=%s\n' "$1" "$2"; cat "$fake/probe.sh"; } | PATH="$fbin:$PATH" sh 2>/dev/null) || gstatus=$?
}
gget() { printf '%s\n' "$gout" | sed -n "s/^$1=//p" | sed -n 1p; }
mic_state() { sed -n 1p "$state/ctl.EP1_TX_Mixer_INTERNAL_MIC_TX"; }
no_leftover() { [ ! -e "$fake/run/ferrix-mic-capture-proof" ]; }

reset_guest
guest preflight ''
check "guest preflight: completes on the fake tegu guest" sh -c '[ "$1" -eq 0 ] && printf "%s\n" "$2" | grep -Fxq probe_complete=yes' sh "$gstatus" "$gout"
check "guest preflight: od byte order self-test passes" [ "$(gget od_selftest)" = ok ]
check "guest preflight: parses the card driver, PCM, and controls" \
	sh -c '[ "$1" = aoc-snd-card ] && [ "$2" = CAPTURE ] && [ "$3" = "EP1 capture (*)" ] && [ "$4" = 45 ] && [ "$5" = off ] && [ "$6" = BOOLEAN ]' \
	sh "$(gget card_driver)" "$(gget pcm_stream)" "$(gget pcm_id)" "$(gget mic.numid)" "$(gget mic.value)" "$(gget mic.type)"
check "guest preflight: the observed listing parses as one capture substream with an empty name" \
	sh -c '[ "$1" = ok ] && [ "$2" = 1 ] && [ "$3" = 1 ] && [ -z "$4" ] && printf "%s\n" "$5" | grep -Fxq pcm_name=' \
	sh "$(gget pcm_list)" "$(gget pcm_list_entries)" "$(gget pcm_substreams)" "$(gget pcm_name)" "$gout"
check "guest preflight: missing pcm8c procfs is not-exposed, never closed" \
	sh -c '[ "$1" = not-exposed ] && [ "$2" = not-exposed ] && [ "$3" = not-exposed ]' sh "$(gget pcm_procfs)" "$(gget pcm_state)" "$(gget pcm_info_id)"
check "guest preflight: arecord -l names the same PCM with every substream free" \
	sh -c '[ "$1" = ok ] && [ "$2" = "EP1 capture (*)" ] && [ -z "$3" ] && [ "$4" = 1/1 ]' \
	sh "$(gget arecord_list)" "$(gget arecord_list_id)" "$(gget arecord_list_name)" "$(gget arecord_list_subdevices)"
check "guest preflight: a complete fd scan finds no opener of pcmC0D8c" \
	sh -c '[ "$1" = 0 ] && [ "$2" = complete ] && [ "$3" = 2 ]' sh "$(gget pcm_openers)" "$(gget pcm_opener_scan)" "$(gget pcm_opener_procs)"
check "guest preflight: writes nothing and records nothing" sh -c '[ ! -f "$1/writes" ] && [ ! -f "$1/arecord.log" ] && ! grep -q cset "$1/amixer.log"' sh "$state"
pre_binding=$(gget binding)
check "guest preflight: binding is a SHA-256" sh -c 'printf "%s\n" "$1" | grep -Eqx "[0-9a-f]{64}"' sh "$pre_binding"
printf '%s\n' "$gout" >"$work/fx/real-preflight"
check "guest preflight: the host validator accepts the real probe output" sh -c '
	awk "/^validate\\(\\) \\{/,/^}/" "$1" >"$3/validate.sh"; . "$3/validate.sh"; validate "$2" preflight' sh "$proof" "$work/fx/real-preflight" "$work"
# The host's own fail-closed reading of the real probe output.
awk '/^get\(\) \{/,/^}/; /^preflight_problems\(\) \{/,/^}/' "$proof" >"$work/problems.sh"
host_problems() { sh -c '. "$1"; pinned_kernel=6.1.124-android14-11-ga4662a84a15f; mic_control="EP1 TX Mixer INTERNAL_MIC_TX"
	pcm_expected_id="EP1 capture (*)"; preflight_problems "$2"' sh "$work/problems.sh" "$1"; }
check "guest preflight: the host finds no problem in the observed procfs-info-absent shape" [ -z "$(host_problems "$work/fx/real-preflight")" ]
reset_guest
expose_procfs
guest preflight ''
printf '%s\n' "$gout" >"$work/fx/real-preflight-exposed"
check "guest preflight: procfs-info-present PCM is exposed and closed, and the host accepts it" \
	sh -c '[ "$1" = exposed ] && [ "$2" = closed ] && [ "$3" = "EP1 capture (*)" ] && [ "$4" = 1 ] && [ -z "$5" ]' \
	sh "$(gget pcm_procfs)" "$(gget pcm_state)" "$(gget pcm_info_id)" "$(gget pcm_info_subdevices_count)" "$(host_problems "$work/fx/real-preflight-exposed")"
check "guest preflight: procfs exposure is part of the binding" [ "$(gget binding)" != "$pre_binding" ]
exposed_binding=$(gget binding)
guest capture "$exposed_binding"
check "guest capture: procfs-info-present path captures and leaves PCM 8 closed" \
	sh -c '[ "$1" -eq 0 ] && [ "$2" = closed ] && [ "$3" = off ]' sh "$gstatus" "$(gget pcm_state_after)" "$(mic_state)"

reset_guest tone
guest capture "$pre_binding"
check "guest capture: tone completes and cleans up" sh -c '[ "$1" -eq 0 ] && [ "$2" = off ] && [ "$3" = yes ]' sh "$gstatus" "$(mic_state)" "$(gget raw_removed)"
check "guest capture: the route was on during the recording and only it was written" \
	sh -c '[ "$(cat "$1/route-during-record")" = on ] && [ "$(cat "$1/writes")" = "$(printf "EP1 TX Mixer INTERNAL_MIC_TX=on\nEP1 TX Mixer INTERNAL_MIC_TX=off")" ]' sh "$state"
check "guest capture: arecord got the exact device and format" \
	grep -Eq -- "^-q -D hw:0,8 -t raw -f S16_LE -r 48000 -c 2 -d 2 .*/ferrix-mic-capture-proof/capture.raw$" "$state/arecord.log"
check "guest capture: no raw file is left" no_leftover
check "guest capture: all 384000 bytes are PCM samples" \
	sh -c '[ "$1" = 384000 ] && [ "$2" = 192000 ] && [ "$3" = 96000 ] && [ "$4" = 20 ]' sh "$(gget pcm_bytes)" "$(gget samples)" "$(gget frames)" "$(gget windows)"
check "guest capture: tone statistics" \
	sh -c '[ "$1" = 3000 ] && [ "$2" = -3000 ] && awk -v r="$3" "BEGIN { exit !(r > 2100 && r < 2130) }"' sh "$(gget ch0.max)" "$(gget ch0.min)" "$(gget ch0.ac_rms)"
check "guest capture: only a bounded set of aggregate lines leaves the guest" sh -c '[ "$(printf "%s\n" "$1" | wc -l)" -lt 130 ]' sh "$gout"
printf '%s\n' "$gout" >"$work/fx/real-capture"
check "guest capture: the host validator accepts the real probe output" sh -c '. "$2/validate.sh"; validate "$1" capture' sh "$work/fx/real-capture" "$work"

reset_guest quiet
guest capture "$pre_binding"
check "guest capture: noise-floor PCM measures a small non-zero AC RMS" \
	sh -c 'awk -v r="$1" "BEGIN { exit !(r > 0.5 && r < 32) }"' sh "$(gget ch0.ac_rms)"
reset_guest zero
guest capture "$pre_binding"
check "guest capture: digital zero counts every sample as zero" \
	sh -c '[ "$1" = 96000 ] && [ "$2" = 96000 ] && [ "$3" = 96000 ]' sh "$(gget ch0.zero)" "$(gget ch1.zero)" "$(gget leading_zero_frames)"
reset_guest dc
guest capture "$pre_binding"
check "guest capture: a constant offset is mean, not AC energy" sh -c '[ "$1" = -12.0 ] && [ "$2" = 0.00 ]' sh "$(gget ch0.mean)" "$(gget ch0.ac_rms)"
reset_guest wav
guest capture "$pre_binding"
check "guest capture: a RIFF container is refused, not measured" \
	sh -c '[ "$1" = riff-refused ] && [ "$2" = 1 ] && [ -z "$3" ] && [ "$4" = off ]' sh "$(gget pcm_container)" "$(gget measure_exit)" "$(gget samples)" "$(mic_state)"
reset_guest short
guest capture "$pre_binding"
check "guest capture: a short recording reports its real byte count" sh -c '[ "$1" = 96000 ] && [ "$2" = 48000 ]' sh "$(gget pcm_bytes)" "$(gget samples)"

reset_guest
printf 'fail\n' >"$state/arecord-mode"
guest capture "$pre_binding"
check "guest capture: a hw-params failure is classified and the route restored" \
	sh -c '[ "$1" -eq 0 ] && [ "$2" = params-or-prepare ] && [ "$3" = off ]' sh "$gstatus" "$(gget record_error)" "$(mic_state)"
check "guest capture: nothing is left after a failed recording" no_leftover
reset_guest
printf 'busy\n' >"$state/arecord-mode"
guest capture "$pre_binding"
check "guest capture: a busy PCM is classified" sh -c '[ "$1" = busy ] && [ "$2" = off ]' sh "$(gget record_error)" "$(mic_state)"

reset_guest
printf 'on\n45\n' >"$state/ctl.EP1_TX_Mixer_INTERNAL_MIC_TX"
guest capture "$pre_binding"
check "guest refusal: a route already on is refused and left as found" \
	sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq refusal=mic-control-not-off && printf "%s\n" "$2" | grep -Fxq state_changed=no &&
		[ ! -f "$3/writes" ] && [ ! -f "$3/arecord.log" ]' sh "$gstatus" "$gout" "$state"
reset_guest
printf 'on\n120\n' >"$state/ctl.TDM_0_RX_Mixer_EP1"
guest capture "$pre_binding"
check "guest refusal: a speaker route that reads on is refused" \
	sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq refusal=speaker-route-not-off-or-unreadable && [ ! -f "$3/writes" ]' sh "$gstatus" "$gout" "$state"
reset_guest
expose_procfs
printf 'state: RUNNING\n' >"$fake/asound/card0/pcm8c/sub0/status"
mkdir -p "$fake/proc/321/fd"
ln -s /dev/snd/pcmC0D8c "$fake/proc/321/fd/7"
guest capture "$exposed_binding"
check "guest refusal: an exposed open PCM with an opener is refused" \
	sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq refusal=pcm-not-closed,pcm-has-openers && [ ! -f "$3/writes" ]' sh "$gstatus" "$gout" "$state"
# guest_refuses NAME BINDING REFUSAL: capture is refused with exactly REFUSAL
# and nothing is written or recorded.
guest_refuses() {
	guest capture "$2"
	check "guest refusal: $1" sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq "refusal=$4" && printf "%s\n" "$2" | grep -Fxq state_changed=no &&
		[ ! -f "$3/writes" ] && [ ! -f "$3/arecord.log" ]' sh "$gstatus" "$gout" "$state" "$3"
}
reset_guest
mkdir -p "$fake/proc/321/fd"
ln -s /dev/snd/pcmC0D8c "$fake/proc/321/fd/7"
guest preflight ''
check "guest preflight: an actual opener is counted when procfs is not exposed" sh -c '[ "$1" = 1 ] && [ "$2" = not-exposed ]' sh "$(gget pcm_openers)" "$(gget pcm_state)"
guest_refuses "a not-exposed PCM with an fd opener" "$pre_binding" pcm-has-openers
reset_guest
mkdir -p "$fake/proc/400/fd"
ln -s /snd/pcmC0D8c "$fake/proc/400/fd/3"
guest_refuses "an opener seen through another mount namespace" "$pre_binding" pcm-has-openers
reset_guest
sed 's#Subdevices: 1/1#Subdevices: 0/1#' "$state/arecord-list" | sed '4,$s#Subdevices: 0/1#Subdevices: 1/1#' >"$state/arecord-list.new"
mv "$state/arecord-list.new" "$state/arecord-list"
guest_refuses "a kernel-counted opener with no visible fd" "$pre_binding" pcm-subdevice-not-available
reset_guest
: >"$state/ls-denied"
guest preflight ''
check "guest preflight: an unreadable fd table makes the opener scan incomplete" [ "$(gget pcm_opener_scan)" = incomplete ]
guest_refuses "an incomplete opener scan" "$pre_binding" pcm-opener-scan-incomplete
reset_guest
rm -rf "$fake/proc"/*
guest preflight ''
check "guest preflight: an empty /proc is an incomplete scan, not zero openers" \
	sh -c '[ "$1" = incomplete ] && [ "$2" = 0 ]' sh "$(gget pcm_opener_scan)" "$(gget pcm_opener_procs)"
reset_guest
expose_procfs 'EP1 playback (*)'
guest preflight ''
check "guest preflight: a pcm8c info file that disagrees with the listing is reported" \
	sh -c '[ "$1" = "EP1 playback (*)" ] && [ "$2" = "EP1 capture (*)" ]' sh "$(gget pcm_info_id)" "$(gget pcm_id)"
guest_refuses "a pcm8c info file that disagrees with the listing" "$(gget binding)" pcm-info-listing-disagree
reset_guest
mkdir -p "$fake/asound/card0/pcm8c"
guest preflight ''
check "guest preflight: a pcm8c directory without its files is partial" sh -c '[ "$1" = partial ] && [ "$2" = unavailable ]' sh "$(gget pcm_procfs)" "$(gget pcm_state)"
guest_refuses "a partial pcm8c directory" "$(gget binding)" pcm-procfs-partial
# list_case NAME REFUSAL LISTING: /proc/asound/pcm replaced by LISTING.
list_case() {
	reset_guest
	printf '%s\n' "$3" >"$fake/asound/pcm"
	guest preflight ''
	list_state=$(gget pcm_list)
	guest_refuses "$1" "$(gget binding)" "$2"
}
list_case "two 00-08 lines are ambiguous" pcm-listing-ambiguous,pcm-not-capture,pcm-capture-count-not-one,pcm-id-not-ep1-capture,pcm-arecord-listing-disagrees \
	"$pcm_list_lines
00-08: EP1 capture (*) :  : capture 1"
check "guest preflight: the duplicate listing is reported as ambiguous" [ "$list_state" = ambiguous ]
list_case "a playback 00-08 line" pcm-not-capture "$(printf '%s\n' "$pcm_list_lines" | sed 's/^00-08: EP1 capture (\*) :  : capture 1$/00-08: EP1 capture (*) :  : playback 1/')"
list_case "a bidirectional 00-08 line" pcm-not-capture,pcm-capture-count-not-one \
	"$(printf '%s\n' "$pcm_list_lines" | sed 's/^00-08: .*/00-08: EP1 capture (*) :  : playback 1 : capture 1/')"
list_case "a 00-08 line with two capture substreams" pcm-capture-count-not-one \
	"$(printf '%s\n' "$pcm_list_lines" | sed 's/^00-08: .*/00-08: EP1 capture (*) :  : capture 2/')"
list_case "a 00-08 line with another id" pcm-id-not-ep1-capture,pcm-arecord-listing-disagrees \
	"$(printf '%s\n' "$pcm_list_lines" | sed 's/^00-08: .*/00-08: EP2 capture (*) :  : capture 1/')"
list_case "a 00-08 line with extra fields" pcm-listing-malformed,pcm-not-capture,pcm-capture-count-not-one,pcm-id-not-ep1-capture,pcm-arecord-listing-disagrees \
	"$(printf '%s\n' "$pcm_list_lines" | sed 's/^00-08: .*/00-08: EP1 capture (*) : x : y : capture 1/')"
check "guest preflight: extra fields are reported as malformed" [ "$list_state" = malformed ]
list_case "a 00-08 id outside the character set" pcm-listing-malformed,pcm-not-capture,pcm-capture-count-not-one,pcm-id-not-ep1-capture,pcm-arecord-listing-disagrees \
	"$(printf '%s\n' "$pcm_list_lines" | sed 's/^00-08: .*/00-08: EP1 capture [*] :  : capture 1/')"
list_case "a listing line without the CC-DD prefix" pcm-listing-malformed,pcm-not-capture,pcm-capture-count-not-one,pcm-id-not-ep1-capture,pcm-arecord-listing-disagrees \
	"$pcm_list_lines
garbage"
list_case "a listing without 00-08" pcm-listing-missing-entry,pcm-not-capture,pcm-capture-count-not-one,pcm-id-not-ep1-capture,pcm-arecord-listing-disagrees \
	"$(printf '%s\n' "$pcm_list_lines" | sed '/^00-08: /d')"
reset_guest
rm -f "$fake/asound/pcm"
guest preflight ''
check "guest preflight: a missing /proc/asound/pcm is absent" [ "$(gget pcm_list)" = absent ]
reset_guest
sed 's/device 8: EP1 capture (\*) \[\]/device 8: EP1 capture (*) [audio_capture0]/' "$state/arecord-list" >"$state/arecord-list.new"
mv "$state/arecord-list.new" "$state/arecord-list"
guest_refuses "arecord -l naming another PCM name" "$pre_binding" pcm-arecord-listing-disagrees
reset_guest
{ printf '%s\n' "$arecord_list_text" | sed -n '1,4p'; printf '%s\n' "$arecord_list_text" | sed -n '2,4p'; } >"$state/arecord-list"
guest preflight ''
check "guest preflight: two arecord -l device 8 lines are ambiguous" [ "$(gget arecord_list)" = ambiguous ]
guest_refuses "an ambiguous arecord -l listing" "$pre_binding" pcm-arecord-listing-disagrees,pcm-subdevice-not-available
reset_guest
printf '%s\n' "$arecord_list_text" | sed '3s/.*/  Subdevices: lots/' >"$state/arecord-list"
guest preflight ''
check "guest preflight: a malformed Subdevices line is malformed" [ "$(gget arecord_list)" = malformed ]
reset_guest
guest capture "$(printf '%064d' 0)"
check "guest refusal: a stale binding is refused" \
	sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq refusal=binding-changed && [ ! -f "$3/writes" ]' sh "$gstatus" "$gout" "$state"
reset_guest
printf 'off\n46\n' >"$state/ctl.EP1_TX_Mixer_INTERNAL_MIC_TX"
guest capture "$pre_binding"
check "guest refusal: a different control numid changes the binding" sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq refusal=binding-changed' sh "$gstatus" "$gout"
reset_guest
mkdir -p "$fake/run/ferrix-mic-capture-proof"
guest capture "$pre_binding"
check "guest refusal: a leftover proof directory is refused" sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq refusal=proof-dir-present' sh "$gstatus" "$gout"
reset_guest
guest bogus ''
check "guest refusal: an unknown mode is exit 5" [ "$gstatus" -eq 5 ]
guest capture 'abXYZ'
check "guest refusal: a malformed binding is exit 5" [ "$gstatus" -eq 5 ]

reset_guest
printf 'hang\n' >"$state/arecord-mode"
{ printf 'proof_mode=capture\nproof_binding=%s\n' "$pre_binding"; cat "$fake/probe.sh"; } >"$work/capture-session.sh"
PATH="$fbin:$PATH" sh "$work/capture-session.sh" >"$work/term.out" 2>/dev/null &
pid=$!
tries=0
while [ ! -f "$state/recording" ] && [ "$tries" -lt 100 ]; do tries=$((tries + 1)); sleep 0.1; done
route_mid=$(mic_state)
kill -TERM "$pid" 2>/dev/null || :
term_status=0
wait "$pid" || term_status=$?
check "signal: the route was on mid-recording" [ "$route_mid" = on ]
check "signal: TERM during recording exits 143 through the trap" [ "$term_status" -eq 143 ]
check "signal: the trap restored the route and removed the raw file" \
	sh -c '[ "$1" = off ] && grep -Fxq cleanup_mixer_restored=yes "$2" && grep -Fxq cleanup_raw_removed=yes "$2"' sh "$(mic_state)" "$work/term.out"
check "signal: nothing is left in the proof directory" no_leftover
check "signal: no measurement was printed after the interrupt" sh -c '! grep -q "^samples=" "$1"' sh "$work/term.out"

reset_guest
printf 'on\n45\n' >"$state/ctl.EP1_TX_Mixer_INTERNAL_MIC_TX"
mkdir -p "$fake/run/ferrix-mic-capture-proof"
: >"$fake/run/ferrix-mic-capture-proof/capture.raw"
guest restore ''
check "guest restore: writes off to the route and removes the proof directory" \
	sh -c '[ "$1" -eq 0 ] && [ "$2" = off ] && [ "$(cat "$3/writes")" = "EP1 TX Mixer INTERNAL_MIC_TX=off" ]' sh "$gstatus" "$(mic_state)" "$state"
check "guest restore: nothing is left" no_leftover
reset_guest
guest restore ''
check "guest restore: an already-off route is not written" sh -c '[ "$1" -eq 0 ] && [ ! -f "$2/writes" ]' sh "$gstatus" "$state"
reset_guest
printf 'on\n45\n' >"$state/ctl.EP1_TX_Mixer_INTERNAL_MIC_TX"
: >"$state/cset-fails"
guest restore ''
check "guest restore: a failed write is exit 7" sh -c '[ "$1" -eq 7 ] && [ "$2" = on ]' sh "$gstatus" "$(mic_state)"

# --- layer 3: end to end through the transport -----------------------------------
alt="$work/alt"
mkdir -p "$alt/tools/pixel-9a"
cp "$proof" "$root/tools/pixel-9a/acm-probe.py" "$alt/tools/pixel-9a/"
cp "$fake/probe.sh" "$alt/tools/pixel-9a/mic-capture-proof-remote.sh"
cp "$fbin"/* "$work/acm-guest/bin/"
ACM_GUEST_PROBE="$alt/tools/pixel-9a/mic-capture-proof-remote.sh"
export ACM_GUEST_PROBE
e2e() {
	# CASE ARGS...
	case_dir="$work/run-$1"
	shift
	mkdir -p "$case_dir"
	acm_guest_start
	acm_guest_ctl header_lines 2
	acm_guest_ctl real 1
	status=0
	out=$(FERRIX_OUT_DIR="$case_dir" FERRIX_PIXEL9A_MIC_CAPTURE_EVIDENCE=mock sh "$alt/tools/pixel-9a/mic-capture-proof.sh" "$@" 2>&1) || status=$?
	runs=$(acm_guest_runs)
	acm_guest_stop
}
reset_guest tone
e2e e2e-preflight
e2e_phrase=$(printf '%s\n' "$out" | sed -n "s/^  FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM='\(.*\)' tools.*/\1/p")
check "e2e: the real probe preflight is ready and prints a phrase" \
	sh -c '[ "$1" -eq 0 ] && printf "%s\n" "$2" | grep -Fxq preflight_status=ready && [ -n "$3" ]' sh "$status" "$out" "$e2e_phrase"
reset_guest tone
export FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM="$e2e_phrase"
e2e e2e-capture capture
check "e2e: one capture measures a non-silent signal and cleans up" \
	sh -c '[ "$1" -eq 0 ] && [ "$2" -eq 2 ] && printf "%s\n" "$3" | grep -Fxq mic_capture_result=non-silent-signal &&
		printf "%s\n" "$3" | grep -Fxq anomalies=none && [ "$4" = off ]' sh "$status" "$runs" "$out" "$(mic_state)"
check "e2e: only the microphone route was written" \
	sh -c '[ "$(cat "$1/writes")" = "$(printf "EP1 TX Mixer INTERNAL_MIC_TX=on\nEP1 TX Mixer INTERNAL_MIC_TX=off")" ]' sh "$state"
check "e2e: no raw file is left" no_leftover
reset_guest zero
e2e e2e-silence capture
check "e2e: digital silence is exit 8, route restored" \
	sh -c '[ "$1" -eq 8 ] && printf "%s\n" "$2" | grep -Fxq mic_capture_result=digital-silence && [ "$3" = off ]' sh "$status" "$out" "$(mic_state)"

check "no test edits docs/status.md" [ "$(cksum <"$root/docs/status.md")" = "$status_before" ]
check "no test edits capabilities.toml" [ "$(cksum <"$root/targets/boards/$target_id/capabilities.toml")" = "$capabilities_before" ]

if [ "$fails" -ne 0 ]; then
	printf '{"event":"pixel-9a-mic-capture-proof-test","status":"failed","failures":%s}\n' "$fails"
	exit 1
fi
printf '{"event":"pixel-9a-mic-capture-proof-test","status":"passed"}\n'
