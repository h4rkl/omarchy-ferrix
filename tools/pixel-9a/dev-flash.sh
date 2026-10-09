#!/bin/sh
set -eu

# Recovery-invariant Pixel 9a development flash route.
#
# Owner policy (2026-10-09): for this exact development device the only hard
# requirement is that the pinned stock factory restore stays possible. This
# workflow therefore does not consult Ferrix image-compatibility, candidate,
# or boot-observation evidence. It enforces only invariants that keep the
# bootloader reachable and the factory image able to rewrite everything:
#
#   - exactly one Fastboot device and no ADB device; exact tegu/MP1.0/GXQ96
#   - bootloader-mode Fastboot (not fastbootd) and unlocked=yes
#   - tested stock recovery in the target manifest and the pinned factory
#     archive present with its exact size and SHA-256
#   - writes only to partitions the pinned factory image rewrites
#   - never bootloader/radio writes except the pinned stock images, never
#     relock, erase, format, oem, or persist/efs/modem-calibration writes
#   - a slot switch first installs the pinned stock bootloader and radio on
#     the target slot, so the switch can never land on an older bootloader
#
# Every mutating mode prints the exact serial, identity, slot, partition
# targets, input hashes, and recovery artifact, then requires a per-run token
# bound to all of them. Running without --confirm is a read-only plan that
# prints the token. Agents may supply the token themselves under the owner
# policy; a changed device, image, or plan changes the token.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
. "$root/tools/pixel-9a/adb-correlation.sh"
target=${FERRIX_PIXEL9A_TARGET_DIR:-"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"}
pins=${FERRIX_PIXEL9A_PINS:-"$target/recovery.pins"}
recovery="$target/recovery"
target_manifest="$target/manifest.toml"
out_root=${FERRIX_OUT_DIR:-"$root/out"}
out="$out_root/pixel-9a/dev-flash"

expected_product=tegu
expected_revision=MP1.0
expected_sku=GXQ96
required_bootloader=tegu-17.0-15819938
required_baseband=g5300t-260604-260807-B-16036236

# Partitions rewritten by the pinned factory image (fastboot-info.txt plus the
# -w wipe). Everything else is refused, including pvmfw, which has no Ferrix
# use. Logical partitions are refused because they need fastbootd.
slotted_partitions='boot init_boot vendor_boot vendor_kernel_boot dtbo vbmeta vbmeta_system vbmeta_vendor'
unslotted_partitions='userdata metadata'

usage() {
  cat >&2 <<'EOF'
usage:
  tools/pixel-9a/dev-flash.sh preflight
  tools/pixel-9a/dev-flash.sh flash [--confirm TOKEN] PARTITION=IMAGE...
  tools/pixel-9a/dev-flash.sh boot [--confirm TOKEN] IMAGE
  tools/pixel-9a/dev-flash.sh set-active [--confirm TOKEN] a|b
  tools/pixel-9a/dev-flash.sh reboot [bootloader]

PARTITION is one of boot init_boot vendor_boot vendor_kernel_boot dtbo vbmeta
vbmeta_system vbmeta_vendor (optionally suffixed _a or _b; unsuffixed means
the current slot), userdata, or metadata.

Without --confirm, a mutating mode is a read-only plan that prints
confirm_token. Rerun the identical command with --confirm TOKEN to execute.
set-active first flashes the pinned stock bootloader and radio to the target
slot, then selects it. Restore stock with: just pixel-9a-factory-restore.
EOF
  exit 2
}

die() {
  printf 'REFUSING: %s\n' "$*" >&2
  record "refused=$*"
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing $1; install Android Platform Tools"
}

transcript=''
record() {
  [ -n "$transcript" ] || return 0
  if [ -n "${serial:-}" ]; then
    printf '%s\n' "$1" | awk -v s="$serial" -v r="$serial_prefix" '{ gsub(s, r); print }' >> "$transcript"
  else
    printf '%s\n' "$1" >> "$transcript"
  fi
}

