#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script="$root/tools/pixel-9a/boot-radio.sh"
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-pixel9a-boot-radio.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
tab=$(printf '\t')
fail() { printf 'radio boot workflow test failed: %s\n' "$*" >&2; exit 1; }
[ -x "$script" ] || fail 'radio boot workflow is not executable'
sh -n "$script" || fail 'shell syntax'
if grep -nE '(^|[^[:alnum:]_-])(adb|fastboot|pmbootstrap[[:space:]]+flasher|flash|erase|format|reboot)([^[:alnum:]_-]|$)' "$script" >/dev/null; then fail 'device or destructive token'; fi
grep -q 'check_manifest_split_contract' "$script" || fail 'radio boot does not verify split-artifact contract provenance'
grep -q 'split_artifact_contract_sha256' "$script" || fail 'radio boot does not verify split-artifact contract hashes'

mkdir -p "$work/bin" "$work/out/pixel-9a/kernel-radio/tegu-wwan/dist" "$work/out/pixel-9a/rootfs/tegu"
printf '%s\n' '#!/bin/sh' 'output=' 'while [ "$#" -gt 0 ]; do case "$1" in --output|--vendor_boot) output=$2; shift 2;; *) shift;; esac; done' '[ -n "$output" ]' 'printf synthetic >"$output"' >"$work/bin/mkbootimg"
printf '%s\n' '#!/bin/sh' 'output=' 'while [ "$#" -gt 0 ]; do case "$1" in --output) output=$2; shift 2;; *) shift;; esac; done' '[ -n "$output" ]' 'printf vbmeta >"$output"' >"$work/bin/avbtool"
chmod 0755 "$work/bin/mkbootimg" "$work/bin/avbtool"

kernel="$work/out/pixel-9a/kernel-radio/tegu-wwan"; dist="$kernel/dist"
printf image >"$dist/Image"; printf dtb >"$dist/dtb.img"
mkdir -p "$dist/split-inputs/dtbo-source-set" "$dist/split-inputs/module-install"
printf dtb >"$dist/split-inputs/dtbo-source-set/tegu.dtb"
printf dtbo >"$dist/split-inputs/dtbo-source-set/tegu.dtbo"
dtb_bytes=$(wc -c <"$dist/split-inputs/dtbo-source-set/tegu.dtb" | tr -d '[:space:]')
dtb_hash=$(sha256sum "$dist/split-inputs/dtbo-source-set/tegu.dtb" | awk '{print $1}')
dtbo_bytes=$(wc -c <"$dist/split-inputs/dtbo-source-set/tegu.dtbo" | tr -d '[:space:]')
dtbo_hash=$(sha256sum "$dist/split-inputs/dtbo-source-set/tegu.dtbo" | awk '{print $1}')
printf 'schema_version\t1\nsource_root\tarch/arm64/boot/dts\nsource\ttegu.dtb\t%s\t%s\nsource\ttegu.dtbo\t%s\t%s\n' \
	"$dtb_bytes" "$dtb_hash" "$dtbo_bytes" "$dtbo_hash" >"$dist/split-inputs/dtbo-source-set.tsv"
printf 'schema_version\t1\ninstall_root\tsplit-inputs/module-install\n' >"$dist/split-inputs/module-install.tsv"
{
	printf 'schema_version\t1\n'
	for partition in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
		printf 'partition\t%s\tstatus=not-built\n' "$partition"
	done
} >"$dist/split-inputs/partition-sizes.tsv"
contract="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/radio-split-artifact-contract.toml"
contract_hash=$(sha256sum "$contract" | awk '{print $1}')
{
 printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\nroute_id\texperimental-linux-google-tegu-s5300-wwan\nkernel_backend\tlinux-google-tegu-wwan\nfirmware_mapping\tfirmware-radio-route.tsv\t%s\ninitfs_module_list\tpmos/device-google-tegu/modules-initfs-radio\t%s\nstatus\tbuilt-not-booted\nbootable\tfalse\nbuild_system\tkbuild-make\n' "$(sha256sum "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/firmware-radio-route.tsv" | awk '{print $1}')" "$(sha256sum "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/modules-initfs-radio" | awk '{print $1}')"
 printf 'split_artifact_contract\tradio-split-artifact-contract.toml\t%s\n' "$contract_hash"
 for input in dtbo-source-set module-install partition-sizes; do
	file="$dist/split-inputs/$input.tsv"
	printf '%s\tsplit-inputs/%s.tsv\t%s\t%s\n' "$(printf '%s' "$input" | tr '-' '_')" "$input" "$(wc -c <"$file" | tr -d '[:space:]')" "$(sha256sum "$file" | awk '{print $1}')"
 done
 printf 'artifact\tImage\t%s\t%s\n' "$(wc -c <"$dist/Image" | tr -d '[:space:]')" "$(sha256sum "$dist/Image" | awk '{print $1}')"
 printf 'artifact\tdtb.img\t%s\t%s\n' "$(wc -c <"$dist/dtb.img" | tr -d '[:space:]')" "$(sha256sum "$dist/dtb.img" | awk '{print $1}')"
} >"$kernel/build-manifest.tsv"

