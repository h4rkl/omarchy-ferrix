#!/bin/sh
# Offline proof for the gated Pixel 9a flash workflow. Platform Tools are
# replaced with strict stubs that log every call and exit 99 on any verb
# outside the allow-list, so this test never enumerates or contacts a real
# device. Images, manifest, recovery pins, and the factory archive are tiny
# fabricated fixtures in the style of tests/boot-workflow.sh.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script="$root/tools/pixel-9a/flash.sh"
real_target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
fails=0
tab=$(printf '\t')

check() {
  name=$1
  shift
  if "$@"; then
    printf 'ok   %s\n' "$name"
  else
    printf 'FAIL %s\n' "$name"
    fails=$((fails + 1))
  fi
}

[ -x "$script" ] || { echo 'flash workflow is not executable' >&2; exit 1; }
sh -n "$script"

# Static allow-list: every executable adb/fastboot invocation must be one of
# the exact forms below. Comment lines and message-only lines (emit/die/fail/
# printf) are excluded; the dynamic call-log check below covers behaviour.
offending=$(grep -nE '(^|[^[:alnum:]_])(adb|fastboot)([^[:alnum:]_]|$)' "$script" \
  | grep -vE '^[0-9]+:[[:space:]]*#' \
  | grep -vE '^[0-9]+:[[:space:]]*\. ' \
  | grep -vE '^[0-9]+:[[:space:]]*(emit|die|fail|printf) ' \
  | grep -vE '^[0-9]+:[[:space:]]*need (adb|fastboot)$' \
  | grep -vE '^[0-9]+:[[:space:]]*adb_rows=\$\(adb devices -l \| awk ' \
  | grep -vE '^[0-9]+:[[:space:]]*fastboot_rows=\$\(fastboot devices -l 2>/dev/null \| awk ' \
  | grep -vE '^[0-9]+:[[:space:]]*fastboot -s "\$fastboot_serial" getvar "\$variable" 2>&1 \| awk ' \
  | grep -vE '^[0-9]+:[[:space:]]*fastboot -s "\$fastboot_serial" boot "\$boot_image" \|\| boot_status=\$\?$' \
  | grep -vE '^[0-9]+:[[:space:]]*fastboot -s "\$fastboot_serial" flash "\$partition" "\$image" \|\| flash_status=\$\?$' \
  | grep -vE '^[0-9]+:[[:space:]]*fastboot -s "\$fastboot_serial" reboot (fastboot|bootloader) \|\| reboot_status=\$\?$' \
  || true)
check "static: only allow-listed device verbs are executable" [ -z "$offending" ]
[ -z "$offending" ] || printf '%s\n' "$offending"

forbidden=$(grep -nE '(^|[^[:alnum:]_])(adb|fastboot)([^[:alnum:]_]|$)' "$script" \
  | grep -vE '^[0-9]+:[[:space:]]*#' \
  | grep -E '(erase|format|flashing|set_active|oem|wipe|[[:space:]]-w([[:space:]]|$))' || true)
check "static: no destructive or lock/slot verbs near a device tool" [ -z "$forbidden" ]
[ -z "$forbidden" ] || printf '%s\n' "$forbidden"

check "static: every mutation is announced before it runs" sh -c "
  [ \$(grep -c 'emit mutation \"fastboot -s' '$script') -eq 4 ] &&
  [ \$(grep -cE '^[[:space:]]*fastboot -s \"\\\$fastboot_serial\" (boot|flash|reboot) ' '$script') -eq 4 ]"

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}
bytes() { wc -c <"$1" | tr -d '[:space:]'; }

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-flash-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/bin"
calls="$work/calls.log"
state="$work/fastboot.mode"
: >"$calls"
echo bootloader >"$state"

