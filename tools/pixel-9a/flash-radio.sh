#!/bin/sh
set -eu

# Target-scoped, confirmation-gated temporary boot and persistent flash of the
# independently source-built radio-first Pixel 9a split artifacts.  This is a
# separate route from flash.sh: the latter owns the Google Kleaf/CPIF ABI and
# must never consume the linux-google-tegu-wwan manifest produced by
# radio-split.sh.
#
# Device verbs used by this file, and nothing else:
#   adb devices -l
#   fastboot devices -l
#   fastboot -s <serial> getvar <variable>
#   fastboot -s <serial> boot <boot.img>
#   fastboot -s <serial> flash <partition> <image>
#   fastboot -s <serial> reboot fastboot
#   fastboot -s <serial> reboot bootloader
#
# It never unlocks or locks a bootloader, changes slots, erases, formats,
# wipes, updates a factory archive, or flashes bootloader/radio firmware.  The
# factory archive remains the separately reviewed recovery path.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
. "$root/tools/pixel-9a/data-safety.sh"
. "$root/tools/pixel-9a/adb-correlation.sh"
. "$root/tools/pixel-9a/temp-boot-observation.sh"
target=${FERRIX_PIXEL9A_TARGET_DIR:-"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"}
route="$target/kernel-radio-route.toml"
contract="$target/radio-split-artifact-contract.toml"
pins=${FERRIX_PIXEL9A_PINS:-"$target/recovery.pins"}
recovery_dir="$target/recovery"
layout="$target/image-layout.toml"
deviceinfo="$target/pmos/device-google-tegu/deviceinfo"
target_manifest="$target/manifest.toml"
out_root=${FERRIX_OUT_DIR:-"$root/out"}
image_out=${FERRIX_PIXEL9A_RADIO_SPLIT_OUT:-"$out_root/pixel-9a/images/tegu-wwan-split"}
manifest=${FERRIX_PIXEL9A_RADIO_SPLIT_MANIFEST:-"$image_out/boot-manifest.tsv"}
evidence_dir="$out_root/pixel-9a/flash-radio"
wait_seconds=${FERRIX_PIXEL9A_FLASH_WAIT_SECONDS:-90}
tab=$(printf '\t')

expected_target_id=google-pixel-9a-tegu-gxq96-mp1-0
expected_product=tegu
expected_sku=GXQ96
expected_revision=MP1.0
expected_route_id=experimental-linux-google-tegu-s5300-wwan
expected_backend=linux-google-tegu-wwan

# Physical partitions written from bootloader Fastboot, in the order used by
# the split composer.  Dynamic partitions are written only from fastbootd.
physical_artifacts='boot init_boot vendor_boot vendor_kernel_boot dtbo vbmeta vbmeta_vendor vbmeta_system'
dynamic_artifacts='vendor_dlkm system_dlkm'
source_artifacts='Image dtb.img'
all_artifacts='Image dtb.img boot init_boot vendor_boot vendor_kernel_boot dtbo vbmeta vbmeta_vendor vbmeta_system vendor_dlkm system_dlkm rootfs_handoff rootfs_image'
bound_artifacts='boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm'
rootfs_partition_allowlist=userdata

mode=
stage=kernel
slot_override=
allow_unsigned_avb=no
skip_temp_boot_gate=no
serial=
fastboot_serial=
transcript=
verified_bootloader_serial_prefix=

usage() {
	cat >&2 <<'EOF'
usage:
  tools/pixel-9a/flash-radio.sh preflight [--stage kernel|rootfs|all] [--slot a|b] [--allow-unsigned-avb]
  tools/pixel-9a/flash-radio.sh boot-temp [--allow-unsigned-avb]
  tools/pixel-9a/flash-radio.sh observe-temp [--allow-unsigned-avb]
  tools/pixel-9a/flash-radio.sh flash [--stage kernel|rootfs|all] [--slot a|b] [--allow-unsigned-avb] [--skip-temp-boot-gate]

preflight   read-only radio-first manifest, recovery, identity, and gate checks
boot-temp   temporary fastboot boot of radio-first boot.img; no persistent write
observe-temp  record reviewed kernel progress after the device returns to
              ordinary Fastboot; requires FERRIX_PIXEL9A_RADIO_TEMP_BOOT_EVIDENCE
              and FERRIX_PIXEL9A_RADIO_TEMP_BOOT_KERNEL_LOG (redacted text)
flash       persistent slot-explicit kernel/rootfs flash; requires the
            boot-temp evidence gate unless explicitly skipped and acknowledged
EOF
	exit 2
}

die() {
	printf 'REFUSING: %s\n' "$*" >&2
	if [ -n "$transcript" ]; then
		message=$(printf '%s\n' "$*" | redact_literal "$serial" "serial-sha256:$(sha256_prefix "${serial:-none}")")
		printf 'refused=%s\n' "$message" >>"$transcript"
	fi
	exit 1
}

fail() {
	emit result "$1"
	printf 'FAILED: %s\n' "$2" >&2
	printf '%s\n' 'Restore the exact device with the verified factory archive before normal use.' >&2
	exit 1
}

need() {
	command -v "$1" >/dev/null 2>&1 || die "missing $1; install Android Platform Tools explicitly"
}

sha256_of() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | awk '{ print $1 }'
	else
		shasum -a 256 "$1" | awk '{ print $1 }'
	fi
}

sha256_prefix() {
	if command -v sha256sum >/dev/null 2>&1; then
		printf '%s' "$1" | sha256sum | cut -c1-12
	else
		printf '%s' "$1" | shasum -a 256 | cut -c1-12
	fi
}

bytes_of() { wc -c <"$1" | tr -d '[:space:]'; }

count_rows() {
	printf '%s\n' "$1" | awk 'NF { count++ } END { print count + 0 }'
}

# Replace the serial as a literal byte string.  This keeps ignored transcripts
# useful without placing a unique hardware identifier in generated evidence.
redact_literal() {
	REDACT_NEEDLE=$1 REDACT_REPLACEMENT=$2 awk '
		BEGIN { needle = ENVIRON["REDACT_NEEDLE"]; replacement = ENVIRON["REDACT_REPLACEMENT"] }
		{
			if (needle == "") { print; next }
			output = ""
			rest = $0
			while ((position = index(rest, needle)) != 0) {
				output = output substr(rest, 1, position - 1) replacement
				rest = substr(rest, position + length(needle))
			}
			print output rest
		}
	'
}

emit() {
	key=$1
	value=$2
	printf '%s=%s\n' "$key" "$value"
	[ -n "$transcript" ] || return 0
	if [ -n "$serial" ]; then
		value=$(printf '%s\n' "$value" | redact_literal "$serial" "serial-sha256:$(sha256_prefix "$serial")")
	fi
	printf '%s=%s\n' "$key" "$value" >>"$transcript"
}

open_transcript() {
	mkdir -p "$evidence_dir"
	stamp=$(date -u +%Y%m%dT%H%M%SZ)
	transcript="$evidence_dir/$mode-$stamp-$$.txt"
	{
		printf '# Pixel 9a radio-first %s transcript, %s UTC, host %s\n' "$mode" "$stamp" "$(uname -s) $(uname -m)"
		printf '# Serial numbers are replaced by a SHA-256 prefix. Never commit this file.\n'
	} >"$transcript"
	emit operation "pixel-9a-radio-$mode"
	emit target_id "$expected_target_id"
}

