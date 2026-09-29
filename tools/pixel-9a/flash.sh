#!/bin/sh
set -eu

# Target-scoped, confirmation-gated temporary boot and persistent flash of the
# composed Ferrix split-boot artifacts on the exact Google Pixel 9a
# (tegu / GXQ96 / MP1.0). It consumes a boot-manifest.tsv written by either the
# canonical Google Kleaf composer or the separately reviewed radio-first split
# composer, re-hashes every artifact, verifies the pinned
# stock recovery archive, resolves exactly one Fastboot device, and prints the
# serial, product, hardware revision, slot, partition targets, and input
# hashes before every state-changing command.
#
# Device verbs used by this file, and nothing else:
#   adb devices -l
#   fastboot devices -l
#   fastboot -s <serial> getvar <variable>
#   fastboot -s <serial> boot <boot.img>
#   fastboot -s <serial> flash <partition_slot> <image>
#   fastboot -s <serial> reboot fastboot
#   fastboot -s <serial> reboot bootloader
# It never issues erase, format, -w, flashing lock/unlock, set_active, oem, or
# a flash to a partition outside the manifest/image-layout mapping.
#
# Modes:
#   preflight  read-only manifest, recovery, identity, and gate checks
#   boot-temp  `fastboot boot boot.img` without a persistent write; writes the
#              redacted evidence transcript that later gates persistent flashing
#   flash      persistent slot-explicit flash of the kernel stage (and, when a
#              flashable rootfs image exists, the rootfs stage)
#
# Temporary-boot limitation (Android boot header v4): `fastboot boot boot.img`
# replaces only the GKI kernel container. The bootloader still combines it with
# the vendor_boot (vendor ramdisk, DTB, bootconfig), init_boot (generic
# ramdisk), and dtbo currently flashed on the device. Until those partitions
# hold Ferrix images, a temporary boot proves only that the bootloader accepts
# and starts the Ferrix kernel; it is not a Linux userspace boot.
#
# AVB assumption (state it, do not hide it): `fastboot getvar secure` reports
# whether the production-signed secure-boot chain is in use, which is `yes`
# on every retail Pixel and does not change with the lock state. Enforcement
# of Android Verified Boot for the partitions Ferrix owns is governed by the
# flashing lock: after `fastboot flashing unlock` (unlocked=yes) the Pixel
# bootloader tolerates vbmeta verification failures and boots in the orange
# "unlocked" state, so unsigned (`avb_mode=NONE`) Ferrix vbmeta images can
# boot. On a locked device they cannot, and relocking a device holding
# unsigned images makes it unbootable. Therefore this workflow requires
# unlocked=yes without exception when avb_mode is NONE, and on a secure=yes
# device it additionally requires the operator's explicit
# --allow-unsigned-avb acknowledgement.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
. "$root/tools/pixel-9a/data-safety.sh"
. "$root/tools/pixel-9a/adb-correlation.sh"
target=${FERRIX_PIXEL9A_TARGET_DIR:-"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"}
pins=${FERRIX_PIXEL9A_PINS:-"$target/recovery.pins"}
recovery_dir="$target/recovery"
layout="$target/image-layout.toml"
deviceinfo="$target/pmos/device-google-tegu/deviceinfo"
target_manifest="$target/manifest.toml"
out_root=${FERRIX_OUT_DIR:-"$root/out"}
image_out=${FERRIX_PIXEL9A_BOOT_OUT:-"$out_root/pixel-9a/images/tegu"}
manifest=${FERRIX_PIXEL9A_BOOT_MANIFEST:-"$image_out/boot-manifest.tsv"}
evidence_dir="$out_root/pixel-9a/flash"
wait_seconds=${FERRIX_PIXEL9A_FLASH_WAIT_SECONDS:-90}
tab=$(printf '\t')

expected_target_id=google-pixel-9a-tegu-gxq96-mp1-0
expected_product=tegu
expected_sku=GXQ96
expected_revision=MP1.0

# Physical partitions the bootloader's Fastboot writes, in flash order. Each is
# flashed under its explicit slot-suffixed name.
physical_artifacts='boot init_boot vendor_boot vendor_kernel_boot dtbo vbmeta vbmeta_system vbmeta_vendor'
# Dynamic partitions inside super; only fastbootd (userspace Fastboot) can
# resolve their logical names.
dynamic_artifacts='vendor_dlkm system_dlkm'
# The only non-slotted partition this workflow will ever accept for a rootfs
# image. postmarketOS Pixel ports place the Linux rootfs in userdata; the
# checked-in deviceinfo must name it explicitly before the stage is allowed.
rootfs_partition_allowlist=userdata

mode=
stage=kernel
slot_override=
allow_unsigned_avb=no
skip_temp_boot_gate=no
serial=
fastboot_serial=
transcript=
result_emitted=no

