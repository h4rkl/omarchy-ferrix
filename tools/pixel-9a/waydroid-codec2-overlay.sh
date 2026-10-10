#!/bin/sh
# Extracts the stock Codec2 hardware video services (Samsung Exynos MFC and
# Google BigWave AV1, AIDL android.hardware.media.c2 V2) from the
# hash-verified pinned factory archive into an ignored Waydroid vendor
# overlay for google-pixel-9a-tegu-gxq96-mp1-0. Offline; never talks to a
# device; copies only targets/boards/<target>/waydroid-codec2-overlay.tsv
# roots and their NEEDED closure inside vendor.img, and generates the
# media_codecs XMLs (secure codecs removed), a VINTF fragment, and a
# property file. Separate from waydroid-mali-overlay.sh so hardware codecs
# stay a second opt-in on top of the Mali renderer (they need its gchips
# allocator) and either overlay can be rebuilt alone.
#
#   FERRIX_PIXEL9A_CODEC2_OVERLAY=I-understand-this-extracts-vendor-codec-blobs \
#     sh tools/pixel-9a/waydroid-codec2-overlay.sh
#
# Output: ${FERRIX_OUT_DIR:-out}/pixel-9a/waydroid-codec2/<stamp>/
#   overlay/vendor/...    files for /var/lib/waydroid/overlay/vendor
#   waydroid-codec2.prop  [properties] for waydroid.cfg
#   manifest.tsv          every file with size and SHA-256
# Check it with tools/pixel-9a/waydroid-mali-abi-check.sh VENDOR_IMG
# <output>/overlay/vendor SYSTEM_LIB_DIRS before installing.
# Needs debugfs, readelf, unzip, sha256sum and python3 (reported, never
# installed).
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
pins="$target/recovery.pins"
roots="$target/waydroid-codec2-overlay.tsv"
xmltool="$root/tools/pixel-9a/waydroid-codec2-xml.py"
out_root=${FERRIX_OUT_DIR:-"$root/out"}/pixel-9a/waydroid-codec2
ack=I-understand-this-extracts-vendor-codec-blobs
# MediaCodecList rank for the hardware codecs: ahead of the container's
# ffmpeg video codecs (moved to their upstream default 0x110 below) and the
# AOSP software codecs (0x200), which stay available as fallback.
hw_rank=256
ffmpeg_video_rank=272
# LL-NDK pieces come from the Android system image (as for Mali).
llndk=" libc.so libm.so libdl.so liblog.so libnativewindow.so libbinder_ndk.so libvndksupport.so libsync.so libmediandk.so libandroid_net.so libselinux.so libEGL.so libGLESv2.so libGLESv3.so libapexsupport.so "
# Every other library the services need comes from the stock vendor image
# into this private directory, reached only through the services'
# LD_LIBRARY_PATH (kind rc). The stock Codec2 runtime and the Waydroid
# vendor's (used by its ffmpeg Codec2 service) are mutually incompatible:
# stock libcodec2_aidl_V2 needs CreateCodec2BlockPool(C2PlatformAllocatorDescV2)
# that only the stock libcodec2_vndk defines, and Waydroid's libcodec2_hidl/
# libcodec2_aidl need symbols the stock copy lacks, so neither can shadow
# the other in /vendor/lib64.
private=lib64/ferrix-codec2

die() { printf 'FAIL pixel-9a-waydroid-codec2: %s\n' "$*" >&2; exit 1; }
for t in debugfs readelf unzip sha256sum awk python3; do
  command -v "$t" >/dev/null 2>&1 || die "missing prerequisite '$t'; install it explicitly"
done
[ "${FERRIX_PIXEL9A_CODEC2_OVERLAY:-}" = "$ack" ] || die "opt-in; set FERRIX_PIXEL9A_CODEC2_OVERLAY=$ack"