# Fabricated composed image directory in boot.sh's manifest shape.
make_images() {
  dir=$1
  mkdir -p "$dir"
  for name in boot.img init_boot.img vendor_boot.img vendor_kernel_boot.img dtbo.img vendor_dlkm.img system_dlkm.img rootfs.tar rootfs.img vbmeta.img vbmeta_vendor.img vbmeta_system.img; do
    printf 'ferrix-%s\n' "$name" >"$dir/$name"
  done
  {
    printf 'schema_version\t1\n'
    printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
    printf 'codename\ttegu\n'
    printf 'hardware_revision\tMP1.0\n'
    printf 'header_version\t4\n'
    printf 'kernel_backend\tgoogle-kleaf-cpif\n'
    printf 'status\tcomposed-not-booted\n'
    printf 'bootable\tfalse\n'
    printf 'avb_mode\tNONE\n'
    printf 'kernel_manifest\t/synthetic/build-manifest.tsv\t1\t%s\n' "$(printf '0%.0s' $(seq 1 64))"
    for pair in boot:boot.img:kernel init_boot:init_boot.img:generated-mkbootimg vendor_boot:vendor_boot.img:generated-mkbootimg \
      vendor_kernel_boot:vendor_kernel_boot.img:kernel dtbo:dtbo.img:kernel vendor_dlkm:vendor_dlkm.img:kernel \
      system_dlkm:system_dlkm.img:kernel rootfs_handoff:rootfs.tar:rootfs rootfs_image:rootfs.img:rootfs vbmeta:vbmeta.img:generated-avbtool \
      vbmeta_vendor:vbmeta_vendor.img:generated-avbtool vbmeta_system:vbmeta_system.img:generated-avbtool; do
      name=${pair%%:*}
      rest=${pair#*:}
      file=${rest%%:*}
      origin=${rest#*:}
      printf 'artifact\t%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$file" "$(bytes "$dir/$file")" "$(sha256 "$dir/$file")" "$origin" "/synthetic/staging/$file"
    done
  } >"$dir/boot-manifest.tsv"
}

# Fabricated board package: real image-layout and deviceinfo, private pins,
# tiny factory archive.
make_target() {
  dir=$1
  with_archive=$2
  mkdir -p "$dir/recovery" "$dir/pmos/device-google-tegu"
  cp "$real_target/image-layout.toml" "$dir/image-layout.toml"
  cp "$real_target/pmos/device-google-tegu/deviceinfo" "$dir/pmos/device-google-tegu/deviceinfo"
  printf 'synthetic-factory-archive\n' >"$work/factory.zip"
  if [ "$with_archive" = yes ]; then
    cp "$work/factory.zip" "$dir/recovery/tegu-synthetic-factory.zip"
  fi
  {
    printf '# kind\tbuild\tversion\tfilename\turl\tsha256\tbytes\tsource-page\n'
    printf 'factory\tSYN\t0\ttegu-synthetic-factory.zip\thttps://example.invalid/factory\t%s\t%s\thttps://example.invalid\n' "$(sha256 "$work/factory.zip")" "$(bytes "$work/factory.zip")"
  } >"$dir/recovery.pins"
  cat >"$dir/manifest.toml" <<'EOF'
[gates]
identity = "passed"
image_compatibility = "passed"
backup = "verified"
recovery_procedure = "tested"
per_run_confirmation = "required"
EOF
}

make_images "$work/images"
image_dir=$(CDPATH= cd -- "$work/images" && pwd)
make_target "$work/target" yes
make_target "$work/target-no-recovery" no
boot_hash=$(sha256 "$image_dir/boot.img")
boot_short=$(printf '%s' "$boot_hash" | cut -c1-8)

cat >"$work/bin/adb" <<'STUB'
#!/bin/sh
printf 'adb\t' >>"$STUB_CALLS"
printf '%s\t' "$@" >>"$STUB_CALLS"
printf '\n' >>"$STUB_CALLS"
case "$1" in
  devices)
    [ "$#" -eq 2 ] && [ "$2" = -l ] || exit 99
    echo "List of devices attached"
    printf '%s' "${STUB_ADB_DEVICES:-}"
    ;;
  *) echo "stub: unexpected adb verb $1" >&2; exit 99 ;;
esac
STUB

cat >"$work/bin/fastboot" <<'STUB'
#!/bin/sh
printf 'fastboot\t' >>"$STUB_CALLS"
printf '%s\t' "$@" >>"$STUB_CALLS"
printf '\n' >>"$STUB_CALLS"
mode=$(cat "$STUB_STATE" 2>/dev/null || echo bootloader)
case "$1" in
  devices)
    [ "$#" -eq 2 ] && [ "$2" = -l ] || exit 99
    [ "$mode" = gone ] || printf '%s' "${STUB_FASTBOOT_DEVICES:-}"
    ;;
  -s)
    [ "$#" -ge 3 ] || exit 99
    [ "$2" = "$STUB_SERIAL" ] || { echo "stub: unexpected serial $2" >&2; exit 99; }
    case "$3" in
      getvar)
        [ "$#" -eq 4 ] || exit 99
        case "$4" in
          product) value=${STUB_PRODUCT:-tegu} ;;
          variant) value=${STUB_VARIANT:-GXQ96} ;;
          hw-revision) value=${STUB_HW_REVISION:-MP1.0} ;;
          unlocked) value=${STUB_UNLOCKED:-yes} ;;
          secure) value=${STUB_SECURE:-yes} ;;
          current-slot) value=${STUB_CURRENT_SLOT:-a} ;;
          slot-count) value=${STUB_SLOT_COUNT:-2} ;;
          is-userspace) if [ "$mode" = fastbootd ]; then value=yes; else value=no; fi ;;
          serialno) value=$STUB_SERIAL ;;
          *) echo "stub: unexpected getvar $4" >&2; exit 99 ;;
        esac
        printf '%s: %s\nFinished. Total time: 0.001s\n' "$4" "$value" >&2
        ;;
      boot)
        [ "$#" -eq 4 ] && [ -f "$4" ] || exit 99
        [ "${STUB_BOOT_LEAVES:-1}" = 0 ] || echo gone >"$STUB_STATE"
        exit "${STUB_BOOT_EXIT:-0}"
        ;;
      flash)
        [ "$#" -eq 5 ] && [ -f "$5" ] || exit 99
        case "$4" in
          vendor_dlkm_*|system_dlkm_*)
            # The bootloader cannot resolve logical partitions; only fastbootd can.
            [ "$mode" = fastbootd ] || { echo "FAILED (remote: 'No such partition')" >&2; exit 1; }
            ;;
        esac
        printf 'Sending %s OKAY\nWriting %s OKAY\n' "$4" "$4" >&2
        ;;
      reboot)
        [ "$#" -eq 4 ] || exit 99
        case "$4" in
          fastboot) echo fastbootd >"$STUB_STATE" ;;
          bootloader) echo bootloader >"$STUB_STATE" ;;
          *) exit 99 ;;
        esac
        ;;
      *) echo "stub: unexpected fastboot verb $3" >&2; exit 99 ;;
    esac
    ;;
  *) echo "stub: unexpected fastboot verb $1" >&2; exit 99 ;;
