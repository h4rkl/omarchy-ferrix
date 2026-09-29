#!/bin/sh
set -eu

# Read-only live partition and boot-state inventory for the exact Pixel 9a.
# The only device verbs below are ADB property/file reads and Fastboot getvar
# reads. The generated transcript is redacted and ignored.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
out=${FERRIX_OUT_DIR:-"$root/out"}/pixel-9a
expected_product=tegu
expected_revision=MP1.0
expected_sku=GXQ96
expected_target_id=google-pixel-9a-tegu-gxq96-mp1-0

die() {
  printf 'REFUSING partition inventory: %s\n' "$*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing $1; install Android Platform Tools"
}

sha256_prefix() {
  if command -v shasum >/dev/null 2>&1; then
    printf '%s' "$1" | shasum -a 256 | cut -c1-12
  else
    printf '%s' "$1" | sha256sum | cut -c1-12
  fi
}

redact() {
  REDACT_NEEDLE=$1 REDACT_REPLACEMENT=$2 awk '
    BEGIN { needle = ENVIRON["REDACT_NEEDLE"]; replacement = ENVIRON["REDACT_REPLACEMENT"] }
    {
      if (needle == "") { print; next }
      output = ""
      rest = $0
      while ((position = index(rest, needle)) != 0) {
        output = output substr(rest, 1, position - 1) replacement
        rest = substr(rest, position + length(needle))
      }
      print output rest
    }
  '
}

mkdir -p "$out"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
transcript="$out/partition-inventory-$stamp.txt"
serial=''
{
  printf '# Pixel 9a read-only partition inventory, %s UTC\n' "$stamp"
  printf '# Serial numbers are replaced by a SHA-256 prefix. Never commit this file.\n'
} > "$transcript"

emit() {
  key=$1
  value=$2
  printf '%s=%s\n' "$key" "$value"
  if [ -n "$serial" ]; then
    value=$(printf '%s\n' "$value" | redact "$serial" "serial-sha256:$(sha256_prefix "$serial")")
  fi
  printf '%s=%s\n' "$key" "$value" >> "$transcript"
}

adb_prop() {
  adb -s "$serial" shell getprop "$1" </dev/null | tr -d '\r'
}

count_rows() {
  printf '%s\n' "$1" | awk 'NF { count++ } END { print count + 0 }'
}

read_remote_file() {
  remote_path=$1
  value=$(adb -s "$serial" shell cat "$remote_path" </dev/null 2>/dev/null | tr -d '\r')
  if [ -n "$value" ]; then
    printf '%s\n' "$value"
  else
    printf '%s\n' 'unavailable:permission-denied-or-empty'
  fi
}

need adb
need fastboot
if command -v shasum >/dev/null 2>&1; then :; else need sha256sum; fi

adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
adb_count=$(count_rows "$adb_rows")
fastboot_count=$(count_rows "$fastboot_rows")
total=$((adb_count + fastboot_count))
emit adb_devices "$adb_count"
emit fastboot_devices "$fastboot_count"
emit target_id "$expected_target_id"
[ "$total" -eq 1 ] || die "expected one visible transport, found adb=$adb_count fastboot=$fastboot_count"

