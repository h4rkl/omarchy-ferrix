#!/bin/sh
set -eu

# Non-destructive, target-scoped Android -> bootloader Fastboot transition for
# development. This is the only checked-in workflow allowed to issue the
# transport-changing adb reboot bootloader for a normal dev session. It never
# enters Fastbootd, unlocks, flashes, boots an image, changes slots, or writes
# storage.

expected_product=tegu
expected_model='Pixel 9a'
expected_manufacturer=Google
expected_revision=MP1.0
expected_sku=GXQ96
wait_seconds=${FERRIX_PIXEL9A_FASTBOOT_WAIT_SECONDS:-30}
mode=
adb_serial=
fastboot_serial=
current_transport=

usage() {
	cat >&2 <<'EOF'
usage:
  tools/pixel-9a/enter-fastboot.sh --preflight
  tools/pixel-9a/enter-fastboot.sh --confirm-enter

--preflight       read-only exact-target transport and identity checks
--confirm-enter   require one typed confirmation, then issue adb reboot bootloader
EOF
	exit 2
}

die() {
	printf 'REFUSING: %s\n' "$*" >&2
	exit 1
}

need() {
	command -v "$1" >/dev/null 2>&1 || die "missing $1; install Android Platform Tools explicitly"
}

count_rows() {
	printf '%s\n' "$1" | awk 'NF { count++ } END { print count + 0 }'
}

adb_prop() {
	adb -s "$adb_serial" shell getprop "$1" | tr -d '\r' | sed -n '1p'
}

fastboot_getvar() {
	variable=$1
	fastboot -s "$fastboot_serial" getvar "$variable" 2>&1 | awk -v wanted="$variable" '
		index($0, "(bootloader) " wanted ":") {
			value=$0
			sub(".*\\(bootloader\\) " wanted ":[[:space:]]*", "", value)
			print value
			exit
		}
		index($0, wanted ":") {
			value=$0
			sub(".*" wanted ":[[:space:]]*", "", value)
			print value
			exit
		}
	'
}

read_transports() {
	adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
	fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
	adb_count=$(count_rows "$adb_rows")
	fastboot_count=$(count_rows "$fastboot_rows")
}

validate_adb_identity() {
	adb_serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1; exit }')
	adb_state=$(printf '%s\n' "$adb_rows" | awk '{ print $2; exit }')
	[ "$adb_state" = device ] || die "ADB device state is '$adb_state'; unlock the screen and accept USB debugging"
	[ "$(adb_prop ro.product.manufacturer)" = "$expected_manufacturer" ] || die 'ADB manufacturer is not Google'
	[ "$(adb_prop ro.product.model)" = "$expected_model" ] || die 'ADB model is not Pixel 9a'
	[ "$(adb_prop ro.product.device)" = "$expected_product" ] || die 'ADB codename is not tegu'
	[ "$(adb_prop ro.boot.hardware.revision)" = "$expected_revision" ] || die 'ADB hardware revision is not MP1.0'
	[ "$(adb_prop ro.boot.hardware.sku)" = "$expected_sku" ] || die 'ADB SKU is not GXQ96'
	current_slot=$(adb_prop ro.boot.slot_suffix)
	case "$current_slot" in
		''|a|b|_a|_b) ;;
		*) die "ADB current slot is unexpected: $current_slot" ;;
	esac
	printf 'transport=adb\n'
	printf 'serial=%s\n' "$adb_serial"
	printf 'manufacturer=%s\n' "$expected_manufacturer"
	printf 'model=%s\n' "$expected_model"
	printf 'product=%s\n' "$expected_product"
	printf 'variant=%s\n' "$expected_sku"
	printf 'hardware_revision=%s\n' "$expected_revision"
	printf 'current_slot=%s\n' "${current_slot:-unknown}"
	printf 'partition_target=none (transport transition only)\n'
	printf 'input_image_sha256=none\n'
}

