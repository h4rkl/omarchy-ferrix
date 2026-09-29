#!/bin/sh
set -eu

# Offline proof for the checked-in inactive-slot activation/boot workflow.
# The adb/fastboot programs below are strict stubs; this test never contacts a
# physical device.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
workflow="$root/tools/pixel-9a/boot-inactive.sh"
tab=$(printf '\t')
expected_target_id=google-pixel-9a-tegu-gxq96-mp1-0

[ -x "$workflow" ] || { echo 'inactive-slot workflow is not executable' >&2; exit 1; }
sh -n "$workflow"

offending=$(grep -nE '^[[:space:]]*(adb|fastboot)[[:space:]]' "$workflow" \
	| grep -vE '^[0-9]+:[[:space:]]*(adb devices -l|fastboot -s "\$fastboot_serial" (getvar|set_active|reboot))' \
	|| true)
[ -z "$offending" ] || { printf '%s\n' "$offending" >&2; exit 1; }
forbidden=$(grep -nE '^[[:space:]]*(adb|fastboot)[[:space:]].*(flash|erase|format|flashing|oem|wipe|reboot (fastboot|bootloader))' "$workflow" || true)
[ -z "$forbidden" ] || { printf '%s\n' "$forbidden" >&2; exit 1; }

sha256() {
	if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{ print $1 }'; else shasum -a 256 "$1" | awk '{ print $1 }'; fi
}
bytes() { wc -c <"$1" | tr -d '[:space:]'; }

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-boot-inactive-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/bin" "$work/target/recovery" "$work/images" "$work/out/pixel-9a"

printf '%s\n' 'synthetic-factory' >"$work/factory.zip"
printf '%s\n' 'synthetic-ota' >"$work/ota.zip"
cp "$work/factory.zip" "$work/target/recovery/factory.zip"
cp "$work/ota.zip" "$work/target/recovery/ota.zip"
factory_hash=$(sha256 "$work/factory.zip")
ota_hash=$(sha256 "$work/ota.zip")
factory_bytes=$(bytes "$work/factory.zip")
ota_bytes=$(bytes "$work/ota.zip")
{
	printf '# kind\tbuild\tversion\tfilename\turl\tsha256\tbytes\tsource-page\n'
	printf 'factory\tSYN\t0\tfactory.zip\thttps://example.invalid\t%s\t%s\thttps://example.invalid\n' "$factory_hash" "$factory_bytes"
	printf 'ota\tSYN\t0\tota.zip\thttps://example.invalid\t%s\t%s\thttps://example.invalid\n' "$ota_hash" "$ota_bytes"
} >"$work/target/recovery.pins"
cat >"$work/target/manifest.toml" <<'MANIFEST'
schema_version = 1
target_id = "google-pixel-9a-tegu-gxq96-mp1-0"
kind = "physical"
codename = "tegu"
hardware_revision = "MP1.0"
[gates]
identity = "passed"
image_compatibility = "passed"
backup = "not-required-dev-device"
data_policy = "dev-device-no-user-data"
recovery_procedure = "tested"
per_run_confirmation = "required"
MANIFEST

printf '%s\n' 'SYN-BOOT-001' >"$work/serial"
serial=$(cat "$work/serial")
serial_prefix=$(printf '%s' "$serial" | sha256sum | cut -c1-12)
cat >"$work/out/pixel-9a/discovery-adb.txt" <<ADB
transport=adb
serial_sha256_prefix=$serial_prefix
ro.product.manufacturer=Google
ro.product.model=Pixel 9a
ro.product.device=tegu
ro.boot.hardware.revision=MP1.0
ro.boot.hardware.sku=GXQ96
result=ok
ADB

printf 'schema_version\t1\ntarget_id\t%s\ncodename\ttegu\nhardware_revision\tMP1.0\nroute_id\texperimental-linux-google-tegu-s5300-wwan\nkernel_backend\tlinux-google-tegu-wwan\nstatus\tcomposed-not-booted\nbootable\tfalse\navb_mode\tfresh-generated\navb_algorithm\tNONE\navb_descriptor_inputs\tfresh-avbtool-hash-footers-from-final-images\n' "$expected_target_id" >"$work/images/boot-manifest.tsv"

for name in Image dtb.img boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system rootfs_handoff rootfs_image; do
	case "$name" in
		Image) relative=Image ;;
		dtb.img) relative=dtb.img ;;
		rootfs_handoff) relative=rootfs.tar ;;
		rootfs_image) relative=rootfs.img ;;
		*) relative="$name.img" ;;
	esac
	printf 'inactive-%s\n' "$name" >"$work/images/$relative"
	file_bytes=$(bytes "$work/images/$relative")
	file_hash=$(sha256 "$work/images/$relative")
	printf 'artifact\t%s\t%s\t%s\t%s\tsynthetic\n' "$name" "$relative" "$file_bytes" "$file_hash" >>"$work/images/boot-manifest.tsv"
done

