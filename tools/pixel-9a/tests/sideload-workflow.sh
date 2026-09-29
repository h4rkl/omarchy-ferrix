#!/bin/sh
# Offline proof for the Pixel 9a stock OTA sideload workflow. Platform Tools
# are replaced with strict stubs that exit 99 on any unexpected verb, tiny
# fake recovery archives stand in for the multi-gigabyte pins, and typed
# confirmations are fed on stdin. This test never enumerates or contacts a
# real device.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script="$root/tools/pixel-9a/sideload-ota.sh"
fails=0
tab=$(printf '\t')

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

[ -x "$script" ] || {
  echo 'sideload workflow is not executable' >&2
  exit 1
}
sh -n "$script"

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-sideload-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM

# Statically require every adb/fastboot token in command position (line
# start, or right after `(`, `|`, `{`, `;`, `&`, or a backtick) to be one of
# these exact invocations: devices, shell getprop, get-state, reboot recovery,
# sideload, and fastboot devices (ambiguity counting only). Tokens inside
# message strings are not commands and are ignored.
cat > "$work/allow.txt" <<'EOF'
adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
adb -s "$adb_serial" shell getprop "$1" </dev/null | tr -d '\r'
[ "$(adb -s "$adb_serial" get-state </dev/null | tr -d '\r')" = "$wanted_state" ] || {
adb -s "$adb_serial" reboot recovery
adb -s "$adb_serial" sideload "$ota_path" </dev/null 2>&1 || printf '%s' "$?" > "$status_file"
EOF
offending=$(grep -nE '(^[[:space:]]*|[(|{;&`][[:space:]]*)(adb|fastboot)([[:space:]]|$)' "$script" \
  | grep -vE '^[0-9]+:[[:space:]]*#' \
  | sed 's/^[0-9]*:[[:space:]]*//' \
  | grep -vxF -f "$work/allow.txt" || true)
check "static: sideload contains only allow-listed device verbs" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
check "static: sideload issues no fastboot -s command at all" \
  sh -c "! grep -qE 'fastboot -s' '$script'"
check "static: sideload never spells a flash/erase/unlock/set_active adb or fastboot verb" \
  sh -c "! grep -nE '(adb|fastboot) (-s \"[^\"]*\" )?(flashing|erase|format|set_active|oem|flash|update|boot|reboot bootloader|root|remount|push|install)([^[:alnum:]_-]|\$)' '$script' | grep -q ."

mkdir -p "$work/bin" "$work/target/recovery" "$work/state"
calls="$work/calls.log"
: > "$calls"

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    sha256sum "$1" | awk '{ print $1 }'
  fi
}

# Tiny fake archives with real zip structure; the workflow only needs their
# pinned size and hash.
mkdir -p "$work/ota-src/META-INF/com/android" "$work/factory-src/tegu-fake"
printf 'post-build=google/tegu/tegu:17/FAKE/1:user/release-keys\npre-device=tegu\n' > "$work/ota-src/META-INF/com/android/metadata"
printf 'fake payload\n' > "$work/ota-src/payload.bin"
(cd "$work/ota-src" && zip -q -X -r "$work/target/recovery/tegu-ota-fake.zip" .)
printf 'bootloader fixture\n' > "$work/factory-src/tegu-fake/bootloader-tegu-fake.img"
(cd "$work/factory-src" && zip -q -X -r "$work/target/recovery/tegu-fake-factory.zip" tegu-fake)
ota_hash=$(sha256_of "$work/target/recovery/tegu-ota-fake.zip")
ota_bytes=$(wc -c < "$work/target/recovery/tegu-ota-fake.zip" | tr -d '[:space:]')
factory_hash=$(sha256_of "$work/target/recovery/tegu-fake-factory.zip")
factory_bytes=$(wc -c < "$work/target/recovery/tegu-fake-factory.zip" | tr -d '[:space:]')
{
  printf '# kind\tbuild\tversion\tfilename\turl\tsha256\tbytes\tsource-page\n'
  printf 'factory\tFAKE\t17.0.0\ttegu-fake-factory.zip\thttps://example.invalid/factory.zip\t%s\t%s\thttps://example.invalid/images\n' "$factory_hash" "$factory_bytes"
  printf 'ota\tFAKE\t17.0.0\ttegu-ota-fake.zip\thttps://example.invalid/ota.zip\t%s\t%s\thttps://example.invalid/ota\n' "$ota_hash" "$ota_bytes"
} > "$work/target/recovery.pins"
cat > "$work/target/manifest.toml" <<'EOF'
[gates]
identity = "passed"
image_compatibility = "passed"
backup = "verified"
recovery_procedure = "tested"
per_run_confirmation = "required"
EOF
sed "s/${tab}${ota_hash}${tab}/${tab}0000000000000000000000000000000000000000000000000000000000000000${tab}/" \
  "$work/target/recovery.pins" > "$work/target/wrong.pins"

