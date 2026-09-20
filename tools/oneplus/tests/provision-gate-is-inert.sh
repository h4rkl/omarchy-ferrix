#!/bin/sh
# Proves tools/oneplus/provision-gate.sh and discover.sh are read-only:
#   1. static: every adb/fastboot invocation in both scripts uses a read-only
#      subcommand (devices, shell getprop, getvar);
#   2. dynamic: with stubbed adb/fastboot the gate refuses zero devices,
#      refuses multiple devices, refuses a wrong confirmation, and after the
#      exact confirmation exits 3 with reason no-approved-proposal; the stubs
#      log every call so a mutating verb would show up.
# No device, no Platform Tools, and no network are needed.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
gate="$root/tools/oneplus/provision-gate.sh"
discover="$root/tools/oneplus/discover.sh"
fails=0
check() { # NAME CONDITION...
  name=$1
  shift
  if "$@"; then
    printf 'ok   %s\n' "$name"
  else
    printf 'FAIL %s\n' "$name"
    fails=$((fails + 1))
  fi
}

# --- 1. static ---------------------------------------------------------------
# Every line that runs adb or fastboot (not comments, not the word inside a
# message string) must use one of the allowed subcommands.
for script in "$gate" "$discover"; do
  offending=$(grep -nE '(^|[^a-z_-])(adb|fastboot)([[:space:]]|$)' "$script" \
    | grep -vE '^[0-9]+:[[:space:]]*#' \
    | grep -vE 'command -v|need (adb|fastboot)|"missing|refuse "missing|for tool in|echo|printf|transport' \
    | grep -vE '(adb|fastboot)( -s "\$serial")? (devices|shell getprop|getvar)' || true)
  check "static: only read-only adb/fastboot verbs in $(basename "$script")" [ -z "$offending" ]
  [ -z "$offending" ] || printf '%s\n' "$offending"
  mutating=$(grep -nE 'fastboot[^#]*\b(flash|flashing|erase|format|oem|boot|set_active|reboot|wipe|update|continue)\b' "$script" | grep -vE '^[0-9]+:[[:space:]]*#' | grep -vE 'ro\.boot\.|bootloader_locked|"boot|\[boot\]' || true)
  check "static: no fastboot mutation verb in $(basename "$script")" [ -z "$mutating" ]
  [ -z "$mutating" ] || printf '%s\n' "$mutating"
  adb_mut=$(grep -nE 'adb[^#]*\b(reboot|sideload|root|remount|push|install|disable-verity|unroot)\b' "$script" | grep -vE '^[0-9]+:[[:space:]]*#' || true)
  check "static: no adb mutation verb in $(basename "$script")" [ -z "$adb_mut" ]
  [ -z "$adb_mut" ] || printf '%s\n' "$adb_mut"
done

# --- 2. dynamic, with stubs ----------------------------------------------------
work=$(mktemp -d "${TMPDIR:-/tmp}/oneplus-gate-test.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
calls="$work/calls.log"
: > "$calls"
printf 'synthetic image\n' > "$work/image.img"
printf 'synthetic backup\n' > "$work/backup.img"
# Stub adb: STUB_ADB_DEVICES lines are printed after the header. getprop
# answers from a fixed synthetic table.
cat > "$work/bin/adb" <<'STUB'
#!/bin/sh
printf 'adb %s\n' "$*" >> "$STUB_CALLS"
case "$1" in
  devices)
    echo "List of devices attached"
    printf '%s' "${STUB_ADB_DEVICES:-}"
    ;;
  -s)
    # adb -s SERIAL shell getprop NAME
    [ "$3" = shell ] && [ "$4" = getprop ] || { echo "stub: unexpected adb verb $3" >&2; exit 99; }
    case "$5" in
      ro.product.device) echo "syntheticdev" ;;
      ro.product.model) echo "SYN-0001" ;;
      ro.boot.hardware.revision) echo "rev-a" ;;
      ro.boot.slot_suffix) echo "_a" ;;
      ro.boot.flash.locked) echo "1" ;;
      *) echo "" ;;
    esac
    ;;
  *) echo "stub: unexpected adb verb $1" >&2; exit 99 ;;
esac
STUB
cat > "$work/bin/fastboot" <<'STUB'
#!/bin/sh
printf 'fastboot %s\n' "$*" >> "$STUB_CALLS"
case "$1" in
  devices) printf '%s' "${STUB_FASTBOOT_DEVICES:-}" ;;
  -s) [ "$3" = getvar ] || { echo "stub: unexpected fastboot verb $3" >&2; exit 99; }; echo "$4: stub" >&2 ;;
  *) echo "stub: unexpected fastboot verb $1" >&2; exit 99 ;;
esac
STUB
chmod 0755 "$work/bin/adb" "$work/bin/fastboot"
export PATH="$work/bin:$PATH" STUB_CALLS="$calls"
run_gate() { # stdin text, then args
  printf '%s\n' "$1" | sh "$gate" --image "$work/image.img" --recovery "$work/backup.img" --partition boot > "$work/out.txt" 2> "$work/err.txt" || echo "$?" > "$work/status"
  [ -f "$work/status" ] || echo 0 > "$work/status"
  status=$(cat "$work/status")
  rm -f "$work/status"
  return 0
}

