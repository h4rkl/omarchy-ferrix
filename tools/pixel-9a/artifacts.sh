#!/bin/sh
set -eu

# Validate and hash the complete native-Linux artifact set. This workflow is
# intentionally device-free: it never invokes adb/fastboot and never creates a
# bootable image by mixing stock Android modules with a Ferrix kernel.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
layout="$target/image-layout.toml"
pins="$target/recovery.pins"
out_root=${FERRIX_OUT_DIR:-"$root/out"}
kernel_out=${FERRIX_PIXEL9A_KERNEL_OUT:-"$out_root/pixel-9a/kernel/tegu"}
kernel_dist="$kernel_out/dist"
rootfs_out=${FERRIX_PIXEL9A_ROOTFS_OUT:-"$out_root/pixel-9a/rootfs/tegu"}
kernel_manifest=${FERRIX_PIXEL9A_KERNEL_MANIFEST:-"$kernel_out/build-manifest.tsv"}
rootfs_manifest=${FERRIX_PIXEL9A_ROOTFS_MANIFEST:-"$rootfs_out/manifest.tsv"}
boot_out=${FERRIX_PIXEL9A_BOOT_OUT:-"$out_root/pixel-9a/images/tegu"}
boot_manifest=${FERRIX_PIXEL9A_BOOT_MANIFEST:-"$boot_out/boot-manifest.tsv"}
image_out=${FERRIX_PIXEL9A_IMAGE_OUT:-"$out_root/pixel-9a/images/tegu"}
artifact_manifest="$image_out/artifact-manifest.tsv"
tab=$(printf '\t')
expected_target_id=google-pixel-9a-tegu-gxq96-mp1-0

die() {
  printf 'FAIL pixel-9a-artifacts: %s\n' "$*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing prerequisite '$1'; install it explicitly"
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

bytes_of() {
  wc -c < "$1" | tr -d '[:space:]'
}

pin() {
  awk -F '\t' -v kind="$1" -v column="$2" '$1 == kind { print $column; exit }' "$pins"
}

factory_path() {
  filename=$(pin factory 4)
  [ -n "$filename" ] || die "factory pin is missing from $pins"
  printf '%s\n' "${FERRIX_PIXEL9A_FACTORY_ARCHIVE:-$target/recovery/$filename}"
}

verify_factory() {
  archive=$(factory_path)
  [ -f "$archive" ] || die "missing pinned factory archive: $archive"
  [ ! -L "$archive" ] || die "pinned factory archive must not be a symlink: $archive"
  expected_bytes=$(pin factory 7)
  expected_hash=$(pin factory 6)
  actual_bytes=$(bytes_of "$archive")
  actual_hash=$(sha256_of "$archive")
  [ "$actual_bytes" = "$expected_bytes" ] ||
    die "factory archive size mismatch: expected $expected_bytes, got $actual_bytes"
  [ "$actual_hash" = "$expected_hash" ] ||
    die "factory archive SHA-256 mismatch: expected $expected_hash, got $actual_hash"
  printf '%s\t%s\t%s\n' "$archive" "$actual_bytes" "$actual_hash"
}

manifest_value() {
  manifest=$1
  name=$2
  if [ "$manifest" = "$boot_manifest" ]; then
    awk -F '\t' -v wanted="$name" '
      $1 == "artifact" && $2 == wanted { print $3 "\t" $4 "\t" $5; found=1; exit }
      END { if (!found) exit 1 }
    ' "$manifest"
  else
    awk -F '\t' -v wanted="$name" '
      $1 == "artifact" && ($2 == wanted || $2 ~ ("/" wanted "$")) {
        print $2 "\t" $3 "\t" $4
        found=1
        exit
      }
      END { if (!found) exit 1 }
    ' "$manifest"
  fi
}

manifest_source_file() {
  manifest=$1
  relative=$2
  case "$manifest" in
    "$kernel_manifest") printf '%s\n' "$kernel_dist/$relative" ;;
    "$rootfs_manifest") printf '%s\n' "$rootfs_out/$relative" ;;
    "$boot_manifest") printf '%s\n' "$boot_out/$relative" ;;
    *) die "artifact manifest is outside the configured kernel/rootfs/boot outputs: $manifest" ;;
  esac
}

manifest_field() {
  manifest=$1
  key=$2
  awk -F "$tab" -v key="$key" '$1 == key { print $2; found=1; exit } END { if (!found) exit 1 }' "$manifest"
}