cat >"$work/temp-evidence.txt" <<EVIDENCE
operation=pixel-9a-radio-boot-temp
target_id=$expected_target_id
serial_sha256_prefix=$serial_prefix
boot_exit_status=0
result=temporary-boot-issued
EVIDENCE
cat >"$work/flash-evidence.txt" <<EVIDENCE
operation=pixel-9a-flash
target_id=$expected_target_id
serial_sha256_prefix=$serial_prefix
current_slot=a
target_slot=b
kernel_stage=complete
result=flashed-not-booted
EVIDENCE
for name in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
	row=$(awk -F "$tab" -v name="$name" '$1 == "artifact" && $2 == name { print; exit }' "$work/images/boot-manifest.tsv")
	hash=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $5 }')
	printf 'flashed=partition=%s_b slot=b sha256=%s status=0\n' "$name" "$hash" >>"$work/flash-evidence.txt"
done

cat >"$work/bin/adb" <<'STUB'
#!/bin/sh
printf 'adb\t%s\n' "$*" >>"$STUB_CALLS"
[ "${1:-}" = devices ] && [ "${2:-}" = -l ] || exit 99
echo 'List of devices attached'
STUB
cat >"$work/bin/fastboot" <<'STUB'
#!/bin/sh
printf 'fastboot\t%s\n' "$*" >>"$STUB_CALLS"
state=$(cat "$STUB_STATE")
[ "$1" = devices ] && { [ "$2" = -l ] || exit 99; [ "$state" = gone ] || printf '%s fastboot usb:1\n' "$STUB_SERIAL"; exit 0; }
[ "$1" = -s ] && [ "$2" = "$STUB_SERIAL" ] || exit 99
case "$3" in
	getvar)
		case "$4" in
			product) value=tegu ;;
			variant) value=N/A ;;
			hw-revision) value=MP1.0 ;;
			unlocked) value=yes ;;
			secure) value=yes ;;
			current-slot) value=$(cat "$STUB_ACTIVE") ;;
			slot-count) value=2 ;;
			is-userspace) value=no ;;
			serialno) value=$STUB_SERIAL ;;
			*) exit 99 ;;
		esac
		printf '(bootloader) %s: %s\n' "$4" "$value" >&2
		;;
	set_active)
		[ "$4" = b ] || exit 98
		echo b >"$STUB_ACTIVE"
		;;
	reboot)
		[ -z "${4:-}" ] || exit 98
		echo gone >"$STUB_STATE"
		;;
	*) exit 99 ;;
esac
STUB
chmod 0755 "$work/bin/adb" "$work/bin/fastboot"

calls="$work/calls.log"
: >"$calls"
export PATH="$work/bin:$PATH" STUB_CALLS="$calls" STUB_STATE="$work/state" STUB_ACTIVE="$work/active" STUB_SERIAL="$serial"
export FERRIX_PIXEL9A_TARGET_DIR="$work/target" FERRIX_OUT_DIR="$work/out"
export FERRIX_PIXEL9A_RADIO_SPLIT_MANIFEST="$work/images/boot-manifest.tsv"
export FERRIX_PIXEL9A_RADIO_TEMP_BOOT_EVIDENCE="$work/temp-evidence.txt"
export FERRIX_PIXEL9A_RADIO_FLASH_EVIDENCE="$work/flash-evidence.txt"
export FERRIX_PIXEL9A_BOOT_WAIT_SECONDS=1

run() {
	case_name=$1
	input=$2
	shift 2
	: >"$calls"
	echo bootloader >"$work/state"
	echo a >"$work/active"
	status=0
	printf '%s\n' "$input" | sh "$workflow" "$@" >"$work/$case_name.out" 2>"$work/$case_name.err" || status=$?
}

run preflight '' --preflight --slot b --allow-unsigned-avb
[ "$status" -eq 0 ]
grep -q '^preflight=passed$' "$work/preflight.out"
! grep -qE 'set_active|reboot' "$calls"

run refused 'CONFIRM PIXEL 9A ACTIVATE INACTIVE SLOT B AND BOOT' --confirm-boot --slot b --allow-unsigned-avb
[ "$status" -eq 1 ]
grep -q 'backup is not marked verified' "$work/refused.err"
! grep -qE 'set_active|reboot' "$calls"

export FERRIX_PIXEL9A_NO_USER_DATA=I-understand-this-is-a-new-development-device-with-no-personal-data
run boot 'CONFIRM PIXEL 9A ACTIVATE INACTIVE SLOT B AND BOOT' --confirm-boot --slot b --allow-unsigned-avb
[ "$status" -eq 0 ]
grep -q '^set_active_exit_status=0$' "$work/boot.out"
grep -q '^post_set_active_current_slot=b$' "$work/boot.out"
grep -q '^result=slot-activated-boot-requested$' "$work/boot.out"
grep -q '^post_reboot_transport=not-observed$' "$work/boot.out"
[ "$(grep -c 'fastboot.*set_active b' "$calls")" -eq 1 ]
[ "$(grep -c 'fastboot.*reboot$' "$calls")" -eq 1 ]
! grep -q 'flash' "$calls"

run active '' --preflight --slot a --allow-unsigned-avb
[ "$status" -eq 1 ]
grep -q "requested slot 'a' is active" "$work/active.err"
! grep -qE 'set_active|reboot' "$calls"

printf '%s\n' 'pixel-9a inactive-slot workflow tests passed'
