#!/bin/sh
# Host tests for the Android acquisition scripts (Waydroid image pins).
#
#   sh tools/android/tests/run.sh
#
# The Browser is LineageOS Jelly, built into the Android image (ADR-008), so
# no APK is fetched any more. What remains host-side is the pinned Waydroid
# image fetch: its pin values must be treated as data and never executed,
# and the checked-in pins must parse. Requires sh, shasum, unzip (reported
# when missing). Never touches the network.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
tools=$(cd "$here/.." && pwd)
for tool in shasum unzip; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf '{"evidence":"host","event":"android-tools-test-skipped","reason":"missing-tool","tool":"%s"}\n' "$tool"
        exit 2
    fi
done

work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-android-tests.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM
passed=0
failed=0

pass() { passed=$((passed + 1)); printf 'ok   - %s\n' "$1"; }
fail() { failed=$((failed + 1)); printf 'FAIL - %s\n  %s\n' "$1" "$2"; }

# expect NAME EXPECTED_EXIT NEEDLE -- command...
expect() {
    _name=$1; _exit=$2; _needle=$3; shift 3
    set +e
    _out=$("$@" 2>&1)
    _code=$?
    set -e
    if [ "$_code" -ne "$_exit" ]; then
        fail "$_name" "exit $_code, expected $_exit: $_out"
    elif ! printf '%s' "$_out" | grep -q -- "$_needle"; then
        fail "$_name" "missing $_needle in: $_out"
    else
        pass "$_name"
    fi
}

cd "$work"

# --- Waydroid pin parser ----------------------------------------------------
# A shell fragment in a pin value must be rejected as a filename, never
# re-evaluated. The marker path makes command execution directly observable.
waydroid_marker="$work/waydroid-pin-executed"
cat >waydroid-injection.pins <<PINS
WAYDROID_CHANNEL=waydroid_arm64_only
WAYDROID_BUILD=20260403
WAYDROID_SYSTEM_FILE=\$(touch $waydroid_marker)
WAYDROID_SYSTEM_URL=https://sourceforge.net/projects/waydroid/files/images/system/x.zip/download
WAYDROID_SYSTEM_SIZE=1
WAYDROID_SYSTEM_SHA256=0000000000000000000000000000000000000000000000000000000000000000
WAYDROID_VENDOR_FILE=v.zip
WAYDROID_VENDOR_URL=https://sourceforge.net/projects/waydroid/files/images/vendor/v.zip/download
WAYDROID_VENDOR_SIZE=1
WAYDROID_VENDOR_SHA256=0000000000000000000000000000000000000000000000000000000000000000
PINS
expect "Waydroid pin shell text is rejected without execution" 1 '"reason":"pin-invalid"' \
    env FERRIX_OUT_DIR="$work/waydroid-out" \
        sh "$tools/waydroid-images-fetch.sh" --verify-only --pins waydroid-injection.pins
[ ! -e "$waydroid_marker" ] \
    && pass "Waydroid pins are data, not shell" \
    || fail "Waydroid pins are data, not shell" "pin value executed"

expect "missing pins file refused" 1 '"reason":"pins-not-found"' \
    env FERRIX_OUT_DIR="$work/waydroid-out" \
        sh "$tools/waydroid-images-fetch.sh" --verify-only --pins "$work/nope.pins"

# The checked-in pins parse; with nothing downloaded, verify-only stops at
# the first missing file rather than at a pin error.
expect "checked-in Waydroid pins parse" 1 '"reason":"file-missing"' \
    env FERRIX_OUT_DIR="$work/waydroid-empty" \
        sh "$tools/waydroid-images-fetch.sh" --verify-only

printf '{"evidence":"host","event":"android-tools-tests","passed":%s,"failed":%s}\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