emit() {
  printf '%s=%s\n' "$1" "$2"
  record "$1=$2"
}

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    sha256sum "$1" | awk '{ print $1 }'
  fi
}

sha256_text() {
  if command -v shasum >/dev/null 2>&1; then
    printf '%s' "$1" | shasum -a 256 | awk '{ print $1 }'
  else
    printf '%s' "$1" | sha256sum | awk '{ print $1 }'
  fi
}

count_rows() {
  printf '%s\n' "$1" | awk 'NF { count++ } END { print count + 0 }'
}

manifest_gate() {
  awk -v wanted="$1" '
    /^\[gates\][[:space:]]*$/ { in_gates=1; next }
    /^\[/ { in_gates=0 }
    in_gates && $0 ~ "^[[:space:]]*" wanted "[[:space:]]*=" {
      value=$0
      sub(/^[^=]*=[[:space:]]*/, "", value)
      gsub(/[[:space:]"]/, "", value)
      print value
      exit
    }
  ' "$target_manifest"
}

fastboot_getvar() {
  fastboot -s "$serial" getvar "$1" 2>&1 | awk -v key="$1" '
    index($0, "(bootloader) " key ":") {
      line = $0
      sub(".*\\(bootloader\\) " key ":[[:space:]]*", "", line)
      print line
      exit
    }
    index($0, key ":") == 1 {
      line = $0
      sub("^" key ":[[:space:]]*", "", line)
      print line
      exit
    }
  '
}

# --- recovery invariant -------------------------------------------------------

verify_recovery() {
  [ -f "$target_manifest" ] || die "missing target manifest: $target_manifest"
  [ "$(manifest_gate identity)" = passed ] || die 'target identity gate is not passed'
  case "$(manifest_gate recovery_procedure)" in
    tested|passed|verified) ;;
    *) die 'stock recovery is not tested for this exact target; run the factory-restore first test before any Ferrix write' ;;
  esac
  factory_name=$(awk -F '\t' '$1 == "factory" { print $4; exit }' "$pins")
  factory_expected_hash=$(awk -F '\t' '$1 == "factory" { print $6; exit }' "$pins")
  factory_expected_bytes=$(awk -F '\t' '$1 == "factory" { print $7; exit }' "$pins")
  [ -n "$factory_name" ] && [ -n "$factory_expected_hash" ] && [ -n "$factory_expected_bytes" ] ||
    die "malformed factory recovery pin in $pins"
  factory_path="$recovery/$factory_name"
  [ -f "$factory_path" ] && [ ! -L "$factory_path" ] ||
    die "pinned factory recovery archive is missing: $factory_path; run just pixel-9a-recovery-download"
  factory_bytes=$(wc -c < "$factory_path" | tr -d '[:space:]')
  [ "$factory_bytes" = "$factory_expected_bytes" ] ||
    die "factory archive size $factory_bytes differs from pinned $factory_expected_bytes"
  factory_hash=$(sha256_of "$factory_path")
  [ "$factory_hash" = "$factory_expected_hash" ] || die 'factory archive SHA-256 differs from the pin'
  emit recovery_artifact "$factory_path"
  emit recovery_artifact_sha256 "$factory_hash"
  emit recovery_command 'just pixel-9a-factory-restore --confirm-restore'
}

# --- exact device ---------------------------------------------------------------

read_device() {
  adb_rows=$(adb devices 2>/dev/null | awk '!/^\*/ && !/^List of devices/ && NF { print }' || true)
  fastboot_rows=$(fastboot devices 2>/dev/null | awk 'NF { print }')
  adb_count=$(count_rows "$adb_rows")
  fastboot_count=$(count_rows "$fastboot_rows")
  [ "$adb_count" -eq 0 ] && [ "$fastboot_count" -eq 1 ] ||
    die "need exactly one Fastboot device and no ADB device (adb=$adb_count fastboot=$fastboot_count); use just pixel-9a-enter-fastboot"
  observed_serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
  if [ -n "${serial:-}" ]; then
    [ "$observed_serial" = "$serial" ] || die 'Fastboot serial changed during this run'
  fi
  serial=$observed_serial
  serial_prefix=$(sha256_text "$serial" | cut -c1-12)
  product=$(fastboot_getvar product)
  revision=$(fastboot_getvar hw-revision)
  variant=$(fastboot_getvar variant)
  unlocked=$(fastboot_getvar unlocked)
  userspace=$(fastboot_getvar is-userspace)
  current_slot=$(fastboot_getvar current-slot | sed 's/^_//')
  bootloader=$(fastboot_getvar version-bootloader)
  baseband=$(fastboot_getvar version-baseband)
  [ "$product" = "$expected_product" ] || die "Fastboot product is '$product', not $expected_product"
  [ "$revision" = "$expected_revision" ] || die "Fastboot hardware revision is '$revision', not $expected_revision"
  pixel9a_fastboot_sku_gate "$variant" "$serial_prefix" ||
    die "Fastboot variant '$variant' does not prove $expected_sku and no matching ADB discovery exists; run just pixel-9a-discover"
  [ "$userspace" = no ] || die 'device is in fastbootd; reboot to bootloader Fastboot'
  [ "$unlocked" = yes ] || die "bootloader reports unlocked='$unlocked'; refusing (a relocked device cannot be reflashed)"
  case "$current_slot" in a|b) ;; *) die "unexpected current slot '$current_slot'" ;; esac
}

