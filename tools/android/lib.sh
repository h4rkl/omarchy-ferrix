# Shared helpers for tools/android/*.sh (POSIX sh; sourced, not executed).
# Host-side tooling only: nothing here runs in the guest or touches a device.

PLACEHOLDER="UNPINNED-FILL-ME"
MIN_APK_BYTES=1048576        # 1 MiB: Fulguris is a compact F-Droid APK
MAX_APK_BYTES=524288000     # 500 MiB: refuse anything implausibly large

# Tool overrides let tests point at a missing tool; defaults resolve via PATH.
CURL="${CURL:-curl}"
APKSIGNER="${APKSIGNER:-apksigner}"
SHASUM="${SHASUM:-shasum}"

# emit EVENT [key value]...  -> one JSON line, "evidence" first.
# Values are emitted as JSON strings; keys and values must not contain quotes.
emit() {
    _event=$1
    shift
    printf '{"evidence":"host","event":"%s"' "$_event"
    while [ "$#" -ge 2 ]; do
        printf ',"%s":"%s"' "$1" "$2"
        shift 2
    done
    printf '}\n'
}

# reject REASON [key value]... -> record + exit 1
reject() {
    _reason=$1
    shift
    emit fulguris-apk-rejected reason "$_reason" "$@"
    exit 1
}

# require_tools -> reports (never installs) the first missing host tool.
require_tools() {
    for _tool in "$@"; do
        if ! command -v "$_tool" >/dev/null 2>&1; then
            reject "missing-tool" tool "$(basename "$_tool")"
        fi
    done
}

# pin FILE KEY -> value of KEY=... in FILE (no shell evaluation).
pin() {
    _value=$(sed -n "s/^$2=//p" "$1" | head -n 1)
    if [ -z "$_value" ]; then
        reject "pin-missing" key "$2" pins "$1"
    fi
    case $_value in
        *"$PLACEHOLDER"*) reject "pin-unpinned" key "$2" pins "$1" ;;
    esac
    printf '%s' "$_value"
}

# Validate the pin shapes so a bad pins file cannot inject into a URL/path.
check_pin_shapes() {
    case $FULGURIS_VERSION in
        [0-9]*.[0-9]*.[0-9]*) ;;
        *) reject "pin-invalid" key FULGURIS_VERSION ;;
    esac
    case $FULGURIS_VERSION in
        *[!0-9a-zA-Z.]*) reject "pin-invalid" key FULGURIS_VERSION ;;
    esac
    case $FULGURIS_VERSION_CODE in
        ''|*[!0-9]*) reject "pin-invalid" key FULGURIS_VERSION_CODE ;;
    esac
    case $FULGURIS_PACKAGE in
        net.slions.fulguris.full.fdroid) ;;
        *) reject "pin-invalid" key FULGURIS_PACKAGE ;;
    esac
    case $FULGURIS_ASSET in
        *.apk) ;;
        *) reject "pin-invalid" key FULGURIS_ASSET ;;
    esac
    case $FULGURIS_ASSET in
        *[!0-9a-zA-Z._-]*|.*|*/*) reject "pin-invalid" key FULGURIS_ASSET ;;
    esac
    case $FULGURIS_SIZE in
        ''|*[!0-9]*) reject "pin-invalid" key FULGURIS_SIZE ;;
    esac
    if [ "$FULGURIS_SIZE" -lt "$MIN_APK_BYTES" ] || [ "$FULGURIS_SIZE" -gt "$MAX_APK_BYTES" ]; then
        reject "pin-invalid" key FULGURIS_SIZE
    fi
    if [ "$(printf '%s' "$FULGURIS_SHA256" | tr -d '0-9a-f' | wc -c | tr -d ' ')" != 0 ] \
        || [ "$(printf '%s' "$FULGURIS_SHA256" | wc -c | tr -d ' ')" != 64 ]; then
        reject "pin-invalid" key FULGURIS_SHA256
    fi
    if [ "$(printf '%s' "$FULGURIS_SIGNER_SHA256" | tr -d '0-9A-F' | wc -c | tr -d ' ')" != 0 ] \
        || [ "$(printf '%s' "$FULGURIS_SIGNER_SHA256" | wc -c | tr -d ' ')" != 64 ]; then
        reject "pin-invalid" key FULGURIS_SIGNER_SHA256
    fi
    case $FULGURIS_URL in
        https://f-droid.org/repo/$FULGURIS_ASSET) ;;
        *) reject "pin-invalid" key FULGURIS_URL ;;
    esac
}

load_pins() {
    _pins=$1
    if [ ! -f "$_pins" ]; then
        reject "pins-not-found" pins "$_pins"
    fi
    # Checked here (not only in `pin`) because `pin` runs in a subshell and
    # its exit would otherwise be swallowed by the assignment.
    if grep -q "$PLACEHOLDER" "$_pins"; then
        _key=$(grep "$PLACEHOLDER" "$_pins" | head -n 1 | cut -d= -f1)
        reject "pin-unpinned" key "$_key" pins "$_pins"
    fi
    for _key in FULGURIS_VERSION FULGURIS_VERSION_CODE FULGURIS_PACKAGE FULGURIS_ASSET FULGURIS_URL FULGURIS_SIZE FULGURIS_SHA256 FULGURIS_SIGNER_SHA256; do
        if ! grep -q "^$_key=." "$_pins"; then
            reject "pin-missing" key "$_key" pins "$_pins"
        fi
    done
    FULGURIS_VERSION=$(pin "$_pins" FULGURIS_VERSION)
    FULGURIS_VERSION_CODE=$(pin "$_pins" FULGURIS_VERSION_CODE)
    FULGURIS_PACKAGE=$(pin "$_pins" FULGURIS_PACKAGE)
    FULGURIS_ASSET=$(pin "$_pins" FULGURIS_ASSET)
    FULGURIS_URL=$(pin "$_pins" FULGURIS_URL)
    FULGURIS_SIZE=$(pin "$_pins" FULGURIS_SIZE)
    FULGURIS_SHA256=$(pin "$_pins" FULGURIS_SHA256 | tr 'A-F' 'a-f')
    FULGURIS_SIGNER_SHA256=$(pin "$_pins" FULGURIS_SIGNER_SHA256 | tr -d ':' | tr 'a-f' 'A-F')
    check_pin_shapes
}

file_size() {
    # POSIX: wc -c reads the file; fine for a few hundred MB.
    wc -c <"$1" | tr -d ' '
}

check_size() {
    _size=$(file_size "$1")
    if [ "$_size" -lt "$MIN_APK_BYTES" ] || [ "$_size" -gt "$MAX_APK_BYTES" ]; then
        reject "unexpected-size" size "$_size" min "$MIN_APK_BYTES" max "$MAX_APK_BYTES"
    fi
}

sha256_of() {
    "$SHASUM" -a 256 "$1" | cut -d' ' -f1 | tr 'A-F' 'a-f'
}
