#!/bin/sh
# Extracts the stock Mali graphics stack (GLES/Vulkan blobs, gchips allocator
# and stable-C mapper, their vendor libraries, init and VINTF fragments) from
# the hash-verified pinned factory archive into an ignored Waydroid vendor
# overlay for google-pixel-9a-tegu-gxq96-mp1-0. Offline; never talks to a
# device; copies only targets/boards/<target>/waydroid-mali-overlay.tsv roots
# and their NEEDED closure inside vendor.img.
#
#   FERRIX_PIXEL9A_MALI_OVERLAY=I-understand-this-extracts-vendor-graphics-blobs \
#     sh tools/pixel-9a/waydroid-mali-overlay.sh
#
# Output: ${FERRIX_OUT_DIR:-out}/pixel-9a/waydroid-mali/<stamp>/
#   overlay/vendor/...   files for /var/lib/waydroid/overlay/vendor
#   waydroid-mali.prop   [properties] for waydroid.cfg (renderer=mali)
#   manifest.tsv         every file with size and SHA-256
# Needs debugfs, readelf, unzip and sha256sum (reported, never installed).
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
pins="$target/recovery.pins"
roots="$target/waydroid-mali-overlay.tsv"
out_root=${FERRIX_OUT_DIR:-"$root/out"}/pixel-9a/waydroid-mali
ack=I-understand-this-extracts-vendor-graphics-blobs
# LL-NDK (and libbinder_ndk users' platform pieces) come from the Android
# system image; shadowing them with vendor copies breaks the system side.
llndk=" libc.so libm.so libdl.so liblog.so libnativewindow.so libbinder_ndk.so libvndksupport.so libsync.so libmediandk.so libandroid_net.so libselinux.so "
# Vendor libraries the Waydroid LineageOS 23.2 vendor image already ships in
# a form the stock blobs link against. Shadowing them broke Waydroid's own
# vendor binaries (libbase: the Bluetooth HAL; libdmabufheap: codec2), so the
# closure stops at them. libc++ stays: four stock blobs need symbols only the
# stock copy exports. Re-check with tools/pixel-9a/waydroid-mali-abi-check.sh
# whenever the factory or Waydroid pins move.
container_provided=" libbase.so libcutils.so libutils.so libz.so libhidlbase.so libhardware.so libdrm.so libdmabufheap.so libgralloctypes.so libaconfig_storage_read_api_cc.so android.hardware.common-V2-ndk.so android.hardware.graphics.allocator-V2-ndk.so android.hardware.graphics.allocator@4.0.so android.hardware.graphics.common-V7-ndk.so android.hardware.graphics.common@1.0.so android.hardware.graphics.common@1.1.so android.hardware.graphics.common@1.2.so android.hardware.graphics.mapper@4.0.so "

die() { printf 'FAIL pixel-9a-waydroid-mali: %s\n' "$*" >&2; exit 1; }
for t in debugfs readelf unzip sha256sum awk; do
  command -v "$t" >/dev/null 2>&1 || die "missing prerequisite '$t'; install it explicitly"
done
[ "${FERRIX_PIXEL9A_MALI_OVERLAY:-}" = "$ack" ] || die "opt-in; set FERRIX_PIXEL9A_MALI_OVERLAY=$ack"

pin() { awk -F '\t' -v c="$1" '$1 == "factory" { print $c; exit }' "$pins"; }
archive=${FERRIX_PIXEL9A_FACTORY_ARCHIVE:-"$target/recovery/$(pin 4)"}
[ -f "$archive" ] && [ ! -L "$archive" ] || die "missing pinned factory archive: $archive"
[ "$(wc -c <"$archive" | tr -d ' ')" = "$(pin 7)" ] || die "factory archive size mismatch"
[ "$(sha256sum "$archive" | awk '{ print $1 }')" = "$(pin 6)" ] || die "factory archive SHA-256 mismatch"

