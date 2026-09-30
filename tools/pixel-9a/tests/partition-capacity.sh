#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
workflow="$root/tools/pixel-9a/partition-capacity.sh"
[ -x "$workflow" ] || exit 1
sh -n "$workflow"

# Only reject direct device-changing command forms; explanatory text is not a
# command and should not create a false positive.
if grep -nE '(^|[;&|[:space:]])(adb|fastboot)[[:space:]].*(reboot|sideload|shell[[:space:]]+(rm|dd|mkfs|setprop|su)|flash|erase|format|set_active|boot)' "$workflow" >/dev/null; then
	echo 'partition capacity workflow contains a mutating device command' >&2
	exit 1
fi

work=$(mktemp -d "/tmp/pixel-9a-partition-capacity.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/bin" "$work/out"

cat >"$work/bin/adb" <<'STUB'
#!/bin/sh
case "$1" in
devices)
	echo 'List of devices attached'
	devices=$(printenv STUB_ADB_DEVICES)
	if [ "${STUB_ADB_DEVICES_EMPTY:-0}" != 1 ] && [ -z "$devices" ]; then
		devices='SERIAL-A device usb:1'
	fi
	printf '%s\n' "$devices" | tr '|' '\n'
	;;
-s)
	[ "$3" = shell ] || exit 99
	if [ "$4" = getprop ]; then
		case "$5" in
		ro.product.device) echo tegu ;;
		ro.boot.hardware.revision) echo MP1.0 ;;
		ro.boot.hardware.sku) echo GXQ96 ;;
		ro.boot.slot_suffix) echo _a ;;
		*) exit 99 ;;
		esac
	elif [ "$4" = ls ] && [ "$5" = -1 ]; then
		printf '%s\n' boot_a boot_b init_boot_a init_boot_b vendor_boot_a vendor_boot_b vendor_kernel_boot_a vendor_kernel_boot_b dtbo_a dtbo_b vbmeta_a vbmeta_b vbmeta_vendor_a vbmeta_vendor_b vbmeta_system_a vbmeta_system_b
	elif [ "$4" = readlink ] && [ "$5" = -f ]; then
		path=$6
		missing=$(printenv STUB_MISSING_DYNAMIC_B)
		case "$path" in
		/dev/block/by-name/boot_a) echo /dev/block/sda13 ;;
		/dev/block/by-name/boot_b) echo /dev/block/sda23 ;;
		/dev/block/by-name/init_boot_a) echo /dev/block/sda14 ;;
		/dev/block/by-name/init_boot_b) echo /dev/block/sda24 ;;
		/dev/block/by-name/vendor_boot_a) echo /dev/block/sda15 ;;
		/dev/block/by-name/vendor_boot_b) echo /dev/block/sda25 ;;
		/dev/block/by-name/vendor_kernel_boot_a) echo /dev/block/sda16 ;;
		/dev/block/by-name/vendor_kernel_boot_b) echo /dev/block/sda26 ;;
		/dev/block/by-name/dtbo_a) echo /dev/block/sda17 ;;
		/dev/block/by-name/dtbo_b) echo /dev/block/sda27 ;;
		/dev/block/by-name/vbmeta_a) echo /dev/block/sda18 ;;
		/dev/block/by-name/vbmeta_b) echo /dev/block/sda28 ;;
		/dev/block/by-name/vbmeta_vendor_a) echo /dev/block/sda20 ;;
		/dev/block/by-name/vbmeta_vendor_b) echo /dev/block/sda30 ;;
		/dev/block/by-name/vbmeta_system_a) echo /dev/block/sda19 ;;
		/dev/block/by-name/vbmeta_system_b) echo /dev/block/sda29 ;;
		/dev/block/mapper/vendor_dlkm_a) echo /dev/block/dm-12 ;;
		/dev/block/mapper/vendor_dlkm_b) [ "$missing" = 1 ] && exit 13 || echo /dev/block/dm-22 ;;
		/dev/block/mapper/vendor_dlkm) echo /dev/block/dm-12 ;;
		/dev/block/mapper/system_dlkm_a) echo /dev/block/dm-8 ;;
		/dev/block/mapper/system_dlkm_b) [ "$missing" = 1 ] && exit 13 || echo /dev/block/dm-18 ;;
		/dev/block/mapper/system_dlkm) echo /dev/block/dm-8 ;;
		*) exit 13 ;;
		esac
	elif [ "$4" = cat ]; then
		path=$5
		case "$path" in
		/sys/class/block/*/size)
			[ "${STUB_DENY_SYSFS:-0}" = 1 ] && exit 13
			block=$(printf '%s\n' "$path" | sed 's#^/sys/class/block/##; s#/size$##')
			case "$block" in
			dm-8|dm-12|dm-18|dm-22) echo 16384 ;;
			sda13|sda23) echo 32768 ;;
			sda14|sda24) echo 16384 ;;
			sda15|sda25) echo 65536 ;;
			sda16|sda26) echo 32768 ;;
			sda17|sda27|sda18|sda28|sda19|sda29|sda20|sda30) echo 8192 ;;
			*) exit 13 ;;
			esac
			;;
		/sys/class/block/*/queue/logical_block_size) echo 512 ;;
		/proc/partitions) deny=$(printenv STUB_DENY_PROC); [ "$deny" = 1 ] && exit 13 || echo 'major minor #blocks name' ;;
		/proc/mounts) echo '/dev/block/dm-8 /system_dlkm ext4 ro 0 0'; echo '/dev/block/dm-12 /vendor_dlkm ext4 ro 0 0' ;;
		/proc/cmdline) deny=$(printenv STUB_DENY_PROC); [ "$deny" = 1 ] && exit 13 || echo 'root=PARTLABEL=Ferrix' ;;
		*) exit 99 ;;
		esac
	else
		exit 99
	fi
	;;