print_device() {
  emit serial "$serial"
  emit serial_sha256_prefix "$serial_prefix"
  emit product "$product"
  emit hardware_revision "$revision"
  emit sku "$expected_sku"
  emit sku_source "$pixel9a_sku_source"
  emit unlocked "$unlocked"
  emit current_slot "$current_slot"
  emit bootloader_version "$bootloader"
  emit baseband_version "$baseband"
}

# --- plan -----------------------------------------------------------------------

in_list() {
  for item in $2; do [ "$item" = "$1" ] && return 0; done
  return 1
}

resolve_partition() {
  requested=$1
  base=${requested%_[ab]}
  if in_list "$requested" "$unslotted_partitions"; then
    resolved=$requested
  elif in_list "$base" "$slotted_partitions"; then
    if [ "$base" = "$requested" ]; then
      resolved="${base}_$current_slot"
    else
      resolved=$requested
    fi
  else
    case "$base" in
      bootloader|radio|abl|bl1|bl2|bl31|pbl|gsa|gcf|tzsw|ldfw|modem|dpm|ufs|keystorage|pvmfw)
        die "partition '$requested' is boot/radio firmware; only the pinned factory restore may write it" ;;
      persist|efs|efs_backup|efs_cache|modem_userdata|devinfo|frp|misc|fips|klog|mfg_data|vbmeta_none)
        die "partition '$requested' holds device-unique or bootloader state the factory image cannot restore" ;;
      super|system|system_ext|system_dlkm|product|vendor|vendor_dlkm)
        die "partition '$requested' is logical or super; this route stays in bootloader Fastboot" ;;
      *) die "partition '$requested' is not rewritten by the pinned factory image" ;;
    esac
  fi
  if in_list "$resolved" "$unslotted_partitions"; then
    [ "$(manifest_gate data_policy)" = dev-device-no-user-data ] ||
      die "writing $resolved requires the checked-in dev-device-no-user-data policy"
  fi
}

check_image() {
  [ -f "$1" ] && [ ! -L "$1" ] || die "image must be a regular non-symlink file: $1"
  image_bytes=$(wc -c < "$1" | tr -d '[:space:]')
  [ "$image_bytes" -gt 0 ] || die "image is empty: $1"
  image_hash=$(sha256_of "$1")
}

check_fits() {
  size_hex=$(fastboot_getvar "partition-size:$1")
  case "$size_hex" in
    0x*|0X*) [ "$image_bytes" -le "$((size_hex))" ] || die "$2 ($image_bytes bytes) exceeds partition $1 ($((size_hex)) bytes)" ;;
    *) die "could not read the size of partition $1" ;;
  esac
}

