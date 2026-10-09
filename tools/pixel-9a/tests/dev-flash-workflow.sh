#!/bin/sh
# Offline proof for the recovery-invariant Pixel 9a development flash route.
# Strict adb/fastboot stubs and a synthetic board package; never contacts a
# real device.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script="$root/tools/pixel-9a/dev-flash.sh"
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

[ -x "$script" ] || { echo 'dev-flash workflow is not executable' >&2; exit 1; }
sh -n "$script"

check "static: no relock/erase/format/oem/unlock verb" \
  sh -c "! grep -nE 'fastboot[^|]*[[:space:]](erase|format|oem|flashing|wipe-super|-w)([[:space:]]|\$)' '$script' | grep -q ."
check "static: bootloader/radio writes use only staged stock images" \
  sh -c "[ \"\$(grep -cE 'flash (bootloader|radio)' '$script')\" -eq 2 ] && grep -q 'flash bootloader \"\$stock_bootloader\"' '$script' && grep -q 'flash radio \"\$stock_radio\"' '$script'"

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-dev-flash-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/bin" "$work/target/recovery" "$work/fw/tegu-syn.000" "$work/images"

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    sha256sum "$1" | awk '{ print $1 }'
  fi
}

printf 'stock bootloader\n' > "$work/fw/tegu-syn.000/bootloader-tegu-tegu-17.0-15819938.img"
printf 'stock radio\n' > "$work/fw/tegu-syn.000/radio-tegu-g5300t-260604-260807-b-16036236.img"
(cd "$work/fw" && zip -q -r "$work/target/recovery/factory.zip" tegu-syn.000)
factory_hash=$(sha256_of "$work/target/recovery/factory.zip")
factory_bytes=$(wc -c < "$work/target/recovery/factory.zip" | tr -d '[:space:]')
printf 'factory\tSYN\t1\tfactory.zip\thttps://example.invalid\t%s\t%s\thttps://example.invalid\n' \
  "$factory_hash" "$factory_bytes" > "$work/target/recovery.pins"

write_manifest() {
  cat > "$work/target/manifest.toml" <<EOF
[gates]
identity = "passed"
image_compatibility = "partial"
data_policy = "$2"
recovery_procedure = "$1"
EOF
}
write_manifest tested dev-device-no-user-data

serial='SYN-DEVFLASH-001'
printf 'ferrix boot\n' > "$work/images/boot.img"
printf 'rootfs\n' > "$work/images/rootfs.img"
head -c 4096 /dev/zero > "$work/images/big.img"

cat > "$work/bin/adb" <<'STUB'
#!/bin/sh
printf 'adb' >> "$STUB_CALLS"; for a in "$@"; do printf '\t%s' "$a" >> "$STUB_CALLS"; done; printf '\n' >> "$STUB_CALLS"
[ "$1" = devices ] || exit 9
printf 'List of devices attached\n%s' "${STUB_ADB_DEVICES:-}"
STUB
cat > "$work/bin/fastboot" <<'STUB'
#!/bin/sh
printf 'fastboot' >> "$STUB_CALLS"; for a in "$@"; do printf '\t%s' "$a" >> "$STUB_CALLS"; done; printf '\n' >> "$STUB_CALLS"
if [ "$1" = devices ]; then printf '%s' "$STUB_FASTBOOT_DEVICES"; exit 0; fi
[ "$1" = -s ] || exit 9
shift 2
slot=$(cat "$STUB_SLOT")
case "$1" in
  getvar)
    case "$2" in
      product) v=tegu ;;
      hw-revision) v=${STUB_REVISION:-MP1.0} ;;
      variant) v=GXQ96 ;;
      unlocked) v=${STUB_UNLOCKED:-yes} ;;
      is-userspace) v=${STUB_USERSPACE:-no} ;;
      current-slot) v=$slot ;;
      version-bootloader) v=${STUB_BOOTLOADER:-tegu-17.0-15819938} ;;
      version-baseband) v=g5300t-260604-260807-B-16036236 ;;
      partition-size:*) v=0x400 ;;
      *) v='' ;;
    esac
    printf '%s: %s\nFinished. Total time: 0.001s\n' "$2" "$v" >&2 ;;
  set_active) printf '%s' "$2" > "$STUB_SLOT" ;;
  fetch) printf 'fetched %s\n' "$2" > "$3" ;;
  flash|boot|--slot=*|reboot|reboot-bootloader) exit "${STUB_MUTATION_EXIT:-0}" ;;
  *) exit 9 ;;
esac
STUB
chmod 0755 "$work/bin/adb" "$work/bin/fastboot"