rootfs="$work/out/pixel-9a/rootfs/tegu"; printf rootfs >"$rootfs/rootfs.tar"; printf image >"$rootfs/rootfs.img"
{
	printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\ncodename\ttegu\nhardware_revision\tMP1.0\nsubstrate\tpostmarketos\nkernel_backend\tlinux-google-tegu-wwan\nstatus\tbuilt-substrate-only\nbootable\tfalse\n'
	printf 'split_artifact_contract\tradio-split-artifact-contract.toml\t%s\n' "$contract_hash"
 printf 'artifact\trootfs.tar\t%s\t%s\n' "$(wc -c <"$rootfs/rootfs.tar" | tr -d '[:space:]')" "$(sha256sum "$rootfs/rootfs.tar" | awk '{print $1}')"
 printf 'artifact\trootfs.img\t%s\t%s\n' "$(wc -c <"$rootfs/rootfs.img" | tr -d '[:space:]')" "$(sha256sum "$rootfs/rootfs.img" | awk '{print $1}')"
} >"$rootfs/manifest.tsv"
printf initramfs >"$work/initramfs"; printf vendor >"$work/vendor"; printf config >"$work/config"
common="PATH=$work/bin:$PATH FERRIX_OUT_DIR=$work/out FERRIX_PIXEL9A_RADIO_KERNEL_DIST=$dist FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST=$kernel/build-manifest.tsv FERRIX_PIXEL9A_ROOTFS_OUT=$rootfs FERRIX_PIXEL9A_ROOTFS_MANIFEST=$rootfs/manifest.tsv FERRIX_PIXEL9A_INITRAMFS=$work/initramfs FERRIX_PIXEL9A_VENDOR_RAMDISK=$work/vendor FERRIX_PIXEL9A_VENDOR_BOOTCONFIG=$work/config FERRIX_PIXEL9A_RADIO_BOOT_OUT=$work/out/images"

cp "$dist/split-inputs/dtbo-source-set.tsv" "$work/valid-dtbo-source-set.tsv"
awk -F "$tab" -v OFS="$tab" '$1 != "source" || $2 != "tegu.dtbo" { print }' \
	"$work/valid-dtbo-source-set.tsv" >"$dist/split-inputs/dtbo-source-set.tsv"
no_dtbo_bytes=$(wc -c <"$dist/split-inputs/dtbo-source-set.tsv" | tr -d '[:space:]')
no_dtbo_hash=$(sha256sum "$dist/split-inputs/dtbo-source-set.tsv" | awk '{print $1}')
awk -F "$tab" -v OFS="$tab" -v bytes="$no_dtbo_bytes" -v hash="$no_dtbo_hash" \
	'$1 == "dtbo_source_set" { $3=bytes; $4=hash } { print }' \
	"$kernel/build-manifest.tsv" >"$work/no-dtbo-build-manifest.tsv"
if env $common FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$work/no-dtbo-build-manifest.tsv" \
	"$script" preflight >"$work/no-dtbo.out" 2>"$work/no-dtbo.err"; then
	fail 'accepted a DTB-only source set without a DTBO overlay'
fi
grep -q 'contains no source-built DTBO overlay inputs' "$work/no-dtbo.err" || fail 'DTB-only refusal reason'
cp "$work/valid-dtbo-source-set.tsv" "$dist/split-inputs/dtbo-source-set.tsv"

sed "s/^build_system${tab}kbuild-make$/build_system${tab}google-kleaf-bazel/" "$kernel/build-manifest.tsv" >"$work/google.tsv"
if env $common FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$work/google.tsv" "$script" preflight >/dev/null 2>"$work/google.err"; then fail 'accepted Google Kleaf manifest'; fi
grep -q 'non-Kbuild kernel manifest' "$work/google.err" || fail 'Google refusal reason'

awk -F "$tab" -v OFS="$tab" \
	'!($1 == "partition" && $2 == "system_dlkm") { print }' \
	"$dist/split-inputs/partition-sizes.tsv" >"$work/missing-partition-sizes.tsv"
cp "$work/missing-partition-sizes.tsv" "$dist/split-inputs/missing-partition-sizes.tsv"
missing_partition_relative=split-inputs/missing-partition-sizes.tsv
missing_partition_bytes=$(wc -c <"$work/missing-partition-sizes.tsv" | tr -d '[:space:]')
missing_partition_hash=$(sha256sum "$work/missing-partition-sizes.tsv" | awk '{print $1}')
awk -F "$tab" -v OFS="$tab" -v path="$missing_partition_relative" \
	-v bytes="$missing_partition_bytes" -v hash="$missing_partition_hash" \
	'$1 == "partition_sizes" { $2=path; $3=bytes; $4=hash } { print }' \
	"$kernel/build-manifest.tsv" >"$work/missing-partition.tsv"
if env $common FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$work/missing-partition.tsv" \
	"$script" preflight >"$work/missing-partition.out" 2>"$work/missing-partition.err"; then
	fail 'accepted a radio kernel manifest with an incomplete partition-size handoff'
fi
grep -q 'partition_sizes must contain all 10 split partitions' "$work/missing-partition.err" ||
	fail 'incomplete partition-size refusal reason'

env $common FERRIX_PIXEL9A_RADIO_BOOT_BUILD=I-understand-this-composes-pixel-9a-radio-first-boot-artifacts "$script" build >/dev/null
[ -f "$work/out/images/boot-manifest.tsv" ] || fail 'radio boot manifest missing'
awk -F "$tab" '$1 == "kernel_backend" && $2 == "linux-google-tegu-wwan" { found=1 } END { exit(found ? 0 : 1) }' "$work/out/images/boot-manifest.tsv"
awk -F "$tab" '$1 == "partition_contract" && $2 == "incomplete" { found=1 } END { exit(found ? 0 : 1) }' "$work/out/images/boot-manifest.tsv"
awk -F "$tab" '$1 == "missing_partition" && $2 == "vendor_kernel_boot" { found=1 } END { exit(found ? 0 : 1) }' "$work/out/images/boot-manifest.tsv"
printf '%s\n' 'pixel-9a radio boot workflow tests passed'