plan=''
add_plan() {
  plan="$plan$1
"
  emit plan_step "$1"
}

stage_dir=''
cleanup() {
  [ -z "$stage_dir" ] || rm -rf "$stage_dir"
}
trap cleanup EXIT HUP INT TERM

stage_firmware() {
  # Same stock bootloader/radio the factory restore flashes. unzip checks
  # member CRCs; the outer archive SHA-256 is already verified.
  need unzip
  mkdir -p "$out"
  stage_dir=$(mktemp -d "$out/stage-XXXXXX")
  prefix=$(unzip -Z1 "$factory_path" | awk -F/ 'NR == 1 { print $1; exit }')
  bootloader_member=$(unzip -Z1 "$factory_path" | grep -E "^$prefix/bootloader-tegu-.*\.img$" || true)
  radio_member=$(unzip -Z1 "$factory_path" | grep -E "^$prefix/radio-tegu-.*\.img$" || true)
  [ "$(count_rows "$bootloader_member")" -eq 1 ] && [ "$(count_rows "$radio_member")" -eq 1 ] ||
    die 'factory archive does not contain exactly one bootloader and one radio image'
  case "$(printf '%s' "$bootloader_member" | tr 'A-Z' 'a-z')" in
    *"$(printf '%s' "$required_bootloader" | tr 'A-Z' 'a-z')"*) ;;
    *) die "factory bootloader member does not name $required_bootloader" ;;
  esac
  case "$(printf '%s' "$radio_member" | tr 'A-Z' 'a-z')" in
    *"$(printf '%s' "$required_baseband" | tr 'A-Z' 'a-z')"*) ;;
    *) die "factory radio member does not name $required_baseband" ;;
  esac
  unzip -q -o -j "$factory_path" "$bootloader_member" "$radio_member" -d "$stage_dir" ||
    die 'could not extract stock firmware from the factory archive'
  stock_bootloader="$stage_dir/${bootloader_member##*/}"
  stock_radio="$stage_dir/${radio_member##*/}"
  stock_bootloader_hash=$(sha256_of "$stock_bootloader")
  stock_radio_hash=$(sha256_of "$stock_radio")
}

# Re-read the device immediately before each state change.
recheck() {
  read_device
  [ "$current_slot" = "$planned_slot" ] || die 'current slot changed during this run'
}

run_fastboot() {
  recheck
  emit mutation "fastboot $*"
  fastboot_status=0
  fastboot -s "$serial" "$@" > "$out/.fastboot-$$.log" 2>&1 || fastboot_status=$?
  cat "$out/.fastboot-$$.log" >&2
  record "$(cat "$out/.fastboot-$$.log")"
  rm -f "$out/.fastboot-$$.log"
  [ "$fastboot_status" -eq 0 ] ||
    die "fastboot $* exited $fastboot_status; device stays in bootloader Fastboot; restore with just pixel-9a-factory-restore if needed"
}

# --- main -----------------------------------------------------------------------

[ "$#" -ge 1 ] || usage
mode=$1
shift
confirm=''
if [ "${1:-}" = --confirm ]; then
  [ "$#" -ge 2 ] || usage
  confirm=$2
  shift 2
fi

case "$mode" in
  preflight) [ "$#" -eq 0 ] || usage ;;
  flash) [ "$#" -ge 1 ] || usage ;;
  boot) [ "$#" -eq 1 ] || usage ;;
  set-active) [ "$#" -eq 1 ] && case "$1" in a|b) true ;; *) false ;; esac || usage ;;
  reboot) [ "$#" -le 1 ] && case "${1:-}" in ''|bootloader) true ;; *) false ;; esac || usage ;;
  *) usage ;;
esac

need adb
need fastboot
mkdir -p "$out"
transcript="$out/$mode-$(date -u +%Y%m%dT%H%M%SZ)-$$.txt"
record "operation=pixel-9a-dev-flash-$mode"
record 'target_id=google-pixel-9a-tegu-gxq96-mp1-0'

