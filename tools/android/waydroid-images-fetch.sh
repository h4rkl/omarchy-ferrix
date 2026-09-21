#!/bin/sh
# Fetches the pinned Waydroid system/vendor image zips for the OPT-IN Android
# compatibility runtime (postmarketOS QEMU route, FERRIX_PMOS_ANDROID=1),
# verifies them, and unpacks system.img / vendor.img.
#
#   sh tools/android/waydroid-images-fetch.sh                 # download + verify + unzip
#   sh tools/android/waydroid-images-fetch.sh --verify-only   # re-check what is on disk, no network
#   sh tools/android/waydroid-images-fetch.sh --pins FILE
#
# Only the pinned https://sourceforge.net/... URLs are fetched; every redirect
# must end on sourceforge.net or a *.dl.sourceforge.net mirror; the size must
# equal the pinned size and the SHA-256 must match before anything is kept.
# Files land under ${FERRIX_OUT_DIR:-out}/android/waydroid/<build>/ (ignored
# by Git) and `.../current` points at that directory for pmos-build. Nothing
# auto-updates; no credentials are used. Requires curl, unzip, shasum
# (reported when missing, never installed). Host tooling only.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
pins="$here/waydroid.pins"
verify_only=0
while [ "$#" -gt 0 ]; do
  case $1 in
    --pins) [ "$#" -ge 2 ] || { echo "--pins needs a file" >&2; exit 2; }; pins=$2; shift 2 ;;
    --verify-only) verify_only=1; shift ;;
    -h|--help) sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