STUB_ADB_DEVICES="" STUB_FASTBOOT_DEVICES="" run_gate ""
check "dynamic: zero devices -> exit 2 no-device" [ "$status" = 2 ]
check "dynamic: zero devices record" grep -q '"reason":"no-device"' "$work/out.txt"

STUB_ADB_DEVICES="SYN000001 device usb:1 product:syn
SYN000002 device usb:2 product:syn
" STUB_FASTBOOT_DEVICES="" run_gate ""
check "dynamic: two devices -> exit 5 multiple-devices" [ "$status" = 5 ]
check "dynamic: two devices record" grep -q '"reason":"multiple-devices"' "$work/out.txt"

STUB_ADB_DEVICES="SYN000001 device usb:1 product:syn
" STUB_FASTBOOT_DEVICES="SYN000009
" run_gate ""
check "dynamic: adb + fastboot device -> ambiguous" [ "$status" = 5 ]

export STUB_ADB_DEVICES="SYN000001 device usb:1 product:syn
" STUB_FASTBOOT_DEVICES=""
run_gate "I have verified nope syntheticdev rev-a"
check "dynamic: wrong confirmation -> exit 4" [ "$status" = 4 ]
check "dynamic: wrong confirmation record" grep -q '"reason":"confirmation-mismatch"' "$work/out.txt"
check "dynamic: identity printed (serial)" grep -q '^serial=SYN000001$' "$work/out.txt"
check "dynamic: identity printed (codename)" grep -q '^product_codename=syntheticdev$' "$work/out.txt"
check "dynamic: identity printed (revision)" grep -q '^hardware_revision=rev-a$' "$work/out.txt"
check "dynamic: identity printed (slot)" grep -q '^current_slot=_a$' "$work/out.txt"
check "dynamic: identity printed (partition)" grep -q '^partition_target=boot$' "$work/out.txt"
check "dynamic: image hash printed" grep -q '^image_sha256=[0-9a-f]\{64\}$' "$work/out.txt"
check "dynamic: recovery hash printed" grep -q '^recovery_sha256=[0-9a-f]\{64\}$' "$work/out.txt"

run_gate "I have verified SYN000 syntheticdev rev-a"
check "dynamic: exact confirmation -> exit 3" [ "$status" = 3 ]
check "dynamic: refused for lack of an approved proposal" grep -q '"event":"provisioning-refused","reason":"no-approved-proposal"' "$work/out.txt"

run_gate_expect() {
  printf '%s\n' "I have verified SYN000 syntheticdev rev-a" | sh "$gate" --image "$work/image.img" --recovery "$work/backup.img" --partition boot "$@" > "$work/out.txt" 2>/dev/null || echo "$?" > "$work/status"
  [ -f "$work/status" ] || echo 0 > "$work/status"
  status=$(cat "$work/status"); rm -f "$work/status"; return 0
}
run_gate_expect --expect-codename otherdev
check "dynamic: codename mismatch -> exit 4" [ "$status" = 4 ]
run_gate_expect --expect-revision rev-b
check "dynamic: revision mismatch -> exit 4" [ "$status" = 4 ]

bad_calls=$(grep -vE '^adb devices -l$|^adb -s [A-Za-z0-9]+ shell getprop ro\.|^fastboot devices -l$|^fastboot -s [A-Za-z0-9]+ getvar ' "$calls" || true)
check "dynamic: every stub call was read-only" [ -z "$bad_calls" ]
[ -z "$bad_calls" ] || printf '%s\n' "$bad_calls"

# discover.sh with stubs: no device -> exit 2, transcript exists and carries no serial
export FERRIX_OUT_DIR="$work/out"
disc_status=0
STUB_ADB_DEVICES="" STUB_FASTBOOT_DEVICES="" sh "$discover" >/dev/null 2>&1 || disc_status=$?
check "discover: zero devices -> exit 2" [ "$disc_status" = 2 ]
disc_status=0
STUB_ADB_DEVICES="SYN000001 device usb:1 product:syn
" sh "$discover" > "$work/disc.txt" 2>/dev/null || disc_status=$?
check "discover: one adb device -> exit 0" [ "$disc_status" = 0 ]
transcript=$(ls "$work/out/oneplus"/discovery-*.txt | tail -1)
check "discover: stdout carries the serial" grep -q '^serial=SYN000001$' "$work/disc.txt"
check "discover: transcript redacts the serial" sh -c "! grep -q 'SYN000001' '$transcript'"
check "discover: transcript has the redacted marker" grep -q 'serial=serial-sha256:' "$transcript"
check "discover: new props queried" grep -q '^ro.build.fingerprint=' "$work/disc.txt"

if [ "$fails" -eq 0 ]; then
  echo '{"event":"oneplus-tools-test","status":"passed"}'
else
  printf '{"event":"oneplus-tools-test","status":"failed","failures":%s}\n' "$fails"
  exit 1
fi
