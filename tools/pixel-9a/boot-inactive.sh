#!/bin/sh
set -eu

# Target-scoped activation/boot workflow for the separately reviewed
# radio-first Pixel 9a split route.  Persistent flashing deliberately leaves
# the inactive slot unselected; this is the only checked-in workflow that may
# select that already-flashed slot and request a reboot.
#
# Device verbs used by this file, and nothing else:
#   adb devices -l
#   fastboot devices -l
#   fastboot -s <serial> getvar <variable>
#   fastboot -s <serial> set_active <inactive-slot>
#   fastboot -s <serial> reboot
# It never flashes, erases, formats, unlocks/locks, enters Fastbootd, or
# accepts a raw set_active command outside this target workflow.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
. "$root/tools/pixel-9a/data-safety.sh"
. "$root/tools/pixel-9a/adb-correlation.sh"
. "$root/tools/pixel-9a/temp-boot-observation.sh"

target=${FERRIX_PIXEL9A_TARGET_DIR:-"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"}
pins=${FERRIX_PIXEL9A_PINS:-"$target/recovery.pins"}
recovery_dir="$target/recovery"
target_manifest="$target/manifest.toml"
out_root=${FERRIX_OUT_DIR:-"$root/out"}
manifest=${FERRIX_PIXEL9A_RADIO_SPLIT_MANIFEST:-"$out_root/pixel-9a/images/tegu-wwan-split/boot-manifest.tsv"}
flash_evidence=${FERRIX_PIXEL9A_RADIO_FLASH_EVIDENCE:-}
temp_evidence=${FERRIX_PIXEL9A_RADIO_TEMP_BOOT_EVIDENCE:-}
evidence_dir="$out_root/pixel-9a/boot-inactive"
wait_seconds=${FERRIX_PIXEL9A_BOOT_WAIT_SECONDS:-90}
tab=$(printf '\t')

expected_target_id=google-pixel-9a-tegu-gxq96-mp1-0
expected_product=tegu
expected_sku=GXQ96
expected_revision=MP1.0
expected_route=experimental-linux-google-tegu-s5300-wwan
expected_backend=linux-google-tegu-wwan
all_artifacts='Image dtb.img boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system rootfs_handoff rootfs_image'
flashed_artifacts='boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system'

mode=
slot_override=
allow_unsigned_avb=no
serial=
fastboot_serial=
current_slot=
original_current_slot=
slot=
transcript=

usage() {
	cat >&2 <<'EOF'
usage:
  tools/pixel-9a/boot-inactive.sh --preflight --slot a|b [--allow-unsigned-avb]
  tools/pixel-9a/boot-inactive.sh --confirm-boot --slot a|b [--allow-unsigned-avb]

preflight    read-only manifest, evidence, recovery, gate, and device checks
confirm-boot repeat those checks, require one typed confirmation, select the
             already-flashed inactive slot through this workflow, and reboot
             the exact device
EOF
	exit 2
}

need() {
	command -v "$1" >/dev/null 2>&1 || {
		printf 'missing %s; install Android Platform Tools explicitly\n' "$1" >&2
		exit 1
	}
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

redact_literal() {
	[ -n "$1" ] || { cat; return 0; }
	REDACT_NEEDLE=$1 REDACT_REPLACEMENT=$2 awk '
		BEGIN { needle = ENVIRON["REDACT_NEEDLE"]; replacement = ENVIRON["REDACT_REPLACEMENT"] }
		{
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

die() {
	printf 'REFUSING pixel-9a inactive-slot boot: %s\n' "$*" >&2
	if [ -n "$transcript" ]; then
		message=$(printf '%s\n' "$*" | redact_literal "$serial" "serial-sha256:$(sha256_prefix "${serial:-none}")")
		printf 'refused=%s\n' "$message" >>"$transcript"
	fi
	exit 1
}

parse_args() {
	[ "$#" -ge 1 ] || usage
	mode=$1
	shift
	case "$mode" in
		--preflight|preflight) mode=preflight ;;
		--confirm-boot|confirm-boot) mode=confirm-boot ;;
		*) usage ;;
	esac
	while [ "$#" -gt 0 ]; do
		case "$1" in
			--slot)
				[ "$#" -ge 2 ] || usage
				case "$2" in a|b) slot_override=$2 ;; *) usage ;; esac
				shift 2
				;;
			--allow-unsigned-avb) allow_unsigned_avb=yes; shift ;;
			*) usage ;;
		esac
	done
	[ -n "$slot_override" ] || die 'an explicit inactive --slot a|b is required'
}

