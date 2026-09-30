#!/bin/sh
# Offline proof for the generic device discovery workflow. Platform Tools are
# replaced with strict stubs, so this test never enumerates or contacts a real
# device. The stubs reject every verb except devices, shell getprop, and
# getvar, and their call log is checked again after all discovery branches.
# The fixture is a synthetic non-Google vendor to prove vendor independence.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
discover="$root/tools/device/discover.sh"
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

# Statically require every executable adb/fastboot reference to match the
# discovery allow-list. This catches a new branch that the dynamic fixtures do
# not happen to exercise.
offending=$(grep -nE '^[[:space:]]*[^#]*(adb|fastboot)([[:space:]]|$)' "$discover" \
  | grep -vE 'need (adb|fastboot)$' \
  | grep -vE 'emit transport (adb|fastboot)$' \
  | grep -vE 'identity_set transport (adb|fastboot)$' \
  | grep -vE 'adb_rows=\$\(adb devices -l ' \
  | grep -vE 'fastboot_rows=\$\(fastboot devices -l ' \
  | grep -vE 'value=\$\(adb -s "\$serial" shell getprop "\$prop" ' \
  | grep -vE 'value=\$\(fastboot -s "\$serial" getvar "\$var" ' \
  || true)
check "static: discovery contains only allow-listed device verbs" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
check "static: discovery never mentions a mutating verb" sh -c "! grep -nE '[[:space:]](reboot|flashing|flash|erase|format|sideload|set_active|oem|update)[[:space:]]' '$discover' | grep -vE '^[0-9]+:[[:space:]]*#'"

work=$(mktemp -d "${TMPDIR:-/tmp}/device-discovery-test.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
calls="$work/calls.log"
: > "$calls"

cat > "$work/bin/adb" <<'STUB'
#!/bin/sh
printf 'adb\t' >> "$STUB_CALLS"
printf '%s\t' "$@" >> "$STUB_CALLS"
printf '\n' >> "$STUB_CALLS"
case "$1" in
  devices)
    [ "$#" -eq 2 ] && [ "$2" = -l ] || exit 99
    echo "List of devices attached"
    printf '%s' "${STUB_ADB_DEVICES:-}"
    ;;
  -s)
    [ "$#" -eq 5 ] && [ "$3" = shell ] && [ "$4" = getprop ] || exit 99
    case "$5" in
      ro.product.manufacturer) echo OnePlus ;;
      ro.product.brand) echo OnePlus ;;
      ro.product.model) echo "SYN2001" ;;
      ro.product.name) echo synthetic-op-eea ;;
      ro.product.device) echo synthetic-op ;;
      ro.product.board) echo synthboard ;;
      ro.hardware) echo qcom ;;
      ro.hardware.chipname) echo synthetic-soc ;;
      ro.boot.hardware.revision) echo "${STUB_REVISION:-EVT2}" ;;
      ro.boot.hardware.sku) echo "SKU'7" ;;
      ro.boot.hardware) echo qcom ;;
      ro.boot.bootloader) echo synthetic-bl-1 ;;
      ro.boot.slot_suffix) echo _b ;;
      ro.boot.slot_count) echo 2 ;;
      ro.boot.verifiedbootstate) echo green ;;
      ro.boot.vbmeta.device_state) echo locked ;;
      ro.boot.flash.locked) echo 1 ;;
      ro.boot.dynamic_partitions) echo true ;;
      ro.boot.boot_devices) echo synthetic.ufs ;;
      ro.build.fingerprint) echo "OnePlus/synthetic-op/synthetic-op:14/SYN.001/1:user/release-keys" ;;
      ro.build.version.release) echo 14 ;;
      ro.build.version.sdk) echo 34 ;;
      ro.build.version.security_patch|ro.vendor.build.version.security_patch) echo 2026-01-01 ;;
      ro.soc.manufacturer) echo Qualcomm ;;
      ro.soc.model) echo SYN8000 ;;
      ro.oem_unlock_supported) echo 1 ;;
      sys.oem_unlock_allowed) echo 0 ;;
      *) echo "stub: unexpected getprop $5" >&2; exit 99 ;;
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
  -s)
    [ "$#" -eq 4 ] && [ "$3" = getvar ] || exit 99
    case "$4" in
      product) value=synthetic-op ;;
      variant) value=SKU7 ;;
      hw-revision) value=EVT2 ;;
      current-slot) value=b ;;
      slot-count) value=2 ;;
      unlocked) value=no ;;
      secure) value=yes ;;
      is-userspace) value=no ;;
      version-bootloader) value=synthetic-bl-1 ;;
      version-baseband) value=synthetic-bb-1 ;;
      serialno) value=$2 ;;
      *) echo "stub: unexpected getvar $4" >&2; exit 99 ;;
    esac
    printf '%s: %s\n' "$4" "$value" >&2
    ;;
  *) echo "stub: unexpected fastboot verb $1" >&2; exit 99 ;;