# The adb stub is stateful: after `reboot recovery` it reports the
# post-reboot device list and get-state so the workflow's polling can be
# exercised without sleeping.
cat > "$work/bin/adb" <<'STUB'
#!/bin/sh
printf 'adb\t' >> "$STUB_CALLS"
printf '%s\t' "$@" >> "$STUB_CALLS"
printf '\n' >> "$STUB_CALLS"
rebooted=0
[ -f "$STUB_STATE/rebooted" ] && rebooted=1
case "$1" in
  devices)
    [ "$#" -eq 2 ] && [ "$2" = -l ] || exit 99
    echo "List of devices attached"
    if [ "$rebooted" -eq 1 ]; then
      printf '%s' "${STUB_ADB_DEVICES_AFTER_REBOOT:-}"
    else
      printf '%s' "${STUB_ADB_DEVICES:-}"
    fi
    ;;
  -s)
    [ "$#" -ge 3 ] || exit 99
    case "$3" in
      shell)
        [ "$#" -eq 5 ] && [ "$4" = getprop ] || exit 99
        [ "${STUB_GETPROP_AVAILABLE:-1}" = 1 ] || { echo "stub: getprop in sideload" >&2; exit 99; }
        case "$5" in
          ro.product.manufacturer) echo Google ;;
          ro.product.name|ro.product.device|ro.product.board) echo "${STUB_PRODUCT:-tegu}" ;;
          ro.product.model) echo "Pixel 9a" ;;
          ro.boot.hardware.revision) echo "${STUB_REVISION:-MP1.0}" ;;
          ro.boot.hardware.sku) echo "${STUB_SKU:-GXQ96}" ;;
          ro.boot.bootloader) echo tegu-16.0-fake ;;
          ro.boot.slot_suffix) echo "${STUB_SLOT:-_a}" ;;
          ro.boot.slot_count) echo 2 ;;
          ro.boot.vbmeta.device_state) echo unlocked ;;
          ro.boot.dynamic_partitions) echo true ;;
          ro.build.fingerprint) echo google/tegu/tegu:15/FAKE/1:user/release-keys ;;
          *) echo "stub: unexpected getprop $5" >&2; exit 99 ;;
        esac
        ;;
      get-state)
        [ "$#" -eq 3 ] || exit 99
        if [ "$rebooted" -eq 1 ]; then
          echo "${STUB_GET_STATE_AFTER_REBOOT:-sideload}"
        else
          echo "${STUB_GET_STATE:-device}"
        fi
        ;;
      reboot)
        [ "$#" -eq 4 ] && [ "$4" = recovery ] || exit 99
        : > "$STUB_STATE/rebooted"
        ;;
      sideload)
        [ "$#" -eq 4 ] && [ -f "$4" ] || exit 99
        echo "serving: '$4'  (~47%)"
        echo "Total xfer: 1.00x"
        exit "${STUB_SIDELOAD_EXIT:-0}"
        ;;
      *) echo "stub: unexpected adb -s verb $3" >&2; exit 99 ;;
    esac
    ;;
  *) echo "stub: unexpected adb verb $1" >&2; exit 99 ;;
esac
STUB

cat > "$work/bin/fastboot" <<'STUB'
#!/bin/sh
printf 'fastboot\t' >> "$STUB_CALLS"
printf '%s\t' "$@" >> "$STUB_CALLS"
printf '\n' >> "$STUB_CALLS"
case "$1" in
  devices)
    [ "$#" -eq 2 ] && [ "$2" = -l ] || exit 99
    printf '%s' "${STUB_FASTBOOT_DEVICES:-}"
    ;;
  *) echo "stub: unexpected fastboot verb $1" >&2; exit 99 ;;
esac
STUB
chmod 0755 "$work/bin/adb" "$work/bin/fastboot"
export PATH="$work/bin:$PATH" STUB_CALLS="$calls" STUB_STATE="$work/state"
export FERRIX_PIXEL9A_TARGET_DIR="$work/target"
export FERRIX_PIXEL9A_SIDELOAD_WAIT_SECONDS=2