require_manifest_field() {
  manifest=$1
  label=$2
  key=$3
  expected=$4
  actual=$(manifest_field "$manifest" "$key" 2>/dev/null || true)
  [ "$actual" = "$expected" ] ||
    die "$label manifest must contain $key=$expected (found: ${actual:-missing})"
}

require_manifest_nonempty() {
  manifest=$1
  label=$2
  key=$3
  actual=$(manifest_field "$manifest" "$key" 2>/dev/null || true)
  [ -n "$actual" ] || die "$label manifest must contain a non-empty $key field"
}

safe_relative_path() {
  path=$1
  case "$path" in
    ''|/*|.|..|../*|*/../*|*/..|*' '*|*"$tab"*)
      die "manifest contains an unsafe relative artifact path: $path"
      ;;
  esac
}

check_manifest_paths() {
  label=$1
  manifest=$2
  paths=$(awk -F "$tab" '$1 == "artifact" { print $2 }' "$manifest")
  while IFS= read -r relative; do
    [ -n "$relative" ] || continue
    safe_relative_path "$relative"
  done <<EOF
$paths
EOF
}

check_manifest_file() {
  label=$1
  manifest=$2
  [ -f "$manifest" ] || die "$label manifest is missing: $manifest"
  awk -F '\t' '
    $1 == "artifact" && NF != 4 { bad=1 }
    $1 == "artifact" && $3 !~ /^[0-9]+$/ { bad=1 }
    $1 == "artifact" && $4 !~ /^[0-9a-f]{64}$/ { bad=1 }
    END { exit bad }
  ' "$manifest" || die "$label manifest has malformed artifact rows: $manifest"
  check_manifest_paths "$label" "$manifest"
}

check_kernel_manifest_contract() {
  manifest=$1
  require_manifest_field "$manifest" kernel schema_version 1
  require_manifest_field "$manifest" kernel target_id "$expected_target_id"
  require_manifest_field "$manifest" kernel build_system google-kleaf-bazel
  require_manifest_field "$manifest" kernel kernel_package '@//aosp'
}

check_rootfs_manifest_contract() {
  manifest=$1
  require_manifest_field "$manifest" rootfs schema_version 1
  require_manifest_field "$manifest" rootfs target_id "$expected_target_id"
  require_manifest_field "$manifest" rootfs codename tegu
  require_manifest_field "$manifest" rootfs hardware_revision MP1.0
  require_manifest_field "$manifest" rootfs substrate postmarketos
}

check_boot_manifest() {
  [ -f "$boot_manifest" ] || die "boot manifest is missing: $boot_manifest; run pixel-9a-boot-build first"
  awk -F '\t' '
    $1 == "artifact" && NF < 6 { bad=1 }
    $1 == "artifact" && $3 !~ /^[^[:space:]]+$/ { bad=1 }
    $1 == "artifact" && $4 !~ /^[0-9]+$/ { bad=1 }
    $1 == "artifact" && $5 !~ /^[0-9a-f]{64}$/ { bad=1 }
    END { exit bad }
  ' "$boot_manifest" || die "boot manifest has malformed artifact rows: $boot_manifest"
  check_manifest_paths boot "$boot_manifest"
  require_manifest_field "$boot_manifest" boot schema_version 1
  require_manifest_field "$boot_manifest" boot target_id "$expected_target_id"
  require_manifest_field "$boot_manifest" boot codename tegu
  require_manifest_field "$boot_manifest" boot hardware_revision MP1.0
  require_manifest_field "$boot_manifest" boot header_version 4
  require_manifest_field "$boot_manifest" boot status composed-not-booted
  require_manifest_field "$boot_manifest" boot bootable false
  require_manifest_nonempty "$boot_manifest" boot avb_mode
  require_manifest_field "$boot_manifest" boot kernel_backend google-kleaf-cpif
}

check() {
  for command_name in awk find sort wc tr grep; do need "$command_name"; done
  if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
    die "missing sha256sum or shasum"
  fi
  [ -f "$layout" ] || die "missing target image layout: $layout"
  [ -f "$pins" ] || die "missing recovery pins: $pins"
  grep -q '^target_id = "google-pixel-9a-tegu-gxq96-mp1-0"$' "$layout" ||
    die "image layout target identity mismatch"
  grep -q '^stock_modules_may_be_reused = false$' "$layout" ||
    die "image layout must prohibit stock module reuse"
  grep -q '^android_userspace_required = false$' "$layout" ||
    die "image layout must keep Android userspace optional"
  printf '%s\n' 'PASS pixel-9a-artifacts layout: exact tegu split-image policy loaded'
}