field() { printf '%s\n' "$1" | awk -F "$tab" -v number="$2" '{ print $number }'; }

manifest_value() {
	awk -F "$tab" -v key="$1" '$1 == key { print $2; exit }' "$manifest"
}

manifest_artifact_row() {
	awk -F "$tab" -v name="$1" '$1 == "artifact" && $2 == name { print; found=1; exit } END { if (!found) exit 1 }' "$manifest"
}

artifact_relative() {
	case "$1" in
		Image) printf '%s\n' Image ;;
		dtb.img) printf '%s\n' dtb.img ;;
		rootfs_handoff) printf '%s\n' rootfs.tar ;;
		rootfs_image) printf '%s\n' rootfs.img ;;
		*) printf '%s.img\n' "$1" ;;
	esac
}

safe_relative_path() {
	case "$1" in
		''|/*|.|..|../*|*/../*|*/..|*' '*|*"$tab"*) die "manifest contains an unsafe artifact path: $1" ;;
	esac
}

require_file() {
	[ -f "$1" ] || die "$2 is missing: $1"
	[ ! -L "$1" ] || die "$2 must not be a symlink: $1"
}

verify_manifest() {
	require_file "$manifest" 'radio-first split manifest'
	[ "$(manifest_value schema_version)" = 1 ] || die 'radio-first split manifest schema_version is not 1'
	[ "$(manifest_value target_id)" = "$expected_target_id" ] || die 'radio-first split manifest target_id is not the exact Pixel 9a target'
	[ "$(manifest_value codename)" = "$expected_product" ] || die 'radio-first split manifest codename is not tegu'
	[ "$(manifest_value hardware_revision)" = "$expected_revision" ] || die 'radio-first split manifest hardware_revision is not MP1.0'
	[ "$(manifest_value route_id)" = "$expected_route" ] || die 'radio-first split manifest route_id is not the reviewed S5300/WWAN route'
	[ "$(manifest_value kernel_backend)" = "$expected_backend" ] || die 'radio-first split manifest kernel_backend is not linux-google-tegu-wwan'
	[ "$(manifest_value status)" = composed-not-booted ] || die 'radio-first split manifest is not composed-not-booted'
	[ "$(manifest_value bootable)" = false ] || die 'radio-first split manifest must remain bootable=false before observed Linux evidence'
	[ "$(manifest_value avb_mode)" = fresh-generated ] || die 'radio-first split manifest does not record fresh-generated AVB metadata'
	[ "$(manifest_value avb_descriptor_inputs)" = fresh-avbtool-hash-footers-from-final-images ] || die 'radio-first split manifest lacks final-image AVB provenance'
	image_dir=$(CDPATH= cd -- "$(dirname -- "$manifest")" && pwd)
	artifact_count=$(awk -F "$tab" '$1 == "artifact" { count++ } END { print count + 0 }' "$manifest")
	[ "$artifact_count" -eq 14 ] || die "radio-first split manifest must contain 14 artifacts, found $artifact_count"
	for name in $all_artifacts; do
		row=$(manifest_artifact_row "$name") || die "radio-first split manifest lacks artifact: $name"
		relative=$(field "$row" 3)
		expected_bytes=$(field "$row" 4)
		expected_hash=$(field "$row" 5)
		safe_relative_path "$relative"
		[ "$relative" = "$(artifact_relative "$name")" ] || die "$name artifact path is not canonical: $relative"
		case "$expected_bytes" in ''|*[!0-9]*) die "$name artifact byte count is malformed" ;; esac
		case "$expected_hash" in ''|*[!0-9a-f]*) die "$name artifact hash is malformed" ;; esac
		file="$image_dir/$relative"
		require_file "$file" "$name artifact"
		[ "$(bytes_of "$file")" = "$expected_bytes" ] || die "$name artifact size differs from the reviewed manifest"
		[ "$(sha256_of "$file")" = "$expected_hash" ] || die "$name artifact hash differs from the reviewed manifest"
	done
}

pin() { awk -F '\t' -v wanted="$1" -v number="$2" '$1 == wanted { print $number; exit }' "$pins"; }