parse_args() {
	[ "$#" -ge 1 ] || usage
	mode=$1
	shift
	case "$mode" in
		preflight|boot-temp|observe-temp|flash) ;;
		*) usage ;;
	esac
	while [ "$#" -gt 0 ]; do
		case "$1" in
			--stage)
				[ "$#" -ge 2 ] || usage
				case "$mode" in boot-temp|observe-temp) usage ;; esac
				case "$2" in
					kernel|rootfs|all) stage=$2 ;;
					*) usage ;;
				 esac
				shift 2
				;;
			--slot)
				[ "$#" -ge 2 ] || usage
				case "$mode" in boot-temp|observe-temp) usage ;; esac
				case "$2" in
					a|b) slot_override=$2 ;;
					*) usage ;;
				 esac
				shift 2
				;;
			--allow-unsigned-avb)
				allow_unsigned_avb=yes
				shift
				;;
			--skip-temp-boot-gate)
				[ "$mode" = flash ] || usage
				skip_temp_boot_gate=yes
				shift
				;;
			*) usage ;;
		esac
	done
}

field() { printf '%s\n' "$1" | awk -F "$tab" -v number="$2" '{ print $number }'; }

manifest_value() {
	awk -F "$tab" -v key="$1" '$1 == key { print $2; exit }' "$manifest"
}

manifest_artifact_row() {
	awk -F "$tab" -v name="$1" '$1 == "artifact" && $2 == name { print; found=1; exit } END { if (!found) exit 1 }' "$manifest"
}

manifest_binding_row() {
	awk -F "$tab" -v name="$1" '$1 == "avb_bound_image" && $2 == name { print; found=1; exit } END { if (!found) exit 1 }' "$manifest"
}

safe_relative_path() {
	case "$1" in
		''|/*|.|..|../*|*/../*|*/..|*' '*|*"$tab"*) die "manifest contains an unsafe relative artifact path: $1" ;;
	esac
}

require_file() {
	file=$1
	label=$2
	[ -f "$file" ] || die "$label is missing: $file"
	[ ! -L "$file" ] || die "$label must not be a symlink: $file"
}

route_value() {
	key=$1
	awk -v key="$key" '
		$0 ~ "^" key "[[:space:]]*=" {
			value=$0
			sub(/^[^=]*=[[:space:]]*/, "", value)
			sub(/[[:space:]]*#.*/, "", value)
			if (value ~ /^"/) { sub(/^"/, "", value); sub(/"[[:space:]]*$/, "", value) }
			print value
			found=1
			exit
		}
		END { if (!found) exit 1 }
	' "$route"
}

check_route() {
	require_file "$route" 'radio route metadata'
	require_file "$contract" 'radio split-artifact contract'
	[ "$(route_value target_id)" = "$expected_target_id" ] || die 'radio route target is not the exact Pixel 9a'
	[ "$(route_value route_id)" = "$expected_route_id" ] || die 'radio route is not the reviewed WWAN route'
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

check_layout() {
	require_file "$layout" 'target image layout'
	grep -Fqx "target_id = \"$expected_target_id\"" "$layout" || die 'image layout identity is not the exact Pixel 9a target'
	grep -Fqx 'temporary_boot_before_persistent_write = true' "$layout" || die 'radio-first route requires the temporary-boot policy'
	for name in $physical_artifacts $dynamic_artifacts; do
		case "$name" in
			vbmeta|vbmeta_system|vbmeta_vendor)
				grep -q "\"$name.img\"" "$layout" || die "image layout omits verified-boot artifact $name.img" ;;
			*) grep -q "^\[partition.$name\]$" "$layout" || die "image layout has no [partition.$name] mapping" ;;
		esac
	done
}

check_manifest_shape() {
	require_file "$manifest" 'radio-first boot manifest'
	awk -F "$tab" '
		$1 == "artifact" && NF != 6 { bad=1 }
		$1 == "artifact" && $4 !~ /^[0-9]+$/ { bad=1 }
		$1 == "artifact" && $5 !~ /^[0-9a-f]{64}$/ { bad=1 }
		$1 == "avb_bound_image" && NF != 5 { bad=1 }
		$1 == "avb_bound_image" && $4 !~ /^[0-9]+$/ { bad=1 }
		$1 == "avb_bound_image" && $5 !~ /^[0-9a-f]{64}$/ { bad=1 }
		END { exit bad }
	' "$manifest" || die 'radio-first manifest has malformed artifact or AVB binding rows'
	[ "$(manifest_value schema_version)" = 1 ] || die 'radio-first manifest schema_version is not 1'
	[ "$(manifest_value target_id)" = "$expected_target_id" ] || die 'radio-first manifest target_id is not the exact Pixel 9a target'
	[ "$(manifest_value codename)" = "$expected_product" ] || die 'radio-first manifest codename is not tegu'
	[ "$(manifest_value hardware_revision)" = "$expected_revision" ] || die 'radio-first manifest hardware_revision is not MP1.0'
	[ "$(manifest_value route_id)" = "$expected_route_id" ] || die 'radio-first manifest route_id is not the selected route'
	[ "$(manifest_value kernel_backend)" = "$expected_backend" ] || die 'radio-first manifest kernel_backend is not linux-google-tegu-wwan'
	[ "$(manifest_value status)" = composed-not-booted ] || die 'radio-first manifest must remain composed-not-booted before observed boot evidence'
	[ "$(manifest_value bootable)" = false ] || die 'radio-first manifest must remain bootable=false before observed boot evidence'
	[ "$(manifest_value header_version)" = 4 ] || die 'radio-first manifest header_version is not 4'
	[ "$(manifest_value partition_contract)" = complete-source-built ] || die 'radio-first manifest does not claim the complete source-built split contract'
	[ "$(manifest_value avb_mode)" = fresh-generated ] || die 'radio-first manifest AVB metadata was not freshly generated'
	[ "$(manifest_value avb_descriptor_inputs)" = fresh-avbtool-hash-footers-from-final-images ] || die 'radio-first manifest lacks fresh final-image AVB provenance'
	[ "$(manifest_value avb_binding_salt)" = sha256-of-pre-footer-image-bytes ] || die 'radio-first manifest lacks the reviewed AVB salt contract'
	avb_algorithm=$(manifest_value avb_algorithm)
	case "$avb_algorithm" in
		''|*[!A-Za-z0-9_-]*) die 'radio-first manifest AVB algorithm is malformed' ;;
	esac
	contract_row=$(manifest_value split_artifact_contract)
	[ "$contract_row" = "$(route_value split_artifact_contract)" ] || die 'radio-first manifest split-artifact contract path is not the target contract'
	[ "$(awk -F "$tab" '$1 == "split_artifact_contract" { print $3; exit }' "$manifest")" = "$expected_contract_hash" ] || die 'radio-first manifest split-artifact contract hash differs from route metadata'
	artifact_count=$(awk -F "$tab" '$1 == "artifact" { count++ } END { print count + 0 }' "$manifest")
	[ "$artifact_count" -eq 14 ] || die "radio-first manifest must contain exactly 14 artifact rows, found $artifact_count"
	binding_count=$(awk -F "$tab" '$1 == "avb_bound_image" { count++ } END { print count + 0 }' "$manifest")
	[ "$binding_count" -eq 7 ] || die "radio-first manifest must contain exactly 7 AVB binding rows, found $binding_count"
}

