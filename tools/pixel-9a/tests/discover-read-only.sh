#!/bin/sh
# Offline proof for the Pixel 9a discovery workflow. Platform Tools are
# replaced with strict stubs, so this test never enumerates or contacts a real
# device. The stubs reject every verb except devices, shell getprop, and
# getvar, and their call log is checked again after all discovery branches.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
tab=$(printf '\t')
discover="$root/tools/pixel-9a/discover.sh"
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
  | grep -vE 'adb_listing=\$\(adb devices -l\)' \
  | grep -vE 'fastboot_listing=\$\(fastboot devices -l\)' \
  | grep -vE 'raw=\$\(adb -s "\$serial" shell getprop "\$prop" ' \
  | grep -vE 'raw=\$\(fastboot -s "\$serial" getvar "\$var" ' \
  || true)
check "static: discovery contains only allow-listed device verbs" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-discovery-test.XXXXXX")
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
    if [ "${STUB_ADB_GETPROP_FAIL:-}" = "$5" ]; then
      echo 'error: closed'
      exit 1
    fi
    if [ "${STUB_ADB_GETPROP_TEXT_ERROR:-}" = "$5" ]; then
      echo 'error: closed'
      exit 0
    fi
    case "$5" in
      ro.product.manufacturer) echo Google ;;
      ro.product.name|ro.product.device|ro.product.vendor.device|ro.product.board) echo tegu ;;
      ro.product.model|ro.product.vendor.model) echo "Pixel 9a" ;;
      ro.hardware) echo tegu ;;
      ro.hardware.chipname) echo tensor-synthetic ;;
      ro.boot.hardware.revision) echo MP1.0 ;;
      ro.boot.hardware.sku) echo GXQ96 ;;
      ro.boot.bootloader) echo tegu-test ;;
      ro.boot.slot_suffix) echo _a ;;
      ro.boot.slot_count) echo 2 ;;
      ro.boot.verifiedbootstate) echo green ;;
      ro.boot.vbmeta.device_state) echo locked ;;
      ro.boot.flash.locked) echo 1 ;;
      ro.boot.dynamic_partitions) echo true ;;
      ro.boot.boot_devices) echo synthetic.ufs ;;
      ro.build.fingerprint) echo google/tegu/synthetic ;;
      ro.build.version.release) echo 15 ;;
      ro.build.version.security_patch|ro.vendor.build.version.security_patch) echo 2026-01-01 ;;
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
    if [ "${STUB_FASTBOOT_GETVAR_FAIL:-}" = "$4" ]; then
      echo 'FAILED (remote: query failed)' >&2
      exit 1
    fi
    if [ "${STUB_FASTBOOT_GETVAR_ABSENT:-}" = "$4" ]; then
      echo 'Finished. Total time: 0.001s' >&2
      exit 0
    fi
    case "$4" in
      product) value=tegu ;;
      variant) value=GXQ96 ;;
      hw-revision) value=MP1.0 ;;
      current-slot) value=a ;;
      slot-count) value=2 ;;
      unlocked) value=yes ;;
      secure) value=yes ;;
      is-userspace) value=no ;;
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

special_serial='SYN.00[1]/&$'
export STUB_ADB_DEVICES="$special_serial device usb:1 product:tegu
" STUB_FASTBOOT_DEVICES=""
run_discovery adb_special_serial
check "dynamic: one authorized ADB device succeeds" [ "$status" -eq 0 ]
adb_transcript=$(find "$work/adb_special_serial/pixel-9a" -name 'discovery-*.txt' -type f | head -1)
check "privacy: operator stdout retains exact serial" grep -Fq "serial=$special_serial" "$work/adb_special_serial/stdout"
check "privacy: regex-special serial is absent from transcript" env \
  SPECIAL_SERIAL="$special_serial" TRANSCRIPT="$adb_transcript" \
  sh -c '! grep -Fq "$SPECIAL_SERIAL" "$TRANSCRIPT"'
check "privacy: transcript contains hashed marker" grep -q '^serial=serial-sha256:[0-9a-f][0-9a-f]*$' "$adb_transcript"
check "dynamic: ADB properties were queried" grep -q 'shell.*getprop.*ro.build.fingerprint' "$calls"

export STUB_ADB_GETPROP_FAIL=ro.product.model
run_discovery adb_shell_closed
check "dynamic: failed ADB shell is refused" [ "$status" -eq 5 ]
check "dynamic: failed ADB shell has no ok marker" sh -c '! grep -q "^result=ok$" "$1"' sh "$work/adb_shell_closed/stdout"
check "dynamic: failed ADB query is recorded" grep -q '^result=identity-query-failed$' "$work/adb_shell_closed/stdout"
unset STUB_ADB_GETPROP_FAIL

export STUB_ADB_GETPROP_TEXT_ERROR=ro.product.model
run_discovery adb_text_error
check "dynamic: error text from ADB is refused even on exit zero" [ "$status" -eq 5 ]
check "dynamic: error text has no ok marker" sh -c '! grep -q "^result=ok$" "$1"' sh "$work/adb_text_error/stdout"
unset STUB_ADB_GETPROP_TEXT_ERROR

export STUB_ADB_DEVICES="" STUB_FASTBOOT_DEVICES="$special_serial fastboot
"
run_discovery fastboot_special_serial
check "dynamic: one Fastboot device succeeds" [ "$status" -eq 0 ]
fastboot_transcript=$(find "$work/fastboot_special_serial/pixel-9a" -name 'discovery-*.txt' -type f | head -1)
check "privacy: Fastboot transcript redacts regex-special serial" env \
  SPECIAL_SERIAL="$special_serial" TRANSCRIPT="$fastboot_transcript" \
  sh -c '! grep -Fq "$SPECIAL_SERIAL" "$TRANSCRIPT"'
check "dynamic: Fastboot identity variables were queried" grep -q 'getvar.*hw-revision' "$calls"

export STUB_FASTBOOT_GETVAR_FAIL=hw-revision
run_discovery fastboot_query_failed
check "dynamic: failed Fastboot getvar is refused" [ "$status" -eq 5 ]
check "dynamic: failed Fastboot getvar has no ok marker" sh -c '! grep -q "^result=ok$" "$1"' sh "$work/fastboot_query_failed/stdout"
unset STUB_FASTBOOT_GETVAR_FAIL

export STUB_FASTBOOT_GETVAR_ABSENT=hw-revision
run_discovery fastboot_query_absent
check "dynamic: absent Fastboot value is refused" [ "$status" -eq 5 ]
unset STUB_FASTBOOT_GETVAR_ABSENT

bad_calls=$(grep -vE '^adb'"$tab"'devices'"$tab"'-l'"$tab"'$|^adb'"$tab"'-s'"$tab"'[^[:space:]]+'"$tab"'shell'"$tab"'getprop'"$tab"'ro\.[^[:space:]]+'"$tab"'$|^fastboot'"$tab"'devices'"$tab"'-l'"$tab"'$|^fastboot'"$tab"'-s'"$tab"'[^[:space:]]+'"$tab"'getvar'"$tab"'(product|variant|hw-revision|current-slot|slot-count|unlocked|secure|is-userspace)'"$tab"'$' "$calls" || true)
check "dynamic: every stubbed device call was read-only" [ -z "$bad_calls" ]
[ -z "$bad_calls" ] || printf '%s\n' "$bad_calls"

if [ "$fails" -eq 0 ]; then
  echo '{"event":"pixel-9a-tools-test","status":"passed"}'
else
  printf '{"event":"pixel-9a-tools-test","status":"failed","failures":%s}\n' "$fails"
  exit 1
fi