verify_recovery() {
	require_file "$pins" 'recovery pins'
	for kind in factory ota; do
		filename=$(pin "$kind" 4)
		expected_hash=$(pin "$kind" 6)
		expected_bytes=$(pin "$kind" 7)
		[ -n "$filename" ] && [ -n "$expected_hash" ] && [ -n "$expected_bytes" ] || die "malformed $kind recovery pin"
		path="$recovery_dir/$filename"
		require_file "$path" "$kind recovery artifact"
		actual_bytes=$(bytes_of "$path")
		actual_hash=$(sha256_of "$path")
		[ "$actual_bytes" = "$expected_bytes" ] || die "$kind recovery size mismatch"
		[ "$actual_hash" = "$expected_hash" ] || die "$kind recovery SHA-256 mismatch"
		emit "recovery_${kind}_artifact" "$path sha256=$actual_hash bytes=$actual_bytes"
	done
}

target_manifest_value() {
	awk -F '=' -v key="$1" '$1 { left=$1; gsub(/[[:space:]]/, "", left) } left == key { value=$2; sub(/^[[:space:]]*/, "", value); gsub(/[[:space:]\"]/, "", value); print value; exit }' "$target_manifest"
}

read_gates() {
	require_file "$target_manifest" 'target manifest'
	[ "$(target_manifest_value target_id)" = "$expected_target_id" ] || die 'target manifest target_id is not the exact Pixel 9a target'
	[ "$(target_manifest_value kind)" = physical ] || die 'target manifest kind is not physical'
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
	identity_gate=$(gate_value identity) || die 'target manifest has no identity gate'
	image_compatibility_gate=$(gate_value image_compatibility) || die 'target manifest has no image_compatibility gate'
	backup_gate=$(gate_value backup) || die 'target manifest has no backup gate'
	data_policy_gate=$(gate_value data_policy || true)
	recovery_procedure_gate=$(gate_value recovery_procedure) || die 'target manifest has no recovery_procedure gate'
	confirmation_gate=$(gate_value per_run_confirmation) || die 'target manifest has no per_run_confirmation gate'
	emit gate_identity "$identity_gate"
	emit gate_image_compatibility "$image_compatibility_gate"
	emit backup_gate "$backup_gate"
	emit data_policy "$data_policy_gate"
	emit gate_recovery_procedure "$recovery_procedure_gate"
	emit per_run_confirmation "$confirmation_gate"
}

evidence_value() { awk -F '=' -v key="$2" '$1 == key { sub("^[^=]*=", ""); print; exit }' "$1"; }

verify_temp_evidence() {
	require_file "$temp_evidence" 'temporary-boot evidence'
	[ "$(evidence_value "$temp_evidence" operation)" = pixel-9a-radio-boot-temp ] || die 'temporary-boot evidence is not the radio-first transcript'
	[ "$(evidence_value "$temp_evidence" target_id)" = "$expected_target_id" ] || die 'temporary-boot evidence names a different target'
	[ "$(evidence_value "$temp_evidence" boot_exit_status)" = 0 ] || die 'temporary-boot evidence records a failed boot command'
	[ "$(evidence_value "$temp_evidence" result)" = temporary-boot-issued ] || die 'temporary-boot evidence does not record result=temporary-boot-issued'
	[ "$(evidence_value "$temp_evidence" serial_sha256_prefix)" = "$(sha256_prefix "$fastboot_serial")" ] || die 'temporary-boot evidence was recorded on a different device'
	boot_hash=$(field "$(manifest_artifact_row boot)" 5)
	[ "$(evidence_value "$temp_evidence" boot_img_sha256)" = "$boot_hash" ] || die 'temporary-boot evidence names a different boot image'
	pixel9a_verify_temp_observation "${FERRIX_PIXEL9A_RADIO_TEMP_BOOT_OBSERVATION:-}" "$temp_evidence" "$boot_hash" "$(sha256_prefix "$fastboot_serial")"
	emit temporary_boot_evidence "$temp_evidence"
}