if [ "$mode" = reboot ]; then
  read_device
  print_device
  if [ "${1:-}" = bootloader ]; then
    emit mutation "fastboot reboot-bootloader"
    fastboot -s "$serial" reboot-bootloader >&2
  else
    emit mutation "fastboot reboot"
    fastboot -s "$serial" reboot >&2
  fi
  emit result rebooted
  exit 0
fi

verify_recovery
read_device
print_device
planned_slot=$current_slot

case "$mode" in
  preflight)
    emit preflight passed-read-only
    emit transcript "$transcript"
    exit 0
    ;;
  flash)
    for pair in "$@"; do
      case "$pair" in *=*) ;; *) usage ;; esac
      resolve_partition "${pair%%=*}"
      check_image "${pair#*=}"
      check_fits "$resolved" "${pair#*=}"
      add_plan "flash $resolved ${pair#*=} bytes=$image_bytes sha256=$image_hash"
    done
    ;;
  boot)
    check_image "$1"
    add_plan "boot $1 bytes=$image_bytes sha256=$image_hash (temporary; no storage write)"
    ;;
  set-active)
    target_slot=$1
    [ "$target_slot" != "$current_slot" ] || die "slot $target_slot is already current"
    [ "$(printf '%s' "$bootloader" | tr 'A-Z' 'a-z')" = "$(printf '%s' "$required_bootloader" | tr 'A-Z' 'a-z')" ] ||
      die "current bootloader '$bootloader' is not pinned stock $required_bootloader; run the factory restore first"
    [ "$(printf '%s' "$baseband" | tr 'A-Z' 'a-z')" = "$(printf '%s' "$required_baseband" | tr 'A-Z' 'a-z')" ] ||
      die "current baseband '$baseband' is not pinned stock $required_baseband; run the factory restore first"
    stage_firmware
    add_plan "flash --slot=$target_slot bootloader stock sha256=$stock_bootloader_hash"
    add_plan "flash --slot=$target_slot radio stock sha256=$stock_radio_hash"
    add_plan "set_active $target_slot"
    ;;
esac

token="DEVFLASH-$(sha256_text "$mode|$serial|$planned_slot|$factory_hash|$plan" | cut -c1-16)"
emit confirm_token "$token"
if [ -z "$confirm" ]; then
  emit result plan-only-no-mutation
  emit transcript "$transcript"
  exit 0
fi
[ "$confirm" = "$token" ] || die 'confirm token does not match this exact device and plan; no mutation performed'
record 'confirmation=accepted'

case "$mode" in
  flash)
    for pair in "$@"; do
      resolve_partition "${pair%%=*}"
      check_image "${pair#*=}"
      printf '%s' "$plan" | grep -Fq "flash $resolved ${pair#*=} bytes=$image_bytes sha256=$image_hash" ||
        die "image ${pair#*=} changed after planning"
      run_fastboot flash "$resolved" "${pair#*=}"
    done
    emit result flashed
    ;;
  boot)
    check_image "$1"
    printf '%s' "$plan" | grep -Fq "sha256=$image_hash" || die "image $1 changed after planning"
    run_fastboot boot "$1"
    emit result temporary-boot-issued
    ;;
  set-active)
    [ "$(sha256_of "$stock_bootloader")" = "$stock_bootloader_hash" ] || die 'staged stock bootloader changed'
    run_fastboot "--slot=$target_slot" flash bootloader "$stock_bootloader"
    [ "$(sha256_of "$stock_radio")" = "$stock_radio_hash" ] || die 'staged stock radio changed'
    run_fastboot "--slot=$target_slot" flash radio "$stock_radio"
    run_fastboot set_active "$target_slot"
    after=$(fastboot_getvar current-slot | sed 's/^_//')
    emit current_slot_after "$after"
    [ "$after" = "$target_slot" ] || die "set_active returned but current slot is '$after'"
    emit result slot-selected
    ;;
esac
emit transcript "$transcript"
