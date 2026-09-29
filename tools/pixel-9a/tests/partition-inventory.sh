#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
workflow="$root/tools/pixel-9a/partition-inventory.sh"

[ -x "$workflow" ] || {
  echo 'partition inventory workflow is not executable' >&2
  exit 1
}
sh -n "$workflow"
if grep -nE '\b(flash|erase|format|reboot|sideload|unlock)\b' "$workflow" >/dev/null; then
  echo 'partition inventory contains a mutating operation' >&2
  exit 1
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-partition-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/bin" "$work/out"
cat > "$work/bin/adb" <<'STUB'
#!/bin/sh
case "$1" in
  devices)
    echo 'List of devices attached'
    printf '%s\n' "${STUB_ADB_DEVICES:-}" | tr '|' '\n'
    ;;
  -s)
    [ "$3" = shell ] || exit 99
    if [ "$4" = getprop ]; then
      case "$5" in
        ro.product.device) echo tegu ;;
        ro.product.model) echo 'Pixel 9a' ;;
        ro.boot.hardware.revision) echo MP1.0 ;;
        ro.boot.hardware.sku) echo GXQ96 ;;
        ro.boot.slot_suffix) echo _a ;;
        ro.boot.slot_count) printf '%s\n' "${STUB_SLOT_COUNT-2}" ;;
        ro.boot.bootloader) echo tegu-test ;;
        ro.boot.verifiedbootstate) echo orange ;;
        ro.boot.vbmeta.device_state) echo unlocked ;;
        ro.boot.dynamic_partitions) echo true ;;
        ro.boot.boot_devices) echo 13200000.ufs ;;
        *) exit 99 ;;
      esac
    elif [ "$4" = ls ]; then
      case "$5" in
        -1)
          if [ "${STUB_MISSING_SLOT_B:-}" = 1 ]; then
            printf '%s\n' 'boot_a'
          else
            printf '%s\n' 'boot_a' 'boot_b'
          fi
          ;;
        *)
          if [ "${STUB_MISSING_SLOT_B:-}" = 1 ]; then
            printf '%s\n' 'lrwxrwxrwx boot_a -> /dev/block/sda1'
          else
            printf '%s\n' 'lrwxrwxrwx boot_a -> /dev/block/sda1' 'lrwxrwxrwx boot_b -> /dev/block/sda2'
          fi
          ;;
      esac
    elif [ "$4" = cat ]; then
      case "$5" in
        /proc/partitions) [ "${STUB_DENY_PROC:-}" = 1 ] && exit 13; echo 'major minor  #blocks  name' ;;
        /proc/mounts) echo '/dev/root / ext4 rw 0 0' ;;
        /proc/cmdline) [ "${STUB_DENY_PROC:-}" = 1 ] && exit 13; echo 'root=PARTLABEL=Ferrix' ;;
        *) exit 99 ;;
      esac
    else
      exit 99
    fi
    ;;
  *) exit 99 ;;
esac
STUB
cat > "$work/bin/fastboot" <<'STUB'
#!/bin/sh
case "$1" in
  devices) printf '%s\n' "${STUB_FASTBOOT_DEVICES:-}" | tr '|' '\n' ;;
  -s)
    [ "$3" = getvar ] || exit 99
    case "$4" in
      product) value=tegu ;;
      hw-revision) value=MP1.0 ;;
      variant) value=GXQ96 ;;
      current-slot) value=a ;;
      slot-count) value=2 ;;
      unlocked) value=yes ;;
      is-userspace) value=no ;;
      all) value='(bootloader) product: tegu' ;;
      *) exit 99 ;;
    esac
    printf '%s: %s\n' "$4" "$value" >&2
    ;;
  *) exit 99 ;;
esac
STUB
chmod +x "$work/bin/adb" "$work/bin/fastboot"
export PATH="$work/bin:$PATH" FERRIX_OUT_DIR="$work/out"

refuse() {
  label=$1
  pattern=$2
  error="$work/$label.err"
  if "$workflow" >"$work/$label.out" 2>"$error"; then
    echo "partition inventory unexpectedly passed: $label" >&2
    exit 1
  fi
  grep -q "$pattern" "$error"
}

STUB_ADB_DEVICES='' STUB_FASTBOOT_DEVICES='' refuse no-transport 'expected one visible transport'
STUB_ADB_DEVICES='SERIAL-A device usb:1|SERIAL-B device usb:2' \
  STUB_FASTBOOT_DEVICES='' refuse multiple-adb 'expected one visible transport'

# A real Pixel Android build can omit ro.boot.slot_count after unlock. The
# workflow must derive the A/B topology from the read-only by-name map.
STUB_ADB_DEVICES='SERIAL-A device usb:1' STUB_FASTBOOT_DEVICES='' STUB_SLOT_COUNT='' \
  "$workflow" >"$work/slot-map.out" 2>"$work/slot-map.err"
grep -q '^slot_count=2$' "$work/slot-map.out"
grep -q '^slot_count_source=block-by-name$' "$work/slot-map.out"
grep -q '^slot_map_a=yes$' "$work/slot-map.out"
grep -q '^slot_map_b=yes$' "$work/slot-map.out"
grep -q '^inventory_completeness=complete$' "$work/slot-map.out"
slot_map_transcript=$(find "$work/out/pixel-9a" -type f -name 'partition-inventory-*.txt' -print | sort | tail -1)
[ -n "$slot_map_transcript" ]
! grep -Fq 'SERIAL-A' "$slot_map_transcript"
grep -q '^serial=serial-sha256:[0-9a-f][0-9a-f]*$' "$slot_map_transcript"

# A populated property remains acceptable only with the same independent map
# evidence; retain the source label so existing consumers can distinguish the
# property-derived count from the by-name-derived fallback.
STUB_ADB_DEVICES='SERIAL-A device usb:1' STUB_FASTBOOT_DEVICES='' STUB_SLOT_COUNT=2 \
  "$workflow" >"$work/property.out" 2>"$work/property.err"
grep -q '^slot_count_source=property$' "$work/property.out"
grep -q '^slot_map_a=yes$' "$work/property.out"
grep -q '^slot_map_b=yes$' "$work/property.out"

# Even an explicit ro.boot.slot_count=2 is insufficient without an
# independent by-name observation of both slot suffixes.  This models a
# contradictory/incomplete map and must fail closed before any flash gate can
# consume the inventory transcript.
STUB_ADB_DEVICES='SERIAL-A device usb:1' STUB_FASTBOOT_DEVICES='' STUB_SLOT_COUNT=2 STUB_MISSING_SLOT_B=1 \
  refuse missing-slot-map 'read-only /dev/block/by-name map does not expose both A/B slot suffixes'
unset STUB_MISSING_SLOT_B

# A contradictory explicit property must refuse with a useful topology
# diagnostic rather than referencing an unset shell variable.
STUB_ADB_DEVICES='SERIAL-A device usb:1' STUB_FASTBOOT_DEVICES='' STUB_SLOT_COUNT=1 \
  refuse bad-slot-property 'unexpected slot count: 1'
unset STUB_SLOT_COUNT

STUB_ADB_DEVICES='SERIAL-A device usb:1' STUB_FASTBOOT_DEVICES='' STUB_DENY_PROC=1 \
  "$workflow" >"$work/partial.out" 2>"$work/partial.err"
grep -q '^inventory_completeness=partial$' "$work/partial.out"
grep -q '^block_devices=unavailable:permission-denied-or-empty$' "$work/partial.out"
grep -q '^cmdline=unavailable:permission-denied-or-empty$' "$work/partial.out"

printf '%s\n' 'pixel-9a partition inventory tests passed'
