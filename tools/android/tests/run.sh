#!/bin/sh
# Host tests for the Fulguris and Waydroid acquisition scripts.
#
#   sh tools/android/tests/run.sh
#
# Builds a fake APK (a stored zip > 1 MiB) and uses a deterministic fake
# apksigner to assert: correct local file accepted; wrong hash rejected; wrong
# signer and invalid signatures rejected; wrong redirect host refused (fake
# `curl` on PATH); missing tool refused; placeholder pins refused; Waydroid
# pin values are treated as data and cannot execute shell commands.
# Requires sh, zip, unzip, openssl, shasum, dd (reported when missing).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
tools=$(cd "$here/.." && pwd)
for tool in zip unzip openssl shasum dd; do
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

# --- fixtures ---------------------------------------------------------------
cd "$work"
openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem -days 30 \
    -subj '/CN=ferrix-test-signer' >/dev/null 2>&1
openssl req -x509 -newkey rsa:2048 -nodes -keyout key2.pem -out cert2.pem -days 30 \
    -subj '/CN=other-signer' >/dev/null 2>&1
mkdir -p signed/META-INF unsigned/META-INF
# PKCS#7 certs-only block, as JAR signing produces (no CRL).
openssl crl2pkcs7 -nocrl -certfile cert.pem -outform DER -out signed/META-INF/CERT.RSA
printf 'Manifest-Version: 1.0\n' >signed/META-INF/MANIFEST.MF
printf 'Manifest-Version: 1.0\n' >unsigned/META-INF/MANIFEST.MF
# 2 MiB payload stored uncompressed so the archive itself passes the size floor.
dd if=/dev/zero of=signed/classes.dex bs=1048576 count=2 2>/dev/null
cp signed/classes.dex unsigned/classes.dex
(cd signed && zip -q -0 -r ../fake.apk .)
(cd unsigned && zip -q -0 -r ../unsigned.apk .)
dd if=/dev/zero of=tiny.apk bs=1024 count=1 2>/dev/null

sha=$(shasum -a 256 fake.apk | cut -d' ' -f1)
signer=$(openssl x509 -in cert.pem -fingerprint -sha256 -noout | sed 's/^[sS][hH][aA]256 Fingerprint=//' | tr -d ':')
other_signer=$(openssl x509 -in cert2.pem -fingerprint -sha256 -noout | sed 's/^[sS][hH][aA]256 Fingerprint=//' | tr -d ':')
unsigned_sha=$(shasum -a 256 unsigned.apk | cut -d' ' -f1)

pins() {
    # pins FILE SHA SIGNER SIZE
    cat >"$1" <<PINS
FULGURIS_VERSION=9.99.999
FULGURIS_VERSION_CODE=999999
FULGURIS_PACKAGE=net.slions.fulguris.full.fdroid
FULGURIS_ASSET=net.slions.fulguris.full.fdroid_261.apk
FULGURIS_URL=https://f-droid.org/repo/net.slions.fulguris.full.fdroid_261.apk
FULGURIS_SIZE=$4
FULGURIS_SHA256=$2
FULGURIS_SIGNER_SHA256=$3
PINS
}
fake_size=$(wc -c <fake.apk | tr -d ' ')
unsigned_size=$(wc -c <unsigned.apk | tr -d ' ')
pins good.pins "$sha" "$signer" "$fake_size"
pins badhash.pins "0000000000000000000000000000000000000000000000000000000000000000" "$signer" "$fake_size"
pins badsigner.pins "$sha" "$other_signer" "$fake_size"
pins unsigned.pins "$unsigned_sha" "$signer" "$unsigned_size"
pins placeholder.pins "UNPINNED-FILL-ME" "$signer" "$fake_size"

# Fake curl: honours -o FILE, -D FILE, -w FMT; body is $FAKE_CURL_BODY,
# effective URL is $FAKE_CURL_EFFECTIVE_URL. Never touches the network.
mkdir -p fakebin
cat >fakebin/curl <<'FAKE'
#!/bin/sh
out=""; dump=""; write=""
while [ "$#" -gt 0 ]; do
    case $1 in
        -o) out=$2; shift 2 ;;
        -D) dump=$2; shift 2 ;;
        -w) write=$2; shift 2 ;;
        --proto|--max-redirs) shift 2 ;;
        *) shift ;;
    esac
done
if [ -n "$dump" ]; then
    size=$(wc -c <"$FAKE_CURL_BODY" | tr -d ' ')
    printf 'HTTP/1.1 302 Found\r\nLocation: elsewhere\r\n\r\nHTTP/1.1 200 OK\r\nContent-Type: application/vnd.android.package-archive\r\nContent-Length: %s\r\n\r\n' "$size" >"$dump"
fi
if [ -n "$out" ] && [ "$out" != /dev/null ]; then
    cp "$FAKE_CURL_BODY" "$out"
fi
if [ -n "$write" ]; then
    printf '%s' "$FAKE_CURL_EFFECTIVE_URL"
