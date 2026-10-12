#!/bin/sh
# Offline (mock) proof for the Pixel 9a read-only audio inventory. The real
# CDC-ACM transport talks to a fake guest (acm-fake-guest.sh): a real
# interactive shell on a pty whose final `sh` is a stub that checks it got the
# exact checked-in probe bytes and answers from synthetic fixtures, so this
# test never contacts a device. It checks the static read-only allow-list,
# the exact-target identity gates (QEMU, mismatch, forged, truncated,
# unreachable), transport timeout and output bounds, explicit
# unavailable/unknown outcomes, unsupported-only statuses, mock/physical
# labelling, and redaction of the transcript and TSV.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
audio="$root/tools/pixel-9a/audio-inventory.sh"
probe="$root/tools/pixel-9a/audio-inventory-remote.sh"
fixtures="$root/tools/pixel-9a/tests/fixtures/audio"
target_id=google-pixel-9a-tegu-gxq96-mp1-0
fails=0
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

[ -x "$audio" ] || { echo 'audio inventory workflow is not executable' >&2; exit 1; }
check "static: host workflow POSIX syntax" sh -n "$audio"
check "static: guest probe POSIX syntax" sh -n "$probe"

# Exactly one executable transport line, inside remote(), sending the probe.
transport_lines=$(grep -v '^[[:space:]]*#' "$audio" | grep -E 'python3 "\$transport"' || true)
transport_count=$(printf '%s\n' "$transport_lines" | awk 'NF { count++ } END { print count + 0 }')
check "static: exactly one CDC-ACM transport invocation" [ "$transport_count" -eq 1 ]
check "static: transport sends the checked-in probe with bounds and merged stderr" \
  sh -c 'printf "%s\n" "$1" | grep -qE "^[[:space:]]+python3 \"\\\$transport\" --probe \"\\\$remote_probe\" --timeout \"\\\$acm_timeout\" --max-bytes \"\\\$max_output\" --stderr merge "' sh "$transport_lines"
check "static: transport line lives inside remote()" \
  sh -c 'awk "/^remote\\(\\) \\{/{inside=1} inside && /^}/{inside=0} inside && /^[[:space:]]+python3 /{found=1} END{exit !found}" "$1"' sh "$audio"
check "static: no SSH or other network transport" \
  sh -c '! grep -v "^[[:space:]]*#" "$1" | grep -nwE "ssh|scp|nc|telnet|curl"' sh "$audio"
check "static: redaction reuses the radios.sh redactor" grep -q '"$radios" redact$' "$audio"
for file in "$audio" "$probe"; do
  offending=$(grep -nwE 'adb|fastboot|flash|erase|format|reboot|sideload|unlock|dd|mkfs|modprobe|insmod|rmmod|rc-update|systemctl' "$file" | grep -vE '^[0-9]+:[[:space:]]*#' || true)
  check "static: no Android, storage, boot, or module verbs in ${file##*/}" [ -z "$offending" ]
  [ -z "$offending" ] || printf '%s\n' "$offending"
done

# The guest probe may name playback/mixer tools only in the availability list
# and must never run them or change audio, service, or sysfs state.
probe_code=$(grep -vE '^[[:space:]]*#' "$probe" | grep -v '^audio_tools=')
offending=$(printf '%s\n' "$probe_code" | grep -nwE 'aplay|arecord|speaker-test|amixer|alsactl|mount|umount|tee|kill|pkill' || true)
check "static: probe never runs playback, capture, mixer, mount, or kill tools" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
offending=$(printf '%s\n' "$probe_code" | grep -nE 'set-default|set-volume|set-mute|set-profile|set-sink|load-module|unload-module|pactl (set|load|unload|move|suspend)|wpctl (set|clear)|alsaucm .*(set|reset|reload)|rc-service [^ ]+ (start|stop|restart)' || true)
check "static: probe makes no mixer, route, profile, or service change" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
offending=$(printf '%s\n' "$probe_code" | grep -vE '^[[:space:]]*(audio_tools=)' |
  sed -e 's/2>&1//g' -e 's/2>\/dev\/null//g' -e 's/>\/dev\/null//g' | grep -n '>' || true)