serial='SYN.00[1]/&$'

reset_stub() {
  rm -f "$STUB_STATE/rebooted"
  : > "$calls"
  unset STUB_ADB_DEVICES STUB_ADB_DEVICES_AFTER_REBOOT STUB_FASTBOOT_DEVICES \
    STUB_GET_STATE STUB_GET_STATE_AFTER_REBOOT STUB_GETPROP_AVAILABLE \
    STUB_PRODUCT STUB_REVISION STUB_SKU STUB_SLOT STUB_SIDELOAD_EXIT
  export FERRIX_PIXEL9A_TARGET_DIR="$work/target"
  export FERRIX_PIXEL9A_PINS="$work/target/recovery.pins"
}

run_case() {
  case_name=$1
  shift
  mkdir -p "$work/$case_name"
  export FERRIX_OUT_DIR="$work/$case_name"
  status=0
  sh "$script" "$@" > "$work/$case_name/stdout" 2> "$work/$case_name/stderr" < "$work/$case_name/stdin" || status=$?
}

feed() {
  case_dir=$1
  shift
  mkdir -p "$work/$case_dir"
  : > "$work/$case_dir/stdin"
  for line in "$@"; do
    printf '%s\n' "$line" >> "$work/$case_dir/stdin"
  done
}

mutations() {
  grep -E '^adb\t[^[:space:]]+\t(reboot|sideload)\t|^adb\t-s\t[^[:space:]]+\t(reboot|sideload)\t' "$calls" | sed 's/\t$//' || true
}

transcript_of() {
  find "$work/$1/pixel-9a/sideload" -name '*.txt' -type f | head -1
}

# --- preflight in Android -------------------------------------------------
reset_stub
export STUB_ADB_DEVICES="$serial device usb:1 product:tegu
"
feed device_preflight
run_case device_preflight --preflight
check "preflight: exact identity in Android passes" [ "$status" -eq 0 ]
check "preflight: identity_check names the Android transport" grep -q '^identity_check=verified-in-android$' "$work/device_preflight/stdout"
check "preflight: operator stdout retains exact serial" grep -Fq "serial=$serial" "$work/device_preflight/stdout"
check "preflight: partition target names stock A/B OTA" grep -q '^partition_target=stock A/B OTA (both slots per package)$' "$work/device_preflight/stdout"
check "preflight: input hash is the pinned OTA hash" grep -q "^input_image_sha256=$ota_hash$" "$work/device_preflight/stdout"
check "preflight: recovery artifact is the pinned factory archive" grep -q "^recovery_artifact=.*tegu-fake-factory.zip sha256:$factory_hash$" "$work/device_preflight/stdout"
check "preflight: no mutation verb was issued" [ -z "$(mutations)" ]
transcript=$(transcript_of device_preflight)
check "privacy: preflight transcript exists" [ -n "$transcript" ]
check "privacy: preflight transcript redacts the serial" env S="$serial" T="$transcript" sh -c '! grep -Fq "$S" "$T"'
check "privacy: preflight transcript carries the hashed marker" grep -q '^serial=serial-sha256:[0-9a-f]\{12\}$' "$transcript"

# --- preflight in stock recovery and in sideload mode ---------------------
reset_stub
export STUB_ADB_DEVICES="$serial recovery usb:1
"
feed recovery_preflight
run_case recovery_preflight --preflight
check "preflight: recovery state with matching visible props passes" [ "$status" -eq 0 ]
check "preflight: recovery identity_check is verified-in-recovery" grep -q '^identity_check=verified-in-recovery$' "$work/recovery_preflight/stdout"

reset_stub
export STUB_ADB_DEVICES="$serial sideload usb:1
" STUB_GETPROP_AVAILABLE=0
feed sideload_preflight
run_case sideload_preflight --preflight
check "preflight: sideload state passes without getprop" [ "$status" -eq 0 ]
check "preflight: sideload identity_check is unavailable-in-sideload" grep -q '^identity_check=unavailable-in-sideload$' "$work/sideload_preflight/stdout"
check "preflight: sideload state never calls getprop" sh -c "! grep -q 'getprop' '$calls'"

# Keep a copy of the admitted physical target's current gate state. The OTA
# route must stop before rebooting into recovery or sending the package while
# backup, image compatibility, and recovery remain unresolved.
cp -R "$work/target" "$work/target-gates-current"
sed -i.bak \
  -e 's/image_compatibility = "passed"/image_compatibility = "partial"/' \
  -e 's/backup = "verified"/backup = "pending"/' \
  -e 's/recovery_procedure = "tested"/recovery_procedure = "documented-not-tested"/' \
  "$work/target-gates-current/manifest.toml"
