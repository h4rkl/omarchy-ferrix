#!/bin/sh
# Offline (mock) proof for the Pixel 9a read-only audio inventory. `ssh` is
# replaced by a stub that checks it received the checked-in guest probe on
# stdin and answers from synthetic fixtures, so this test never contacts a
# device. It checks the static read-only allow-list, the exact-target
# identity gates (QEMU, mismatch, forged, truncated, unreachable), explicit
# unavailable/unknown outcomes, unsupported-only statuses, mock/physical
# labelling, and redaction of the transcript and TSV.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
audio="$root/tools/pixel-9a/audio-inventory.sh"
probe="$root/tools/pixel-9a/audio-inventory-remote.sh"
fixtures="$root/tools/pixel-9a/tests/fixtures/audio"
target_id=google-pixel-9a-tegu-gxq96-mp1-0
fails=0

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

# Exactly one executable ssh line, inside remote(), sending the probe on stdin.
ssh_lines=$(grep -nE '^[[:space:]]*[^#]*(^|[^A-Za-z0-9_./-])ssh([[:space:]]|$)' "$audio" \
  | grep -vE '^[0-9]+:[[:space:]]*#' \
  | grep -vE '^[0-9]+:[[:space:]]*need ssh$' || true)
ssh_count=$(printf '%s\n' "$ssh_lines" | awk 'NF { count++ } END { print count + 0 }')
check "static: exactly one ssh invocation" [ "$ssh_count" -eq 1 ]
check "static: ssh uses the radios.sh options and sends the probe on stdin" \
  sh -c 'printf "%s\n" "$1" | grep -qE "^[0-9]+:[[:space:]]+ssh -o BatchMode=yes -o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new .*.sh -s. < \"\\\$remote_probe\"$"' sh "$ssh_lines"
check "static: ssh line lives inside remote()" \
  sh -c 'awk "/^remote\\(\\) \\{/{inside=1} inside && /^}/{inside=0} inside && /^[[:space:]]+ssh /{found=1} END{exit !found}" "$1"' sh "$audio"
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
check "static: identity gate precedes the first audio probe" \
  sh -c 'gate=$(grep -n "exit 4" "$1" | head -n 1 | cut -d: -f1); first=$(grep -nE "asound|wpctl status" "$1" | grep -v "^[0-9]*:#" | head -n 1 | cut -d: -f1); [ -n "$gate" ] && [ -n "$first" ] && [ "$gate" -lt "$first" ]' sh "$probe"

# Running the probe on this (non-tegu) host must refuse before any audio
# probe; only the identity verdict is printed.
local_status=0
local_out=$(sh "$probe" 2>&1) || local_status=$?
check "probe: refuses a non-tegu host with exit 4 or 5" sh -c '[ "$1" -eq 4 ] || [ "$1" -eq 5 ]' sh "$local_status"
check "probe: refusal prints no audio fields" \
  sh -c '! printf "%s\n" "$1" | grep -Eq "^(alsa_|tool_|pipewire|probe=)"' sh "$local_out"

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-audio-test.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
calls="$work/calls.log"

cat > "$work/bin/ssh" <<'STUB'
#!/bin/sh
# Stub: require BatchMode and `sh -s`, require the exact checked-in probe on
# stdin, then answer from a synthetic fixture.
[ "$1" = -o ] && [ "$2" = BatchMode=yes ] || { echo "stub: ssh without BatchMode" >&2; exit 98; }
for arg; do last=$arg; done
[ "$last" = 'sh -s' ] || { echo "stub: unexpected remote command: $last" >&2; exit 99; }
printf '%s\n' "$*" >> "$STUB_CALLS"
cat > "$STUB_CALLS.stdin"
cmp -s "$STUB_CALLS.stdin" "$STUB_PROBE" || { echo "stub: stdin is not the checked-in probe" >&2; exit 97; }
[ "${STUB_UNREACHABLE:-0}" = 1 ] && { echo "ssh: connect to host 172.16.42.1 port 22: No route to host" >&2; exit 255; }
cat "$STUB_FIXTURES/$STUB_GUEST.txt"
exit "${STUB_EXIT:-0}"
STUB
chmod 0755 "$work/bin/ssh"
export PATH="$work/bin:$PATH" STUB_CALLS="$calls" STUB_FIXTURES="$fixtures" STUB_PROBE="$probe"
unset FERRIX_PIXEL9A_SSH_HOST FERRIX_PIXEL9A_SSH_USER FERRIX_PIXEL9A_SSH_PORT FERRIX_PIXEL9A_SSH_OPTS 2>/dev/null || true
export FERRIX_PIXEL9A_AUDIO_EVIDENCE=mock