check "static: probe writes nothing except to /dev/null" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
check "static: rc-service is only used for status" \
  sh -c '! grep -E "rc-service" "$1" | grep -vE "have rc-service|rc-service \"\\\$service\" status"' sh "$probe"
check "static: AoC firmware control is only tested for existence" \
  sh -c '! grep -v "^[[:space:]]*#" "$1" | grep -F "aoc_device/firmware" | grep -vF "[ -e \"\$aoc_device/firmware\" ]"' sh "$probe"
check "static: AoC revision text is classified, never printed" \
  sh -c '! grep -v "^[[:space:]]*#" "$1" | grep -E "(echo|printf).*aoc_state"' sh "$probe"
check "static: identity gate precedes the first audio probe" \
  sh -c 'gate=$(grep -n "exit 4" "$1" | head -n 1 | cut -d: -f1); first=$(grep -nE "asound|wpctl status" "$1" | grep -v "^[0-9]*:#" | head -n 1 | cut -d: -f1); [ -n "$gate" ] && [ -n "$first" ] && [ "$gate" -lt "$first" ]' sh "$probe"

# Running the probe on this (non-tegu) host must refuse before any audio
# probe; only the identity verdict is printed.
local_status=0
local_out=$(sh "$probe" 2>&1) || local_status=$?
check "probe: refuses a non-tegu host with exit 4 or 5" sh -c '[ "$1" -eq 4 ] || [ "$1" -eq 5 ]' sh "$local_status"
check "probe: refusal prints no audio fields" \
  sh -c '! printf "%s\n" "$1" | grep -Eq "^(alsa_|tool_|pipewire|probe=)"' sh "$local_out"

command -v python3 >/dev/null 2>&1 || { echo 'python3 is required for the CDC-ACM transport test' >&2; exit 1; }
work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-audio-test.XXXXXX")
. "$root/tools/pixel-9a/tests/acm-fake-guest.sh"
trap 'acm_guest_stop; rm -rf "$work"' EXIT
acm_guest_init "$root" "$work" "$probe"
STUB_FIXTURES=$fixtures
unset FERRIX_PIXEL9A_ACM_TIMEOUT 2>/dev/null || true
export FERRIX_PIXEL9A_AUDIO_EVIDENCE=mock

# One fresh fake guest per case. STUB_GUEST/STUB_EXIT pick the fixture and
# probe status; STUB_UNREACHABLE=1 gives a port with no shell, STUB_SLEEP a
# wedged probe and STUB_FLOOD an oversized one. `runs` counts probe runs.
run_audio() {
  case_name=$1
  shift
  mkdir -p "$work/$case_name"
  export FERRIX_OUT_DIR="$work/$case_name"
  if [ "${STUB_UNREACHABLE:-0}" = 1 ]; then
    acm_guest_start --silent
  else
    acm_guest_start
    acm_guest_ctl fixture "$STUB_FIXTURES/$STUB_GUEST.txt"
    acm_guest_ctl exit "${STUB_EXIT:-0}"
    [ -z "${STUB_SLEEP:-}" ] || acm_guest_ctl sleep "$STUB_SLEEP"
    [ -z "${STUB_FLOOD:-}" ] || acm_guest_ctl flood "$STUB_FLOOD"
  fi
  case_port=$FERRIX_SHELL
  status=0
  "$audio" "$@" > "$work/$case_name/stdout" 2> "$work/$case_name/stderr" || status=$?
  runs=$(acm_guest_runs)
  acm_guest_stop
}

tsv_row() {
  awk -F '\t' -v subsystem="$2" -v progress="$3" \
    '$1 == subsystem && $2 == "unsupported" && $3 == progress { found = 1 } END { exit !found }' "$1"
}