esac
STUB
chmod 0755 "$work/bin/adb" "$work/bin/fastboot"

serial='SYN.00[1]/&$'
export PATH="$work/bin:$PATH" STUB_CALLS="$calls" STUB_STATE="$state" STUB_SERIAL="$serial"
export STUB_ADB_DEVICES="" STUB_FASTBOOT_DEVICES="$serial fastboot usb:1
"
export FERRIX_PIXEL9A_FLASH_WAIT_SECONDS=1
export FERRIX_PIXEL9A_TARGET_DIR="$work/target" FERRIX_PIXEL9A_BOOT_OUT="$image_dir"
unset FERRIX_PIXEL9A_BOOT_MANIFEST FERRIX_PIXEL9A_PINS FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE || true

if command -v sha256sum >/dev/null 2>&1; then
  serial_prefix=$(printf '%s' "$serial" | sha256sum | cut -c1-12)
else
  serial_prefix=$(printf '%s' "$serial" | shasum -a 256 | cut -c1-12)
fi
partition_inventory="$work/partition-inventory.txt"
cat >"$partition_inventory" <<EOF
# Pixel 9a read-only partition inventory, synthetic UTC
# Serial numbers are replaced by a SHA-256 prefix. Never commit this file.
target_id=google-pixel-9a-tegu-gxq96-mp1-0
transport=adb
serial_sha256_prefix=$serial_prefix
product=tegu
hardware_revision=MP1.0
sku=GXQ96
current_slot=a
slot_count=2
partitions=/dev/block/by-name/userdata
block_devices=/dev/block/sda
mounts=/dev/block/by-name/userdata /data ext4 rw
cmdline=root=PARTLABEL=userdata
inventory_completeness=complete
result=ok
EOF

# run <case> <stdin-text> <args...>: fresh call log, fresh bootloader state.
# Per-case overrides are given as VAR=value prefixes; because POSIX leaves it
# unspecified whether such prefixes persist after a function call (macOS
# /bin/sh keeps them), every override is reset to its default afterwards.
run() {
  case_name=$1
  input=$2
  shift 2
  mkdir -p "$work/$case_name"
  export FERRIX_OUT_DIR="$work/$case_name"
  : >"$calls"
  echo "${STUB_START_MODE:-bootloader}" >"$state"
  status=0
  printf '%s' "$input" | sh "$script" "$@" >"$work/$case_name/stdout" 2>"$work/$case_name/stderr" || status=$?
  export FERRIX_PIXEL9A_TARGET_DIR="$work/target" FERRIX_PIXEL9A_BOOT_OUT="$image_dir"
  export STUB_ADB_DEVICES="" STUB_FASTBOOT_DEVICES="$serial fastboot usb:1
"
  unset FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE STUB_HW_REVISION STUB_PRODUCT STUB_VARIANT STUB_UNLOCKED STUB_SECURE STUB_CURRENT_SLOT STUB_START_MODE || true
}

mutations() {
  grep -cE "^fastboot${tab}-s${tab}[^${tab}]*${tab}(boot|flash|reboot)${tab}" "$calls" || true
}

