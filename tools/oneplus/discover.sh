#!/bin/sh
set -eu

# Read-only transport and identity discovery for the attached OnePlus device
# (docs/board-support/oneplus-attached.md). This script deliberately has no
# unlock, reboot, boot, erase, format, flash, or slot-changing operations:
# only `adb devices`, `adb shell getprop`, `fastboot devices`, and
# `fastboot getvar`. Stdout shows the operator the exact serial; the
# transcript written under out/oneplus/ replaces the serial with the first
# 12 hex digits of its SHA-256 so it can be shared, and it is never committed.
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/oneplus

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing $1; install Android Platform Tools" >&2
    exit 1
  }
}

need adb
need fastboot

sha256_prefix() {
  if command -v shasum >/dev/null 2>&1; then
    printf '%s' "$1" | shasum -a 256 | cut -c1-12
  else
    printf '%s' "$1" | sha256sum | cut -c1-12
  fi
}

mkdir -p "$out"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
transcript="$out/discovery-$stamp.txt"
serial=""
# emit KEY VALUE: stdout verbatim, transcript with the serial redacted.
emit() {
  printf '%s=%s\n' "$1" "$2"
  value=$2
  if [ -n "$serial" ]; then
    value=$(printf '%s' "$value" | sed "s/$serial/serial-sha256:$(sha256_prefix "$serial")/g")
  fi
  printf '%s=%s\n' "$1" "$value" >> "$transcript"
}
{
  printf '# OnePlus read-only discovery, %s UTC, host %s\n' "$stamp" "$(uname -s) $(uname -m)"
  printf '# Serial numbers are replaced by a SHA-256 prefix. Never commit this file.\n'
} > "$transcript"

adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
adb_count=$(printf '%s\n' "$adb_rows" | awk 'NF { count++ } END { print count + 0 }')
fastboot_count=$(printf '%s\n' "$fastboot_rows" | awk 'NF { count++ } END { print count + 0 }')
total=$((adb_count + fastboot_count))
emit adb_devices "$adb_count"
emit fastboot_devices "$fastboot_count"

if [ "$total" -eq 0 ]; then
  emit result no-device
  echo "No ADB or Fastboot device is visible." >&2
  echo "Use a data-capable cable and either:" >&2
  echo "  - unlock Android, enable USB debugging, select a data USB mode, and accept the RSA prompt; or" >&2
  echo "  - enter the bootloader's Fastboot mode." >&2
  echo "transcript: $transcript" >&2
  exit 2
fi

if [ "$total" -ne 1 ]; then
  emit result ambiguous
  echo "Refusing ambiguous discovery: $adb_count ADB and $fastboot_count Fastboot devices." >&2
  echo "transcript: $transcript" >&2
  exit 3
fi

if [ "$adb_count" -eq 1 ]; then
  serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1 }')
  state=$(printf '%s\n' "$adb_rows" | awk '{ print $2 }')
  emit transport adb
  emit serial "$serial"
  emit serial_sha256_prefix "$(sha256_prefix "$serial")"
  emit state "$state"
  if [ "$state" != "device" ]; then
    emit result adb-unauthorized
    echo "ADB is not authorized and no properties were queried." >&2
    echo "transcript: $transcript" >&2
    exit 4
  fi

  for prop in \
    ro.product.manufacturer \
    ro.product.model \
    ro.product.vendor.model \
    ro.product.device \
    ro.product.vendor.device \
    ro.product.board \
    ro.hardware \
    ro.hardware.chipname \
    ro.boot.hardware.revision \
    ro.boot.hardware.sku \
    ro.boot.bootloader \
    ro.boot.slot_suffix \
    ro.boot.verifiedbootstate \
    ro.boot.flash.locked \
    ro.boot.dynamic_partitions \
    ro.build.fingerprint \
    ro.build.version.release \
    ro.build.version.security_patch; do
    value=$(adb -s "$serial" shell getprop "$prop" </dev/null | tr -d '\r')
    emit "$prop" "$value"
  done
  emit result ok
  echo "transcript (serial redacted): $transcript" >&2
  exit 0
fi

serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
emit transport fastboot
emit serial "$serial"
emit serial_sha256_prefix "$(sha256_prefix "$serial")"
for var in product hw-revision current-slot slot-count unlocked secure is-userspace; do
  # Fastboot writes getvar responses to stderr on many versions.
  value=$(fastboot -s "$serial" getvar "$var" 2>&1 | awk -v key="$var" '
    index($0, key ":") { sub(".*" key ":[[:space:]]*", ""); print; exit }
  ')
  emit "$var" "$value"
done
emit result ok
echo "transcript (serial redacted): $transcript" >&2
