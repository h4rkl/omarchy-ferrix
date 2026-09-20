#!/bin/sh
# Fetches the pinned Brave APK (Android application for the Android
# compatibility runtime) from the official Brave GitHub release and verifies
# it, or verifies a developer-supplied APK.
#
#   sh tools/android/brave-fetch.sh              # download + verify
#   sh tools/android/brave-fetch.sh --local PATH # verify an existing APK
#   sh tools/android/brave-fetch.sh --pins FILE  # alternative pins file
#
# Only https://github.com/brave/brave-browser/releases/download/<TAG>/<ASSET>
# is fetched; redirects must end on github.com or a *.githubusercontent.com
# release host, the content type must be an APK/octet stream, and the size
# must be within [50 MiB, 500 MiB]. The file lands under
# ${FERRIX_OUT_DIR:-out}/android/brave/<TAG>/<ASSET> (ignored by Git) only
# after tools/android/brave-verify.sh accepted it. Nothing auto-updates.
# Requires curl, apksigner (Android SDK Build-Tools), and shasum (reported
# when missing, never installed). No credentials are used or printed.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
. "$here/lib.sh"

local_apk=""
pins="$here/brave.pins"
while [ "$#" -gt 0 ]; do
    case $1 in
        --local)
            [ "$#" -ge 2 ] || reject "usage" detail "--local needs a path"
            local_apk=$2
            shift 2
            ;;
        --pins)
            [ "$#" -ge 2 ] || reject "usage" detail "--pins needs a file"
            pins=$2
            shift 2
            ;;
        -h|--help)
            sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) reject "usage" detail "unknown argument $1" ;;
    esac
done

if [ -n "$local_apk" ]; then
    exec sh "$here/brave-verify.sh" "$local_apk" --pins "$pins"
fi

require_tools "$CURL" "$APKSIGNER" "$SHASUM"
load_pins "$pins"

url="https://github.com/brave/brave-browser/releases/download/$BRAVE_TAG/$BRAVE_ASSET"
out_root="${FERRIX_OUT_DIR:-out}/android/brave/$BRAVE_TAG"
final="$out_root/$BRAVE_ASSET"
part="$final.part"
headers="$out_root/$BRAVE_ASSET.headers"
mkdir -p "$out_root"

if [ -f "$final" ]; then
    # Already fetched: re-verify instead of downloading again.
    exec sh "$here/brave-verify.sh" "$final" --pins "$pins"
fi

host_of() {
    # https://HOST[:port]/... -> HOST
    printf '%s' "$1" | sed -n 's#^https://\([^/:?]*\).*#\1#p'
}

check_host() {
    _host=$(host_of "$1")
    case $_host in
        github.com|objects.githubusercontent.com|release-assets.githubusercontent.com) ;;
        *) reject "unexpected-host" host "${_host:-none}" tag "$BRAVE_TAG" ;;
    esac
}

# Header value from the LAST response in a header dump (the final redirect
# target), lowercase key match, CR stripped.
last_header() {
    tr -d '\r' <"$2" | awk -v key="$1" '
        /^HTTP\// { value = "" }
        { split($0, kv, ": "); if (tolower(kv[1]) == key) value = kv[2] }
        END { print value }'
}

# 1. Preflight (HEAD): host, type, and size before anything is saved.
rm -f "$headers" "$part"
effective=$("$CURL" -fsSIL --proto '=https' --max-redirs 3 -o /dev/null -D "$headers" -w '%{url_effective}' "$url") \
    || reject "preflight-failed" url "$url"
check_host "$effective"
content_type=$(last_header content-type "$headers")
case $content_type in
    application/vnd.android.package-archive*|application/octet-stream*) ;;
    *) reject "unexpected-content-type" content_type "${content_type:-none}" tag "$BRAVE_TAG" ;;
esac
length=$(last_header content-length "$headers")
case $length in
    ''|*[!0-9]*) reject "content-length-missing" tag "$BRAVE_TAG" ;;
esac
if [ "$length" -lt "$MIN_APK_BYTES" ] || [ "$length" -gt "$MAX_APK_BYTES" ]; then
    reject "unexpected-size" size "$length" min "$MIN_APK_BYTES" max "$MAX_APK_BYTES"
fi

# 2. Download to .part, checking the final host again.
emit brave-apk-fetching tag "$BRAVE_TAG" asset "$BRAVE_ASSET" url "$url" content_length "$length"
effective=$("$CURL" -fL --proto '=https' --max-redirs 3 -sS -o "$part" -w '%{url_effective}' "$url") \
    || { rm -f "$part"; reject "download-failed" url "$url"; }
check_host "$effective"
if [ "$(file_size "$part")" != "$length" ]; then
    _got=$(file_size "$part")
    rm -f "$part"
    reject "truncated-download" expected "$length" actual "$_got"
fi

# 3. Verify, then rename atomically.
if ! sh "$here/brave-verify.sh" "$part" --pins "$pins"; then
    rm -f "$part"
    exit 1
fi
mv "$part" "$final"
rm -f "$headers"
emit brave-apk-stored tag "$BRAVE_TAG" asset "$BRAVE_ASSET" path "$final"
