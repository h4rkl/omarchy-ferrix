#!/bin/sh
# Offline proof for the Pixel 9a Fastboot factory restore workflow. Platform
# Tools are replaced with strict stateful stubs that exit 99 on any unexpected
# verb, a tiny real zip stands in for Google's factory archive, and typed
# confirmations are fed on stdin. This test never enumerates or contacts a
# real device.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script="$root/tools/pixel-9a/factory-restore.sh"
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
  echo 'factory restore workflow is not executable' >&2
  exit 1
}
sh -n "$script"
for tool in zip unzip; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "missing $tool; the offline restore test builds a fake factory archive" >&2
    exit 1
  }
done

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-restore-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM

# Statically require every adb/fastboot token in command position (line
# start, or right after `(`, `|`, `{`, `;`, `&`, or a backtick) to be one of
# these exact invocations. Tokens inside message strings are not commands.
cat > "$work/allow.txt" <<'EOF'
adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
adb -s "$serial" shell getprop "$1" </dev/null | tr -d '\r'
fastboot -s "$serial" getvar "$variable" 2>&1 | awk -v key="$variable" '
[ "$(adb -s "$serial" get-state </dev/null | tr -d '\r')" = device ] || {
adb -s "$serial" reboot bootloader
fastboot -s "$serial" flash bootloader "$image" </dev/null 2>&1 || printf '%s' "$?" > "$status_file"
fastboot -s "$serial" flash radio "$image" </dev/null 2>&1 || printf '%s' "$?" > "$status_file"
fastboot -s "$serial" reboot-bootloader </dev/null 2>&1 || printf '%s' "$?" > "$status_file"
fastboot -s "$serial" -w update "$stage_image" </dev/null 2>&1 || printf '%s' "$?" > "$status_file"
fastboot -s "$serial" update "$stage_image" </dev/null 2>&1 || printf '%s' "$?" > "$status_file"
EOF
offending=$(grep -nE '(^[[:space:]]*|[(|{;&`][[:space:]]*)(adb|fastboot)([[:space:]]|$)' "$script" \
  | grep -vE '^[0-9]+:[[:space:]]*#' \
  | sed 's/^[0-9]*:[[:space:]]*//' \
  | grep -vxF -f "$work/allow.txt" || true)
check "static: restore contains only allow-listed device verbs" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
check "static: restore never issues flashing/erase/format/set_active/oem/boot" \
  sh -c "! grep -nE 'fastboot -s \"[^\"]*\" (-w )?(flashing|erase|format|set_active|oem|boot|continue|reboot |wipe-super|snapshot-update|delete-logical-partition)' '$script' | grep -q ."
check "static: restore never passes --slot or --skip-reboot" \
  sh -c "! grep -qE -- '--slot|--skip-reboot' '$script'"

mkdir -p "$work/bin" "$work/target/recovery" "$work/state" "$work/inventory"
calls="$work/calls.log"
: > "$calls"

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    sha256sum "$1" | awk '{ print $1 }'
  fi
}

required_bootloader=tegu-17.0-fake
required_baseband=G5300T-FAKE-B-1

# Fake factory archive with the real outer layout: flash-all.sh,
# bootloader-*.img, radio-*.img, and a nested image-tegu-*.zip whose
# android-info.txt carries the require lines.
build_factory() {
  destination=$1
  flash_all_extra=$2
  src="$work/factory-src-$(basename "$destination" .zip)"
  rm -rf "$src"
  mkdir -p "$src/tegu-fake" "$src/inner"
  printf 'require version-baseband=%s\nrequire board=tegu\nrequire version-bootloader=%s\n' \
    "$required_baseband" "$required_bootloader" > "$src/inner/android-info.txt"
  printf 'version 1\nflash boot\nreboot fastboot\nupdate-super\n' > "$src/inner/fastboot-info.txt"
  printf 'boot fixture\n' > "$src/inner/boot.img"
  (cd "$src/inner" && zip -q -X "$src/tegu-fake/image-tegu-fake.zip" android-info.txt fastboot-info.txt boot.img)
  printf 'bootloader fixture\n' > "$src/tegu-fake/bootloader-tegu-fake.img"
  printf 'radio fixture\n' > "$src/tegu-fake/radio-g5300t-fake.img"
  {
    printf '#!/bin/sh\n\n# fixture copy of the vendor flash-all.sh\n'
    printf 'if ! [ $($(which fastboot) --version | grep "version" | cut -c18-23 | sed '"'"'s/\\.//g'"'"' ) -ge 3301 ]; then\n'
    printf '  echo "fastboot too old"\n  exit 1\nfi\n'
    printf 'fastboot flash bootloader bootloader-tegu-fake.img\n'
    printf 'fastboot reboot-bootloader\nsleep 5\n'
    printf 'fastboot flash radio radio-g5300t-fake.img\n'
    printf 'fastboot reboot-bootloader\nsleep 5\n'
    [ -z "$flash_all_extra" ] || printf '%s\n' "$flash_all_extra"
    printf 'fastboot -w update image-tegu-fake.zip\n'
  } > "$src/tegu-fake/flash-all.sh"
  rm -f "$destination"
  (cd "$src" && zip -q -X -r "$destination" tegu-fake)
}