reset_stub
export STUB_ADB_DEVICES="$(printf '%s device usb:1\n' "$serial")" FERRIX_PIXEL9A_TARGET_DIR="$work/target-gates-current"
feed pending_target_gates
run_case pending_target_gates --confirm-sideload
check "gates: pending/partial/documented-not-tested sideload refuses before confirmation" \
  sh -c "[ $status -eq 1 ] && grep -q \"^REFUSING: backup gate is 'pending'\" '$work/pending_target_gates/stderr'"
check "gates: pending sideload issues zero mutations" [ -z "$(mutations)" ]

# --- refusals -------------------------------------------------------------
reset_stub
export STUB_ADB_DEVICES="$serial device usb:1
" STUB_REVISION=EVT1.0
feed wrong_revision
run_case wrong_revision --preflight
check "refuse: wrong hardware revision" [ "$status" -eq 1 ]
check "refuse: wrong revision prints REFUSING" grep -q '^REFUSING: hardware revision mismatch: EVT1.0$' "$work/wrong_revision/stderr"

reset_stub
export STUB_ADB_DEVICES="$serial recovery usb:1
" STUB_PRODUCT=komodo
feed wrong_recovery_product
run_case wrong_recovery_product --preflight
check "refuse: contradicting codename in recovery" [ "$status" -eq 1 ]

reset_stub
export STUB_ADB_DEVICES="$serial device usb:1
SERIAL-B device usb:2
"
feed ambiguous_adb
run_case ambiguous_adb --preflight
check "refuse: two ADB devices" [ "$status" -eq 1 ]
check "refuse: ambiguous devices never query properties" sh -c "! grep -q 'getprop' '$calls'"

reset_stub
export STUB_ADB_DEVICES="$serial device usb:1
" STUB_FASTBOOT_DEVICES="SERIAL-B fastboot
"
feed ambiguous_transports
run_case ambiguous_transports --preflight
check "refuse: simultaneous ADB and Fastboot" [ "$status" -eq 1 ]

reset_stub
export STUB_ADB_DEVICES="$serial device usb:1
" FERRIX_PIXEL9A_PINS="$work/target/wrong.pins"
feed hash_mismatch
run_case hash_mismatch --preflight
check "refuse: OTA hash mismatch" [ "$status" -eq 1 ]
check "refuse: hash mismatch happens before any device call" [ ! -s "$calls" ]

reset_stub
export STUB_ADB_DEVICES="$serial unauthorized usb:1
"
feed unauthorized
run_case unauthorized --preflight
check "refuse: unauthorized ADB state" [ "$status" -eq 1 ]

# --- execution: wrong phrase stops before any mutation ---------------------
reset_stub
export STUB_ADB_DEVICES="$serial device usb:1
"
feed wrong_phrase "CONFIRM SOMETHING ELSE"
run_case wrong_phrase --confirm-sideload
check "execute: wrong first phrase exits 1" [ "$status" -eq 1 ]
check "execute: wrong first phrase performs no mutation" [ -z "$(mutations)" ]
check "execute: wrong phrase message names no mutation" grep -q 'no mutation performed' "$work/wrong_phrase/stderr"

reset_stub
export STUB_ADB_DEVICES="$serial device usb:1
" STUB_ADB_DEVICES_AFTER_REBOOT="$serial sideload usb:1
"
feed wrong_second_phrase "CONFIRM PIXEL 9A REBOOT TO RECOVERY FOR OTA SIDELOAD" "CONFIRM PIXEL 9A OTA SIDELOAD wrong.zip SHA256 0000000000000000"
run_case wrong_second_phrase --confirm-sideload
check "execute: wrong second phrase exits 1" [ "$status" -eq 1 ]
expected_mutations=$(printf 'adb\t-s\t%s\treboot\trecovery' "$serial")
check "execute: wrong second phrase rebooted but never sideloaded" [ "$(mutations)" = "$expected_mutations" ]