validate_fastboot_identity() {
	fastboot_serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1; exit }')
	[ -n "$fastboot_serial" ] || die 'Fastboot serial is empty'
	[ "$(fastboot_getvar product)" = "$expected_product" ] || die 'Fastboot product is not tegu'
	fastboot_variant=$(fastboot_getvar variant)
	case "$fastboot_variant" in
		"$expected_sku"|N/A|'') ;;
		*) die "Fastboot SKU/variant is '$fastboot_variant', not $expected_sku or unavailable" ;;
	esac
	[ "$(fastboot_getvar hw-revision)" = "$expected_revision" ] || die 'Fastboot hardware revision is not MP1.0'
	current_slot=$(fastboot_getvar current-slot)
	case "$current_slot" in
		a|b|_a|_b) ;;
		*) die "Fastboot current slot is unexpected: $current_slot" ;;
	esac
	slot_count=$(fastboot_getvar slot-count)
	[ "$slot_count" = 2 ] || die "Fastboot slot-count is '$slot_count', not 2"
	is_userspace=$(fastboot_getvar is-userspace)
	[ "$is_userspace" = no ] || die "device is in Fastbootd (is-userspace=$is_userspace), not bootloader Fastboot"
	printf 'transport=fastboot\n'
	printf 'serial=%s\n' "$fastboot_serial"
	printf 'product=%s\n' "$expected_product"
	printf 'variant=%s\n' "${fastboot_variant:-N/A}"
	printf 'target_sku=%s\n' "$expected_sku"
	[ "${fastboot_variant:-N/A}" = "$expected_sku" ] || printf 'sku_gate=adb-correlated-or-fastboot-unavailable\n'
	printf 'hardware_revision=%s\n' "$expected_revision"
	printf 'current_slot=%s\n' "$current_slot"
	printf 'slot_count=%s\n' "$slot_count"
	printf 'is_userspace=%s\n' "$is_userspace"
	printf 'partition_target=none (transport transition only)\n'
	printf 'input_image_sha256=none\n'
}

preflight() {
	read_transports
	if [ "$adb_count" -eq 1 ] && [ "$fastboot_count" -eq 0 ]; then
		current_transport=adb
		validate_adb_identity
		printf 'preflight=passed\n'
		return 0
	fi
	if [ "$adb_count" -eq 0 ] && [ "$fastboot_count" -eq 1 ]; then
		current_transport=fastboot
		validate_fastboot_identity
		printf 'preflight=passed\n'
		return 0
	fi
	die "expected exactly one ADB device or one bootloader Fastboot device; adb=$adb_count fastboot=$fastboot_count"
}

recheck_adb() {
	read_transports
	[ "$adb_count" -eq 1 ] && [ "$fastboot_count" -eq 0 ] ||
		die "transport changed before reboot: adb=$adb_count fastboot=$fastboot_count"
	recheck_serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1; exit }')
	[ "$recheck_serial" = "$adb_serial" ] || die 'ADB serial changed before reboot'
	validate_adb_identity >/dev/null
}

wait_for_fastboot() {
	attempt=0
	while [ "$attempt" -lt "$wait_seconds" ]; do
		read_transports
		if [ "$adb_count" -eq 0 ] && [ "$fastboot_count" -eq 1 ]; then
			candidate=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1; exit }')
			[ "$candidate" = "$adb_serial" ] || die 'a different Fastboot serial appeared after reboot'
			fastboot_serial=$candidate
			return 0
		fi
		[ "$adb_count" -le 1 ] && [ "$fastboot_count" -le 1 ] ||
			die "ambiguous transports while waiting: adb=$adb_count fastboot=$fastboot_count"
		attempt=$((attempt + 1))
		sleep 1
	done
	die "exact device did not enter bootloader Fastboot within ${wait_seconds}s"
}

confirm_enter() {
	phrase='CONFIRM PIXEL 9A ENTER FASTBOOT'
	printf '%s\n' 'The next command reboots the exact Pixel 9a from Android into bootloader Fastboot.' >&2
	printf '%s\n' 'No partition, slot, bootloader, or user data is changed.' >&2
	printf 'Type exactly: %s\n> ' "$phrase" >&2
	IFS= read -r confirmation || die 'confirmation was not provided; no reboot performed'
	[ "$confirmation" = "$phrase" ] || die 'confirmation did not match; no reboot performed'
}

[ "$#" -eq 1 ] || usage
case "$1" in
	--preflight) mode=preflight ;;
	--confirm-enter) mode=enter ;;
	*) usage ;;
esac

need adb
need fastboot
need awk
need date
need sed
need sleep
need tr

preflight
[ "$mode" = preflight ] && exit 0

if [ "$current_transport" = fastboot ]; then
	printf 'result=already-in-fastboot\n'
	exit 0
fi

confirm_enter
recheck_adb
printf 'mutation=adb reboot bootloader\n'
adb -s "$adb_serial" reboot bootloader
wait_for_fastboot
read_transports
validate_fastboot_identity
printf 'result=entered-fastboot\n'