build_factory "$work/target/recovery/tegu-fake-factory.zip" ""
mkdir -p "$work/ota-src/META-INF/com/android"
printf 'post-build=google/tegu/tegu:17/FAKE/1:user/release-keys\npre-device=tegu\n' > "$work/ota-src/META-INF/com/android/metadata"
printf 'fake payload\n' > "$work/ota-src/payload.bin"
(cd "$work/ota-src" && zip -q -X -r "$work/target/recovery/tegu-ota-fake.zip" .)

factory_hash=$(sha256_of "$work/target/recovery/tegu-fake-factory.zip")
factory_bytes=$(wc -c < "$work/target/recovery/tegu-fake-factory.zip" | tr -d '[:space:]')
ota_hash=$(sha256_of "$work/target/recovery/tegu-ota-fake.zip")
ota_bytes=$(wc -c < "$work/target/recovery/tegu-ota-fake.zip" | tr -d '[:space:]')
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
sed "s/${tab}${factory_hash}${tab}/${tab}0000000000000000000000000000000000000000000000000000000000000000${tab}/" \
  "$work/target/recovery.pins" > "$work/target/wrong.pins"

write_inventory() {
  cat > "$1" <<EOF
{
  "schema_version": 1,
  "target_id": "google-pixel-9a-tegu-gxq96-mp1-0",
  "factory": {
    "filename": "tegu-fake-factory.zip",
    "sha256": "$2",
    "verified": true
  },
  "android_info": {
    "lines": [
      "require version-baseband=$required_baseband",
      "require board=tegu",
      "require version-bootloader=$required_bootloader"
    ]
  }
}
EOF
}
write_inventory "$work/inventory/inventory.json" "$factory_hash"
write_inventory "$work/inventory/stale.json" 1111111111111111111111111111111111111111111111111111111111111111

# A tampered archive whose flash-all.sh carries an extra erase command; the
# workflow must refuse to replay anything that differs from the reviewed
# five-line sequence.
mkdir -p "$work/tampered/recovery"
build_factory "$work/tampered/recovery/tegu-fake-factory.zip" "fastboot erase userdata"
cp "$work/target/recovery/tegu-ota-fake.zip" "$work/tampered/recovery/"
tampered_hash=$(sha256_of "$work/tampered/recovery/tegu-fake-factory.zip")
tampered_bytes=$(wc -c < "$work/tampered/recovery/tegu-fake-factory.zip" | tr -d '[:space:]')
sed "s/${tab}${factory_hash}${tab}${factory_bytes}${tab}/${tab}${tampered_hash}${tab}${tampered_bytes}${tab}/" \
  "$work/target/recovery.pins" > "$work/tampered/recovery.pins"
write_inventory "$work/inventory/tampered.json" "$tampered_hash"

