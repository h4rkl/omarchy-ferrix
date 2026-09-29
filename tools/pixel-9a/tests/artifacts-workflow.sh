#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
workflow="$root/tools/pixel-9a/artifacts.sh"

[ -x "$workflow" ] || {
  echo 'artifacts workflow is not executable' >&2
  exit 1
}
sh -n "$workflow"

# Static safety boundary: artifact validation cannot acquire a device or issue
# a bootloader mutation. The words in the explanatory comments are allowed;
# executable command substitutions must not contain either tool.
offending=$(grep -nE '(^|[^[:alnum:]_-])(adb|fastboot)([^[:alnum:]_-]|$)' "$workflow" \
  | grep -v 'never invokes adb/fastboot' || true)
[ -z "$offending" ] || { printf '%s\n' "$offending" >&2; exit 1; }
if grep -nE '(flash|erase|format|reboot|sideload|unlock)[[:space:]]' "$workflow" \
  | grep -v 'without mixing stock' >/dev/null; then
  echo 'artifact workflow contains a device mutation verb' >&2
  exit 1
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-artifacts-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/out"

if FERRIX_OUT_DIR="$work/out" "$workflow" check >"$work/stdout" 2>"$work/stderr"; then
  :
else
  echo 'artifact layout check unexpectedly failed' >&2
  cat "$work/stderr" >&2
  exit 1
fi
grep -q 'exact tegu split-image policy loaded' "$work/stdout"

# The manifest path must fail closed before any output is created when the
# source build manifests do not exist.
if FERRIX_OUT_DIR="$work/out" "$workflow" manifest >"$work/missing.out" 2>"$work/missing.err"; then
  echo 'artifact manifest unexpectedly passed without build inputs' >&2
  exit 1
fi
grep -q 'kernel manifest is missing' "$work/missing.err"
[ ! -e "$work/out/pixel-9a/images/tegu/artifact-manifest.tsv" ]

# Build a fully synthetic success fixture without downloading the pinned
# factory archive. The sparse file keeps the pinned byte count while the
# test-local hash wrapper returns only that archive's already-pinned digest;
# every generated artifact is still checked with the real hash implementation.
tab=$(printf '\t')
fixture_out="$work/out"
kernel="$fixture_out/pixel-9a/kernel/tegu"
kernel_dist="$kernel/dist"
rootfs="$fixture_out/pixel-9a/rootfs/tegu"
image="$fixture_out/pixel-9a/images/tegu"
layout="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/image-layout.toml"
mkdir -p "$kernel_dist" "$rootfs" "$image" "$work/bin"

file_bytes() {
  wc -c <"$1" | tr -d '[:space:]'
}

file_hash() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

write_boot_artifact() {
  name=$1
  relative=$2
  origin=$3
  file="$image/$relative"
  printf 'artifact\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$name" "$relative" "$(file_bytes "$file")" "$(file_hash "$file")" "$origin" "$file"
}

for name in boot.img dtbo.img vendor_kernel_boot.img vendor_dlkm.img system_dlkm.img; do
  printf 'synthetic-kernel-%s\n' "$name" >"$kernel_dist/$name"
done
printf '%s\n' 'synthetic-rootfs-source' >"$rootfs/rootfs.tar"
printf '%s\n' 'synthetic-rootfs-image' >"$rootfs/rootfs.img"
printf '%s\n' 'synthetic-init-boot' >"$image/init_boot.img"
printf '%s\n' 'synthetic-vendor-boot' >"$image/vendor_boot.img"
printf '%s\n' 'synthetic-rootfs-source' >"$image/rootfs.tar"
printf '%s\n' 'synthetic-rootfs-image' >"$image/rootfs.img"

kernel_manifest="$kernel/build-manifest.tsv"
{
  printf 'schema_version\t1\n'
  printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
  printf 'build_system\tgoogle-kleaf-bazel\n'
  printf 'kernel_package\t@//aosp\n'
  for name in boot.img dtbo.img vendor_kernel_boot.img vendor_dlkm.img system_dlkm.img; do
    file="$kernel_dist/$name"
    printf 'artifact\t%s\t%s\t%s\n' "$name" "$(file_bytes "$file")" "$(file_hash "$file")"
  done
} >"$kernel_manifest"