usage() {
  cat >&2 <<'EOF'
usage:
  tools/pixel-9a/flash.sh preflight [--stage kernel|rootfs|all] [--slot a|b] [--allow-unsigned-avb]
  tools/pixel-9a/flash.sh boot-temp [--allow-unsigned-avb]
  tools/pixel-9a/flash.sh flash [--stage kernel|rootfs|all] [--slot a|b] [--allow-unsigned-avb] [--skip-temp-boot-gate]

preflight   read-only manifest, recovery, exact-identity, and gate checks
boot-temp   temporary `boot` of the manifest boot.img; no persistent write;
            writes the evidence transcript that gates persistent flashing
flash       persistent slot-explicit flash of the manifest images; requires
            FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE=<boot-temp transcript> unless
            --skip-temp-boot-gate is given and named in the typed phrase

--stage     kernel (default): boot, init_boot, vendor_boot, vendor_kernel_boot,
            dtbo, vbmeta, vbmeta_system, vbmeta_vendor in the bootloader, then
            vendor_dlkm and system_dlkm from fastbootd
            rootfs: the flashable rootfs image, only if the manifest has one
            all: kernel stage, then rootfs stage
--slot      target slot; required for flash and must be inactive; preflight
            may omit it to inspect the device's current slot

environment:
  FERRIX_OUT_DIR                     output root (default: <repo>/out)
  FERRIX_PIXEL9A_BOOT_OUT            composed image directory
  FERRIX_PIXEL9A_BOOT_MANIFEST       boot-manifest.tsv path
  FERRIX_PIXEL9A_TARGET_DIR          board package directory
  FERRIX_PIXEL9A_PINS                recovery.pins path
  FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE  boot-temp transcript for the flash gate
  FERRIX_PIXEL9A_FLASH_WAIT_SECONDS  bounded device wait (default 90)
EOF
  exit 2
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

sha256_prefix() {
  if command -v sha256sum >/dev/null 2>&1; then
    printf '%s' "$1" | sha256sum | cut -c1-12
  else
    printf '%s' "$1" | shasum -a 256 | cut -c1-12
  fi
}

bytes_of() {
  wc -c <"$1" | tr -d '[:space:]'
}

count_rows() {
  printf '%s\n' "$1" | awk 'NF { count++ } END { print count + 0 }'
}

# Replace the device serial as a literal byte string (never as a regex).
redact_literal() {
  REDACT_NEEDLE=$1 REDACT_REPLACEMENT=$2 awk '
    BEGIN {
      needle = ENVIRON["REDACT_NEEDLE"]
      replacement = ENVIRON["REDACT_REPLACEMENT"]
    }
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

# Operator stdout keeps the exact serial; the ignored transcript replaces it.
emit() {
  key=$1
  value=$2
  printf '%s=%s\n' "$key" "$value"
  [ -n "$transcript" ] || return 0
  if [ -n "$serial" ]; then
    value=$(printf '%s\n' "$value" | redact_literal "$serial" "serial-sha256:$(sha256_prefix "$serial")")
  fi
  printf '%s=%s\n' "$key" "$value" >>"$transcript"
}

die() {
  printf 'REFUSING: %s\n' "$*" >&2
  if [ -n "$transcript" ]; then
    message=$(printf '%s\n' "$*" | redact_literal "$serial" "serial-sha256:$(sha256_prefix "${serial:-none}")")
    printf 'refused=%s\n' "$message" >>"$transcript"
  fi
  exit 1
}

# A mutation that returned a failure is not a refusal; it is a failed write
# that the operator must resolve with the verified recovery archive.
fail() {
  emit result "$1"
  printf 'FAILED: %s\n' "$2" >&2
  printf '%s\n' 'Restore the exact device with the verified factory archive before normal use.' >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || {
    printf 'missing %s; install Android Platform Tools explicitly\n' "$1" >&2
    exit 1
  }
}

open_transcript() {
  mkdir -p "$evidence_dir"
  stamp=$(date -u +%Y%m%dT%H%M%SZ)
  transcript="$evidence_dir/$mode-$stamp-$$.txt"
  {
    printf '# Pixel 9a %s transcript, %s UTC, host %s\n' "$mode" "$stamp" "$(uname -s) $(uname -m)"
    printf '# Serial numbers are replaced by a SHA-256 prefix. Never commit this file.\n'
  } >"$transcript"
  emit operation "pixel-9a-$mode"
  emit target_id "$expected_target_id"
}

parse_args() {
  [ "$#" -ge 1 ] || usage
  mode=$1
  shift
  case "$mode" in
    preflight|boot-temp|flash) ;;
    *) usage ;;
  esac
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --stage)
        [ "$#" -ge 2 ] || usage
        [ "$mode" != boot-temp ] || usage
        case "$2" in
          kernel|rootfs|all) stage=$2 ;;
          *) usage ;;
        esac
        shift 2
        ;;
      --slot)
        [ "$#" -ge 2 ] || usage
        [ "$mode" != boot-temp ] || usage
        case "$2" in
          a|b) slot_override=$2 ;;
          *) usage ;;
        esac
        shift 2
        ;;
      --allow-unsigned-avb)
        allow_unsigned_avb=yes
        shift
        ;;
      --skip-temp-boot-gate)
        [ "$mode" = flash ] || usage
        skip_temp_boot_gate=yes
        shift
        ;;
      *) usage ;;
    esac
  done
}

manifest_value() {
  awk -F "$tab" -v key="$1" '$1 == key { print $2; exit }' "$manifest"
}

# Artifact rows: artifact<TAB>name<TAB>relpath<TAB>bytes<TAB>sha256<TAB>origin<TAB>staging-path
manifest_artifact_row() {
  awk -F "$tab" -v name="$1" '$1 == "artifact" && $2 == name { print; found=1; exit } END { if (!found) exit 1 }' "$manifest"
}

manifest_artifact_by_relpath() {
  awk -F "$tab" -v relpath="$1" '$1 == "artifact" && $3 == relpath { print; found=1; exit } END { if (!found) exit 1 }' "$manifest"
}

field() {
  printf '%s\n' "$1" | awk -F "$tab" -v number="$2" '{ print $number }'
}

safe_relative_path() {
  case "$1" in
    ''|/*|.|..|../*|*/../*|*/..|*"$tab"*) die "manifest contains an unsafe relative artifact path: $1" ;;
  esac
}

