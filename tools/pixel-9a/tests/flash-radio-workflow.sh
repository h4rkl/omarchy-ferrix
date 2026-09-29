#!/bin/sh
# Offline proof for the separately gated radio-first Pixel 9a flash route.
# adb/fastboot are strict local stubs; this test never contacts a device.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script="$root/tools/pixel-9a/flash-radio.sh"
real_target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
tab=$(printf '\t')
expected_target_id=google-pixel-9a-tegu-gxq96-mp1-0
fails=0

check() {
	name=$1
	shift
	if "$@"; then printf 'ok   %s\n' "$name"; else printf 'FAIL %s\n' "$name"; fails=$((fails + 1)); fi
}

[ -x "$script" ] || { echo 'radio-first flash route is not executable' >&2; exit 1; }
sh -n "$script"

# Keep the device-tool surface reviewable.  The route may only probe transports
# and use the explicitly gated boot/flash/reboot verbs.
offending=$(grep -nE '^[[:space:]]*(adb|fastboot)[[:space:]]' "$script" \
	| grep -vE '^[0-9]+:[[:space:]]*fastboot -s "\$fastboot_serial" (getvar "\$variable"|boot "\$boot_image"|flash "\$partition" "\$image"|reboot (fastboot|bootloader))' \
	| grep -vE '^[0-9]+:[[:space:]]*fastboot -s "\$fastboot_serial" (getvar|boot|flash|reboot) .*\|\|' \
	|| true)