# --- full synthetic guest ---
export STUB_GUEST=guest-tegu STUB_EXIT=0
run_audio full inventory
tsv="$work/full/pixel-9a/audio/audio-evidence-latest.tsv"
transcript=$(find "$work/full/pixel-9a/audio" -name 'inventory-*.txt' -type f | head -n 1)
check "inventory: succeeds" [ "$status" -eq 0 ]
check "inventory: the probe ran exactly once over CDC-ACM" [ "$runs" -eq 1 ]
check "inventory: the guest received the exact checked-in probe bytes" cmp -s "$ACM_GUEST_CTL/stdin" "$probe"
check "inventory: transcript names the transport, not the port" \
  sh -c 'grep -q "^# transport cdc-acm, guest port redacted; probe sha256 [0-9a-f]\{64\}$" "$1" && ! grep -rFq "$2" "$3"' sh "$transcript" "$case_port" "$work/full"
check "privacy: guest prompt and bootstrap echo are not retained" \
  sh -c '! grep -rEq "fake-guest-host|FERRIXACM|base64 -d" "$1"' sh "$work/full"
check "inventory: identity verified" grep -q '^guest_identity=verified$' "$work/full/stdout"
check "inventory: writes the TSV and transcript" sh -c '[ -f "$1" ] && [ -n "$2" ] && [ -f "$2" ]' sh "$tsv" "$transcript"
check "inventory: header has subsystem/status/progress/evidence" grep -q "^subsystem	status	progress	evidence$" "$tsv"
check "inventory: ALSA playback and capture PCMs enumerated" tsv_row "$tsv" alsa alsa-playback-and-capture-pcms-enumerated
check "inventory: ALSA evidence records counts" grep -q 'cards=1 pcms=3 playback=2 capture=1' "$tsv"
check "inventory: kernel audio surface present without logged failures" tsv_row "$tsv" kernel-audio audio-kernel-surface-present-no-probe-failure-logged
check "inventory: firmware files present" tsv_row "$tsv" audio-firmware audio-firmware-files-present-no-load-failure-logged
check "inventory: AoC online is classified" tsv_row "$tsv" aoc-dsp aoc-reports-online
check "inventory: AoC evidence records control, image, manifest row, and loader" \
  grep -q 'device=present revision=online firmware_control=present image=regular manifest_rows=1 loader=started; revision text is classified, not recorded; no sysfs write' "$tsv"
check "inventory: UCM card profile present" tsv_row "$tsv" ucm-and-tools ucm-card-profile-present
check "inventory: tool availability is recorded" grep -q 'aplay=present .* alsactl=unavailable .* speaker_test=unavailable' "$tsv"
check "inventory: PipeWire sinks and sources present" tsv_row "$tsv" pipewire-graph graph-sinks-and-sources-present
check "inventory: graph counts and adapter list exit recorded" grep -q 'sinks=2 sources=1 default_sink=marked default_source=marked adapter_list_sinks_exit=1' "$tsv"
check "inventory: Ferrix core audio state observed" tsv_row "$tsv" ferrix-audio-adapter core-audio-state-observed
check "inventory: adapter evidence records backend, services, and route" \
  grep -q 'backend=wireplumber ferrix-pipewire=started ferrix-audio=started ferrix-core=started core_socket=present core_state=observed audio_capability=unsupported audio_route=speaker' "$tsv"
check "inventory: summary row stays inventory-only" tsv_row "$tsv" audio inventory-only-playback-capture-not-exercised
check "inventory: status column is unsupported only" \
  awk -F '\t' '!/^#/ && $1 != "subsystem" && $2 != "unsupported" { bad = 1 } END { exit bad }' "$tsv"
check "inventory: every row carries the mock label" \
  sh -c 'test "$(grep -vc "^#" "$1")" -eq 9 && test "$(grep -c "	mock: stubbed transport for $2 (not physical evidence); " "$1")" -eq 8' sh "$tsv" "$target_id"
check "inventory: mock run never claims physical evidence" sh -c '! grep -q "physical: $2" "$1" "$3"' sh "$tsv" "$target_id" "$transcript"
check "inventory: transcript labels mock" grep -q "^# evidence: mock: stubbed transport for $target_id" "$transcript"
check "inventory: transcript keeps raw sections" \
  sh -c 'grep -q "^## begin proc-asound-pcm$" "$1" && grep -q "^## begin wpctl-status$" "$1" && grep -q "^## begin ferrix-core-state$" "$1"' sh "$transcript"