cat > "$work/bin/adb" <<'STUB'
#!/bin/sh
printf 'adb\t' >> "$STUB_CALLS"
printf '%s\t' "$@" >> "$STUB_CALLS"
printf '\n' >> "$STUB_CALLS"
rebooted=0
[ -f "$STUB_STATE/adb-rebooted" ] && rebooted=1
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
        case "$5" in
          ro.product.manufacturer) echo Google ;;
          ro.product.name|ro.product.device|ro.product.board) echo tegu ;;
          ro.product.model) echo "Pixel 9a" ;;
          ro.boot.hardware.revision) echo "${STUB_REVISION:-MP1.0}" ;;
          ro.boot.hardware.sku) echo GXQ96 ;;
          ro.boot.bootloader) echo tegu-16.0-old ;;
          gsm.version.baseband) echo g5300t-old ;;
          ro.boot.slot_suffix) echo _a ;;
          ro.boot.slot_count) echo 2 ;;
          ro.boot.vbmeta.device_state) echo unlocked ;;
          ro.boot.dynamic_partitions) echo true ;;
          *) echo "stub: unexpected getprop $5" >&2; exit 99 ;;
        esac
        ;;
      get-state)
        [ "$#" -eq 3 ] || exit 99
        echo device
        ;;
      reboot)
        [ "$#" -eq 4 ] && [ "$4" = bootloader ] || exit 99
        : > "$STUB_STATE/adb-rebooted"
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
    if [ -f "$STUB_STATE/adb-rebooted" ]; then
      printf '%s' "${STUB_FASTBOOT_DEVICES_AFTER_REBOOT:-}"
    else
      printf '%s' "${STUB_FASTBOOT_DEVICES:-}"
    fi
    ;;
  -s)
    [ "$#" -ge 3 ] || exit 99
    serial=$2
    case "$3" in
      getvar)
        [ "$#" -eq 4 ] || exit 99
        case "$4" in
          product) value=tegu ;;
          variant) value=${STUB_VARIANT:-GXQ96} ;;
          hw-revision) value=${STUB_HW_REVISION:-MP1.0} ;;
          current-slot) value=a ;;
          slot-count) value=2 ;;
          unlocked) value=${STUB_UNLOCKED:-yes} ;;
          secure) value=yes ;;
          is-userspace) value=${STUB_IS_USERSPACE:-no} ;;
          serialno) value=$serial ;;
          version-bootloader)
            if [ -f "$STUB_STATE/flashed-bootloader" ] || [ "${STUB_ALREADY_CURRENT:-0}" = 1 ]; then
              value=tegu-17.0-fake
            else
              value=tegu-16.0-old
            fi
            ;;
          version-baseband)
            if [ -f "$STUB_STATE/flashed-radio" ] || [ "${STUB_ALREADY_CURRENT:-0}" = 1 ]; then
              value=g5300t-fake-b-1
            else
              value=g5300t-old
            fi
            ;;
          *) echo "stub: unexpected getvar $4" >&2; exit 99 ;;
        esac
        printf '%s: %s\n' "$4" "$value" >&2
        printf 'Finished. Total time: 0.001s\n' >&2
        ;;
      flash)
        [ "$#" -eq 5 ] && [ -f "$5" ] || exit 99
        case "$4" in
          bootloader) : > "$STUB_STATE/flashed-bootloader" ;;
          radio) : > "$STUB_STATE/flashed-radio" ;;
          *) echo "stub: unexpected flash partition $4" >&2; exit 99 ;;
        esac
        printf 'Sending %s OKAY\nWriting %s OKAY\n' "$4" "$4" >&2
        ;;
      reboot-bootloader)
        [ "$#" -eq 3 ] || exit 99
        printf 'Rebooting into bootloader OKAY\n' >&2
        ;;
      update)
        [ "$#" -eq 4 ] && [ -f "$4" ] || exit 99
        printf 'stub update %s\n' "$4" >&2
        exit "${STUB_UPDATE_EXIT:-0}"
        ;;
      -w)
        [ "$#" -eq 5 ] && [ "$4" = update ] && [ -f "$5" ] || exit 99
        printf 'stub -w update %s\n' "$5" >&2
        exit "${STUB_UPDATE_EXIT:-0}"
        ;;
      *) echo "stub: unexpected fastboot -s verb $3" >&2; exit 99 ;;
    esac
    ;;
  *) echo "stub: unexpected fastboot verb $1" >&2; exit 99 ;;
esac
STUB
chmod 0755 "$work/bin/adb" "$work/bin/fastboot"
export PATH="$work/bin:$PATH" STUB_CALLS="$calls" STUB_STATE="$work/state"
export FERRIX_PIXEL9A_SETTLE_SECONDS=0 FERRIX_PIXEL9A_FASTBOOT_WAIT_SECONDS=2

serial='SYN.00[1]/&$'