# --- preflight ---------------------------------------------------------------
run preflight_ok '' preflight --allow-unsigned-avb
check "preflight: passes with exact identity, hashes, and recovery" [ "$status" -eq 0 ]
check "preflight: reports preflight=passed" grep -q '^preflight=passed$' "$work/preflight_ok/stdout"
check "preflight: prints exact serial for the operator" grep -Fq "serial=$serial" "$work/preflight_ok/stdout"
check "preflight: prints product and revision" sh -c "grep -q '^product=tegu$' '$work/preflight_ok/stdout' && grep -q '^hardware_revision=MP1.0$' '$work/preflight_ok/stdout'"
check "preflight: prints and verifies exact SKU variant" grep -q '^sku=GXQ96$' "$work/preflight_ok/stdout"
check "preflight: prints slot-explicit partition targets with hashes" grep -q "^plan=artifact=boot partition_target=boot_a via=bootloader input_image_sha256=$boot_hash bytes=" "$work/preflight_ok/stdout"
check "preflight: marks dlkm partitions as fastbootd" grep -q '^plan=artifact=vendor_dlkm partition_target=vendor_dlkm_a via=fastbootd ' "$work/preflight_ok/stdout"
check "preflight: names the verified recovery artifact" grep -q "^recovery_artifact=$work/target/recovery/tegu-synthetic-factory.zip sha256=$(sha256 "$work/factory.zip") bytes=" "$work/preflight_ok/stdout"
check "preflight: issues zero mutations" [ "$(mutations)" -eq 0 ]

run preflight_slot_b '' preflight --slot b --allow-unsigned-avb
check "preflight: --slot b overrides the current slot" sh -c "grep -q '^target_slot=b$' '$work/preflight_slot_b/stdout' && grep -q '^slot_source=--slot$' '$work/preflight_slot_b/stdout' && grep -q '^plan=artifact=dtbo partition_target=dtbo_b ' '$work/preflight_slot_b/stdout'"

# The radio-first WWAN/SIT composer intentionally has overlapping artifact
# names but is not a complete Pixel split image.  Its explicit backend must be
# rejected before any transport query or mutation can occur.
make_images "$work/images-radio"
sed "s/^kernel_backend${tab}google-kleaf-cpif$/kernel_backend${tab}linux-google-tegu-wwan/" \
  "$work/images/boot-manifest.tsv" >"$work/images-radio/boot-manifest.tsv"
FERRIX_PIXEL9A_BOOT_OUT="$work/images-radio" run radio_backend '' preflight --allow-unsigned-avb
check "preflight: refuses radio-first backend on canonical flash route" sh -c "[ $status -eq 1 ] && grep -q 'radio-first WWAN/SIT artifacts require a separate reviewed flash route' '$work/radio_backend/stderr'"
check "preflight: radio-first refusal issues zero device calls" [ ! -s "$calls" ]

run preflight_avb '' preflight
check "preflight: refuses unsigned AVB on a secure device without acknowledgement" [ "$status" -eq 1 ]
check "preflight: AVB refusal explains the unlocked/orange assumption" grep -q '^REFUSING: avb_mode is NONE on a secure=yes device' "$work/preflight_avb/stderr"

# --- manifest and recovery refusals -------------------------------------------
make_images "$work/images-bad"
# Same byte count as the manifest row, different content: only the hash differs.
printf 'ferrix-dtbo.imX\n' >"$work/images-bad/dtbo.img"
FERRIX_PIXEL9A_BOOT_OUT="$work/images-bad" run hash_mismatch '' preflight --allow-unsigned-avb
check "refuses: artifact hash mismatch" [ "$status" -eq 1 ]
check "refuses: hash mismatch names the artifact" grep -q '^REFUSING: dtbo hash differs from its manifest' "$work/hash_mismatch/stderr"
check "refuses: hash mismatch happens before any device call" [ ! -s "$calls" ]

make_images "$work/images-rev"
awk -F "$tab" -v OFS="$tab" '$1 == "hardware_revision" { $2 = "MP1.1" } { print }' "$work/images/boot-manifest.tsv" >"$work/images-rev/boot-manifest.tsv"
FERRIX_PIXEL9A_BOOT_OUT="$work/images-rev" run manifest_revision '' preflight --allow-unsigned-avb
check "refuses: manifest hardware_revision is not MP1.0" sh -c "[ $status -eq 1 ] && grep -q 'hardware_revision is not MP1.0' '$work/manifest_revision/stderr'"

STUB_HW_REVISION=MP1.1 run device_revision '' preflight --allow-unsigned-avb
check "refuses: device hw-revision mismatch" sh -c "[ $status -eq 1 ] && grep -q \"^REFUSING: Fastboot hardware revision mismatch: 'MP1.1'\" '$work/device_revision/stderr'"