check "privacy: underscore Bluetooth MAC is hashed" \
  sh -c '! grep -q "AA_BB_CC_DD_EE_01" "$1" && grep -q "bluez_output.mac:[0-9a-f]\{12\}" "$1"' sh "$transcript"
check "privacy: serials are removed" sh -c '! grep -Eq "SYNSERIAL42|SYNAUD0001" "$1"' sh "$transcript"
check "privacy: cookies are removed" sh -c '! grep -Eq "1234567890|6f6f:aa11" "$1"' sh "$transcript"
check "privacy: host names are removed" sh -c '! grep -q "ferrix-synth-host" "$1"' sh "$transcript"
check "inventory: capabilities.toml, status.md and status-log are untouched" \
  sh -c '[ "$1" = "$2" ] && [ "$3" = "$(cksum <"$4")" ]' sh \
    "$status_before" "$(status_sum)" "$capabilities_before" \
    "$root/targets/boards/$target_id/capabilities.toml"

# --- every interface absent: explicit unavailable/unknown, never success ---
export STUB_GUEST=guest-minimal
run_audio minimal inventory
tsv="$work/minimal/pixel-9a/audio/audio-evidence-latest.tsv"
check "minimal: inventory completes" [ "$status" -eq 0 ]
check "minimal: ALSA proc interface unavailable" tsv_row "$tsv" alsa alsa-proc-interface-unavailable
check "minimal: unreadable modules leave the kernel surface unknown" tsv_row "$tsv" kernel-audio kernel-audio-surface-unknown
check "minimal: firmware log unavailable" tsv_row "$tsv" audio-firmware audio-firmware-log-unavailable
check "minimal: UCM data unavailable" tsv_row "$tsv" ucm-and-tools ucm-data-unavailable
check "minimal: wpctl unavailable" tsv_row "$tsv" pipewire-graph wpctl-unavailable
check "minimal: core socket absent" tsv_row "$tsv" ferrix-audio-adapter ferrix-core-socket-absent
check "minimal: AoC device absent" tsv_row "$tsv" aoc-dsp aoc-device-absent
check "minimal: unknown AoC manifest and loader stay unknown" grep -q 'manifest_rows=unknown loader=unknown' "$tsv"
check "minimal: unknown counts stay unknown" grep -q 'cards=unknown pcms=unknown' "$tsv"
check "minimal: service manager absence is unknown" grep -q 'ferrix-audio=unknown' "$tsv"

# --- derived variants ---
variant() {
  name=$1
  shift
  sed "$@" "$fixtures/guest-tegu.txt" > "$work/fixtures/$name.txt"
}
mkdir -p "$work/fixtures"
variant failures -e 's/^audio_dt_unbound=0$/audio_dt_unbound=1/' -e 's/^dmesg_audio_failure_lines=0$/dmesg_audio_failure_lines=3/' \
  -e 's/^dmesg_audio_firmware_failures=0$/dmesg_audio_firmware_failures=1/' -e 's/^deferred_audio_devices=0$/deferred_audio_devices=1/'
variant graph_down -e 's/^pipewire_graph=reachable$/pipewire_graph=unreachable/' -e '/^graph_/d'
variant no_socat -e 's/^core_state=observed$/core_state=socket-client-unavailable/' -e '/^core_audio_/d'
variant no_route -e 's/^core_audio_route=speaker$/core_audio_route=absent/'
# The shape of the 2026-10-11 exact-device finding, with synthetic values:
# no card, AoC Offline, firmware control present, nothing staged or loaded.
variant aoc_offline -e 's/^alsa_cards=1$/alsa_cards=0/' -e 's/^aoc_revision=online$/aoc_revision=offline/' \
  -e 's/^aoc_firmware_image=regular$/aoc_firmware_image=absent/' -e 's/^aoc_firmware_manifest_rows=1$/aoc_firmware_manifest_rows=0/' \
  -e 's/^service_ferrix_aoc_firmware=started$/service_ferrix_aoc_firmware=not-started/'
