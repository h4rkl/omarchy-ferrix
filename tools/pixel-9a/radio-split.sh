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
vendor_kernel_boot_cpio=${FERRIX_PIXEL9A_VENDOR_KERNEL_BOOT_CPIO:-}
vendor_dlkm_root=${FERRIX_PIXEL9A_VENDOR_DLKM_ROOT:-}
system_dlkm_root=${FERRIX_PIXEL9A_SYSTEM_DLKM_ROOT:-}
initramfs=${FERRIX_PIXEL9A_INITRAMFS:-}
vendor_ramdisk=${FERRIX_PIXEL9A_VENDOR_RAMDISK:-}
bootconfig=${FERRIX_PIXEL9A_VENDOR_BOOTCONFIG:-}
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
		''|/*|.|..|../*|*/../*|*/..|*' '*) die "unsafe relative path: $1" ;;
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
		'dtbo_tool = "mkdtboimg.py"' \
		'dtbo_page_size = 4096' \
		'dynamic_partition_transport = "fastbootd"'; do
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
	dtbo_count=0
	while IFS="$tab" read -r kind relative bytes hash extra; do
		case "$kind" in
			''|'#'*) continue ;;
			schema_version) [ "$relative" = 1 ] && [ -z "${bytes:-}" ] && [ -z "${hash:-}" ] && [ -z "${extra:-}" ] || die 'DTB/DTBO source manifest schema row is malformed' ;;
			source)
				safe_relative "$relative"
				case "$relative" in *.dtbo) dtbo_count=$((dtbo_count + 1)) ;; *.dtb) ;; *) die "DTB/DTBO source manifest contains a non-DTB path: $relative" ;; esac
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
	[ "$dtbo_count" -gt 0 ] || die 'source-built DTBO overlay set is empty'
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
	for input in "$initramfs" "$vendor_ramdisk" "$bootconfig" "$vendor_kernel_boot_cpio"; do
		require_file "$input" 'split boot input'
	done
	verify_module_root "$vendor_dlkm_root" vendor_dlkm_root
	verify_module_root "$system_dlkm_root" system_dlkm_root
}

check_tools() {
	for command_name in awk cp dirname find grep mkdir mktemp mv rm tr wc; do
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
	blocks=$((capacity / 4096))
	"$mke2fs" -q -F -t ext4 -L "$label" -U clear -d "$root_dir" "$output" "$blocks"
	[ -f "$output" ] || die "mke2fs did not produce $output"
	[ "$(file_size "$output")" -le "$capacity" ] || die "$label image exceeds its explicit partition capacity"
}

check_capacity() {
	partition=$1
	file=$2
	[ "$(file_size "$file")" -le "$(partition_capacity "$partition")" ] || die "$partition artifact exceeds its explicit partition capacity"
}

add_avb_hash_footer() {
	partition=$1
	file=$2
	# Derive a deterministic salt from the exact pre-footer bytes.  avbtool
	# computes the descriptor digest over those bytes and then appends only the
	# AVB footer, so the descriptor cannot silently describe an older image.
	salt=$(sha256_of "$file")
	if [ "$avb_algorithm" = NONE ]; then
		"$avbtool" add_hash_footer \
			--image "$file" \
			--partition_name "$partition" \
			--partition_size "$(partition_capacity "$partition")" \
			--hash_algorithm sha256 \
			--salt "$salt" \
			--algorithm NONE \
			--do_not_append_vbmeta_image
	else
		"$avbtool" add_hash_footer \
			--image "$file" \
			--partition_name "$partition" \
			--partition_size "$(partition_capacity "$partition")" \
			--hash_algorithm sha256 \
			--salt "$salt" \
			--algorithm "$avb_algorithm" \
			--key "$avb_key" \
			--do_not_append_vbmeta_image
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
	kernel_image=$(verify_artifact "$kernel_manifest" "$kernel_dist" Image)
	dtb=$(verify_artifact "$kernel_manifest" "$kernel_dist" dtb.img)
	rootfs_tar=$(verify_artifact "$rootfs_manifest" "$rootfs_out" rootfs.tar)
	rootfs_image=$(verify_artifact "$rootfs_manifest" "$rootfs_out" rootfs.img)
	cp "$kernel_image" "$stage/Image"
	cp "$dtb" "$stage/dtb.img"
	cp "$rootfs_tar" "$stage/rootfs.tar"
	cp "$rootfs_image" "$stage/rootfs.img"
	"$mkbootimg" --header_version 4 --pagesize 4096 --kernel "$stage/Image" --output "$stage/boot.img"
	"$mkbootimg" --header_version 4 --pagesize 4096 --ramdisk "$initramfs" --output "$stage/init_boot.img"
	"$mkbootimg" --header_version 4 --pagesize 4096 --vendor_ramdisk "$vendor_ramdisk" --dtb "$stage/dtb.img" --vendor_bootconfig "$bootconfig" --output "$stage/vendor_boot.img"
	dtbo_inputs="$stage/.dtbo-inputs"
	awk -F "$tab" '$1 == "source" && $2 ~ /\\.dtbo$/ { print $2 }' "$kernel_dist/split-inputs/dtbo-source-set.tsv" | while IFS= read -r relative; do
		printf '%s\n' "$kernel_dist/split-inputs/dtbo-source-set/$relative"
	done >"$dtbo_inputs"
	# mkdtboimg.py accepts the output followed by source-built DTBO blobs. The
	# source manifest is sorted by Kbuild output order, so the table is stable.
	set --
	while IFS= read -r dtbo; do set -- "$@" "$dtbo"; done <"$dtbo_inputs"
	"$mkdtboimg" create "$stage/dtbo.img" --page_size=4096 "$@"
	cp "$vendor_kernel_boot_cpio" "$stage/vendor_kernel_boot.img"
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
		printf 'partition_capacities\t%s\t%s\t%s\n' "$partition_sizes" "$(file_size "$partition_sizes")" "$(sha256_of "$partition_sizes")"
		printf 'kernel_manifest\t%s\t%s\t%s\n' "$kernel_manifest" "$(file_size "$kernel_manifest")" "$(sha256_of "$kernel_manifest")"
		printf 'rootfs_manifest\t%s\t%s\t%s\n' "$rootfs_manifest" "$(file_size "$rootfs_manifest")" "$(sha256_of "$rootfs_manifest")"
		printf 'vendor_kernel_boot_input\t%s\t%s\t%s\n' "$vendor_kernel_boot_cpio" "$(file_size "$vendor_kernel_boot_cpio")" "$(sha256_of "$vendor_kernel_boot_cpio")"
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
	record_artifact vendor_kernel_boot source-built-cpio "$stage/vendor_kernel_boot.img"
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