record_partition() {
  partition=$1
  source_manifest=$2
  shift 2
  found=''
  for candidate in "$@"; do
    row=$(manifest_value "$source_manifest" "$candidate" 2>/dev/null || true)
    if [ -n "$row" ]; then
      found=$row
      break
    fi
  done
  [ -n "$found" ] || die "$partition is missing from $source_manifest (tried: $*)"
  relative=$(printf '%s\n' "$found" | awk -F '\t' '{ print $1 }')
  expected_bytes=$(printf '%s\n' "$found" | awk -F '\t' '{ print $2 }')
  expected_hash=$(printf '%s\n' "$found" | awk -F '\t' '{ print $3 }')
  source_file=$(manifest_source_file "$source_manifest" "$relative")
  [ -f "$source_file" ] || die "$partition manifest points to missing file: $source_file"
  actual_bytes=$(bytes_of "$source_file")
  actual_hash=$(sha256_of "$source_file")
  [ "$actual_bytes" = "$expected_bytes" ] ||
    die "$partition size differs from its source manifest"
  [ "$actual_hash" = "$expected_hash" ] ||
    die "$partition hash differs from its source manifest"
  printf 'artifact\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$partition" "$relative" "$actual_bytes" "$actual_hash" "$source_manifest" "$source_file"
}

write_manifest() {
  tmp="$artifact_manifest.tmp.$$"
  trap 'rm -f "$tmp"' EXIT HUP INT TERM
  mkdir -p "$image_out"
  factory=$(verify_factory)
  factory_archive=$(printf '%s\n' "$factory" | awk -F '\t' '{ print $1 }')
  factory_bytes=$(printf '%s\n' "$factory" | awk -F '\t' '{ print $2 }')
  factory_hash=$(printf '%s\n' "$factory" | awk -F '\t' '{ print $3 }')
  {
    printf 'schema_version\t1\n'
    printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
    printf 'evidence\tphysical:google-pixel-9a-tegu-gxq96-mp1-0\n'
    printf 'factory_archive\t%s\t%s\t%s\n' "$factory_archive" "$factory_bytes" "$factory_hash"
    # Keep the provenance rows path/size/hash shaped.  A four-column printf
    # silently wrapped the third argument onto a second malformed line, which
    # made the manifest lose the input digest that downstream consumers need.
    printf 'input\tlayout\t%s\t%s\t%s\n' "$layout" "$(bytes_of "$layout")" "$(sha256_of "$layout")"
    printf 'input\tkernel_manifest\t%s\t%s\t%s\n' "$kernel_manifest" "$(bytes_of "$kernel_manifest")" "$(sha256_of "$kernel_manifest")"
    printf 'input\trootfs_manifest\t%s\t%s\t%s\n' "$rootfs_manifest" "$(bytes_of "$rootfs_manifest")" "$(sha256_of "$rootfs_manifest")"
    record_partition boot "$kernel_manifest" boot.img Image Image.lz4
    record_partition dtbo "$kernel_manifest" dtbo.img
    record_partition vendor_kernel_boot "$kernel_manifest" vendor_kernel_boot.img
    record_partition vendor_dlkm "$kernel_manifest" vendor_dlkm.img
    record_partition system_dlkm "$kernel_manifest" system_dlkm.img
    record_partition init_boot "$boot_manifest" init_boot init_boot.img
    record_partition vendor_boot "$boot_manifest" vendor_boot vendor_boot.img
    # rootfs.tar is a deterministic handoff archive, never a partition image.
    # Do not silently fall back to the archive when the generated ext4 image is
    # absent; flash.sh enforces the same distinction.
    record_partition rootfs "$boot_manifest" rootfs_image rootfs.img
  } > "$tmp"
  mv "$tmp" "$artifact_manifest"
  trap - EXIT HUP INT TERM
  printf '%s\n' "PASS pixel-9a-artifacts manifest: $artifact_manifest"
}

usage() {
  printf '%s\n' 'usage: tools/pixel-9a/artifacts.sh check|manifest' >&2
  exit 2
}

[ "$#" -eq 1 ] || usage
case "$1" in
  check) check ;;
  manifest)
    check
    check_manifest_file kernel "$kernel_manifest"
    check_manifest_file rootfs "$rootfs_manifest"
    check_kernel_manifest_contract "$kernel_manifest"
    check_rootfs_manifest_contract "$rootfs_manifest"
    check_boot_manifest
    write_manifest
    ;;
  *) usage ;;
esac
