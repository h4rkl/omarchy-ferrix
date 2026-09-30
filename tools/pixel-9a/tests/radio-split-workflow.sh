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
vendor_output=
vendor_ramdisk=
pagesize=
dtb=
vendor_cmdline=
while [ "$#" -gt 0 ]; do
	case "$1" in
		--output) output=$2; shift 2 ;;
		--vendor_boot) vendor_output=$2; shift 2 ;;
		--vendor_ramdisk) vendor_ramdisk=$2; shift 2 ;;
		--pagesize) pagesize=$2; shift 2 ;;
		--dtb) dtb=$2; shift 2 ;;
		--vendor_cmdline) vendor_cmdline=$2; shift 2 ;;
		*) shift ;;
	esac
done
if [ -n "$vendor_ramdisk" ]; then
	[ -n "$vendor_output" ] && [ -z "$output" ] || {
		printf '%s\n' 'vendor ramdisk requires --vendor_boot, not --output' >&2
		exit 1
	}
	[ "$pagesize" = 2048 ] || exit 1
	case "$vendor_output" in
		*/vendor_boot.img)
			[ -z "$dtb" ] && [ -n "$vendor_cmdline" ] || exit 1 ;;
		*/vendor_kernel_boot.img)
			[ -n "$dtb" ] && [ -z "$vendor_cmdline" ] || exit 1 ;;
		*) exit 1 ;;
	esac
	printf 'VNDRBOOTsynthetic\n' >"$vendor_output"
else
	[ -n "$output" ] && [ "$pagesize" = 4096 ]
	printf 'ANDROID!synthetic\n' >"$output"
fi
EOF
cat >"$work/bin/mkdtboimg.py" <<'EOF'
#!/bin/sh
set -eu
[ "${1:-}" = create ]
output=${2:-}
[ -n "$output" ]
	[ "$#" -ge 4 ]
case "$*" in
	*tegu.dtbo*) ;;
	*) printf '%s\n' 'mkdtboimg received no source-built DTBO input' >&2; exit 1 ;;
esac
printf 'synthetic-mkdtboimg\n' >"$output"
EOF
cat >"$work/bin/mke2fs" <<'EOF'
#!/bin/sh
set -eu
output=
block_size=1024
blocks=
while [ "$#" -gt 0 ]; do
	case "$1" in
		-b) block_size=$2; shift 2 ;;
		-d|-t|-L|-U) shift 2 ;;
		-*) shift ;;
		*) if [ -z "$output" ]; then output=$1; else blocks=$1; fi; shift ;;
	esac
done
[ -n "$output" ] && [ -n "$blocks" ]
[ "$block_size" = 4096 ] || { echo 'explicit ext4 block size required' >&2; exit 1; }
dd if=/dev/zero of="$output" bs="$block_size" count=0 seek="$blocks" 2>/dev/null
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
partition_size=
calculate=no
append=yes
while [ "$#" -gt 0 ]; do
	case "$1" in
		--image) image=$2; shift 2 ;;
		--output) output=$2; shift 2 ;;
		--partition_size) partition_size=$2; shift 2 ;;
		--calc_max_image_size) calculate=yes; shift ;;
		--do_not_append_vbmeta_image) append=no; shift ;;
		--include_descriptors_from_image)
			grep -aq footer "$2" || { echo 'no embedded AVB descriptor' >&2; exit 1; }
			printf 'include\t%s\n' "$2" >>"$log"
			descriptors="$descriptors ${2##*/}"
			shift 2 ;;
		*) shift ;;
	esac
done
case "$command" in
	add_hash_footer)
		if [ "$calculate" = yes ]; then
			printf '%s\n' "$((partition_size - 69632))"
			exit 0
		fi
		[ -f "$image" ]
		[ "$(wc -c <"$image" | tr -d '[:space:]')" -le "$((partition_size - 69632))" ] || {
			echo 'no room for AVB metadata' >&2; exit 1
		}
		if [ "$append" = yes ]; then printf 'footer\n' >>"$image"; fi
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
	verify_image)
		[ -f "$image" ]
		[ "${STUB_AVB_VERIFY_FAIL:-0}" = 0 ] || exit 1
		for descriptor in $(tr ' ' '\n' <"$image"); do
			grep -aq footer "$(dirname -- "$image")/$descriptor" || exit 1
		done
		printf 'verify_image\t%s\n' "$image" >>"$log"
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