calls="$work/calls.log"
reset_stub() {
  : > "$calls"
  printf 'a' > "$work/slot"
  unset STUB_ADB_DEVICES STUB_REVISION STUB_UNLOCKED STUB_USERSPACE STUB_BOOTLOADER STUB_MUTATION_EXIT || true
  export STUB_CALLS="$calls" STUB_SLOT="$work/slot" PATH="$work/bin:$PATH"
  export STUB_FASTBOOT_DEVICES="$serial	fastboot
"
  export FERRIX_PIXEL9A_TARGET_DIR="$work/target" FERRIX_PIXEL9A_PINS="$work/target/recovery.pins"
  write_manifest tested dev-device-no-user-data
}

run_case() {
  name=$1
  shift
  mkdir -p "$work/$name"
  export FERRIX_OUT_DIR="$work/$name/out"
  status=0
  sh "$script" "$@" > "$work/$name/stdout" 2> "$work/$name/stderr" < /dev/null || status=$?
}

mutations() {
  awk -F '\t' '$1 == "fastboot" && $4 != "getvar" && $2 != "devices" {
    out = ""
    for (i = 4; i <= NF; i++) { f = $i; sub(".*/", "", f); out = out (out == "" ? "" : " ") f }
    print out
  }' "$calls"
}

token_of() {
  sed -n 's/^confirm_token=//p' "$work/$1/stdout"
}

refuses() {
  [ "$status" -eq 1 ] && grep -q "$2" "$work/$1/stderr" && [ -z "$(mutations)" ]
}

# --- preflight and plan ------------------------------------------------------
reset_stub
run_case preflight preflight
check "preflight: passes read-only" sh -c "[ $status -eq 0 ] && [ -z \"$(mutations)\" ]"
check "preflight: prints serial, slot, and recovery artifact hash" \
  sh -c "grep -q '^serial=$serial$' '$work/preflight/stdout' && grep -q '^current_slot=a$' '$work/preflight/stdout' && grep -q '^recovery_artifact_sha256=$factory_hash$' '$work/preflight/stdout'"

reset_stub
run_case plan flash boot="$work/images/boot.img"
check "plan: no --confirm performs no mutation" sh -c "[ $status -eq 0 ] && [ -z \"$(mutations)\" ] && grep -q '^result=plan-only-no-mutation$' '$work/plan/stdout'"
check "plan: unsuffixed partition resolves to the current slot with image hash" \
  grep -q "^plan_step=flash boot_a .*sha256=$(sha256_of "$work/images/boot.img")$" "$work/plan/stdout"
token=$(token_of plan)
check "plan: prints a bound token" sh -c "printf '%s' '$token' | grep -qE '^DEVFLASH-[0-9a-f]{16}$'"

reset_stub
run_case flash flash --confirm "$token" boot="$work/images/boot.img"
check "flash: matching token flashes exactly the planned partition" \
  sh -c "[ $status -eq 0 ] && [ \"\$(printf '%s\n' \"$(mutations)\")\" = 'flash boot_a boot.img' ]"
transcript=$(sed -n 's/^transcript=//p' "$work/flash/stdout")
check "privacy: transcript exists and redacts the serial" sh -c "[ -f '$transcript' ] && ! grep -Fq '$serial' '$transcript'"

reset_stub
run_case wrong_token flash --confirm DEVFLASH-0000000000000000 boot="$work/images/boot.img"
check "refuse: wrong token" refuses wrong_token 'confirm token does not match'

reset_stub
printf 'changed\n' > "$work/images/boot.img"
run_case stale_token flash --confirm "$token" boot="$work/images/boot.img"
check "refuse: token from a different image" refuses stale_token 'confirm token does not match'

reset_stub
run_case slot_b flash boot_b="$work/images/boot.img" userdata="$work/images/rootfs.img"
token=$(token_of slot_b)
run_case slot_b_run flash --confirm "$token" boot_b="$work/images/boot.img" userdata="$work/images/rootfs.img"
check "flash: explicit slot and userdata" \
  sh -c "[ $status -eq 0 ] && [ \"\$(printf '%s\n' \"$(mutations)\" | tr '\n' ';')\" = 'flash boot_b boot.img;flash userdata rootfs.img;' ]"

# --- partition allowlist -----------------------------------------------------
for part in bootloader radio abl modem pvmfw persist efs devinfo frp super system vendor_dlkm random; do
  reset_stub
  run_case "part_$part" flash "$part=$work/images/boot.img"
  check "refuse: partition $part" refuses "part_$part" "partition '$part'"
done

reset_stub
write_manifest tested ''
run_case no_data_policy flash userdata="$work/images/rootfs.img"
check "refuse: userdata without dev-device data policy" refuses no_data_policy 'dev-device-no-user-data'

reset_stub
run_case too_big flash boot="$work/images/big.img"
check "refuse: image larger than partition" refuses too_big 'exceeds partition'

# --- recovery invariant and identity ----------------------------------------
reset_stub
write_manifest documented-not-tested dev-device-no-user-data
run_case untested flash boot="$work/images/boot.img"
check "refuse: untested stock recovery" refuses untested 'stock recovery is not tested'