if [ "$adb_count" -eq 1 ]; then
  serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1 }')
  state=$(printf '%s\n' "$adb_rows" | awk '{ print $2 }')
  emit transport adb
  emit serial "$serial"
  emit serial_sha256_prefix "$(sha256_prefix "$serial")"
  [ "$state" = device ] || die "ADB state is '$state', not device"

  product=$(adb_prop ro.product.device)
  model=$(adb_prop ro.product.model)
  revision=$(adb_prop ro.boot.hardware.revision)
  sku=$(adb_prop ro.boot.hardware.sku)
  slot=$(adb_prop ro.boot.slot_suffix)
  slot_count=$(adb_prop ro.boot.slot_count)
  slot_count_source=property
  has_slot_a=unobserved
  has_slot_b=unobserved
  bootloader=$(adb_prop ro.boot.bootloader)
  verified=$(adb_prop ro.boot.verifiedbootstate)
  device_state=$(adb_prop ro.boot.vbmeta.device_state)
  dynamic=$(adb_prop ro.boot.dynamic_partitions)
  boot_devices=$(adb_prop ro.boot.boot_devices)
  [ "$product" = "$expected_product" ] || die "product mismatch: $product"
  [ "$revision" = "$expected_revision" ] || die "hardware revision mismatch: $revision"
  [ "$sku" = "$expected_sku" ] || die "SKU mismatch: $sku"
  case "$slot" in _a|_b) ;; *) die "unexpected current slot: $slot" ;; esac
  # Never trust the slot-count property on its own.  A stale, spoofed, or
  # incomplete property must not turn an unverified partition map into an
  # A/B admission gate.  Require an independent read-only by-name listing to
  # expose both suffixes even when ro.boot.slot_count is already "2".
  partition_names=$(adb -s "$serial" shell ls -1 /dev/block/by-name </dev/null | tr -d '\r')
  has_slot_a=$(printf '%s\n' "$partition_names" | awk '$0 ~ /(_a|\.a)$/ { found=1 } END { exit(found ? 0 : 1) }' && printf yes || printf no)
  has_slot_b=$(printf '%s\n' "$partition_names" | awk '$0 ~ /(_b|\.b)$/ { found=1 } END { exit(found ? 0 : 1) }' && printf yes || printf no)
  # Some toybox builds render the listing as a single line or expose only
  # symlink targets with `ls -1`; use the long listing as a second read-only
  # observation before refusing an unobservable topology.
  if [ "$has_slot_a" != yes ] || [ "$has_slot_b" != yes ]; then
    partition_long=$(adb -s "$serial" shell ls -l /dev/block/by-name </dev/null | tr -d '\r')
    has_slot_a=$(printf '%s\n' "$partition_long" | awk '{ for (i = 1; i <= NF; i++) if ($i ~ /_a$/ || $i ~ /\.a$/) found=1 } END { print found ? "yes" : "no" }')
    has_slot_b=$(printf '%s\n' "$partition_long" | awk '{ for (i = 1; i <= NF; i++) if ($i ~ /_b$/ || $i ~ /\.b$/) found=1 } END { print found ? "yes" : "no" }')
    [ "$has_slot_a" = yes ] && [ "$has_slot_b" = yes ] && partition_names=$partition_long
  fi
  [ "$has_slot_a" = yes ] && [ "$has_slot_b" = yes ] ||
    die "read-only /dev/block/by-name map does not expose both A/B slot suffixes (slot-map-a=$has_slot_a slot-map-b=$has_slot_b)"
  if [ -z "$slot_count" ]; then
    slot_count=2
    slot_count_source=block-by-name
  else
    [ "$slot_count" = 2 ] || die "unexpected slot count: $slot_count (slot-map-a=$has_slot_a slot-map-b=$has_slot_b)"
  fi
  emit product "$product"
  emit model "$model"
  emit hardware_revision "$revision"
  emit sku "$sku"
  emit current_slot "$slot"
  emit slot_count "$slot_count"
  emit slot_count_source "$slot_count_source"
  emit slot_map_a "$has_slot_a"
  emit slot_map_b "$has_slot_b"
  emit bootloader "$bootloader"
  emit verified_boot_state "$verified"
  emit vbmeta_device_state "$device_state"
  emit dynamic_partitions "$dynamic"
  emit boot_devices "$boot_devices"
  emit partitions "$(adb -s "$serial" shell ls -l /dev/block/by-name </dev/null | tr -d '\r')"
  block_devices=$(read_remote_file /proc/partitions)
  mounts=$(read_remote_file /proc/mounts)
  cmdline=$(read_remote_file /proc/cmdline)
  inventory_completeness=complete
  case "$block_devices:$mounts:$cmdline" in
    *unavailable:permission-denied-or-empty*) inventory_completeness=partial ;;
  esac
  emit block_devices "$block_devices"
  emit mounts "$mounts"
  emit cmdline "$cmdline"
  emit inventory_completeness "$inventory_completeness"
  emit result ok
  printf 'transcript=%s\n' "$transcript" >&2
  exit 0
fi

serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
emit transport fastboot
emit serial "$serial"
emit serial_sha256_prefix "$(sha256_prefix "$serial")"
fastboot_getvar() {
  variable=$1
  fastboot -s "$serial" getvar "$variable" 2>&1 | awk -v key="$variable" '
    index($0, key ":") { line = $0; sub(".*" key ":[[:space:]]*", "", line); print line; exit }
  '
}
product=$(fastboot_getvar product)
revision=$(fastboot_getvar hw-revision)
sku=$(fastboot_getvar variant)
slot=$(fastboot_getvar current-slot)
slot_count=$(fastboot_getvar slot-count)
[ "$product" = "$expected_product" ] || die "product mismatch: $product"
[ "$revision" = "$expected_revision" ] || die "hardware revision mismatch: $revision"
[ "$sku" = "$expected_sku" ] || die "SKU/variant mismatch: $sku"
case "$slot" in a|b|_a|_b) ;; *) die "unexpected current slot: $slot" ;; esac
[ "$slot_count" = 2 ] || die "unexpected slot count: $slot_count"
emit product "$product"
emit hardware_revision "$revision"
emit variant "$sku"
emit current_slot "$slot"
emit slot_count "$slot_count"
emit unlocked "$(fastboot_getvar unlocked)"
emit is_userspace "$(fastboot_getvar is-userspace)"
emit all_read_only "$(fastboot -s "$serial" getvar all 2>&1)"
emit result ok
printf 'transcript=%s\n' "$transcript" >&2