rootfs_manifest="$rootfs/manifest.tsv"
rootfs_source="$rootfs/rootfs.tar"
rootfs_image_source="$rootfs/rootfs.img"
{
  printf 'schema_version\t1\n'
  printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
  printf 'codename\ttegu\n'
  printf 'hardware_revision\tMP1.0\n'
  printf 'substrate\tpostmarketos\n'
  printf 'status\tbuilt-substrate-only\n'
  printf 'artifact\trootfs.tar\t%s\t%s\n' "$(file_bytes "$rootfs_source")" "$(file_hash "$rootfs_source")"
  printf 'artifact\trootfs.img\t%s\t%s\n' "$(file_bytes "$rootfs_image_source")" "$(file_hash "$rootfs_image_source")"
} >"$rootfs_manifest"

boot_manifest="$image/boot-manifest.tsv"
{
  printf 'schema_version\t1\n'
  printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
  printf 'codename\ttegu\n'
  printf 'hardware_revision\tMP1.0\n'
  printf 'header_version\t4\n'
  printf 'kernel_backend\tgoogle-kleaf-cpif\n'
  printf 'status\tcomposed-not-booted\n'
  printf 'bootable\tfalse\n'
  printf 'avb_mode\tNONE\n'
  printf 'kernel_manifest\t%s\t%s\t%s\n' "$kernel_manifest" "$(file_bytes "$kernel_manifest")" "$(file_hash "$kernel_manifest")"
  printf 'rootfs_manifest\t%s\t%s\t%s\n' "$rootfs_manifest" "$(file_bytes "$rootfs_manifest")" "$(file_hash "$rootfs_manifest")"
  write_boot_artifact init_boot init_boot.img generated-mkbootimg
  write_boot_artifact vendor_boot vendor_boot.img generated-mkbootimg
  write_boot_artifact rootfs_handoff rootfs.tar rootfs
  write_boot_artifact rootfs_image rootfs.img rootfs
} >"$boot_manifest"

# Path traversal in an otherwise hash-valid upstream manifest must be rejected
# before the factory archive is considered.  Artifact manifests are consumed
# by later target-scoped tooling, so a forged relative path must not escape its
# configured dist/output root.
bad_path_manifest="$work/kernel-bad-path.tsv"
awk -F "$tab" -v OFS="$tab" '
  $1 == "artifact" && !done { $2 = "../escape.img"; done = 1 }
  { print }
' "$kernel_manifest" >"$bad_path_manifest"
if FERRIX_OUT_DIR="$fixture_out" \
  FERRIX_PIXEL9A_KERNEL_OUT="$kernel" \
  FERRIX_PIXEL9A_KERNEL_DIST="$kernel_dist" \
  FERRIX_PIXEL9A_KERNEL_MANIFEST="$bad_path_manifest" \
  FERRIX_PIXEL9A_ROOTFS_OUT="$rootfs" \
  FERRIX_PIXEL9A_ROOTFS_MANIFEST="$rootfs_manifest" \
  FERRIX_PIXEL9A_BOOT_OUT="$image" \
  FERRIX_PIXEL9A_BOOT_MANIFEST="$boot_manifest" \
  "$workflow" manifest >"$work/bad-path.out" 2>"$work/bad-path.err"; then
  echo 'artifact manifest unexpectedly accepted a traversal path' >&2
  exit 1
fi
grep -q 'unsafe relative artifact path' "$work/bad-path.err"

pins="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/recovery.pins"
factory_name=$(awk -F '\t' '$1 == "factory" { print $4; exit }' "$pins")
factory_bytes=$(awk -F '\t' '$1 == "factory" { print $7; exit }' "$pins")
factory_hash=$(awk -F '\t' '$1 == "factory" { print $6; exit }' "$pins")
factory="$work/$factory_name"
dd if=/dev/zero of="$factory" bs=1 count=0 seek="$factory_bytes" 2>/dev/null

real_sha256sum=$(command -v sha256sum 2>/dev/null || true)
if [ -n "$real_sha256sum" ]; then
  real_hash_tool="$real_sha256sum"
  real_hash_args=''
else
  real_hash_tool=$(command -v shasum)
  real_hash_args='-a 256'