stamp=$(date -u +%Y%m%dT%H%M%SZ)
dest="$out_root/$stamp"
work="$out_root/.work-$stamp"
mkdir -p "$work" "$dest/overlay/vendor"
trap 'rm -rf "$work"' EXIT HUP INT TERM
nested=$(unzip -Z1 "$archive" | awk -v p='(^|/)image-tegu-[^/]+[.]zip$' '$0 ~ p')
[ "$(printf '%s\n' "$nested" | awk 'NF' | wc -l)" -eq 1 ] || die "expected one nested image archive"
unzip -p "$archive" "$nested" >"$work/image.zip"
unzip -p "$work/image.zip" vendor.img >"$work/vendor.img"

vendor_type() { debugfs -R "stat $1" "$work/vendor.img" 2>/dev/null | sed -n 's/.*Type: \([a-z]*\).*/\1/p' | head -n 1; }
copy() { # vendor-path overlay-path
  case "$2" in /*|*..*) die "unsafe overlay path: $2" ;; esac
  [ "$(vendor_type "$1")" = regular ] || die "not a regular file in vendor.img: $1"
  mkdir -p "$(dirname "$dest/overlay/vendor/$2")"
  debugfs -R "dump -p $1 $dest/overlay/vendor/$2" "$work/vendor.img" >/dev/null 2>&1 || die "dump failed: $1"
}

queue=
while IFS="$(printf '\t')" read -r source destination closure extra; do
  case "$source" in ''|'#'*) continue ;; esac
  [ -z "${extra:-}" ] || die "allowlist row has extra fields: $source"
  copy "$source" "$destination"
  [ "$closure" = 1 ] && queue="$queue $destination"
done <"$roots"

seen=" "
while [ -n "${queue# }" ]; do
  set -- $queue; current=$1; shift; queue="$*"
  for lib in $(readelf -d "$dest/overlay/vendor/$current" 2>/dev/null | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'); do
    case "$llndk$container_provided$seen" in *" $lib "*) continue ;; esac
    seen="$seen$lib "
    [ "$(vendor_type "/lib64/$lib")" = regular ] || continue
    copy "/lib64/$lib" "lib64/$lib"
    queue="$queue lib64/$lib"
  done
done

# Stock properties for this vendor (vendor/build.prop), minus paths that do
# not exist in the overlay. `ro.hardware.gralloc` only has to be something
# other than default/gbm/minigbm* for Waydroid to treat buffers as vendor
# (GRALLOC_ANDROID); the mapper suffix comes from the allocator service.
{
  printf '[properties]\n'
  printf 'ro.hardware.egl=mali\n'
  printf 'ro.hardware.gralloc=pixel\n'
  printf 'ro.hardware.vulkan=mali\n'
  # gchips allocates GPU-rendered RGB buffers as AFBC, which neither the
  # Ferrix software compositor (dma-buf) nor the hwcomposer's CPU copy can
  # read; MALI_GRALLOC_USAGE_NO_AFBC (GRALLOC_USAGE_PRIVATE_1) keeps them
  # linear (Ferrix frameworks/native libui patch).
  printf 'ro.ferrix.gralloc.extra_rgb_usage=0x20000000\n'
  debugfs -R 'cat /build.prop' "$work/vendor.img" 2>/dev/null |
    awk -F= '/^vendor\.mali\.(base_protected_max_core_count|base_protected_tls_max|platform_agt_frequency_khz)=/ { print }'
} >"$dest/waydroid-mali.prop"

{
  printf 'schema_version\t1\n'
  printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
  printf 'factory_archive\t%s\t%s\n' "$(basename "$archive")" "$(pin 6)"
  printf 'roots\t%s\n' "$(sha256sum "$roots" | awk '{ print $1 }')"
  (cd "$dest/overlay" && find vendor -type f | LC_ALL=C sort | while IFS= read -r f; do
    printf 'file\t%s\t%s\t%s\n' "$f" "$(wc -c <"$f" | tr -d ' ')" "$(sha256sum "$f" | awk '{ print $1 }')"
  done)
  printf 'prop\twaydroid-mali.prop\t%s\n' "$(sha256sum "$dest/waydroid-mali.prop" | awk '{ print $1 }')"
} >"$dest/manifest.tsv"
printf 'PASS pixel-9a-waydroid-mali: %s (%s files)\n' "$dest" "$(grep -c '^file' "$dest/manifest.tsv")"