variant aoc_no_control -e 's/^aoc_revision=online$/aoc_revision=offline/' -e 's/^aoc_firmware_control=present$/aoc_firmware_control=absent/'
variant aoc_unreadable -e 's/^aoc_revision=online$/aoc_revision=unreadable/'
variant aoc_missing -e '/^aoc_/d'
export STUB_FIXTURES="$work/fixtures"

export STUB_GUEST=failures
run_audio failures inventory
tsv="$work/failures/pixel-9a/audio/audio-evidence-latest.tsv"
check "failures: probe failures are reported" tsv_row "$tsv" kernel-audio audio-probe-failures-or-unbound-nodes-observed
check "failures: firmware load failures are reported" tsv_row "$tsv" audio-firmware audio-firmware-load-failures-logged

export STUB_GUEST=graph_down
run_audio graph_down inventory
tsv="$work/graph_down/pixel-9a/audio/audio-evidence-latest.tsv"
check "graph: unreachable WirePlumber is reported" tsv_row "$tsv" pipewire-graph pipewire-graph-unreachable
check "graph: missing counts are unknown, not zero" grep -q 'sinks=unknown sources=unknown' "$tsv"

export STUB_GUEST=no_socat
run_audio no_socat inventory
check "adapter: missing socket client is reported" \
  tsv_row "$work/no_socat/pixel-9a/audio/audio-evidence-latest.tsv" ferrix-audio-adapter core-socket-client-unavailable

export STUB_GUEST=no_route
run_audio no_route inventory
check "adapter: core state without audio is not promoted" \
  tsv_row "$work/no_route/pixel-9a/audio/audio-evidence-latest.tsv" ferrix-audio-adapter core-state-without-audio

export STUB_GUEST=aoc_offline
run_audio aoc_offline inventory
tsv="$work/aoc_offline/pixel-9a/audio/audio-evidence-latest.tsv"
check "aoc: offline DSP with a firmware control is classified" tsv_row "$tsv" aoc-dsp aoc-offline-firmware-control-present
check "aoc: missing image, manifest row, and loader are recorded" \
  grep -q 'revision=offline firmware_control=present image=absent manifest_rows=0 loader=not-started' "$tsv"
check "aoc: offline DSP leaves ALSA without cards" tsv_row "$tsv" alsa no-alsa-cards
check "aoc: summary row stays inventory-only" tsv_row "$tsv" audio inventory-only-playback-capture-not-exercised

export STUB_GUEST=aoc_no_control
run_audio aoc_no_control inventory
check "aoc: offline DSP without a firmware control is distinct" \
  tsv_row "$work/aoc_no_control/pixel-9a/audio/audio-evidence-latest.tsv" aoc-dsp aoc-offline-firmware-control-absent

export STUB_GUEST=aoc_unreadable
run_audio aoc_unreadable inventory
check "aoc: unreadable revision is not promoted" \
  tsv_row "$work/aoc_unreadable/pixel-9a/audio/audio-evidence-latest.tsv" aoc-dsp aoc-revision-unavailable

export STUB_GUEST=aoc_missing
run_audio aoc_missing inventory
tsv="$work/aoc_missing/pixel-9a/audio/audio-evidence-latest.tsv"
check "aoc: missing fields are unknown, not absent" tsv_row "$tsv" aoc-dsp aoc-state-unknown
check "aoc: missing manifest count is unknown, not zero" grep -q 'manifest_rows=unknown' "$tsv"
export STUB_FIXTURES="$fixtures"

# --- identity refusals ---
export STUB_GUEST=guest-qemu STUB_EXIT=5
run_audio qemu inventory
check "qemu: refused with exit 5" [ "$status" -eq 5 ]
check "qemu: refusal is machine-readable" grep -q '"event":"pixel-9a-audio-inventory".*"status":"failed".*"exit":5' "$work/qemu/stderr"
check "qemu: no evidence TSV written" [ ! -f "$work/qemu/pixel-9a/audio/audio-evidence-latest.tsv" ]