reset_stub
cp "$work/target/recovery.pins" "$work/pins.bak"
sed "s/$factory_hash/$(printf '0%.0s' $(seq 1 64))/" "$work/pins.bak" > "$work/target/recovery.pins"
run_case bad_factory preflight
cp "$work/pins.bak" "$work/target/recovery.pins"
check "refuse: factory archive hash mismatch" refuses bad_factory 'SHA-256 differs'

reset_stub
export STUB_UNLOCKED=no
run_case locked preflight
check "refuse: locked bootloader" refuses locked "unlocked='no'"

reset_stub
export STUB_USERSPACE=yes
run_case fastbootd preflight
check "refuse: fastbootd" refuses fastbootd 'fastbootd'

reset_stub
export STUB_REVISION=EVT1.0
run_case revision preflight
check "refuse: wrong hardware revision" refuses revision 'hardware revision'

reset_stub
export STUB_FASTBOOT_DEVICES="$serial	fastboot
OTHER	fastboot
"
run_case ambiguous preflight
check "refuse: two Fastboot devices" refuses ambiguous 'exactly one Fastboot device'

reset_stub
export STUB_ADB_DEVICES="OTHER	device
"
run_case mixed preflight
check "refuse: ADB device also present" refuses mixed 'no ADB device'

# --- temporary boot ----------------------------------------------------------
reset_stub
run_case boot_plan boot "$work/images/boot.img"
token=$(token_of boot_plan)
run_case boot_run boot --confirm "$token" "$work/images/boot.img"
check "boot: temporary boot issues only fastboot boot" \
  sh -c "[ $status -eq 0 ] && [ \"\$(printf '%s\n' \"$(mutations)\")\" = 'boot boot.img' ]"

# --- slot switch -------------------------------------------------------------
reset_stub
run_case active_plan set-active b
token=$(token_of active_plan)
check "set-active: plan is read-only" sh -c "[ $status -eq 0 ] && [ -z \"$(mutations)\" ]"
run_case active_run set-active --confirm "$token" b
check "set-active: stock bootloader and radio reach the target slot before the switch" \
  sh -c "[ $status -eq 0 ] && [ \"\$(printf '%s\n' \"$(mutations)\" | tr '\n' ';')\" = '--slot=b flash bootloader bootloader-tegu-tegu-17.0-15819938.img;--slot=b flash radio radio-tegu-g5300t-260604-260807-b-16036236.img;set_active b;' ]"
check "set-active: verifies the selected slot" grep -q '^current_slot_after=b$' "$work/active_run/stdout"
check "set-active: staged firmware is removed" sh -c "! find '$work/active_run/out' -name 'stage-*' | grep -q ."

reset_stub
export STUB_BOOTLOADER=tegu-16.0-12754673
run_case active_old set-active b
check "refuse: slot switch from non-pinned bootloader" refuses active_old 'not pinned stock'

reset_stub
run_case active_same set-active a
check "refuse: slot switch to the current slot" refuses active_same 'already current'

# --- read-only fetch ---------------------------------------------------------
reset_stub
run_case fetch_ok fetch vendor_boot_b "$work/fetch_ok/out/pixel-9a/dev-flash/vendor_boot_b.img"
check "fetch: allowlisted partition is copied read-only without a token" \
  sh -c "[ $status -eq 0 ] && grep -q '^result=fetched' '$work/fetch_ok/stdout' && grep -q 'fetched vendor_boot_b' '$work/fetch_ok/out/pixel-9a/dev-flash/vendor_boot_b.img' && ! grep -q confirm_token '$work/fetch_ok/stdout'"

reset_stub
run_case fetch_deny fetch persist "$work/fetch_deny/out/pixel-9a/dev-flash/persist.img"
check "fetch: refuses partitions outside the allowlist" \
  sh -c "[ $status -eq 1 ] && ! grep -q 'fetch' '$calls'"

reset_stub
run_case fetch_outside fetch vendor_boot_b "$work/elsewhere.img"
check "fetch: refuses output outside the ignored output directory" \
  sh -c "[ $status -eq 1 ] && [ ! -e '$work/elsewhere.img' ]"

# --- failure handling --------------------------------------------------------
reset_stub
run_case fail_plan flash boot="$work/images/boot.img"
token=$(token_of fail_plan)
export STUB_MUTATION_EXIT=1
run_case fail_run flash --confirm "$token" boot="$work/images/boot.img"
check "flash: fastboot failure exits 1 and names the restore" \
  sh -c "[ $status -eq 1 ] && grep -q 'pixel-9a-factory-restore' '$work/fail_run/stderr'"

if [ "$fails" -ne 0 ]; then
  printf '{"event":"pixel-9a-dev-flash-test","status":"failed","failures":%s}\n' "$fails"
  exit 1
fi
printf '{"event":"pixel-9a-dev-flash-test","status":"passed"}\n'
