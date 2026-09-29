#!/bin/sh
set -eu

# Device-free composition for the separately pinned radio-first Kbuild route.
# It accepts only the linux-google-tegu-wwan kernel contract and a rootfs
# carrying the matching WWAN/SIT userspace.  tools/pixel-9a/boot.sh remains a
# different route and rejects this backend instead of mixing cpif and WWAN
# artifacts.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
route_pins=${FERRIX_PIXEL9A_RADIO_ROUTE_PINS:-"$target/kernel-radio-route.toml"}
out_root=${FERRIX_OUT_DIR:-"$root/out"}
kernel_out=${FERRIX_PIXEL9A_RADIO_KERNEL_OUT:-"$out_root/pixel-9a/kernel-radio/tegu-wwan"}
kernel_dist=${FERRIX_PIXEL9A_RADIO_KERNEL_DIST:-"$kernel_out/dist"}
kernel_manifest=${FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST:-"$kernel_out/build-manifest.tsv"}
rootfs_out=${FERRIX_PIXEL9A_ROOTFS_OUT:-"$out_root/pixel-9a/rootfs/tegu"}
rootfs_manifest=${FERRIX_PIXEL9A_ROOTFS_MANIFEST:-"$rootfs_out/manifest.tsv"}
image_out=${FERRIX_PIXEL9A_RADIO_BOOT_OUT:-"$out_root/pixel-9a/images/tegu-wwan"}
radio_boot_manifest=${FERRIX_PIXEL9A_RADIO_BOOT_MANIFEST:-"$image_out/boot-manifest.tsv"}
boot_inputs_out=${FERRIX_PIXEL9A_BOOT_INPUTS_OUT:-"$out_root/pixel-9a/boot-inputs/tegu-wwan"}
build_confirmation=I-understand-this-composes-pixel-9a-radio-first-boot-artifacts
tab=$(printf '\t')

die() {
	printf 'FAIL pixel-9a-radio-boot: %s\n' "$*" >&2
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
	' "$route_pins"
}

