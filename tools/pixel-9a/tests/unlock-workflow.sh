#!/bin/sh
# Offline proof for the Pixel 9a bootloader-unlock workflow. The test runs a
# generated copy of the target-scoped script against a synthetic board package
# and strict adb/fastboot stubs; it never contacts a real device.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
tab=$(printf '\t')
script="$root/tools/pixel-9a/unlock-bootloader.sh"
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

[ -x "$script" ] || { echo 'unlock workflow is not executable' >&2; exit 1; }
sh -n "$script"

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-unlock-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/bin" "$work/target/recovery" "$work/state"

# Generate a test copy only to redirect the script's fixed board path to this
# private fixture. The checked-in workflow remains fixed to the exact Pixel 9a
# board target and is statically inspected below.
sed -e "s|^root=.*|root=\"$root\"|" \
  -e "s|target=\"\$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0\"|target=\"$work/target\"|" \
  "$script" > "$work/unlock.sh"
chmod 0755 "$work/unlock.sh"

check "static: checked-in unlock script is POSIX shell" sh -n "$script"
check "static: unlock script has no erase/format/slot/relock verb" \
  sh -c "! grep -nE 'fastboot .* (erase|format|set_active|oem|flashing lock|relock|wipe-super)' '$script' | grep -q ."
check "static: unlock script announces the state-changing verbs" \
  sh -c "grep -q 'mutation=adb reboot bootloader' '$script' && grep -q 'mutation=fastboot flashing unlock' '$script'"

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    sha256sum "$1" | awk '{ print $1 }'
  fi
}

printf 'factory fixture\n' > "$work/target/recovery/factory.zip"
printf 'ota fixture\n' > "$work/target/recovery/ota.zip"
factory_hash=$(sha256_of "$work/target/recovery/factory.zip")
factory_bytes=$(wc -c < "$work/target/recovery/factory.zip" | tr -d '[:space:]')
ota_hash=$(sha256_of "$work/target/recovery/ota.zip")
ota_bytes=$(wc -c < "$work/target/recovery/ota.zip" | tr -d '[:space:]')
{
  printf '# kind\tbuild\tversion\tfilename\turl\tsha256\tbytes\tsource-page\n'
  printf 'factory\tSYN\t1\tfactory.zip\thttps://example.invalid/factory\t%s\t%s\thttps://example.invalid\n' "$factory_hash" "$factory_bytes"
  printf 'ota\tSYN\t1\tota.zip\thttps://example.invalid/ota\t%s\t%s\thttps://example.invalid\n' "$ota_hash" "$ota_bytes"
} > "$work/target/recovery.pins"

write_manifest() {
  cat > "$work/target/manifest.toml" <<EOF
[gates]
identity = "$1"
image_compatibility = "$2"
backup = "$3"
recovery_procedure = "$4"
per_run_confirmation = "$5"
EOF
}
write_manifest passed passed verified tested required

calls="$work/calls.log"
: > "$calls"
serial='SYN-UNLOCK-001'
cat > "$work/bin/adb" <<'STUB'
#!/bin/sh
printf 'adb\t' >> "$STUB_CALLS"
printf '%s\t' "$@" >> "$STUB_CALLS"
printf '\n' >> "$STUB_CALLS"
case "$1" in
  devices)
    [ "$#" -eq 2 ] && [ "$2" = -l ] || exit 99
    echo 'List of devices attached'
    printf '%s device usb:1\n' "$STUB_SERIAL"
    ;;
  -s)
    [ "$#" -ge 3 ] || exit 99
    case "$3" in
      shell)
        [ "$#" -eq 5 ] && [ "$4" = getprop ] || exit 99
        case "$5" in
          ro.product.manufacturer) echo Google ;;
          ro.product.model) echo 'Pixel 9a' ;;
          ro.product.name|ro.product.device|ro.product.board) echo tegu ;;
          ro.boot.hardware.revision) echo MP1.0 ;;
          ro.boot.hardware.sku) echo GXQ96 ;;
          ro.boot.bootloader) echo tegu-16.0-synthetic ;;
          ro.boot.slot_suffix) echo _a ;;
          ro.boot.slot_count) echo 2 ;;
          ro.boot.verifiedbootstate) echo green ;;
          ro.boot.vbmeta.device_state) echo locked ;;
          ro.boot.flash.locked) echo 1 ;;
          ro.boot.dynamic_partitions) echo true ;;
          ro.boot.boot_devices) echo synthetic.ufs ;;
          *) echo "stub: unexpected getprop $5" >&2; exit 99 ;;
        esac
        ;;
      get-state)
        [ "$#" -eq 3 ] || exit 99
        echo device
        ;;
      reboot)
        [ "$#" -eq 4 ] && [ "$4" = bootloader ] || exit 99
        : > "$STUB_STATE/rebooted"
        ;;
      *) echo "stub: unexpected adb verb $3" >&2; exit 99 ;;
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
    [ -f "$STUB_STATE/rebooted" ] && printf '%s fastboot usb:1\n' "$STUB_SERIAL"
    ;;
  -s)
    [ "$#" -ge 3 ] || exit 99
    [ "$2" = "$STUB_SERIAL" ] || exit 99
    case "$3" in
      getvar)
        [ "$#" -eq 4 ] || exit 99
        case "$4" in
          product) value=tegu ;;
          variant) value=${STUB_VARIANT:-N/A} ;;
          hw-revision) value=MP1.0 ;;
          current-slot) value=a ;;
          unlocked) if [ -f "$STUB_STATE/unlocked" ]; then value=yes; else value=no; fi ;;
          serialno) value=$STUB_SERIAL ;;
          version-bootloader) value=tegu-16.0-synthetic ;;
          *) echo "stub: unexpected getvar $4" >&2; exit 99 ;;
        esac
        printf '%s: %s\nFinished. Total time: 0.001s\n' "$4" "$value" >&2
        ;;
      flashing)
        [ "$#" -eq 4 ] && [ "$4" = unlock ] || exit 99
        : > "$STUB_STATE/unlocked"
        ;;
      *) echo "stub: unexpected fastboot verb $3" >&2; exit 99 ;;
    esac
    ;;
  *) echo "stub: unexpected fastboot verb $1" >&2; exit 99 ;;