fi
cat >"$work/bin/sha256sum" <<EOF
#!/bin/sh
set -eu
if [ "\${1:-}" = "$factory" ]; then
  printf '%s  %s\\n' "$factory_hash" "\$1"
elif [ -n "$real_hash_args" ]; then
  "$real_hash_tool" $real_hash_args "\$@"
else
  "$real_hash_tool" "\$@"
fi
EOF
chmod +x "$work/bin/sha256sum"

FERRIX_OUT_DIR="$fixture_out" \
FERRIX_PIXEL9A_KERNEL_OUT="$kernel" \
FERRIX_PIXEL9A_KERNEL_DIST="$kernel_dist" \
FERRIX_PIXEL9A_KERNEL_MANIFEST="$kernel_manifest" \
FERRIX_PIXEL9A_ROOTFS_OUT="$rootfs" \
FERRIX_PIXEL9A_ROOTFS_MANIFEST="$rootfs_manifest" \
FERRIX_PIXEL9A_BOOT_OUT="$image" \
FERRIX_PIXEL9A_BOOT_MANIFEST="$boot_manifest" \
FERRIX_PIXEL9A_IMAGE_OUT="$image" \
FERRIX_PIXEL9A_FACTORY_ARCHIVE="$factory" \
PATH="$work/bin:$PATH" \
  "$workflow" manifest >"$work/success.out"

artifact_manifest="$image/artifact-manifest.tsv"
[ -f "$artifact_manifest" ]
grep -q 'PASS pixel-9a-artifacts manifest:' "$work/success.out"
awk -F "$tab" -v archive="$factory" -v bytes="$factory_bytes" -v hash="$factory_hash" '
  $1 == "factory_archive" && $2 == archive && $3 == bytes && $4 == hash { found=1 }
  END { exit(found ? 0 : 1) }
' "$artifact_manifest"

# The three input provenance rows must retain all of path, byte count, and
# digest.  This catches accidental printf arity regressions that would leave a
# hash on a detached malformed row while still allowing artifact rows through.
for input in layout kernel_manifest rootfs_manifest; do
  case "$input" in
    layout) input_file="$layout" ;;
    kernel_manifest) input_file="$kernel_manifest" ;;
    rootfs_manifest) input_file="$rootfs_manifest" ;;
  esac
  input_bytes=$(file_bytes "$input_file")
  input_hash=$(file_hash "$input_file")
  awk -F "$tab" -v name="$input" -v path="$input_file" \
    -v bytes="$input_bytes" -v hash="$input_hash" '
    $1 == "input" && $2 == name && NF == 5 && $3 == path && $4 == bytes && $5 == hash { found=1 }
    END { exit(found ? 0 : 1) }
  ' "$artifact_manifest"
done

for partition in boot dtbo vendor_kernel_boot vendor_dlkm system_dlkm init_boot vendor_boot rootfs; do
  awk -F "$tab" -v partition="$partition" '
    $1 == "artifact" && $2 == partition && NF == 7 && $4 ~ /^[0-9]+$/ && $5 ~ /^[0-9a-f]{64}$/ { found=1 }
    END { exit(found ? 0 : 1) }
  ' "$artifact_manifest"
done

rootfs_bytes=$(file_bytes "$image/rootfs.img")
rootfs_hash=$(file_hash "$image/rootfs.img")
awk -F "$tab" -v rootfs_file="$image/rootfs.tar" \
  -v bytes="$(file_bytes "$image/rootfs.tar")" -v hash="$(file_hash "$image/rootfs.tar")" '
  $1 == "artifact" && $2 == "rootfs_handoff" && $3 == "rootfs.tar" && $4 == bytes && $5 == hash && $6 == "rootfs" && $7 == rootfs_file { found=1 }
  END { exit(found ? 0 : 1) }
' "$boot_manifest"
awk -F "$tab" -v boot_manifest="$boot_manifest" -v rootfs_file="$image/rootfs.img" \
  -v bytes="$rootfs_bytes" -v hash="$rootfs_hash" '
  $1 == "artifact" && $2 == "rootfs" && $3 == "rootfs.img" && $4 == bytes && $5 == hash && $6 == boot_manifest && $7 == rootfs_file { found=1 }
  END { exit(found ? 0 : 1) }
' "$artifact_manifest"

printf '%s\n' 'pixel-9a artifact workflow tests passed'