check_split_contract() {
	contract_rel=$(route_value split_artifact_contract) || die 'radio route has no split-artifact contract path'
	case "$contract_rel" in
		''|/*|*..*|*' '*) die "radio split-artifact contract path is unsafe: $contract_rel" ;;
	esac
	contract="$target/$contract_rel"
	[ -f "$contract" ] || die "radio split-artifact contract is missing: $contract"
	[ ! -L "$contract" ] || die "radio split-artifact contract must not be a symlink: $contract"
	expected=$(route_value split_artifact_contract_sha256) || die 'radio route has no split-artifact contract hash'
	case "$expected" in ''|*[!0-9a-f]*) die 'radio split-artifact contract hash is malformed' ;; esac
	[ "${#expected}" -eq 64 ] || die 'radio split-artifact contract hash must contain 64 hexadecimal characters'
	[ "$(sha256_of "$contract")" = "$expected" ] || die 'radio split-artifact contract hash differs from route metadata'
	for required in \
		'source_only = true' \
		'stock_modules_may_be_reused = false' \
		'android_userspace_required = false' \
		'boot_header_version = 4' \
		'boot_page_size = 4096' \
		'dtbo_tool = "mkdtboimg.py"' \
		'dtbo_page_size = 4096' \
		'boot_input_generator = "tools/pixel-9a/boot-inputs.sh"' \
		'boot_input_manifest_schema = 1' \
		'dynamic_partition_transport = "fastbootd"'; do
		grep -Fqx "$required" "$contract" || die "radio split-artifact contract is missing: $required"
	done
	for section in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
		grep -Fqx "[artifact.$section]" "$contract" || die "radio split-artifact contract omits artifact section: $section"
	done
}

manifest_value() {
	manifest=$1
	key=$2
	awk -F "$tab" -v key="$key" '$1 == key { print $2; exit }' "$manifest"
}

manifest_artifact() {
	manifest=$1
	name=$2
	awk -F "$tab" -v name="$name" '$1 == "artifact" && $2 == name { print; found=1; exit } END { if (!found) exit 1 }' "$manifest"
}

verify_manifest_input() {
	manifest=$1
	base=$2
	key=$3
	label=$4
	row=$(awk -F "$tab" -v key="$key" '$1 == key { print; found=1; exit } END { if (!found) exit 1 }' "$manifest") ||
		die "$label is missing the $key split-input row"
	relative=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $2 }')
	expected_size=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $3 }')
	expected_hash=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $4 }')
	case "$relative" in
		''|/*|*..*|*' '*) die "$label has an unsafe $key path: $relative" ;;
	esac
	case "$expected_size" in ''|*[!0-9]*) die "$label has an invalid $key byte count" ;; esac
	case "$expected_hash" in ''|*[!0-9a-f]*) die "$label has an invalid $key hash" ;; esac
	[ "${#expected_hash}" -eq 64 ] || die "$label has an invalid $key hash length"
	file="$base/$relative"
	[ -f "$file" ] || die "$label split input is missing: $file"
	[ ! -L "$file" ] || die "$label split input must not be a symlink: $file"
	[ "$(file_size "$file")" = "$expected_size" ] || die "$label $key byte count differs from its manifest"
	[ "$(sha256_of "$file")" = "$expected_hash" ] || die "$label $key hash differs from its manifest"
}

verify_dtbo_source_set() {
	manifest=$1
	base=$2
	label=$3
	row=$(awk -F "$tab" '$1 == "dtbo_source_set" { print; found=1; exit } END { if (!found) exit 1 }' "$manifest") ||
		die "$label is missing the dtbo_source_set split-input row"
	relative=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $2 }')
	[ "$relative" = split-inputs/dtbo-source-set.tsv ] || die "$label dtbo_source_set path is not canonical"
	file="$base/$relative"
	[ -f "$file" ] || die "$label dtbo_source_set split input is missing: $file"
	[ ! -L "$file" ] || die "$label dtbo_source_set split input must not be a symlink: $file"

	schema_count=0
	source_root_count=0
	source_count=0
	dtbo_count=0
	seen=''
	source_dir="$base/split-inputs/dtbo-source-set"
	while IFS="$tab" read -r kind value third fourth fifth; do
		case "$kind" in
			''|'#'*) continue ;;
			schema_version)
				[ "$value" = 1 ] && [ -z "${third:-}" ] && [ -z "${fourth:-}" ] && [ -z "${fifth:-}" ] ||
					die "$label dtbo_source_set has a malformed schema_version row"
				schema_count=$((schema_count + 1))
				;;
			source_root)
				[ "$value" = arch/arm64/boot/dts ] && [ -z "${third:-}" ] && [ -z "${fourth:-}" ] && [ -z "${fifth:-}" ] ||
					die "$label dtbo_source_set has an unexpected source_root"
				source_root_count=$((source_root_count + 1))
				;;
			source)
				case "$value" in
					''|/*|*..*|*' '*) die "$label dtbo_source_set has an unsafe source path: $value" ;;
					*.dtb|*.dtbo) ;;
					*) die "$label dtbo_source_set contains a non-DTB source: $value" ;;
				 esac
				case " $seen " in
					*" $value "*) die "$label dtbo_source_set repeats source: $value" ;;
				 esac
				case "$third" in ''|*[!0-9]*) die "$label dtbo_source_set has an invalid byte count: $value" ;; esac
				case "$fourth" in ''|*[!0-9a-f]*) die "$label dtbo_source_set has an invalid hash: $value" ;; esac
				[ "${#fourth}" -eq 64 ] || die "$label dtbo_source_set has an invalid hash length: $value"
				[ -z "${fifth:-}" ] || die "$label dtbo_source_set has extra fields for source: $value"
				source="$source_dir/$value"
				[ -f "$source" ] || die "$label dtbo_source_set source is missing: $source"
				[ ! -L "$source" ] || die "$label dtbo_source_set source must not be a symlink: $source"
				[ "$(file_size "$source")" = "$third" ] || die "$label dtbo_source_set source size differs from its manifest: $value"
				[ "$(sha256_of "$source")" = "$fourth" ] || die "$label dtbo_source_set source hash differs from its manifest: $value"
				seen="$seen $value"
				source_count=$((source_count + 1))
				case "$value" in *.dtbo) dtbo_count=$((dtbo_count + 1)) ;; esac
				;;
			*) die "$label dtbo_source_set has an unknown row: $kind" ;;
		esac
	done <"$file"
	[ "$schema_count" -eq 1 ] || die "$label dtbo_source_set must contain exactly one schema_version=1 row"
	[ "$source_root_count" -eq 1 ] || die "$label dtbo_source_set must contain exactly one source_root row"
	[ "$source_count" -gt 0 ] || die "$label dtbo_source_set contains no source-built DTB inputs"
	[ "$dtbo_count" -gt 0 ] || die "$label dtbo_source_set contains no source-built DTBO overlay inputs"
}

verify_module_install() {
	manifest=$1
	base=$2
	label=$3
	row=$(awk -F "$tab" '$1 == "module_install" { print; found=1; exit } END { if (!found) exit 1 }' "$manifest") ||
		die "$label is missing the module_install split-input row"
	relative=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $2 }')
	[ "$relative" = split-inputs/module-install.tsv ] || die "$label module_install path is not canonical"
	file="$base/$relative"
	[ -f "$file" ] || die "$label module_install split input is missing: $file"
	[ ! -L "$file" ] || die "$label module_install split input must not be a symlink: $file"

	schema_count=0
	install_root_count=0
	seen=''
	module_dir="$base/split-inputs/module-install"
	while IFS="$tab" read -r kind value third fourth fifth; do
		case "$kind" in
			''|'#'*) continue ;;
			schema_version)
				[ "$value" = 1 ] && [ -z "${third:-}" ] && [ -z "${fourth:-}" ] && [ -z "${fifth:-}" ] ||
					die "$label module_install has a malformed schema_version row"
				schema_count=$((schema_count + 1))
				;;
			install_root)
				[ "$value" = split-inputs/module-install ] && [ -z "${third:-}" ] && [ -z "${fourth:-}" ] && [ -z "${fifth:-}" ] ||
					die "$label module_install has an unexpected install_root"
				install_root_count=$((install_root_count + 1))
				;;
			file)
				case "$value" in
					''|/*|*..*|*' '*) die "$label module_install has an unsafe file path: $value" ;;
				esac
				case " $seen " in
					*" $value "*) die "$label module_install repeats file: $value" ;;
				 esac
				case "$third" in ''|*[!0-9]*) die "$label module_install has an invalid byte count: $value" ;; esac
				case "$fourth" in ''|*[!0-9a-f]*) die "$label module_install has an invalid hash: $value" ;; esac
				[ "${#fourth}" -eq 64 ] || die "$label module_install has an invalid hash length: $value"
				[ -z "${fifth:-}" ] || die "$label module_install has extra fields for file: $value"
				module="$module_dir/$value"
				[ -f "$module" ] || die "$label module_install file is missing: $module"
				[ ! -L "$module" ] || die "$label module_install file must not be a symlink: $module"
				[ "$(file_size "$module")" = "$third" ] || die "$label module_install file size differs from its manifest: $value"
				[ "$(sha256_of "$module")" = "$fourth" ] || die "$label module_install file hash differs from its manifest: $value"
				seen="$seen $value"
				;;
			*) die "$label module_install has an unknown row: $kind" ;;
		esac
	done <"$file"
	[ "$schema_count" -eq 1 ] || die "$label module_install must contain exactly one schema_version=1 row"
	[ "$install_root_count" -eq 1 ] || die "$label module_install must contain exactly one install_root row"
}

verify_partition_size_input() {
	manifest=$1
	base=$2
	label=$3
	row=$(awk -F "$tab" '$1 == "partition_sizes" { print; found=1; exit } END { if (!found) exit 1 }' "$manifest") ||
		die "$label is missing the partition_sizes split-input row"
	relative=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $2 }')
	case "$relative" in
		''|/*|*..*|*' '*) die "$label has an unsafe partition_sizes path: $relative" ;;
	esac
	file="$base/$relative"
	[ -f "$file" ] || die "$label partition_sizes split input is missing: $file"
	[ ! -L "$file" ] || die "$label partition_sizes split input must not be a symlink: $file"

	schema_count=$(awk -F "$tab" '$1 == "schema_version" && $2 == "1" { count++ } END { print count + 0 }' "$file")
	[ "$schema_count" -eq 1 ] || die "$label partition_sizes must contain exactly one schema_version=1 row"
	seen=''
	count=0
	while IFS="$tab" read -r kind partition status extra; do
		case "$kind" in
			''|'#'*) continue ;;
			schema_version)
				[ "$partition" = 1 ] && [ -z "${status:-}" ] && [ -z "${extra:-}" ] ||
					die "$label partition_sizes has a malformed schema_version row"
				;;
			partition)
				[ -n "$partition" ] && [ "$status" = status=not-built ] && [ -z "${extra:-}" ] ||
					die "$label partition_sizes has a malformed partition row"
				case "$partition" in
					boot|init_boot|vendor_boot|vendor_kernel_boot|dtbo|vendor_dlkm|system_dlkm|vbmeta|vbmeta_vendor|vbmeta_system) ;;
					*) die "$label partition_sizes names an unexpected partition: $partition" ;;
				esac
				case " $seen " in
					*" $partition "*) die "$label partition_sizes repeats partition: $partition" ;;
					esac
				seen="$seen $partition"
				count=$((count + 1))
				;;
			*) die "$label partition_sizes has an unknown row: $kind" ;;
		esac
	done <"$file"
	[ "$count" -eq 10 ] || die "$label partition_sizes must contain all 10 split partitions"
	for partition in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
		case " $seen " in
			*" $partition "*) ;;
			*) die "$label partition_sizes is missing partition: $partition" ;;
		esac
	done
}

verify_artifact() {
	manifest=$1
	base=$2
	name=$3
	row=$(manifest_artifact "$manifest" "$name") || die "$name is missing from $manifest"
	relative=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $2 }')
	expected_size=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $3 }')
	expected_hash=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $4 }')
	case "$relative" in ''|/*|*..*|*' '*) die "unsafe artifact path in $manifest: $relative" ;; esac
	file="$base/$relative"
	[ -f "$file" ] || die "manifest artifact is missing: $file"
	[ ! -L "$file" ] || die "manifest artifact must not be a symlink: $file"
	[ "$(file_size "$file")" = "$expected_size" ] || die "artifact size mismatch: $name"
	[ "$(sha256_of "$file")" = "$expected_hash" ] || die "artifact hash mismatch: $name"
	printf '%s\n' "$file"
}

check_metadata() {
	[ -f "$route_pins" ] || die "radio route metadata is missing: $route_pins"
	[ "$(route_value target_id)" = google-pixel-9a-tegu-gxq96-mp1-0 ] || die 'radio route target is not the exact Pixel 9a'
	[ "$(route_value route_id)" = experimental-linux-google-tegu-s5300-wwan ] || die 'radio route id is not the reviewed WWAN route'
	[ "$(route_value status)" = experimental-not-adopted ] || die 'radio route metadata status must remain experimental-not-adopted'
	check_split_contract
}

check_manifest_split_contract() {
	manifest=$1
	label=$2
	row=$(awk -F "$tab" '$1 == "split_artifact_contract" { print; found=1; exit } END { if (!found) exit 1 }' "$manifest") ||
		die "$label lacks the split-artifact contract row"
	[ "$(printf '%s\n' "$row" | awk -F "$tab" '{ print $2 }')" = "$(route_value split_artifact_contract)" ] ||
		die "$label split-artifact contract path is not canonical"
	[ "$(printf '%s\n' "$row" | awk -F "$tab" '{ print $3 }')" = "$(route_value split_artifact_contract_sha256)" ] ||
		die "$label split-artifact contract hash differs from the checked-in contract"
}

check_kernel_contract() {
	check_metadata
	[ -d "$kernel_dist" ] || die "radio-first kernel dist is missing: $kernel_dist; run just pixel-9a-kernel-radio-build"
	[ -f "$kernel_manifest" ] || die "radio-first kernel manifest is missing: $kernel_manifest"
	[ "$(manifest_value "$kernel_manifest" target_id)" = google-pixel-9a-tegu-gxq96-mp1-0 ] || die 'radio kernel manifest target is not the exact Pixel 9a'
	[ "$(manifest_value "$kernel_manifest" route_id)" = "$(route_value route_id)" ] || die 'radio kernel manifest route id does not match kernel-radio-route.toml'
	[ "$(manifest_value "$kernel_manifest" kernel_backend)" = linux-google-tegu-wwan ] || die 'radio kernel manifest does not select linux-google-tegu-wwan'
	[ "$(manifest_value "$kernel_manifest" build_system)" = kbuild-make ] || die 'radio boot refuses a non-Kbuild kernel manifest'
	check_manifest_split_contract "$kernel_manifest" 'radio kernel manifest'
	for input in dtbo_source_set module_install partition_sizes; do
		verify_manifest_input "$kernel_manifest" "$kernel_dist" "$input" 'radio kernel manifest'
	done
	verify_dtbo_source_set "$kernel_manifest" "$kernel_dist" 'radio kernel manifest'
	verify_module_install "$kernel_manifest" "$kernel_dist" 'radio kernel manifest'
	verify_partition_size_input "$kernel_manifest" "$kernel_dist" 'radio kernel manifest'
	mapping_row=$(awk -F "$tab" '$1 == "firmware_mapping" { print; found=1; exit } END { if (!found) exit 1 }' "$kernel_manifest") || die 'radio kernel manifest lacks the firmware mapping row'
	[ "$(printf '%s\n' "$mapping_row" | awk -F "$tab" '{ print $2 }')" = firmware-radio-route.tsv ] || die 'radio kernel manifest firmware mapping path is not canonical'
	[ "$(printf '%s\n' "$mapping_row" | awk -F "$tab" '{ print $3 }')" = "$(sha256_of "$target/firmware-radio-route.tsv")" ] || die 'radio kernel manifest firmware mapping hash differs from the checked-in route'
	initfs_row=$(awk -F "$tab" '$1 == "initfs_module_list" { print; found=1; exit } END { if (!found) exit 1 }' "$kernel_manifest") || die 'radio kernel manifest lacks the initfs module list row'
	[ "$(printf '%s\n' "$initfs_row" | awk -F "$tab" '{ print $2 }')" = "$(route_value initfs_module_list)" ] || die 'radio kernel manifest initfs module list is not canonical'
	[ "$(printf '%s\n' "$initfs_row" | awk -F "$tab" '{ print $3 }')" = "$(sha256_of "$target/$(route_value initfs_module_list)")" ] || die 'radio kernel manifest initfs module list hash differs from the checked-in route'
	[ "$(manifest_value "$kernel_manifest" status)" = built-not-booted ] || die 'radio kernel manifest is not a completed build handoff'
	[ "$(manifest_value "$kernel_manifest" bootable)" = false ] || die 'radio kernel manifest must remain bootable=false before evidence'
	image=$(verify_artifact "$kernel_manifest" "$kernel_dist" Image)
	dtb=$(verify_artifact "$kernel_manifest" "$kernel_dist" dtb.img)
	[ -n "$image" ] && [ -n "$dtb" ]
}

check_rootfs_contract() {
	[ -d "$rootfs_out" ] || die "native Linux rootfs output is missing: $rootfs_out; run just pixel-9a-rootfs-build"
	[ -f "$rootfs_manifest" ] || die "rootfs manifest is missing: $rootfs_manifest"
	[ "$(manifest_value "$rootfs_manifest" target_id)" = google-pixel-9a-tegu-gxq96-mp1-0 ] || die 'rootfs manifest target is not the exact Pixel 9a'
	[ "$(manifest_value "$rootfs_manifest" codename)" = tegu ] || die 'rootfs manifest codename is not tegu'
	[ "$(manifest_value "$rootfs_manifest" hardware_revision)" = MP1.0 ] || die 'rootfs manifest hardware revision is not MP1.0'
	[ "$(manifest_value "$rootfs_manifest" substrate)" = postmarketos ] || die 'rootfs manifest substrate is not postmarketOS'
	[ "$(manifest_value "$rootfs_manifest" kernel_backend)" = linux-google-tegu-wwan ] || die 'radio boot refuses a rootfs without the matching WWAN/SIT backend'
	[ "$(manifest_value "$rootfs_manifest" status)" = built-substrate-only ] || die 'rootfs manifest is not a completed built-substrate-only handoff'
	[ "$(manifest_value "$rootfs_manifest" bootable)" = false ] || die 'rootfs manifest must remain bootable=false before evidence'
	check_manifest_split_contract "$rootfs_manifest" 'rootfs manifest'
	verify_artifact "$rootfs_manifest" "$rootfs_out" rootfs.tar >/dev/null
	verify_artifact "$rootfs_manifest" "$rootfs_out" rootfs.img >/dev/null
}

check_ramdisk_inputs() {
	initramfs=${FERRIX_PIXEL9A_INITRAMFS:-"$boot_inputs_out/initramfs.cpio"}
	vendor_ramdisk=${FERRIX_PIXEL9A_VENDOR_RAMDISK:-"$boot_inputs_out/vendor-ramdisk.cpio"}
	bootconfig=${FERRIX_PIXEL9A_VENDOR_BOOTCONFIG:-"$boot_inputs_out/vendor-bootconfig"}
	for file in "$initramfs" "$vendor_ramdisk" "$bootconfig"; do
		[ -f "$file" ] || die "boot container input is missing: $file"
		[ ! -L "$file" ] || die "boot container input must not be a symlink: $file"
	done
}

resolve_tool() {
	requested=$1
	label=$2
	case "$requested" in
		*/*) [ -x "$requested" ] || die "$label is not executable: $requested"; printf '%s\n' "$requested" ;;
		*) command -v "$requested" >/dev/null 2>&1 || die "missing $label '$requested'"; command -v "$requested" ;;
	esac
}