check 'static: only allow-listed device verbs are executable' [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"
forbidden=$(grep -nE '^[[:space:]]*(adb|fastboot)[[:space:]]' "$script" \
	| grep -E '(erase|format|flashing|set_active|oem|wipe|[[:space:]]-w([[:space:]]|$))' || true)
check 'static: no destructive, unlock, or slot-changing verbs' [ -z "$forbidden" ]

sha256() {
	if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{ print $1 }'; else shasum -a 256 "$1" | awk '{ print $1 }'; fi
}
bytes() { wc -c <"$1" | tr -d '[:space:]'; }

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-flash-radio-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/bin" "$work/images" "$work/target/recovery" "$work/target/pmos/device-google-tegu"
cp "$real_target/image-layout.toml" "$work/target/image-layout.toml"
cp "$real_target/kernel-radio-route.toml" "$work/target/kernel-radio-route.toml"
cp "$real_target/radio-split-artifact-contract.toml" "$work/target/radio-split-artifact-contract.toml"
cp "$real_target/pmos/device-google-tegu/deviceinfo" "$work/target/pmos/device-google-tegu/deviceinfo"
printf '%s\n' 'synthetic-factory' >"$work/factory.zip"
cp "$work/factory.zip" "$work/target/recovery/tegu-synthetic-factory.zip"
printf 'deviceinfo_flash_fastboot_partition_rootfs="userdata"\n' >>"$work/target/pmos/device-google-tegu/deviceinfo"
printf '%s\n' 'schema_version = 1' 'target_id = "google-pixel-9a-tegu-gxq96-mp1-0"' 'kind = "physical"' 'codename = "tegu"' 'hardware_revision = "MP1.0"' '[gates]' 'identity = "passed"' 'image_compatibility = "passed"' 'backup = "verified"' 'recovery_procedure = "tested"' 'per_run_confirmation = "required"' >"$work/target/manifest.toml"
printf '# kind\tbuild\tversion\tfilename\turl\tsha256\tbytes\tsource-page\n' >"$work/target/recovery.pins"
printf 'factory\tSYN\t0\ttegu-synthetic-factory.zip\thttps://example.invalid\t%s\t%s\thttps://example.invalid\n' "$(sha256 "$work/factory.zip")" "$(bytes "$work/factory.zip")" >>"$work/target/recovery.pins"

# Build tiny, hash-recorded source handoffs and the exact ten-partition capacity
# handoff consumed by the split manifest.
printf 'source-image\n' >"$work/images/Image"
printf 'source-dtb\n' >"$work/images/dtb.img"
printf 'schema_version\t1\ntarget_id\t%s\nroute_id\texperimental-linux-google-tegu-s5300-wwan\nkernel_backend\tlinux-google-tegu-wwan\nstatus\tbuilt-not-booted\nbootable\tfalse\n' "$expected_target_id" >"$work/kernel.tsv"
printf 'schema_version\t1\ntarget_id\t%s\nkernel_backend\tlinux-google-tegu-wwan\nstatus\tbuilt-substrate-only\nbootable\tfalse\n' "$expected_target_id" >"$work/rootfs.tsv"
printf 'early-cpio\n' >"$work/vendor-kernel-boot.cpio"
{
	printf 'schema_version\t1\ntarget_id\t%s\n' 'google-pixel-9a-tegu-gxq96-mp1-0'
	for name in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do printf 'partition\t%s\t4096\n' "$name"; done
} >"$work/capacity.tsv"
for name in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do printf 'radio-%s\n' "$name" >"$work/images/$name.img"; done
printf 'rootfs-tar\n' >"$work/images/rootfs.tar"
printf 'rootfs-image\n' >"$work/images/rootfs.img"

contract_hash=$(sha256 "$work/target/radio-split-artifact-contract.toml")
{
	printf 'schema_version\t1\ntarget_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\ncodename\ttegu\nhardware_revision\tMP1.0\nroute_id\texperimental-linux-google-tegu-s5300-wwan\nkernel_backend\tlinux-google-tegu-wwan\nstatus\tcomposed-not-booted\nbootable\tfalse\nheader_version\t4\npartition_contract\tcomplete-source-built\nsplit_artifact_contract\tradio-split-artifact-contract.toml\t%s\navb_mode\tfresh-generated\navb_algorithm\tNONE\navb_descriptor_inputs\tfresh-avbtool-hash-footers-from-final-images\navb_binding_salt\tsha256-of-pre-footer-image-bytes\n' "$contract_hash"
	printf 'partition_capacities\t%s\t%s\t%s\n' "$work/capacity.tsv" "$(bytes "$work/capacity.tsv")" "$(sha256 "$work/capacity.tsv")"
	printf 'kernel_manifest\t%s\t%s\t%s\n' "$work/kernel.tsv" "$(bytes "$work/kernel.tsv")" "$(sha256 "$work/kernel.tsv")"
	printf 'rootfs_manifest\t%s\t%s\t%s\n' "$work/rootfs.tsv" "$(bytes "$work/rootfs.tsv")" "$(sha256 "$work/rootfs.tsv")"
	printf 'vendor_kernel_boot_input\t%s\t%s\t%s\n' "$work/vendor-kernel-boot.cpio" "$(bytes "$work/vendor-kernel-boot.cpio")" "$(sha256 "$work/vendor-kernel-boot.cpio")"
	printf 'artifact\tImage\tImage\t%s\t%s\tkernel\n' "$(bytes "$work/images/Image")" "$(sha256 "$work/images/Image")"
	printf 'artifact\tdtb.img\tdtb.img\t%s\t%s\tkernel\n' "$(bytes "$work/images/dtb.img")" "$(sha256 "$work/images/dtb.img")"
	for name in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
		case "$name" in boot|init_boot|vendor_boot) origin=generated-mkbootimg ;; vendor_kernel_boot) origin=source-built-cpio ;; dtbo) origin=generated-mkdtboimg ;; vendor_dlkm|system_dlkm) origin=source-built-modules ;; *) origin=generated-avbtool-from-final-images ;; esac
		printf 'artifact\t%s\t%s.img\t%s\t%s\t%s\n' "$name" "$name" "$(bytes "$work/images/$name.img")" "$(sha256 "$work/images/$name.img")" "$origin"
	done
	printf 'artifact\trootfs_handoff\trootfs.tar\t%s\t%s\trootfs\n' "$(bytes "$work/images/rootfs.tar")" "$(sha256 "$work/images/rootfs.tar")"
	printf 'artifact\trootfs_image\trootfs.img\t%s\t%s\trootfs\n' "$(bytes "$work/images/rootfs.img")" "$(sha256 "$work/images/rootfs.img")"
	for name in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm; do printf 'avb_bound_image\t%s\t%s.img\t%s\t%s\n' "$name" "$name" "$(bytes "$work/images/$name.img")" "$(sha256 "$work/images/$name.img")"; done
} >"$work/images/boot-manifest.tsv"

cat >"$work/bin/adb" <<'EOF'
#!/bin/sh
printf 'adb\t%s\n' "$*" >>"$STUB_CALLS"
[ "${1:-}" = devices ] && [ "${2:-}" = -l ] || exit 99
echo 'List of devices attached'
EOF
cat >"$work/bin/fastboot" <<'EOF'
#!/bin/sh
printf 'fastboot\t%s\n' "$*" >>"$STUB_CALLS"
state=$(cat "$STUB_STATE")
case "$1" in
  devices) [ "$2" = -l ] || exit 99; [ "$state" = gone ] || printf '%s fastboot usb:1\n' "$STUB_SERIAL" ;;
  -s)
    [ "$2" = "$STUB_SERIAL" ] || exit 99
    case "$3" in
      getvar) case "$4" in product) value=tegu ;; variant) value=GXQ96 ;; hw-revision) value=MP1.0 ;; unlocked) value=yes ;; secure) value=yes ;; current-slot) value=a ;; slot-count) value=2 ;; serialno) value=$STUB_SERIAL ;; is-userspace) [ "$state" = fastbootd ] && value=yes || value=no ;; *) exit 99 ;; esac; printf '(bootloader) %s: %s\n' "$4" "$value" >&2 ;;
      boot) [ -f "$4" ] || exit 99; echo gone >"$STUB_STATE" ;;
      flash) [ -f "$5" ] || exit 99; case "$4" in vendor_dlkm_*|system_dlkm_*) [ "$state" = fastbootd ] || exit 98 ;; esac ;;
      reboot) case "$4" in fastboot) echo fastbootd >"$STUB_STATE" ;; bootloader) echo bootloader >"$STUB_STATE" ;; *) exit 99 ;; esac ;;
      *) exit 99 ;;
    esac ;;
  *) exit 99 ;;