verify_flash_evidence() {
	require_file "$flash_evidence" 'persistent-flash evidence'
	[ "$(evidence_value "$flash_evidence" operation)" = pixel-9a-flash ] || die 'persistent-flash evidence is not the canonical radio flash transcript'
	[ "$(evidence_value "$flash_evidence" target_id)" = "$expected_target_id" ] || die 'persistent-flash evidence names a different target'
	[ "$(evidence_value "$flash_evidence" result)" = flashed-not-booted ] || die 'persistent-flash evidence is not a flashed-not-booted result'
	[ "$(evidence_value "$flash_evidence" kernel_stage)" = complete ] || die 'persistent-flash evidence does not contain a complete kernel stage'
	[ "$(evidence_value "$flash_evidence" serial_sha256_prefix)" = "$(sha256_prefix "$fastboot_serial")" ] || die 'persistent-flash evidence was recorded on a different device'
	evidence_target_slot=$(evidence_value "$flash_evidence" target_slot)
	[ "$evidence_target_slot" = "$slot" ] || die 'persistent-flash evidence names a different target slot'
	evidence_current_slot=$(evidence_value "$flash_evidence" current_slot)
	case "$evidence_current_slot" in a|b|_a|_b) ;; *) die 'persistent-flash evidence has no valid current slot' ;; esac
	[ "$(normalize_slot "$evidence_current_slot")" = "$original_current_slot" ] || die 'persistent-flash evidence current slot differs from the live inactive-slot base'
	[ "$evidence_current_slot" != "$evidence_target_slot" ] || die 'persistent-flash evidence claims the active slot was flashed'
	for name in $flashed_artifacts; do
		row=$(manifest_artifact_row "$name")
		expected_hash=$(field "$row" 5)
		partition="${name}_$slot"
		grep -Eq "^flashed=partition=${partition} slot=${slot} sha256=${expected_hash} status=0$" "$flash_evidence" ||
			die "persistent-flash evidence lacks the reviewed successful write for $partition"
	done
	emit persistent_flash_evidence "$flash_evidence"
}

normalize_slot() {
	case "$1" in a|_a) printf 'a\n' ;; b|_b) printf 'b\n' ;; *) die "unexpected slot value: $1" ;; esac
}

fastboot_getvar() {
	variable=$1
	fastboot -s "$fastboot_serial" getvar "$variable" 2>&1 | awk -v key="$variable" '
		index($0, "(bootloader) " key ":") { line=$0; sub(".*\\(bootloader\\) " key ":[[:space:]]*", "", line); print line; exit }
		index($0, key ":") { line=$0; sub(".*" key ":[[:space:]]*", "", line); print line; exit }
	'
}

read_transports() {
	adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
	fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
	adb_count=$(count_rows "$adb_rows")
	fastboot_count=$(count_rows "$fastboot_rows")
}

read_identity() {
	read_transports
	[ "$adb_count" -eq 0 ] || die "expected zero ADB devices, found $adb_count; the activation workflow requires ordinary bootloader Fastboot"
	[ "$fastboot_count" -eq 1 ] || die "expected exactly one Fastboot device, found $fastboot_count"
	fastboot_serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
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
	[ "$unlocked" = yes ] || die "Fastboot reports unlocked='$unlocked'; inactive-slot activation requires an unlocked bootloader"
	case "$secure" in yes|no) ;; *) die "Fastboot secure state is malformed: '$secure'" ;; esac
	case "$current_slot_raw" in a|b|_a|_b) ;; *) die "unexpected Fastboot current slot: '$current_slot_raw'" ;; esac
	[ "$slot_count" = 2 ] || die "Fastboot slot-count is '$slot_count', not 2"
	[ "$is_userspace" = no ] || die 'inactive-slot activation requires ordinary bootloader Fastboot, not Fastbootd'
	[ -z "$serialno" ] || [ "$serialno" = "$fastboot_serial" ] || die 'Fastboot serialno does not match the enumerated serial'
	current_slot=$(normalize_slot "$current_slot_raw")
	original_current_slot=$current_slot
	slot=$slot_override
	[ "$slot" != "$current_slot" ] || die "requested slot '$slot' is active; this workflow only selects the inactive slot"
	emit transport fastboot
	emit serial "$fastboot_serial"
	emit serial_sha256_prefix "$serial_prefix"
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
	emit slot_warning 'target slot is inactive; only this checked-in workflow may select it'
}

check_avb_gate() {
	avb_mode=$(manifest_value avb_algorithm)
	emit avb_algorithm "$avb_mode"
	emit avb_acknowledged "$allow_unsigned_avb"
	if [ "$avb_mode" = NONE ] && [ "$secure" = yes ] && [ "$allow_unsigned_avb" != yes ]; then
		die 'fresh-generated AVB algorithm NONE on secure=yes requires --allow-unsigned-avb'
	fi
}

print_plan() {
	emit recovery_plan 'factory and OTA archives re-hashed immediately before slot activation'
	for name in $flashed_artifacts; do
		row=$(manifest_artifact_row "$name")
		emit plan "partition_target=${name}_$slot input_image_sha256=$(field "$row" 5) bytes=$(field "$row" 4)"
	done
	emit plan 'partition_target=inactive-slot-selector input_image_sha256=verified-radio-flash-evidence'
	emit plan 'post_selection=fastboot reboot (boot observation remains separate evidence)'
}