reset_stub() {
  rm -f "$STUB_STATE"/*
  : > "$calls"
  unset STUB_ADB_DEVICES STUB_ADB_DEVICES_AFTER_REBOOT STUB_FASTBOOT_DEVICES \
    STUB_FASTBOOT_DEVICES_AFTER_REBOOT STUB_HW_REVISION STUB_VARIANT STUB_UNLOCKED \
    STUB_IS_USERSPACE STUB_ALREADY_CURRENT STUB_UPDATE_EXIT STUB_REVISION
  export FERRIX_PIXEL9A_TARGET_DIR="$work/target"
  export FERRIX_PIXEL9A_PINS="$work/target/recovery.pins"
  export FERRIX_PIXEL9A_FACTORY_INVENTORY="$work/inventory/inventory.json"
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

run_case() {
  case_name=$1
  shift
  mkdir -p "$work/$case_name"
  export FERRIX_OUT_DIR="$work/$case_name"
  status=0
  sh "$script" "$@" > "$work/$case_name/stdout" 2> "$work/$case_name/stderr" < "$work/$case_name/stdin" || status=$?
}

# State-changing calls from the log, with the serial dropped and staged paths
# reduced to their basenames so the sequence can be compared exactly.
mutations() {
  awk -F '\t' '
    ($1 == "fastboot" && ($4 == "flash" || $4 == "reboot-bootloader" || $4 == "update" || $4 == "-w")) ||
    ($1 == "adb" && $4 == "reboot") {
      out = $1
      for (i = 4; i < NF; i++) {
        field = $i
        sub(".*/", "", field)
        out = out "\t" field
      }
      print out
    }' "$calls"
}

transcript_of() {
  find "$work/$1/pixel-9a/restore" -name '*.txt' -type f | head -1
}

stage_dirs_of() {
  find "$work/$1/pixel-9a/restore" -name 'stage-*' -type d 2>/dev/null
}

fastboot_one="$serial fastboot usb:1
"

# --- preflight over Fastboot ------------------------------------------------
reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one"
feed preflight
run_case preflight --preflight
check "preflight: exact Fastboot identity passes" [ "$status" -eq 0 ]
check "preflight: vendor flash-all.sh matches the reviewed sequence" grep -q '^vendor_sequence_check=matches-reviewed-sequence$' "$work/preflight/stdout"
check "preflight: bootloader update is marked needed" grep -q '^bootloader_update=needed$' "$work/preflight/stdout"
check "preflight: radio update is marked needed" grep -q '^radio_update=needed$' "$work/preflight/stdout"
check "preflight: required bootloader comes from the inventory" grep -q "^required_bootloader=$required_bootloader$" "$work/preflight/stdout"
check "preflight: partition target names the vendor stages" grep -q '^partition_target=bootloader, radio, then all images in image-tegu-fake.zip via fastboot update$' "$work/preflight/stdout"
check "preflight: input hash is the pinned factory hash" grep -q "^input_image_sha256=$factory_hash$" "$work/preflight/stdout"
check "preflight: recovery artifact is the pinned OTA" grep -q "^recovery_artifact=.*tegu-ota-fake.zip sha256:$ota_hash$" "$work/preflight/stdout"
check "preflight: flash slot is the vendor default" grep -q '^flash_slot=vendor-default(current)$' "$work/preflight/stdout"
check "preflight: default mode wipes user data" grep -q '^user_data=wipe via fastboot -w update$' "$work/preflight/stdout"
check "preflight: operator stdout retains exact serial" grep -Fq "serial=$serial" "$work/preflight/stdout"
check "preflight: no mutation verb was issued" [ -z "$(mutations)" ]
check "preflight: no stage directory remains" [ -z "$(stage_dirs_of preflight)" ]
transcript=$(transcript_of preflight)
check "privacy: preflight transcript redacts the serial" env S="$serial" T="$transcript" sh -c '! grep -Fq "$S" "$T"'
check "privacy: preflight transcript carries the hashed marker" grep -q '^serial=serial-sha256:[0-9a-f]\{12\}$' "$transcript"

reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" STUB_ALREADY_CURRENT=1
feed preflight_current
run_case preflight_current --preflight --keep-data
check "preflight: current versions are marked already-current (case-insensitive)" \
  sh -c "grep -q '^bootloader_update=already-current$' '$work/preflight_current/stdout' && grep -q '^radio_update=already-current$' '$work/preflight_current/stdout'"