artifact_expected_relative() {
	case "$1" in
		Image) printf '%s\n' Image ;;
		dtb.img) printf '%s\n' dtb.img ;;
		rootfs_handoff) printf '%s\n' rootfs.tar ;;
		rootfs_image) printf '%s\n' rootfs.img ;;
		*) printf '%s.img\n' "$1" ;;
	esac
}

artifact_origin_allowed() {
	case "$1:$2" in
		Image:kernel|dtb.img:kernel|\
		boot:generated-mkbootimg|init_boot:generated-mkbootimg|vendor_boot:generated-mkbootimg|vendor_kernel_boot:generated-mkbootimg|dtbo:generated-mkdtboimg|vendor_dlkm:source-built-modules|system_dlkm:source-built-modules|vbmeta:generated-avbtool-from-final-images|vbmeta_vendor:generated-avbtool-from-final-images|vbmeta_system:generated-avbtool-from-final-images|rootfs_handoff:rootfs|rootfs_image:rootfs) return 0 ;;
		*) return 1 ;;
	esac
}

verify_artifact() {
	name=$1
	row=$(manifest_artifact_row "$name") || die "radio-first manifest lacks artifact row: $name"
	relative=$(field "$row" 3)
	expected_bytes=$(field "$row" 4)
	expected_hash=$(field "$row" 5)
	origin=$(field "$row" 6)
	safe_relative_path "$relative"
	[ "$relative" = "$(artifact_expected_relative "$name")" ] || die "$name artifact path is not canonical: $relative"
	artifact_origin_allowed "$name" "$origin" || die "$name has an unreviewed source origin: $origin"
	file="$image_dir/$relative"
	require_file "$file" "$name artifact"
	[ "$(bytes_of "$file")" = "$expected_bytes" ] || die "$name size differs from its manifest"
	[ "$(sha256_of "$file")" = "$expected_hash" ] || die "$name hash differs from its manifest"
	printf '%s\t%s\t%s\n' "$file" "$expected_bytes" "$expected_hash"
}

verify_source_handoff() {
	row=$(awk -F "$tab" -v key="$1" '$1 == key { print; found=1; exit } END { if (!found) exit 1 }' "$manifest") || die "radio-first manifest lacks $1 provenance row"
	[ "$(field "$row" 2)" != '' ] || die "radio-first manifest $1 path is empty"
	path=$(field "$row" 2)
	case "$path" in
		/*) ;;
		*) safe_relative_path "$path"; path="$image_dir/$path" ;;
	esac
	bytes_expected=$(field "$row" 3)
	hash_expected=$(field "$row" 4)
	require_file "$path" "radio-first $1 handoff"
	[ "$(bytes_of "$path")" = "$bytes_expected" ] || die "radio-first $1 handoff size differs from its manifest"
	[ "$(sha256_of "$path")" = "$hash_expected" ] || die "radio-first $1 handoff hash differs from its manifest"
	printf '%s\n' "$path"
}

verify_partition_capacities() {
	row=$(awk -F "$tab" '$1 == "partition_capacities" { print; found=1; exit } END { if (!found) exit 1 }' "$manifest") || die 'radio-first manifest lacks partition-capacity provenance'
	capacity_file=$(field "$row" 2)
	case "$capacity_file" in
		/*) ;;
		*) safe_relative_path "$capacity_file"; capacity_file="$image_dir/$capacity_file" ;;
	esac
	require_file "$capacity_file" 'radio-first partition capacity handoff'
	[ "$(bytes_of "$capacity_file")" = "$(field "$row" 3)" ] || die 'radio-first partition capacity handoff size differs from its manifest'
	[ "$(sha256_of "$capacity_file")" = "$(field "$row" 4)" ] || die 'radio-first partition capacity handoff hash differs from its manifest'
	schema_count=0
	target_count=0
	count=0
	seen=
	while IFS="$tab" read -r kind value bytes extra; do
		case "$kind" in
			''|'#'*) continue ;;
			schema_version) [ "$value" = 1 ] && [ -z "${bytes:-}" ] && [ -z "${extra:-}" ] || die 'partition capacity schema row is malformed'; schema_count=$((schema_count + 1)) ;;
			target_id) [ "$value" = "$expected_target_id" ] && [ -z "${bytes:-}" ] && [ -z "${extra:-}" ] || die 'partition capacity target row is malformed'; target_count=$((target_count + 1)) ;;
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
	done <"$capacity_file"
	[ "$schema_count" -eq 1 ] || die 'partition capacity handoff must contain exactly one schema_version=1 row'
	[ "$target_count" -eq 1 ] || die 'partition capacity handoff must contain exactly one exact-target row'
	[ "$count" -eq 10 ] || die 'partition capacity handoff must contain all 10 split partitions'
}

verify_avb_bindings() {
	for name in $bound_artifacts; do
		row=$(manifest_binding_row "$name") || die "radio-first manifest lacks AVB binding row: $name"
		artifact=$(manifest_artifact_row "$name") || die "radio-first manifest lacks bound artifact: $name"
		[ "$(field "$row" 3)" = "$(field "$artifact" 3)" ] || die "AVB binding path differs from final $name image"
		[ "$(field "$row" 4)" = "$(field "$artifact" 4)" ] || die "AVB binding size differs from final $name image"
		[ "$(field "$row" 5)" = "$(field "$artifact" 5)" ] || die "AVB binding hash differs from final $name image"
		safe_relative_path "$(field "$row" 3)"
	done
}

verify_manifest() {
	check_route
	check_layout
	check_manifest_shape
	image_dir=$(CDPATH= cd -- "$(dirname -- "$manifest")" && pwd)
	emit boot_manifest "$manifest"
	emit image_dir "$image_dir"
	emit route_id "$(manifest_value route_id)"
	emit manifest_status "$(manifest_value status)"
	emit manifest_bootable "$(manifest_value bootable)"
	emit avb_mode "$(manifest_value avb_mode)"
	emit avb_algorithm "$avb_algorithm"
	for name in $all_artifacts; do
		verify_artifact "$name" >/dev/null
	done
	# Do not let an otherwise hash-valid hand-edited manifest smuggle an
	# unreviewed artifact into the route. The exact fourteen names are the
	# complete source-built split contract; every row must be one of them.
	while IFS="$tab" read -r kind name _rest; do
		[ "$kind" = artifact ] || continue
		case " $all_artifacts " in
			*" $name "*) ;;
			*) die "radio-first manifest contains an unknown artifact: $name" ;;
		esac
	done <"$manifest"
	for name in $bound_artifacts; do
		manifest_binding_row "$name" >/dev/null || die "radio-first manifest lacks AVB binding row: $name"
	done
	verify_avb_bindings
	verify_partition_capacities
	kernel_handoff=$(verify_source_handoff kernel_manifest)
	rootfs_handoff_manifest=$(verify_source_handoff rootfs_manifest)
	verify_source_handoff boot_inputs_manifest >/dev/null
	verify_source_handoff vendor_kernel_boot_input >/dev/null
	# The handoff manifests must retain the selected native Linux ABI and the
	# pre-observation status. This prevents a hand-edited split manifest from
	# turning an Android or already-claimed boot into the radio route.
	for handoff in "$kernel_handoff" "$rootfs_handoff_manifest"; do
		require_file "$handoff" 'radio-first source handoff'
		[ "$(awk -F "$tab" '$1 == "schema_version" { print $2; exit }' "$handoff")" = 1 ] || die 'radio-first source handoff schema is not 1'
		[ "$(awk -F "$tab" '$1 == "target_id" { print $2; exit }' "$handoff")" = "$expected_target_id" ] || die 'radio-first source handoff target is not exact Pixel 9a'
		[ "$(awk -F "$tab" '$1 == "kernel_backend" { print $2; exit }' "$handoff")" = "$expected_backend" ] || die 'radio-first source handoff backend is not linux-google-tegu-wwan'
		[ "$(awk -F "$tab" '$1 == "bootable" { print $2; exit }' "$handoff")" = false ] || die 'radio-first source handoff is not bootable=false'
	done
	[ "$(awk -F "$tab" '$1 == "status" { print $2; exit }' "$kernel_handoff")" = built-not-booted ] || die 'radio-first kernel handoff status is not built-not-booted'
	[ "$(awk -F "$tab" '$1 == "status" { print $2; exit }' "$rootfs_handoff_manifest")" = built-substrate-only ] || die 'radio-first rootfs handoff status is not built-substrate-only'
}

pin() { awk -F "$tab" -v wanted="$1" -v number="$2" '$1 == wanted { print $number; exit }' "$pins"; }

verify_recovery() {
	require_file "$pins" 'recovery pins'
	filename=$(pin factory 4)
	expected_hash=$(pin factory 6)
	expected_bytes=$(pin factory 7)
	[ -n "$filename" ] && [ -n "$expected_hash" ] && [ -n "$expected_bytes" ] || die "malformed factory recovery pin in $pins"
	recovery_path="$recovery_dir/$filename"
	require_file "$recovery_path" 'verified factory recovery artifact'
	actual_bytes=$(bytes_of "$recovery_path")
	actual_hash=$(sha256_of "$recovery_path")
	[ "$actual_bytes" = "$expected_bytes" ] || die "factory recovery size mismatch: expected $expected_bytes, got $actual_bytes"
	[ "$actual_hash" = "$expected_hash" ] || die "factory recovery SHA-256 mismatch: expected $expected_hash, got $actual_hash"
	emit recovery_artifact "$recovery_path sha256=$actual_hash bytes=$actual_bytes"
}

partition_inventory_value() { awk -F '=' -v key="$2" '$1 == key { sub("^[^=]*=", ""); print; exit }' "$1"; }

target_manifest_value() {
	awk -F '=' -v key="$1" '$1 { left=$1; gsub(/[[:space:]]/, "", left) } left == key { value=$2; sub(/^[[:space:]]*/, "", value); gsub(/[[:space:]\"]/, "", value); print value; exit }' "$target_manifest"
}

normalize_slot() {
	case "$1" in a|_a) printf 'a\n' ;; b|_b) printf 'b\n' ;; *) die "unexpected slot value: $1" ;; esac
}