recheck_identity() {
	read_transports
	[ "$adb_count" -eq 0 ] && [ "$fastboot_count" -eq 1 ] || die "transport changed before slot mutation: adb=$adb_count fastboot=$fastboot_count"
	recheck_serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
	[ "$recheck_serial" = "$fastboot_serial" ] || die 'Fastboot serial changed before slot mutation'
	[ "$(fastboot_getvar product)" = "$expected_product" ] || die 'Fastboot product changed before slot mutation'
	recheck_variant=$(fastboot_getvar variant)
	pixel9a_fastboot_sku_gate "$recheck_variant" "$(sha256_prefix "$fastboot_serial")" || die 'Fastboot SKU/variant changed before slot mutation and no matching ADB correlation is available'
	[ "$(fastboot_getvar hw-revision)" = "$expected_revision" ] || die 'Fastboot hardware revision changed before slot mutation'
	[ "$(fastboot_getvar unlocked)" = yes ] || die 'Fastboot unlocked state changed before slot mutation'
	[ "$(fastboot_getvar secure)" = "$secure" ] || die 'Fastboot secure state changed before slot mutation'
	[ "$(fastboot_getvar is-userspace)" = no ] || die 'Fastboot entered Fastbootd before slot mutation'
	[ "$(normalize_slot "$(fastboot_getvar current-slot)")" = "$original_current_slot" ] || die 'Fastboot current slot changed before slot mutation'
	[ "$(fastboot_getvar slot-count)" = 2 ] || die 'Fastboot slot-count changed before slot mutation'
	recheck_serialno=$(fastboot_getvar serialno)
	[ -z "$recheck_serialno" ] || [ "$recheck_serialno" = "$fastboot_serial" ] || die 'Fastboot serialno changed before slot mutation'
}

recheck_selected_slot() {
	read_transports
	[ "$adb_count" -eq 0 ] && [ "$fastboot_count" -eq 1 ] || die 'transport changed after selecting the inactive slot'
	[ "$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')" = "$fastboot_serial" ] || die 'Fastboot serial changed after selecting the inactive slot'
	[ "$(fastboot_getvar product)" = "$expected_product" ] || die 'Fastboot product changed after selecting the inactive slot'
	[ "$(fastboot_getvar hw-revision)" = "$expected_revision" ] || die 'Fastboot hardware revision changed after selecting the inactive slot'
	[ "$(fastboot_getvar unlocked)" = yes ] || die 'Fastboot unlocked state changed after selecting the inactive slot'
	[ "$(fastboot_getvar secure)" = "$secure" ] || die 'Fastboot secure state changed after selecting the inactive slot'
	[ "$(normalize_slot "$(fastboot_getvar current-slot)")" = "$slot" ] || die 'Fastboot did not report the requested inactive slot after set_active'
	[ "$(fastboot_getvar slot-count)" = 2 ] || die 'Fastboot slot-count changed after selecting the inactive slot'
	selected_serialno=$(fastboot_getvar serialno)
	[ -z "$selected_serialno" ] || [ "$selected_serialno" = "$fastboot_serial" ] || die 'Fastboot serialno changed after selecting the inactive slot'
	[ "$(fastboot_getvar is-userspace)" = no ] || die 'Fastboot entered Fastbootd after selecting the inactive slot'
	emit post_set_active_current_slot "$slot"
}

confirm_phrase() {
	phrase="CONFIRM PIXEL 9A ACTIVATE INACTIVE SLOT $(printf '%s' "$slot" | tr a-z A-Z) AND BOOT"
	printf '%s\n' 'The next commands select the already-flashed inactive slot and reboot the exact Pixel 9a.' >&2
	printf '%s\n' 'No partition is written. If the boot fails, return to ordinary Fastboot and use the verified recovery route; do not relock.' >&2
	printf 'Type exactly: %s\n> ' "$phrase" >&2
	IFS= read -r confirmation || die 'confirmation was not provided; no slot change performed'
	[ "$confirmation" = "$phrase" ] || die 'confirmation did not match; no slot change performed'
	emit confirmed "$phrase"
}