check "preflight: --keep-data is reported" grep -q '^user_data=keep via fastboot update without -w (--keep-data)$' "$work/preflight_current/stdout"

# Keep a copy of the admitted physical target's current gate state. The
# restore path must stop before its first confirmation or state-changing
# command while backup, image compatibility, and recovery remain unresolved.
cp -R "$work/target" "$work/target-gates-current"
sed -i.bak \
  -e 's/image_compatibility = "passed"/image_compatibility = "partial"/' \
  -e 's/backup = "verified"/backup = "pending"/' \
  -e 's/recovery_procedure = "tested"/recovery_procedure = "documented-not-tested"/' \
  "$work/target-gates-current/manifest.toml"
reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" FERRIX_PIXEL9A_TARGET_DIR="$work/target-gates-current"
feed pending_target_gates
run_case pending_target_gates --confirm-restore --keep-data
check "gates: pending/partial/documented-not-tested restore refuses before confirmation" \
  sh -c "[ $status -eq 1 ] && grep -q \"^REFUSING: backup gate is 'pending'\" '$work/pending_target_gates/stderr'"
check "gates: pending restore issues zero mutations" [ -z "$(mutations)" ]

# --- refusals ---------------------------------------------------------------
reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" STUB_HW_REVISION=EVT1.0
feed wrong_revision
run_case wrong_revision --preflight
check "refuse: wrong hardware revision" [ "$status" -eq 1 ]
check "refuse: wrong revision prints REFUSING" grep -q '^REFUSING: Fastboot hardware revision mismatch: EVT1.0$' "$work/wrong_revision/stderr"

reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" STUB_VARIANT=GB62
feed wrong_variant
run_case wrong_variant --preflight
check "refuse: wrong Fastboot SKU/variant" [ "$status" -eq 1 ]
check "refuse: wrong variant prints REFUSING" grep -q '^REFUSING: Fastboot SKU/variant mismatch: GB62$' "$work/wrong_variant/stderr"

reset_stub
export STUB_FASTBOOT_DEVICES="$serial fastboot usb:1
SERIAL-B fastboot usb:2
"
feed ambiguous
run_case ambiguous --preflight
check "refuse: two Fastboot devices" [ "$status" -eq 1 ]
check "refuse: ambiguous devices never read getvar" sh -c "! grep -q 'getvar' '$calls'"

reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" STUB_ADB_DEVICES="SERIAL-B device usb:2
"
feed mixed
run_case mixed --preflight
check "refuse: simultaneous ADB and Fastboot" [ "$status" -eq 1 ]

reset_stub
export STUB_ADB_DEVICES="$serial device usb:1
"
feed adb_only
run_case adb_only --preflight
check "refuse: Android device without --from-adb" [ "$status" -eq 1 ]
check "refuse: Android device hint names --from-adb" grep -q 'rerun with --from-adb' "$work/adb_only/stderr"

reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" FERRIX_PIXEL9A_PINS="$work/target/wrong.pins"
feed hash_mismatch
run_case hash_mismatch --preflight
check "refuse: factory hash mismatch" [ "$status" -eq 1 ]
check "refuse: hash mismatch happens before any device call" [ ! -s "$calls" ]

reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" STUB_UNLOCKED=no
feed locked
run_case locked --preflight
check "refuse: unlocked=no" [ "$status" -eq 1 ]
check "refuse: locked message names the unlock requirement" grep -q "REFUSING: Fastboot reports unlocked='no'" "$work/locked/stderr"

reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" STUB_IS_USERSPACE=yes
feed fastbootd
run_case fastbootd --preflight
check "refuse: is-userspace=yes (fastbootd)" [ "$status" -eq 1 ]

reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" FERRIX_PIXEL9A_FACTORY_INVENTORY="$work/inventory/missing.json"
feed no_inventory
run_case no_inventory --preflight
check "refuse: missing factory inventory" [ "$status" -eq 1 ]
check "refuse: missing inventory hint names the inspect recipe" grep -q 'pixel-9a-factory-inspect' "$work/no_inventory/stderr"

reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" FERRIX_PIXEL9A_FACTORY_INVENTORY="$work/inventory/stale.json"
feed stale_inventory
run_case stale_inventory --preflight
check "refuse: inventory recorded for a different archive hash" [ "$status" -eq 1 ]

reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" FERRIX_PIXEL9A_TARGET_DIR="$work/tampered" \
  FERRIX_PIXEL9A_PINS="$work/tampered/recovery.pins" FERRIX_PIXEL9A_FACTORY_INVENTORY="$work/inventory/tampered.json"
feed tampered
run_case tampered --preflight
check "refuse: vendor flash-all.sh with an extra erase line" [ "$status" -eq 1 ]
check "refuse: tampered sequence is named" grep -q 'differs from the reviewed' "$work/tampered/stderr"
check "refuse: tampered sequence is detected before any device call" [ ! -s "$calls" ]

# --- execution: wrong phrases stop before the corresponding stage -----------
reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one"
feed wrong_phrase "CONFIRM SOMETHING ELSE"
run_case wrong_phrase --confirm-restore
check "execute: wrong stage-1 phrase exits 1" [ "$status" -eq 1 ]
check "execute: wrong stage-1 phrase performs no mutation" [ -z "$(mutations)" ]
check "execute: wrong phrase message names no mutation" grep -q 'no mutation performed' "$work/wrong_phrase/stderr"
check "execute: stage directory is removed after a refused run" [ -z "$(stage_dirs_of wrong_phrase)" ]

reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one"
feed wrong_phrase_2 "CONFIRM PIXEL 9A FLASH BOOTLOADER AND RADIO (WIPES ALL USER DATA)" "CONFIRM PIXEL 9A FASTBOOT UPDATE (KEEP USER DATA)"
run_case wrong_phrase_2 --confirm-restore
check "execute: wrong stage-2 phrase exits 1" [ "$status" -eq 1 ]
expected_stage1=$(printf 'fastboot\tflash\tbootloader\tbootloader-tegu-fake.img\nfastboot\treboot-bootloader\nfastboot\tflash\tradio\tradio-g5300t-fake.img\nfastboot\treboot-bootloader')
check "execute: wrong stage-2 phrase ran only the bootloader/radio stage" [ "$(mutations)" = "$expected_stage1" ]

# --- execution: full wipe flow over Fastboot -------------------------------
reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one"
feed full_wipe "CONFIRM PIXEL 9A FLASH BOOTLOADER AND RADIO (WIPES ALL USER DATA)" "CONFIRM PIXEL 9A FASTBOOT UPDATE (WIPES ALL USER DATA)"
run_case full_wipe --confirm-restore
check "execute: full wipe flow exits 0" [ "$status" -eq 0 ]
expected_wipe=$(printf '%s\nfastboot\t-w\tupdate\timage-tegu-fake.zip' "$expected_stage1")
check "execute: exact vendor mutation sequence with -w" [ "$(mutations)" = "$expected_wipe" ]
check "execute: mutation lines are printed" \
  sh -c "grep -q '^mutation=fastboot flash bootloader bootloader-tegu-fake.img$' '$work/full_wipe/stdout' && grep -q '^mutation=fastboot reboot-bootloader$' '$work/full_wipe/stdout' && grep -q '^mutation=fastboot flash radio radio-g5300t-fake.img$' '$work/full_wipe/stdout' && grep -q '^mutation=fastboot -w update image-tegu-fake.zip$' '$work/full_wipe/stdout'"
check "execute: staged member hashes are printed" \
  sh -c "grep -q '^staged_bootloader_sha256=[0-9a-f]\{64\}$' '$work/full_wipe/stdout' && grep -q '^staged_radio_sha256=[0-9a-f]\{64\}$' '$work/full_wipe/stdout' && grep -q '^staged_image_sha256=[0-9a-f]\{64\}$' '$work/full_wipe/stdout'"