STUB_PRODUCT=komodo run device_product '' preflight --allow-unsigned-avb
check "refuses: device product mismatch" sh -c "[ $status -eq 1 ] && grep -q \"^REFUSING: Fastboot product mismatch: 'komodo'\" '$work/device_product/stderr'"

STUB_VARIANT=GB62 run device_variant '' preflight --allow-unsigned-avb
check "refuses: device SKU/variant mismatch" sh -c "[ $status -eq 1 ] && grep -q \"^REFUSING: Fastboot SKU/variant mismatch: 'GB62'\" '$work/device_variant/stderr'"

STUB_UNLOCKED=no run locked '' preflight --allow-unsigned-avb
check "refuses: unlocked=no" sh -c "[ $status -eq 1 ] && grep -q \"^REFUSING: Fastboot reports unlocked='no'\" '$work/locked/stderr'"

FERRIX_PIXEL9A_TARGET_DIR="$work/target-no-recovery" run no_recovery '' preflight --allow-unsigned-avb
check "refuses: missing verified recovery artifact" sh -c "[ $status -eq 1 ] && grep -q '^REFUSING: missing verified factory recovery artifact' '$work/no_recovery/stderr'"
check "refuses: recovery gate fails before any device call" [ ! -s "$calls" ]

STUB_FASTBOOT_DEVICES="$serial fastboot usb:1
OTHER-SERIAL fastboot usb:2
" run two_fastboot '' preflight --allow-unsigned-avb
check "refuses: two Fastboot devices" sh -c "[ $status -eq 1 ] && grep -q 'found 2; refusing ambiguous' '$work/two_fastboot/stderr'"

STUB_ADB_DEVICES="OTHER device usb:2
" run adb_and_fastboot '' preflight --allow-unsigned-avb
check "refuses: simultaneous ADB and Fastboot devices" sh -c "[ $status -eq 1 ] && grep -q '^REFUSING: expected zero ADB devices, found 1' '$work/adb_and_fastboot/stderr'"

STUB_FASTBOOT_DEVICES="" run no_device '' preflight --allow-unsigned-avb
check "refuses: no device" sh -c "[ $status -eq 1 ] && grep -q 'found 0; refusing ambiguous or absent' '$work/no_device/stderr'"

# --- boot-temp -----------------------------------------------------------------
run boot_temp_wrong 'CONFIRM SOMETHING ELSE
' boot-temp --allow-unsigned-avb
check "boot-temp: wrong phrase is refused" sh -c "[ $status -eq 1 ] && grep -q 'confirmation did not match; no mutation performed' '$work/boot_temp_wrong/stderr'"
check "boot-temp: wrong phrase issues zero mutations" [ "$(mutations)" -eq 0 ]

run boot_temp_ok "CONFIRM PIXEL 9A TEMPORARY BOOT boot.img@$boot_short
" boot-temp --allow-unsigned-avb
check "boot-temp: exact phrase succeeds" [ "$status" -eq 0 ]
check "boot-temp: the only mutation is one boot" sh -c "[ \"\$(grep -cE '^fastboot${tab}-s${tab}[^${tab}]*${tab}boot${tab}' '$calls')\" -eq 1 ] && [ \"$(mutations)\" -eq 1 ]"
check "boot-temp: announces the mutation with the exact image" grep -Fq "mutation=fastboot -s $serial boot $image_dir/boot.img" "$work/boot_temp_ok/stdout"
check "boot-temp: result recorded" grep -q '^result=temporary-boot-issued$' "$work/boot_temp_ok/stdout"
check "boot-temp: device leaving Fastboot is observed" grep -q '^device_left_fastboot=yes$' "$work/boot_temp_ok/stdout"
check "boot-temp: header v4 limitation is stated" grep -q '^limitation=header v4 temporary boot uses the vendor_boot, init_boot, and dtbo currently flashed' "$work/boot_temp_ok/stdout"

STUB_START_MODE=fastbootd run boot_temp_fastbootd "CONFIRM PIXEL 9A TEMPORARY BOOT boot.img@$boot_short
" boot-temp --allow-unsigned-avb
check "boot-temp: refuses to run from fastbootd" sh -c "[ $status -eq 1 ] && grep -q 'temporary boot requires the bootloader Fastboot mode' '$work/boot_temp_fastbootd/stderr'"
check "boot-temp: fastbootd refusal issues zero mutations" [ "$(mutations)" -eq 0 ]
evidence=$(find "$work/boot_temp_ok/pixel-9a/flash" -name 'boot-temp-*.txt' -type f | head -1)
check "boot-temp: evidence transcript written" [ -n "$evidence" ]
check "boot-temp: evidence records boot.img hash and exit status" sh -c "grep -q '^boot_img_sha256=$boot_hash$' '$evidence' && grep -q '^boot_exit_status=0$' '$evidence' && grep -q '^result=temporary-boot-issued$' '$evidence'"
check "privacy: boot-temp transcript contains no serial" env SERIAL="$serial" EVIDENCE="$evidence" sh -c '! grep -Fq "$SERIAL" "$EVIDENCE"'
check "privacy: boot-temp transcript carries the hashed marker" grep -q '^serial=serial-sha256:[0-9a-f]*$' "$evidence"

