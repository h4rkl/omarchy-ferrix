#!/bin/sh
# Provisioning gate SKELETON for the attached OnePlus device.
#
# What it does: resolves and prints the exact device serial, product/codename,
# hardware revision, current slot, the requested partition target, the input
# image hash, and the recovery artifact path and hash; refuses zero or
# multiple devices; requires the typed per-run confirmation
#     I have verified <serial-prefix> <codename> <revision>
# on stdin; and then EXITS 3 with {"event":"provisioning-refused",
# "reason":"no-approved-proposal"} because no board-support proposal has
# passed its identity, image-compatibility, backup, and recovery gates
# (docs/board-support/oneplus-attached.md).
#
# What it never does: unlock, erase, format, flash, boot an image, change
# slots, or reboot. The only device commands are `adb devices`,
# `adb shell getprop`, `fastboot devices`, and `fastboot getvar`;
# tools/oneplus/tests/provision-gate-is-inert.sh proves that with stubs.
set -eu

usage() {
  cat >&2 <<'USAGE'
usage: provision-gate.sh --image PATH --recovery PATH --partition NAME
                         [--expect-codename NAME] [--expect-revision REV]
Reads the confirmation phrase from stdin. Exit codes: 2 usage/no device,
3 refused (no approved proposal), 4 identity/confirmation mismatch,
5 ambiguous devices.
USAGE
  exit 2
}

image=""
recovery=""
partition=""
expect_codename=""
expect_revision=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --image) [ "$#" -ge 2 ] || usage; image=$2; shift 2 ;;
    --recovery) [ "$#" -ge 2 ] || usage; recovery=$2; shift 2 ;;
    --partition) [ "$#" -ge 2 ] || usage; partition=$2; shift 2 ;;
    --expect-codename) [ "$#" -ge 2 ] || usage; expect_codename=$2; shift 2 ;;
    --expect-revision) [ "$#" -ge 2 ] || usage; expect_revision=$2; shift 2 ;;
    -h|--help) usage ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done
[ -n "$image" ] && [ -n "$recovery" ] && [ -n "$partition" ] || usage
case "$partition" in
  *[!a-z0-9_]*|'') echo "partition target must be lowercase [a-z0-9_]: $partition" >&2; usage ;;
esac

refuse() {
  printf '{"event":"provisioning-refused","reason":"%s"}\n' "$1"
  exit "$2"
}

for tool in adb fastboot; do
  command -v "$tool" >/dev/null 2>&1 || refuse "missing-$tool" 2
done
[ -f "$image" ] || refuse "image-missing" 2
[ -f "$recovery" ] || refuse "recovery-artifact-missing" 2

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    sha256sum "$1" | cut -d' ' -f1
  fi
}
image_sha=$(sha256_of "$image")
recovery_sha=$(sha256_of "$recovery")

# --- Read-only device resolution ------------------------------------------
adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
adb_count=$(printf '%s\n' "$adb_rows" | awk 'NF { count++ } END { print count + 0 }')
fastboot_count=$(printf '%s\n' "$fastboot_rows" | awk 'NF { count++ } END { print count + 0 }')
total=$((adb_count + fastboot_count))
[ "$total" -ne 0 ] || refuse "no-device" 2
[ "$total" -eq 1 ] || refuse "multiple-devices" 5

if [ "$adb_count" -eq 1 ]; then
  transport=adb
  serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1 }')
  state=$(printf '%s\n' "$adb_rows" | awk '{ print $2 }')
  [ "$state" = device ] || refuse "adb-unauthorized" 2
  getprop() { adb -s "$serial" shell getprop "$1" </dev/null | tr -d '\r'; }
  codename=$(getprop ro.product.device)
  model=$(getprop ro.product.model)
  revision=$(getprop ro.boot.hardware.revision)
  slot=$(getprop ro.boot.slot_suffix)
  locked=$(getprop ro.boot.flash.locked)
else
  transport=fastboot
  serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
  getvar() {
    fastboot -s "$serial" getvar "$1" 2>&1 | awk -v key="$1" '
      index($0, key ":") { sub(".*" key ":[[:space:]]*", ""); print; exit }
    '
  }
  codename=$(getvar product)
  model=""
  revision=$(getvar hw-revision)
  slot=$(getvar current-slot)
  locked=$(getvar unlocked)
fi
[ -n "$codename" ] || refuse "codename-unresolved" 4
[ -n "$revision" ] || refuse "revision-unresolved" 4
if [ -n "$expect_codename" ] && [ "$expect_codename" != "$codename" ]; then
  refuse "codename-mismatch" 4
fi
if [ -n "$expect_revision" ] && [ "$expect_revision" != "$revision" ]; then
  refuse "revision-mismatch" 4
fi
serial_prefix=$(printf '%s' "$serial" | cut -c1-6)

# The operator must see exactly what would be touched. Stdout only; nothing
# here is written to a file.
cat <<RESOLVED
transport=$transport
serial=$serial
serial_prefix=$serial_prefix
product_codename=$codename
model=$model
hardware_revision=$revision
current_slot=$slot
bootloader_locked=$locked
partition_target=$partition
image=$image
image_sha256=$image_sha
recovery_artifact=$recovery
recovery_sha256=$recovery_sha
RESOLVED

expected="I have verified $serial_prefix $codename $revision"
printf 'Type exactly: %s\n> ' "$expected" >&2
IFS= read -r typed || typed=""
[ "$typed" = "$expected" ] || refuse "confirmation-mismatch" 4

# No proposal has passed the identity, image-compatibility, backup, and
# recovery gates. Until docs/board-support/oneplus-attached.md records that
# with reviewed evidence, this gate refuses unconditionally.
refuse "no-approved-proposal" 3
