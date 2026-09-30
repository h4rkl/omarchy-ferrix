#!/bin/sh
set -eu

# Device-free native-Linux Kbuild workflow for the separately pinned
# radio-first Pixel 9a backend.  This is intentionally not a mode of
# kernel.sh: the Google Kleaf/cpif source set and the s5300_modem WWAN source
# set expose different kernel/userspace ABIs and must never share a manifest.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
route_pins=${FERRIX_PIXEL9A_RADIO_ROUTE_PINS:-"$target/kernel-radio-route.toml"}
cache_root=${FERRIX_CACHE_DIR:-"$root/.cache/ferrix"}
radio_cache_root=${FERRIX_PIXEL9A_RADIO_KERNEL_CACHE_DIR:-"$cache_root/pixel-9a/kernel-radio"}
source_root=${FERRIX_PIXEL9A_RADIO_SOURCE:-"$radio_cache_root/source"}
out_root=${FERRIX_OUT_DIR:-"$root/out"}
kernel_out=${FERRIX_PIXEL9A_RADIO_KERNEL_OUT:-"$out_root/pixel-9a/kernel-radio/tegu-wwan"}
build_out="$kernel_out/build"
dist="$kernel_out/dist"
manifest=${FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST:-"$kernel_out/build-manifest.tsv"}
sync_confirmation=I-understand-this-downloads-the-radio-first-kernel
build_confirmation=I-understand-this-runs-the-radio-first-kernel-build
# The radio kernel source is large enough that a normal clone needlessly
# transfers its complete history and every blob.  Keep the optimization
# explicit and fixed: the checkout remains shallow, but Git can lazily fetch
# the blobs Kbuild actually reads through the promisor remote.
source_clone_depth=1
source_clone_filter=blob:none

die() {
	printf 'FAIL pixel-9a-radio-kernel: %s\n' "$*" >&2
	exit 1
}

sha256_of() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | awk '{ print $1 }'
	else
		shasum -a 256 "$1" | awk '{ print $1 }'
	fi
}