esac
STUB
chmod 0755 "$work/bin/adb" "$work/bin/fastboot"
export PATH="$work/bin:$PATH" STUB_CALLS="$calls" STUB_STATE="$work/state" STUB_SERIAL="$serial"

mutations() {
  grep -E '^adb'"$tab"'-s'"$tab"'[^[:space:]]+'"$tab"'reboot'"$tab"'|^fastboot'"$tab"'-s'"$tab"'[^[:space:]]+'"$tab"'flashing'"$tab"'unlock'"$tab"'' "$calls" || true
}

run_case() {
  case_name=$1
  shift
  mkdir -p "$work/$case_name"
  [ -f "$work/$case_name/stdin" ] || : > "$work/$case_name/stdin"
  : > "$calls"
  rm -f "$work/state/rebooted" "$work/state/unlocked"
  export FERRIX_OUT_DIR="$work/$case_name"
  status=0
  sh "$work/unlock.sh" "$@" > "$work/$case_name/stdout" 2> "$work/$case_name/stderr" < "$work/$case_name/stdin" || status=$?
}

# The admitted target's current state is intentionally not ready for any
# unlock mutation. Gate refusal must happen after read-only identity checks but
# before the first confirmation and before adb reboot bootloader.
write_manifest passed partial pending documented-not-tested required
run_case pending --confirm-unlock
check "gates: pending/partial/documented-not-tested unlock refuses before confirmation" \
  sh -c "[ $status -eq 1 ] && grep -q \"^REFUSING: backup gate is 'pending'\" '$work/pending/stderr'"
check "gates: pending unlock issues zero mutations" [ -z "$(mutations)" ]

# A fully approved synthetic fixture exercises both typed confirmations and
# the final identity/hash recheck immediately before fastboot flashing unlock.
write_manifest passed passed verified tested required
mkdir -p "$work/approved"
cat > "$work/approved/stdin" <<'EOF'
CONFIRM PIXEL 9A REBOOT AND UNLOCK (WIPES ALL USER DATA)
CONFIRM PIXEL 9A BOOTLOADER UNLOCK (WIPES ALL USER DATA)
EOF
run_case approved --confirm-unlock
check "execute: approved unlock flow succeeds" [ "$status" -eq 0 ]
check "execute: adb reboot precedes fastboot unlock" sh -c "grep -q '^adb${tab}-s${tab}$serial${tab}reboot${tab}bootloader${tab}$' '$calls' && grep -q '^fastboot${tab}-s${tab}$serial${tab}flashing${tab}unlock${tab}$' '$calls'"
check "execute: result records bootloader unlock" grep -q '^result=bootloader-unlocked$' "$work/approved/stdout"
check "execute: mutation output names partition and recovery hashes" \
  sh -c "grep -q '^partition_target=bootloader (fastboot flashing unlock)$' '$work/approved/stdout' && grep -q '^factory_recovery_sha256=$factory_hash$' '$work/approved/stdout' && grep -q '^ota_recovery_sha256=$ota_hash$' '$work/approved/stdout' && grep -q '^sku_source=adb-correlated-live$' '$work/approved/stdout'"

export STUB_VARIANT=GB62
mkdir -p "$work/wrong_variant"
cat > "$work/wrong_variant/stdin" <<'EOF'
CONFIRM PIXEL 9A REBOOT AND UNLOCK (WIPES ALL USER DATA)
EOF
run_case wrong_variant --confirm-unlock
check "refuses: Fastboot SKU/variant mismatch after ADB reboot" \
  sh -c "[ $status -eq 1 ] && grep -q 'REFUSING: Fastboot SKU/variant mismatch: GB62' '$work/wrong_variant/stderr'"
check "refuses: SKU mismatch issues no unlock mutation" \
  sh -c "! grep -q '^fastboot.*flashing.*unlock' '$calls'"
unset STUB_VARIANT

if [ "$fails" -eq 0 ]; then
  echo '{"event":"pixel-9a-unlock-test","status":"passed"}'
else
  printf '{"event":"pixel-9a-unlock-test","status":"failed","failures":%s}\n' "$fails"
  exit 1
fi