run_audio() {
  case_name=$1
  shift
  mkdir -p "$work/$case_name"
  export FERRIX_OUT_DIR="$work/$case_name"
  : > "$calls"
  status=0
  "$audio" "$@" > "$work/$case_name/stdout" 2> "$work/$case_name/stderr" || status=$?
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
check "inventory: one SSH session to the pmOS default endpoint" \
  sh -c 'test "$(wc -l < "$1" | tr -d " ")" -eq 1 && grep -q "user@172.16.42.1 sh -s$" "$1"' sh "$calls"
check "inventory: identity verified" grep -q '^guest_identity=verified$' "$work/full/stdout"
check "inventory: writes the TSV and transcript" sh -c '[ -f "$1" ] && [ -n "$2" ] && [ -f "$2" ]' sh "$tsv" "$transcript"
check "inventory: header has subsystem/status/progress/evidence" grep -q "^subsystem	status	progress	evidence$" "$tsv"
check "inventory: ALSA playback and capture PCMs enumerated" tsv_row "$tsv" alsa alsa-playback-and-capture-pcms-enumerated
check "inventory: ALSA evidence records counts" grep -q 'cards=1 pcms=3 playback=2 capture=1' "$tsv"
check "inventory: kernel audio surface present without logged failures" tsv_row "$tsv" kernel-audio audio-kernel-surface-present-no-probe-failure-logged
check "inventory: firmware files present" tsv_row "$tsv" audio-firmware audio-firmware-files-present-no-load-failure-logged
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
  sh -c 'test "$(grep -vc "^#" "$1")" -eq 8 && test "$(grep -c "	mock: stubbed transport for $2 (not physical evidence); " "$1")" -eq 7' sh "$tsv" "$target_id"
check "inventory: mock run never claims physical evidence" sh -c '! grep -q "physical: $2" "$1" "$3"' sh "$tsv" "$target_id" "$transcript"
check "inventory: transcript labels mock" grep -q "^# evidence: mock: stubbed transport for $target_id" "$transcript"
check "inventory: transcript keeps raw sections" \
  sh -c 'grep -q "^## begin proc-asound-pcm$" "$1" && grep -q "^## begin wpctl-status$" "$1" && grep -q "^## begin ferrix-core-state$" "$1"' sh "$transcript"
check "privacy: underscore Bluetooth MAC is hashed" \
  sh -c '! grep -q "AA_BB_CC_DD_EE_01" "$1" && grep -q "bluez_output.mac:[0-9a-f]\{12\}" "$1"' sh "$transcript"
check "privacy: serials are removed" sh -c '! grep -Eq "SYNSERIAL42|SYNAUD0001" "$1"' sh "$transcript"
check "privacy: cookies are removed" sh -c '! grep -Eq "1234567890|6f6f:aa11" "$1"' sh "$transcript"
check "privacy: host names are removed" sh -c '! grep -q "ferrix-synth-host" "$1"' sh "$transcript"
check "inventory: capabilities.toml and status.md are untouched" \
  sh -c 'cd "$1" && git diff --quiet -- "targets/boards/$2/capabilities.toml" docs/status.md' sh "$root" "$target_id"

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

export STUB_GUEST=guest-tegu STUB_UNREACHABLE=1
run_audio unreachable inventory
check "unreachable: SSH transport failure exits 3" [ "$status" -eq 3 ]
unset STUB_UNREACHABLE

# --- labels and usage ---
unset FERRIX_PIXEL9A_AUDIO_EVIDENCE
run_audio physical_label inventory
check "label: default run labels the exact physical target" \
  sh -c 'test "$(grep -c "	physical: $2; " "$1")" -eq 7' sh "$work/physical_label/pixel-9a/audio/audio-evidence-latest.tsv" "$target_id"
export FERRIX_PIXEL9A_AUDIO_EVIDENCE=qemu
run_audio bad_label inventory
check "label: only physical or mock labels are accepted" sh -c 'test "$1" -eq 2 && test ! -s "$2"' sh "$status" "$calls"
export FERRIX_PIXEL9A_AUDIO_EVIDENCE=mock
export FERRIX_PIXEL9A_SSH_HOST='172.16.42.1;id'
run_audio bad_host inventory
check "usage: malformed SSH host is refused before SSH" sh -c 'test "$1" -eq 2 && test ! -s "$2"' sh "$status" "$calls"
unset FERRIX_PIXEL9A_SSH_HOST
run_audio usage bogus
check "usage: unknown command exits 2 without contacting the guest" sh -c 'test "$1" -eq 2 && test ! -s "$2"' sh "$status" "$calls"

if [ "$fails" -eq 0 ]; then
  echo '{"event":"pixel-9a-audio-inventory-test","evidence":"mock","status":"passed"}'
else
  printf '{"event":"pixel-9a-audio-inventory-test","evidence":"mock","status":"failed","failures":%s}\n' "$fails"
  exit 1
fi