check_manifest_shape() {
  [ -f "$manifest" ] || die "boot manifest is missing: $manifest; run tools/pixel-9a/boot.sh build first"
  awk -F "$tab" '
    $1 == "artifact" && NF != 7 { bad=1 }
    $1 == "artifact" && $4 !~ /^[0-9]+$/ { bad=1 }
    $1 == "artifact" && $5 !~ /^[0-9a-f]{64}$/ { bad=1 }
    END { exit bad }
  ' "$manifest" || die "boot manifest has malformed artifact rows: $manifest"
  [ "$(manifest_value schema_version)" = 1 ] || die "boot manifest schema_version is not 1"
  [ "$(manifest_value target_id)" = "$expected_target_id" ] || die "boot manifest target_id is not the exact Pixel 9a target"
  [ "$(manifest_value codename)" = "$expected_product" ] || die "boot manifest codename is not tegu"
  [ "$(manifest_value hardware_revision)" = "$expected_revision" ] || die "boot manifest hardware_revision is not MP1.0"
  [ "$(manifest_value header_version)" = 4 ] || die "boot manifest header_version is not 4"
  # The canonical flash workflow owns only the Google Kleaf/CPIF image set.
  # The separate radio-first composer intentionally emits overlapping image
  # names but a different WWAN/SIT ABI and an incomplete partition set.  An
  # explicit backend contract prevents that handoff from reaching a physical
  # mutation path by accident.
  [ "$(manifest_value kernel_backend)" = google-kleaf-cpif ] ||
    die "boot manifest kernel_backend is not google-kleaf-cpif; radio-first WWAN/SIT artifacts require a separate reviewed flash route"
  avb_mode=$(manifest_value avb_mode)
  [ -n "$avb_mode" ] || die "boot manifest lacks an avb_mode row"
  manifest_status=$(manifest_value status)
  manifest_bootable=$(manifest_value bootable)
}

check_layout() {
  [ -f "$layout" ] || die "target image layout is missing: $layout"
  grep -q "^target_id = \"$expected_target_id\"$" "$layout" || die "image layout identity is not the exact Pixel 9a target"
  for name in $physical_artifacts $dynamic_artifacts; do
    case "$name" in
      vbmeta|vbmeta_system|vbmeta_vendor)
        grep -q "\"$name.img\"" "$layout" || die "image layout omits verified-boot artifact $name.img"
        ;;
      *)
        grep -q "^\[partition.$name\]$" "$layout" || die "image layout has no [partition.$name] mapping"
        ;;
    esac
  done
  if grep -q '^temporary_boot_before_persistent_write = true$' "$layout"; then
    policy_temp_boot=true
  else
    policy_temp_boot=false
  fi
}

# Every artifact is re-read from <manifest dir>/<relpath>; the manifest's
# staging-path column describes the composer's temporary directory and is not
# trusted for locating bytes.
verify_artifact() {
  name=$1
  row=$(manifest_artifact_row "$name") || die "boot manifest lacks artifact row: $name"
  relative=$(field "$row" 3)
  expected_bytes=$(field "$row" 4)
  expected_hash=$(field "$row" 5)
  safe_relative_path "$relative"
  file="$image_dir/$relative"
  [ -f "$file" ] || die "$name manifest row points to a missing file: $file"
  [ ! -L "$file" ] || die "$name must not be a symlink: $file"
  actual_bytes=$(bytes_of "$file")
  actual_hash=$(sha256_of "$file")
  [ "$actual_bytes" = "$expected_bytes" ] || die "$name size differs from its manifest: expected $expected_bytes, got $actual_bytes"
  [ "$actual_hash" = "$expected_hash" ] || die "$name hash differs from its manifest: expected $expected_hash, got $actual_hash"
  printf '%s\t%s\t%s\n' "$file" "$actual_bytes" "$actual_hash"
}

artifact_file() { field "$1" 1; }
artifact_bytes() { field "$1" 2; }
artifact_hash() { field "$1" 3; }

verify_manifest() {
  check_manifest_shape
  image_dir=$(CDPATH= cd -- "$(dirname -- "$manifest")" && pwd)
  emit boot_manifest "$manifest"
  emit image_dir "$image_dir"
  emit manifest_status "$manifest_status"
  emit manifest_bootable "$manifest_bootable"
  emit avb_mode "$avb_mode"
  artifact_count=$(awk -F "$tab" '$1 == "artifact" { count++ } END { print count + 0 }' "$manifest")
  [ "$artifact_count" -ge 11 ] || die "boot manifest records $artifact_count artifacts; the split-boot set needs at least 11"
  # Re-hash every artifact row exactly once, including rows this run will not
  # flash; each flashed image is hashed again immediately before its write.
  awk -F "$tab" '$1 == "artifact" { print $2 }' "$manifest" | while IFS= read -r name; do
    verify_artifact "$name" >/dev/null
  done
  for name in $physical_artifacts $dynamic_artifacts rootfs_handoff; do
    manifest_artifact_row "$name" >/dev/null || die "boot manifest lacks artifact row: $name"
  done
  boot_image=$(manifest_file boot)
  boot_hash=$(manifest_hash boot)
  emit artifacts_rehashed "$artifact_count"
}

# Verified-manifest accessors (verify_manifest has already re-hashed the row).
manifest_file() { printf '%s/%s\n' "$image_dir" "$(field "$(manifest_artifact_row "$1")" 3)"; }
manifest_bytes() { field "$(manifest_artifact_row "$1")" 4; }
manifest_hash() { field "$(manifest_artifact_row "$1")" 5; }

partition_inventory_value() {
  awk -F '=' -v key="$2" '$1 == key { sub("^[^=]*=", ""); print; exit }' "$1"
}

