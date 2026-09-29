#!/bin/sh
set -eu

# Device-free Pixel 9a split-boot composer.  This workflow consumes one
# verified Google Kleaf output and one verified native-Linux rootfs manifest.
# It never imports stock kernel modules, uses Android userspace, or talks to a
# physical target.  Generated files live below the ignored out/ tree.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
layout="$target/pmos/device-google-tegu/boot-layout.toml"
module_list="$target/pmos/device-google-tegu/modules-initfs"
kernel_pins=${FERRIX_PIXEL9A_KERNEL_PINS:-"$target/kernel-sources.toml"}
out_root=${FERRIX_OUT_DIR:-"$root/out"}
kernel_out=${FERRIX_PIXEL9A_KERNEL_OUT:-"$out_root/pixel-9a/kernel/tegu"}
kernel_dist=${FERRIX_PIXEL9A_KERNEL_DIST:-"$kernel_out/dist"}
kernel_manifest=${FERRIX_PIXEL9A_KERNEL_MANIFEST:-"$kernel_out/build-manifest.tsv"}
rootfs_out=${FERRIX_PIXEL9A_ROOTFS_OUT:-"$out_root/pixel-9a/rootfs/tegu"}
rootfs_manifest=${FERRIX_PIXEL9A_ROOTFS_MANIFEST:-"$rootfs_out/manifest.tsv"}
image_out=${FERRIX_PIXEL9A_BOOT_OUT:-"$out_root/pixel-9a/images/tegu"}
build_confirmation=I-understand-this-composes-pixel-9a-boot-artifacts
tab=$(printf '\t')

die() {
  printf 'FAIL pixel-9a-boot: %s\n' "$*" >&2
  exit 1
}

need_command() {
  command -v "$1" >/dev/null 2>&1 || die "missing prerequisite '$1'; install it explicitly"
}

resolve_tool() {
  requested=$1
  label=$2
  case "$requested" in
    */*)
      [ -x "$requested" ] || die "$label is not executable: $requested"
      printf '%s\n' "$requested"
      ;;
    *)
      command -v "$requested" >/dev/null 2>&1 ||
        die "missing $label '$requested'; install Android boot image tooling explicitly"
      command -v "$requested"
      ;;
  esac
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

bytes_of() {
  wc -c <"$1" | tr -d '[:space:]'
}

pin_top() {
  key=$1
  awk -v key="$key" '
    $0 ~ "^" key "[[:space:]]*=" {
      value=$0
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/"[[:space:]]*$/, "", value)
      print value
      found=1
      exit
    }
    END { if (!found) exit 1 }
  ' "$kernel_pins"
}

source_revision() {
  wanted=$1
  awk -v wanted="$wanted" '
    /^\[\[source\]\]/ { active=0 }
    /^name[[:space:]]*=/ {
      value=$0
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/"[[:space:]]*$/, "", value)
      active=(value == wanted)
    }
    active && /^revision[[:space:]]*=/ {
      value=$0
      sub(/^[^=]*=[[:space:]]*"/, "", value)
      sub(/"[[:space:]]*$/, "", value)
      print value
      found=1
      exit
    }
    END { if (!found) exit 1 }
  ' "$kernel_pins"
}

manifest_value() {
  manifest=$1
  key=$2
  awk -F "$tab" -v key="$key" '$1 == key { print $2; exit }' "$manifest"
}

manifest_artifact() {
  manifest=$1
  name=$2
  awk -F "$tab" -v name="$name" '$1 == "artifact" && $2 == name { print $0; found=1; exit } END { if (!found) exit 1 }' "$manifest"
}

safe_relative_path() {
  path=$1
  case "$path" in
    ''|/*|.|..|../*|*/../*|*/..)
      die "manifest contains unsafe relative artifact path: $path"
      ;;
  esac
}

check_manifest_shape() {
  manifest=$1
  label=$2
  [ -f "$manifest" ] || die "$label manifest is missing: $manifest"
  awk -F "$tab" '
    $1 == "artifact" && NF != 4 { bad=1 }
    $1 == "artifact" && $3 !~ /^[0-9]+$/ { bad=1 }
    $1 == "artifact" && $4 !~ /^[0-9a-f]{64}$/ { bad=1 }
    END { exit bad }
  ' "$manifest" || die "$label manifest has malformed artifact rows: $manifest"
}