esac
STUB
chmod 0755 "$work/bin/adb" "$work/bin/fastboot"
export PATH="$work/bin:$PATH" STUB_CALLS="$calls"

run_discovery() {
  case_name=$1
  mkdir -p "$work/$case_name"
  export FERRIX_OUT_DIR="$work/$case_name"
  status=0
  sh "$discover" > "$work/$case_name/stdout" 2> "$work/$case_name/stderr" || status=$?
}

export STUB_ADB_DEVICES="" STUB_FASTBOOT_DEVICES=""
run_discovery no_device
check "dynamic: zero devices are refused" [ "$status" -eq 2 ]
check "dynamic: zero-device result is recorded" grep -q '^result=no-device$' "$work/no_device/stdout"
check "dynamic: zero devices leave no identity file" [ ! -e "$work/no_device/device/identity-latest.env" ]

export STUB_ADB_DEVICES="SERIAL-A device usb:1
SERIAL-B device usb:2
" STUB_FASTBOOT_DEVICES=""
run_discovery two_adb
check "dynamic: two ADB devices are refused as ambiguous" [ "$status" -eq 3 ]
check "dynamic: ambiguous discovery does not query properties" sh -c "! grep -q 'shell.*getprop' '$calls'"

export STUB_ADB_DEVICES="SERIAL-A device usb:1
" STUB_FASTBOOT_DEVICES="SERIAL-B fastboot
"
run_discovery two_transports
check "dynamic: simultaneous ADB and Fastboot are refused" [ "$status" -eq 3 ]

export STUB_ADB_DEVICES="SERIAL-U unauthorized usb:1
" STUB_FASTBOOT_DEVICES=""
run_discovery unauthorized
check "dynamic: unauthorized ADB is refused" [ "$status" -eq 4 ]
check "dynamic: unauthorized ADB does not query properties" sh -c "! grep -q 'shell.*getprop' '$calls'"
check "dynamic: unauthorized ADB leaves no identity file" [ ! -e "$work/unauthorized/device/identity-latest.env" ]

special_serial='SYN.00[1]/&$'
export STUB_ADB_DEVICES="$special_serial device usb:1 product:synthetic-op
" STUB_FASTBOOT_DEVICES=""
run_discovery adb_special_serial
check "dynamic: one authorized ADB device succeeds" [ "$status" -eq 0 ]
adb_transcript=$(find "$work/adb_special_serial/device" -name 'discovery-*.txt' -type f | head -1)
adb_identity="$work/adb_special_serial/device/identity-latest.env"
check "privacy: operator stdout retains exact serial" grep -Fq "serial=$special_serial" "$work/adb_special_serial/stdout"
check "privacy: regex-special serial is absent from transcript" env \
  SPECIAL_SERIAL="$special_serial" TRANSCRIPT="$adb_transcript" \
  sh -c '! grep -Fq "$SPECIAL_SERIAL" "$TRANSCRIPT"'
check "privacy: transcript contains hashed marker" grep -q '^serial=serial-sha256:[0-9a-f][0-9a-f]*$' "$adb_transcript"
check "identity: file exists" [ -f "$adb_identity" ]
check "privacy: identity file has no serial key" sh -c "! grep -q '^serial=' '$adb_identity'"
check "privacy: identity file does not contain the serial" env \
  SPECIAL_SERIAL="$special_serial" IDENTITY="$adb_identity" \
  sh -c '! grep -Fq "$SPECIAL_SERIAL" "$IDENTITY"'
