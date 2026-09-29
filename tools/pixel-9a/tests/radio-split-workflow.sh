#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script="$root/tools/pixel-9a/radio-split.sh"
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-pixel9a-radio-split.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
tab=$(printf '\t')

fail() {
	printf 'radio split workflow test failed: %s\n' "$*" >&2
	exit 1
}

[ -x "$script" ] || fail 'radio split workflow is not executable'
sh -n "$script" || fail 'shell syntax'
if grep -nE '(^|[^[:alnum:]_-])(adb|fastboot)([^[:alnum:]_-]|$)' "$script" >/dev/null; then
	fail 'device transport token'
fi
grep -q 'partition capacity handoff' "$script" || fail 'explicit partition capacity gate missing'
grep -q 'complete-source-built' "$script" || fail 'complete source-built manifest marker missing'

mkdir -p "$work/bin" \
	"$work/out/pixel-9a/kernel-radio/tegu-wwan/dist/split-inputs/dtbo-source-set" \
	"$work/out/pixel-9a/kernel-radio/tegu-wwan/dist/split-inputs/module-install" \
	"$work/out/pixel-9a/rootfs/tegu" \
		"$work/vendor-dlkm/lib/modules" "$work/system-dlkm/lib/modules"

cat >"$work/bin/mkbootimg" <<'EOF'
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
printf 'synthetic-mkbootimg\n' >"$output"
EOF
cat >"$work/bin/mkdtboimg.py" <<'EOF'
#!/bin/sh
set -eu
[ "${1:-}" = create ]
output=${2:-}
[ -n "$output" ]
printf 'synthetic-mkdtboimg\n' >"$output"
EOF
cat >"$work/bin/mke2fs" <<'EOF'
#!/bin/sh
set -eu
output=
skip=
for arg in "$@"; do
	if [ "$skip" = yes ]; then
		skip=
		continue
	fi
	case "$arg" in
		-d) skip=yes ;;
		[0-9]*) : ;;
		-*) : ;;
		*) output=$arg ;;
	esac
done
[ -n "$output" ]
printf 'synthetic-ext4\n' >"$output"
EOF
cat >"$work/bin/avbtool" <<'EOF'
#!/bin/sh
set -eu
log=${AVB_LOG:?}
command=${1:-}
shift
image=
output=
descriptors=
while [ "$#" -gt 0 ]; do
	case "$1" in
		--image) image=$2; shift 2 ;;
		--output) output=$2; shift 2 ;;
		--include_descriptors_from_image)
			printf 'include\t%s\n' "$2" >>"$log"
			descriptors="$descriptors ${2##*/}"
			shift 2 ;;
		*) shift ;;
	esac
done
case "$command" in
	add_hash_footer)
		[ -f "$image" ]
		printf 'footer\n' >>"$image"
		printf 'add_hash_footer\t%s\n' "$image" >>"$log"
		;;
	make_vbmeta_image)
		[ -n "$output" ]
		printf '%s\n' "$descriptors" >"$output"
		printf 'make_vbmeta_image\t%s\n' "$output" >>"$log"
		;;
	info_image)
		[ -f "$image" ]
		printf 'Descriptors:\n'
		for descriptor in $(tr ' ' '\n' <"$image"); do
			case "$descriptor" in
				boot.img) printf 'Partition Name: boot\n' ;;
				init_boot.img) printf 'Partition Name: init_boot\n' ;;
				vendor_boot.img) printf 'Partition Name: vendor_boot\n' ;;
				vendor_kernel_boot.img) printf 'Partition Name: vendor_kernel_boot\n' ;;
				dtbo.img) printf 'Partition Name: dtbo\n' ;;
				vendor_dlkm.img) printf 'Partition Name: vendor_dlkm\n' ;;
				system_dlkm.img) printf 'Partition Name: system_dlkm\n' ;;
			esac
		done
		;;
	*) exit 2 ;;
esac
EOF
cat >"$work/bin/uname" <<'EOF'
#!/bin/sh
case "${1:-}" in
	-m) printf '%s\n' x86_64 ;;
	*) printf '%s\n' Linux ;;
esac
EOF
chmod 0755 "$work/bin/mkbootimg" "$work/bin/mkdtboimg.py" "$work/bin/mke2fs" "$work/bin/avbtool"
chmod 0755 "$work/bin/uname"