check "execute: staged android-info matched the inventory" grep -q '^staged_android_info=matches-inventory$' "$work/full_wipe/stdout"
check "execute: bootloader version was re-read after the flash" grep -q "^bootloader_version_after=$required_bootloader$" "$work/full_wipe/stdout"
check "execute: result is factory-restore-complete" grep -q '^result=factory-restore-complete$' "$work/full_wipe/stdout"
check "execute: stage directory is removed on exit" [ -z "$(stage_dirs_of full_wipe)" ]
transcript=$(transcript_of full_wipe)
check "privacy: execution transcript exists" [ -n "$transcript" ]
check "privacy: execution transcript redacts the serial" env S="$serial" T="$transcript" sh -c '! grep -Fq "$S" "$T"'
check "privacy: execution transcript records the update mutation" grep -q '^mutation=fastboot -w update image-tegu-fake.zip$' "$transcript"
check "privacy: execution transcript records confirmations without the phrase" grep -q '^confirmation=accepted$' "$transcript"

# --- execution: --keep-data omits -w ---------------------------------------
reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one"
feed keep_data "CONFIRM PIXEL 9A FLASH BOOTLOADER AND RADIO (KEEP USER DATA)" "CONFIRM PIXEL 9A FASTBOOT UPDATE (KEEP USER DATA)"
run_case keep_data --confirm-restore --keep-data
check "execute: --keep-data flow exits 0" [ "$status" -eq 0 ]
expected_keep=$(printf '%s\nfastboot\tupdate\timage-tegu-fake.zip' "$expected_stage1")
check "execute: --keep-data omits -w from fastboot update" [ "$(mutations)" = "$expected_keep" ]
check "execute: --keep-data prompts name KEEP USER DATA and never a wipe" \
  sh -c "grep -q 'Type exactly: CONFIRM PIXEL 9A FASTBOOT UPDATE (KEEP USER DATA)' '$work/keep_data/stderr' && ! grep -q 'WIPES ALL USER DATA' '$work/keep_data/stderr'"

# --- execution: --from-adb reboots first with the same identity gate --------
reset_stub
export STUB_ADB_DEVICES="$serial device usb:1
" STUB_FASTBOOT_DEVICES_AFTER_REBOOT="$fastboot_one"
feed from_adb "CONFIRM PIXEL 9A REBOOT TO BOOTLOADER FOR FACTORY RESTORE" "CONFIRM PIXEL 9A FLASH BOOTLOADER AND RADIO (WIPES ALL USER DATA)" "CONFIRM PIXEL 9A FASTBOOT UPDATE (WIPES ALL USER DATA)"
run_case from_adb --confirm-restore --from-adb
check "execute: --from-adb flow exits 0" [ "$status" -eq 0 ]
expected_from_adb=$(printf 'adb\treboot\tbootloader\n%s' "$expected_wipe")
check "execute: --from-adb issues adb reboot bootloader first, then the vendor sequence" [ "$(mutations)" = "$expected_from_adb" ]
check "execute: --from-adb verified the Android identity" grep -q '^identity\|^hardware_revision=MP1.0$' "$work/from_adb/stdout"
check "execute: --from-adb re-read the Fastboot identity after the reboot" grep -q '^is_userspace=no$' "$work/from_adb/stdout"

reset_stub
export STUB_ADB_DEVICES="$serial device usb:1
" STUB_REVISION=EVT1.0
feed from_adb_wrong
run_case from_adb_wrong --preflight --from-adb
check "refuse: --from-adb wrong Android hardware revision" [ "$status" -eq 1 ]

# --- execution: update failure is reported honestly ------------------------
reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" STUB_UPDATE_EXIT=1
feed update_fail "CONFIRM PIXEL 9A FLASH BOOTLOADER AND RADIO (WIPES ALL USER DATA)" "CONFIRM PIXEL 9A FASTBOOT UPDATE (WIPES ALL USER DATA)"
run_case update_fail --confirm-restore
check "execute: failed update exits 1" [ "$status" -eq 1 ]
check "execute: failed update reports result=failed" grep -q '^result=failed$' "$work/update_fail/stdout"
check "execute: stage directory is removed after a failed update" [ -z "$(stage_dirs_of update_fail)" ]