*) exit 99 ;;
esac
STUB

cat >"$work/bin/fastboot" <<'STUB'
#!/bin/sh
[ -z "${STUB_FASTBOOT_LOG:-}" ] || printf '%s\n' "$*" >>"$STUB_FASTBOOT_LOG"
device_serial=''
if [ "$1" = -s ]; then
	[ -n "${2:-}" ] || exit 98
	device_serial=$2
	shift 2
fi
case "$1" in
devices)
	devices=$(printenv STUB_FASTBOOT_DEVICES)
	printf '%s\n' "$devices" | tr '|' '\n'
	;;
getvar)
	variable=$2
	if [ "${STUB_FAIL_VENDOR_BOOT:-0}" = 1 ] && [ "$variable" = partition-size:vendor_boot_a ]; then
		printf "FAILED (remote: 'Could not open partition') device=%s\n" "$device_serial" >&2
		exit "${STUB_VENDOR_BOOT_EXIT:-1}"
	fi
	case "$variable" in
	product) value=${STUB_FASTBOOT_PRODUCT:-tegu} ;;
	hw-revision) value=${STUB_FASTBOOT_REVISION:-MP1.0} ;;
	variant) value=${STUB_FASTBOOT_VARIANT:-GXQ96} ;;
	current-slot) value=${STUB_FASTBOOT_SLOT:-a} ;;
	slot-count) value=${STUB_FASTBOOT_SLOT_COUNT:-2} ;;
	is-userspace) value=${STUB_FASTBOOT_IS_USERSPACE:-yes} ;;
	unlocked|secure) value=yes ;;
	super-partition-name) value=super ;;
	is-logical:*) value=yes ;;
	snapshot-update-status)
		[ -n "${STUB_SNAPSHOT_STATUS:-}" ] || exit 1
		value=$STUB_SNAPSHOT_STATUS
		;;
	partition-size:boot_a) value=0x01000000 ;;
	partition-size:boot_b) value=16777216 ;;
	partition-size:init_boot_a) value=0x00800000 ;;
	partition-size:init_boot_b) value=8388608 ;;
	partition-size:vendor_boot_a) value=0x02000000 ;;
	partition-size:vendor_boot_b) value=33554432 ;;
	partition-size:vendor_kernel_boot_a) value=0x01000000 ;;
	partition-size:vendor_kernel_boot_b) value=16777216 ;;
	partition-size:dtbo_a) value=0x00400000 ;;
	partition-size:dtbo_b) value=4194304 ;;
	partition-size:vbmeta_a) value=0x00400000 ;;
	partition-size:vbmeta_b) value=4194304 ;;
	partition-size:vbmeta_vendor_a) value=0x00400000 ;;
	partition-size:vbmeta_vendor_b) value=4194304 ;;
	partition-size:vbmeta_system_a) value=0x00400000 ;;
	partition-size:vbmeta_system_b) value=4194304 ;;
	partition-size:vendor_dlkm_a) value=0x00800000 ;;
	partition-size:vendor_dlkm_b)
		[ "${STUB_FASTBOOT_MISSING_DYNAMIC_B:-0}" = 1 ] && exit 1
		value=${STUB_FASTBOOT_DYNAMIC_B_SIZE:-8388608}
		;;
	partition-size:system_dlkm_a) value=0x00800000 ;;
	partition-size:system_dlkm_b) [ "${STUB_FASTBOOT_MISSING_DYNAMIC_B:-0}" = 1 ] && exit 1 || value=8388608 ;;
	*) exit 1 ;;
	esac
	if [ "${STUB_FASTBOOT_BAD_HEX:-0}" = 1 ] && [ "$variable" = partition-size:dtbo_a ]; then
		value=0xnot-hex
	fi
	printf '(bootloader) %s: %s\n' "$variable" "$value" >&2
	printf 'Finished. Total time: 0.000s\n' >&2
	;;