# --- execution: full flow from Android ------------------------------------
reset_stub
export STUB_ADB_DEVICES="$serial device usb:1
" STUB_ADB_DEVICES_AFTER_REBOOT="$serial sideload usb:1
"
ota_short=$(printf '%s' "$ota_hash" | cut -c1-16)
feed full_flow "CONFIRM PIXEL 9A REBOOT TO RECOVERY FOR OTA SIDELOAD" "CONFIRM PIXEL 9A OTA SIDELOAD tegu-ota-fake.zip SHA256 $ota_short"
run_case full_flow --confirm-sideload
check "execute: full flow from Android exits 0" [ "$status" -eq 0 ]
expected_mutations=$(printf 'adb\t-s\t%s\treboot\trecovery\nadb\t-s\t%s\tsideload\t%s' "$serial" "$serial" "$work/target/recovery/tegu-ota-fake.zip")
check "execute: exact mutation sequence (reboot recovery, sideload)" [ "$(mutations)" = "$expected_mutations" ]
check "execute: mutation lines precede commands on stdout" grep -q '^mutation=adb reboot recovery$' "$work/full_flow/stdout"
check "execute: sideload mutation line names the OTA file" grep -q '^mutation=adb sideload tegu-ota-fake.zip$' "$work/full_flow/stdout"
check "execute: result is ota-sideload-complete" grep -q '^result=ota-sideload-complete$' "$work/full_flow/stdout"
check "execute: get-state was rechecked in sideload before sending" grep -q "get-state" "$calls"
transcript=$(transcript_of full_flow)
check "privacy: execution transcript redacts the serial" env S="$serial" T="$transcript" sh -c '! grep -Fq "$S" "$T"'
check "privacy: execution transcript records the mutations" grep -q '^mutation=adb sideload tegu-ota-fake.zip$' "$transcript"
check "privacy: execution transcript records confirmations without the phrase" grep -q '^confirmation=accepted$' "$transcript"

# --- execution: already in sideload requires the identity acknowledgement --
reset_stub
export STUB_ADB_DEVICES="$serial sideload usb:1
" STUB_GET_STATE=sideload STUB_GETPROP_AVAILABLE=0
feed sideload_plain "CONFIRM PIXEL 9A OTA SIDELOAD tegu-ota-fake.zip SHA256 $ota_short"
run_case sideload_plain --confirm-sideload
check "execute: sideload-state phrase without identity acknowledgement is refused" [ "$status" -eq 1 ]
check "execute: refused acknowledgement performs no mutation" [ -z "$(mutations)" ]

reset_stub
export STUB_ADB_DEVICES="$serial sideload usb:1
" STUB_GET_STATE=sideload STUB_GETPROP_AVAILABLE=0
feed sideload_ack "CONFIRM PIXEL 9A OTA SIDELOAD tegu-ota-fake.zip SHA256 $ota_short IDENTITY unavailable-in-sideload"
run_case sideload_ack --confirm-sideload
check "execute: sideload-state flow with acknowledgement exits 0" [ "$status" -eq 0 ]
check "execute: sideload-state flow issues only the sideload mutation" \
  [ "$(mutations)" = "$(printf 'adb\t-s\t%s\tsideload\t%s' "$serial" "$work/target/recovery/tegu-ota-fake.zip")" ]

# --- execution: sideload failure is reported honestly ----------------------
reset_stub
export STUB_ADB_DEVICES="$serial sideload usb:1
" STUB_GET_STATE=sideload STUB_GETPROP_AVAILABLE=0 STUB_SIDELOAD_EXIT=1
feed sideload_fail "CONFIRM PIXEL 9A OTA SIDELOAD tegu-ota-fake.zip SHA256 $ota_short IDENTITY unavailable-in-sideload"
run_case sideload_fail --confirm-sideload
check "execute: failed sideload exits 1" [ "$status" -eq 1 ]
check "execute: failed sideload reports result=failed" grep -q '^result=failed$' "$work/sideload_fail/stdout"

# --- execution: device never enters sideload -------------------------------
reset_stub
export STUB_ADB_DEVICES="$serial device usb:1
" STUB_ADB_DEVICES_AFTER_REBOOT="$serial recovery usb:1
"
feed never_sideload "CONFIRM PIXEL 9A REBOOT TO RECOVERY FOR OTA SIDELOAD"
run_case never_sideload --confirm-sideload
check "execute: bounded wait refuses when sideload never appears" [ "$status" -eq 1 ]
check "execute: bounded wait sent no OTA" sh -c "! grep -q 'sideload.*tegu-ota-fake' '$calls'"

if [ "$fails" -eq 0 ]; then
  echo '{"event":"pixel-9a-sideload-test","status":"passed"}'
else
  printf '{"event":"pixel-9a-sideload-test","status":"failed","failures":%s}\n' "$fails"
  exit 1
fi
