#!/bin/sh
# Offline proof for the issue #80 controlled-tone acoustic mode of the
# Pixel 9a internal-microphone proof (mic-capture-proof.sh acoustic). No
# device is contacted, nothing is played on any real output, and no real
# mixer exists. The host tone player is a fake on PATH that records what it
# was given and sleeps until stopped. Four layers:
#
#   0. static policy and the reference tone generator itself;
#   1. the host workflow over the real CDC-ACM transport and a fake guest
#      answering from synthetic acoustic fixtures;
#   2. the exact guest probe in acoustic mode against a fake procfs and
#      fake amixer/arecord, including an interrupted recording;
#   3. end to end: host, transport, fake guest running that probe, and
#      synthetic PCM built from the real generated tone (positive), noise
#      (absent), varying electrical-style noise, a continuous tone
#      (contaminated baseline), and gated broadband bursts.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
proof="$root/tools/pixel-9a/mic-capture-proof.sh"
probe="$root/tools/pixel-9a/mic-capture-proof-remote.sh"
tone_tool="$root/tools/pixel-9a/mic-acoustic-tone.py"
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

# --- layer 0: static policy -------------------------------------------------------
check "static: host and probe POSIX syntax" sh -c 'sh -n "$1" && sh -n "$2"' sh "$proof" "$probe"
check "static: generator parses" python3 -c 'import ast, sys; ast.parse(open(sys.argv[1]).read())' "$tone_tool"
const() { sed -n "s/^$2=//p" "$1" | sed -n 1p; }
pyconst() { sed -n "s/^$1 = //p" "$tone_tool" | sed -n 1p; }
check "static: host, guest, and generator agree on the stimulus frequency" \
	sh -c '[ "$1" = 1180 ] && [ "$2" = 1180 ] && [ "$3" = 1180 ]' sh "$(const "$proof" tone_hz)" "$(const "$probe" tone_hz)" "$(pyconst TONE_HZ)"
check "static: host and guest agree on control bins, window, and period" \
	sh -c '[ "$1" = "$2" ] && [ "$1" = "'"'"'1060 1280'"'"'" ] && [ "$3" = "$4" ] && [ "$3" = 2400 ] && [ "$5" = "$6" ] && [ "$5" = 8 ]' sh \
	"$(const "$proof" control_hz)" "$(const "$probe" control_hz)" "$(const "$proof" tone_window_frames)" "$(const "$probe" tone_window_frames)" \
	"$(const "$proof" tone_period_windows)" "$(const "$probe" tone_period_windows)"
check "static: the generator gate period equals the guest fold period (8 x 2400 frames at 48 kHz = 400 ms)" \
	sh -c '[ "$(($1 + $2))" -eq $(($3 * $4 * 1000 / 48000)) ] && [ "$1" -eq "$2" ]' sh "$(pyconst ON_MS)" "$(pyconst OFF_MS)" 2400 8
check "static: every analysed frequency is whole cycles per window and no 50/60 Hz mains harmonic" \
	sh -c 'for f in $(sed -n "s/^tone_hz=//p" "$1") $(sed -n "s/^control_hz=//p" "$1" | tr -d "'"'"'"); do
		[ $((f * 2400 % 48000)) -eq 0 ] && [ $((f % 50)) -ne 0 ] && [ $((f % 60)) -ne 0 ] || exit 1; done' sh "$probe"
check "static: the generator level is the reviewed -20 dBFS and the host reports the same" \
	sh -c '[ "$1" = -20.0 ] && [ "$2" = -20 ]' sh "$(pyconst LEVEL_DBFS)" "$(const "$proof" tone_level_dbfs)"
host_code=$(grep -vE '^[[:space:]]*(#|printf )' "$proof")
verbs='osascript|SwitchAudioSource|amixer|alsamixer|wpctl|pactl|pacmd|aplay|speaker-test|say|adb|fastboot|ssh|scp|defaults'
offending=$(printf '%s\n' "$host_code" | grep -nE "(^|[;|&(]|then|do|else)[[:space:]]*($verbs)([[:space:]]|$)" || true)
check "static: the host never changes a volume or device, plays on the phone, or uses Android tools" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
check "static: the player is only ever given the generated tone file under out/" \
	sh -c '[ "$(grep -c "^	\"\$player\" " "$1")" -eq 1 ] && grep -Fq "	\"\$player\" \"\$stimulus_dir/tone.wav\" </dev/null" "$1" &&
		grep -Fxq "stimulus_dir=\"\$out/stimulus-\$stamp-\$\$\"" "$1" && ! grep -Eq "\"\\\$player\" -|--volume|-v [0-9]" "$1"' sh "$proof"
check "static: the guest still writes only the microphone route (four csets, one recording)" \
	sh -c 'c=$(grep -vE "^[[:space:]]*#" "$1"); [ "$(printf "%s\n" "$c" | grep -c "amixer .*cset")" -eq 4 ] &&
		[ "$(printf "%s\n" "$c" | grep -cF "cset \"iface=MIXER,name='"'"'\$mic_control'"'"'\" ")" -eq 4 ] && [ "$(printf "%s\n" "$c" | grep -c "arecord -q")" -eq 1 ]' sh "$probe"
check "static: the tone analysis runs after the route is restored and before the file is deleted" \
	awk '/^run_t 5 amixer .*"\$original"/ { r = NR } /^\t\ttoned=\$\(tone_measure/ { t = NR } /^rm -f "\$raw"$/ { d = NR } END { exit !(r && t && d && r < t && t < d) }' "$probe"
check "static: no per-window value is printed by the guest tone analysis" \
	sh -c 'awk "/^# --- tone measure begin/,/^# --- tone measure end/" "$1" | grep "printf" | grep -Eqv "tone_windows|ch%d\\." && exit 1; exit 0' sh "$probe"