check_tools() {
	for command_name in awk cp dirname grep mkdir mktemp mv tr wc; do
		command -v "$command_name" >/dev/null 2>&1 || die "missing prerequisite '$command_name'"
	done
	command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || die 'missing sha256sum or shasum'
	mkbootimg=$(resolve_tool "${FERRIX_PIXEL9A_MKBOOTIMG:-mkbootimg}" mkbootimg)
	avbtool=$(resolve_tool "${FERRIX_PIXEL9A_AVBTOOL:-avbtool}" avbtool)
}

preflight() {
	check_tools
	check_kernel_contract
	check_rootfs_contract
	check_ramdisk_inputs
	printf '%s\n' 'PASS pixel-9a-radio-boot preflight: WWAN kernel, matching SIT rootfs, and boot inputs are hash-verified'
}

make_vbmeta() {
	output=$1
	algorithm=${FERRIX_PIXEL9A_AVB_ALGORITHM:-NONE}
	case "$algorithm" in
		NONE) "$avbtool" make_vbmeta_image --algorithm NONE --output "$output" ;;
		*) key=${FERRIX_PIXEL9A_AVB_KEY:-}; [ -n "$key" ] || die "FERRIX_PIXEL9A_AVB_KEY is required for AVB algorithm $algorithm"; [ -f "$key" ] || die "AVB key is missing: $key"; [ ! -L "$key" ] || die 'AVB key must not be a symlink'; "$avbtool" make_vbmeta_image --algorithm "$algorithm" --key "$key" --output "$output" ;;
	esac
	[ -s "$output" ] || die "avbtool produced an empty image: $output"
}