*) exit 99 ;;
esac
STUB
chmod +x "$work/bin/adb" "$work/bin/fastboot"
export PATH="$work/bin:$PATH" FERRIX_OUT_DIR="$work/out"

STUB_DENY_PROC=1 "$workflow" >"$work/pass.out" 2>"$work/pass.err"
handoff=$(awk -F= '$1 == "capacity_handoff" { print $2; exit }' "$work/pass.out")
latest=$(awk -F= '$1 == "capacity_handoff_latest" { print $2; exit }' "$work/pass.out")
[ -f "$handoff" ] && [ -f "$latest" ]
grep -q '^inventory_completeness=partial$' "$work/pass.out"
grep -q '^schema_version	1$' "$handoff"
grep -q '^target_id	google-pixel-9a-tegu-gxq96-mp1-0$' "$handoff"
[ "$(grep -c '^partition	' "$handoff")" -eq 10 ]
while IFS="$(printf '\t')" read -r kind name bytes; do
	[ "$kind" = partition ] || continue
	case "$bytes" in ''|*[!0-9]*) exit 1 ;; esac
	[ "$bytes" -gt 0 ]
	[ $((bytes % 4096)) -eq 0 ]
done <"$handoff"
! grep -Fq 'SERIAL-A' "$handoff"
generated="$work/out/pixel-9a"
! grep -R -Fq 'SERIAL-A' "$generated"
grep -q 'inventory_completeness=partial' "$generated"/partition-capacity-*.txt
grep -q 'vendor_dlkm_inactive_block=dm-22' "$generated"/partition-capacity-*.txt
grep -q 'system_dlkm_inactive_block=dm-18' "$generated"/partition-capacity-*.txt

if STUB_DENY_PROC=1 STUB_DENY_SYSFS=1 "$workflow" >"$work/denied-sysfs.out" 2>"$work/denied-sysfs.err"; then
	echo 'partition capacity unexpectedly passed with denied sysfs reads' >&2
	exit 1
fi
grep -q 'boot_a capacity is not readable from /sys/class/block/sda13/size' "$work/denied-sysfs.err"

if STUB_DENY_PROC=0 STUB_MISSING_DYNAMIC_B=1 "$workflow" >"$work/missing.out" 2>"$work/missing.err"; then
	echo 'partition capacity unexpectedly passed without inactive dynamic mapping' >&2
	exit 1
fi
grep -q 'vendor_dlkm_b is not independently resolvable' "$work/missing.err"

if STUB_ADB_DEVICES='SERIAL-A device usb:1|SERIAL-B device usb:2' "$workflow" >"$work/multi.out" 2>"$work/multi.err"; then
	echo 'partition capacity unexpectedly passed with multiple ADB devices' >&2
	exit 1
fi
grep -q 'requires exactly one ADB or one Fastboot device' "$work/multi.err"

# Fastbootd can expose logical partition metadata when Android does not expose
# the inactive dynamic mapper nodes. Exercise both hexadecimal and decimal
# getvar values, exact identity, and independent A/B dynamic rows.
if ! STUB_ADB_DEVICES_EMPTY=1 STUB_FASTBOOT_DEVICES='SERIAL-F fastboot usb:1' "$workflow" >"$work/fastboot.out" 2>"$work/fastboot.err"; then
	cat "$work/fastboot.err" >&2
	exit 1