check "identity: has serial_sha256_prefix" grep -q "^serial_sha256_prefix='[0-9a-f]*'\$" "$adb_identity"
check "identity: vendor-agnostic manufacturer captured" grep -q "^manufacturer='OnePlus'\$" "$adb_identity"
check "identity: codename from ro.product.device" grep -q "^codename='synthetic-op'\$" "$adb_identity"
check "identity: revision captured" grep -q "^hardware_revision='EVT2'\$" "$adb_identity"
check "identity: bootloader_state normalized from ro.boot.flash.locked" grep -q "^bootloader_state='locked'\$" "$adb_identity"
check "identity: every line is a quoted key=value" sh -c "! grep -vE \"^[a-z][a-z0-9_]*='([^']|'\\\\\\\\'')*'\\$\" '$adb_identity'"
# Sourcing must reproduce the quote-bearing SKU exactly.
check "identity: file is sh-sourceable and quotes are escaped" sh -c ". '$adb_identity' && [ \"\$sku\" = \"SKU'7\" ] && [ \"\$transport\" = adb ]"
for key in transport manufacturer model codename board hardware_revision sku bootloader_version bootloader_state verified_boot current_slot slot_count dynamic_partitions android_release fingerprint; do
  check "identity: key $key present" grep -q "^$key=" "$adb_identity"
done
check "dynamic: ADB properties were queried" grep -q 'shell.*getprop.*ro.build.fingerprint' "$calls"
check "dynamic: generic SoC property was queried" grep -q 'shell.*getprop.*ro.soc.model' "$calls"

export STUB_ADB_DEVICES="" STUB_FASTBOOT_DEVICES="$special_serial fastboot
"
run_discovery fastboot_special_serial
check "dynamic: one Fastboot device succeeds" [ "$status" -eq 0 ]
fastboot_transcript=$(find "$work/fastboot_special_serial/device" -name 'discovery-*.txt' -type f | head -1)
fastboot_identity="$work/fastboot_special_serial/device/identity-latest.env"
check "privacy: Fastboot transcript redacts regex-special serial" env \
  SPECIAL_SERIAL="$special_serial" TRANSCRIPT="$fastboot_transcript" \
  sh -c '! grep -Fq "$SPECIAL_SERIAL" "$TRANSCRIPT"'
check "privacy: Fastboot identity redacts serialno getvar echo" env \
  SPECIAL_SERIAL="$special_serial" IDENTITY="$fastboot_identity" \
  sh -c '! grep -Fq "$SPECIAL_SERIAL" "$IDENTITY"'
check "identity: Fastboot transport recorded" grep -q "^transport='fastboot'\$" "$fastboot_identity"
check "identity: Fastboot codename from product" grep -q "^codename='synthetic-op'\$" "$fastboot_identity"
check "identity: Fastboot locked state normalized" grep -q "^bootloader_state='locked'\$" "$fastboot_identity"
check "identity: Fastboot manufacturer is empty without operator hint" grep -q "^manufacturer=''\$" "$fastboot_identity"
check "identity: Fastboot userspace mode is recorded" grep -q "^fastboot_userspace='no'\$" "$fastboot_identity"
check "dynamic: Fastboot identity variables were queried" grep -q 'getvar.*hw-revision' "$calls"

bad_calls=$(grep -vE '^adb\tdevices\t-l\t$|^adb\t-s\t[^[:space:]]+\tshell\tgetprop\t(ro|sys)\.[^[:space:]]+\t$|^fastboot\tdevices\t-l\t$|^fastboot\t-s\t[^[:space:]]+\tgetvar\t(product|variant|hw-revision|current-slot|slot-count|unlocked|secure|is-userspace|version-bootloader|version-baseband|serialno)\t$' "$calls" || true)
check "dynamic: every stubbed device call was read-only" [ -z "$bad_calls" ]
[ -z "$bad_calls" ] || printf '%s\n' "$bad_calls"

if [ "$fails" -eq 0 ]; then
  echo '{"event":"device-tools-test","suite":"discover","status":"passed"}'
else
  printf '{"event":"device-tools-test","suite":"discover","status":"failed","failures":%s}\n' "$fails"
  exit 1
fi