CURL=${CURL:-curl}; UNZIP=${UNZIP:-unzip}; SHASUM=${SHASUM:-shasum}
emit() { _e=$1; shift; printf '{"evidence":"host","event":"%s"' "$_e"; while [ "$#" -ge 2 ]; do printf ',"%s":"%s"' "$1" "$2"; shift 2; done; printf '}\n'; }
reject() { _r=$1; shift; emit waydroid-images-rejected reason "$_r" "$@"; exit 1; }
pin() { _v=$(sed -n "s/^$1=//p" "$pins" | head -n 1); [ -n "$_v" ] || reject pin-missing key "$1" pins "$pins"; printf '%s' "$_v"; }
[ -f "$pins" ] || reject pins-not-found pins "$pins"
for tool in "$SHASUM" "$UNZIP"; do command -v "$tool" >/dev/null 2>&1 || reject missing-tool tool "$(basename "$tool")"; done
[ "$verify_only" = 1 ] || command -v "$CURL" >/dev/null 2>&1 || reject missing-tool tool "$(basename "$CURL")"
build=$(pin WAYDROID_BUILD); channel=$(pin WAYDROID_CHANNEL)
case "$build" in *[!0-9]*|'') reject pin-invalid key WAYDROID_BUILD ;; esac
case "$channel" in *[!0-9a-z_]*|'') reject pin-invalid key WAYDROID_CHANNEL ;; esac
out_root="${FERRIX_OUT_DIR:-$root/out}/android/waydroid"
dest="$out_root/$build"
mkdir -p "$dest"
host_of() { printf '%s' "$1" | sed -n 's#^https://\([^/:?]*\).*#\1#p'; }
check_host() {
  # Do not use `_h` here: callers keep their pinned hash in that variable.
  # POSIX sh has no function-local variables, so the old name clobbered the
  # expected digest after validating SourceForge's redirected download host.
  resolved_host=$(host_of "$1")
  case $resolved_host in sourceforge.net|*.dl.sourceforge.net) ;; *) reject unexpected-host host "${resolved_host:-none}" ;; esac
}
sha256_of() { "$SHASUM" -a 256 "$1" | cut -d' ' -f1 | tr 'A-F' 'a-f'; }
size_of() { wc -c <"$1" | tr -d ' '; }
check_shape() { # KIND
  eval "_f=\$${1}_FILE _u=\$${1}_URL _s=\$${1}_SIZE _h=\$${1}_SHA256"
  case $_f in *.zip) ;; *) reject pin-invalid key "${1}_FILE" ;; esac
  case $_f in *[!0-9a-zA-Z._-]*|.*|*/*) reject pin-invalid key "${1}_FILE" ;; esac
  case $_s in ''|*[!0-9]*) reject pin-invalid key "${1}_SIZE" ;; esac
  [ "$(printf '%s' "$_h" | tr -d '0-9a-f' | wc -c | tr -d ' ')" = 0 ] && [ "$(printf '%s' "$_h" | wc -c | tr -d ' ')" = 64 ] || reject pin-invalid key "${1}_SHA256"
  check_host "$_u"
  case $_u in *"/$_f/download") ;; *) reject pin-invalid key "${1}_URL" detail "url must end in /<file>/download" ;; esac
}
fetch_one() { # KIND
  eval "_f=\$${1}_FILE _u=\$${1}_URL _s=\$${1}_SIZE _h=\$${1}_SHA256"
  final="$dest/$_f"; part="$final.part"
  if [ ! -f "$final" ]; then
    [ "$verify_only" = 0 ] || reject file-missing file "$final"
    emit waydroid-image-fetching kind "$1" file "$_f" url "$_u" content_length "$_s"
    rm -f "$part"
    effective=$("$CURL" -fL --proto '=https' --max-redirs 5 -sS -o "$part" -w '%{url_effective}' "$_u") || { rm -f "$part"; reject download-failed url "$_u"; }
    check_host "$effective"
    mv "$part" "$final"
  fi
  got=$(size_of "$final")
  [ "$got" = "$_s" ] || { rm -f "$final"; reject unexpected-size file "$_f" expected "$_s" actual "$got"; }
  sum=$(sha256_of "$final")
  [ "$sum" = "$_h" ] || { rm -f "$final"; reject sha256-mismatch file "$_f" expected "$_h" actual "$sum"; }
  emit waydroid-image-verified kind "$1" file "$_f" sha256 "$sum" bytes "$got"
}
for kind in WAYDROID_SYSTEM WAYDROID_VENDOR; do
  eval "${kind}_FILE=\$(pin ${kind}_FILE) ${kind}_URL=\$(pin ${kind}_URL) ${kind}_SIZE=\$(pin ${kind}_SIZE) ${kind}_SHA256=\$(pin ${kind}_SHA256 | tr 'A-F' 'a-f')"
  check_shape "$kind"
done
fetch_one WAYDROID_SYSTEM
fetch_one WAYDROID_VENDOR
# Unpack: each zip holds exactly one image (system.img / vendor.img).
for pair in "WAYDROID_SYSTEM system.img" "WAYDROID_VENDOR vendor.img"; do
  kind=${pair% *}; img=${pair#* }
  eval "_f=\$${kind}_FILE"
  if [ ! -f "$dest/$img" ]; then
    [ "$verify_only" = 0 ] || reject image-missing file "$dest/$img"
    "$UNZIP" -o -q "$dest/$_f" "$img" -d "$dest" || reject unzip-failed file "$_f"
  fi
  emit waydroid-image-unpacked file "$img" sha256 "$(sha256_of "$dest/$img")" bytes "$(size_of "$dest/$img")"
done
# Record what pmos-build will consume; `current` is the build directory pmos-build reads by default.
{
  printf '{"schema_version":1,"channel":"%s","build":"%s","system":{"zip":"%s","zip_sha256":"%s","img_sha256":"%s"},"vendor":{"zip":"%s","zip_sha256":"%s","img_sha256":"%s"}}\n' \
    "$channel" "$build" "$WAYDROID_SYSTEM_FILE" "$WAYDROID_SYSTEM_SHA256" "$(sha256_of "$dest/system.img")" \
    "$WAYDROID_VENDOR_FILE" "$WAYDROID_VENDOR_SHA256" "$(sha256_of "$dest/vendor.img")"
} > "$dest/images.json"
rm -f "$out_root/current"; ln -s "$build" "$out_root/current"
emit waydroid-images-ready dir "$dest" current "$out_root/current"