# --- flash gate ----------------------------------------------------------------
run flash_no_slot 'CONFIRM PIXEL 9A FLASH KERNEL STAGE TO SLOT B
' flash --allow-unsigned-avb
check "flash: persistent mode requires an explicit slot" sh -c "[ $status -eq 1 ] && grep -q \"^REFUSING: persistent flash requires an explicit --slot a|b\" '$work/flash_no_slot/stderr'"
check "flash: missing slot issues zero mutations" [ "$(mutations)" -eq 0 ]

run flash_active_slot 'CONFIRM PIXEL 9A FLASH KERNEL STAGE TO SLOT A
' flash --slot a --allow-unsigned-avb
check "flash: refuses the active slot" sh -c "[ $status -eq 1 ] && grep -q \"^REFUSING: persistent flash refuses the active slot 'a'\" '$work/flash_active_slot/stderr'"
check "flash: active-slot refusal issues zero mutations" [ "$(mutations)" -eq 0 ]

run flash_no_evidence 'CONFIRM PIXEL 9A FLASH KERNEL STAGE TO SLOT B
' flash --slot b --allow-unsigned-avb
check "flash: refuses without temporary-boot evidence" sh -c "[ $status -eq 1 ] && grep -q '^REFUSING: image-layout policy requires a temporary boot before a persistent write' '$work/flash_no_evidence/stderr'"
check "flash: refusal issues zero mutations" [ "$(mutations)" -eq 0 ]

sed "s/^boot_img_sha256=.*/boot_img_sha256=$(printf 'f%.0s' $(seq 1 64))/" "$evidence" >"$work/stale-evidence.txt"
FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE="$work/stale-evidence.txt" run flash_stale_evidence 'CONFIRM PIXEL 9A FLASH KERNEL STAGE TO SLOT B
' flash --slot b --allow-unsigned-avb
check "flash: refuses evidence recorded for a different boot.img" sh -c "[ $status -eq 1 ] && grep -q 'records a different boot.img hash' '$work/flash_stale_evidence/stderr'"
check "flash: stale evidence issues zero mutations" [ "$(mutations)" -eq 0 ]

FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE="$evidence" run flash_wrong_phrase 'CONFIRM PIXEL 9A FLASH KERNEL STAGE TO SLOT A
' flash --slot b --allow-unsigned-avb
check "flash: wrong phrase is refused" sh -c "[ $status -eq 1 ] && grep -q 'confirmation did not match; no mutation performed' '$work/flash_wrong_phrase/stderr'"
check "flash: wrong phrase issues zero mutations" [ "$(mutations)" -eq 0 ]

FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE="$evidence" run flash_skip_plain 'CONFIRM PIXEL 9A FLASH KERNEL STAGE TO SLOT B
' flash --slot b --allow-unsigned-avb --skip-temp-boot-gate
check "flash: --skip-temp-boot-gate demands the skip in the phrase" sh -c "[ $status -eq 1 ] && grep -q 'Type exactly: CONFIRM PIXEL 9A FLASH KERNEL STAGE TO SLOT B WITHOUT TEMP BOOT GATE' '$work/flash_skip_plain/stderr'"
check "flash: plain phrase with skip flag issues zero mutations" [ "$(mutations)" -eq 0 ]

make_images "$work/images-no-rootfs"
rm -f "$work/images-no-rootfs/rootfs.img"
sed -i.bak '/artifact\trootfs_image\t/d' "$work/images-no-rootfs/boot-manifest.tsv"
FERRIX_PIXEL9A_BOOT_OUT="$work/images-no-rootfs" FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE="$evidence" run flash_rootfs '' flash --stage rootfs --slot b --allow-unsigned-avb
check "flash: rootfs stage refuses without a flashable rootfs image" sh -c "[ $status -eq 1 ] && grep -q '^REFUSING: no flashable rootfs image in manifest (rootfs.img); build it first' '$work/flash_rootfs/stderr'"
check "flash: rootfs refusal issues zero mutations" [ "$(mutations)" -eq 0 ]

FERRIX_PIXEL9A_BOOT_OUT="$work/images-no-rootfs" FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE="$evidence" run flash_all_refuses 'CONFIRM PIXEL 9A FLASH KERNEL STAGE TO SLOT B
' flash --stage all --slot b --allow-unsigned-avb
check "flash: --stage all refuses before the kernel stage when rootfs is unavailable" sh -c "[ $status -eq 1 ] && [ $(mutations) -eq 0 ]"