require_partition_inventory() {
  inventory=${FERRIX_PIXEL9A_PARTITION_INVENTORY:-}
  [ -n "$inventory" ] ||
    die 'rootfs flash requires FERRIX_PIXEL9A_PARTITION_INVENTORY pointing to the redacted read-only ADB partition inventory transcript'
  [ -f "$inventory" ] || die "partition inventory transcript is missing: $inventory"
  [ ! -L "$inventory" ] || die "partition inventory transcript must not be a symlink: $inventory"
  grep -q '^# Serial numbers are replaced by a SHA-256 prefix\.' "$inventory" ||
    die 'partition inventory transcript is not the checked-in read-only inventory format'
  [ "$(partition_inventory_value "$inventory" result)" = ok ] ||
    die 'partition inventory did not complete with result=ok'
  [ "$(partition_inventory_value "$inventory" target_id)" = "$expected_target_id" ] ||
    die 'partition inventory target_id is not the exact Pixel 9a target'
  inventory_transport_prefix=a
  [ "$(partition_inventory_value "$inventory" transport)" = "${inventory_transport_prefix}db" ] ||
    die 'rootfs partition flashing requires a read-only ADB inventory from the exact target (stock Android or Ferrix Linux), not a Fastboot-only inventory'
  [ "$(partition_inventory_value "$inventory" serial_sha256_prefix)" = "$(sha256_prefix "$fastboot_serial")" ] ||
    die 'partition inventory was captured from a different device'
  [ "$(partition_inventory_value "$inventory" product)" = "$expected_product" ] ||
    die 'partition inventory product is not tegu'
  [ "$(partition_inventory_value "$inventory" hardware_revision)" = "$expected_revision" ] ||
    die 'partition inventory hardware revision is not MP1.0'
  [ "$(partition_inventory_value "$inventory" sku)" = GXQ96 ] ||
    die 'partition inventory SKU is not GXQ96'
  [ "$(partition_inventory_value "$inventory" slot_count)" = 2 ] ||
    die 'partition inventory does not report exactly two slots'
  [ "$(partition_inventory_value "$inventory" inventory_completeness)" = complete ] ||
    die 'partition inventory is partial; capture a complete read-only ADB inventory before rootfs flashing'
  inventory_slot=$(partition_inventory_value "$inventory" current_slot)
  case "$inventory_slot" in
    a|b|_a|_b) ;;
    *) die "partition inventory has an unexpected current slot: $inventory_slot" ;;
  esac
  [ "$(normalize_slot "$inventory_slot")" = "$current_slot" ] ||
    die 'partition inventory current slot differs from the live Fastboot identity'
  partitions_block=$(sed -n '/^partitions=/,/^block_devices=/p' "$inventory")
  printf '%s\n' "$partitions_block" | grep -q 'userdata' ||
    die 'partition inventory does not list the allow-listed userdata partition'
  grep -q '^mounts=' "$inventory" || die 'partition inventory is missing the read-only mount table'
  grep -q '^cmdline=' "$inventory" || die 'partition inventory is missing the read-only kernel command line'
  emit partition_inventory "$inventory"
  emit partition_inventory_gate "passed target=$expected_target_id transport=adb userdata=present"
}

# The rootfs stage needs the generated flashable image, not the rootfs.tar
# handoff, and an explicitly reviewed deviceinfo partition selection. The
# image exists after a native rootfs build; the target keeps the partition
# selection unset until the exact live partition inventory is complete.
resolve_rootfs_stage() {
  rootfs_row=$(manifest_artifact_by_relpath rootfs.img) ||
    die "no flashable rootfs image in manifest (rootfs.img); build it first. rootfs.tar is a handoff archive, not a partition image"
  rootfs_name=$(field "$rootfs_row" 2)
  rootfs_verified=$(verify_artifact "$rootfs_name")
  rootfs_image=$(artifact_file "$rootfs_verified")
  rootfs_hash=$(artifact_hash "$rootfs_verified")
  rootfs_bytes=$(artifact_bytes "$rootfs_verified")
  [ -f "$deviceinfo" ] || die "target deviceinfo is missing: $deviceinfo"
  rootfs_partition=$(sed -n 's/^deviceinfo_flash_fastboot_partition_rootfs="\([^"]*\)"$/\1/p' "$deviceinfo" | head -1)
  [ -n "$rootfs_partition" ] ||
    die "deviceinfo does not name a rootfs partition (deviceinfo_flash_fastboot_partition_rootfs); postmarketOS Pixel ports use userdata, but this target has not selected one"
  allowed=no
  for candidate in $rootfs_partition_allowlist; do
    [ "$rootfs_partition" = "$candidate" ] && allowed=yes
  done
  [ "$allowed" = yes ] || die "rootfs partition '$rootfs_partition' is outside this workflow's allow-list ($rootfs_partition_allowlist)"
  require_partition_inventory
}

pin() {
  awk -F "$tab" -v wanted="$1" -v number="$2" '$1 == wanted { print $number; exit }' "$pins"
}

verify_recovery() {
  [ -f "$pins" ] || die "recovery pins are missing: $pins"
  filename=$(pin factory 4)
  expected_hash=$(pin factory 6)
  expected_bytes=$(pin factory 7)
  [ -n "$filename" ] && [ -n "$expected_hash" ] && [ -n "$expected_bytes" ] || die "malformed factory recovery pin in $pins"
  recovery_path="$recovery_dir/$filename"
  [ -f "$recovery_path" ] ||
    die "missing verified factory recovery artifact: $recovery_path; flashing without a verified restore path violates the recovery gate (run just pixel-9a-recovery-download)"
  [ ! -L "$recovery_path" ] || die "factory recovery artifact must not be a symlink: $recovery_path"
  actual_bytes=$(bytes_of "$recovery_path")
  actual_hash=$(sha256_of "$recovery_path")
  [ "$actual_bytes" = "$expected_bytes" ] || die "factory recovery size mismatch: expected $expected_bytes, got $actual_bytes"
  [ "$actual_hash" = "$expected_hash" ] || die "factory recovery SHA-256 mismatch: expected $expected_hash, got $actual_hash"
  emit recovery_artifact "$recovery_path sha256=$actual_hash bytes=$actual_bytes"
  ota_filename=$(pin ota 4)
  if [ -n "$ota_filename" ] && [ -f "$recovery_dir/$ota_filename" ]; then
    [ ! -L "$recovery_dir/$ota_filename" ] || die "OTA recovery artifact must not be a symlink: $recovery_dir/$ota_filename"
    ota_hash=$(sha256_of "$recovery_dir/$ota_filename")
    [ "$ota_hash" = "$(pin ota 6)" ] || die "OTA recovery SHA-256 mismatch for $ota_filename"
    emit recovery_ota "$recovery_dir/$ota_filename sha256=$ota_hash"
  else
    emit recovery_ota "absent (factory archive is the verified restore path)"
  fi
}

