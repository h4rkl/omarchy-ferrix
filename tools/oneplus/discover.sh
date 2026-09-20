#!/bin/sh
set -eu

# Read-only transport and identity discovery. This script deliberately has no
# unlock, reboot, boot, erase, format, flash, or slot-changing operations.

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing $1; install Android Platform Tools" >&2
    exit 1
  }
}

need adb
need fastboot

adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
adb_count=$(printf '%s\n' "$adb_rows" | awk 'NF { count++ } END { print count + 0 }')
fastboot_count=$(printf '%s\n' "$fastboot_rows" | awk 'NF { count++ } END { print count + 0 }')
total=$((adb_count + fastboot_count))

if [ "$total" -eq 0 ]; then
  echo "No ADB or Fastboot device is visible." >&2
  echo "Use a data-capable cable and either:" >&2
  echo "  - unlock Android, enable USB debugging, select a data USB mode, and accept the RSA prompt; or" >&2
  echo "  - enter the bootloader's Fastboot mode." >&2
  exit 2
fi

if [ "$total" -ne 1 ]; then
  echo "Refusing ambiguous discovery: $adb_count ADB and $fastboot_count Fastboot devices." >&2
  exit 3
fi

if [ "$adb_count" -eq 1 ]; then
  serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1 }')
  state=$(printf '%s\n' "$adb_rows" | awk '{ print $2 }')
  echo "transport=adb"
  echo "serial=$serial"
  echo "state=$state"
  if [ "$state" != "device" ]; then
    echo "ADB is not authorized and no properties were queried." >&2
    exit 4
  fi

  for prop in \
    ro.product.manufacturer \
    ro.product.model \
    ro.product.device \
    ro.product.board \
    ro.hardware \
    ro.boot.hardware.revision \
    ro.boot.slot_suffix \
    ro.boot.verifiedbootstate \
    ro.boot.flash.locked \
    ro.boot.dynamic_partitions; do
    value=$(adb -s "$serial" shell getprop "$prop" </dev/null | tr -d '\r')
    printf '%s=%s\n' "$prop" "$value"
  done
  exit 0
fi

serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
echo "transport=fastboot"
echo "serial=$serial"
for var in product hw-revision current-slot slot-count unlocked secure is-userspace; do
  # Fastboot writes getvar responses to stderr on many versions.
  value=$(fastboot -s "$serial" getvar "$var" 2>&1 | awk -v key="$var" '
    index($0, key ":") { sub(".*" key ":[[:space:]]*", ""); print; exit }
  ')
  printf '%s=%s\n' "$var" "$value"
done
