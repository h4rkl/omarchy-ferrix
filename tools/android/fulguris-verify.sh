#!/bin/sh
# Verifies a Fulguris APK (Android application for the Android compatibility
# runtime) against tools/android/fulguris.pins: exact size, SHA-256, and the
# SHA-256 fingerprint of the APK signing certificate.
#
#   sh tools/android/fulguris-verify.sh PATH [--pins FILE]
#
# Exit 0 and a `fulguris-apk-verified` record on success; exit 1 and a
# `fulguris-apk-rejected` record with a typed reason otherwise. Host tooling
# only; nothing is downloaded or installed. Requires apksigner from Android
# SDK Build-Tools and shasum (reported when missing, never installed).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
. "$here/lib.sh"

apk=""
pins="$here/fulguris.pins"
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
actual_size=$(file_size "$apk")
if [ "$actual_size" != "$FULGURIS_SIZE" ]; then
    reject "unexpected-size" expected "$FULGURIS_SIZE" actual "$actual_size" version "$FULGURIS_VERSION"
fi

actual_sha=$(sha256_of "$apk")
if [ "$actual_sha" != "$FULGURIS_SHA256" ]; then
    reject "wrong-sha256" version "$FULGURIS_VERSION" expected "$FULGURIS_SHA256" actual "$actual_sha"
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
    reject "signature-invalid" version "$FULGURIS_VERSION" sha256 "$actual_sha"
fi
fingerprint=$(printf '%s\n' "$verify_output" \
    | sed -n 's/^Signer #1 certificate SHA-256 digest: //p' \
    | head -n 1 | tr -d ': ' | tr 'a-f' 'A-F')
if [ -z "$fingerprint" ]; then
    reject "signature-unreadable" version "$FULGURIS_VERSION"
fi
if [ "$fingerprint" != "$FULGURIS_SIGNER_SHA256" ]; then
    reject "wrong-signer" version "$FULGURIS_VERSION" expected "$FULGURIS_SIGNER_SHA256" actual "$fingerprint"
fi

emit fulguris-apk-verified version "$FULGURIS_VERSION" asset "$FULGURIS_ASSET" sha256 "$actual_sha" signer_sha256 "$FULGURIS_SIGNER_SHA256" size "$actual_size" path "$apk"
