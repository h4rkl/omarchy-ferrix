#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script="$root/tools/pixel-9a/boot.sh"
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-pixel9a-boot.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
tab=$(printf '\t')

[ -x "$script" ] || {
  echo 'boot workflow is not executable' >&2
  exit 1
}
sh -n "$script"

# This slice is device-free and must not grow a provisioning path.
if grep -nE '(^|[^[:alnum:]_-])(adb|fastboot|pmbootstrap[[:space:]]+flasher|flash|erase|format|reboot)([^[:alnum:]_-]|$)' "$script" >/dev/null; then
  echo 'boot workflow contains a device or destructive command token' >&2
  exit 1
fi

mkdir -p "$work/bin" "$work/out/pixel-9a/kernel/tegu/dist" "$work/out/pixel-9a/rootfs/tegu"

# Minimal offline tool doubles. They preserve the arguments relevant to this
# workflow and make the test independent of Android build-tool installation.
cat >"$work/bin/mkbootimg" <<'EOF'
#!/bin/sh
set -eu
output=
header=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) output=$2; shift 2 ;;
    --header_version) header=$2; shift 2 ;;
    *) shift ;;
  esac
done
[ "$header" = 4 ]
[ -n "$output" ]
printf 'synthetic-header-v4\n' >"$output"
EOF
cat >"$work/bin/avbtool" <<'EOF'
#!/bin/sh
set -eu
output=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) output=$2; shift 2 ;;
    *) shift ;;
  esac
done
[ -n "$output" ]
printf 'synthetic-fresh-avb-metadata\n' >"$output"
EOF
chmod +x "$work/bin/mkbootimg" "$work/bin/avbtool"

kernel="$work/out/pixel-9a/kernel/tegu"
dist="$kernel/dist"
for name in boot.img dtb.img dtbo.img vendor_kernel_boot.img vendor_dlkm.img system_dlkm.img; do
  printf 'kernel-%s\n' "$name" >"$dist/$name"
done
while IFS= read -r module; do
  case "$module" in
    ''|'#'*) continue ;;
  esac
  printf 'kernel-module-%s\n' "$module" >"$dist/$module.ko"