fi
fastboot_handoff=$(awk -F= '$1 == "capacity_handoff" { print $2; exit }' "$work/fastboot.out")
[ -f "$fastboot_handoff" ]
grep -q '^inventory_completeness=partial$' "$work/fastboot.out"
grep -q '^# source=Fastboot getvar partition-size:' "$fastboot_handoff"
grep -q '^partition	vendor_dlkm	8388608$' "$fastboot_handoff"
[ "$(grep -c '^partition	' "$fastboot_handoff")" -eq 10 ]
! grep -R -Fq 'SERIAL-F' "$work/out/pixel-9a"
grep -q 'transport=fastboot' "$work/out/pixel-9a"/partition-capacity-*.txt
grep -q 'fastboot_userspace=yes' "$work/out/pixel-9a"/partition-capacity-*.txt

# Google's userspace HAL returns a placeholder rather than the board SKU.
# The capacity-only exception must still prove the exact same Android device.
if command -v shasum >/dev/null 2>&1; then
	fixture_prefix=$(printf '%s' SERIAL-F | shasum -a 256 | cut -c1-12)
else
	fixture_prefix=$(printf '%s' SERIAL-F | sha256sum | cut -c1-12)
fi
write_adb_identity() {
	mkdir -p "$1/pixel-9a"
	{
		printf 'transport=adb\nresult=ok\nserial_sha256_prefix=%s\n' "$2"
		printf 'ro.product.manufacturer=Google\nro.product.model=Pixel 9a\n'
		printf 'ro.product.device=tegu\nro.boot.hardware.revision=MP1.0\n'
		[ -z "$3" ] || printf 'ro.boot.hardware.sku=%s\n' "$3"
	} >"$1/pixel-9a/discovery-fixture.txt"
}
placeholder_out="$work/placeholder"
write_adb_identity "$placeholder_out" "$fixture_prefix" GXQ96
if ! FERRIX_OUT_DIR="$placeholder_out" STUB_ADB_DEVICES_EMPTY=1 \
	STUB_FASTBOOT_DEVICES='SERIAL-F fastboot usb:1' STUB_FASTBOOT_VARIANT='MSM USF' \
	"$workflow" >"$work/placeholder.out" 2>"$work/placeholder.err"; then
	cat "$work/placeholder.err" >&2
	exit 1
fi
grep -q '^sku=GXQ96$' "$placeholder_out"/pixel-9a/partition-capacity-*.txt
grep -q '^observed_variant=MSM USF$' "$placeholder_out"/pixel-9a/partition-capacity-*.txt
grep -q '^sku_source=adb-correlated-fastbootd-placeholder$' "$placeholder_out"/pixel-9a/partition-capacity-*.txt
[ "$(grep -c '^partition	' "$placeholder_out/pixel-9a/partition-capacities-latest.tsv")" -eq 10 ]
! grep -R -Fq SERIAL-F "$placeholder_out"

# Every refusal uses a fresh output root: no completed handoff may appear.
for refusal in missing-evidence wrong-serial wrong-sku incomplete-evidence bootloader unknown-variant wrong-product wrong-revision wrong-slot-count wrong-slot multiple-devices; do
	refusal_out="$work/$refusal"
	set --
	case "$refusal" in
		missing-evidence) ;;
		wrong-serial) write_adb_identity "$refusal_out" 000000000000 GXQ96 ;;
		wrong-sku) write_adb_identity "$refusal_out" "$fixture_prefix" WRONG ;;
		incomplete-evidence) write_adb_identity "$refusal_out" "$fixture_prefix" '' ;;
		*) write_adb_identity "$refusal_out" "$fixture_prefix" GXQ96 ;;
	esac
	expected='placeholder variant requires matching redacted ADB identity'
	case "$refusal" in
		bootloader) set -- STUB_FASTBOOT_IS_USERSPACE=no; expected='requires fastbootd' ;;
		unknown-variant) set -- STUB_FASTBOOT_VARIANT=WRONG; expected="SKU/variant 'WRONG'" ;;
		wrong-product) set -- STUB_FASTBOOT_PRODUCT=other; expected='product mismatch' ;;
		wrong-revision) set -- STUB_FASTBOOT_REVISION=MP2.0; expected='hardware revision mismatch' ;;
		wrong-slot-count) set -- STUB_FASTBOOT_SLOT_COUNT=1; expected='unexpected slot count' ;;
		wrong-slot) set -- STUB_FASTBOOT_SLOT=c; expected='unexpected current slot' ;;
		multiple-devices) set -- 'STUB_FASTBOOT_DEVICES=SERIAL-F fastboot usb:1|SERIAL-G fastboot usb:2'; expected='requires exactly one' ;;
	esac
	if env FERRIX_OUT_DIR="$refusal_out" STUB_ADB_DEVICES_EMPTY=1 \
		STUB_FASTBOOT_DEVICES='SERIAL-F fastboot usb:1' STUB_FASTBOOT_VARIANT='MSM USF' \
		"$@" "$workflow" >"$work/$refusal.out" 2>"$work/$refusal.err"; then
		echo "placeholder variant unexpectedly accepted $refusal" >&2
		exit 1
	fi
	grep -Fq "$expected" "$work/$refusal.err"
	[ ! -e "$refusal_out/pixel-9a/partition-capacities-latest.tsv" ]