esac
EOF
chmod 0755 "$work/bin/adb" "$work/bin/fastboot"

serial=SYN-RADIO-001
calls="$work/calls.log"
: >"$calls"
echo bootloader >"$work/state"
export PATH="$work/bin:$PATH" STUB_CALLS="$calls" STUB_STATE="$work/state" STUB_SERIAL="$serial"
export FERRIX_PIXEL9A_TARGET_DIR="$work/target" FERRIX_PIXEL9A_RADIO_SPLIT_MANIFEST="$work/images/boot-manifest.tsv" FERRIX_OUT_DIR="$work/out" FERRIX_PIXEL9A_FLASH_WAIT_SECONDS=1

run() {
	case_name=$1; input=$2; shift 2
	: >"$calls"; echo bootloader >"$work/state"; status=0
	printf '%s\n' "$input" | sh "$script" "$@" >"$work/$case_name.out" 2>"$work/$case_name.err" || status=$?
}

run preflight_ok '' preflight --allow-unsigned-avb
check 'preflight: verifies source-built radio contract without mutation' [ "$status" -eq 0 ]
check 'preflight: identifies WWAN/SIT route' grep -q '^route_id=experimental-linux-google-tegu-s5300-wwan$' "$work/preflight_ok.out"
check 'preflight: no device mutation' sh -c "! grep -qE 'fastboot.*(boot|flash|reboot)' '$calls'"

boot_hash=$(awk -F "$tab" '$1 == "artifact" && $2 == "boot" { print $5; exit }' "$work/images/boot-manifest.tsv")
boot_phrase=$(printf 'CONFIRM PIXEL 9A RADIO TEMPORARY BOOT boot.img@%s\n' "$(printf '%s' "$boot_hash" | cut -c1-8)")
run boot_temp "$boot_phrase" boot-temp --allow-unsigned-avb
check 'boot-temp: exact radio phrase succeeds' [ "$status" -eq 0 ]
evidence=$(find "$work/out/pixel-9a/flash-radio" -name 'boot-temp-*.txt' -type f | head -1)
check 'boot-temp: writes route-specific evidence' [ -n "$evidence" ]
check 'privacy: evidence does not contain serial' env SERIAL="$serial" EVIDENCE="$evidence" sh -c '! grep -Fq "$SERIAL" "$EVIDENCE"'

export FERRIX_PIXEL9A_RADIO_TEMP_BOOT_EVIDENCE="$evidence"
flash_phrase=$(printf 'CONFIRM PIXEL 9A RADIO FLASH KERNEL STAGE TO SLOT B\n')
run flash_kernel "$flash_phrase" flash --stage kernel --slot b --allow-unsigned-avb
check 'flash: complete split kernel stage succeeds on inactive slot' [ "$status" -eq 0 ]
check 'flash: ten partition writes and two reboots' sh -c "[ \$(grep -c 'flash' '$calls') -eq 10 ] && [ \$(grep -c 'reboot' '$calls') -eq 2 ]"
check 'flash: never writes active slot' sh -c "! grep -qE 'flash [^ ]+_a ' '$calls'"

# A canonical backend must be rejected before any device probe.
sed 's/^kernel_backend\tlinux-google-tegu-wwan$/kernel_backend\tgoogle-kleaf-cpif/' "$work/images/boot-manifest.tsv" >"$work/canonical.tsv"
export FERRIX_PIXEL9A_RADIO_SPLIT_MANIFEST="$work/canonical.tsv"
run canonical_rejected '' preflight --allow-unsigned-avb
check 'route separation: canonical backend is refused' test "$status" -eq 1
check 'route separation: canonical refusal happens before transport access' sh -c "! grep -q '^adb\|^fastboot' '$calls' && grep -q 'kernel_backend is not linux-google-tegu-wwan' '$work/canonical_rejected.err'"

if [ "$fails" -eq 0 ]; then echo '{"event":"pixel-9a-flash-radio-test","status":"passed"}'; else printf '{"event":"pixel-9a-flash-radio-test","status":"failed","failures":%s}\n' "$fails"; exit 1; fi