record_artifact() {
	name=$1
	origin=$2
	file=$3
	[ -f "$file" ] || die "composed artifact is missing: $file"
	printf 'artifact\t%s\t%s\t%s\t%s\n' "$name" "${file#"$stage"/}" "$(file_size "$file")" "$(sha256_of "$file")" "$origin" >>"$manifest_tmp"
}

compose() {
	preflight >/dev/null
	[ "${FERRIX_PIXEL9A_RADIO_BOOT_BUILD:-}" = "$build_confirmation" ] || die "radio boot composition is opt-in; set FERRIX_PIXEL9A_RADIO_BOOT_BUILD=$build_confirmation"
	[ ! -e "$image_out" ] || die "refusing to overwrite existing radio boot output; choose a fresh FERRIX_PIXEL9A_RADIO_BOOT_OUT: $image_out"
	[ ! -e "$radio_boot_manifest" ] || die "refusing to overwrite existing radio boot manifest; choose a fresh FERRIX_PIXEL9A_RADIO_BOOT_MANIFEST: $radio_boot_manifest"
	image_parent=$(dirname -- "$image_out")
	mkdir -p "$image_parent"
	stage=$(mktemp -d "$image_parent/.tegu-radio-boot.XXXXXX")
	manifest_tmp="$stage/boot-manifest.tsv.tmp"
	trap 'rm -rf "$stage"' EXIT HUP INT TERM
	kernel_image=$(verify_artifact "$kernel_manifest" "$kernel_dist" Image)
	dtb=$(verify_artifact "$kernel_manifest" "$kernel_dist" dtb.img)
	cp "$kernel_image" "$stage/Image"
	cp "$dtb" "$stage/dtb.img"
	# The candidate is a normal arm64 Kbuild Image. It is wrapped independently
	# of the Google Kleaf output so a raw candidate Image can never be mistaken
	# for a Google boot-manifest artifact.
	"$mkbootimg" --header_version 4 --kernel "$stage/Image" --output "$stage/boot.img"
	"$mkbootimg" --header_version 4 --ramdisk "$initramfs" --output "$stage/init_boot.img"
	"$mkbootimg" --header_version 4 --pagesize 4096 --vendor_ramdisk "$vendor_ramdisk" --dtb "$stage/dtb.img" --vendor_bootconfig "$bootconfig" --vendor_boot "$stage/vendor_boot.img"
	rootfs_tar=$(verify_artifact "$rootfs_manifest" "$rootfs_out" rootfs.tar)
	rootfs_image=$(verify_artifact "$rootfs_manifest" "$rootfs_out" rootfs.img)
	cp "$rootfs_tar" "$stage/rootfs.tar"
	cp "$rootfs_image" "$stage/rootfs.img"
	make_vbmeta "$stage/vbmeta.img"
	{
		printf 'schema_version\t1\n'
		printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
		printf 'route_id\t%s\n' "$(route_value route_id)"
		printf 'kernel_backend\tlinux-google-tegu-wwan\n'
		printf 'status\tcomposed-not-booted\n'
		printf 'bootable\tfalse\n'
		printf 'split_artifact_contract\t%s\t%s\n' "$(route_value split_artifact_contract)" "$(route_value split_artifact_contract_sha256)"
		# The factory fastboot-info contract requires these split partitions.
		# This candidate deliberately does not synthesize the module/overlay
		# images yet; make that gap machine-readable so the handoff cannot be
		# mistaken for a flashable Pixel image.
		printf 'partition_contract\tincomplete\n'
		printf 'missing_partition\tdtbo\n'
		printf 'missing_partition\tvendor_kernel_boot\n'
		printf 'missing_partition\tvendor_dlkm\n'
		printf 'missing_partition\tsystem_dlkm\n'
		printf 'missing_partition\tvbmeta_vendor\n'
		printf 'missing_partition\tvbmeta_system\n'
		printf 'preserved_partition\tpvmfw\tstock-unmodified\n'
		printf 'preserved_partition\tsystem\tstock-unmodified\n'
		printf 'preserved_partition\tsystem_ext\tstock-unmodified\n'
		printf 'preserved_partition\tproduct\tstock-unmodified\n'
		printf 'preserved_partition\tvendor\tstock-unmodified\n'
		printf 'header_version\t4\n'
		printf 'avb_mode\t%s\n' "${FERRIX_PIXEL9A_AVB_ALGORITHM:-NONE}"
		printf 'kernel_manifest\t%s\t%s\t%s\n' "$kernel_manifest" "$(file_size "$kernel_manifest")" "$(sha256_of "$kernel_manifest")"
		printf 'rootfs_manifest\t%s\t%s\t%s\n' "$rootfs_manifest" "$(file_size "$rootfs_manifest")" "$(sha256_of "$rootfs_manifest")"
	} >"$manifest_tmp"
	record_artifact Image kernel "$stage/Image"
	record_artifact dtb kernel "$stage/dtb.img"
	record_artifact boot generated-mkbootimg "$stage/boot.img"
	record_artifact init_boot generated-mkbootimg "$stage/init_boot.img"
	record_artifact vendor_boot generated-mkbootimg "$stage/vendor_boot.img"
	record_artifact rootfs_handoff rootfs "$stage/rootfs.tar"
	record_artifact rootfs_image rootfs "$stage/rootfs.img"
	record_artifact vbmeta generated-avbtool "$stage/vbmeta.img"
	mv "$manifest_tmp" "$stage/boot-manifest.tsv"
	mv "$stage" "$image_out"
	if [ "$radio_boot_manifest" != "$image_out/boot-manifest.tsv" ]; then
		mkdir -p "$(dirname -- "$radio_boot_manifest")"
		cp "$image_out/boot-manifest.tsv" "$radio_boot_manifest"
	fi
	trap - EXIT HUP INT TERM
	printf '%s\n' "PASS pixel-9a-radio-boot composed artifacts: $image_out"
	printf '%s\n' 'NOTE: candidate artifacts are hash-recorded, unbooted, and bootable=false until exact-target evidence exists'
}

usage() {
	printf '%s\n' 'usage: tools/pixel-9a/boot-radio.sh preflight|build' >&2
	exit 2
}

case "${1:-}" in
	preflight) [ "$#" -eq 1 ] || usage; preflight ;;
	build) [ "$#" -eq 1 ] || usage; compose ;;
	*) usage ;;
esac
