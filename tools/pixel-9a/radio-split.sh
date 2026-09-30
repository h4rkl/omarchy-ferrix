#!/bin/sh
set -eu

# Compose the missing split artifacts for the independent radio-first route.
#
# This workflow is deliberately separate from boot.sh and flash.sh.  It takes
# only source-built kernel/rootfs inputs and an explicit, redacted partition
# capacity handoff.  It never contacts a device and it never consumes stock
# Android modules or overlays.  The resulting manifest remains
# composed-not-booted/bootable=false; a future target-scoped physical route
# must review this manifest before using it.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
route="$target/kernel-radio-route.toml"
contract="$target/radio-split-artifact-contract.toml"
out_root=${FERRIX_OUT_DIR:-"$root/out"}
kernel_dist=${FERRIX_PIXEL9A_RADIO_KERNEL_DIST:-"$out_root/pixel-9a/kernel-radio/tegu-wwan/dist"}
kernel_manifest=${FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST:-"$out_root/pixel-9a/kernel-radio/tegu-wwan/build-manifest.tsv"}
rootfs_out=${FERRIX_PIXEL9A_ROOTFS_OUT:-"$out_root/pixel-9a/rootfs/tegu"}
rootfs_manifest=${FERRIX_PIXEL9A_ROOTFS_MANIFEST:-"$rootfs_out/manifest.tsv"}
image_out=${FERRIX_PIXEL9A_RADIO_SPLIT_OUT:-"$out_root/pixel-9a/images/tegu-wwan-split"}
manifest=${FERRIX_PIXEL9A_RADIO_SPLIT_MANIFEST:-"$image_out/boot-manifest.tsv"}
partition_sizes=${FERRIX_PIXEL9A_RADIO_PARTITION_SIZES:-}
boot_inputs_out=${FERRIX_PIXEL9A_BOOT_INPUTS_OUT:-"$out_root/pixel-9a/boot-inputs/tegu-wwan"}
boot_inputs_manifest=${FERRIX_PIXEL9A_BOOT_INPUTS_MANIFEST:-"$boot_inputs_out/manifest.tsv"}
vendor_kernel_boot_cpio=
vendor_dlkm_root=
system_dlkm_root=
initramfs=
vendor_ramdisk=
bootconfig=
mkbootimg_request=${FERRIX_PIXEL9A_MKBOOTIMG:-mkbootimg}
mkdtboimg_request=${FERRIX_PIXEL9A_MKDTBOIMG:-mkdtboimg.py}
mke2fs_request=${FERRIX_PIXEL9A_MKE2FS:-mke2fs}
avbtool_request=${FERRIX_PIXEL9A_AVBTOOL:-avbtool}
avb_algorithm=${FERRIX_PIXEL9A_AVB_ALGORITHM:-NONE}
avb_key=${FERRIX_PIXEL9A_AVB_KEY:-}
split_build_confirmation=I-understand-this-composes-the-radio-first-split-artifacts
tab=$(printf '\t')

die() {
	printf 'FAIL pixel-9a-radio-split: %s\n' "$*" >&2
	exit 1
}

sha256_of() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | awk '{ print $1 }'
	else
		shasum -a 256 "$1" | awk '{ print $1 }'
	fi
}

file_size() {
	wc -c <"$1" | tr -d '[:space:]'
}