boot_inputs="$work/boot-inputs"
mkdir -p "$boot_inputs/vendor-dlkm/lib/modules/6.12.0-ferrix" "$boot_inputs/system-dlkm/lib/modules/6.12.0-ferrix" "$boot_inputs/vendor-ramdisk"
cp "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/initramfs-init" "$boot_inputs/initramfs.cpio.source"
cp "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/vendor-bootconfig" "$boot_inputs/vendor-bootconfig.source"
cp "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/modules-initfs-radio" "$boot_inputs/modules-initfs.policy"
cp "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/modules-vendor-dlkm" "$boot_inputs/modules-vendor.policy"
cp "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/modules-system-dlkm" "$boot_inputs/modules-system.policy"
cp "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/vendor-ramdisk/ferrix/route-id" "$boot_inputs/vendor-ramdisk/route-id"
cp "$work/initramfs" "$boot_inputs/initramfs.cpio"
cp "$work/vendor-ramdisk" "$boot_inputs/vendor-ramdisk.cpio"
cp "$work/vendor-kernel-boot.cpio" "$boot_inputs/vendor-kernel-boot.cpio"
cp "$work/bootconfig" "$boot_inputs/vendor-bootconfig"
awk 'NF && $1 !~ /^#/ { printf "%s%s", separator, $0; separator=" " } END { print "" }' \
	"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/kernel-cmdline.conf" >"$boot_inputs/vendor-cmdline"
printf '%s\n' '# synthetic depmod' >"$boot_inputs/vendor-dlkm/lib/modules/6.12.0-ferrix/modules.dep"
printf '%s\n' '# synthetic depmod' >"$boot_inputs/system-dlkm/lib/modules/6.12.0-ferrix/modules.dep"
{
	printf 'schema_version\t1\n'
	printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
	printf 'route_id\texperimental-linux-google-tegu-s5300-wwan\n'
	printf 'kernel_backend\tlinux-google-tegu-wwan\n'
	printf 'status\tbuilt-source-inputs\n'
	printf 'bootable\tfalse\n'
	printf 'kernel_release\t6.12.0-ferrix\n'
	printf 'kernel_manifest\t%s\t%s\t%s\n' "$kernel/build-manifest.tsv" "$(wc -c <"$kernel/build-manifest.tsv" | tr -d '[:space:]')" "$(sha256sum "$kernel/build-manifest.tsv" | awk '{ print $1 }')"
	printf 'rootfs_manifest\t%s\t%s\t%s\n' "$rootfs/manifest.tsv" "$(wc -c <"$rootfs/manifest.tsv" | tr -d '[:space:]')" "$(sha256sum "$rootfs/manifest.tsv" | awk '{ print $1 }')"
	for row in \
		"initramfs_source|$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/initramfs-init" \
		"vendor_bootconfig_source|$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/vendor-bootconfig" \
		"kernel_cmdline_source|$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/kernel-cmdline.conf" \
		"modules_initfs_policy|$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/modules-initfs-radio" \
		"modules_vendor_policy|$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/modules-vendor-dlkm" \
		"modules_system_policy|$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/modules-system-dlkm"; do
		key=${row%%|*}; source=${row#*|}
		printf 'input\t%s\t%s\t%s\t%s\n' "$key" "$source" "$(wc -c <"$source" | tr -d '[:space:]')" "$(sha256sum "$source" | awk '{ print $1 }')"
	done
	printf 'input\tvendor_ramdisk_source\t%s\n' "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/vendor-ramdisk"
	for row in \
		'initramfs.cpio|source-built-initramfs' \
		'vendor-ramdisk.cpio|source-built-vendor-ramdisk' \
		'vendor-kernel-boot.cpio|source-built-early-modules' \
		'vendor-bootconfig|source-owned-bootconfig' \
		'vendor-cmdline|source-owned-kernel-cmdline'; do
		name=${row%%|*}; origin=${row#*|}; file="$boot_inputs/$name"
		printf 'artifact\t%s\t%s\t%s\t%s\t%s\n' "$name" "$name" "$(wc -c <"$file" | tr -d '[:space:]')" "$(sha256sum "$file" | awk '{ print $1 }')" "$origin"
	done
	for name in vendor_dlkm system_dlkm; do
		relative=${name%_dlkm}-dlkm
		printf 'module_root\t%s\t%s\t%s\t%s\n' "$name" "$relative" "$(sha256sum "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/modules-${name%_dlkm}-dlkm" | awk '{ print $1 }')" "$(wc -c <"$boot_inputs/$relative/lib/modules/6.12.0-ferrix/modules.dep" | tr -d '[:space:]')"
	done
	for tree_name in vendor_ramdisk_source vendor_dlkm system_dlkm; do
		case "$tree_name" in
			vendor_ramdisk_source) tree_root="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/vendor-ramdisk" ;;
			*) tree_root="$boot_inputs/${tree_name%_dlkm}-dlkm" ;;
		esac
		find "$tree_root" -type f -print | LC_ALL=C sort | while IFS= read -r file; do
			printf 'tree_file\t%s\t%s\t%s\t%s\n' "$tree_name" "${file#"$tree_root"/}" "$(wc -c <"$file" | tr -d '[:space:]')" "$(sha256sum "$file" | awk '{ print $1 }')"
		done
	done
} >"$boot_inputs/manifest.tsv"