sha512_of() {
	if command -v sha512sum >/dev/null 2>&1; then
		sha512sum "$1" | awk '{ print $1 }'
	else
		shasum -a 512 "$1" | awk '{ print $1 }'
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

contract_value() {
	key=$1
	awk -v key="$key" '
		/^[[:space:]]*\[/ { section_seen=1 }
		!section_seen && $0 ~ ("^[[:space:]]*" key "[[:space:]]*=") {
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
	' "$split_contract"
}

contract_section_value() {
	section_name=$1
	key=$2
	awk -v section_name="$section_name" -v key="$key" '
		BEGIN { wanted = "[" section_name "]" }
		$0 == wanted { in_section=1; next }
		in_section && $0 ~ /^[[:space:]]*\[/ { exit }
		in_section && $0 ~ ("^[[:space:]]*" key "[[:space:]]*=") {
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
	' "$split_contract"
}

required_config() {
	sed -n '/^required_config[[:space:]]*=[[:space:]]*\[/,/^\]/p' "$route_pins" |
		sed -n 's/.*"\([^\"]*\)".*/\1/p'
}

check_path() {
	value=$1
	label=$2
	case "$value" in
		''|/*|*..*|*' '*) die "$label is not a safe source-relative path: $value" ;;
	esac
}

source_archive_path() {
	printf '%s/%s.tar.gz\n' "$(dirname -- "$source_root")" "$(route_value source_revision)"
}

verify_clone_strategy() {
	origin_url=$(git -C "$source_root" remote get-url origin 2>/dev/null || true)
	[ "$origin_url" = "$(route_value source_url)" ] ||
		die "radio kernel source remote mismatch: expected $(route_value source_url), found ${origin_url:-unset}"
	shallow=$(git -C "$source_root" rev-parse --is-shallow-repository 2>/dev/null || true)
	[ "$shallow" = true ] ||
		die 'radio kernel source checkout is not shallow; use a fresh source path instead of a full clone'
	commit_count=$(git -C "$source_root" rev-list --count HEAD 2>/dev/null || true)
	[ "$commit_count" = "$source_clone_depth" ] ||
		die "radio kernel source checkout is not depth-$source_clone_depth: found ${commit_count:-unknown} commits"
	promisor=$(git -C "$source_root" config --get remote.origin.promisor 2>/dev/null || true)
	[ "$promisor" = true ] ||
		die 'radio kernel source checkout is not a blob-filtered promisor clone'
	filter=$(git -C "$source_root" config --get remote.origin.partialclonefilter 2>/dev/null || true)
	[ "$filter" = "$source_clone_filter" ] ||
		die "radio kernel source checkout has unexpected partial-clone filter: ${filter:-unset}"
}

verify_source_archive() {
	archive=$1
	[ -f "$archive" ] || die "radio source archive is missing: $archive; run just pixel-9a-kernel-radio-sync"
	[ ! -L "$archive" ] || die "radio source archive must not be a symlink: $archive"
	expected_bytes=$(route_value source_archive_bytes)
	expected_sha256=$(route_value source_archive_sha256)
	expected_sha512=$(route_value source_archive_sha512)
	actual_bytes=$(file_size "$archive")
	[ "$actual_bytes" = "$expected_bytes" ] ||
		die "radio source archive size mismatch: expected $expected_bytes, found $actual_bytes"
	[ "$(sha256_of "$archive")" = "$expected_sha256" ] ||
		die 'radio source archive SHA-256 differs from the pinned route'
	[ "$(sha512_of "$archive")" = "$expected_sha512" ] ||
		die 'radio source archive SHA-512 differs from the pinned route'
}

check_split_contract() {
	[ -f "$split_contract" ] || die "radio split-artifact contract is missing: $split_contract"
	[ ! -L "$split_contract" ] || die "radio split-artifact contract must not be a symlink: $split_contract"
	contract_hash=$(route_value split_artifact_contract_sha256)
	case "$contract_hash" in
		''|*[!0-9a-f]*) die 'radio split_artifact_contract_sha256 is malformed' ;;
	esac
	[ "${#contract_hash}" -eq 64 ] || die 'radio split_artifact_contract_sha256 must contain 64 hexadecimal characters'
	[ "$(sha256_of "$split_contract")" = "$contract_hash" ] || die 'radio split-artifact contract hash differs from route metadata'
	[ "$(contract_value schema_version)" = 1 ] || die 'radio split-artifact contract schema_version is not 1'
	[ "$(contract_value target_id)" = google-pixel-9a-tegu-gxq96-mp1-0 ] || die 'radio split-artifact contract target_id is not the exact Pixel 9a target'
	[ "$(contract_value route_id)" = experimental-linux-google-tegu-s5300-wwan ] || die 'radio split-artifact contract route_id does not match the radio route'
	[ "$(contract_value status)" = source-build-required ] || die 'radio split-artifact contract must remain source-build-required'
	[ "$(contract_value source_only)" = true ] || die 'radio split-artifact contract must be source-only'
	[ "$(contract_value stock_modules_may_be_reused)" = false ] || die 'radio split-artifact contract permits stock module reuse'
	[ "$(contract_value android_userspace_required)" = false ] || die 'radio split-artifact contract requires Android userspace'
	[ "$(contract_value boot_header_version)" = 4 ] || die 'radio split-artifact contract boot header version is not 4'
	[ "$(contract_value boot_page_size)" = 4096 ] || die 'radio split-artifact contract boot page size is not 4096'
	[ "$(contract_value dtbo_tool)" = mkdtboimg.py ] || die 'radio split-artifact contract does not select mkdtboimg.py'
	[ "$(contract_value dtbo_page_size)" = 4096 ] || die 'radio split-artifact contract DTBO page size is not 4096'
	[ "$(contract_value dynamic_partition_transport)" = fastbootd ] || die 'radio split-artifact contract dynamic partition transport is not fastbootd'

	while IFS=: read -r artifact partition; do
		[ -n "$artifact" ] || continue
		section="artifact.$artifact"
		[ "$(contract_section_value "$section" partition)" = "$partition" ] ||
			die "radio split-artifact contract has the wrong partition for $artifact"
		[ "$(contract_section_value "$section" status)" = not-built ] ||
			die "radio split-artifact contract marks $artifact as built"
		[ -n "$(contract_section_value "$section" source_inputs)" ] ||
			die "radio split-artifact contract has no source inputs for $artifact"
	done <<'EOF'
boot:boot
init_boot:init_boot
vendor_boot:vendor_boot
vendor_kernel_boot:vendor_kernel_boot
dtbo:dtbo
vendor_dlkm:vendor_dlkm
system_dlkm:system_dlkm
vbmeta:vbmeta
vbmeta_vendor:vbmeta_vendor
vbmeta_system:vbmeta_system
EOF
	[ "$(contract_section_value artifact.vendor_dlkm transport)" = fastbootd ] || die 'radio split-artifact contract vendor_dlkm transport is not fastbootd'
	[ "$(contract_section_value artifact.system_dlkm transport)" = fastbootd ] || die 'radio split-artifact contract system_dlkm transport is not fastbootd'
	required_rows=$(contract_section_value verification required_rows) || die 'radio split-artifact contract has no verification row contract'
	for row in source_revision source_archive split_artifact_contract dtbo_source_set module_install partition_sizes artifact; do
		case "$required_rows" in
			*"$row"*) ;;
			*) die "radio split-artifact contract omits required manifest row: $row" ;;
		esac
	done
}

check_metadata() {
	[ -f "$route_pins" ] || die "radio route metadata is missing: $route_pins"
	[ "$(route_value schema_version)" = 1 ] || die 'radio route metadata schema_version is not 1'
	[ "$(route_value target_id)" = google-pixel-9a-tegu-gxq96-mp1-0 ] || die 'radio route target_id is not the exact Pixel 9a target'
	[ "$(route_value route_id)" = experimental-linux-google-tegu-s5300-wwan ] || die 'unexpected radio route id'
	[ "$(route_value status)" = experimental-not-adopted ] || die 'radio route must remain experimental-not-adopted until physical evidence exists'
	[ "$(route_value architecture)" = arm64 ] || die 'radio route architecture is not arm64'
	[ "$(route_value build_system)" = kbuild-make ] || die 'radio route is not the pinned Kbuild backend'
	[ "$(route_value compiler_mode)" = llvm ] || die 'radio route must use its pinned LLVM build contract'
	[ "$(route_value source_revision)" != "$(route_value source_branch)" ] || die 'radio route source revision may not be a branch name'
	check_path "$(route_value config_fragment)" config_fragment
	check_path "$(route_value extra_config_fragment)" extra_config_fragment
	check_path "$(route_value device_tree_source)" device_tree_source
	check_path "$(route_value device_tree_artifact)" device_tree_artifact
	check_path "$(route_value kernel_artifact)" kernel_artifact
	check_path "$(route_value firmware_mapping)" firmware_mapping
	check_path "$(route_value initfs_module_list)" initfs_module_list
	check_path "$(route_value split_artifact_contract)" split_artifact_contract
	[ "$(route_value gnss_binding)" = samsung,kepler ] || die 'radio route GNSS binding is not the reviewed KEPLER binding'
	check_path "$(route_value gnss_driver_source)" gnss_driver_source
	[ "$(route_value gnss_runtime_device_glob)" = '/dev/gnss*' ] || die 'radio route GNSS runtime device contract is not /dev/gnss*'
	[ "$(route_value gnss_userspace_status)" = raw-device-only ] || die 'radio route must keep GNSS userspace at raw-device-only'
	[ "$(route_value nfc_route_status)" = not-selected ] || die 'radio route unexpectedly selects an NFC backend'
	[ "$(route_value nfc_comparison_modules)" = st21nfc.ko,st54spi.ko ] || die 'radio route NFC comparison module record changed'
	[ "$(route_value st54_route_status)" = not-selected ] || die 'radio route unexpectedly selects an ST54 backend'
	split_contract="$target/$(route_value split_artifact_contract)"
	check_split_contract
	revision=$(route_value source_revision)
	case "$revision" in ''|*[!0-9a-f]*) die 'radio source_revision is malformed' ;; esac
	[ "${#revision}" -eq 40 ] || die 'radio source_revision must contain 40 hexadecimal characters'
	config_hash=$(route_value config_sha256)
	extra_config_hash=$(route_value extra_config_sha256)
	dtb_hash=$(route_value device_tree_sha256)
	archive_bytes=$(route_value source_archive_bytes)
	archive_hash=$(route_value source_archive_sha256)
	archive_sha512=$(route_value source_archive_sha512)
	case "$archive_bytes" in ''|*[!0-9]*) die 'radio source_archive_bytes is malformed' ;; esac
	case "$archive_hash" in ''|*[!0-9a-f]*) die 'radio source_archive_sha256 is malformed' ;; esac
	case "$archive_sha512" in ''|*[!0-9a-f]*) die 'radio source_archive_sha512 is malformed' ;; esac
	[ "${#archive_hash}" -eq 64 ] || die 'radio source_archive_sha256 must contain 64 hexadecimal characters'
	[ "${#archive_sha512}" -eq 128 ] || die 'radio source_archive_sha512 must contain 128 hexadecimal characters'
	case "$config_hash" in ''|*[!0-9a-f]*) die 'radio config_sha256 is malformed' ;; esac
	case "$dtb_hash" in ''|*[!0-9a-f]*) die 'radio device_tree_sha256 is malformed' ;; esac
	case "$extra_config_hash" in ''|*[!0-9a-f]*) die 'radio extra_config_sha256 is malformed' ;; esac
	[ "${#config_hash}" -eq 64 ] || die 'radio config_sha256 must contain 64 hexadecimal characters'
	[ "${#extra_config_hash}" -eq 64 ] || die 'radio extra_config_sha256 must contain 64 hexadecimal characters'
	[ "${#dtb_hash}" -eq 64 ] || die 'radio device_tree_sha256 must contain 64 hexadecimal characters'
	case "$(route_value source_url)" in
		https://github.com/SteffenDE/linux-google-tegu.git) ;;
		*) die 'radio route source URL is not the reviewed linux-google-tegu repository' ;;
	esac
	case "$(route_value source_archive_url)" in
		https://github.com/SteffenDE/linux-google-tegu/archive/*.tar.gz) ;;
		*) die 'radio route source archive URL is not the reviewed commit archive' ;;
	esac
	[ "$(route_value source_archive_url)" = "https://github.com/SteffenDE/linux-google-tegu/archive/$revision.tar.gz" ] ||
		die 'radio route source archive URL is not derived from source_revision'
	mapping="$target/$(route_value firmware_mapping)"
	[ -f "$mapping" ] || die "radio firmware mapping is missing: $mapping"
	[ "$(sha256_of "$mapping")" = "$(route_value firmware_mapping_sha256)" ] ||
		die 'radio firmware mapping hash differs from the pinned route metadata'
	initfs_modules="$target/$(route_value initfs_module_list)"
	[ -f "$initfs_modules" ] || die "radio initfs module list is missing: $initfs_modules"
	if grep -Eiq '(^|[^[:alnum:]_.-])(bcm47765|st21nfc|st54spi|kepler)([^[:alnum:]_.-]|$)' "$initfs_modules"; then
		die 'radio initfs module list carries an unreviewed associated-radio or Android NFC module'
	fi
	[ -n "$(required_config)" ] || die 'radio route has no required kernel configuration contract'
}

check_host() {
	os=$(uname -s)
	arch=$(uname -m)
	[ "$os" = Linux ] || die "native Linux host required; detected $os/$arch"
	[ "$arch" = x86_64 ] || die "native Linux x86_64 host required; detected $os/$arch"
	for command_name in awk curl find git grep make mkdir perl python3 sed sort tr uname wc; do
		command -v "$command_name" >/dev/null 2>&1 || die "missing prerequisite '$command_name'; install it explicitly"
	done
	for command_name in bc bison depmod flex; do
		command -v "$command_name" >/dev/null 2>&1 || die "missing Kbuild prerequisite '$command_name'; install it explicitly"
	done
	for command_name in clang ld.lld llvm-ar llvm-nm llvm-objcopy llvm-objdump llvm-readelf; do
		command -v "$command_name" >/dev/null 2>&1 || die "missing LLVM prerequisite '$command_name'; install it explicitly"
	done
	if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
		die 'missing sha256sum or shasum'
	fi
	if ! command -v sha512sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
		die 'missing sha512sum or shasum'
	fi
}

print_resolution() {
	printf '%s\n' 'target: google-pixel-9a-tegu-gxq96-mp1-0'
	printf '%s\n' "route: $(route_value route_id)"
	printf '%s\n' "source: $source_root"
	printf '%s\n' "source URL: $(route_value source_url)"
	printf '%s\n' "source branch: $(route_value source_branch)"
	printf '%s\n' "source revision: $(route_value source_revision)"
	printf '%s\n' "source archive: $(route_value source_archive_url)"
	printf '%s\n' "source archive cache: $(source_archive_path)"
	printf '%s\n' "source archive sha256: $(route_value source_archive_sha256)"
	printf '%s\n' "source archive sha512: $(route_value source_archive_sha512)"
	printf '%s\n' "source clone: shallow depth=$source_clone_depth, filter=$source_clone_filter, single branch, no tags"
	printf '%s\n' "extra config: $(route_value extra_config_fragment) ($(route_value extra_config_sha256))"
	printf '%s\n' "firmware mapping: $(route_value firmware_mapping)"
	printf '%s\n' "initfs module list: $(route_value initfs_module_list)"
	printf '%s\n' "split-artifact contract: $(route_value split_artifact_contract) ($(route_value split_artifact_contract_sha256))"
	printf '%s\n' "build output: $kernel_out"
	printf '%s\n' 'build command: make ARCH=arm64 LLVM=1 O=<output> defconfig + tegu.config + kernel-radio-extra.config + Image dtbs modules modules_install'
}

verify_source() {
	check_metadata
	[ -d "$source_root/.git" ] || die "radio kernel source is not initialized: $source_root; run just pixel-9a-kernel-radio-sync"
	verify_clone_strategy
	verify_source_archive "$(source_archive_path)"
	actual=$(git -C "$source_root" rev-parse HEAD 2>/dev/null) || die 'radio kernel source is not a readable Git checkout'
	expected=$(route_value source_revision)
	[ "$actual" = "$expected" ] || die "radio kernel source revision mismatch: expected $expected, found $actual"
	git -C "$source_root" diff --quiet --ignore-submodules -- || die 'radio kernel source has unstaged changes'
	git -C "$source_root" diff --cached --quiet --ignore-submodules -- || die 'radio kernel source has staged changes'
	if git -C "$source_root" status --porcelain --untracked-files=all | grep -q .; then
		die 'radio kernel source has untracked changes'
	fi
	config="$source_root/$(route_value config_fragment)"
	extra_config="$target/$(route_value extra_config_fragment)"
	dtb_source="$source_root/$(route_value device_tree_source)"
	[ -f "$config" ] || die "radio config fragment is missing: $config"
	[ -f "$extra_config" ] || die "radio extra config fragment is missing: $extra_config"
	[ -f "$dtb_source" ] || die "radio device-tree source is missing: $dtb_source"
	[ "$(sha256_of "$config")" = "$(route_value config_sha256)" ] || die 'radio config fragment hash differs from the pinned route metadata'
	[ "$(sha256_of "$extra_config")" = "$(route_value extra_config_sha256)" ] || die 'radio extra config fragment hash differs from the pinned route metadata'
	[ "$(sha256_of "$dtb_source")" = "$(route_value device_tree_sha256)" ] || die 'radio device-tree source hash differs from the pinned route metadata'
	printf '%s\n' "PASS radio kernel source: $actual, config and device-tree hashes verified"
}

verify_target_overlay_source() {
	overlay_dir="$source_root/arch/arm64/boot/dts/exynos/google"
	[ -f "$overlay_dir/zumapro-tegu-mp.dtso" ] ||
		die 'pinned kernel source has no tegu MP1.0 overlay source; refusing an unusable build'
	[ -f "$overlay_dir/Makefile" ] && grep -Fq 'zumapro-tegu-mp.dtbo' "$overlay_dir/Makefile" ||
		die 'pinned kernel Kbuild does not select the tegu MP1.0 overlay'
}

preflight() {
	check_metadata
	print_resolution
	check_host
	printf '%s\n' 'PASS pixel-9a-radio-kernel host and route metadata'
}

sync_source() {
	preflight >/dev/null
	[ "${FERRIX_PIXEL9A_RADIO_SYNC:-}" = "$sync_confirmation" ] ||
		die "radio kernel source sync is opt-in; set FERRIX_PIXEL9A_RADIO_SYNC=$sync_confirmation"
	if [ -e "$source_root" ]; then
		[ -d "$source_root/.git" ] || die "radio source path exists but is not a Git checkout: $source_root"
		verify_clone_strategy
	fi
	archive=$(source_archive_path)
	mkdir -p "$(dirname -- "$archive")"
	if [ -e "$archive" ]; then
		verify_source_archive "$archive"
	else
		archive_tmp="$archive.tmp.$$"
		[ ! -e "$archive_tmp" ] || die "refusing to overwrite an existing archive download: $archive_tmp"
		curl --fail --location --retry 3 --connect-timeout 30 \
			--output "$archive_tmp" "$(route_value source_archive_url)"
		if ! (verify_source_archive "$archive_tmp"); then
			rm -f "$archive_tmp"
			exit 1
		fi
		mv "$archive_tmp" "$archive"
	fi
	if [ ! -e "$source_root" ]; then
		mkdir -p "$(dirname -- "$source_root")"
		git clone \
			--filter="$source_clone_filter" \
			--no-tags \
			--single-branch \
			--depth="$source_clone_depth" \
			--branch "$(route_value source_branch)" \
			--no-checkout \
			"$(route_value source_url)" "$source_root"
		verify_clone_strategy
	fi
	git -C "$source_root" fetch \
		--no-tags \
		--filter="$source_clone_filter" \
		--depth="$source_clone_depth" \
		origin "$(route_value source_revision)"
	git -C "$source_root" checkout --detach "$(route_value source_revision)"
	verify_source
}

check_built_config() {
	config_file="$build_out/.config"
	[ -f "$config_file" ] || die 'radio kernel build did not produce .config'
	while IFS= read -r symbol; do
		[ -n "$symbol" ] || continue
		grep -qx "$symbol" "$config_file" || die "radio kernel .config lacks required symbol: $symbol"
	done <<EOF
$(required_config)
EOF
	# This physical phone does not host virtual machines. Keep KVM disabled in
	# the target-local route; otherwise Zig's nVHE objects produce GOT
	# relocations 311/312 that gen-hyprel intentionally rejects.
	grep -qx '# CONFIG_KVM is not set' "$config_file" ||
		die 'radio kernel .config must keep CONFIG_KVM disabled for the physical route'
}

write_dtbo_source_set() {
	# Kbuild produces the board overlay, but not the Android DTBO container.
	# Preserve only the exact MP1.0 source-built input for image composition.
	dtb_source_root="$build_out/arch/arm64/boot/dts"
	[ -d "$dtb_source_root" ] || die "radio kernel DTB output directory is missing: $dtb_source_root"
	relative=exynos/google/zumapro-tegu-mp.dtbo
	source_file="$dtb_source_root/$relative"
	[ -s "$source_file" ] ||
		die 'radio kernel build produced no tegu MP1.0 DTBO overlay'
	source_dir="$dist/split-inputs/dtbo-source-set"
	source_manifest="$dist/split-inputs/dtbo-source-set.tsv"
	destination="$source_dir/$relative"
	mkdir -p "$(dirname -- "$destination")"
	cp "$source_file" "$destination"
	tmp="$source_manifest.tmp.$$"
	trap 'rm -f "$tmp"' EXIT HUP INT TERM
	{
		printf 'schema_version\t1\n'
		printf 'source_root\tarch/arm64/boot/dts\n'
		printf 'source\t%s\t%s\t%s\n' "$relative" "$(file_size "$destination")" "$(sha256_of "$destination")"
	} >"$tmp"
	mv "$tmp" "$source_manifest"
	trap - EXIT HUP INT TERM
}

verify_dtbo_application() {
	fdt_tool="$build_out/scripts/dtc/fdtoverlay"
	[ -x "$fdt_tool" ] || die 'Kbuild did not produce fdtoverlay for base/MP overlay validation'
	overlay="$dist/split-inputs/dtbo-source-set/exynos/google/zumapro-tegu-mp.dtbo"
	merged="$dist/split-inputs/tegu-mp-applied.dtb"
	log="$dist/split-inputs/tegu-mp-apply.log"
	if ! "$fdt_tool" -i "$dist/dtb.img" -o "$merged" "$overlay" >"$log" 2>&1; then
		cat "$log" >&2
		die 'source-built tegu MP1.0 overlay cannot be applied to source-built base DTB'
	fi
	[ -s "$merged" ] || die 'tegu MP1.0 overlay application produced an empty DTB'
}

install_kernel_modules() {
	# Keep module installation as a source-built input handoff. The output is
	# deliberately not called vendor_dlkm/system_dlkm until a partition image
	# builder assigns modules to those exact Pixel partitions.
	module_dir="$dist/split-inputs/module-install"
	module_manifest="$dist/split-inputs/module-install.tsv"
	mkdir -p "$module_dir"
	(
		cd "$source_root"
		MAKEFLAGS="-j$(kernel_make_jobs)" make ARCH=arm64 LLVM=1 O="$build_out" modules_install INSTALL_MOD_PATH="$module_dir"
	)
	module_list="$dist/split-inputs/.module-install-list.$$"
	find "$module_dir" -type f -print | LC_ALL=C sort >"$module_list"
	tmp="$module_manifest.tmp.$$"
	trap 'rm -f "$tmp" "$module_list"' EXIT HUP INT TERM
	{
		printf 'schema_version\t1\n'
		printf 'install_root\tsplit-inputs/module-install\n'
		while IFS= read -r module_file; do
			relative=${module_file#"$module_dir"/}
			case "$relative" in
				''|/*|*..*|*' '*) die "unsafe installed module path: $relative" ;;
			esac
			printf 'file\t%s\t%s\t%s\n' "$relative" "$(file_size "$module_file")" "$(sha256_of "$module_file")"
		done <"$module_list"
	} >"$tmp"
	mv "$tmp" "$module_manifest"
	rm -f "$module_list"
	trap - EXIT HUP INT TERM
}

kernel_make_jobs() {
	# The native builder is deliberately single-architecture and runs on a
	# Linux x86_64 host. Use all reported host CPUs for the expensive Kbuild
	# phases while retaining a deterministic single-job fallback for unusual
	# minimal environments.
	jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || printf '1')
	case "$jobs" in
		''|*[!0-9]*|0) jobs=1 ;;
	esac
	printf '%s\n' "$jobs"
}

write_partition_size_status() {
	# No partition size is claimed before a real split-image builder emits the
	# bytes. This status file keeps that omission explicit and machine-readable.
	partition_manifest="$dist/split-inputs/partition-sizes.tsv"
	mkdir -p "$(dirname -- "$partition_manifest")"
	tmp="$partition_manifest.tmp.$$"
	trap 'rm -f "$tmp"' EXIT HUP INT TERM
	{
		printf 'schema_version\t1\n'
		for partition in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
			printf 'partition\t%s\tstatus=not-built\n' "$partition"
		done
	} >"$tmp"
	mv "$tmp" "$partition_manifest"
	trap - EXIT HUP INT TERM
}

write_manifest() {
	image="$dist/Image"
	dtb="$dist/dtb.img"
	dtbo_source_set="$dist/split-inputs/dtbo-source-set.tsv"
	module_install="$dist/split-inputs/module-install.tsv"
	partition_sizes="$dist/split-inputs/partition-sizes.tsv"
	[ -f "$dtbo_source_set" ] || die 'radio split input manifest is missing: dtbo-source-set.tsv'
	[ -f "$module_install" ] || die 'radio split input manifest is missing: module-install.tsv'
	[ -f "$partition_sizes" ] || die 'radio split input manifest is missing: partition-sizes.tsv'
	tmp="$manifest.tmp.$$"
	trap 'rm -f "$tmp"' EXIT HUP INT TERM
	{
		printf 'schema_version\t1\n'
		printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
		printf 'route_id\t%s\n' "$(route_value route_id)"
		printf 'kernel_backend\tlinux-google-tegu-wwan\n'
		printf 'status\tbuilt-not-booted\n'
		printf 'bootable\tfalse\n'
		printf 'build_system\tkbuild-make\n'
		printf 'architecture\tarm64\n'
		printf 'compiler_mode\tllvm\n'
		printf 'source_url\t%s\n' "$(route_value source_url)"
		printf 'source_branch\t%s\n' "$(route_value source_branch)"
		printf 'source_revision\t%s\n' "$(route_value source_revision)"
		source_archive=$(source_archive_path)
		verify_source_archive "$source_archive"
		printf 'source_archive\t%s\t%s\t%s\t%s\n' "$source_archive" "$(file_size "$source_archive")" "$(sha256_of "$source_archive")" "$(sha512_of "$source_archive")"
		printf 'config_fragment\t%s\t%s\n' "$(route_value config_fragment)" "$(route_value config_sha256)"
		printf 'extra_config_fragment\t%s\t%s\n' "$(route_value extra_config_fragment)" "$(route_value extra_config_sha256)"
		printf 'device_tree_source\t%s\t%s\n' "$(route_value device_tree_source)" "$(route_value device_tree_sha256)"
		printf 'firmware_mapping\t%s\t%s\n' "$(route_value firmware_mapping)" "$(route_value firmware_mapping_sha256)"
		printf 'initfs_module_list\t%s\t%s\n' "$(route_value initfs_module_list)" "$(sha256_of "$target/$(route_value initfs_module_list)")"
		printf 'split_artifact_contract\t%s\t%s\n' "$(route_value split_artifact_contract)" "$(route_value split_artifact_contract_sha256)"
		printf 'dtbo_source_set\tsplit-inputs/dtbo-source-set.tsv\t%s\t%s\n' "$(file_size "$dtbo_source_set")" "$(sha256_of "$dtbo_source_set")"
		printf 'module_install\tsplit-inputs/module-install.tsv\t%s\t%s\n' "$(file_size "$module_install")" "$(sha256_of "$module_install")"
		printf 'partition_sizes\tsplit-inputs/partition-sizes.tsv\t%s\t%s\n' "$(file_size "$partition_sizes")" "$(sha256_of "$partition_sizes")"
		printf 'artifact\tImage\t%s\t%s\n' "$(file_size "$image")" "$(sha256_of "$image")"
		printf 'artifact\tdtb.img\t%s\t%s\n' "$(file_size "$dtb")" "$(sha256_of "$dtb")"
		find "$dist" -type f -name '*.ko' -print | LC_ALL=C sort | while IFS= read -r module; do
			relative=${module#"$dist"/}
			printf 'artifact\t%s\t%s\t%s\n' "$relative" "$(file_size "$module")" "$(sha256_of "$module")"
		done
	} >"$tmp"
	mv "$tmp" "$manifest"
	trap - EXIT HUP INT TERM
}

build_kernel() {
	preflight >/dev/null
	verify_source
	verify_target_overlay_source
	[ "${FERRIX_PIXEL9A_RADIO_BUILD:-}" = "$build_confirmation" ] ||
		die "radio kernel build is opt-in; set FERRIX_PIXEL9A_RADIO_BUILD=$build_confirmation"
	[ ! -e "$kernel_out" ] || die "refusing to overwrite existing radio kernel output; choose a fresh FERRIX_PIXEL9A_RADIO_KERNEL_OUT: $kernel_out"
	mkdir -p "$build_out" "$dist"
	config="$source_root/$(route_value config_fragment)"
	extra_config="$target/$(route_value extra_config_fragment)"
	merge_config="$source_root/scripts/kconfig/merge_config.sh"
	[ -x "$merge_config" ] || die "kernel config merge helper is missing or not executable: $merge_config"
	jobs=$(kernel_make_jobs)
	(
		cd "$source_root"
		MAKEFLAGS="-j$jobs" make ARCH=arm64 LLVM=1 O="$build_out" defconfig
		MAKEFLAGS="-j$jobs" ARCH=arm64 LLVM=1 "$merge_config" -m -O "$build_out" "$build_out/.config" "$config" "$extra_config"
		MAKEFLAGS="-j$jobs" make ARCH=arm64 LLVM=1 O="$build_out" olddefconfig
		MAKEFLAGS="-j$jobs" make ARCH=arm64 LLVM=1 O="$build_out" Image dtbs modules
	)
	check_built_config
	image="$build_out/$(route_value kernel_artifact)"
	dtb="$build_out/$(route_value device_tree_artifact)"
	[ -f "$image" ] || die "radio kernel artifact is missing: $image"
	[ -f "$dtb" ] || die "radio device-tree artifact is missing: $dtb"
	cp "$image" "$dist/Image"
	cp "$dtb" "$dist/dtb.img"
	write_dtbo_source_set
	verify_dtbo_application
	install_kernel_modules
	write_partition_size_status
	write_manifest
	printf '%s\n' "PASS pixel-9a-radio-kernel build: artifacts and hashes recorded in $manifest"
}

usage() {
	printf '%s\n' 'usage: tools/pixel-9a/kernel-radio.sh preflight|sync|verify|build' >&2
	exit 2
}

case "${1:-}" in
	preflight) [ "$#" -eq 1 ] || usage; preflight ;;
	sync) [ "$#" -eq 1 ] || usage; sync_source ;;
	verify) [ "$#" -eq 1 ] || usage; verify_source ;;
	build) [ "$#" -eq 1 ] || usage; build_kernel ;;
	*) usage ;;
esac