verify_manifest_artifact() {
  manifest=$1
  base=$2
  name=$3
  row=$(manifest_artifact "$manifest" "$name") || die "$name is missing from $manifest"
  relative=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $2 }')
  expected_bytes=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $3 }')
  expected_hash=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $4 }')
  safe_relative_path "$relative"
  source="$base/$relative"
  [ -f "$source" ] || die "$name manifest row points to missing file: $source"
  [ ! -L "$source" ] || die "$name must not be a symlink: $source"
  actual_bytes=$(bytes_of "$source")
  actual_hash=$(sha256_of "$source")
  [ "$actual_bytes" = "$expected_bytes" ] ||
    die "$name size differs from its manifest: expected $expected_bytes, got $actual_bytes"
  [ "$actual_hash" = "$expected_hash" ] ||
    die "$name hash differs from its manifest: expected $expected_hash, got $actual_hash"
  printf '%s\t%s\t%s\t%s\n' "$relative" "$actual_bytes" "$actual_hash" "$source"
}

check_required_modules() {
  [ -f "$module_list" ] || die "target initfs module list is missing: $module_list"
  while IFS= read -r module; do
    case "$module" in
      ''|'#'*) continue ;;
    esac
    awk -F "$tab" -v module="$module" '
      $1 == "artifact" && ($2 == module ".ko" || $2 ~ ("/" module "\\.ko$")) { found=1 }
      END { exit(found ? 0 : 1) }
    ' "$kernel_manifest" || die "kernel manifest is missing required tegu module: $module.ko"
  done < "$module_list"
}

check_layout() {
  [ -f "$layout" ] || die "target boot layout is missing: $layout"
  grep -q '^target_id = "google-pixel-9a-tegu-gxq96-mp1-0"$' "$layout" ||
    die "target boot layout identity is not Pixel 9a tegu"
  grep -q '^header_version = 4$' "$layout" ||
    die "target boot layout must require Android boot header v4"
  grep -q '"boot.img"' "$layout" || die "target boot layout omits boot.img"
  grep -q '"init_boot.img"' "$layout" || die "target boot layout omits init_boot.img"
  grep -q '"vendor_boot.img"' "$layout" || die "target boot layout omits vendor_boot.img"
  grep -q '"vendor_kernel_boot.img"' "$layout" || die "target boot layout omits vendor_kernel_boot.img"
  grep -q '"dtbo.img"' "$layout" || die "target boot layout omits dtbo.img"
  grep -q '"vendor_dlkm.img"' "$layout" || die "target boot layout omits vendor_dlkm.img"
  grep -q '"system_dlkm.img"' "$layout" || die "target boot layout omits system_dlkm.img"
  grep -q '^rootfs_artifact = "rootfs.img"$' "$layout" ||
    die "target boot layout must use rootfs.img as the Linux rootfs artifact"
  grep -q '^rootfs_handoff_artifact = "rootfs.tar"$' "$layout" ||
    die "target boot layout must retain rootfs.tar as the Linux rootfs handoff"
  grep -q '"vbmeta.img"' "$layout" || die "target boot layout omits fresh vbmeta metadata"
  grep -q '"vbmeta_vendor.img"' "$layout" || die "target boot layout omits fresh vendor vbmeta metadata"
  grep -q '"vbmeta_system.img"' "$layout" || die "target boot layout omits fresh system vbmeta metadata"
  grep -q '^stock_modules_may_be_reused = false$' "$target/image-layout.toml" ||
    die "target image policy must prohibit stock module reuse"
}