wait_for_boot_departure() {
	attempt=0
	while [ "$attempt" -lt "$wait_seconds" ]; do
		read_transports
		[ "$fastboot_count" -le 1 ] || die "ambiguous transports after reboot: fastboot=$fastboot_count"
		[ "$adb_count" -le 1 ] || die "ambiguous transports after reboot: adb=$adb_count"
		if [ "$fastboot_count" -eq 0 ]; then
			if [ "$adb_count" -eq 1 ]; then
				candidate=$(printf '%s\n' "$adb_rows" | awk '{ print $1 }')
				[ "$candidate" = "$fastboot_serial" ] || die 'a different ADB serial appeared after reboot'
				candidate_state=$(printf '%s\n' "$adb_rows" | awk '{ print $2 }')
				[ "$candidate_state" = device ] || die "exact device appeared in unexpected ADB state: $candidate_state"
				emit post_reboot_transport adb
			else
				emit post_reboot_transport not-observed
			fi
			return 0
		fi
		attempt=$((attempt + 1))
		sleep 1
	done
	die "the exact device did not leave bootloader Fastboot within ${wait_seconds}s"
}

preflight() {
	verify_manifest
	verify_recovery
	read_gates
	[ "$identity_gate" = passed ] || die "identity gate is '$identity_gate'"
	[ "$confirmation_gate" = required ] || die "per-run confirmation gate is '$confirmation_gate'"
	case "$image_compatibility_gate" in passed|verified) ;; *) die "image compatibility gate is '$image_compatibility_gate'; exact split artifacts are not approved" ;; esac
	case "$recovery_procedure_gate" in tested|passed|verified) ;; *) die "recovery procedure gate is '$recovery_procedure_gate'; test exact stock recovery first" ;; esac
	read_identity
	verify_temp_evidence
	verify_flash_evidence
	check_avb_gate
	print_plan
	emit preflight passed
}

confirm_boot() {
	pixel9a_data_safety_check "$backup_gate" "$data_policy_gate" || die "$pixel9a_data_safety_reason"
	emit data_safety_gate "$pixel9a_data_safety_status"
	confirm_phrase
	verify_recovery
	verify_manifest
	verify_temp_evidence
	verify_flash_evidence
	recheck_identity
	print_plan
	emit mutation_identity "serial_sha256_prefix=$(sha256_prefix "$fastboot_serial") product=$expected_product hardware_revision=$expected_revision current_slot=$original_current_slot target_slot=$slot partition_target=inactive-slot-selector recovery_factory_and_ota_rehashed=true"
	emit mutation "fastboot -s $fastboot_serial set_active $slot"
	set_active_status=0
	fastboot -s "$fastboot_serial" set_active "$slot" || set_active_status=$?
	emit set_active_exit_status "$set_active_status"
	[ "$set_active_status" -eq 0 ] || die "Fastboot set_active returned status $set_active_status; the slot may still be unchanged"
	recheck_selected_slot
	emit mutation "fastboot -s $fastboot_serial reboot"
	reboot_status=0
	fastboot -s "$fastboot_serial" reboot || reboot_status=$?
	emit reboot_exit_status "$reboot_status"
	[ "$reboot_status" -eq 0 ] || die "Fastboot reboot returned status $reboot_status after selecting slot $slot"
	wait_for_boot_departure
	emit result slot-activated-boot-requested
	emit reminder 'This transcript requests a boot; it is not Linux, display, service, Wi-Fi, Bluetooth, or modem evidence'
	printf 'observe the exact device through the checked-in read-only evidence workflows; transcript (serial redacted): %s\n' "$transcript" >&2
}

parse_args "$@"
need adb
need awk
need cut
need date
need fastboot
need grep
need mkdir
need sleep
need tr
need wc
command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || die 'missing sha256sum or shasum'

case "$out_root" in
	/*) ;;
	*) out_root="$PWD/$out_root"; evidence_dir="$out_root/pixel-9a/boot-inactive" ;;
esac
mkdir -p "$evidence_dir"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
transcript="$evidence_dir/$mode-$stamp-$$.txt"
{
	printf '# Pixel 9a inactive-slot boot transcript, %s UTC, host %s\n' "$stamp" "$(uname -s) $(uname -m)"
	printf '# Serial numbers are replaced by a SHA-256 prefix. Never commit this file.\n'
} >"$transcript"
emit operation pixel-9a-boot-inactive
emit target_id "$expected_target_id"
preflight
[ "$mode" = preflight ] && { printf 'transcript=%s\n' "$transcript" >&2; exit 0; }
require_file "$flash_evidence" 'persistent-flash evidence'
pixel9a_data_safety_check "$backup_gate" "$data_policy_gate" || die "$pixel9a_data_safety_reason"
emit data_safety_gate "$pixel9a_data_safety_status"
confirm_boot