require_partition_inventory() {
	inventory=${FERRIX_PIXEL9A_PARTITION_INVENTORY:-}
	[ -n "$inventory" ] || die 'rootfs flash requires FERRIX_PIXEL9A_PARTITION_INVENTORY pointing to a redacted read-only ADB partition inventory transcript'
	require_file "$inventory" 'partition inventory transcript'
	grep -q '^# Serial numbers are replaced by a SHA-256 prefix\.' "$inventory" || die 'partition inventory transcript is not the checked-in read-only inventory format'
	[ "$(partition_inventory_value "$inventory" result)" = ok ] || die 'partition inventory did not complete with result=ok'
	[ "$(partition_inventory_value "$inventory" target_id)" = "$expected_target_id" ] || die 'partition inventory target_id is not the exact Pixel 9a target'
	[ "$(partition_inventory_value "$inventory" transport)" = adb ] || die 'rootfs flashing requires a read-only ADB inventory from the exact target'
	[ "$(partition_inventory_value "$inventory" serial_sha256_prefix)" = "$(sha256_prefix "$fastboot_serial")" ] || die 'partition inventory was captured from a different device'
	[ "$(partition_inventory_value "$inventory" product)" = "$expected_product" ] || die 'partition inventory product is not tegu'
	[ "$(partition_inventory_value "$inventory" hardware_revision)" = "$expected_revision" ] || die 'partition inventory hardware revision is not MP1.0'
	[ "$(partition_inventory_value "$inventory" sku)" = "$expected_sku" ] || die 'partition inventory SKU is not GXQ96'
	[ "$(partition_inventory_value "$inventory" slot_count)" = 2 ] || die 'partition inventory does not report exactly two slots'
	[ "$(partition_inventory_value "$inventory" inventory_completeness)" = complete ] || die 'partition inventory is partial'
	inventory_slot=$(partition_inventory_value "$inventory" current_slot)
	[ "$(normalize_slot "$inventory_slot")" = "$current_slot" ] || die 'partition inventory current slot differs from the live Fastboot identity'
	sed -n '/^partitions=/,/^block_devices=/p' "$inventory" | grep -q userdata || die 'partition inventory does not list the allow-listed userdata partition'
	grep -q '^mounts=' "$inventory" || die 'partition inventory is missing the read-only mount table'
	grep -q '^cmdline=' "$inventory" || die 'partition inventory is missing the read-only kernel command line'
	emit partition_inventory "$inventory"
	emit partition_inventory_gate "passed target=$expected_target_id transport=adb userdata=present"
}

resolve_rootfs_stage() {
	rootfs_row=$(manifest_artifact_row rootfs_image) || die 'radio-first manifest lacks rootfs_image'
	rootfs_verified=$(verify_artifact rootfs_image)
	rootfs_image=$(field "$rootfs_verified" 1)
	rootfs_hash=$(field "$rootfs_verified" 3)
	rootfs_bytes=$(field "$rootfs_verified" 2)
	require_file "$deviceinfo" 'target deviceinfo'
	rootfs_partition=$(sed -n 's/^deviceinfo_flash_fastboot_partition_rootfs="\([^"]*\)"$/\1/p' "$deviceinfo" | head -1)
	[ -n "$rootfs_partition" ] || die 'deviceinfo does not name a rootfs partition'
	allowed=no
	for candidate in $rootfs_partition_allowlist; do [ "$rootfs_partition" = "$candidate" ] && allowed=yes; done
	[ "$allowed" = yes ] || die "rootfs partition '$rootfs_partition' is outside this workflow's allow-list ($rootfs_partition_allowlist)"
	require_partition_inventory
}