pin() { awk -F '\t' -v c="$1" '$1 == "factory" { print $c; exit }' "$pins"; }
archive=${FERRIX_PIXEL9A_FACTORY_ARCHIVE:-"$target/recovery/$(pin 4)"}
[ -f "$archive" ] && [ ! -L "$archive" ] || die "missing pinned factory archive: $archive"
[ "$(wc -c <"$archive" | tr -d ' ')" = "$(pin 7)" ] || die "factory archive size mismatch"
[ "$(sha256sum "$archive" | awk '{ print $1 }')" = "$(pin 6)" ] || die "factory archive SHA-256 mismatch"

stamp=$(date -u +%Y%m%dT%H%M%SZ)
dest="$out_root/$stamp"
work="$out_root/.work-$stamp"
[ ! -e "$dest" ] || die "refusing to overwrite $dest"
mkdir -p "$work/xml" "$dest/overlay/vendor"
trap 'rm -rf "$work"' EXIT HUP INT TERM
nested=$(unzip -Z1 "$archive" | awk -v p='(^|/)image-tegu-[^/]+[.]zip$' '$0 ~ p')
[ "$(printf '%s\n' "$nested" | awk 'NF' | wc -l)" -eq 1 ] || die "expected one nested image archive"
unzip -p "$archive" "$nested" >"$work/image.zip"
unzip -p "$work/image.zip" vendor.img >"$work/vendor.img"
rm -f "$work/image.zip"

