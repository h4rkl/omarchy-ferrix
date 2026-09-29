#!/bin/sh
set -eu

# Offline matcher: compares a redacted device identity (from
# tools/device/discover.sh) against every checked-in physical board target under
# targets/boards/*/manifest.toml. It never contacts a device and never
# generalizes across hardware revisions: a same-codename, different-revision
# device is `match=none`.
#
# Exit codes: 0 exact match, 10 no exact match, 1 refusal (missing or malformed
# identity, ambiguous manifests).

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
boards=${FERRIX_BOARDS_DIR:-"$root/targets/boards"}
identity=${1:-"${FERRIX_OUT_DIR:-"$root/out"}/device/identity-latest.env"}

die() {
  echo "$*" >&2
  exit 1
}

[ -f "$identity" ] || die "missing identity file: $identity (run: just device-discover)"
[ -d "$boards" ] || die "missing boards directory: $boards"

# Only accept the exact shape discover.sh writes: lowercase keys with
# single-quoted values. Anything else is refused before it is sourced.
bad_lines=$(grep -nvE "^[a-z][a-z0-9_]*='([^']|'\\\\'')*'\$" "$identity" | grep -vE '^[0-9]+:[[:space:]]*(#.*)?$' || true)
[ -z "$bad_lines" ] || die "refusing malformed identity file $identity:
$bad_lines"

manufacturer="" codename="" hardware_revision="" sku="" model="" transport=""
# shellcheck disable=SC1090
. "$identity"

lower() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]'
}

# Read a top-level `key = "value"` line from a manifest, stopping at the first
# TOML table header so [gates] keys cannot shadow identity keys.
toml_value() {
  awk -v wanted="$2" '
    /^[[:space:]]*\[/ { exit }
    /^[[:space:]]*#/ { next }
    {
      line = $0
      if (match(line, /^[[:space:]]*[A-Za-z0-9_-]+[[:space:]]*=[[:space:]]*"/)) {
        key = line
        sub(/[[:space:]]*=.*$/, "", key)
        sub(/^[[:space:]]*/, "", key)
        if (key != wanted) next
        sub(/^[^"]*"/, "", line)
        sub(/"[[:space:]]*(#.*)?$/, "", line)
        print line
        exit
      }
    }
  ' "$1"
}

printf 'identity=%s\n' "$identity"
printf 'identity_transport=%s\n' "$transport"
printf 'identity_manufacturer=%s\n' "$manufacturer"
printf 'identity_codename=%s\n' "$codename"
printf 'identity_hardware_revision=%s\n' "$hardware_revision"
printf 'identity_sku=%s\n' "$sku"

if [ -z "$manufacturer" ] || [ -z "$codename" ] || [ -z "$hardware_revision" ]; then
  printf 'match=none\n'
  [ -n "$manufacturer" ] || printf 'note=identity lacks manufacturer; Fastboot does not expose it, rediscover over ADB or set FERRIX_DEVICE_MANUFACTURER\n'
  [ -n "$codename" ] || printf 'note=identity lacks codename\n'
  [ -n "$hardware_revision" ] || printf 'note=identity lacks hardware_revision; never match without an exact revision\n'
  exit 10
fi

exact_count=0
exact_id=""
exact_dir=""
partials=""
scanned=0
for manifest in "$boards"/*/manifest.toml; do
  [ -f "$manifest" ] || continue
  kind=$(toml_value "$manifest" kind)
  [ "$kind" = physical ] || continue
  scanned=$((scanned + 1))
  dir=$(dirname -- "$manifest")
  t_id=$(toml_value "$manifest" target_id)
  [ -n "$t_id" ] || t_id=$(basename -- "$dir")
  t_manufacturer=$(toml_value "$manifest" manufacturer)
  t_codename=$(toml_value "$manifest" codename)
  t_revision=$(toml_value "$manifest" hardware_revision)
  t_sku=$(toml_value "$manifest" sku)

  same_manufacturer=no
  [ "$(lower "$t_manufacturer")" = "$(lower "$manufacturer")" ] && same_manufacturer=yes
  same_codename=no
  [ "$t_codename" = "$codename" ] && same_codename=yes
  same_revision=no
  [ "$t_revision" = "$hardware_revision" ] && same_revision=yes
  sku_ok=yes
  if [ -n "$sku" ] && [ -n "$t_sku" ] && [ "$sku" != "$t_sku" ]; then
    sku_ok=no
  fi

  if [ "$same_manufacturer" = yes ] && [ "$same_codename" = yes ] && [ "$same_revision" = yes ] && [ "$sku_ok" = yes ]; then
    exact_count=$((exact_count + 1))
    exact_id=$t_id
    exact_dir=$dir
    continue
  fi

  if [ "$same_codename" = yes ] && [ "$same_revision" = no ]; then
    partials="${partials}partial=$t_id reason=same-codename-different-revision target_revision=$t_revision
note=same codename, different hardware revision; do not reuse target evidence
"
  elif [ "$same_codename" = yes ] && [ "$sku_ok" = no ]; then
    partials="${partials}partial=$t_id reason=same-codename-different-sku target_sku=$t_sku
note=same codename and revision, different SKU; do not reuse target evidence
"
  elif [ "$same_codename" = yes ] && [ "$same_manufacturer" = no ]; then
    partials="${partials}partial=$t_id reason=same-codename-different-manufacturer target_manufacturer=$t_manufacturer
"
  elif [ "$same_manufacturer" = yes ]; then
    partials="${partials}partial=$t_id reason=same-manufacturer-only target_codename=$t_codename
"
  fi
done

printf 'targets_scanned=%s\n' "$scanned"

if [ "$exact_count" -eq 1 ]; then
  printf 'match=exact target_id=%s target_dir=%s\n' "$exact_id" "$exact_dir"
  exit 0
fi

if [ "$exact_count" -gt 1 ]; then
  printf 'match=ambiguous exact_matches=%s\n' "$exact_count"
  die "refusing: $exact_count physical manifests match the same exact identity; fix targets/boards"
fi

printf 'match=none\n'
[ -z "$partials" ] || printf '%s' "$partials"
printf 'next=just device-propose\n'
exit 10