# --- flash kernel stage: verbatim ordered command list -------------------------
FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE="$evidence" run flash_ok 'CONFIRM PIXEL 9A FLASH KERNEL STAGE TO SLOT B
' flash --slot b --allow-unsigned-avb
check "flash: kernel stage succeeds with evidence and exact phrase" [ "$status" -eq 0 ]
check "flash: result is flashed-not-booted" grep -q '^result=flashed-not-booted$' "$work/flash_ok/stdout"
check "flash: reminds that bootable evidence needs an observed Linux boot" grep -q '^reminder=bootable evidence still requires an observed Linux boot' "$work/flash_ok/stdout"

expected="$work/expected-calls.log"
line() { printf '%s\t' "$@" >>"$expected"; printf '\n' >>"$expected"; }
identity() {
  line adb devices -l
  line fastboot devices -l
  for var in product variant hw-revision unlocked secure current-slot slot-count is-userspace serialno; do
    line fastboot -s "$serial" getvar "$var"
  done
}
recheck() {
  line adb devices -l
  line fastboot devices -l
  for var in product variant hw-revision unlocked serialno secure is-userspace current-slot slot-count; do
    line fastboot -s "$serial" getvar "$var"
  done
}
wait_and_probe() {
  line adb devices -l
  line fastboot devices -l
  line fastboot -s "$serial" getvar is-userspace
}
: >"$expected"
identity
for part in boot init_boot vendor_boot vendor_kernel_boot dtbo vbmeta vbmeta_system vbmeta_vendor; do
  recheck
  line fastboot -s "$serial" flash "${part}_b" "$image_dir/$part.img"
done
recheck
line fastboot -s "$serial" reboot fastboot
wait_and_probe
for part in vendor_dlkm system_dlkm; do
  recheck
  line fastboot -s "$serial" flash "${part}_b" "$image_dir/$part.img"
done
recheck
line fastboot -s "$serial" reboot bootloader
wait_and_probe
check "flash: call log matches the exact ordered command list (with fastbootd hop)" cmp -s "$expected" "$calls"
cmp -s "$expected" "$calls" || diff "$expected" "$calls" || true
check "flash: exactly ten flash commands and two reboots" sh -c "[ \$(grep -cE '^fastboot${tab}-s${tab}[^${tab}]*${tab}flash${tab}' '$calls') -eq 10 ] && [ \$(grep -cE '^fastboot${tab}-s${tab}[^${tab}]*${tab}reboot${tab}' '$calls') -eq 2 ]"
flash_transcript=$(find "$work/flash_ok/pixel-9a/flash" -name 'flash-*.txt' -type f | head -1)
check "flash: transcript written" [ -n "$flash_transcript" ]
check "flash: transcript lists partition, slot, hash, and status per write" grep -q "^flashed=partition=boot_b slot=b sha256=$boot_hash status=0$" "$flash_transcript"
check "flash: transcript records dlkm writes" grep -q '^flashed=partition=system_dlkm_b slot=b sha256=[0-9a-f]* status=0$' "$flash_transcript"
check "privacy: flash transcript contains no serial" env SERIAL="$serial" T="$flash_transcript" sh -c '! grep -Fq "$SERIAL" "$T"'
check "privacy: flash transcript redacts mutation lines" grep -q '^mutation=fastboot -s serial-sha256:[0-9a-f]* flash boot_b ' "$flash_transcript"

# --- flash kernel stage to the inactive slot -----------------------------------
FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE="$evidence" run flash_slot_b 'CONFIRM PIXEL 9A FLASH KERNEL STAGE TO SLOT B
' flash --slot b --allow-unsigned-avb
check "flash: --slot b flashes only _b partitions" sh -c "[ $status -eq 0 ] && [ \$(grep -cE '${tab}flash${tab}[a-z_]*_b${tab}' '$calls') -eq 10 ] && ! grep -qE '${tab}flash${tab}[a-z_]*_a${tab}' '$calls'"
check "flash: inactive slot warning states no set_active is issued" grep -q '^slot_warning=target slot is inactive' "$work/flash_slot_b/stdout"

# --- rootfs stage with a fabricated flashable image and deviceinfo selection ----
make_images "$work/images-rootfs"
printf 'ferrix-rootfs-image\n' >"$work/images-rootfs/rootfs.img"
rootfs_image_bytes=$(bytes "$work/images-rootfs/rootfs.img")
rootfs_image_hash=$(sha256 "$work/images-rootfs/rootfs.img")
awk -F "$tab" '$1 != "artifact" || $2 != "rootfs_image" { print }' \
  "$work/images-rootfs/boot-manifest.tsv" >"$work/images-rootfs/boot-manifest.tsv.tmp"