# A physical mutation is allowed only after the owner has verified a readable
# backup outside this repository.  Keep the read-only preflight useful by
# reporting the gate there; the execution modes call require_backup_gate after
# all other preflight checks and immediately before their first confirmation.
read_backup_gate() {
  [ -f "$target_manifest" ] && [ ! -L "$target_manifest" ] ||
    die "target manifest is missing or is a symlink: $target_manifest"
  gate_value() {
    key=$1
    awk -v wanted="$key" '
      /^\[gates\][[:space:]]*$/ { in_gates=1; next }
      /^\[/ { in_gates=0 }
      in_gates && $0 ~ "^[[:space:]]*" wanted "[[:space:]]*=" {
        value=$0
        sub(/^[^=]*=[[:space:]]*/, "", value)
        gsub(/[[:space:]"]/, "", value)
        print value
        found=1
        exit
      }
      END { if (!found) exit 1 }
    ' "$target_manifest"
  }
  identity_gate=$(gate_value identity) || die "target manifest has no [gates].identity entry: $target_manifest"
  image_compatibility_gate=$(gate_value image_compatibility) || die "target manifest has no [gates].image_compatibility entry: $target_manifest"
  backup_gate=$(gate_value backup) || die "target manifest has no [gates].backup entry: $target_manifest"
  data_policy_gate=$(gate_value data_policy || true)
  recovery_procedure_gate=$(gate_value recovery_procedure) || die "target manifest has no [gates].recovery_procedure entry: $target_manifest"
  confirmation_gate=$(gate_value per_run_confirmation) || die "target manifest has no [gates].per_run_confirmation entry: $target_manifest"
  [ -n "$identity_gate" ] && [ -n "$image_compatibility_gate" ] && [ -n "$backup_gate" ] && [ -n "$recovery_procedure_gate" ] && [ -n "$confirmation_gate" ] ||
    die "target manifest has an empty provisioning gate: $target_manifest"
  emit gate_identity "$identity_gate"
  emit gate_image_compatibility "$image_compatibility_gate"
  emit gate_recovery_procedure "$recovery_procedure_gate"
  emit backup_gate "$backup_gate"
  emit data_policy "$data_policy_gate"
  emit per_run_confirmation "$confirmation_gate"
}

require_backup_gate() {
  read_backup_gate
  [ "$identity_gate" = passed ] || die "identity gate is '$identity_gate'; exact target identity must be approved before this mutation"
  [ "$confirmation_gate" = required ] || die "per-run confirmation gate is '$confirmation_gate'; manifest must require typed confirmation"
  pixel9a_data_safety_check "$backup_gate" "$data_policy_gate" || die "$pixel9a_data_safety_reason"
  emit data_safety_gate "$pixel9a_data_safety_status"
}

require_ferrix_gates() {
  require_backup_gate
  case "$image_compatibility_gate" in
    passed|verified) ;;
    *) die "image compatibility gate is '$image_compatibility_gate'; exact Ferrix artifacts are not approved for this target" ;;
  esac
  case "$recovery_procedure_gate" in
    tested|passed|verified) ;;
    *) die "recovery procedure gate is '$recovery_procedure_gate'; test the exact-target stock recovery path before flashing Ferrix" ;;
  esac
}

fastboot_getvar() {
  variable=$1
  fastboot -s "$fastboot_serial" getvar "$variable" 2>&1 | awk -v key="$variable" '
    index($0, "(bootloader) " key ":") {
      line = $0
      sub(".*\\(bootloader\\) " key ":[[:space:]]*", "", line)
      print line
      exit
    }
    index($0, key ":") {
      line = $0
      sub(".*" key ":[[:space:]]*", "", line)
      print line
      exit
    }
  '
}

read_transports() {
  adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
  fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
  adb_count=$(count_rows "$adb_rows")
  fastboot_count=$(count_rows "$fastboot_rows")
}

normalize_slot() {
  case "$1" in
    a|_a) printf 'a\n' ;;
    b|_b) printf 'b\n' ;;
    *) die "unexpected slot value: $1" ;;
  esac
}