vendor_type() { debugfs -R "stat $1" "$work/vendor.img" 2>/dev/null | sed -n 's/.*Type: \([a-z]*\).*/\1/p' | head -n 1; }
safe() { case "$1" in /*|*..*|'') die "unsafe overlay path: $1" ;; esac; }
dump() { # vendor-path local-path
  [ "$(vendor_type "$1")" = regular ] || die "not a regular file in vendor.img: $1"
  mkdir -p "$(dirname "$2")"
  debugfs -R "dump -p $1 $2" "$work/vendor.img" >/dev/null 2>&1 || die "dump failed: $1"
}

queue=
xml_outputs=
n=0
while IFS="$(printf '\t')" read -r source destination kind extra; do
  case "$source" in ''|'#'*) continue ;; esac
  [ -z "${extra:-}" ] || die "allowlist row has extra fields: $source"
  safe "$destination"
  case "$kind" in
    0|1)
      dump "$source" "$dest/overlay/vendor/$destination"
      [ "$kind" = 0 ] || queue="$queue $destination"
      ;;
    rc)
      dump "$source" "$work/rc"
      [ "$(grep -c '^service ' "$work/rc")" = 1 ] || die "expected exactly one service in $source"
      mkdir -p "$(dirname "$dest/overlay/vendor/$destination")"
      awk -v env="    setenv LD_LIBRARY_PATH /vendor/$private" '{ print } /^service / { print env }' \
        "$work/rc" >"$dest/overlay/vendor/$destination"
      ;;
    xml)
      n=$((n + 1))
      dump "$source" "$work/xml/$n.xml"
      key=$(printf '%s' "$destination" | tr '/' '_')
      printf '%s\n' "$work/xml/$n.xml" >>"$work/xml/$key.inputs"
      case "$xml_outputs" in *" $destination "*) ;; *) xml_outputs="$xml_outputs $destination " ;; esac
      ;;
    *) die "allowlist row has unknown kind '$kind': $source" ;;
  esac
done <"$roots"

seen=" "
while [ -n "${queue# }" ]; do
  set -- $queue; current=$1; shift; queue="$*"
  for lib in $(readelf -d "$dest/overlay/vendor/$current" 2>/dev/null | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'); do
    case "$llndk$seen" in *" $lib "*) continue ;; esac
    seen="$seen$lib "
    [ "$(vendor_type "/lib64/$lib")" = regular ] || continue
    [ -f "$dest/overlay/vendor/$private/$lib" ] || dump "/lib64/$lib" "$dest/overlay/vendor/$private/$lib"
    queue="$queue $private/$lib"
  done
done

# media_codecs: the stock lists merged per destination, secure codecs out,
# hardware rank in. Waydroid's media_codecs.xml already includes
# media_codecs_c2.xml (absent from its vendor image) after its ffmpeg list,
# and Codec2InfoBuilder parses media_codecs{,_performance}_c2.xml before
# media_codecs.xml, so these land without replacing any container file; the
# second, included parse of the same codecs is skipped as ALREADY_EXISTS.
for destination in $xml_outputs; do
  key=$(printf '%s' "$destination" | tr '/' '_')
  rank=
  case "$destination" in *performance*) ;; *) rank="--rank $hw_rank" ;; esac
  # shellcheck disable=SC2046,SC2086
  python3 -I "$xmltool" $rank "$dest/overlay/vendor/$destination" $(cat "$work/xml/$key.inputs") ||
    die "media_codecs generation failed: $destination"
done
! grep -Eq '[.]secure"|secure-playback' "$dest/overlay/vendor/etc/media_codecs_c2.xml" ||
  die "secure codec left in media_codecs_c2.xml"
# Nothing but the services and the private directory may land in lib64.
for f in "$dest/overlay/vendor/lib64"/*; do
  [ "$f" = "$dest/overlay/vendor/$private" ] || die "library outside $private: $f"
done

# VINTF: the stock declarations live in its monolithic manifest; declare
# exactly the two stores in a Ferrix-owned fragment.
mkdir -p "$dest/overlay/vendor/etc/vintf/manifest"
cat >"$dest/overlay/vendor/etc/vintf/manifest/ferrix-pixel-media-c2.xml" <<'EOF'
<!-- Generated by tools/pixel-9a/waydroid-codec2-overlay.sh: stock Exynos MFC
     (default) and BigWave AV1 (default1) Codec2 stores. -->
<manifest version="8.0" type="device">
    <hal format="aidl">
        <name>android.hardware.media.c2</name>
        <version>2</version>
        <fqname>IComponentStore/default</fqname>
        <fqname>IComponentStore/default1</fqname>
    </hal>
</manifest>
EOF

# Stock vendor/build.prop Codec2 selection (both stores are AIDL-only; the
# container's ffmpeg store registers AIDL too) plus Ferrix ranking and
# linear-output hints: SBWC/AFBC-compressed decoder output is readable only by
# the stock display pipeline, not Waydroid's composer or the Ferrix shell.
{
  printf '[properties]\n'
  debugfs -R 'cat /build.prop' "$work/vendor.img" 2>/dev/null |
    awk -F= '/^(media\.c2\.hal\.selection|debug\.c2\.use_dmabufheaps|media\.c2\.dmabuf\.padding)=/ { print }'
  printf 'debug.ffmpeg-codec2.rank.video=%s\n' "$ffmpeg_video_rank"
  printf 'vendor.debug.c2.sbwc.enable=0\n'
  printf 'vendor.google.av1dec.afbc=0\n'
} >"$dest/waydroid-codec2.prop"
grep -qx 'media.c2.hal.selection=aidl' "$dest/waydroid-codec2.prop" || die "stock vendor does not select the AIDL Codec2 HAL"

{
  printf 'schema_version\t1\n'
  printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
  printf 'factory_archive\t%s\t%s\n' "$(basename "$archive")" "$(pin 6)"
  printf 'roots\t%s\n' "$(sha256sum "$roots" | awk '{ print $1 }')"
  printf 'xml_tool\t%s\n' "$(sha256sum "$xmltool" | awk '{ print $1 }')"
  (cd "$dest/overlay" && find vendor -type f | LC_ALL=C sort | while IFS= read -r f; do
    printf 'file\t%s\t%s\t%s\n' "$f" "$(wc -c <"$f" | tr -d ' ')" "$(sha256sum "$f" | awk '{ print $1 }')"
  done)
  printf 'prop\twaydroid-codec2.prop\t%s\n' "$(sha256sum "$dest/waydroid-codec2.prop" | awk '{ print $1 }')"
} >"$dest/manifest.tsv"
printf 'PASS pixel-9a-waydroid-codec2: %s (%s files)\n' "$dest" "$(grep -c '^file' "$dest/manifest.tsv")"