done

# The shared gate used by mutating workflows must not gain this exception.
(
	. "$root/tools/pixel-9a/adb-correlation.sh"
	FERRIX_OUT_DIR=$placeholder_out
	if pixel9a_fastboot_sku_gate 'MSM USF' "$fixture_prefix"; then
		echo 'shared bootloader SKU gate accepted userspace placeholder' >&2
		exit 1
	fi
)

if STUB_ADB_DEVICES_EMPTY=1 STUB_FASTBOOT_DEVICES='SERIAL-F fastboot usb:1' STUB_FASTBOOT_IS_USERSPACE=no "$workflow" >"$work/not-userspace.out" 2>"$work/not-userspace.err"; then
	echo 'partition capacity unexpectedly passed outside fastbootd' >&2
	exit 1
fi
grep -q 'requires fastbootd' "$work/not-userspace.err"

if STUB_ADB_DEVICES_EMPTY=1 STUB_FASTBOOT_DEVICES='SERIAL-F fastboot usb:1' STUB_FASTBOOT_BAD_HEX=1 "$workflow" >"$work/bad-hex.out" 2>"$work/bad-hex.err"; then
	echo 'partition capacity unexpectedly accepted malformed Fastboot capacity' >&2
	exit 1
fi
grep -q 'invalid hex/decimal byte capacity' "$work/bad-hex.err"

if STUB_ADB_DEVICES_EMPTY=1 STUB_FASTBOOT_DEVICES='SERIAL-F fastboot usb:1' STUB_FASTBOOT_MISSING_DYNAMIC_B=1 "$workflow" >"$work/missing-fastboot-dynamic.out" 2>"$work/missing-fastboot-dynamic.err"; then
	echo 'partition capacity unexpectedly passed without Fastboot inactive dynamic capacity' >&2
	exit 1
fi
grep -q 'partition-size:vendor_dlkm_b is not reported' "$work/missing-fastboot-dynamic.err"

# Retain the real response on a normal handoff refusal and never claim that
# all capacities were observed before all ten partitions passed.
if FERRIX_OUT_DIR="$work/query-failure" STUB_ADB_DEVICES_EMPTY=1 \
	STUB_FASTBOOT_DEVICES='SERIAL.[F] fastboot usb:1' STUB_FAIL_VENDOR_BOOT=1 \
	"$workflow" >"$work/query-failure.out" 2>"$work/query-failure.err"; then
	echo 'handoff accepted failed vendor_boot query' >&2
	exit 1
fi
grep -Fq 'see redacted getvar response' "$work/query-failure.err"
grep -Fq 'Could not open partition' "$work/query-failure"/pixel-9a/partition-capacity-*.txt
! grep -R -Fq 'SERIAL.[F]' "$work/query-failure"
! grep -Rq '^handoff_status=exact-capacities-observed$' "$work/query-failure"
[ ! -e "$work/query-failure/pixel-9a/partition-capacities-latest.tsv" ]

# The local Fastboot CLI can exit zero even when the device reports FAILED.
# A successful process exit must not substitute for an exact size response.
if FERRIX_OUT_DIR="$work/query-failure-zero-exit" STUB_ADB_DEVICES_EMPTY=1 \
	STUB_FASTBOOT_DEVICES='SERIAL-F fastboot usb:1' STUB_FAIL_VENDOR_BOOT=1 \
	STUB_VENDOR_BOOT_EXIT=0 "$workflow" >"$work/zero-exit.out" 2>"$work/zero-exit.err"; then
	echo 'handoff accepted device failure with successful CLI exit' >&2
	exit 1
fi
grep -Fq 'partition-size:vendor_boot_a is not reported' "$work/zero-exit.err"
[ -z "$(find "$work/query-failure-zero-exit" -name '*.tsv' -print)" ]