check_output_scope() {
  case "$image_out" in
    "$out_root"/*) ;;
    *) die "boot output must stay below the configured ignored output root: $out_root" ;;
  esac
  [ ! -L "$image_out" ] || die "boot output must not be a symlink: $image_out"
}

check_kernel_contract() {
  [ -f "$kernel_pins" ] || die "kernel pin file is missing: $kernel_pins"
  [ -d "$kernel_dist" ] || die "Google Kleaf dist is missing: $kernel_dist; run the pinned kernel build first"
  check_manifest_shape "$kernel_manifest" kernel
  [ "$(manifest_value "$kernel_manifest" target_id)" = "google-pixel-9a-tegu-gxq96-mp1-0" ] ||
    die "kernel manifest target_id is not the exact Pixel 9a target"
  [ "$(manifest_value "$kernel_manifest" build_system)" = "google-kleaf-bazel" ] ||
    die "kernel manifest was not produced by Google Kleaf/Bazel"
  [ "$(manifest_value "$kernel_manifest" branch)" = "$(pin_top branch)" ] ||
    die "kernel manifest branch does not match kernel-sources.toml"
  [ "$(manifest_value "$kernel_manifest" kernel_package)" = "@//aosp" ] ||
    die "kernel manifest is not the pinned @//aosp package"
  config_hash=$(awk -F "$tab" '$1 == "config" && $2 == "tegu_defconfig" { print $3; found=1; exit } END { if (!found) exit 1 }' "$kernel_manifest") ||
    die "kernel manifest lacks the pinned tegu_defconfig digest"
  expected_config_hash=$(sha256_of "$target/kernel-config.tegu")
  [ "$config_hash" = "$expected_config_hash" ] ||
    die "kernel manifest tegu_defconfig digest does not match the target fragment"

  source_count=0
  for source_name in manifest superproject $(awk '/^name[[:space:]]*=/{ value=$0; sub(/^[^=]*=[[:space:]]*"/, "", value); sub(/"[[:space:]]*$/, "", value); if (value != "manifest" && value != "superproject") print value }' "$kernel_pins"); do
    expected=$(source_revision "$source_name") || die "kernel pin lacks revision for $source_name"
    actual=$(awk -F "$tab" -v name="$source_name" '$1 == "source" && $2 == name { print $3; found=1; exit } END { if (!found) exit 1 }' "$kernel_manifest") ||
      die "kernel manifest lacks pinned source row: $source_name"
    [ "$actual" = "$expected" ] ||
      die "kernel source revision mismatch for $source_name: expected $expected, got $actual"
    source_count=$((source_count + 1))
  done
  [ "$source_count" -ge 3 ] || die "kernel manifest did not record enough pinned sources"

  for name in boot.img dtb.img dtbo.img vendor_kernel_boot.img vendor_dlkm.img system_dlkm.img; do
    verify_manifest_artifact "$kernel_manifest" "$kernel_dist" "$name" >/dev/null
  done
  check_required_modules
}

check_rootfs_contract() {
  [ -d "$rootfs_out" ] || die "native Linux rootfs output is missing: $rootfs_out; run the rootfs build first"
  check_manifest_shape "$rootfs_manifest" rootfs
  [ "$(manifest_value "$rootfs_manifest" target_id)" = "google-pixel-9a-tegu-gxq96-mp1-0" ] ||
    die "rootfs manifest target_id is not the exact Pixel 9a target"
  [ "$(manifest_value "$rootfs_manifest" codename)" = tegu ] ||
    die "rootfs manifest codename is not tegu"
  [ "$(manifest_value "$rootfs_manifest" hardware_revision)" = MP1.0 ] ||
    die "rootfs manifest hardware revision is not MP1.0"
	[ "$(manifest_value "$rootfs_manifest" substrate)" = postmarketos ] ||
		die "rootfs manifest substrate is not postmarketos"
	[ "$(manifest_value "$rootfs_manifest" kernel_backend)" = google-kleaf-cpif ] ||
		die "rootfs manifest is paired with the radio-first WWAN/SIT userspace; canonical Google cpif boot refuses this ABI mismatch"
  [ "$(manifest_value "$rootfs_manifest" status)" = built-substrate-only ] ||
    die "rootfs manifest is not a completed built-substrate-only handoff"
  [ "$(manifest_value "$rootfs_manifest" bootable)" = false ] ||
    die "rootfs manifest must remain bootable=false before physical evidence"
  verify_manifest_artifact "$rootfs_manifest" "$rootfs_out" rootfs.tar >/dev/null
  verify_manifest_artifact "$rootfs_manifest" "$rootfs_out" rootfs.img >/dev/null
}

check_ramdisk_inputs() {
  initramfs=${FERRIX_PIXEL9A_INITRAMFS:-}
  vendor_ramdisk=${FERRIX_PIXEL9A_VENDOR_RAMDISK:-}
  bootconfig=${FERRIX_PIXEL9A_VENDOR_BOOTCONFIG:-}
  [ -n "$initramfs" ] || die 'FERRIX_PIXEL9A_INITRAMFS is required: provide the Linux initramfs cpio for init_boot.img'
  [ -n "$vendor_ramdisk" ] || die 'FERRIX_PIXEL9A_VENDOR_RAMDISK is required: provide the Linux vendor ramdisk cpio for vendor_boot.img'
  [ -n "$bootconfig" ] || die 'FERRIX_PIXEL9A_VENDOR_BOOTCONFIG is required: provide the target vendor bootconfig'
  for file in "$initramfs" "$vendor_ramdisk" "$bootconfig"; do
    [ -f "$file" ] || die "boot container input is missing: $file"
    [ ! -L "$file" ] || die "boot container input must not be a symlink: $file"
  done
}

check_tools() {
  for command_name in awk cp dirname find grep mkdir mktemp mv tr wc; do
    need_command "$command_name"
  done
  if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
    die "missing sha256sum or shasum"
  fi
  mkbootimg=$(resolve_tool "${FERRIX_PIXEL9A_MKBOOTIMG:-mkbootimg}" mkbootimg)
  avbtool=$(resolve_tool "${FERRIX_PIXEL9A_AVBTOOL:-avbtool}" avbtool)
}

preflight() {
  check_layout
  check_output_scope
  check_tools
  check_kernel_contract
  check_rootfs_contract
  check_ramdisk_inputs
  printf '%s\n' 'PASS pixel-9a-boot preflight: pinned kernel, Linux rootfs, header-v4 inputs, mkbootimg, and avbtool are ready'
}

copy_kernel_artifact() {
  name=$1
  destination=$2
  row=$(verify_manifest_artifact "$kernel_manifest" "$kernel_dist" "$name")
  source=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $4 }')
  cp "$source" "$destination"
}

copy_rootfs_handoff() {
  row=$(verify_manifest_artifact "$rootfs_manifest" "$rootfs_out" rootfs.tar)
  source=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $4 }')
  cp "$source" "$1"
}

copy_rootfs_image() {
  row=$(verify_manifest_artifact "$rootfs_manifest" "$rootfs_out" rootfs.img)
  source=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $4 }')
  cp "$source" "$1"
}

make_boot_containers() {
  initramfs=$FERRIX_PIXEL9A_INITRAMFS
  vendor_ramdisk=$FERRIX_PIXEL9A_VENDOR_RAMDISK
  bootconfig=$FERRIX_PIXEL9A_VENDOR_BOOTCONFIG
  dtb_row=$(verify_manifest_artifact "$kernel_manifest" "$kernel_dist" dtb.img)
  dtb=$(printf '%s\n' "$dtb_row" | awk -F "$tab" '{ print $4 }')
  "$mkbootimg" --header_version 4 --ramdisk "$initramfs" --output "$1/init_boot.img"
  "$mkbootimg" --header_version 4 --vendor_ramdisk "$vendor_ramdisk" --dtb "$dtb" --vendor_bootconfig "$bootconfig" --output "$1/vendor_boot.img"
  [ -s "$1/init_boot.img" ] || die "mkbootimg produced an empty init_boot.img"
  [ -s "$1/vendor_boot.img" ] || die "mkbootimg produced an empty vendor_boot.img"
}

make_vbmeta() {
  output=$1
  algorithm=${FERRIX_PIXEL9A_AVB_ALGORITHM:-NONE}
  case "$algorithm" in
    NONE)
      "$avbtool" make_vbmeta_image --algorithm NONE --output "$output"
      ;;
    *)
      key=${FERRIX_PIXEL9A_AVB_KEY:-}
      [ -n "$key" ] || die "FERRIX_PIXEL9A_AVB_KEY is required when FERRIX_PIXEL9A_AVB_ALGORITHM is $algorithm"
      [ -f "$key" ] || die "AVB key is missing: $key"
      [ ! -L "$key" ] || die "AVB key must not be a symlink: $key"
      "$avbtool" make_vbmeta_image --algorithm "$algorithm" --key "$key" --output "$output"
      ;;
  esac
  [ -s "$output" ] || die "avbtool produced an empty metadata image: $output"
}

record_artifact() {
  name=$1
  origin=$2
  file=$3
  [ -f "$file" ] || die "composed artifact is missing: $file"
  printf 'artifact\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$name" "${file#"$stage"/}" "$(bytes_of "$file")" "$(sha256_of "$file")" "$origin" "$image_out/${file#"$stage"/}" >>"$manifest_tmp"
}

compose() {
  preflight >/dev/null
  [ "${FERRIX_PIXEL9A_BOOT_BUILD:-}" = "$build_confirmation" ] ||
    die "boot composition is opt-in; set FERRIX_PIXEL9A_BOOT_BUILD=$build_confirmation"
  [ ! -e "$image_out" ] || die "refusing to overwrite existing boot output; choose a fresh FERRIX_PIXEL9A_BOOT_OUT: $image_out"
  image_parent=$(dirname -- "$image_out")
  mkdir -p "$image_parent"
  stage=$(mktemp -d "$image_parent/.tegu-boot.XXXXXX")
  manifest_tmp="$stage/boot-manifest.tsv.tmp"
  trap 'rm -rf "$stage"' EXIT HUP INT TERM

  copy_kernel_artifact boot.img "$stage/boot.img"
  copy_kernel_artifact vendor_kernel_boot.img "$stage/vendor_kernel_boot.img"
  copy_kernel_artifact dtbo.img "$stage/dtbo.img"
  copy_kernel_artifact vendor_dlkm.img "$stage/vendor_dlkm.img"
  copy_kernel_artifact system_dlkm.img "$stage/system_dlkm.img"
  make_boot_containers "$stage"
  copy_rootfs_handoff "$stage/rootfs.tar"
  copy_rootfs_image "$stage/rootfs.img"
  make_vbmeta "$stage/vbmeta.img"
  make_vbmeta "$stage/vbmeta_vendor.img"
  make_vbmeta "$stage/vbmeta_system.img"

  {
    printf 'schema_version\t1\n'
    printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
    printf 'codename\ttegu\n'
    printf 'hardware_revision\tMP1.0\n'
    printf 'header_version\t4\n'
    # This manifest is consumed by the canonical device-write workflow.  Keep
    # the backend explicit so a radio-first WWAN/SIT handoff cannot be passed
    # there merely by having the same artifact names.
    printf 'kernel_backend\tgoogle-kleaf-cpif\n'
    printf 'status\tcomposed-not-booted\n'
    printf 'bootable\tfalse\n'
    printf 'avb_mode\t%s\n' "${FERRIX_PIXEL9A_AVB_ALGORITHM:-NONE}"
    printf 'kernel_manifest\t%s\t%s\t%s\n' "$kernel_manifest" "$(bytes_of "$kernel_manifest")" "$(sha256_of "$kernel_manifest")"
    printf 'rootfs_manifest\t%s\t%s\t%s\n' "$rootfs_manifest" "$(bytes_of "$rootfs_manifest")" "$(sha256_of "$rootfs_manifest")"
    printf 'input\tbootconfig\t%s\t%s\n' "$bootconfig" "$(sha256_of "$bootconfig")"
  } >"$manifest_tmp"
  record_artifact boot kernel "$stage/boot.img"
  record_artifact init_boot generated-mkbootimg "$stage/init_boot.img"
  record_artifact vendor_boot generated-mkbootimg "$stage/vendor_boot.img"
  record_artifact vendor_kernel_boot kernel "$stage/vendor_kernel_boot.img"
  record_artifact dtbo kernel "$stage/dtbo.img"
  record_artifact vendor_dlkm kernel "$stage/vendor_dlkm.img"
  record_artifact system_dlkm kernel "$stage/system_dlkm.img"
  record_artifact rootfs_handoff rootfs "$stage/rootfs.tar"
  record_artifact rootfs_image rootfs "$stage/rootfs.img"
  record_artifact vbmeta generated-avbtool "$stage/vbmeta.img"
  record_artifact vbmeta_vendor generated-avbtool "$stage/vbmeta_vendor.img"
  record_artifact vbmeta_system generated-avbtool "$stage/vbmeta_system.img"
  mv "$manifest_tmp" "$stage/boot-manifest.tsv"
  mv "$stage" "$image_out"
  trap - EXIT HUP INT TERM
  printf '%s\n' "PASS pixel-9a-boot composed split artifacts: $image_out"
  printf '%s\n' 'NOTE: artifacts are hash-recorded but remain unbooted and bootable=false until target-scoped physical evidence exists'
}

usage() {
  printf '%s\n' 'usage: tools/pixel-9a/boot.sh preflight|build' >&2
  exit 2
}

case "${1:-}" in
  preflight) [ "$#" -eq 1 ] || usage; preflight ;;
  build) [ "$#" -eq 1 ] || usage; compose ;;
  *) usage ;;
esac