read_backup_gate() {
	require_file "$target_manifest" 'target manifest'
	[ "$(target_manifest_value target_id)" = "$expected_target_id" ] || die 'target manifest target_id is not the exact Pixel 9a target'
	[ "$(target_manifest_value kind)" = physical ] || die 'target manifest kind is not physical'
	[ "$(target_manifest_value codename)" = "$expected_product" ] || die 'target manifest codename is not tegu'
	[ "$(target_manifest_value hardware_revision)" = "$expected_revision" ] || die 'target manifest hardware_revision is not MP1.0'
	gate_value() {
		key=$1
		awk -v wanted="$key" '
			/^\[gates\][[:space:]]*$/ { in_gates=1; next }
			/^\[/ { in_gates=0 }
			in_gates && $0 ~ "^[[:space:]]*" wanted "[[:space:]]*=" {
				value=$0; sub(/^[^=]*=[[:space:]]*/, "", value); gsub(/[[:space:]\"]/, "", value)
				print value; found=1; exit
			}
			END { if (!found) exit 1 }
		' "$target_manifest"
	}
	identity_gate=$(gate_value identity) || die 'target manifest has no [gates].identity entry'
	image_compatibility_gate=$(gate_value image_compatibility) || die 'target manifest has no [gates].image_compatibility entry'
	backup_gate=$(gate_value backup) || die 'target manifest has no [gates].backup entry'
	data_policy_gate=$(gate_value data_policy || true)
	recovery_procedure_gate=$(gate_value recovery_procedure) || die 'target manifest has no [gates].recovery_procedure entry'
	confirmation_gate=$(gate_value per_run_confirmation) || die 'target manifest has no [gates].per_run_confirmation entry'
	emit gate_identity "$identity_gate"
	emit gate_image_compatibility "$image_compatibility_gate"
	emit gate_recovery_procedure "$recovery_procedure_gate"
	emit backup_gate "$backup_gate"
	emit data_policy "$data_policy_gate"
	emit per_run_confirmation "$confirmation_gate"
}

require_ferrix_gates() {
	read_backup_gate
	[ "$identity_gate" = passed ] || die "identity gate is '$identity_gate'"
	[ "$confirmation_gate" = required ] || die "per-run confirmation gate is '$confirmation_gate'"
	pixel9a_data_safety_check "$backup_gate" "$data_policy_gate" || die "$pixel9a_data_safety_reason"
	emit data_safety_gate "$pixel9a_data_safety_status"
	case "$image_compatibility_gate" in passed|verified) ;; *) die "image compatibility gate is '$image_compatibility_gate'; exact radio-first artifacts are not approved for this target" ;; esac
	case "$recovery_procedure_gate" in tested|passed|verified) ;; *) die "recovery procedure gate is '$recovery_procedure_gate'; test the exact-target stock recovery path first" ;; esac
}

fastboot_getvar() {
	variable=$1
	fastboot -s "$fastboot_serial" getvar "$variable" 2>&1 | awk -v key="$variable" '
		index($0, "(bootloader) " key ":") { line=$0; sub(".*\\(bootloader\\) " key ":[[:space:]]*", "", line); print line; exit }
		index($0, key ":") { line=$0; sub(".*" key ":[[:space:]]*", "", line); print line; exit }
	'
}

# These safety reads must not accept a successful CLI exit with a remote
# FAILED response, duplicate values, or a similarly named partition's row.
layout_getvar() (
	layout_key=$1
	layout_output=$(fastboot -s "$fastboot_serial" getvar "$layout_key" 2>&1) ||
		die "live layout query failed: $layout_key"
	printf '%s\n' "$layout_output" | awk -v key="$layout_key" '
		/FAILED/ { failed=1 }
		{
			line=$0
			sub(/^[[:space:]]*/, "", line)
			sub(/^\(bootloader\)[[:space:]]*/, "", line)
			if (index(line, key ":") == 1) {
				value=substr(line, length(key)+2)
				gsub(/^[[:space:]]+|[[:space:]]+$/, "", value)
				count++
			}
		}
		END { if (failed || count != 1 || value == "") exit 1; print value }
	' || die "live layout query missing, failed, or ambiguous: $layout_key"
)

live_partition_capacity() (
	live_raw=$(layout_getvar "partition-size:$1") || exit 1
	# POSIX awk is already a workflow prerequisite. Bound conversion to exact
	# integer arithmetic rather than trusting shell-specific hexadecimal syntax.
	printf '%s\n' "$live_raw" | awk '
		{
			text=$0; base=10; value=0
			if (text ~ /^0[xX][0-9a-fA-F]+$/) { base=16; text=substr(text,3) }
			else if (text !~ /^[0-9]+$/) exit 1
			for (i=1; i<=length(text); i++) {
				digit=index("0123456789abcdef", tolower(substr(text,i,1)))-1
				value=value*base+digit
				if (value>9007199254740991) exit 1
			}
			if (value<=0 || value%4096!=0) exit 1
			printf "%.0f\n", value
		}
	' || die "live capacity is zero, malformed, or unaligned: $1"
)

check_live_kernel_layout() (
	[ "$slot" != "$current_slot" ] || die 'live layout check requires an explicitly selected inactive slot'
	for live_name in $physical_artifacts $dynamic_artifacts; do
		live_expected=$(awk -F "$tab" -v name="$live_name" '$1 == "partition" && $2 == name { print $3 }' "$capacity_file")
		live_image_bytes=$(field "$(manifest_artifact_row "$live_name")" 4)
		[ "$live_image_bytes" -le "$live_expected" ] || die "$live_name image exceeds the reviewed capacity"
		case "$live_name" in
			vendor_dlkm|system_dlkm)
				[ "$live_image_bytes" = "$live_expected" ] || die "$live_name image would resize its logical partition; allocation changes are not authorized"
				;;
		esac
		for live_suffix in a b; do
			live_bytes=$(live_partition_capacity "${live_name}_$live_suffix") || exit 1
			[ "$live_bytes" = "$live_expected" ] || die "live ${live_name}_$live_suffix capacity differs from the reviewed handoff; no allocation change is authorized"
			emit live_capacity "partition=${live_name}_$live_suffix bytes=$live_bytes"
		done
	done
	emit live_layout matched-existing-allocation
)

check_live_write_capacity() (
	live_write_partition=$1
	live_write_bytes=$2
	live_write_userspace=$3
	live_write_capacity=$(live_partition_capacity "$live_write_partition") || exit 1
	[ "$live_write_bytes" -le "$live_write_capacity" ] || die "$live_write_partition image exceeds its live capacity"
	if [ "$live_write_userspace" = no ] && [ "$live_write_partition" != "$rootfs_partition_allowlist" ]; then
		live_write_name=${live_write_partition%_*}
		live_reviewed=$(awk -F "$tab" -v name="$live_write_name" '$1 == "partition" && $2 == name { print $3 }' "$capacity_file")
		[ "$live_write_capacity" = "$live_reviewed" ] || die "$live_write_partition capacity changed before write"
	fi
	if [ "$live_write_userspace" = yes ]; then
		[ "$(layout_getvar "is-logical:$live_write_partition")" = yes ] || die "$live_write_partition is not a verified logical partition"
		[ "$(layout_getvar snapshot-update-status)" = none ] || die 'snapshot update status is not none; shared storage writes are forbidden'
		[ "$live_write_bytes" = "$live_write_capacity" ] || die "$live_write_partition flash would resize shared storage; no allocation change is authorized"
	fi
)

read_transports() {
	adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
	fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
	adb_count=$(count_rows "$adb_rows")
	fastboot_count=$(count_rows "$fastboot_rows")
}

