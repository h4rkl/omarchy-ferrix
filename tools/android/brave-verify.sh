#!/bin/sh
# Verifies a Brave APK (Android application for the Android compatibility
# runtime) against tools/android/brave.pins: size bounds, SHA-256, and the
# SHA-256 fingerprint of the APK signing certificate.
#
#   sh tools/android/brave-verify.sh PATH [--pins FILE]
#
# Exit 0 and a `brave-apk-verified` record on success; exit 1 and a
# `brave-apk-rejected` record with a typed reason otherwise. Host tooling
# only; nothing is downloaded or installed. Requires apksigner from Android
# SDK Build-Tools and shasum (reported when missing, never installed).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
. "$here/lib.sh"

apk=""
pins="$here/brave.pins"
while [ "$#" -gt 0 ]; do
    case $1 in
        --pins)
            [ "$#" -ge 2 ] || reject "usage" detail "--pins needs a file"
            pins=$2
            shift 2
            ;;
        -h|--help)
            sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        -*) reject "usage" detail "unknown option $1" ;;
        *)
            [ -z "$apk" ] || reject "usage" detail "one APK path only"
            apk=$1
            shift
            ;;
    esac
done
[ -n "$apk" ] || reject "usage" detail "APK path required"

require_tools "$APKSIGNER" "$SHASUM"
load_pins "$pins"

[ -f "$apk" ] || reject "apk-not-found" path "$apk"
check_size "$apk"

actual_sha=$(sha256_of "$apk")
if [ "$actual_sha" != "$BRAVE_SHA256" ]; then
    reject "wrong-sha256" tag "$BRAVE_TAG" expected "$BRAVE_SHA256" actual "$actual_sha"
fi

# apksigner validates every signature scheme present (v1 through the schemes
# supported by the installed Android SDK Build-Tools) before printing the
# verified signer certificate. Do not treat a certificate embedded in the zip
# as proof: only output from a successful APK verification is accepted.
set +e
verify_output=$("$APKSIGNER" verify --print-certs "$apk" 2>&1)
verify_status=$?
set -e
if [ "$verify_status" -ne 0 ]; then
    reject "signature-invalid" tag "$BRAVE_TAG" sha256 "$actual_sha"
fi
fingerprint=$(printf '%s\n' "$verify_output" \
    | sed -n 's/^Signer #1 certificate SHA-256 digest: //p' \
    | head -n 1 | tr -d ': ' | tr 'a-f' 'A-F')
if [ -z "$fingerprint" ]; then
    reject "signature-unreadable" tag "$BRAVE_TAG"
fi
if [ "$fingerprint" != "$BRAVE_SIGNER_SHA256" ]; then
    reject "wrong-signer" tag "$BRAVE_TAG" expected "$BRAVE_SIGNER_SHA256" actual "$fingerprint"
fi

emit brave-apk-verified tag "$BRAVE_TAG" asset "$BRAVE_ASSET" sha256 "$actual_sha" signer_sha256 "$BRAVE_SIGNER_SHA256" size "$(file_size "$apk")" path "$apk"