read_identity() {
  read_transports
  [ "$adb_count" -eq 0 ] || die "expected zero ADB devices, found $adb_count; put the exact device in Fastboot and disconnect others"
  [ "$fastboot_count" -eq 1 ] || die "expected exactly one Fastboot device, found $fastboot_count; refusing ambiguous or absent transports"
  fastboot_serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
  [ -n "$fastboot_serial" ] || die "Fastboot serial is empty"
  serial=$fastboot_serial

  product=$(fastboot_getvar product)
  variant=$(fastboot_getvar variant)
  hardware_revision=$(fastboot_getvar hw-revision)
  unlocked=$(fastboot_getvar unlocked)
  secure=$(fastboot_getvar secure)
  current_slot_raw=$(fastboot_getvar current-slot)
  slot_count=$(fastboot_getvar slot-count)
  is_userspace=$(fastboot_getvar is-userspace)
  serialno=$(fastboot_getvar serialno)

  [ "$product" = "$expected_product" ] || die "Fastboot product mismatch: '$product' is not $expected_product"
  if ! pixel9a_fastboot_sku_gate "$variant" "$(sha256_prefix "$fastboot_serial")"; then
    case "$variant" in
      N/A|NA|unknown|unavailable|'') die "Fastboot SKU/variant '$variant' is unavailable and no matching redacted ADB identity proves $expected_sku" ;;
      *) die "Fastboot SKU/variant mismatch: '$variant' is not $expected_sku" ;;
    esac
  fi
  [ "$hardware_revision" = "$expected_revision" ] || die "Fastboot hardware revision mismatch: '$hardware_revision' is not $expected_revision"
  [ "$unlocked" = yes ] || die "Fastboot reports unlocked='$unlocked'; a locked bootloader cannot boot or flash Ferrix images"
  case "$secure" in
    yes|no) ;;
    *) die "Fastboot secure state is malformed or unavailable: '$secure'" ;;
  esac
  case "$current_slot_raw" in
    a|b|_a|_b) ;;
    *) die "unexpected Fastboot current slot: '$current_slot_raw'" ;;
  esac
  [ "$slot_count" = 2 ] || die "Fastboot slot-count is '$slot_count', not 2"
  case "$is_userspace" in
    yes|no) ;;
    *) die "Fastboot is-userspace is '$is_userspace'; expected yes or no" ;;
  esac
  if [ -n "$serialno" ] && [ "$serialno" != "$fastboot_serial" ]; then
    die "Fastboot getvar serialno does not match the enumerated serial"
  fi
  current_slot=$(normalize_slot "$current_slot_raw")
  if [ -n "$slot_override" ]; then
    slot=$slot_override
    slot_source=--slot
  else
    slot=$current_slot
    slot_source=current-slot
  fi

  emit transport fastboot
  emit serial "$fastboot_serial"
  emit serial_sha256_prefix "$(sha256_prefix "$fastboot_serial")"
  emit product "$product"
  emit variant "$variant"
  emit sku "$expected_sku"
  emit sku_source "$pixel9a_sku_source"
  emit sku_evidence "$pixel9a_sku_evidence"
  emit hardware_revision "$hardware_revision"
  emit current_slot "$current_slot_raw"
  emit slot_count "$slot_count"
  emit unlocked "$unlocked"
  emit secure "$secure"
  emit is_userspace "$is_userspace"
  emit target_slot "$slot"
  emit slot_source "$slot_source"
  if [ "$slot" = "$current_slot" ]; then
    emit slot_warning "target slot equals the active slot; the next normal boot uses the flashed images and the verified factory archive is the restore path"
  else
    emit slot_warning "target slot is inactive; this workflow never issues set_active, so the flashed slot will not boot until a separately approved slot change"
  fi
}

check_avb_gate() {
  emit avb_acknowledged "$allow_unsigned_avb"
  [ "$avb_mode" = NONE ] || return 0
  [ "$unlocked" = yes ] || die "avb_mode is NONE and the device is not unlocked; unsigned vbmeta cannot boot on a locked device"
  if [ "$secure" = yes ] && [ "$allow_unsigned_avb" != yes ]; then
    die "avb_mode is NONE on a secure=yes device: the images carry unsigned vbmeta, boot only because unlocked=yes tolerates verification errors (orange state), and would leave the device unbootable if it were ever relocked. Pass --allow-unsigned-avb to acknowledge this"
  fi
}

print_plan() {
  for name in $physical_artifacts $dynamic_artifacts; do
    case "$name" in
      vendor_dlkm|system_dlkm) via=fastbootd ;;
      *) via=bootloader ;;
    esac
    emit plan "artifact=$name partition_target=${name}_$slot via=$via input_image_sha256=$(manifest_hash "$name") bytes=$(manifest_bytes "$name")"
  done
  emit plan "artifact=rootfs_handoff partition_target=none (rootfs.tar is a handoff archive, not flashed) input_image_sha256=$(manifest_hash rootfs_handoff) bytes=$(manifest_bytes rootfs_handoff)"
  case "$stage" in
    rootfs|all)
      resolve_rootfs_stage
      emit plan "artifact=$rootfs_name partition_target=$rootfs_partition via=bootloader input_image_sha256=$rootfs_hash bytes=$rootfs_bytes"
      ;;
  esac
}

evidence_value() {
  awk -F '=' -v key="$2" '$1 == key { sub("^[^=]*=", ""); print; exit }' "$1"
}

check_temp_boot_gate() {
  emit policy_temporary_boot_before_persistent_write "$policy_temp_boot"
  [ "$policy_temp_boot" = true ] || { emit temp_boot_gate "not-required-by-layout"; return 0; }
  if [ "$skip_temp_boot_gate" = yes ]; then
    emit temp_boot_gate "SKIPPED by --skip-temp-boot-gate; the typed phrase must name the skip"
    return 0
  fi
  evidence=${FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE:-}
  [ -n "$evidence" ] || die "image-layout policy requires a temporary boot before a persistent write; set FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE to the boot-temp transcript for this boot.img (or pass --skip-temp-boot-gate and name it in the typed phrase)"
  [ -f "$evidence" ] || die "temporary-boot evidence file is missing: $evidence"
  [ ! -L "$evidence" ] || die "temporary-boot evidence file must not be a symlink: $evidence"
  [ "$(evidence_value "$evidence" operation)" = pixel-9a-boot-temp ] || die "temporary-boot evidence is not a boot-temp transcript: $evidence"
  [ "$(evidence_value "$evidence" target_id)" = "$expected_target_id" ] || die "temporary-boot evidence names a different target"
  [ "$(evidence_value "$evidence" boot_img_sha256)" = "$boot_hash" ] || die "temporary-boot evidence records a different boot.img hash than the manifest ($boot_hash); re-run boot-temp for these artifacts"
  [ "$(evidence_value "$evidence" boot_exit_status)" = 0 ] || die "temporary-boot evidence records a failed boot command"
  [ "$(evidence_value "$evidence" result)" = temporary-boot-issued ] || die "temporary-boot evidence does not record result=temporary-boot-issued"
  [ "$(evidence_value "$evidence" serial_sha256_prefix)" = "$(sha256_prefix "$fastboot_serial")" ] || die "temporary-boot evidence was recorded on a different device"
  emit temp_boot_gate "passed evidence=$evidence"
}