done <"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/modules-initfs"
{
  printf 'schema_version\t1\n'
  printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
  printf 'branch\tandroid-gs-tegu-6.1-android16\n'
  printf 'build_system\tgoogle-kleaf-bazel\n'
  printf 'kernel_package\t@//aosp\n'
  printf 'config\ttegu_defconfig\t%s\n' "$(sha256sum "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/kernel-config.tegu" | awk '{ print $1 }')"
  for source_name in manifest superproject; do
    revision=$(awk -v wanted="$source_name" '
      /^\[\[source\]\]/ { active=0 }
      /^name[[:space:]]*=/ { value=$0; sub(/^[^=]*=[[:space:]]*"/, "", value); sub(/"[[:space:]]*$/, "", value); active=(value == wanted) }
      active && /^revision[[:space:]]*=/ { value=$0; sub(/^[^=]*=[[:space:]]*"/, "", value); sub(/"[[:space:]]*$/, "", value); print value; exit }
    ' "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/kernel-sources.toml")
    printf 'source\t%s\t%s\n' "$source_name" "$revision"
  done
  awk '
    /^name[[:space:]]*=/ { value=$0; sub(/^[^=]*=[[:space:]]*"/, "", value); sub(/"[[:space:]]*$/, "", value); if (value != "manifest" && value != "superproject") print value }
  ' "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/kernel-sources.toml" | while IFS= read -r source_name; do
    revision=$(awk -v wanted="$source_name" '
      /^\[\[source\]\]/ { active=0 }
      /^name[[:space:]]*=/ { value=$0; sub(/^[^=]*=[[:space:]]*"/, "", value); sub(/"[[:space:]]*$/, "", value); active=(value == wanted) }
      active && /^revision[[:space:]]*=/ { value=$0; sub(/^[^=]*=[[:space:]]*"/, "", value); sub(/"[[:space:]]*$/, "", value); print value; exit }
    ' "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/kernel-sources.toml")
    printf 'source\t%s\t%s\n' "$source_name" "$revision"
  done
  for name in boot.img dtb.img dtbo.img vendor_kernel_boot.img vendor_dlkm.img system_dlkm.img; do
    hash=$(sha256sum "$dist/$name" | awk '{ print $1 }')
    printf 'artifact\t%s\t%s\t%s\n' "$name" "$(wc -c <"$dist/$name" | tr -d '[:space:]')" "$hash"
  done
  while IFS= read -r module; do
    case "$module" in
      ''|'#'*) continue ;;
    esac
    hash=$(sha256sum "$dist/$module.ko" | awk '{ print $1 }')
    printf 'artifact\t%s\t%s\t%s\n' "$module.ko" "$(wc -c <"$dist/$module.ko" | tr -d '[:space:]')" "$hash"
  done <"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/modules-initfs"
} >"$kernel/build-manifest.tsv"

rootfs="$work/out/pixel-9a/rootfs/tegu"
printf 'linux-rootfs-handoff\n' >"$rootfs/rootfs.tar"
printf 'linux-rootfs-ext4-image\n' >"$rootfs/rootfs.img"
rootfs_hash=$(sha256sum "$rootfs/rootfs.tar" | awk '{ print $1 }')
rootfs_image_hash=$(sha256sum "$rootfs/rootfs.img" | awk '{ print $1 }')
{
printf 'schema_version\t1\n'
printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
printf 'codename\ttegu\n'
printf 'hardware_revision\tMP1.0\n'
printf 'substrate\tpostmarketos\n'
printf 'kernel_backend\tgoogle-kleaf-cpif\n'
printf 'status\tbuilt-substrate-only\n'
printf 'bootable\tfalse\n'
printf 'artifact\trootfs.tar\t%s\t%s\n' "$(wc -c <"$rootfs/rootfs.tar" | tr -d '[:space:]')" "$rootfs_hash"
  printf 'artifact\trootfs.img\t%s\t%s\n' "$(wc -c <"$rootfs/rootfs.img" | tr -d '[:space:]')" "$rootfs_image_hash"
} >"$rootfs/manifest.tsv"

printf 'initramfs\n' >"$work/initramfs.cpio"
printf 'vendor-ramdisk\n' >"$work/vendor-ramdisk.cpio"
printf 'console=ttyS0\n' >"$work/vendor-bootconfig"

common_env="PATH=$work/bin:$PATH FERRIX_OUT_DIR=$work/out FERRIX_PIXEL9A_KERNEL_DIST=$dist FERRIX_PIXEL9A_KERNEL_MANIFEST=$kernel/build-manifest.tsv FERRIX_PIXEL9A_ROOTFS_OUT=$rootfs FERRIX_PIXEL9A_ROOTFS_MANIFEST=$rootfs/manifest.tsv FERRIX_PIXEL9A_INITRAMFS=$work/initramfs.cpio FERRIX_PIXEL9A_VENDOR_RAMDISK=$work/vendor-ramdisk.cpio FERRIX_PIXEL9A_VENDOR_BOOTCONFIG=$work/vendor-bootconfig"

# Missing image tooling fails closed before any output creation.
if env $common_env FERRIX_PIXEL9A_MKBOOTIMG="$work/missing-mkbootimg" "$script" preflight >"$work/tool.out" 2>"$work/tool.err"; then
  echo 'preflight unexpectedly accepted a missing mkbootimg' >&2
  exit 1
fi
grep -q 'mkbootimg is not executable' "$work/tool.err"
[ ! -e "$work/out/pixel-9a/images/tegu" ]

# Missing inputs fail before output creation and explain the next action.
if env $common_env FERRIX_PIXEL9A_INITRAMFS="$work/missing-initramfs" "$script" preflight >"$work/missing.out" 2>"$work/missing.err"; then
  echo 'preflight unexpectedly accepted a missing initramfs' >&2
  exit 1
fi
grep -q 'boot container input is missing' "$work/missing.err"
[ ! -e "$work/out/pixel-9a/images/tegu" ]

# A hash-valid but incomplete rootfs handoff must not reach the composer.  The
# status/identity rows are part of the cross-stage contract, not decoration.
bad_rootfs_manifest="$work/rootfs-bad-status.tsv"
sed 's/^status\tbuilt-substrate-only$/status\tnot-built/' "$rootfs/manifest.tsv" >"$bad_rootfs_manifest"
if env $common_env FERRIX_PIXEL9A_ROOTFS_MANIFEST="$bad_rootfs_manifest" \
  "$script" preflight >"$work/bad-rootfs.out" 2>"$work/bad-rootfs.err"; then
  echo 'preflight unexpectedly accepted a not-built rootfs handoff' >&2
  exit 1
fi
grep -q 'completed built-substrate-only handoff' "$work/bad-rootfs.err"
[ ! -e "$work/out/pixel-9a/images/tegu" ]

# The target rootfs currently carries the source-pinned WWAN/SIT userspace.
# It must never be paired with the Google Kleaf cpif kernel route, even if all
# artifact hashes happen to validate.
radio_rootfs_manifest="$work/rootfs-radio-backend.tsv"
sed "s/^kernel_backend${tab}google-kleaf-cpif$/kernel_backend${tab}linux-google-tegu-wwan/" \
	"$rootfs/manifest.tsv" >"$radio_rootfs_manifest"
if env $common_env FERRIX_PIXEL9A_ROOTFS_MANIFEST="$radio_rootfs_manifest" \
	"$script" preflight >"$work/radio-mismatch.out" 2>"$work/radio-mismatch.err"; then
	echo 'preflight unexpectedly accepted a Google cpif + WWAN/SIT rootfs pairing' >&2
	exit 1
fi
grep -q 'radio-first WWAN/SIT userspace' "$work/radio-mismatch.err"
[ ! -e "$work/out/pixel-9a/images/tegu" ]

# The build confirmation is independent from input validation.
if env $common_env "$script" build >"$work/no-confirm.out" 2>"$work/no-confirm.err"; then
  echo 'build unexpectedly accepted missing explicit confirmation' >&2
  exit 1
fi
grep -q 'boot composition is opt-in' "$work/no-confirm.err"
[ ! -e "$work/out/pixel-9a/images/tegu" ]

env $common_env \
  FERRIX_PIXEL9A_BOOT_BUILD=I-understand-this-composes-pixel-9a-boot-artifacts \
  "$script" build >"$work/build.out"

image="$work/out/pixel-9a/images/tegu"
[ -f "$image/boot-manifest.tsv" ]
for name in boot.img init_boot.img vendor_boot.img vendor_kernel_boot.img dtbo.img vendor_dlkm.img system_dlkm.img rootfs.tar rootfs.img vbmeta.img vbmeta_vendor.img vbmeta_system.img; do
  [ -s "$image/$name" ] || { echo "missing composed artifact: $name" >&2; exit 1; }
done
awk -F "$tab" '$1 == "status" && $2 == "composed-not-booted" { found=1 } END { exit(found ? 0 : 1) }' "$image/boot-manifest.tsv"
awk -F "$tab" '$1 == "bootable" && $2 == "false" { found=1 } END { exit(found ? 0 : 1) }' "$image/boot-manifest.tsv"
for name in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm rootfs_handoff rootfs_image vbmeta vbmeta_vendor vbmeta_system; do
  awk -F "$tab" -v name="$name" '$1 == "artifact" && $2 == name && $3 != "" && $4 ~ /^[0-9]+$/ && $5 ~ /^[0-9a-f]{64}$/ { found=1 } END { exit(found ? 0 : 1) }' "$image/boot-manifest.tsv"
done

printf '%s\n' 'pixel-9a boot workflow tests passed'