check "static: the host never edits capabilities or claims a capability" \
	sh -c '! grep -nE "capabilities\.toml\"|>.*capabilities|capability_status (supported|partial)" "$1"' sh "$proof"

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-mic-acoustic-test.XXXXXX")
. "$root/tools/pixel-9a/tests/acm-fake-guest.sh"
trap 'acm_guest_stop; [ ! -f "$work/player/pid" ] || kill "$(cat "$work/player/pid")" 2>/dev/null || :; rm -rf "$work"' EXIT
mkdir -p "$work/fx" "$work/pcm" "$work/gen" "$work/player-bin" "$work/player"
unset FERRIX_PIXEL9A_ACM_TIMEOUT FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM FERRIX_PIXEL9A_MIC_RESTORE_CONFIRM \
	FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM 2>/dev/null || true

# --- layer 0: the generated reference tone ----------------------------------------
python3 "$tone_tool" --out "$work/gen/a.wav" --seconds 10
python3 "$tone_tool" --out "$work/gen/b.wav" --seconds 10
check "generator: output is deterministic" cmp -s "$work/gen/a.wav" "$work/gen/b.wav"
check "generator: mono 16-bit 24 kHz, 10 s, -20 dBFS peak, gated 200/200 ms with silent off halves" python3 - "$work/gen/a.wav" <<'PY'
import struct, sys, wave
w = wave.open(sys.argv[1], "rb")
assert (w.getnchannels(), w.getsampwidth(), w.getframerate(), w.getnframes()) == (1, 2, 24000, 240000)
s = struct.unpack("<%dh" % w.getnframes(), w.readframes(w.getnframes()))
assert 3270 <= max(s) <= 3277 and -3277 <= min(s) <= -3270
for p in range(0, len(s), 9600):
    assert all(v == 0 for v in s[p + 4800:p + 9600])
    assert max(abs(v) for v in s[p + 240:p + 4560]) > 3000
    assert abs(s[p]) <= 1 and abs(s[p + 4799]) <= 30
PY
gstatus=0
python3 "$tone_tool" --out "$work/gen/a.wav" --seconds 10 2>/dev/null || gstatus=$?
check "generator: never overwrites an existing file" sh -c '[ "$1" -ne 0 ] && cmp -s "$2/a.wav" "$2/b.wav"' sh "$gstatus" "$work/gen"
gstatus=0
python3 "$tone_tool" --out "$work/gen/c.wav" --seconds 600 2>/dev/null || gstatus=$?
check "generator: refuses a duration outside 10-140 s" sh -c '[ "$1" -eq 2 ] && [ ! -e "$2/c.wav" ]' sh "$gstatus" "$work/gen"

# --- synthetic phone PCM: 96000 stereo S16_LE frames; channel 1 digital zero, as
# observed on the phone. "positive" is the real generated tone, sample-doubled
# to 48 kHz at a fixed unknown offset, scaled to ~60 LSB, plus 2.2 LSB noise.
python3 - "$work/pcm" "$work/gen/a.wav" <<'PY'
import math, os, random, struct, sys, wave
d, ref = sys.argv[1], sys.argv[2]
w = wave.open(ref, "rb")
tone = struct.unpack("<%dh" % w.getnframes(), w.readframes(w.getnframes()))
R, N = 48000, 96000
def gate(t, off):
    return 1.0 if (t + off) % 0.4 < 0.2 else 0.0
def write(name, f):
    r = random.Random(name)
    data = b"".join(struct.pack("<hh", max(-32768, min(32767, int(round(f(i, r))))), 0) for i in range(N))
    with open(os.path.join(d, name), "wb") as out:
        out.write(data)