printf 'artifact\trootfs_image\trootfs.img\t%s\t%s\tgenerated-test\t/synthetic/staging/rootfs.img\n' \
  "$rootfs_image_bytes" "$rootfs_image_hash" >>"$work/images-rootfs/boot-manifest.tsv.tmp"
mv "$work/images-rootfs/boot-manifest.tsv.tmp" "$work/images-rootfs/boot-manifest.tsv"
make_target "$work/target-rootfs" yes
printf 'deviceinfo_flash_fastboot_partition_rootfs="userdata"\n' >>"$work/target-rootfs/pmos/device-google-tegu/deviceinfo"
rootfs_dir=$(CDPATH= cd -- "$work/images-rootfs" && pwd)
export FERRIX_PIXEL9A_TARGET_DIR="$work/target-rootfs" FERRIX_PIXEL9A_BOOT_OUT="$rootfs_dir" FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE="$evidence"
unset FERRIX_PIXEL9A_PARTITION_INVENTORY || true
run flash_rootfs_missing_inventory 'CONFIRM PIXEL 9A FLASH ROOTFS STAGE TO PARTITION USERDATA
' flash --stage rootfs --slot b --allow-unsigned-avb
missing_inventory_mutations=$(mutations)
check "rootfs: refuses without a matching read-only partition inventory" sh -c "[ $status -eq 1 ] && grep -q 'rootfs flash requires FERRIX_PIXEL9A_PARTITION_INVENTORY' '$work/flash_rootfs_missing_inventory/stderr' && [ $missing_inventory_mutations -eq 0 ]"

export FERRIX_PIXEL9A_TARGET_DIR="$work/target-rootfs" FERRIX_PIXEL9A_BOOT_OUT="$rootfs_dir" FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE="$evidence"
export FERRIX_PIXEL9A_PARTITION_INVENTORY="$partition_inventory"
run flash_rootfs_ok 'CONFIRM PIXEL 9A FLASH ROOTFS STAGE TO PARTITION USERDATA
' flash --stage rootfs --slot b --allow-unsigned-avb
check "rootfs: stage succeeds only with rootfs.img, deviceinfo partition, and live inventory" [ "$status" -eq 0 ]
: >"$expected"
identity
recheck
line fastboot -s "$serial" flash userdata "$rootfs_dir/rootfs.img"
check "rootfs: exactly one flash to userdata" cmp -s "$expected" "$calls"
cmp -s "$expected" "$calls" || diff "$expected" "$calls" || true

printf 'deviceinfo_flash_fastboot_partition_rootfs="super"\n' >>"$work/target-rootfs/pmos/device-google-tegu/deviceinfo"
sed -i.bak '/partition_rootfs="userdata"/d' "$work/target-rootfs/pmos/device-google-tegu/deviceinfo"
export FERRIX_PIXEL9A_TARGET_DIR="$work/target-rootfs" FERRIX_PIXEL9A_BOOT_OUT="$rootfs_dir" FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE="$evidence"
run flash_rootfs_super 'CONFIRM PIXEL 9A FLASH ROOTFS STAGE TO PARTITION SUPER
' flash --stage rootfs --slot b --allow-unsigned-avb
check "rootfs: a deviceinfo partition outside the allow-list is refused" sh -c "[ $status -eq 1 ] && grep -q \"rootfs partition 'super' is outside this workflow's allow-list\" '$work/flash_rootfs_super/stderr' && [ $(mutations) -eq 0 ]"

# The admitted physical target currently carries these exact non-passing gate
# values. Keep a synthetic copy with the same state and prove that no Ferrix
# mutation reaches the device even when all image/identity checks pass.
cp -R "$work/target" "$work/target-gates-current"
sed -i.bak \
  -e 's/image_compatibility = "passed"/image_compatibility = "partial"/' \
  -e 's/backup = "verified"/backup = "pending"/' \
  -e 's/recovery_procedure = "tested"/recovery_procedure = "documented-not-tested"/' \
  "$work/target-gates-current/manifest.toml"
FERRIX_PIXEL9A_TARGET_DIR="$work/target-gates-current" run pending_target_gates '' boot-temp --allow-unsigned-avb
check "gates: pending/partial/documented-not-tested target refuses Ferrix mutation" \
  sh -c "[ $status -eq 1 ] && grep -q \"^REFUSING: backup gate is 'pending'\" '$work/pending_target_gates/stderr'"
check "gates: pending target issues zero mutations" [ "$(mutations)" -eq 0 ]

if [ "$fails" -eq 0 ]; then
  echo '{"event":"pixel-9a-flash-test","status":"passed"}'
else
  printf '{"event":"pixel-9a-flash-test","status":"failed","failures":%s}\n' "$fails"
  exit 1
fi