preflight() {
  check_layout
  verify_manifest
  verify_recovery
  read_backup_gate
  read_identity
  check_avb_gate
  print_plan
  if [ "$mode" = flash ]; then
    [ -n "$slot_override" ] ||
      die "persistent flash requires an explicit --slot a|b; the active slot is never selected implicitly"
    [ "$slot" != "$current_slot" ] ||
      die "persistent flash refuses the active slot '$slot'; choose the inactive slot explicitly"
    [ "$is_userspace" = no ] || die "device is in fastbootd (is-userspace=yes); start the flash from the bootloader so physical partitions are written there first"
    check_temp_boot_gate
    emit stage "$stage"
  elif [ "$mode" = boot-temp ]; then
    [ "$is_userspace" = no ] ||
      die "temporary boot requires the bootloader Fastboot mode; device reports is-userspace=$is_userspace"
  fi
  emit preflight passed
}

confirm_phrase() {
  phrase=$1
  printf '%s\n' "$2" >&2
  printf 'Type exactly: %s\n> ' "$phrase" >&2
  IFS= read -r confirmation || die "confirmation was not provided; no mutation performed"
  [ "$confirmation" = "$phrase" ] || die "confirmation did not match; no mutation performed"
  emit confirmed "$phrase"
}

# Before every mutation: exactly one Fastboot device, zero ADB devices, and
# the same serial, product, hardware revision, unlock/secure state, active
# slot, slot count, and expected bootloader-versus-fastbootd mode.
recheck_identity() {
  expected_userspace=${1:-no}
  case "$expected_userspace" in
    yes|no) ;;
    *) die "internal error: invalid expected mode: $expected_userspace" ;;
  esac
  # Re-hash the recovery path immediately before every device state change so
  # a changed/removed restore artifact cannot pass on stale preflight output.
  verify_recovery
  read_transports
  [ "$adb_count" -eq 0 ] && [ "$fastboot_count" -eq 1 ] || die "transport changed before mutation: adb_devices=$adb_count fastboot_devices=$fastboot_count"
  recheck_serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
  [ "$recheck_serial" = "$fastboot_serial" ] || die "Fastboot serial changed before mutation"
  [ "$(fastboot_getvar product)" = "$expected_product" ] || die "Fastboot product changed before mutation"
  recheck_variant=$(fastboot_getvar variant)
  pixel9a_fastboot_sku_gate "$recheck_variant" "$(sha256_prefix "$fastboot_serial")" || die "Fastboot SKU/variant changed before mutation and no matching ADB correlation is available"
  [ "$(fastboot_getvar hw-revision)" = "$expected_revision" ] || die "Fastboot hardware revision changed before mutation"
  [ "$(fastboot_getvar unlocked)" = yes ] || die "Fastboot unlocked state changed before mutation"
  recheck_serialno=$(fastboot_getvar serialno)
  if [ -n "$recheck_serialno" ] && [ "$recheck_serialno" != "$fastboot_serial" ]; then
    die "Fastboot serialno changed before mutation"
  fi
  recheck_secure=$(fastboot_getvar secure)
  [ "$recheck_secure" = "$secure" ] || die "Fastboot secure state changed before mutation: expected $secure, got $recheck_secure"
  recheck_userspace=$(fastboot_getvar is-userspace)
  [ "$recheck_userspace" = "$expected_userspace" ] ||
    die "Fastboot is-userspace is '$recheck_userspace'; expected '$expected_userspace' before mutation"
  recheck_slot=$(normalize_slot "$(fastboot_getvar current-slot)")
  [ "$recheck_slot" = "$current_slot" ] || die "Fastboot current slot changed before mutation: expected $current_slot, got $recheck_slot"
  [ "$(fastboot_getvar slot-count)" = 2 ] || die "Fastboot slot-count changed before mutation"
}

wait_for_same_serial() {
  attempt=0
  while [ "$attempt" -lt "$wait_seconds" ]; do
    read_transports
    [ "$fastboot_count" -le 1 ] || die "ambiguous Fastboot transports while waiting: $fastboot_count"
    if [ "$fastboot_count" -eq 1 ]; then
      seen=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
      [ "$seen" = "$fastboot_serial" ] || die "a different Fastboot serial appeared while waiting"
      return 0
    fi
    attempt=$((attempt + 1))
    sleep 1
  done
  die "the exact device did not return to Fastboot within $wait_seconds seconds"
}

wait_for_fastboot_gone() {
  attempt=0
  while [ "$attempt" -lt "$wait_seconds" ]; do
    read_transports
    if [ "$fastboot_count" -eq 0 ]; then
      return 0
    fi
    attempt=$((attempt + 1))
    sleep 1
  done
  return 1
}

boot_temp() {
  emit boot_img "$boot_image"
  emit boot_img_sha256 "$boot_hash"
  emit limitation "header v4 temporary boot uses the vendor_boot, init_boot, and dtbo currently flashed on the device; this is a kernel smoke check, not a Linux userspace boot, until those partitions hold Ferrix images"
  boot_bytes=$(bytes_of "$boot_image")
  boot_actual_hash=$(sha256_of "$boot_image")
  [ "$boot_actual_hash" = "$boot_hash" ] ||
    die "boot.img changed after manifest verification: expected $boot_hash, got $boot_actual_hash"
  emit mutation_input_image "partition_target=boot (temporary) input_image_sha256=$boot_actual_hash bytes=$boot_bytes"
  short=$(printf '%s' "$boot_hash" | cut -c1-8)
  confirm_phrase \
    "CONFIRM PIXEL 9A TEMPORARY BOOT boot.img@$short" \
    "The next command sends boot.img to the exact device for a one-time boot. No partition is written."
  recheck_identity no
  emit mutation "fastboot -s $fastboot_serial boot $boot_image"
  boot_status=0
  fastboot -s "$fastboot_serial" boot "$boot_image" || boot_status=$?
  emit boot_exit_status "$boot_status"
  [ "$boot_status" -eq 0 ] || fail temporary-boot-failed "the boot command returned status $boot_status"
  if wait_for_fastboot_gone; then
    emit device_left_fastboot yes
  else
    emit device_left_fastboot "no (still enumerated after $wait_seconds seconds)"
  fi
  emit result temporary-boot-issued
  printf 'observe the kernel on the serial console; this transcript is the gate token for persistent flashing:\n%s\n' "$transcript" >&2
}