start = 3 * 9600 + 1777
write("positive", lambda i, r: tone[start + i // 2] * 60 / 3277 + r.gauss(0, 2.2))
write("absent", lambda i, r: r.gauss(0, 2.2))
am = [random.Random(9).uniform(0.2, 3.0) for _ in range(100)]
write("electrical", lambda i, r: r.gauss(0, 2.2 * am[i // 1000]) + 3 * math.sin(2 * math.pi * 50 * i / R))
write("contaminated", lambda i, r: 40 * math.sin(2 * math.pi * 1180 * i / R) + r.gauss(0, 2.2))
write("broadband", lambda i, r: r.gauss(0, 2.2 + 300 * gate(i / R, 0.29)))
write("zero", lambda i, r: 0)
PY

# Fake host player: records its arguments and the file hash, then plays
# (sleeps) until stopped, or fails or stops early on request.
cat >"$work/player-bin/afplay" <<'EOF'
#!/bin/sh
s=$FAKE_PLAYER_STATE
printf '%s\n' "$*" >"$s/args"
(shasum -a 256 "$1" 2>/dev/null || sha256sum "$1") | cut -c1-64 >"$s/sha"
printf '%s\n' "$$" >"$s/pid"
case "$(cat "$s/mode" 2>/dev/null)" in
	fail) exit 1 ;;
	short) exec sleep 2 ;;
esac
exec sleep 300
EOF
chmod 0755 "$work/player-bin/afplay"
export FAKE_PLAYER_STATE="$work/player"
player_reset() { rm -f "$work/player"/*; [ -z "${1:-}" ] || printf '%s\n' "$1" >"$work/player/mode"; }
player_stopped() { [ -f "$work/player/pid" ] && ! kill -0 "$(cat "$work/player/pid")" 2>/dev/null; }

# --- layer 1: host workflow against fixtures ---------------------------------------
acm_guest_init "$root" "$work" "$probe"
binding=$(printf '%064d' 0 | sed 's/^000000000000/0123456789ab/')
ctl() {
	printf '%s.exit=0\n%s.numid=%s\n%s.iface=MIXER\n%s.name=%s\n' "$1" "$1" "${4:-45}" "$1" "$1" "$2"
	printf '%s.type=BOOLEAN\n%s.access=rw------\n%s.values=1\n%s.value=%s\n' "$1" "$1" "$1" "$1" "$3"
}
common() {
	printf 'probe=mic-capture-proof\nmode=%s\ndt_model=ZUMA PRO TEGU MP on ZUMA PRO\ndt_compatible=google,ZUMA PRO TEGU\n' "$1"
	printf 'kernel_release=6.1.124-android14-11-ga4662a84a15f\nuid=0\n'
	for t in amixer arecord od timeout awk sha256sum; do printf 'tool.%s=present\n' "$t"; done
	printf 'arecord_impl=alsa-utils\namixer_impl=alsa-utils\nod_selftest=ok\n'
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
sed 's/^speaker_route\.value=off$/speaker_route.value=on/' "$work/fx/preflight" >"$work/fx/pre-speaker-on"
{ common preflight on present; printf 'probe_complete=yes\n'; } >"$work/fx/pre-left-on"
# acoustic_fixture NAME RMS0 ZERO0 CLIP0 TONE_ON TONE_OFF CTRL_ON CTRL_OFF USABLE GATED [sed...]
acoustic_fixture() {
	name=$1
	shift
	{
		common acoustic
		printf 'state_changed=proof-dir\nenable_exit=0\nmic_value_enabled=on\nrecord_exit=0\nrecord_error=none\n'
		printf 'restore_exit=0\nmic_value_after=off\nmixer_restored=yes\n'
		printf 'pcm_bytes=384000\npcm_container=raw\nsamples=192000\nframes=96000\nleading_zero_frames=0\n'
		printf 'ch0.samples=96000\nch0.min=-90\nch0.max=90\nch0.mean=0.0\nch0.ac_rms=%s\nch0.zero=%s\nch0.clipped=%s\n' "$1" "$2" "$3"
		printf 'ch1.samples=96000\nch1.min=0\nch1.max=0\nch1.mean=0.0\nch1.ac_rms=0.00\nch1.zero=96000\nch1.clipped=0\n'
		printf 'windows=20\nwindow_ac_rms_min=%s\nwindow_ac_rms_max=%s\n' "$1" "$1"
		printf 'measure_exit=0\npcm_bytes_expected=384000\n'
		printf 'tone_hz=1180\ntone_control_hz=1060,1280\ntone_window_frames=2400\ntone_period_windows=8\ntone_windows=40\n'
		printf 'ch0.tone_phase=3\nch0.tone_on=%s\nch0.tone_off=%s\nch0.control_on=%s\nch0.control_off=%s\n' "$4" "$5" "$6" "$7"
		printf 'ch0.rms_on=%s\nch0.rms_off=2.20\nch0.tone_cycles_usable=%s\nch0.tone_cycles_gated=%s\n' "$1" "$8" "$9"
		printf 'ch1.tone_phase=0\nch1.tone_on=0.000\nch1.tone_off=0.000\nch1.control_on=0.000\nch1.control_off=0.000\n'
		printf 'ch1.rms_on=0.00\nch1.rms_off=0.00\nch1.tone_cycles_usable=5\nch1.tone_cycles_gated=0\n'
		printf 'tone_exit=0\nraw_removed=yes\n'
		printf 'speaker_route_value_after=off\namp_switch_value_after=off\npcm_state_after=not-exposed\n'
		printf 'pcm_openers_after=0\npcm_opener_scan_after=complete\narecord_list_subdevices_after=1/1\nprobe_complete=yes\n'
	} >"$work/fx/$name"
}
derive() {
	src=$1
	name=$2
	shift 2
	sed "$@" "$work/fx/$src" >"$work/fx/$name"
}
acoustic_fixture ac-positive 42.4 300 0 59.980 0.081 0.079 0.066 4 4
acoustic_fixture ac-absent 2.23 21000 0 0.090 0.075 0.090 0.096 5 0
acoustic_fixture ac-electrical 3.30 15000 0 0.168 0.119 0.138 0.104 5 1
acoustic_fixture ac-contaminated 28.4 300 0 40.014 39.982 0.087 0.073 5 0
acoustic_fixture ac-broadband 300 0 0 9.859 0.071 10.805 0.071 4 4
# A strong narrowband tone whose control bins rise and fall with it (gate clicks,
# electrical pickup): never positive.
acoustic_fixture ac-tone-and-broadband 60 300 0 59.980 0.081 2.000 0.070 4 4
acoustic_fixture ac-saturated 9000 0 4000 3000.0 0.5 2.0 1.0 4 4
acoustic_fixture ac-inconsistent 30 300 0 20.0 0.1 0.1 0.1 5 2
acoustic_fixture ac-tiny 2.23 21000 0 0.400 0.010 0.010 0.010 5 5
derive ac-absent ac-zero -e 's/^ch0\.zero=.*/ch0.zero=96000/' -e 's/^ch0\.ac_rms=.*/ch0.ac_rms=0.00/'
derive ac-positive ac-short -e 's/^pcm_bytes=384000$/pcm_bytes=96000/'
derive ac-positive ac-tone-failed -e 's/^tone_exit=0$/tone_exit=1/'
derive ac-positive ac-other-layout -e 's/^tone_hz=1180$/tone_hz=1000/'
derive ac-positive ac-unrestored -e 's/^mic_value_after=off$/mic_value_after=on/' -e 's/^mixer_restored=yes$/mixer_restored=no/'
derive ac-positive ac-raw-left -e 's/^raw_removed=yes$/raw_removed=no/'
derive ac-positive ac-unknown-key -e '/^probe_complete=/i\
ch0.window.3=zzleak'
{
	common acoustic
	printf 'state_changed=proof-dir\nenable_exit=0\nmic_value_enabled=on\nrecord_exit=1\nrecord_error=params-or-prepare\n'
	printf 'restore_exit=0\nmic_value_after=off\nmixer_restored=yes\npcm_bytes=0\nmeasure_exit=not-run\npcm_bytes_expected=384000\n'
	printf 'tone_hz=1180\ntone_control_hz=1060,1280\ntone_window_frames=2400\ntone_period_windows=8\ntone_exit=not-run\nraw_removed=yes\n'
	printf 'speaker_route_value_after=off\namp_switch_value_after=off\npcm_state_after=not-exposed\n'
	printf 'pcm_openers_after=0\npcm_opener_scan_after=complete\narecord_list_subdevices_after=1/1\nprobe_complete=yes\n'
} >"$work/fx/ac-prepare"
{ common acoustic; printf 'refusal=binding-changed\nstate_changed=no\nprobe_complete=yes\n'; } >"$work/fx/ac-refused"

status=0
out=''
# run CASE FIXTURE.1 [FIXTURE.2 EXIT.2 [SLEEP.2]] -- ARGS...
run() {
	case_name=$1
	fx1=$2 fx2=$3 st2=$4 sl2=$5
	shift 5
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
	[ -z "$sl2" ] || acm_guest_ctl sleep.2 "$sl2"
	status=0
	out=$(PATH="$work/player-bin:$PATH" FERRIX_OUT_DIR="$case_dir" FERRIX_PIXEL9A_MIC_CAPTURE_EVIDENCE=mock sh "$proof" "$@" 2>&1) || status=$?
	runs=$(acm_guest_runs)
	acm_guest_stop
	ctl_dir=$ACM_GUEST_CTL
	results="$case_dir/pixel-9a/mic-capture-proof"
}
has() { printf '%s\n' "$out" | grep -Fxq "$1"; }
no_stimulus_left() { ! find "$results" -name 'stimulus-*' | grep -q . && ! find "$results" -name '*.wav' | grep -q .; }
capture_phrase="record 2s from hw:0,8 with EP1 TX Mixer INTERNAL_MIC_TX binding 0123456789ab on $target_id"
phrase="play the gated 1180 Hz host tone and $capture_phrase"

player_reset
run preflight preflight '' 0 ''
check "preflight: ready, read-only, and prints the separate acoustic phrase" \
	sh -c '[ "$1" -eq 0 ] && [ "$2" -eq 1 ] && printf "%s\n" "$3" | grep -Fq "FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM='"'"'$4'"'"' tools/pixel-9a/mic-capture-proof.sh acoustic"' \
	sh "$status" "$runs" "$out" "$phrase"
check "preflight: the player is never started" [ ! -f "$work/player/args" ]
run pre-blocked pre-speaker-on '' 0 ''
check "preflight: a blocked preflight prints no acoustic phrase" sh -c '[ "$1" -eq 0 ] && ! printf "%s\n" "$2" | grep -Fq FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM=' sh "$status" "$out"

mkdir -p "$work/run-gate"
status=0
out=$(PATH="$work/player-bin:$PATH" FERRIX_OUT_DIR="$work/run-gate" sh "$proof" acoustic 2>&1) || status=$?
check "gate: acoustic without its confirmation is refused before any session or tone" \
	sh -c '[ "$1" -eq 2 ] && [ ! -d "$2/pixel-9a" ] && [ ! -f "$3/args" ]' sh "$status" "$work/run-gate" "$work/player"
status=0
out=$(PATH="$work/player-bin:$PATH" FERRIX_OUT_DIR="$work/run-gate" FERRIX_PIXEL9A_ACM_TIMEOUT=300 FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM="$phrase" \
	sh "$proof" acoustic 2>&1) || status=$?
check "gate: a session timeout the tone cannot outlast is refused" sh -c '[ "$1" -eq 2 ] && [ ! -d "$2/pixel-9a" ]' sh "$status" "$work/run-gate"
for args in 'acoustic now' 'tone' 'acoustic capture'; do
	status=0
	# shellcheck disable=SC2086
	FERRIX_OUT_DIR="$work/run-gate" sh "$proof" $args >/dev/null 2>&1 || status=$?
	check "usage: '$args' refused" [ "$status" -eq 2 ]
done

export FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM="$capture_phrase"
player_reset
run gate-capture-phrase preflight '' 0 '' acoustic
check "gate: the plain capture phrase does not authorise an acoustic run" \
	sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 1 ] && [ ! -f "$3/args" ]' sh "$status" "$runs" "$work/player"
unset FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM
export FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM="$phrase"
run gate-acoustic-for-capture preflight '' 0 '' capture
check "gate: the acoustic phrase does not authorise a plain capture" sh -c '[ "$1" -eq 2 ] && [ "$2" -eq 1 ]' sh "$status" "$runs"
unset FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM
export FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM="$phrase"

player_reset
run blocked pre-speaker-on '' 0 '' acoustic
check "refusal: a blocked preflight never starts the tone or a capture session" \
	sh -c '[ "$1" -eq 6 ] && [ "$2" -eq 1 ] && [ ! -f "$3/args" ]' sh "$status" "$runs" "$work/player"
player_reset fail
run player-fails preflight ac-positive 0 '' acoustic
check "refusal: a tone player that stops at once refuses before any guest change" \
	sh -c '[ "$1" -eq 6 ] && [ "$2" -eq 1 ] && printf "%s\n" "$3" | grep -Fq "did not keep playing"' sh "$status" "$runs" "$out"
check "refusal: the unplayed tone file is deleted" no_stimulus_left

player_reset
run positive preflight ac-positive 0 '' acoustic
check "positive: exit 0, two sessions" sh -c '[ "$1" -eq 0 ] && [ "$2" -eq 2 ]' sh "$status" "$runs"
check "positive: the capture header selects acoustic with the full binding" \
	sh -c '[ "$(cat "$1")" = "$(printf "proof_mode=acoustic\nproof_binding=%s" "$2")" ] && cmp -s "$3/body.2" "$4"' sh "$ctl_dir/header.2" "$binding" "$ctl_dir" "$probe"
check "positive: classified acoustic-tone-detected on channel 0 only" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq acoustic_result=acoustic-tone-detected && printf "%s\n" "$1" | grep -Fxq acoustic_class=positive &&
		printf "%s\n" "$1" | grep -Fxq ch0.acoustic=tone-detected && printf "%s\n" "$1" | grep -Fxq ch1.acoustic=digital-zero' sh "$out"
check "positive: reports the gate, narrowband, and control ratios in dB" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq ch0.tone_gate_db=57.4 && printf "%s\n" "$1" | grep -Fxq ch0.tone_over_control_db=57.6 &&
		printf "%s\n" "$1" | grep -Fxq ch0.control_gate_db=1.6' sh "$out"
check "positive: the claim stays scoped and the capability unsupported" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq acoustic_pickup=stimulus-locked-tone-under-documented-setup &&
		printf "%s\n" "$1" | grep -Fxq "capability_status=unsupported (capabilities.toml not edited)"' sh "$out"
check "positive: the player got the generated tone under ignored out/ and the transcript holds its hash" \
	sh -c 'case "$(cat "$1/args")" in "$2"/stimulus-*/tone.wav) ;; *) exit 1 ;; esac &&
		grep -Fxq "stimulus_sha256=$(cat "$1/sha")" "$3/acoustic-latest.txt" && grep -Fxq stimulus_alive_at_session_end=yes "$3/acoustic-latest.txt"' \
	sh "$work/player" "$results" "$results"
check "positive: the player was stopped and the tone file deleted" sh -c 'player_stopped() { [ -f "$1/pid" ] && ! kill -0 "$(cat "$1/pid")" 2>/dev/null; }
	player_stopped "$1"' sh "$work/player"
check "positive: no stimulus is left under out/" no_stimulus_left
check "positive: stimulus parameters are recorded" \
	sh -c 'printf "%s\n" "$1" | grep -Fxq stimulus_tone_hz=1180 && printf "%s\n" "$1" | grep -Fxq stimulus_level_dbfs=-20 &&
		printf "%s\n" "$1" | grep -Fxq stimulus_seconds=135 && printf "%s\n" "$1" | grep -Fxq stimulus_removed=yes' sh "$out"

for item in ac-absent:inconclusive-tone-not-detected ac-electrical:inconclusive-tone-not-detected \
	ac-contaminated:inconclusive-contaminated-baseline ac-broadband:inconclusive-broadband-modulation \
	ac-tone-and-broadband:inconclusive-broadband-modulation \
	ac-saturated:inconclusive-saturated ac-inconsistent:inconclusive-inconsistent-gating ac-tiny:inconclusive-tone-not-detected; do
	player_reset
	run "${item%%:*}" preflight "${item%%:*}" 0 '' acoustic
	check "inconclusive: ${item%%:*} is ${item#*:}, exit 8, tone stopped" \
		sh -c '[ "$1" -eq 8 ] && printf "%s\n" "$2" | grep -Fxq "acoustic_result=$3" && printf "%s\n" "$2" | grep -Fxq acoustic_class=inconclusive &&
			printf "%s\n" "$2" | grep -Fxq acoustic_pickup=unproven && [ -f "$4/pid" ] && ! kill -0 "$(cat "$4/pid")" 2>/dev/null' \
		sh "$status" "$out" "${item#*:}" "$work/player"
done
player_reset short
run interrupted preflight ac-positive 0 4 acoustic
check "inconclusive: a tone that stopped before the session ended never counts as positive" \
	sh -c '[ "$1" -eq 8 ] && printf "%s\n" "$2" | grep -Fxq acoustic_result=inconclusive-stimulus-interrupted &&
		printf "%s\n" "$2" | grep -Fxq stimulus_alive_at_session_end=no' sh "$status" "$out"
for item in ac-zero:capture-failed-digital-silence ac-short:capture-failed-short-capture-non-silent-signal \
	ac-tone-failed:capture-failed-tone-analysis ac-prepare:capture-failed-params-or-prepare; do
	player_reset
	run "${item%%:*}" preflight "${item%%:*}" 0 '' acoustic
	check "failure: ${item%%:*} is ${item#*:}, exit 9" \
		sh -c '[ "$1" -eq 9 ] && printf "%s\n" "$2" | grep -Fxq "acoustic_result=$3" && printf "%s\n" "$2" | grep -Fxq acoustic_class=failure' \
		sh "$status" "$out" "${item#*:}"
done
player_reset
run unrestored preflight ac-unrestored 7 '' acoustic
check "cleanup: an unrestored route is exit 7 and never positive" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq anomalies=mic-route-not-restored && printf "%s\n" "$2" | grep -Fxq acoustic_class=failure' sh "$status" "$out"
player_reset
run raw-left preflight ac-raw-left 7 '' acoustic
check "cleanup: a guest waveform left behind is exit 7" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fxq anomalies=raw-file-not-removed' sh "$status" "$out"
check "cleanup: the host tone is still stopped and deleted on a cleanup failure" sh -c '[ -f "$1/pid" ] && ! kill -0 "$(cat "$1/pid")" 2>/dev/null' sh "$work/player"
player_reset
run refused preflight ac-refused 6 '' acoustic
check "refusal: a guest refusal is exit 6, and the tone is stopped and deleted" \
	sh -c '[ "$1" -eq 6 ] && [ -f "$2/pid" ] && ! kill -0 "$(cat "$2/pid")" 2>/dev/null' sh "$status" "$work/player"
check "refusal: no stimulus is left after a guest refusal" no_stimulus_left
player_reset
run other-layout preflight ac-other-layout 0 '' acoustic
check "malformed: a probe that analysed another stimulus layout is refused" [ "$status" -eq 5 ]
player_reset
run unknown-key preflight ac-unknown-key 0 '' acoustic
check "malformed: an unknown (per-window) field is refused" sh -c '[ "$1" -eq 5 ] && ! printf "%s\n" "$2" | grep -Fq zzleak' sh "$status" "$out"

# A capture session whose transport fails: tone stopped, state checked read-only.
transport_fail() {
	case_dir="$work/run-$1"
	results="$case_dir/pixel-9a/mic-capture-proof"
	mkdir -p "$case_dir"
	player_reset
	acm_guest_start
	acm_guest_ctl header_lines 2
	acm_guest_ctl fixture.1 "$work/fx/preflight"
	acm_guest_ctl fixture.2 "$work/fx/ac-positive"
	acm_guest_ctl flood.2 200000
	acm_guest_ctl fixture.3 "$work/fx/$2"
	status=0
	out=$(PATH="$work/player-bin:$PATH" FERRIX_OUT_DIR="$case_dir" FERRIX_PIXEL9A_MIC_CAPTURE_EVIDENCE=mock sh "$proof" acoustic 2>&1) || status=$?
	runs=$(acm_guest_runs)
	acm_guest_stop
}
transport_fail transport-clean preflight
check "transport: a failed acoustic session is exit 3 after one read-only check, never positive" \
	sh -c '[ "$1" -eq 3 ] && [ "$2" -eq 3 ] && printf "%s\n" "$3" | grep -Fxq capture_transport=failed-output-limit &&
		! printf "%s\n" "$3" | grep -Fq acoustic_class=positive' sh "$status" "$runs" "$out"
check "transport: the tone was stopped and deleted before the check" \
	sh -c '[ -f "$1/pid" ] && ! kill -0 "$(cat "$1/pid")" 2>/dev/null && printf "%s\n" "$2" | grep -Fxq stimulus_removed=yes' sh "$work/player" "$out"
check "transport: no stimulus is left" no_stimulus_left
transport_fail transport-left pre-left-on
check "transport: a route left on is exit 7 and names the restore phrase" \
	sh -c '[ "$1" -eq 7 ] && printf "%s\n" "$2" | grep -Fq "FERRIX_PIXEL9A_MIC_RESTORE_CONFIRM='"'"'restore EP1 TX Mixer INTERNAL_MIC_TX off on $3'"'"'"' \
	sh "$status" "$out" "$target_id"

check "privacy: acoustic transcripts hold aggregates only" \
	sh -c '! cat "$1"/run-*/pixel-9a/mic-capture-proof/*.txt | grep -Eq "^ch[01]\.(sample|window)\.|^w[0-9]+=" &&
		! find "$1" -path "*/run-*" \( -name "*.wav" -o -name "*.raw" \) | grep -q .' sh "$work"
check "privacy: the port path is never retained" sh -c '! grep -rFq "/dev/" "$1"/run-*/pixel-9a/mic-capture-proof/' sh "$work"

# --- layer 2: the exact guest probe, acoustic mode, in a fake guest -----------------
fake="$work/fake-guest"
fbin="$fake/bin"
state="$fake/state"
mkdir -p "$fbin" "$state" "$fake/dt" "$fake/asound/card0" "$fake/proc" "$fake/run"
printf 'ZUMA PRO TEGU MP on ZUMA PRO\000' >"$fake/dt/model"
printf 'google,ZUMA PRO TEGU\000google,ZUMA PRO\000' >"$fake/dt/compatible"
printf ' 0 [googleaocsndcar]: aoc-snd-card - google,aoc-snd-card\n                      google,aoc-snd-card\n' >"$fake/asound/cards"
printf '00-00: EP1 playback (*) :  : playback 1\n00-08: EP1 capture (*) :  : capture 1\n' >"$fake/asound/pcm"
sed -e "s#^dt_root=/proc/device-tree\$#dt_root=$fake/dt#" -e "s#^asound_root=/proc/asound\$#asound_root=$fake/asound#" \
	-e "s#^proc_root=/proc\$#proc_root=$fake/proc#" -e "s#^proof_dir=/run/ferrix-mic-capture-proof\$#proof_dir=$fake/run/ferrix-mic-capture-proof#" \
	"$probe" >"$fake/probe.sh"
check "fake guest: only the four root constants differ from the probe" sh -c '[ "$(diff "$1" "$2" | grep -c "^>")" -eq 4 ]' sh "$probe" "$fake/probe.sh"
cat >"$fbin/amixer" <<'EOF'
#!/bin/sh
s=$FAKE_AUDIO_STATE
[ "${1:-}" != --version ] || { echo 'amixer version 1.2.14'; exit 0; }
[ "${1:-}" != -q ] || shift
[ "$1" = -c ] && [ "$2" = 0 ] || exit 1
verb=$3 id=$4
name=${id#iface=MIXER,name=\'}
name=${name%\'}
file="$s/ctl.$(printf '%s' "$name" | tr ' ' '_')"
[ -f "$file" ] || exit 1
if [ "$verb" = cset ]; then
	printf '%s\n' "$5" >"$file"
	printf '%s=%s\n' "$name" "$5" >>"$s/writes"
fi
printf "numid=%s,iface=MIXER,name='%s'\n  ; type=BOOLEAN,access=rw------,values=1\n  : values=%s\n" "$(sed -n 2p "$file")" "$name" "$(sed -n 1p "$file")"
EOF
cat >"$fbin/arecord" <<'EOF'
#!/bin/sh
s=$FAKE_AUDIO_STATE
[ "${1:-}" != --version ] || { echo 'arecord: version 1.2.14 by Jaroslav Kysela <perex@perex.cz>'; exit 0; }
[ "${1:-}" != -l ] || { printf '%s\n' '**** List of CAPTURE Hardware Devices ****' \
	'card 0: googleaocsndcar [google,aoc-snd-card], device 8: EP1 capture (*) []' '  Subdevices: 1/1' '  Subdevice #0: subdevice #0'; exit 0; }
printf '%s\n' "$*" >>"$s/arecord.log"
for last; do :; done
case "$(cat "$s/arecord-mode" 2>/dev/null)" in
	hang) head -c 4096 "$s/pcm" >"$last"; : >"$s/recording"; sleep 3; exit 1 ;;
esac
cp "$s/pcm" "$last"
EOF
printf '#!/bin/sh\nshift\nexec "$@"\n' >"$fbin/timeout"
printf '#!/bin/sh\necho 6.1.124-android14-11-ga4662a84a15f\n' >"$fbin/uname"
printf '#!/bin/sh\necho 0\n' >"$fbin/id"
command -v sha256sum >/dev/null 2>&1 || printf '#!/bin/sh\nexec shasum -a 256 "$@"\n' >"$fbin/sha256sum"
chmod 0755 "$fbin"/*
export FAKE_AUDIO_STATE="$state"
reset_guest() {
	rm -rf "$state"/* "$fake/run/ferrix-mic-capture-proof" "$fake/proc"/*
	mkdir -p "$fake/proc/1/fd"
	ln -s /dev/null "$fake/proc/1/fd/0"
	printf 'off\n365\n' >"$state/ctl.EP1_TX_Mixer_INTERNAL_MIC_TX"
	printf 'off\n120\n' >"$state/ctl.TDM_0_RX_Mixer_EP1"
	printf 'off\n130\n' >"$state/ctl.TAS25XX_ASI_Switch"
	cp "$work/pcm/${1:-positive}" "$state/pcm"
}
gout=''
gstatus=0
guest() {
	gstatus=0
	gout=$({ printf 'proof_mode=%s\nproof_binding=%s\n' "$1" "$2"; cat "$fake/probe.sh"; } | PATH="$fbin:$PATH" sh 2>/dev/null) || gstatus=$?
}
gget() { printf '%s\n' "$gout" | sed -n "s/^$1=//p" | sed -n 1p; }
mic_state() { sed -n 1p "$state/ctl.EP1_TX_Mixer_INTERNAL_MIC_TX"; }
no_leftover() { [ ! -e "$fake/run/ferrix-mic-capture-proof" ]; }

reset_guest
guest preflight ''
pre_binding=$(gget binding)
check "guest preflight: binds a SHA-256 and prints no tone analysis" \
	sh -c 'printf "%s\n" "$1" | grep -Eqx "[0-9a-f]{64}" && ! printf "%s\n" "$2" | grep -q "^tone_"' sh "$pre_binding" "$gout"
guest acoustic "$pre_binding"
check "guest acoustic: completes, restores the route, and deletes the recording" \
	sh -c '[ "$1" -eq 0 ] && [ "$2" = off ] && [ "$3" = yes ]' sh "$gstatus" "$(mic_state)" "$(gget raw_removed)"
check "guest acoustic: no waveform is left" no_leftover
check "guest acoustic: only the microphone route was written, on then off" \
	sh -c '[ "$(cat "$1/writes")" = "$(printf "EP1 TX Mixer INTERNAL_MIC_TX=on\nEP1 TX Mixer INTERNAL_MIC_TX=off")" ]' sh "$state"
check "guest acoustic: the same single bounded 2 s recording as capture" \
	sh -c '[ "$(grep -c . "$1/arecord.log")" -eq 1 ] && grep -Eq -- "^-q -D hw:0,8 -t raw -f S16_LE -r 48000 -c 2 -d 2 " "$1/arecord.log"' sh "$state"
check "guest acoustic: 40 windows folded; the generated tone is gated, narrowband, and cycle-consistent on channel 0" \
	sh -c '[ "$1" = 40 ] && [ "$2" = 0 ] && awk -v on="$3" -v off="$4" -v c="$5" -v u="$6" -v g="$7" \
		"BEGIN { exit !(on > 30 && off < 1 && c < 1 && u >= 4 && g >= u - 1) }"' sh \
	"$(gget tone_windows)" "$(gget tone_exit)" "$(gget ch0.tone_on)" "$(gget ch0.tone_off)" "$(gget ch0.control_on)" \
	"$(gget ch0.tone_cycles_usable)" "$(gget ch0.tone_cycles_gated)"
check "guest acoustic: a digital-zero channel folds to zero" sh -c '[ "$1" = 0.000 ] && [ "$2" = 0.000 ]' sh "$(gget ch1.tone_on)" "$(gget ch1.tone_off)"
check "guest acoustic: only a bounded set of aggregate lines leaves the guest" \
	sh -c '[ "$(printf "%s\n" "$1" | wc -l)" -lt 150 ] && ! printf "%s\n" "$1" | grep -Eq "^(ch[01]\.)?(w|window)[0-9.]"' sh "$gout"
printf '%s\n' "$gout" >"$work/fx/real-acoustic"
check "guest acoustic: the host validator accepts the real probe output" sh -c '
	awk "/^validate\\(\\) \\{/,/^}/" "$1" >"$3/validate.sh"; . "$3/validate.sh"; validate "$2" acoustic' sh "$proof" "$work/fx/real-acoustic" "$work"
reset_guest
guest capture "$pre_binding"
check "guest capture: plain capture mode still prints no tone analysis" sh -c '[ "$1" -eq 0 ] && ! printf "%s\n" "$2" | grep -q "^tone_"' sh "$gstatus" "$gout"
reset_guest
printf 'on\n120\n' >"$state/ctl.TDM_0_RX_Mixer_EP1"
guest acoustic "$pre_binding"
check "guest refusal: acoustic refuses a speaker route that reads on, writing and recording nothing" \
	sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq refusal=speaker-route-not-off-or-unreadable && [ ! -f "$3/writes" ] && [ ! -f "$3/arecord.log" ]' \
	sh "$gstatus" "$gout" "$state"
reset_guest
guest acoustic "$(printf '%064d' 0)"
check "guest refusal: acoustic refuses a stale binding" sh -c '[ "$1" -eq 6 ] && printf "%s\n" "$2" | grep -Fxq refusal=binding-changed' sh "$gstatus" "$gout"

reset_guest
printf 'hang\n' >"$state/arecord-mode"
{ printf 'proof_mode=acoustic\nproof_binding=%s\n' "$pre_binding"; cat "$fake/probe.sh"; } >"$work/acoustic-session.sh"
PATH="$fbin:$PATH" sh "$work/acoustic-session.sh" >"$work/term.out" 2>/dev/null &
pid=$!
tries=0
while [ ! -f "$state/recording" ] && [ "$tries" -lt 100 ]; do tries=$((tries + 1)); sleep 0.1; done
route_mid=$(mic_state)
kill -TERM "$pid" 2>/dev/null || :
term_status=0
wait "$pid" || term_status=$?
check "signal: TERM during an acoustic recording restores the route and deletes the file" \
	sh -c '[ "$1" = on ] && [ "$2" -eq 143 ] && [ "$3" = off ] && grep -Fxq cleanup_raw_removed=yes "$4" && ! grep -q "^tone_" "$4"' \
	sh "$route_mid" "$term_status" "$(mic_state)" "$work/term.out"
check "signal: nothing is left" no_leftover

# --- layer 3: end to end through the transport -----------------------------------
alt="$work/alt"
mkdir -p "$alt/tools/pixel-9a"
cp "$proof" "$tone_tool" "$root/tools/pixel-9a/acm-probe.py" "$alt/tools/pixel-9a/"
cp "$fake/probe.sh" "$alt/tools/pixel-9a/mic-capture-proof-remote.sh"
cp "$fbin"/* "$work/acm-guest/bin/"
ACM_GUEST_PROBE="$alt/tools/pixel-9a/mic-capture-proof-remote.sh"
export ACM_GUEST_PROBE
e2e() {
	case_dir="$work/run-$1"
	results="$case_dir/pixel-9a/mic-capture-proof"
	shift
	mkdir -p "$case_dir"
	acm_guest_start
	acm_guest_ctl header_lines 2
	acm_guest_ctl real 1
	status=0
	out=$(PATH="$work/player-bin:$PATH" FERRIX_OUT_DIR="$case_dir" FERRIX_PIXEL9A_MIC_CAPTURE_EVIDENCE=mock \
		sh "$alt/tools/pixel-9a/mic-capture-proof.sh" "$@" 2>&1) || status=$?
	runs=$(acm_guest_runs)
	acm_guest_stop
}
reset_guest
e2e e2e-preflight
e2e_phrase=$(printf '%s\n' "$out" | sed -n "s/^  FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM='\(.*\)' tools.*/\1/p")
check "e2e: the real probe preflight prints an acoustic phrase" sh -c '[ "$1" -eq 0 ] && [ -n "$2" ]' sh "$status" "$e2e_phrase"
export FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM="$e2e_phrase"
for item in positive:0:acoustic-tone-detected absent:8:inconclusive-tone-not-detected electrical:8:inconclusive-tone-not-detected \
	contaminated:8:inconclusive-contaminated-baseline broadband:8:inconclusive-broadband-modulation zero:9:capture-failed-digital-silence; do
	pcm=${item%%:*}
	rest=${item#*:}
	reset_guest "$pcm"
	player_reset
	e2e "e2e-$pcm" acoustic
	check "e2e: $pcm PCM is ${rest#*:} (exit ${rest%%:*}), route off, nothing left on guest or host" \
		sh -c '[ "$1" -eq "$2" ] && printf "%s\n" "$3" | grep -Fxq "acoustic_result=$4" && printf "%s\n" "$3" | grep -Fxq anomalies=none &&
			[ "$5" = off ] && [ "$6" -eq 2 ] && [ -f "$7/pid" ] && ! kill -0 "$(cat "$7/pid")" 2>/dev/null' \
		sh "$status" "${rest%%:*}" "$out" "${rest#*:}" "$(mic_state)" "$runs" "$work/player"
	check "e2e: $pcm leaves no waveform" sh -c '[ ! -e "$1" ] && ! find "$2" \( -name "*.wav" -o -name "*.raw" -o -name "stimulus-*" \) | grep -q .' \
		sh "$fake/run/ferrix-mic-capture-proof" "$results"
done
unset FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM

check "no test edits docs/status.md or docs/status-log/" [ "$(status_sum)" = "$status_before" ]
check "no test edits capabilities.toml" [ "$(cksum <"$root/targets/boards/$target_id/capabilities.toml")" = "$capabilities_before" ]

if [ "$fails" -ne 0 ]; then
	printf '{"event":"pixel-9a-mic-acoustic-proof-test","status":"failed","failures":%s}\n' "$fails"
	exit 1
fi
printf '{"event":"pixel-9a-mic-acoustic-proof-test","status":"passed"}\n'