kernel="$work/out/pixel-9a/kernel-radio/tegu-wwan"
dist="$kernel/dist"
rootfs="$work/out/pixel-9a/rootfs/tegu"
printf image >"$dist/Image"
printf dtb >"$dist/dtb.img"
printf dtbo >"$dist/split-inputs/dtbo-source-set/tegu.dtbo"
printf dtb >"$dist/split-inputs/dtbo-source-set/tegu.dtb"
printf module >"$dist/split-inputs/module-install/s5300_modem.ko"
printf 'schema_version\t1\nsource_root\tarch/arm64/boot/dts\nsource\ttegu.dtb\t%s\t%s\nsource\ttegu.dtbo\t%s\t%s\n' \
	"$(wc -c <"$dist/split-inputs/dtbo-source-set/tegu.dtb" | tr -d '[:space:]')" \
	"$(sha256sum "$dist/split-inputs/dtbo-source-set/tegu.dtb" | awk '{ print $1 }')" \
	"$(wc -c <"$dist/split-inputs/dtbo-source-set/tegu.dtbo" | tr -d '[:space:]')" \
	"$(sha256sum "$dist/split-inputs/dtbo-source-set/tegu.dtbo" | awk '{ print $1 }')" \
	>"$dist/split-inputs/dtbo-source-set.tsv"
printf 'schema_version\t1\ninstall_root\tsplit-inputs/module-install\nfile\ts5300_modem.ko\t%s\t%s\n' \
	"$(wc -c <"$dist/split-inputs/module-install/s5300_modem.ko" | tr -d '[:space:]')" \
	"$(sha256sum "$dist/split-inputs/module-install/s5300_modem.ko" | awk '{ print $1 }')" \
	>"$dist/split-inputs/module-install.tsv"
printf rootfs >"$rootfs/rootfs.tar"
printf image >"$rootfs/rootfs.img"
contract="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/radio-split-artifact-contract.toml"
contract_hash=$(sha256sum "$contract" | awk '{ print $1 }')
{
	printf 'schema_version\t1\n'
	printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
	printf 'route_id\texperimental-linux-google-tegu-s5300-wwan\n'
	printf 'kernel_backend\tlinux-google-tegu-wwan\n'
	printf 'build_system\tkbuild-make\n'
	printf 'status\tbuilt-not-booted\n'
	printf 'bootable\tfalse\n'
	printf 'split_artifact_contract\tradio-split-artifact-contract.toml\t%s\n' "$contract_hash"
	for input in dtbo-source-set module-install; do
		file="$dist/split-inputs/$input.tsv"
		printf '%s\tsplit-inputs/%s.tsv\t%s\t%s\n' "$(printf '%s' "$input" | tr '-' '_')" "$input" \
			"$(wc -c <"$file" | tr -d '[:space:]')" "$(sha256sum "$file" | awk '{ print $1 }')"
	done
	printf 'artifact\tImage\t%s\t%s\n' "$(wc -c <"$dist/Image" | tr -d '[:space:]')" "$(sha256sum "$dist/Image" | awk '{ print $1 }')"
	printf 'artifact\tdtb.img\t%s\t%s\n' "$(wc -c <"$dist/dtb.img" | tr -d '[:space:]')" "$(sha256sum "$dist/dtb.img" | awk '{ print $1 }')"
} >"$kernel/build-manifest.tsv"
{
	printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
	printf 'codename\ttegu\n'
	printf 'hardware_revision\tMP1.0\n'
	printf 'substrate\tpostmarketos\n'
	printf 'kernel_backend\tlinux-google-tegu-wwan\n'
	printf 'status\tbuilt-substrate-only\n'
	printf 'bootable\tfalse\n'
	printf 'split_artifact_contract\tradio-split-artifact-contract.toml\t%s\n' "$contract_hash"
	for name in rootfs.tar rootfs.img; do
		printf 'artifact\t%s\t%s\t%s\n' "$name" "$(wc -c <"$rootfs/$name" | tr -d '[:space:]')" "$(sha256sum "$rootfs/$name" | awk '{ print $1 }')"
	done
} >"$rootfs/manifest.tsv"

partition_sizes="$work/partition-capacities.tsv"
{
	printf 'schema_version\t1\n'
	printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
	for partition in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
		printf 'partition\t%s\t1048576\n' "$partition"
	done
} >"$partition_sizes"
printf initramfs >"$work/initramfs"
printf vendor-ramdisk >"$work/vendor-ramdisk"
printf bootconfig >"$work/bootconfig"
printf vendor-kernel-boot >"$work/vendor-kernel-boot.cpio"