bad_calls=$(grep -vE '^adb\tdevices\t-l\t$|^adb\t-s\t[^[:space:]]+\tshell\tgetprop\t[^[:space:]]+\t$|^adb\t-s\t[^[:space:]]+\tget-state\t$|^adb\t-s\t[^[:space:]]+\treboot\tbootloader\t$|^fastboot\tdevices\t-l\t$|^fastboot\t-s\t[^[:space:]]+\tgetvar\t(product|variant|hw-revision|current-slot|slot-count|unlocked|serialno|version-bootloader|version-baseband|is-userspace|secure)\t$|^fastboot\t-s\t[^[:space:]]+\tflash\t(bootloader|radio)\t[^[:space:]]+\t$|^fastboot\t-s\t[^[:space:]]+\treboot-bootloader\t$|^fastboot\t-s\t[^[:space:]]+\t(-w\t)?update\t[^[:space:]]+\t$' "$calls" || true)
check "dynamic: the last run used only allow-listed verbs" [ -z "$bad_calls" ]
[ -z "$bad_calls" ] || printf '%s\n' "$bad_calls"

# Explicit first-test mode resolves the recovery-test bootstrap without
# promoting any target gate or weakening normal restore.
cp -R "$work/target" "$work/target-first-test"
sed -i.bak -e 's/image_compatibility = "passed"/image_compatibility = "partial"/' \
  -e 's/recovery_procedure = "tested"/recovery_procedure = "documented-not-tested"/' \
  "$work/target-first-test/manifest.toml"
first_manifest_hash=$(sha256_of "$work/target-first-test/manifest.toml")
reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" FERRIX_PIXEL9A_TARGET_DIR="$work/target-first-test"
feed first_preflight
run_case first_preflight --preflight-first-test
check 'first test: pinned stock compatibility is independent of Ferrix partial state' [ "$status" -eq 0 ]
check 'first test: preflight is read-only' [ -z "$(mutations)" ]
check 'first test: warns about unproven recovery and firmware changes' grep -q '^recovery_test_limitation=first test changes bootloader/radio firmware' "$work/first_preflight/stdout"
feed normal_still_blocked
run_case normal_still_blocked --confirm-restore
check 'first test: normal restore still refuses unapproved gates' [ "$status" -eq 1 ]
check 'first test: normal refusal performs no mutation' [ -z "$(mutations)" ]

feed first_wrong 'CONFIRM PIXEL 9A FLASH BOOTLOADER AND RADIO (WIPES ALL USER DATA)'
run_case first_wrong --confirm-first-test
check 'first test: existing restore phrase cannot bypass additional acknowledgement' [ "$status" -eq 1 ]
check 'first test: wrong acknowledgement performs no mutation' [ -z "$(mutations)" ]

feed first_restore 'CONFIRM PIXEL 9A FIRST STOCK RECOVERY TEST (WIPES ALL USER DATA)' \
  'CONFIRM PIXEL 9A FLASH BOOTLOADER AND RADIO (WIPES ALL USER DATA)' \
  'CONFIRM PIXEL 9A FASTBOOT UPDATE (WIPES ALL USER DATA)'
run_case first_restore --confirm-first-test
check 'first test: exact extra acknowledgement permits vendor restore sequence' [ "$status" -eq 0 ]
check 'first test: only the existing vendor wipe sequence ran' [ "$(mutations)" = "$expected_wipe" ]
check 'first test: completion still requires Android boot evidence' grep -q '^stock_boot_verification=pending-fresh-same-device-android-discovery$' "$work/first_restore/stdout"
check 'first test: never promotes manifest gates' [ "$(sha256_of "$work/target-first-test/manifest.toml")" = "$first_manifest_hash" ]

reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" FERRIX_PIXEL9A_TARGET_DIR="$work/target-gates-current"
feed first_backup_pending 'CONFIRM PIXEL 9A FIRST STOCK RECOVERY TEST (WIPES ALL USER DATA)'
run_case first_backup_pending --confirm-first-test
check 'first test: unresolved data safety still refuses' [ "$status" -eq 1 ]
check 'first test: pending data safety causes no mutation' [ -z "$(mutations)" ]

reset_stub
export STUB_FASTBOOT_DEVICES="$fastboot_one" FERRIX_PIXEL9A_TARGET_DIR="$work/target-first-test"
feed first_keep_data
run_case first_keep_data --preflight-first-test --keep-data
check 'first test: refuses keep-data variant' [ "$status" -eq 2 ]
check 'first test: invalid mode does not contact phone' [ ! -s "$calls" ]

if [ "$fails" -eq 0 ]; then
  echo '{"event":"pixel-9a-factory-restore-test","status":"passed"}'
else
  printf '{"event":"pixel-9a-factory-restore-test","status":"failed","failures":%s}\n' "$fails"
  exit 1
fi