fi
FAKE
chmod +x fakebin/curl
cat >fakebin/apksigner <<'FAKE'
#!/bin/sh
if [ "${FAKE_APKSIGNER_EXIT:-0}" -ne 0 ]; then
    printf 'DOES NOT VERIFY\n' >&2
    exit "$FAKE_APKSIGNER_EXIT"
fi
printf 'Verifies\nSigner #1 certificate SHA-256 digest: %s\n' "$FAKE_APKSIGNER_SIGNER"
FAKE
chmod +x fakebin/apksigner
APKSIGNER="$work/fakebin/apksigner"
FAKE_APKSIGNER_SIGNER=$signer
export APKSIGNER FAKE_APKSIGNER_SIGNER

# --- verify script ----------------------------------------------------------
expect "correct local file accepted" 0 '"event":"fulguris-apk-verified"' \
    sh "$tools/fulguris-verify.sh" fake.apk --pins good.pins
expect "fetch --local accepts the same file" 0 '"event":"fulguris-apk-verified"' \
    sh "$tools/fulguris-fetch.sh" --local fake.apk --pins good.pins
expect "wrong hash rejected" 1 '"reason":"wrong-sha256"' \
    sh "$tools/fulguris-verify.sh" fake.apk --pins badhash.pins
expect "wrong signer rejected" 1 '"reason":"wrong-signer"' \
    sh "$tools/fulguris-verify.sh" fake.apk --pins badsigner.pins
expect "invalid APK signature rejected" 1 '"reason":"signature-invalid"' \
    env FAKE_APKSIGNER_EXIT=1 sh "$tools/fulguris-verify.sh" unsigned.apk --pins unsigned.pins
expect "implausible size rejected" 1 '"reason":"unexpected-size"' \
    sh "$tools/fulguris-verify.sh" tiny.apk --pins good.pins
expect "placeholder pins refused" 1 '"reason":"pin-unpinned"' \
    sh "$tools/fulguris-verify.sh" fake.apk --pins placeholder.pins
expect "missing tool refused (apksigner)" 1 '"reason":"missing-tool","tool":"apksigner"' \
    env APKSIGNER=/nonexistent/apksigner sh "$tools/fulguris-verify.sh" fake.apk --pins good.pins
expect "missing file rejected" 1 '"reason":"apk-not-found"' \
    sh "$tools/fulguris-verify.sh" nope.apk --pins good.pins
expect "checked-in pins parse" 1 '"reason":"apk-not-found"' \
    sh "$tools/fulguris-verify.sh" nope.apk

# --- fetch script (fake curl on PATH, no network) ---------------------------
expect "wrong redirect host refused" 1 '"reason":"unexpected-host","host":"evil.example.net"' \
    env PATH="$work/fakebin:$PATH" FERRIX_OUT_DIR="$work/out-evil" \
        FAKE_CURL_BODY="$work/fake.apk" FAKE_CURL_EFFECTIVE_URL="https://evil.example.net/x.apk" \
        sh "$tools/fulguris-fetch.sh" --pins good.pins
[ ! -e "$work/out-evil/android/fulguris/9.99.999/net.slions.fulguris.full.fdroid_261.apk" ] \
    && [ ! -e "$work/out-evil/android/fulguris/9.99.999/net.slions.fulguris.full.fdroid_261.apk.part" ] \
    && pass "nothing stored after host refusal" \
    || fail "nothing stored after host refusal" "file exists"
expect "missing tool refused (curl)" 1 '"reason":"missing-tool","tool":"curl"' \
    env CURL=/nonexistent/curl sh "$tools/fulguris-fetch.sh" --pins good.pins
expect "good download through allowed host stored" 0 '"event":"fulguris-apk-stored"' \
    env PATH="$work/fakebin:$PATH" FERRIX_OUT_DIR="$work/out-ok" \
        FAKE_CURL_BODY="$work/fake.apk" FAKE_CURL_EFFECTIVE_URL="https://f-droid.org/repo/net.slions.fulguris.full.fdroid_261.apk" \
        sh "$tools/fulguris-fetch.sh" --pins good.pins
[ -f "$work/out-ok/android/fulguris/9.99.999/net.slions.fulguris.full.fdroid_261.apk" ] \
    && pass "stored at the pinned path" \
    || fail "stored at the pinned path" "missing"
expect "tampered download rejected and removed" 1 '"reason":"wrong-sha256"' \
    env PATH="$work/fakebin:$PATH" FERRIX_OUT_DIR="$work/out-bad" \
        FAKE_CURL_BODY="$work/fake.apk" FAKE_CURL_EFFECTIVE_URL="https://f-droid.org/repo/net.slions.fulguris.full.fdroid_261.apk" \
        sh "$tools/fulguris-fetch.sh" --pins badhash.pins
[ ! -e "$work/out-bad/android/fulguris/9.99.999/net.slions.fulguris.full.fdroid_261.apk.part" ] \
    && pass "part file removed after rejection" \
    || fail "part file removed after rejection" "part exists"

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

printf '{"evidence":"host","event":"android-tools-tests","passed":%s,"failed":%s}\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