route_value() {
	key=$1
	awk -v key="$key" '
		$0 ~ "^" key "[[:space:]]*=" {
			value=$0
			sub(/^[^=]*=[[:space:]]*/, "", value)
			sub(/[[:space:]]*#.*/, "", value)
			if (value ~ /^"/) {
				sub(/^"/, "", value)
				sub(/"[[:space:]]*$/, "", value)
			}
			print value
			found=1
			exit
		}
		END { if (!found) exit 1 }
	' "$route"
}

manifest_value() {
	awk -F "$tab" -v key="$2" '$1 == key { print $2; exit }' "$1"
}

manifest_artifact_row() {
	awk -F "$tab" -v name="$2" '$1 == "artifact" && $2 == name { print; found=1; exit } END { if (!found) exit 1 }' "$1"
}

field() {
	printf '%s\n' "$1" | awk -F "$tab" -v number="$2" '{ print $number }'
}

safe_relative() {
	case "$1" in
		''|/*|.|..|../*|*/../*|*/..|*' '*|*"$tab"*) die "unsafe relative path: $1" ;;
	esac
}

require_file() {
	path=$1
	label=$2
	[ -f "$path" ] || die "$label is missing: $path"
	[ ! -L "$path" ] || die "$label must not be a symlink: $path"
}

resolve_executable() {
	requested=$1
	label=$2
	case "$requested" in
		*/*)
			[ -x "$requested" ] || die "$label is not executable: $requested"
			printf '%s\n' "$requested"
			;;
		*)
			command -v "$requested" >/dev/null 2>&1 || die "missing $label '$requested'"
			command -v "$requested"
			;;
	esac
}

check_route() {
	require_file "$route" 'radio route metadata'
	require_file "$contract" 'radio split-artifact contract'
	[ "$(route_value target_id)" = google-pixel-9a-tegu-gxq96-mp1-0 ] || die 'radio route target is not the exact Pixel 9a'
	[ "$(route_value route_id)" = experimental-linux-google-tegu-s5300-wwan ] || die 'radio route is not the reviewed WWAN route'
	[ "$(route_value status)" = experimental-not-adopted ] || die 'radio route must remain experimental-not-adopted'
	expected_contract_hash=$(route_value split_artifact_contract_sha256) || die 'radio route lacks split-artifact contract hash'
	case "$expected_contract_hash" in ''|*[!0-9a-f]*) die 'radio split-artifact contract hash is malformed' ;; esac
	[ "${#expected_contract_hash}" -eq 64 ] || die 'radio split-artifact contract hash must contain 64 hexadecimal characters'
	[ "$(sha256_of "$contract")" = "$expected_contract_hash" ] || die 'radio split-artifact contract hash differs from route metadata'
	for required in \
		'source_only = true' \
		'stock_modules_may_be_reused = false' \
		'android_userspace_required = false' \
		'boot_header_version = 4' \
		'boot_page_size = 4096' \
		'vendor_boot_page_size = 2048' \
		'vendor_dtb_partition = "vendor_kernel_boot"' \
		'dtbo_source = "exynos/google/zumapro-tegu-mp.dtbo"' \
		'dtbo_board_id = 0x60b06' \
		'dtbo_board_rev = 0x10000' \
		'dtbo_tool = "mkdtboimg.py"' \
		'dtbo_page_size = 4096' \
		'dynamic_partition_transport = "fastbootd"' \
		'boot_input_generator = "tools/pixel-9a/boot-inputs.sh"' \
		'boot_input_manifest_schema = 1'; do
		grep -Fqx "$required" "$contract" || die "radio split-artifact contract is missing: $required"
	done
}

verify_artifact() {
	manifest_file=$1
	base=$2
	name=$3
	row=$(manifest_artifact_row "$manifest_file" "$name") || die "$name is missing from $manifest_file"
	relative=$(field "$row" 2)
	expected_bytes=$(field "$row" 3)
	expected_hash=$(field "$row" 4)
	safe_relative "$relative"
	case "$expected_bytes" in ''|*[!0-9]*) die "$name has an invalid byte count" ;; esac
	case "$expected_hash" in ''|*[!0-9a-f]*) die "$name has an invalid hash" ;; esac
	[ "${#expected_hash}" -eq 64 ] || die "$name has an invalid hash length"
	file="$base/$relative"
	require_file "$file" "$name artifact"
	[ "$(file_size "$file")" = "$expected_bytes" ] || die "$name size differs from its manifest"
	[ "$(sha256_of "$file")" = "$expected_hash" ] || die "$name hash differs from its manifest"
	printf '%s\n' "$file"
}

verify_split_input() {
	key=$1
	label=$2
	row=$(awk -F "$tab" -v key="$key" '$1 == key { print; found=1; exit } END { if (!found) exit 1 }' "$kernel_manifest") || die "$label lacks $key"
	relative=$(field "$row" 2)
	expected_bytes=$(field "$row" 3)
	expected_hash=$(field "$row" 4)
	safe_relative "$relative"
	case "$expected_bytes" in ''|*[!0-9]*) die "$label $key has an invalid byte count" ;; esac
	case "$expected_hash" in ''|*[!0-9a-f]*) die "$label $key has an invalid hash" ;; esac
	[ "${#expected_hash}" -eq 64 ] || die "$label $key has an invalid hash length"
	file="$kernel_dist/$relative"
	require_file "$file" "$label $key"
	[ "$(file_size "$file")" = "$expected_bytes" ] || die "$label $key size differs from its manifest"
	[ "$(sha256_of "$file")" = "$expected_hash" ] || die "$label $key hash differs from its manifest"
}

verify_dtbo_sources() {
	source_row=$(awk -F "$tab" '$1 == "dtbo_source_set" { print; found=1; exit } END { if (!found) exit 1 }' "$kernel_manifest") || die 'radio kernel manifest lacks dtbo_source_set'
	[ "$(field "$source_row" 2)" = split-inputs/dtbo-source-set.tsv ] || die 'dtbo source-set path is not canonical'
	source_manifest="$kernel_dist/$(field "$source_row" 2)"
	require_file "$source_manifest" 'DTB/DTBO source manifest'
	source_dir="$kernel_dist/split-inputs/dtbo-source-set"
	exact_dtbo_count=0
	while IFS="$tab" read -r kind relative bytes hash extra; do
		case "$kind" in
			''|'#'*) continue ;;
			schema_version) [ "$relative" = 1 ] && [ -z "${bytes:-}" ] && [ -z "${hash:-}" ] && [ -z "${extra:-}" ] || die 'DTB/DTBO source manifest schema row is malformed' ;;
			source)
				safe_relative "$relative"
				case "$relative" in *.dtbo|*.dtb) ;; *) die "DTB/DTBO source manifest contains a non-DTB path: $relative" ;; esac
				[ "$relative" != exynos/google/zumapro-tegu-mp.dtbo ] || exact_dtbo_count=$((exact_dtbo_count + 1))
				case "$bytes" in ''|*[!0-9]*) die "DTB/DTBO source manifest has invalid size: $relative" ;; esac
				case "$hash" in ''|*[!0-9a-f]*) die "DTB/DTBO source manifest has invalid hash: $relative" ;; esac
				[ "${#hash}" -eq 64 ] || die "DTB/DTBO source manifest has invalid hash length: $relative"
				[ -z "${extra:-}" ] || die "DTB/DTBO source manifest has extra fields: $relative"
				file="$source_dir/$relative"
				require_file "$file" 'source-built DTB/DTBO'
				[ "$(file_size "$file")" = "$bytes" ] || die "source-built DTB/DTBO size differs: $relative"
				[ "$(sha256_of "$file")" = "$hash" ] || die "source-built DTB/DTBO hash differs: $relative"
				;;
				source_root) [ "$relative" = arch/arm64/boot/dts ] && [ -z "${bytes:-}" ] && [ -z "${hash:-}" ] && [ -z "${extra:-}" ] || die 'DTB/DTBO source root is malformed' ;;
				*) die "DTB/DTBO source manifest has an unknown row: $kind" ;;
		esac
	done <"$source_manifest"
	[ "$exact_dtbo_count" -eq 1 ] || die 'exact tegu MP1.0 source-built DTBO overlay is missing or duplicated'
}

verify_partition_sizes() {
	[ -n "$partition_sizes" ] || die 'FERRIX_PIXEL9A_RADIO_PARTITION_SIZES is required; provide a redacted capacity handoff from the exact target'
	require_file "$partition_sizes" 'radio partition capacity handoff'
	schema_count=0
	target_count=0
	seen=''
	count=0
	while IFS="$tab" read -r kind value bytes extra; do
		case "$kind" in
			''|'#'*) continue ;;
			schema_version) [ "$value" = 1 ] && [ -z "${bytes:-}" ] && [ -z "${extra:-}" ] || die 'partition capacity schema row is malformed'; schema_count=$((schema_count + 1)) ;;
			target_id) [ "$value" = google-pixel-9a-tegu-gxq96-mp1-0 ] && [ -z "${bytes:-}" ] && [ -z "${extra:-}" ] || die 'partition capacity target row is malformed'; target_count=$((target_count + 1)) ;;
			partition)
				case "$value" in boot|init_boot|vendor_boot|vendor_kernel_boot|dtbo|vendor_dlkm|system_dlkm|vbmeta|vbmeta_vendor|vbmeta_system) ;; *) die "partition capacity names an unexpected partition: $value" ;; esac
				case " $seen " in *" $value "*) die "partition capacity repeats: $value" ;; esac
				case "$bytes" in ''|*[!0-9]*) die "partition capacity is not numeric: $value" ;; esac
				[ "$bytes" -gt 0 ] || die "partition capacity must be positive: $value"
				[ $((bytes % 4096)) -eq 0 ] || die "partition capacity must be 4096-byte aligned: $value"
				seen="$seen $value"
				count=$((count + 1))
				;;
			*) die "partition capacity has an unknown row: $kind" ;;
		esac
	done <"$partition_sizes"
	[ "$schema_count" -eq 1 ] || die 'partition capacity handoff must contain exactly one schema_version=1 row'
	[ "$target_count" -eq 1 ] || die 'partition capacity handoff must contain exactly one exact-target row'
	[ "$count" -eq 10 ] || die 'partition capacity handoff must contain all 10 split partitions'
	for partition in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
		case " $seen " in *" $partition "*) ;; *) die "partition capacity is missing: $partition" ;; esac
	done
}

partition_capacity() {
	awk -F "$tab" -v wanted="$1" '$1 == "partition" && $2 == wanted { print $3; exit }' "$partition_sizes"
}

verify_module_root() {
	root_dir=$1
	label=$2
	[ -n "$root_dir" ] || die "$label is required"
	[ -d "$root_dir" ] || die "$label is not a directory: $root_dir"
	[ ! -L "$root_dir" ] || die "$label must not be a symlink: $root_dir"
	if find "$root_dir" -type l -print -quit | grep -q .; then
		die "$label contains a symlink"
	fi
	if find "$root_dir" ! -type f ! -type d -print -quit | grep -q .; then
		die "$label contains a non-regular entry"
	fi
}

verify_boot_input_artifact() {
	name=$1
	row=$(awk -F "$tab" -v name="$name" '$1 == "artifact" && $2 == name { print; found=1; exit } END { if (!found) exit 1 }' "$boot_inputs_manifest") || die "source-owned boot-input manifest lacks artifact: $name"
	relative=$(field "$row" 3)
	expected_bytes=$(field "$row" 4)
	expected_hash=$(field "$row" 5)
	origin=$(field "$row" 6)
	safe_relative "$relative"
	case "$name:$relative" in
		initramfs.cpio:initramfs.cpio|vendor-ramdisk.cpio:vendor-ramdisk.cpio|vendor-kernel-boot.cpio:vendor-kernel-boot.cpio|vendor-bootconfig:vendor-bootconfig|vendor-cmdline:vendor-cmdline) ;;
		*) die "source-owned boot input $name has a non-canonical path: $relative" ;;
	esac
	case "$expected_bytes" in ''|*[!0-9]*) die "source-owned boot input $name has an invalid byte count" ;; esac
	case "$expected_hash" in ''|*[!0-9a-f]*) die "source-owned boot input $name has an invalid SHA-256" ;; esac
	[ "${#expected_hash}" -eq 64 ] || die "source-owned boot input $name has an invalid SHA-256 length"
	case "$name:$origin" in
		initramfs.cpio:source-built-initramfs|vendor-ramdisk.cpio:source-built-vendor-ramdisk|vendor-kernel-boot.cpio:source-built-early-modules|vendor-bootconfig:source-owned-bootconfig|vendor-cmdline:source-owned-kernel-cmdline) ;;
		*) die "source-owned boot input $name has an unreviewed origin: $origin" ;;
	esac
	file="$boot_inputs_out/$relative"
	require_file "$file" "source-owned boot input $name"
	[ "$(file_size "$file")" = "$expected_bytes" ] || die "source-owned boot input $name size differs from its manifest"
	[ "$(sha256_of "$file")" = "$expected_hash" ] || die "source-owned boot input $name SHA-256 differs from its manifest"
	printf '%s\n' "$file"
}

verify_boot_input_source() {
	key=$1
	checked_in=$2
	row=$(awk -F "$tab" -v key="$key" '$1 == "input" && $2 == key { print; found=1; exit } END { if (!found) exit 1 }' "$boot_inputs_manifest") || die "source-owned boot-input manifest lacks $key provenance"
	expected_bytes=$(field "$row" 4)
	expected_hash=$(field "$row" 5)
	case "$expected_bytes" in ''|*[!0-9]*) die "source-owned boot input $key has an invalid provenance byte count" ;; esac
	case "$expected_hash" in ''|*[!0-9a-f]*) die "source-owned boot input $key has an invalid provenance SHA-256" ;; esac
	require_file "$checked_in" "checked-in $key source"
	[ "$(file_size "$checked_in")" = "$expected_bytes" ] || die "checked-in $key source differs in size from the boot-input manifest"
	[ "$(sha256_of "$checked_in")" = "$expected_hash" ] || die "checked-in $key source differs from the boot-input manifest"
}

verify_boot_inputs() {
	require_file "$boot_inputs_manifest" 'source-owned boot-input manifest'
	[ "$(manifest_value "$boot_inputs_manifest" schema_version)" = 1 ] || die 'source-owned boot-input manifest schema_version is not 1'
	[ "$(manifest_value "$boot_inputs_manifest" target_id)" = google-pixel-9a-tegu-gxq96-mp1-0 ] || die 'source-owned boot-input manifest target is not exact Pixel 9a'
	[ "$(manifest_value "$boot_inputs_manifest" route_id)" = experimental-linux-google-tegu-s5300-wwan ] || die 'source-owned boot-input manifest route is not the selected route'
	[ "$(manifest_value "$boot_inputs_manifest" kernel_backend)" = linux-google-tegu-wwan ] || die 'source-owned boot-input manifest backend is not WWAN/SIT'
	[ "$(manifest_value "$boot_inputs_manifest" status)" = built-source-inputs ] || die 'source-owned boot-input manifest is not a completed source-input handoff'
	[ "$(manifest_value "$boot_inputs_manifest" bootable)" = false ] || die 'source-owned boot-input manifest must remain bootable=false'
	for source in initramfs_source vendor_bootconfig_source kernel_cmdline_source modules_initfs_policy modules_vendor_policy modules_system_policy; do
		case "$source" in
			initramfs_source) checked_in="$target/pmos/device-google-tegu/initramfs-init" ;;
			vendor_bootconfig_source) checked_in="$target/pmos/device-google-tegu/vendor-bootconfig" ;;
			kernel_cmdline_source) checked_in="$target/pmos/device-google-tegu/kernel-cmdline.conf" ;;
			modules_initfs_policy) checked_in="$target/pmos/device-google-tegu/modules-initfs-radio" ;;
			modules_vendor_policy) checked_in="$target/pmos/device-google-tegu/modules-vendor-dlkm" ;;
			modules_system_policy) checked_in="$target/pmos/device-google-tegu/modules-system-dlkm" ;;
		esac
		verify_boot_input_source "$source" "$checked_in"
	done
	verify_boot_input_tree vendor_ramdisk_source "$target/pmos/device-google-tegu/vendor-ramdisk"
	input_kernel_row=$(awk -F "$tab" '$1 == "kernel_manifest" { print; found=1; exit } END { if (!found) exit 1 }' "$boot_inputs_manifest") || die 'source-owned boot-input manifest lacks kernel manifest provenance'
	input_rootfs_row=$(awk -F "$tab" '$1 == "rootfs_manifest" { print; found=1; exit } END { if (!found) exit 1 }' "$boot_inputs_manifest") || die 'source-owned boot-input manifest lacks rootfs manifest provenance'
	for handoff in "$kernel_manifest" "$rootfs_manifest"; do
		require_file "$handoff" 'source handoff manifest'
	done
	[ "$(file_size "$kernel_manifest")" = "$(field "$input_kernel_row" 3)" ] || die 'radio kernel manifest differs from the boot-input provenance'
	[ "$(sha256_of "$kernel_manifest")" = "$(field "$input_kernel_row" 4)" ] || die 'radio kernel manifest hash differs from the boot-input provenance'
	[ "$(file_size "$rootfs_manifest")" = "$(field "$input_rootfs_row" 3)" ] || die 'rootfs manifest differs from the boot-input provenance'
	[ "$(sha256_of "$rootfs_manifest")" = "$(field "$input_rootfs_row" 4)" ] || die 'rootfs manifest hash differs from the boot-input provenance'
	kernel_release=$(manifest_value "$boot_inputs_manifest" kernel_release)
	case "$kernel_release" in ''|*[!A-Za-z0-9._+-]*) die 'source-owned boot-input kernel release is malformed' ;; esac
	initramfs=$(verify_boot_input_artifact initramfs.cpio)
	vendor_ramdisk=$(verify_boot_input_artifact vendor-ramdisk.cpio)
	vendor_kernel_boot_cpio=$(verify_boot_input_artifact vendor-kernel-boot.cpio)
	bootconfig=$(verify_boot_input_artifact vendor-bootconfig)
	vendor_cmdline_file=$(verify_boot_input_artifact vendor-cmdline)
	vendor_cmdline=$(cat "$vendor_cmdline_file")
	expected_cmdline=$(awk 'NF && $1 !~ /^#/ { printf "%s%s", separator, $0; separator=" " } END { print "" }' \
		"$target/pmos/device-google-tegu/kernel-cmdline.conf")
	[ "$vendor_cmdline" = "$expected_cmdline" ] || die 'vendor command line differs from checked-in source'
	for name in vendor_dlkm system_dlkm; do
		row=$(awk -F "$tab" -v name="$name" '$1 == "module_root" && $2 == name { print; found=1; exit } END { if (!found) exit 1 }' "$boot_inputs_manifest") || die "source-owned boot-input manifest lacks module root: $name"
		relative=$(field "$row" 3)
		safe_relative "$relative"
		case "$name:$relative" in
			vendor_dlkm:vendor-dlkm|system_dlkm:system-dlkm) ;;
			*) die "source-owned $name root has a non-canonical path: $relative" ;;
		esac
		root_dir="$boot_inputs_out/$relative"
		verify_module_root "$root_dir" "source-owned $name root"
		case "$name" in
			vendor_dlkm) policy="$target/pmos/device-google-tegu/modules-vendor-dlkm" ;;
			system_dlkm) policy="$target/pmos/device-google-tegu/modules-system-dlkm" ;;
		esac
		policy_hash=$(field "$row" 4)
		dep_bytes=$(field "$row" 5)
		case "$policy_hash" in ''|*[!0-9a-f]*) die "source-owned $name policy hash is malformed" ;; esac
		[ "${#policy_hash}" -eq 64 ] || die "source-owned $name policy hash length is malformed"
		case "$dep_bytes" in ''|*[!0-9]*) die "source-owned $name depmod byte count is malformed" ;; esac
		[ "$(sha256_of "$policy")" = "$policy_hash" ] || die "source-owned $name policy differs from its boot-input provenance"
		dep_file="$root_dir/lib/modules/$kernel_release/modules.dep"
		require_file "$dep_file" "source-owned $name depmod output"
		[ "$(file_size "$dep_file")" = "$dep_bytes" ] || die "source-owned $name depmod output differs from its boot-input provenance"
		verify_boot_input_tree "$name" "$root_dir"
		case "$name" in
			vendor_dlkm) vendor_dlkm_root=$root_dir ;;
			system_dlkm) system_dlkm_root=$root_dir ;;
		esac
	done
}

# Verify every file and reject unlisted additions, omissions and duplicates.
# This binds the actual module-root contents, including depmod output, rather
# than just trusting the module policy and a modules.dep byte count.
verify_boot_input_tree() (
	tree_name=$1
	tree_root=$2
	verify_module_root "$tree_root" "boot-input $tree_name tree"
	seen=''
	count=0
	while IFS="$tab" read -r kind name relative bytes hash extra; do
		[ "$kind" = tree_file ] && [ "$name" = "$tree_name" ] || continue
		safe_relative "$relative"
		case " $seen " in *" $relative "*) die "$tree_name tree repeats $relative" ;; esac
		seen="$seen $relative"
		[ -z "${extra:-}" ] || die "$tree_name tree row has extra fields"
		case "$bytes" in ''|*[!0-9]*) die "$tree_name tree has an invalid byte count" ;; esac
		case "$hash" in ''|*[!0-9a-f]*) die "$tree_name tree has an invalid SHA-256" ;; esac
		[ "${#hash}" -eq 64 ] || die "$tree_name tree has an invalid SHA-256 length"
		file="$tree_root/$relative"
		require_file "$file" "$tree_name tree file"
		[ "$(file_size "$file")" = "$bytes" ] || die "$tree_name tree file size differs: $relative"
		[ "$(sha256_of "$file")" = "$hash" ] || die "$tree_name tree file hash differs: $relative"
		count=$((count + 1))
	done <"$boot_inputs_manifest"
	actual_count=$(find "$tree_root" -type f -print | wc -l | tr -d '[:space:]')
	[ "$count" -gt 0 ] && [ "$count" = "$actual_count" ] || die "$tree_name tree file set differs from boot-input manifest"
)

# AVB metadata must be generated after the final split bytes exist.  A
# path/hash-only vbmeta handoff cannot prove that salted descriptors bind these
# newly composed bytes, so this route rejects external vbmeta inputs and
# generates fresh descriptors from the final images during `build`.
verify_avb_generation_contract() {
	[ -z "${FERRIX_PIXEL9A_AVB_INPUT_MANIFEST:-}" ] ||
		die 'external AVB input manifests are forbidden; generate descriptors from final split image bytes'
	[ -z "${FERRIX_PIXEL9A_AVB_INPUT_ROOT:-}" ] ||
		die 'external AVB input roots are forbidden; stock or prebuilt vbmeta images cannot bind this composition'
	case "$avb_algorithm" in
		''|*[!A-Za-z0-9_-]*) die 'FERRIX_PIXEL9A_AVB_ALGORITHM is malformed' ;;
	esac
	case "$avb_algorithm" in
		NONE)
			[ -z "$avb_key" ] || die 'FERRIX_PIXEL9A_AVB_KEY must be unset when AVB algorithm is NONE'
			;;
		*)
			[ -n "$avb_key" ] || die "FERRIX_PIXEL9A_AVB_KEY is required when AVB algorithm is $avb_algorithm"
			require_file "$avb_key" 'AVB signing key'
			;;
	esac
}

verify_required_inputs() {
	require_file "$kernel_manifest" 'radio kernel manifest'
	require_file "$rootfs_manifest" 'radio rootfs manifest'
	[ "$(manifest_value "$kernel_manifest" target_id)" = google-pixel-9a-tegu-gxq96-mp1-0 ] || die 'radio kernel manifest target is not exact Pixel 9a'
	[ "$(manifest_value "$kernel_manifest" kernel_backend)" = linux-google-tegu-wwan ] || die 'radio kernel manifest backend is not WWAN/SIT'
	[ "$(manifest_value "$kernel_manifest" build_system)" = kbuild-make ] || die 'radio kernel manifest is not a Kbuild handoff'
	[ "$(manifest_value "$kernel_manifest" status)" = built-not-booted ] || die 'radio kernel manifest is not a completed build handoff'
	[ "$(manifest_value "$kernel_manifest" bootable)" = false ] || die 'radio kernel manifest must remain bootable=false'
	[ "$(manifest_value "$rootfs_manifest" kernel_backend)" = linux-google-tegu-wwan ] || die 'radio rootfs manifest backend is not WWAN/SIT'
	[ "$(manifest_value "$rootfs_manifest" status)" = built-substrate-only ] || die 'radio rootfs manifest is not a completed substrate handoff'
	[ "$(manifest_value "$rootfs_manifest" bootable)" = false ] || die 'radio rootfs manifest must remain bootable=false'
	verify_artifact "$kernel_manifest" "$kernel_dist" Image >/dev/null
	verify_artifact "$kernel_manifest" "$kernel_dist" dtb.img >/dev/null
	verify_artifact "$rootfs_manifest" "$rootfs_out" rootfs.tar >/dev/null
	verify_artifact "$rootfs_manifest" "$rootfs_out" rootfs.img >/dev/null
	verify_split_input dtbo_source_set 'radio kernel manifest'
	verify_split_input module_install 'radio kernel manifest'
	verify_dtbo_sources
	verify_partition_sizes
	verify_avb_generation_contract
	for legacy in FERRIX_PIXEL9A_INITRAMFS FERRIX_PIXEL9A_VENDOR_RAMDISK FERRIX_PIXEL9A_VENDOR_BOOTCONFIG FERRIX_PIXEL9A_VENDOR_KERNEL_BOOT_CPIO FERRIX_PIXEL9A_VENDOR_DLKM_ROOT FERRIX_PIXEL9A_SYSTEM_DLKM_ROOT; do
		[ -z "$(printenv "$legacy" 2>/dev/null || true)" ] || die "$legacy is no longer accepted; run pixel-9a-boot-inputs-build for source-owned inputs"
	done
	verify_boot_inputs
}

check_tools() {
	for command_name in awk cat cp dd dirname find grep mkdir mktemp mv python3 rm tr wc; do
		command -v "$command_name" >/dev/null 2>&1 || die "missing prerequisite '$command_name'"
	done
	command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || die 'missing sha256sum or shasum'
}

check_host() {
	os=$(uname -s)
	arch=$(uname -m)
	[ "$os" = Linux ] || die "native Linux host required for split image construction; detected $os/$arch"
	[ "$arch" = x86_64 ] || die "native Linux x86_64 host required for split image construction; detected $os/$arch"
}

preflight() {
	check_tools
	check_host
	check_route
	avbtool=$(resolve_executable "$avbtool_request" avbtool)
	verify_required_inputs
	printf '%s\n' 'PASS pixel-9a-radio-split preflight: source-built inputs, exact partition capacities, and fresh AVB generation are ready'
}

record_artifact() {
	name=$1
	origin=$2
	file=$3
	[ -f "$file" ] || die "composed artifact is missing: $file"
	printf 'artifact\t%s\t%s\t%s\t%s\t%s\n' "$name" "${file#"$stage"/}" "$(file_size "$file")" "$(sha256_of "$file")" "$origin" >>"$manifest_tmp"
}

make_ext4() {
	root_dir=$1
	output=$2
	capacity=$3
	label=$4
	# The filesystem must leave room for the descriptor and footer appended
	# below. Ask the selected avbtool rather than guessing its overhead.
	max_bytes=$("$avbtool" add_hash_footer --partition_size "$capacity" --calc_max_image_size) ||
		die "avbtool could not calculate the $label payload capacity"
	case "$max_bytes" in ''|*[!0-9]*) die "avbtool returned an invalid $label payload capacity" ;; esac
	[ "$max_bytes" -gt 0 ] && [ "$max_bytes" -lt "$capacity" ] || die "invalid AVB reservation for $label"
	blocks=$((max_bytes / 4096))
	[ "$blocks" -gt 0 ] || die "no filesystem space remains for $label"
	"$mke2fs" -q -F -t ext4 -b 4096 -L "$label" -U clear -d "$root_dir" "$output" "$blocks"
	[ -f "$output" ] || die "mke2fs did not produce $output"
	[ "$(file_size "$output")" -le "$max_bytes" ] || die "$label image leaves insufficient space for AVB metadata"
}

check_capacity() {
	partition=$1
	file=$2
	[ "$(file_size "$file")" -le "$(partition_capacity "$partition")" ] || die "$partition artifact exceeds its explicit partition capacity"
}

check_boot_magic() {
	partition=$1
	case "$partition" in boot|init_boot) expected_magic='ANDROID!' ;; *) expected_magic=VNDRBOOT ;; esac
	magic=$(dd if="$stage/$partition.img" bs=8 count=1 2>/dev/null) || die "cannot read $partition image header"
	[ "$magic" = "$expected_magic" ] || die "$partition is not a $expected_magic boot container"
}

add_avb_hash_footer() {
	partition=$1
	file=$2
	# Derive a deterministic salt from the exact pre-footer bytes.  avbtool
	# computes the descriptor digest over those bytes, then appends the vbmeta
	# blob and footer so make_vbmeta_image can read the descriptor back.
	salt=$(sha256_of "$file")
	if [ "$avb_algorithm" = NONE ]; then
		"$avbtool" add_hash_footer \
			--image "$file" \
			--partition_name "$partition" \
			--partition_size "$(partition_capacity "$partition")" \
			--hash_algorithm sha256 \
			--salt "$salt" \
			--algorithm NONE
	else
		"$avbtool" add_hash_footer \
			--image "$file" \
			--partition_name "$partition" \
			--partition_size "$(partition_capacity "$partition")" \
			--hash_algorithm sha256 \
			--salt "$salt" \
			--algorithm "$avb_algorithm" \
			--key "$avb_key"
	fi
	[ -s "$file" ] || die "avbtool produced an empty AVB-bound image: $file"
	check_capacity "$partition" "$file"
}

make_avb_metadata() {
	output=$1
	shift
	if [ "$avb_algorithm" = NONE ]; then
		"$avbtool" make_vbmeta_image --algorithm NONE --output "$output" "$@"
	else
		"$avbtool" make_vbmeta_image --algorithm "$avb_algorithm" --key "$avb_key" --output "$output" "$@"
	fi
	[ -s "$output" ] || die "avbtool produced an empty AVB metadata image: $output"
	info=$($avbtool info_image --image "$output") || die "avbtool could not inspect generated metadata: $output"
	printf '%s\n' "$info" | grep -Fq 'Descriptors:' || die "generated AVB metadata has no descriptor section: $output"
}

record_avb_binding() {
	partition=$1
	file=$2
	printf 'avb_bound_image\t%s\t%s\t%s\t%s\n' \
		"$partition" "${file#"$stage/"}" "$(file_size "$file")" "$(sha256_of "$file")" >>"$manifest_tmp"
}

build() {
	preflight >/dev/null
	check_host
	[ "${FERRIX_PIXEL9A_RADIO_SPLIT_BUILD:-}" = "$split_build_confirmation" ] || die "split composition is opt-in; set FERRIX_PIXEL9A_RADIO_SPLIT_BUILD=$split_build_confirmation"
	[ ! -e "$image_out" ] || die "refusing to overwrite existing split output; choose a fresh FERRIX_PIXEL9A_RADIO_SPLIT_OUT: $image_out"
	[ ! -e "$manifest" ] || die "refusing to overwrite existing split manifest; choose a fresh FERRIX_PIXEL9A_RADIO_SPLIT_MANIFEST: $manifest"
	mkbootimg=$(resolve_executable "$mkbootimg_request" mkbootimg)
	mkdtboimg=$(resolve_executable "$mkdtboimg_request" mkdtboimg.py)
	mke2fs=$(resolve_executable "$mke2fs_request" mke2fs)
	avbtool=$(resolve_executable "$avbtool_request" avbtool)
	image_parent=$(dirname -- "$image_out")
	mkdir -p "$image_parent"
	stage=$(mktemp -d "$image_parent/.tegu-radio-split.XXXXXX")
	manifest_tmp="$stage/boot-manifest.tsv.tmp"
	trap 'rm -rf "$stage"' EXIT HUP INT TERM
	mkdir -p "$stage/provenance"
	kernel_image=$(verify_artifact "$kernel_manifest" "$kernel_dist" Image)
	dtb=$(verify_artifact "$kernel_manifest" "$kernel_dist" dtb.img)
	rootfs_tar=$(verify_artifact "$rootfs_manifest" "$rootfs_out" rootfs.tar)
	rootfs_image=$(verify_artifact "$rootfs_manifest" "$rootfs_out" rootfs.img)
	cp "$kernel_manifest" "$stage/provenance/kernel-manifest.tsv"
	cp "$rootfs_manifest" "$stage/provenance/rootfs-manifest.tsv"
	cp "$partition_sizes" "$stage/provenance/partition-capacities.tsv"
	cp "$boot_inputs_manifest" "$stage/provenance/boot-inputs-manifest.tsv"
	cp "$vendor_kernel_boot_cpio" "$stage/provenance/vendor-kernel-boot.cpio"
	cp "$kernel_image" "$stage/Image"
	cp "$dtb" "$stage/dtb.img"
	cp "$rootfs_tar" "$stage/rootfs.tar"
	cp "$rootfs_image" "$stage/rootfs.img"
	"$mkbootimg" --header_version 4 --pagesize 4096 --kernel "$stage/Image" --output "$stage/boot.img"
	"$mkbootimg" --header_version 4 --pagesize 4096 --ramdisk "$initramfs" --output "$stage/init_boot.img"
	"$mkbootimg" --header_version 4 --pagesize 2048 --vendor_ramdisk "$vendor_ramdisk" --vendor_cmdline "$vendor_cmdline" --vendor_bootconfig "$bootconfig" --vendor_boot "$stage/vendor_boot.img"
	dtbo_inputs="$stage/.dtbo-inputs"
	awk -F "$tab" '$1 == "source" && $2 == "exynos/google/zumapro-tegu-mp.dtbo" { print $2 }' "$kernel_dist/split-inputs/dtbo-source-set.tsv" | while IFS= read -r relative; do
		printf '%s\n' "$kernel_dist/split-inputs/dtbo-source-set/$relative"
	done >"$dtbo_inputs"
	# Compose only the exact source-built MP1.0 overlay.
	set --
	while IFS= read -r dtbo; do set -- "$@" "$dtbo"; done <"$dtbo_inputs"
	"$mkdtboimg" create "$stage/dtbo.img" --page_size=4096 --id=/:board_id --rev=/:board_rev "$@"
	"$root/tools/pixel-9a/static-boot.py" verify-dtbo "$stage/dtbo.img" || die 'generated DTBO table failed the exact tegu MP1.0 layout check'
	# vendor_kernel_boot uses the vendor boot container too; a bare CPIO is
	# only its ramdisk payload and is not a bootloader-readable image.
	"$mkbootimg" --header_version 4 --pagesize 2048 --vendor_ramdisk "$vendor_kernel_boot_cpio" --dtb "$stage/dtb.img" --vendor_boot "$stage/vendor_kernel_boot.img"
	for partition in boot init_boot vendor_boot vendor_kernel_boot; do
		check_boot_magic "$partition"
	done
	make_ext4 "$vendor_dlkm_root" "$stage/vendor_dlkm.img" "$(partition_capacity vendor_dlkm)" vendor_dlkm
	make_ext4 "$system_dlkm_root" "$stage/system_dlkm.img" "$(partition_capacity system_dlkm)" system_dlkm
	for partition in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
		case "$partition" in
			vbmeta|vbmeta_vendor|vbmeta_system) ;;
			*) check_capacity "$partition" "$stage/$partition.img" ;;
		esac
	done
	# Bind every final source-built partition to a fresh AVB hash descriptor,
	# then assemble the three vbmeta images from those descriptors.  No stock or
	# prebuilt Android vbmeta image participates in this route.
	add_avb_hash_footer boot "$stage/boot.img"
	add_avb_hash_footer init_boot "$stage/init_boot.img"
	add_avb_hash_footer vendor_boot "$stage/vendor_boot.img"
	add_avb_hash_footer vendor_kernel_boot "$stage/vendor_kernel_boot.img"
	add_avb_hash_footer dtbo "$stage/dtbo.img"
	add_avb_hash_footer vendor_dlkm "$stage/vendor_dlkm.img"
	add_avb_hash_footer system_dlkm "$stage/system_dlkm.img"
	make_avb_metadata "$stage/vbmeta.img" \
		--include_descriptors_from_image "$stage/boot.img" \
		--include_descriptors_from_image "$stage/init_boot.img"
	make_avb_metadata "$stage/vbmeta_vendor.img" \
		--include_descriptors_from_image "$stage/vendor_boot.img" \
		--include_descriptors_from_image "$stage/vendor_kernel_boot.img" \
		--include_descriptors_from_image "$stage/dtbo.img" \
		--include_descriptors_from_image "$stage/vendor_dlkm.img"
	make_avb_metadata "$stage/vbmeta_system.img" \
		--include_descriptors_from_image "$stage/system_dlkm.img"
	for partition in vbmeta vbmeta_vendor vbmeta_system; do
		check_capacity "$partition" "$stage/$partition.img"
		# avbtool resolves descriptor partition names beside this image and
		# verifies their payload digests. A matching outer TSV hash alone is
		# not evidence that the embedded AVB descriptors bind these bytes.
		if ! "$avbtool" verify_image --image "$stage/$partition.img" >"$stage/provenance/$partition-verification.txt" 2>&1; then
			cat "$stage/provenance/$partition-verification.txt" >&2
			die "generated $partition failed AVB payload verification"
		fi
	done
	{
		printf 'schema_version\t1\n'
		printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
		printf 'codename\ttegu\n'
		printf 'hardware_revision\tMP1.0\n'
		printf 'route_id\t%s\n' "$(route_value route_id)"
		printf 'kernel_backend\tlinux-google-tegu-wwan\n'
		printf 'status\tcomposed-not-booted\n'
		printf 'bootable\tfalse\n'
		printf 'header_version\t4\n'
		printf 'split_artifact_contract\t%s\t%s\n' "$(route_value split_artifact_contract)" "$(route_value split_artifact_contract_sha256)"
		printf 'partition_contract\tcomplete-source-built\n'
		printf 'avb_mode\tfresh-generated\n'
		printf 'avb_algorithm\t%s\n' "$avb_algorithm"
		printf 'avb_descriptor_inputs\tfresh-avbtool-hash-footers-from-final-images\n'
		printf 'avb_binding_salt\tsha256-of-pre-footer-image-bytes\n'
		printf 'partition_capacities\tprovenance/partition-capacities.tsv\t%s\t%s\n' "$(file_size "$stage/provenance/partition-capacities.tsv")" "$(sha256_of "$stage/provenance/partition-capacities.tsv")"
		printf 'kernel_manifest\tprovenance/kernel-manifest.tsv\t%s\t%s\n' "$(file_size "$stage/provenance/kernel-manifest.tsv")" "$(sha256_of "$stage/provenance/kernel-manifest.tsv")"
		printf 'rootfs_manifest\tprovenance/rootfs-manifest.tsv\t%s\t%s\n' "$(file_size "$stage/provenance/rootfs-manifest.tsv")" "$(sha256_of "$stage/provenance/rootfs-manifest.tsv")"
		printf 'boot_inputs_manifest\tprovenance/boot-inputs-manifest.tsv\t%s\t%s\n' "$(file_size "$stage/provenance/boot-inputs-manifest.tsv")" "$(sha256_of "$stage/provenance/boot-inputs-manifest.tsv")"
		printf 'vendor_kernel_boot_input\tprovenance/vendor-kernel-boot.cpio\t%s\t%s\n' "$(file_size "$stage/provenance/vendor-kernel-boot.cpio")" "$(sha256_of "$stage/provenance/vendor-kernel-boot.cpio")"
		for partition in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
			printf 'partition_capacity\t%s\t%s\n' "$partition" "$(partition_capacity "$partition")"
		done
		} >"$manifest_tmp"
	record_avb_binding boot "$stage/boot.img"
	record_avb_binding init_boot "$stage/init_boot.img"
	record_avb_binding vendor_boot "$stage/vendor_boot.img"
	record_avb_binding vendor_kernel_boot "$stage/vendor_kernel_boot.img"
	record_avb_binding dtbo "$stage/dtbo.img"
	record_avb_binding vendor_dlkm "$stage/vendor_dlkm.img"
	record_avb_binding system_dlkm "$stage/system_dlkm.img"
	record_artifact Image kernel "$stage/Image"
	record_artifact dtb.img kernel "$stage/dtb.img"
	record_artifact boot generated-mkbootimg "$stage/boot.img"
	record_artifact init_boot generated-mkbootimg "$stage/init_boot.img"
	record_artifact vendor_boot generated-mkbootimg "$stage/vendor_boot.img"
	record_artifact vendor_kernel_boot generated-mkbootimg "$stage/vendor_kernel_boot.img"
	record_artifact dtbo generated-mkdtboimg "$stage/dtbo.img"
	record_artifact vendor_dlkm source-built-modules "$stage/vendor_dlkm.img"
	record_artifact system_dlkm source-built-modules "$stage/system_dlkm.img"
	record_artifact vbmeta generated-avbtool-from-final-images "$stage/vbmeta.img"
	record_artifact vbmeta_vendor generated-avbtool-from-final-images "$stage/vbmeta_vendor.img"
	record_artifact vbmeta_system generated-avbtool-from-final-images "$stage/vbmeta_system.img"
	record_artifact rootfs_handoff rootfs "$stage/rootfs.tar"
	record_artifact rootfs_image rootfs "$stage/rootfs.img"
	mv "$manifest_tmp" "$stage/boot-manifest.tsv"
	mv "$stage" "$image_out"
	if [ "$manifest" != "$image_out/boot-manifest.tsv" ]; then
		mkdir -p "$(dirname -- "$manifest")"
		cp "$image_out/boot-manifest.tsv" "$manifest"
	fi
	trap - EXIT HUP INT TERM
	printf '%s\n' "PASS pixel-9a-radio-split composed source-built split artifacts: $image_out"
	printf '%s\n' 'NOTE: artifacts are hash-recorded, unbooted, and bootable=false; no device operation was performed'
}

usage() {
	printf '%s\n' 'usage: tools/pixel-9a/radio-split.sh preflight|build' >&2
	exit 2
}

case "${1:-}" in
	preflight) [ "$#" -eq 1 ] || usage; preflight ;;
	build) [ "$#" -eq 1 ] || usage; build ;;
	*) usage ;;
esac
