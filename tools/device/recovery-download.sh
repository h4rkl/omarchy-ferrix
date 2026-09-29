#!/bin/sh
set -eu

# Generic pinned downloader for official vendor recovery images. It reads
# targets/boards/<target_id>/recovery.pins (TSV: kind build version filename
# url sha256 bytes source-page), downloads each pinned file with curl into the
# ignored target-local recovery folder, and verifies byte size and SHA-256
# against the pin. It never extracts anything, never contacts a device, and
# never invokes adb or fastboot.
#
# Downloads are large, so the run is opt-in through
# FERRIX_RECOVERY_DOWNLOAD=I-understand-this-downloads-vendor-firmware.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
boards=${FERRIX_BOARDS_DIR:-"$root/targets/boards"}
opt_in="I-understand-this-downloads-vendor-firmware"

die() {
  echo "$*" >&2
  exit 1
}

usage() {
  echo "usage: $0 --target <target_id> [--kind factory|ota|all]" >&2
  exit 2
}

target_id=""
kind=all
while [ "$#" -gt 0 ]; do
  case $1 in
    --target) [ "$#" -ge 2 ] || usage; target_id=$2; shift 2 ;;
    --kind) [ "$#" -ge 2 ] || usage; kind=$2; shift 2 ;;
    -h|--help) usage ;;
    *) usage ;;
  esac
done
[ -n "$target_id" ] || usage
case $target_id in
  *[!a-z0-9-]*|'') die "refusing target id with characters outside [a-z0-9-]: $target_id" ;;
esac
case $kind in
  all|factory|ota) ;;
  *) die "unsupported --kind $kind (factory|ota|all)" ;;
esac

target="$boards/$target_id"
pins="$target/recovery.pins"
out="$target/recovery"
[ -d "$target" ] || die "missing target directory: $target"
[ -f "$pins" ] || die "missing pins file: $pins"

[ "${FERRIX_RECOVERY_DOWNLOAD:-}" = "$opt_in" ] || die "refusing: vendor recovery downloads are large and opt-in.
Re-run with FERRIX_RECOVERY_DOWNLOAD=$opt_in"

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing $1"
}

need curl
if command -v shasum >/dev/null 2>&1; then
  hash_tool=shasum
else
  need sha256sum
  hash_tool=sha256sum
fi

sha256_of() {
  if [ "$hash_tool" = shasum ]; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    sha256sum "$1" | awk '{ print $1 }'
  fi
}

bytes_of() {
  wc -c < "$1" | tr -d '[:space:]'
}

# Non-comment rows, optionally filtered by kind ($1).
rows() {
  awk -F '\t' -v wanted="$1" '
    /^[[:space:]]*#/ || !NF { next }
    wanted == "all" || $1 == wanted { print }
  ' "$pins"
}

row_count=$(rows "$kind" | awk 'END { print NR + 0 }')
[ "$row_count" -gt 0 ] || die "refusing: no pinned $kind rows in $pins; pin the official vendor URL, SHA-256, and byte size first"

download_one() {
  row_kind=$1 filename=$4 url=$5 expected_hash=$6 expected_bytes=$7 source_page=${8:-}
  [ -n "$filename" ] && [ -n "$url" ] && [ -n "$expected_hash" ] && [ -n "$expected_bytes" ] || die "missing or malformed $row_kind pin in $pins"
  case $filename in
    */*|.*) die "refusing pinned filename with path components: $filename" ;;
  esac
  case $expected_hash in
    *[!0-9a-f]*) die "malformed sha256 for $filename" ;;
  esac
  [ "${#expected_hash}" -eq 64 ] || die "malformed sha256 for $filename"
  case $expected_bytes in
    *[!0-9]*|'') die "malformed byte size for $filename" ;;
  esac
  case $url in
    https://*) ;;
    file://*) [ "${FERRIX_RECOVERY_ALLOW_FILE_URL:-}" = 1 ] || die "refusing file:// pin without FERRIX_RECOVERY_ALLOW_FILE_URL=1: $url" ;;
    *) die "refusing non-https recovery URL: $url" ;;
  esac

  final="$out/$filename"
  part="$final.part"
  mkdir -p "$out"

  # Recovery archives are generated inputs. Never follow a link supplied at
  # either output path: a linked final file could make a verified archive
  # point outside the target's ignored recovery directory, and a linked .part
  # path would let curl write arbitrary bytes outside that directory.
  [ ! -L "$final" ] || die "refusing symlinked recovery artifact: $final"
  [ ! -L "$part" ] || die "refusing symlinked partial recovery artifact: $part"

  if [ -f "$final" ]; then
    actual_hash=$(sha256_of "$final")
    actual_bytes=$(bytes_of "$final")
    if [ "$actual_hash" = "$expected_hash" ] && [ "$actual_bytes" = "$expected_bytes" ]; then
      printf 'verified existing %s (%s bytes)\n' "$final" "$actual_bytes"
      return 0
    fi
    echo "existing file failed verification; replacing only generated path: $final" >&2
    rm -f "$final"
  fi

  printf 'downloading %s %s from %s\n' "$row_kind" "$filename" "$url"
  resume=""
  [ -f "$part" ] && resume="--continue-at -"
  # shellcheck disable=SC2086
  curl --fail --location --retry 3 --connect-timeout 30 $resume --output "$part" "$url"
  actual_bytes=$(bytes_of "$part")
  [ "$actual_bytes" = "$expected_bytes" ] || {
    echo "size mismatch for $filename: expected $expected_bytes, got $actual_bytes" >&2
    rm -f "$part"
    exit 1
  }
  actual_hash=$(sha256_of "$part")
  [ "$actual_hash" = "$expected_hash" ] || {
    echo "SHA-256 mismatch for $filename: expected $expected_hash, got $actual_hash" >&2
    rm -f "$part"
    exit 1
  }
  mv "$part" "$final"
  printf 'verified %s (%s bytes, sha256 %s)\n' "$final" "$actual_bytes" "$actual_hash"
  [ -z "$source_page" ] || printf 'source: %s\n' "$source_page"
}

tab=$(printf '\t')
selected=$(mktemp "${TMPDIR:-/tmp}/ferrix-recovery-rows.XXXXXX")
trap 'rm -f "$selected"' EXIT
rows "$kind" > "$selected"
while IFS=$tab read -r c1 c2 c3 c4 c5 c6 c7 c8; do
  download_one "$c1" "$c2" "$c3" "$c4" "$c5" "$c6" "$c7" "$c8"
done < "$selected"

manifest="$out/download-manifest.txt"
{
  printf '# Generated by tools/device/recovery-download.sh\n'
  printf '# Target: %s\n' "$target_id"
  printf '# Files are generated and ignored; do not commit them.\n'
  rows all | while IFS=$tab read -r c1 c2 c3 c4 c5 c6 c7 c8; do
    final="$out/$c4"
    if [ -f "$final" ]; then
      printf '%s\t%s\t%s\t%s\n' "$c1" "$c4" "$(bytes_of "$final")" "$(sha256_of "$final")"
    fi
  done
} > "$manifest"
printf 'manifest: %s\n' "$manifest"