read_identity() {
	read_transports
	[ "$adb_count" -eq 0 ] || die "expected zero ADB devices, found $adb_count; put the exact device in Fastboot and disconnect others"
	[ "$fastboot_count" -eq 1 ] || die "expected exactly one Fastboot device, found $fastboot_count; refusing ambiguous or absent transports"
	fastboot_serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
	[ -n "$fastboot_serial" ] || die 'Fastboot serial is empty'
	serial=$fastboot_serial
	product=$(fastboot_getvar product)
	variant=$(fastboot_getvar variant)
	hardware_revision=$(fastboot_getvar hw-revision)
	unlocked=$(fastboot_getvar unlocked)
	secure=$(fastboot_getvar secure)
	current_slot_raw=$(fastboot_getvar current-slot)
	slot_count=$(fastboot_getvar slot-count)
	is_userspace=$(fastboot_getvar is-userspace)
	serialno=$(fastboot_getvar serialno)
	[ "$product" = "$expected_product" ] || die "Fastboot product mismatch: '$product' is not $expected_product"
	serial_prefix=$(sha256_prefix "$fastboot_serial")
	if ! pixel9a_fastboot_sku_gate "$variant" "$serial_prefix"; then
		case "$variant" in
			N/A|NA|unknown|unavailable|'') die "Fastboot SKU/variant '$variant' is unavailable and no matching redacted ADB identity proves $expected_sku" ;;
			*) die "Fastboot SKU/variant mismatch: '$variant' is not $expected_sku" ;;
		esac
	fi
	[ "$hardware_revision" = "$expected_revision" ] || die "Fastboot hardware revision mismatch: '$hardware_revision' is not $expected_revision"
	[ "$unlocked" = yes ] || die "Fastboot reports unlocked='$unlocked'; a locked bootloader cannot boot or flash Ferrix images"
	case "$secure" in yes|no) ;; *) die "Fastboot secure state is malformed or unavailable: '$secure'" ;; esac
	case "$current_slot_raw" in a|b|_a|_b) ;; *) die "unexpected Fastboot current slot: '$current_slot_raw'" ;; esac
	[ "$slot_count" = 2 ] || die "Fastboot slot-count is '$slot_count', not 2"
	case "$is_userspace" in yes|no) ;; *) die "Fastboot is-userspace is '$is_userspace'; expected yes or no" ;; esac
	[ -z "$serialno" ] || [ "$serialno" = "$fastboot_serial" ] || die 'Fastboot getvar serialno does not match the enumerated serial'
	# Only a fully verified bootloader identity in this process may anchor a
	# subsequent Fastbootd placeholder. An old transcript alone is not enough.
	if [ "$is_userspace" = no ]; then verified_bootloader_serial_prefix=$serial_prefix; fi
	current_slot=$(normalize_slot "$current_slot_raw")
	if [ -n "$slot_override" ]; then slot=$slot_override; slot_source=--slot; else slot=$current_slot; slot_source=current-slot; fi
	emit transport fastboot
	emit serial "$fastboot_serial"
	emit serial_sha256_prefix "$(sha256_prefix "$fastboot_serial")"
	emit product "$product"
	emit variant "$variant"
	emit sku "$expected_sku"
	emit sku_source "$pixel9a_sku_source"
	emit sku_evidence "$pixel9a_sku_evidence"
	emit hardware_revision "$hardware_revision"
	emit current_slot "$current_slot_raw"
	emit slot_count "$slot_count"
	emit unlocked "$unlocked"
	emit secure "$secure"
	emit is_userspace "$is_userspace"
	emit target_slot "$slot"
	if [ "$slot" = "$current_slot" ]; then emit slot_warning 'target slot equals the active slot'; else emit slot_warning 'target slot is inactive; this workflow never issues set_active'; fi
}

check_avb_gate() {
	emit avb_acknowledged "$allow_unsigned_avb"
	[ "$avb_algorithm" = NONE ] || return 0
	[ "$unlocked" = yes ] || die 'fresh-generated AVB algorithm NONE requires unlocked=yes'
	if [ "$secure" = yes ] && [ "$allow_unsigned_avb" != yes ]; then
		die 'fresh-generated AVB algorithm NONE on secure=yes requires --allow-unsigned-avb; relocking would make these images unbootable'
	fi
}

print_plan() {
	for name in $physical_artifacts $dynamic_artifacts; do
		case "$name" in vendor_dlkm|system_dlkm) via=fastbootd ;; *) via=bootloader ;; esac
		emit plan "artifact=$name partition_target=${name}_$slot via=$via input_image_sha256=$(field "$(manifest_artifact_row "$name")" 5) bytes=$(field "$(manifest_artifact_row "$name")" 4)"
	done
	emit plan "artifact=rootfs_handoff partition_target=none (rootfs.tar is a handoff archive, not flashed) input_image_sha256=$(field "$(manifest_artifact_row rootfs_handoff)" 5) bytes=$(field "$(manifest_artifact_row rootfs_handoff)" 4)"
	case "$stage" in rootfs|all) resolve_rootfs_stage; emit plan "artifact=rootfs_image partition_target=$rootfs_partition via=bootloader input_image_sha256=$rootfs_hash bytes=$rootfs_bytes" ;; esac
}

evidence_value() { awk -F '=' -v key="$2" '$1 == key { sub("^[^=]*=", ""); print; exit }' "$1"; }

check_temp_boot_gate() {
	emit policy_temporary_boot_before_persistent_write true
	if [ "$skip_temp_boot_gate" = yes ]; then emit temp_boot_gate 'SKIPPED by --skip-temp-boot-gate; typed phrase must name the skip'; return 0; fi
	check_temp_boot_transcript
	pixel9a_verify_temp_observation "${FERRIX_PIXEL9A_RADIO_TEMP_BOOT_OBSERVATION:-}" "$evidence" "$boot_hash" "$(sha256_prefix "$fastboot_serial")"
	emit temp_boot_gate "passed evidence=$evidence observation=$FERRIX_PIXEL9A_RADIO_TEMP_BOOT_OBSERVATION"
}

check_temp_boot_transcript() {
	evidence=${FERRIX_PIXEL9A_RADIO_TEMP_BOOT_EVIDENCE:-}
	[ -n "$evidence" ] || die 'radio-first image-layout policy requires temporary boot evidence before a persistent write'
	require_file "$evidence" 'temporary-boot evidence file'
	[ "$(evidence_value "$evidence" operation)" = pixel-9a-radio-boot-temp ] || die 'temporary-boot evidence is not a radio-first boot-temp transcript'
	[ "$(evidence_value "$evidence" target_id)" = "$expected_target_id" ] || die 'temporary-boot evidence names a different target'
	boot_hash=$(field "$(manifest_artifact_row boot)" 5)
	[ "$(evidence_value "$evidence" boot_img_sha256)" = "$boot_hash" ] || die 'temporary-boot evidence records a different radio-first boot.img hash'
	[ "$(evidence_value "$evidence" boot_exit_status)" = 0 ] || die 'temporary-boot evidence records a failed boot command'
	[ "$(evidence_value "$evidence" result)" = temporary-boot-issued ] || die 'temporary-boot evidence does not record result=temporary-boot-issued'
	[ "$(evidence_value "$evidence" serial_sha256_prefix)" = "$(sha256_prefix "$fastboot_serial")" ] || die 'temporary-boot evidence was recorded on a different device'
}