avb_log="$work/avb.log"
: >"$avb_log"
common="PATH=$work/bin:$PATH AVB_LOG=$avb_log FERRIX_OUT_DIR=$work/out FERRIX_PIXEL9A_RADIO_KERNEL_DIST=$dist FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST=$kernel/build-manifest.tsv FERRIX_PIXEL9A_ROOTFS_OUT=$rootfs FERRIX_PIXEL9A_ROOTFS_MANIFEST=$rootfs/manifest.tsv FERRIX_PIXEL9A_RADIO_PARTITION_SIZES=$partition_sizes FERRIX_PIXEL9A_BOOT_INPUTS_OUT=$boot_inputs FERRIX_PIXEL9A_BOOT_INPUTS_MANIFEST=$boot_inputs/manifest.tsv FERRIX_PIXEL9A_RADIO_SPLIT_OUT=$work/out/images/split"

env $common "$script" preflight >/dev/null
# Equal-length changes evade the old modules.dep size-only check.
dep_file="$boot_inputs/vendor-dlkm/lib/modules/6.12.0-ferrix/modules.dep"
cp "$dep_file" "$work/modules.dep.original"
printf '%s\n' '# synthetic change' >"$dep_file"
if env $common "$script" preflight >"$work/tree-tamper.out" 2>"$work/tree-tamper.err"; then
	fail 'accepted changed module-root bytes'
fi
grep -q 'vendor_dlkm tree file hash differs' "$work/tree-tamper.err" || fail 'module tree hash refusal missing'
cp "$work/modules.dep.original" "$dep_file"
printf extra >"$boot_inputs/system-dlkm/unlisted.ko"
if env $common "$script" preflight >"$work/tree-extra.out" 2>"$work/tree-extra.err"; then
	fail 'accepted an unlisted module-root file'
fi
grep -q 'system_dlkm tree file set differs' "$work/tree-extra.err" || fail 'extra module file refusal missing'
rm "$boot_inputs/system-dlkm/unlisted.ko"
env $common FERRIX_PIXEL9A_RADIO_SPLIT_BUILD=I-understand-this-composes-the-radio-first-split-artifacts "$script" build >/dev/null
manifest_out="$work/out/images/split/boot-manifest.tsv"
[ -f "$manifest_out" ] || fail 'split manifest missing'
for provenance in partition_capacities kernel_manifest rootfs_manifest boot_inputs_manifest vendor_kernel_boot_input; do
	awk -F "$tab" -v key="$provenance" '$1 == key && $2 ~ /^provenance\// { found=1 } END { exit(found ? 0 : 1) }' "$manifest_out" || fail "relative provenance row missing: $provenance"
done
for provenance_file in partition-capacities.tsv kernel-manifest.tsv rootfs-manifest.tsv boot-inputs-manifest.tsv vendor-kernel-boot.cpio; do
	[ -f "$work/out/images/split/provenance/$provenance_file" ] || fail "self-contained provenance file missing: $provenance_file"
done
for artifact in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
	awk -F "$tab" -v name="$artifact" '$1 == "artifact" && $2 == name { found=1 } END { exit(found ? 0 : 1) }' "$manifest_out" || fail "missing artifact row: $artifact"
done
for partition in vendor_boot vendor_kernel_boot; do
	[ "$(dd if="$work/out/images/split/$partition.img" bs=8 count=1 2>/dev/null)" = VNDRBOOT ] ||
		fail "$partition is not a vendor boot container"
done
awk -F "$tab" '$1 == "artifact" && $2 == "vendor_kernel_boot" && $6 == "generated-mkbootimg" { found=1 } END { exit(found ? 0 : 1) }' "$manifest_out" || fail 'vendor kernel boot container provenance missing'
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
	grep -q "verify_image.*$artifact.img" "$avb_log" || fail "AVB payload verification missing: $artifact"
done
if env $common STUB_AVB_VERIFY_FAIL=1 FERRIX_PIXEL9A_RADIO_SPLIT_OUT="$work/out/images/bad-avb" \
	FERRIX_PIXEL9A_RADIO_SPLIT_BUILD=I-understand-this-composes-the-radio-first-split-artifacts \
	"$script" build >"$work/bad-avb.out" 2>"$work/bad-avb.err"; then
	fail 'published images despite failed AVB payload verification'
fi
grep -q 'failed AVB payload verification' "$work/bad-avb.err" || fail 'AVB payload refusal reason'
[ ! -e "$work/out/images/bad-avb" ] || fail 'failed AVB output was published'
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