flash_partition() {
  partition=$1
  image=$2
  hash=$3
  label=$4
  expected_bytes=$5
  expected_userspace=${6:-no}
  [ -n "$expected_bytes" ] || die "missing expected byte count for $partition"
  [ -f "$image" ] || die "flash image is missing: $image"
  [ ! -L "$image" ] || die "flash image must not be a symlink: $image"
  actual_bytes=$(bytes_of "$image")
  actual_hash=$(sha256_of "$image")
  [ "$actual_bytes" = "$expected_bytes" ] ||
    die "$partition image size changed after manifest verification: expected $expected_bytes, got $actual_bytes"
  [ "$actual_hash" = "$hash" ] ||
    die "$partition image hash changed after manifest verification: expected $hash, got $actual_hash"
  emit mutation_input_image "partition_target=$partition input_image_sha256=$actual_hash bytes=$actual_bytes"
  recheck_identity "$expected_userspace"
  emit mutation "fastboot -s $fastboot_serial flash $partition $image"
  flash_status=0
  fastboot -s "$fastboot_serial" flash "$partition" "$image" || flash_status=$?
  emit flashed "partition=$partition slot=$label sha256=$hash status=$flash_status"
  [ "$flash_status" -eq 0 ] || fail flash-failed "flash of $partition returned status $flash_status; the partition may hold a partial image"
}

enter_fastbootd() {
  recheck_identity no
  emit mutation "fastboot -s $fastboot_serial reboot fastboot"
  reboot_status=0
  fastboot -s "$fastboot_serial" reboot fastboot || reboot_status=$?
  [ "$reboot_status" -eq 0 ] || fail fastbootd-entry-failed "the reboot to fastbootd returned status $reboot_status"
  wait_for_same_serial
  is_userspace=$(fastboot_getvar is-userspace)
  emit is_userspace "$is_userspace"
  [ "$is_userspace" = yes ] || fail fastbootd-entry-failed "device returned without is-userspace=yes"
}

return_to_bootloader() {
  recheck_identity yes
  emit mutation "fastboot -s $fastboot_serial reboot bootloader"
  reboot_status=0
  fastboot -s "$fastboot_serial" reboot bootloader || reboot_status=$?
  [ "$reboot_status" -eq 0 ] || fail bootloader-return-failed "reboot bootloader returned status $reboot_status"
  wait_for_same_serial
  is_userspace=$(fastboot_getvar is-userspace)
  emit is_userspace "$is_userspace"
  [ "$is_userspace" = no ] || fail bootloader-return-failed "device returned without is-userspace=no"
}

flash_kernel_stage() {
  slot_upper=$(printf '%s' "$slot" | tr a-z A-Z)
  phrase="CONFIRM PIXEL 9A FLASH KERNEL STAGE TO SLOT $slot_upper"
  [ "$skip_temp_boot_gate" != yes ] || phrase="$phrase WITHOUT TEMP BOOT GATE"
  partitions=
  for name in $physical_artifacts $dynamic_artifacts; do
    partitions="$partitions ${name}_$slot"
  done
  emit kernel_stage_partitions "${partitions# }"
  confirm_phrase "$phrase" "The next commands write the partitions above on the exact device. Dynamic partitions are written from fastbootd after a reboot to userspace Fastboot."
  for name in $physical_artifacts; do
    row=$(verify_artifact "$name")
    flash_partition "${name}_$slot" "$(artifact_file "$row")" "$(artifact_hash "$row")" "$slot" "$(artifact_bytes "$row")" no
  done
  enter_fastbootd
  for name in $dynamic_artifacts; do
    row=$(verify_artifact "$name")
    flash_partition "${name}_$slot" "$(artifact_file "$row")" "$(artifact_hash "$row")" "$slot" "$(artifact_bytes "$row")" yes
  done
  return_to_bootloader
  emit kernel_stage complete
}

flash_rootfs_stage() {
  partition_upper=$(printf '%s' "$rootfs_partition" | tr a-z A-Z)
  emit rootfs_stage_partitions "$rootfs_partition"
  confirm_phrase \
    "CONFIRM PIXEL 9A FLASH ROOTFS STAGE TO PARTITION $partition_upper" \
    "The next command overwrites $rootfs_partition on the exact device with $rootfs_image."
  flash_partition "$rootfs_partition" "$rootfs_image" "$rootfs_hash" none "$rootfs_bytes" no
  emit rootfs_stage complete
}

flash() {
  case "$stage" in
    kernel) flash_kernel_stage ;;
    rootfs) flash_rootfs_stage ;;
    all) flash_kernel_stage; flash_rootfs_stage ;;
  esac
  emit result flashed-not-booted
  emit reminder "bootable evidence still requires an observed Linux boot (serial console or SSH) collected by the separate evidence workflow; the manifest stays bootable=false until then"
  printf 'transcript (serial redacted): %s\n' "$transcript" >&2
}

parse_args "$@"
need adb
need fastboot
command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || die "missing sha256sum or shasum"
for command_name in awk cut sed tr wc; do
  need "$command_name"
done
case "$out_root" in
  /*) ;;
  *) out_root="$PWD/$out_root"; evidence_dir="$out_root/pixel-9a/flash" ;;
esac
open_transcript
preflight
case "$mode" in
  preflight) printf 'transcript (serial redacted): %s\n' "$transcript" >&2 ;;
  boot-temp) require_ferrix_gates; boot_temp ;;
  flash) require_ferrix_gates; flash ;;
esac