preflight() {
	check_layout
	verify_manifest
	verify_recovery
	read_backup_gate
	read_identity
	check_avb_gate
	print_plan
	if [ "$mode" = flash ]; then
		[ -n "$slot_override" ] || die 'persistent radio-first flash requires an explicit --slot a|b'
		[ "$slot" != "$current_slot" ] || die "persistent flash refuses the active slot '$slot'; choose the inactive slot explicitly"
		[ "$is_userspace" = no ] || die 'device is in fastbootd; start persistent flashing from the bootloader'
		check_temp_boot_gate
		emit stage "$stage"
	elif [ "$mode" = boot-temp ] || [ "$mode" = observe-temp ]; then
		[ "$is_userspace" = no ] || die 'temporary boot requires the bootloader Fastboot mode'
	fi
	case "$mode:$stage" in
		preflight:kernel|preflight:all|flash:kernel|flash:all)
			if [ -n "$slot_override" ]; then
				check_live_kernel_layout
			else
				emit live_layout not-checked-no-explicit-inactive-slot
			fi
			;;
	esac
	emit preflight passed
}

confirm_phrase() {
	phrase=$1
	printf '%s\n' "$2" >&2
	printf 'Type exactly: %s\n> ' "$phrase" >&2
	IFS= read -r confirmation || die 'confirmation was not provided; no mutation performed'
	[ "$confirmation" = "$phrase" ] || die 'confirmation did not match; no mutation performed'
	emit confirmed "$phrase"
}

recheck_identity() {
	expected_userspace=${1:-no}
	verify_recovery
	read_transports
	[ "$adb_count" -eq 0 ] && [ "$fastboot_count" -eq 1 ] || die "transport changed before mutation: adb_devices=$adb_count fastboot_devices=$fastboot_count"
	recheck_serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
	[ "$recheck_serial" = "$fastboot_serial" ] || die 'Fastboot serial changed before mutation'
	[ "$(fastboot_getvar product)" = "$expected_product" ] || die 'Fastboot product changed before mutation'
	recheck_variant=$(fastboot_getvar variant)
	[ "$(fastboot_getvar hw-revision)" = "$expected_revision" ] || die 'Fastboot hardware revision changed before mutation'
	[ "$(fastboot_getvar unlocked)" = yes ] || die 'Fastboot unlocked state changed before mutation'
	recheck_serialno=$(fastboot_getvar serialno)
	[ -z "$recheck_serialno" ] || [ "$recheck_serialno" = "$fastboot_serial" ] || die 'Fastboot serialno changed before mutation'
	[ "$(fastboot_getvar secure)" = "$secure" ] || die 'Fastboot secure state changed before mutation'
	[ "$(fastboot_getvar is-userspace)" = "$expected_userspace" ] || die "Fastboot is-userspace differs from expected $expected_userspace"
	[ "$(normalize_slot "$(fastboot_getvar current-slot)")" = "$current_slot" ] || die 'Fastboot current slot changed before mutation'
	[ "$(fastboot_getvar slot-count)" = 2 ] || die 'Fastboot slot-count changed before mutation'
	if [ "$recheck_variant" = 'MSM USF' ]; then
		# Google's Fastbootd HAL returns a placeholder, not a different SKU.
		# This route-local exception needs both the verified mode/identity above
		# and this run's bootloader anchor plus complete same-serial ADB evidence.
		[ "$expected_userspace" = yes ] || die 'Fastbootd placeholder is forbidden in bootloader mode'
		[ "$verified_bootloader_serial_prefix" = "$(sha256_prefix "$fastboot_serial")" ] || die 'Fastbootd placeholder requires same-session verified bootloader identity'
		radio_sku_evidence=$(pixel9a_adb_correlation_file "$verified_bootloader_serial_prefix" || true)
		[ -n "$radio_sku_evidence" ] || die 'Fastbootd placeholder requires complete same-device ADB identity proving GXQ96'
		emit fastbootd_observed_variant "$recheck_variant"
		emit fastbootd_sku_source same-session-bootloader-and-adb-correlated
		emit fastbootd_sku_evidence "$radio_sku_evidence"
	else
		pixel9a_fastboot_sku_gate "$recheck_variant" "$(sha256_prefix "$fastboot_serial")" || die 'Fastboot SKU/variant changed before mutation and no matching ADB correlation is available'
	fi
}

wait_for_same_serial() {
	attempt=0
	while [ "$attempt" -lt "$wait_seconds" ]; do
		read_transports
		[ "$fastboot_count" -le 1 ] || die "ambiguous Fastboot transports while waiting: $fastboot_count"
		if [ "$fastboot_count" -eq 1 ]; then
			[ "$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')" = "$fastboot_serial" ] || die 'a different Fastboot serial appeared while waiting'
			return 0
		fi
		attempt=$((attempt + 1)); sleep 1
	done
	die "the exact device did not return to Fastboot within $wait_seconds seconds"
}

wait_for_fastboot_gone() {
	attempt=0
	while [ "$attempt" -lt "$wait_seconds" ]; do
		read_transports
		[ "$fastboot_count" -eq 0 ] && return 0
		attempt=$((attempt + 1)); sleep 1
	done
	return 1
}

boot_temp() {
	boot_image="$image_dir/$(artifact_expected_relative boot)"
	boot_hash=$(field "$(manifest_artifact_row boot)" 5)
	boot_bytes=$(field "$(manifest_artifact_row boot)" 4)
	emit boot_img "$boot_image"
	emit boot_img_sha256 "$boot_hash"
	emit limitation 'header v4 temporary boot uses vendor_boot, init_boot, and dtbo currently flashed on the device; this is not a full Linux userspace boot'
	[ "$(bytes_of "$boot_image")" = "$boot_bytes" ] || die 'boot.img changed after manifest verification'
	[ "$(sha256_of "$boot_image")" = "$boot_hash" ] || die 'boot.img hash changed after manifest verification'
	emit mutation_input_image "partition_target=boot (temporary) input_image_sha256=$boot_hash bytes=$boot_bytes"
	short=$(printf '%s' "$boot_hash" | cut -c1-8)
	confirm_phrase "CONFIRM PIXEL 9A RADIO TEMPORARY BOOT boot.img@$short" 'The next command sends the source-built radio-first boot.img to the exact device. No partition is written.'
	recheck_identity no
	emit mutation "fastboot -s $fastboot_serial boot $boot_image"
	boot_status=0
	fastboot -s "$fastboot_serial" boot "$boot_image" || boot_status=$?
	emit boot_exit_status "$boot_status"
	[ "$boot_status" -eq 0 ] || fail temporary-boot-failed "the boot command returned status $boot_status"
	if wait_for_fastboot_gone; then emit device_left_fastboot yes; else emit device_left_fastboot 'no (still enumerated after wait)'; fi
	emit result temporary-boot-issued
	printf 'capture and review kernel behavior, then run pixel-9a-observe-temp-boot; this command transcript alone does not authorize persistent writes:\n%s\n' "$transcript" >&2
}