avb_log="$work/avb.log"
: >"$avb_log"
common="PATH=$work/bin:$PATH AVB_LOG=$avb_log FERRIX_OUT_DIR=$work/out FERRIX_PIXEL9A_RADIO_KERNEL_DIST=$dist FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST=$kernel/build-manifest.tsv FERRIX_PIXEL9A_ROOTFS_OUT=$rootfs FERRIX_PIXEL9A_ROOTFS_MANIFEST=$rootfs/manifest.tsv FERRIX_PIXEL9A_RADIO_PARTITION_SIZES=$partition_sizes FERRIX_PIXEL9A_VENDOR_KERNEL_BOOT_CPIO=$work/vendor-kernel-boot.cpio FERRIX_PIXEL9A_VENDOR_DLKM_ROOT=$work/vendor-dlkm FERRIX_PIXEL9A_SYSTEM_DLKM_ROOT=$work/system-dlkm FERRIX_PIXEL9A_INITRAMFS=$work/initramfs FERRIX_PIXEL9A_VENDOR_RAMDISK=$work/vendor-ramdisk FERRIX_PIXEL9A_VENDOR_BOOTCONFIG=$work/bootconfig FERRIX_PIXEL9A_RADIO_SPLIT_OUT=$work/out/images/split"

env $common "$script" preflight >/dev/null
env $common FERRIX_PIXEL9A_RADIO_SPLIT_BUILD=I-understand-this-composes-the-radio-first-split-artifacts "$script" build >/dev/null
manifest_out="$work/out/images/split/boot-manifest.tsv"
[ -f "$manifest_out" ] || fail 'split manifest missing'
for artifact in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
	awk -F "$tab" -v name="$artifact" '$1 == "artifact" && $2 == name { found=1 } END { exit(found ? 0 : 1) }' "$manifest_out" || fail "missing artifact row: $artifact"
done
awk -F "$tab" '$1 == "partition_contract" && $2 == "complete-source-built" { found=1 } END { exit(found ? 0 : 1) }' "$manifest_out" || fail 'complete source-built partition marker missing'
awk -F "$tab" '$1 == "status" && $2 == "composed-not-booted" { found=1 } END { exit(found ? 0 : 1) }' "$manifest_out" || fail 'status marker missing'
awk -F "$tab" '$1 == "bootable" && $2 == "false" { found=1 } END { exit(found ? 0 : 1) }' "$manifest_out" || fail 'bootable boundary missing'
awk -F "$tab" '$1 == "avb_mode" && $2 == "fresh-generated" { found=1 } END { exit(found ? 0 : 1) }' "$manifest_out" || fail 'AVB mode provenance missing'
awk -F "$tab" '$1 == "avb_algorithm" && $2 == "NONE" { found=1 } END { exit(found ? 0 : 1) }' "$manifest_out" || fail 'AVB algorithm provenance missing'
for artifact in vbmeta vbmeta_vendor vbmeta_system; do
	awk -F "$tab" -v name="$artifact" '$1 == "artifact" && $2 == name && $6 == "generated-avbtool-from-final-images" { found=1 } END { exit(found ? 0 : 1) }' "$manifest_out" || fail "AVB origin missing: $artifact"
done

for partition in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm; do
	awk -F "$tab" -v name="$partition" '$1 == "avb_bound_image" && $2 == name && $4 ~ /^[0-9]+$/ && $5 ~ /^[0-9a-f]{64}$/ { found=1 } END { exit(found ? 0 : 1) }' "$manifest_out" || fail "AVB binding row missing: $partition"
done
for partition in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm; do
	grep -q "add_hash_footer.*$partition" "$avb_log" || fail "fresh AVB footer generation missing: $partition"
done
for artifact in vbmeta vbmeta_vendor vbmeta_system; do
	grep -q "make_vbmeta_image.*$artifact.img" "$avb_log" || fail "fresh AVB metadata generation missing: $artifact"
done
printf forbidden >"$work/forbidden-avb.tsv"
if env $common FERRIX_PIXEL9A_AVB_INPUT_MANIFEST="$work/forbidden-avb.tsv" "$script" preflight >"$work/forbidden.out" 2>"$work/forbidden.err"; then
	fail 'accepted an external AVB input manifest'
fi
grep -q 'external AVB input manifests are forbidden' "$work/forbidden.err" || fail 'external AVB refusal reason'

sed '/^partition\tsystem_dlkm\t/d' "$partition_sizes" >"$work/missing-capacity.tsv"
if env $common FERRIX_PIXEL9A_RADIO_PARTITION_SIZES="$work/missing-capacity.tsv" "$script" preflight >"$work/missing.out" 2>"$work/missing.err"; then
	fail 'accepted incomplete partition capacities'
fi
grep -q 'must contain all 10 split partitions' "$work/missing.err" || fail 'incomplete capacity refusal reason'

printf '%s\n' 'pixel-9a radio split workflow tests passed'