if FERRIX_OUT_DIR="$work/zero-capacity" STUB_ADB_DEVICES_EMPTY=1 \
	STUB_FASTBOOT_DEVICES='SERIAL-F fastboot usb:1' STUB_FASTBOOT_DYNAMIC_B_SIZE=0x0 \
	"$workflow" >"$work/zero-capacity.out" 2>"$work/zero-capacity.err"; then
	echo 'handoff accepted zero inactive logical capacity' >&2
	exit 1
fi
grep -Fq 'partition-size:vendor_dlkm_b has an invalid hex/decimal byte capacity: 0x0' "$work/zero-capacity.err"
[ -z "$(find "$work/zero-capacity" -name '*.tsv' -print)" ]

# Diagnostic mode collects all responses without treating a missing query or
# a zero-sized logical slot as acceptable installation capacity.
FERRIX_OUT_DIR="$work/diagnostic" STUB_ADB_DEVICES_EMPTY=1 \
	STUB_FASTBOOT_DEVICES='SERIAL.[F] fastboot usb:1' STUB_FAIL_VENDOR_BOOT=1 \
	STUB_VENDOR_BOOT_EXIT=0 \
	STUB_FASTBOOT_DYNAMIC_B_SIZE=0x0 STUB_FASTBOOT_LOG="$work/diagnostic.commands" \
	"$workflow" --diagnose-fastboot >"$work/diagnostic.out" 2>"$work/diagnostic.err"
diagnostic=$(awk -F= '$1 == "transcript" { print $2 }' "$work/diagnostic.out")
grep -q '^result=diagnostic-only$' "$work/diagnostic.out"
grep -q '^unavailable_queries=1$' "$work/diagnostic.out"
grep -q '^handoff_status=not-produced-diagnostic-only$' "$diagnostic"
grep -q '^diagnostic_vendor_dlkm_b_raw=0x0$' "$diagnostic"
grep -q '^diagnostic_snapshot-update-status=unavailable$' "$diagnostic"
grep -q '^diagnostic_super-partition-name=super$' "$diagnostic"
grep -q '^diagnostic_is-logical:vendor_dlkm_b=yes$' "$diagnostic"
grep -Fq 'Could not open partition' "$diagnostic"
[ "$(grep -c '^getvar_query=partition-size:' "$diagnostic")" -eq 20 ]
[ "$(grep -c ' getvar partition-size:' "$work/diagnostic.commands")" -eq 20 ]
! grep -Fq 'SERIAL.[F]' "$diagnostic"
! grep -q '^handoff_status=exact-capacities-observed$' "$diagnostic"
[ -z "$(find "$work/diagnostic" -name '*.tsv' -print)" ]

FERRIX_OUT_DIR="$work/diagnostic-snapshot" STUB_ADB_DEVICES_EMPTY=1 \
	STUB_FASTBOOT_DEVICES='SERIAL-F fastboot usb:1' STUB_SNAPSHOT_STATUS=none \
	"$workflow" --diagnose-fastboot >"$work/snapshot.out" 2>"$work/snapshot.err"
snapshot_diagnostic=$(awk -F= '$1 == "transcript" { print $2 }' "$work/snapshot.out")
grep -q '^diagnostic_snapshot-update-status=none$' "$snapshot_diagnostic"
grep -q '^handoff_status=not-produced-diagnostic-only$' "$snapshot_diagnostic"
[ -z "$(find "$work/diagnostic-snapshot" -name '*.tsv' -print)" ]

for bad_diagnostic in adb bootloader wrong-identity; do
	set -- STUB_ADB_DEVICES_EMPTY=1 'STUB_FASTBOOT_DEVICES=SERIAL-F fastboot usb:1'
	case "$bad_diagnostic" in
		adb) set -- ;;
		bootloader) set -- "$@" STUB_FASTBOOT_IS_USERSPACE=no ;;
		wrong-identity) set -- "$@" STUB_FASTBOOT_REVISION=MP2.0 ;;
	esac
	if env FERRIX_OUT_DIR="$work/diagnostic-$bad_diagnostic" \
		STUB_FASTBOOT_LOG="$work/diagnostic-$bad_diagnostic.commands" "$@" \
		"$workflow" --diagnose-fastboot >"$work/diagnostic-$bad_diagnostic.out" 2>"$work/diagnostic-$bad_diagnostic.err"; then
		echo "diagnostic mode accepted $bad_diagnostic" >&2
		exit 1
	fi
	! grep -q ' getvar partition-size:' "$work/diagnostic-$bad_diagnostic.commands"
done

printf '%s\n' 'pixel-9a partition capacity tests passed'