observe_temp() {
	check_temp_boot_transcript
	observation_input=${FERRIX_PIXEL9A_RADIO_TEMP_BOOT_KERNEL_LOG:-}
	pixel9a_check_kernel_observation_log "$observation_input"
	input_hash=$(sha256_of "$observation_input")
	boot_transcript_hash=$(sha256_of "$evidence")
	emit observation_input_sha256 "$input_hash"
	emit observation_scope 'kernel progress only; no Ferrix userspace or radio capability claim'
	short=$(printf '%s' "$boot_hash" | cut -c1-8)
	confirm_phrase "CONFIRM PIXEL 9A KERNEL PROGRESS OBSERVED boot.img@$short" 'Confirm this redacted capture is from the displayed device and temporary boot, that kernel progress was observed, and that no hang, panic, or reboot loop occurred. This only records evidence.'
	recheck_identity no
	[ "$(sha256_of "$observation_input")" = "$input_hash" ] || die 'kernel capture changed during observation review'
	[ "$(sha256_of "$evidence")" = "$boot_transcript_hash" ] || die 'temporary-boot transcript changed during observation review'
	observation_log="${transcript%.txt}-kernel.txt"
	redact_literal "$fastboot_serial" "serial-sha256:$(sha256_prefix "$fastboot_serial")" <"$observation_input" >"$observation_log"
	emit boot_img_sha256 "$boot_hash"
	emit boot_transcript_sha256 "$boot_transcript_hash"
	emit kernel_log "${observation_log##*/}"
	emit kernel_log_bytes "$(bytes_of "$observation_log")"
	emit kernel_log_sha256 "$(sha256_of "$observation_log")"
	emit result temporary-boot-observed
	printf 'observation (serial redacted): %s\n' "$transcript" >&2
}

flash_partition() {
	partition=$1; image=$2; hash=$3; label=$4; flash_expected_bytes=$5; expected_userspace=${6:-no}
	require_file "$image" 'flash image'
	[ "$(bytes_of "$image")" = "$flash_expected_bytes" ] || die "$partition image size changed after manifest verification"
	[ "$(sha256_of "$image")" = "$hash" ] || die "$partition image hash changed after manifest verification"
	emit mutation_input_image "partition_target=$partition input_image_sha256=$hash bytes=$flash_expected_bytes"
	recheck_identity "$expected_userspace"
	check_live_write_capacity "$partition" "$flash_expected_bytes" "$expected_userspace"
	emit mutation "fastboot -s $fastboot_serial flash $partition $image"
	flash_status=0
	fastboot -s "$fastboot_serial" flash "$partition" "$image" || flash_status=$?
	emit flashed "partition=$partition slot=$label sha256=$hash status=$flash_status"
	[ "$flash_status" -eq 0 ] || fail flash-failed "flash of $partition returned status $flash_status; restore with the verified factory archive"
}

enter_fastbootd() {
	recheck_identity no
	emit mutation "fastboot -s $fastboot_serial reboot fastboot"
	reboot_status=0
	fastboot -s "$fastboot_serial" reboot fastboot || reboot_status=$?
	[ "$reboot_status" -eq 0 ] || fail fastbootd-entry-failed 'reboot to fastbootd failed'
	wait_for_same_serial
	is_userspace=$(fastboot_getvar is-userspace)
	emit is_userspace "$is_userspace"
	[ "$is_userspace" = yes ] || fail fastbootd-entry-failed 'device returned without is-userspace=yes'
}

return_to_bootloader() {
	recheck_identity yes
	emit mutation "fastboot -s $fastboot_serial reboot bootloader"
	reboot_status=0
	fastboot -s "$fastboot_serial" reboot bootloader || reboot_status=$?
	[ "$reboot_status" -eq 0 ] || fail bootloader-return-failed 'reboot to bootloader failed'
	wait_for_same_serial
	is_userspace=$(fastboot_getvar is-userspace)
	emit is_userspace "$is_userspace"
	[ "$is_userspace" = no ] || fail bootloader-return-failed 'device returned without is-userspace=no'
}

flash_kernel_stage() {
	phrase="CONFIRM PIXEL 9A RADIO FLASH KERNEL STAGE TO SLOT $(printf '%s' "$slot" | tr a-z A-Z)"
	[ "$skip_temp_boot_gate" != yes ] || phrase="$phrase WITHOUT TEMP BOOT GATE"
	confirm_phrase "$phrase" 'The next commands first visit fastbootd to validate existing logical allocations and snapshot state, return to the bootloader, then write only the inactive radio-first kernel split. No allocation-size change is permitted.'
	check_temp_boot_gate
	recheck_identity no
	check_live_kernel_layout
	# Validate the Fastbootd identity and logical write conditions before any
	# image is written. In particular, an unavailable SKU or snapshot state
	# must not strand a partially written inactive boot set. This transport
	# round trip is covered by the per-run confirmation above.
	enter_fastbootd
	recheck_identity yes
	for name in $dynamic_artifacts; do
		row=$(manifest_artifact_row "$name")
		check_live_write_capacity "${name}_$slot" "$(field "$row" 4)" yes
	done
	return_to_bootloader
	recheck_identity no
	check_live_kernel_layout
	for name in $physical_artifacts; do
		row=$(manifest_artifact_row "$name")
		flash_partition "${name}_$slot" "$image_dir/$(field "$row" 3)" "$(field "$row" 5)" "$slot" "$(field "$row" 4)" no
	done
	enter_fastbootd
	for name in $dynamic_artifacts; do
		row=$(manifest_artifact_row "$name")
		flash_partition "${name}_$slot" "$image_dir/$(field "$row" 3)" "$(field "$row" 5)" "$slot" "$(field "$row" 4)" yes
	done
	return_to_bootloader
	emit kernel_stage complete
}

flash_rootfs_stage() {
	emit rootfs_stage_partitions "$rootfs_partition"
	confirm_phrase "CONFIRM PIXEL 9A RADIO FLASH ROOTFS STAGE TO PARTITION $(printf '%s' "$rootfs_partition" | tr a-z A-Z)" "The next command overwrites $rootfs_partition on the exact device with the source-built radio-first rootfs image."
	check_temp_boot_gate
	flash_partition "$rootfs_partition" "$rootfs_image" "$rootfs_hash" none "$rootfs_bytes" no
	emit rootfs_stage complete
}

flash() {
	case "$stage" in kernel) flash_kernel_stage ;; rootfs) flash_rootfs_stage ;; all) flash_kernel_stage; flash_rootfs_stage ;; esac
	emit result flashed-not-booted
	emit reminder 'bootable evidence still requires an observed Linux boot and radio inventory; the manifest remains bootable=false until then'
	printf 'transcript (serial redacted): %s\n' "$transcript" >&2
}

parse_args "$@"
need adb
need fastboot
need awk
need cut
need date
need grep
need head
need mkdir
need sed
need tr
need wc
command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || die 'missing sha256sum or shasum'
case "$out_root" in /*) ;; *) out_root="$PWD/$out_root"; evidence_dir="$out_root/pixel-9a/flash-radio" ;; esac
open_transcript
preflight
case "$mode" in
	preflight) printf 'transcript (serial redacted): %s\n' "$transcript" >&2 ;;
	boot-temp) require_ferrix_gates; boot_temp ;;
	observe-temp) observe_temp ;;
	flash) require_ferrix_gates; flash ;;
esac