export STUB_GUEST=guest-mismatch STUB_EXIT=4
run_audio mismatch inventory
check "mismatch: near-match model refused with exit 4" [ "$status" -eq 4 ]
check "mismatch: refusal names the exact pinned model" grep -Fq 'ZUMA PRO TEGU MP on ZUMA PRO' "$work/mismatch/stderr"
check "mismatch: no evidence TSV written" [ ! -f "$work/mismatch/pixel-9a/audio/audio-evidence-latest.tsv" ]

export STUB_GUEST=guest-forged STUB_EXIT=0
run_audio forged inventory
check "forged: host re-verification refuses a near-match model" [ "$status" -eq 4 ]
check "forged: no evidence TSV written" [ ! -f "$work/forged/pixel-9a/audio/audio-evidence-latest.tsv" ]

export STUB_GUEST=guest-truncated STUB_EXIT=0
run_audio truncated inventory
check "truncated: incomplete probe output fails" [ "$status" -eq 1 ]
check "truncated: no evidence TSV written" [ ! -f "$work/truncated/pixel-9a/audio/audio-evidence-latest.tsv" ]

export STUB_GUEST=guest-tegu STUB_UNREACHABLE=1 FERRIX_PIXEL9A_ACM_TIMEOUT=3
run_audio unreachable inventory
check "unreachable: CDC-ACM transport failure exits 3" [ "$status" -eq 3 ]
check "unreachable: sanitised reason is reported" grep -Fq 'transport failed (no-shell-response)' "$work/unreachable/stderr"
check "unreachable: no evidence TSV written" [ ! -f "$work/unreachable/pixel-9a/audio/audio-evidence-latest.tsv" ]
check "privacy: port path is neither printed nor retained" \
  sh -c '! grep -rFq "$1" "$2"' sh "$case_port" "$work/unreachable"
unset STUB_UNREACHABLE

export STUB_SLEEP=30
run_audio timeout inventory
check "timeout: wedged probe exits 3" [ "$status" -eq 3 ]
check "timeout: reason is timeout" grep -Fq 'transport failed (timeout)' "$work/timeout/stderr"
check "timeout: no evidence TSV written" [ ! -f "$work/timeout/pixel-9a/audio/audio-evidence-latest.tsv" ]
unset STUB_SLEEP FERRIX_PIXEL9A_ACM_TIMEOUT

export STUB_FLOOD=1100000
run_audio overrun inventory
check "truncation: output over 1 MiB exits 3" [ "$status" -eq 3 ]
check "truncation: reason is output-limit" grep -Fq 'transport failed (output-limit)' "$work/overrun/stderr"
check "truncation: no evidence TSV written" [ ! -f "$work/overrun/pixel-9a/audio/audio-evidence-latest.tsv" ]
unset STUB_FLOOD

# --- labels and usage ---
unset FERRIX_PIXEL9A_AUDIO_EVIDENCE
run_audio physical_label inventory
check "label: default run labels the exact physical target" \
  sh -c 'test "$(grep -c "	physical: $2; " "$1")" -eq 8' sh "$work/physical_label/pixel-9a/audio/audio-evidence-latest.tsv" "$target_id"
export FERRIX_PIXEL9A_AUDIO_EVIDENCE=qemu
run_audio bad_label inventory
check "label: only physical or mock labels are accepted" sh -c 'test "$1" -eq 2 && test "$2" -eq 0' sh "$status" "$runs"
export FERRIX_PIXEL9A_AUDIO_EVIDENCE=mock
export FERRIX_PIXEL9A_ACM_TIMEOUT='5;id'
run_audio bad_timeout inventory
check "usage: malformed transport timeout is refused before any session" sh -c 'test "$1" -eq 2 && test "$2" -eq 0' sh "$status" "$runs"
unset FERRIX_PIXEL9A_ACM_TIMEOUT
run_audio usage bogus
check "usage: unknown command exits 2 without contacting the guest" sh -c 'test "$1" -eq 2 && test "$2" -eq 0' sh "$status" "$runs"

if [ "$fails" -eq 0 ]; then
  echo '{"event":"pixel-9a-audio-inventory-test","evidence":"mock","status":"passed"}'
else
  printf '{"event":"pixel-9a-audio-inventory-test","evidence":"mock","status":"failed","failures":%s}\n' "$fails"
  exit 1
fi
