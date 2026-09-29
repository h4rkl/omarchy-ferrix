#!/bin/sh
set -eu

# Target-scoped, confirmation-gated Pixel 9a bootloader unlock.
# The preflight is read-only. The execution mode is the only workflow in this
# repository that may issue adb reboot bootloader and fastboot flashing unlock;
# it verifies the exact device and recovery artifacts before each
# state-changing command.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
. "$root/tools/pixel-9a/data-safety.sh"
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
pins="$target/recovery.pins"
recovery="$target/recovery"
target_manifest="$target/manifest.toml"

expected_manufacturer=Google
expected_model="Pixel 9a"
expected_product=tegu
expected_revision=MP1.0
expected_sku=GXQ96

usage() {
  cat >&2 <<'EOF'
usage:
  tools/pixel-9a/unlock-bootloader.sh --preflight
  tools/pixel-9a/unlock-bootloader.sh --confirm-unlock

--preflight       read-only identity and recovery checks
--confirm-unlock  repeat the checks, require typed confirmations, then unlock
EOF
  exit 2
}

die() {
  echo "REFUSING: $*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing $1; install Android Platform Tools" >&2
    exit 1
  }
}

[ "$#" -eq 1 ] || usage
case "$1" in
  --preflight) mode=preflight ;;
  --confirm-unlock) mode=unlock ;;
  *) usage ;;
esac

need adb
need fastboot
if command -v shasum >/dev/null 2>&1; then
  hash_tool=shasum
else
  need sha256sum
  hash_tool=sha256sum
fi

sha256_of() {
  if [ "$hash_tool" = shasum ]; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    sha256sum "$1" | awk '{ print $1 }'
  fi
}

pin() {
  kind=$1
  column=$2
  awk -F '\t' -v wanted="$kind" -v number="$column" \
    '$1 == wanted { print $number; exit }' "$pins"
}

verify_artifact() {
  kind=$1
  filename=$(pin "$kind" 4)
  expected_hash=$(pin "$kind" 6)
  expected_bytes=$(pin "$kind" 7)
  [ -n "$filename" ] && [ -n "$expected_hash" ] && [ -n "$expected_bytes" ] || {
    die "malformed $kind recovery pin in $pins"
  }

  path="$recovery/$filename"
  [ -f "$path" ] || die "missing verified $kind recovery artifact: $path"
  [ ! -L "$path" ] || die "$kind recovery artifact must not be a symlink: $path"
  actual_bytes=$(wc -c < "$path" | tr -d '[:space:]')
  actual_hash=$(sha256_of "$path")
  [ "$actual_bytes" = "$expected_bytes" ] || {
    die "$kind recovery size mismatch: expected $expected_bytes, got $actual_bytes"
  }
  [ "$actual_hash" = "$expected_hash" ] || {
    die "$kind recovery SHA-256 mismatch: expected $expected_hash, got $actual_hash"
  }

  printf 'recovery_%s_path=%s\n' "$kind" "$path"
  printf 'recovery_%s_bytes=%s\n' "$kind" "$actual_bytes"
  printf 'recovery_%s_sha256=%s\n' "$kind" "$actual_hash"
  if [ "$kind" = factory ]; then
    factory_path=$path
    factory_hash=$actual_hash
  else
    ota_path=$path
    ota_hash=$actual_hash
  fi
}

read_all_gates() {
  gate_value() {
    key=$1
    awk -v wanted="$key" '
      /^\[gates\][[:space:]]*$/ { in_gates=1; next }
      /^\[/ { in_gates=0 }
      in_gates && $0 ~ "^[[:space:]]*" wanted "[[:space:]]*=" {
        value=$0
        sub(/^[^=]*=[[:space:]]*/, "", value)
        gsub(/[[:space:]"]/, "", value)
        print value
        found=1
        exit
      }
      END { if (!found) exit 1 }
    ' "$target_manifest"
  }
  identity_gate=$(gate_value identity) || die "target manifest has no [gates].identity entry: $target_manifest"
  image_compatibility_gate=$(gate_value image_compatibility) || die "target manifest has no [gates].image_compatibility entry: $target_manifest"
  backup_gate=$(gate_value backup) || die "target manifest has no [gates].backup entry: $target_manifest"
  data_policy_gate=$(gate_value data_policy || true)
  recovery_procedure_gate=$(gate_value recovery_procedure) || die "target manifest has no [gates].recovery_procedure entry: $target_manifest"
  confirmation_gate=$(gate_value per_run_confirmation) || die "target manifest has no [gates].per_run_confirmation entry: $target_manifest"
  [ -n "$identity_gate" ] && [ -n "$image_compatibility_gate" ] && [ -n "$backup_gate" ] && [ -n "$recovery_procedure_gate" ] && [ -n "$confirmation_gate" ] ||
    die "target manifest has an empty provisioning gate: $target_manifest"
  printf 'gate_identity=%s\n' "$identity_gate"
  printf 'gate_image_compatibility=%s\n' "$image_compatibility_gate"
  printf 'gate_recovery_procedure=%s\n' "$recovery_procedure_gate"
  printf 'backup_gate=%s\n' "$backup_gate"
  printf 'data_policy=%s\n' "$data_policy_gate"
  printf 'per_run_confirmation=%s\n' "$confirmation_gate"
}

require_backup_gate() {
  read_all_gates
  [ "$identity_gate" = passed ] || die "identity gate is '$identity_gate'; exact target identity must be approved before unlocking"
  [ "$confirmation_gate" = required ] || die "per-run confirmation gate is '$confirmation_gate'; manifest must require typed confirmation"
  pixel9a_data_safety_check "$backup_gate" "$data_policy_gate" || die "$pixel9a_data_safety_reason"
  printf 'data_safety_gate=%s\n' "$pixel9a_data_safety_status"
}

require_mutation_gates() {
  require_backup_gate
  case "$image_compatibility_gate" in
    passed|verified) ;;
    *) die "image compatibility gate is '$image_compatibility_gate'; exact target image inputs are not approved for unlocking" ;;
  esac
  case "$recovery_procedure_gate" in
    tested|passed|verified) ;;
    *) die "recovery procedure gate is '$recovery_procedure_gate'; exact-target recovery procedure is not tested" ;;
  esac
}

count_rows() {
  printf '%s\n' "$1" | awk 'NF { count++ } END { print count + 0 }'
}

adb_prop() {
  adb -s "$adb_serial" shell getprop "$1" </dev/null | tr -d '\r'
}

fastboot_getvar() {
  variable=$1
  fastboot -s "$fastboot_serial" getvar "$variable" 2>&1 | awk -v key="$variable" '
    index($0, "(bootloader) " key ":") {
      line = $0
      sub(".*\\(bootloader\\) " key ":[[:space:]]*", "", line)
      print line
      exit
    }
    index($0, key ":") {
      line = $0
      sub(".*" key ":[[:space:]]*", "", line)
      print line
      exit
    }
  '
}

read_adb_identity() {
  adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
  fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
  adb_count=$(count_rows "$adb_rows")
  fastboot_count=$(count_rows "$fastboot_rows")
  [ "$((adb_count + fastboot_count))" -eq 1 ] || {
    die "ambiguous transports before unlock: adb=$adb_count fastboot=$fastboot_count"
  }
  [ "$adb_count" -eq 1 ] || {
    die "expected exactly one authorized ADB device before unlock"
  }

  adb_serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1 }')
  adb_state=$(printf '%s\n' "$adb_rows" | awk '{ print $2 }')
  [ "$adb_state" = device ] || die "ADB state is '$adb_state', not 'device'"

  manufacturer=$(adb_prop ro.product.manufacturer)
  model=$(adb_prop ro.product.model)
  product=$(adb_prop ro.product.name)
  codename=$(adb_prop ro.product.device)
  board=$(adb_prop ro.product.board)
  hardware_revision=$(adb_prop ro.boot.hardware.revision)
  sku=$(adb_prop ro.boot.hardware.sku)
  bootloader_version=$(adb_prop ro.boot.bootloader)
  current_slot=$(adb_prop ro.boot.slot_suffix)
  slot_count=$(adb_prop ro.boot.slot_count)
  verified_boot=$(adb_prop ro.boot.verifiedbootstate)
  device_state=$(adb_prop ro.boot.vbmeta.device_state)
  flash_locked=$(adb_prop ro.boot.flash.locked)
  dynamic_partitions=$(adb_prop ro.boot.dynamic_partitions)
  boot_devices=$(adb_prop ro.boot.boot_devices)

  [ "$manufacturer" = "$expected_manufacturer" ] || die "manufacturer mismatch: $manufacturer"
  [ "$model" = "$expected_model" ] || die "model mismatch: $model"
  [ "$product" = "$expected_product" ] || die "product mismatch: $product"
  [ "$codename" = "$expected_product" ] || die "codename mismatch: $codename"
  [ "$board" = "$expected_product" ] || die "board mismatch: $board"
  [ "$hardware_revision" = "$expected_revision" ] || die "hardware revision mismatch: $hardware_revision"
  [ "$sku" = "$expected_sku" ] || die "SKU mismatch: $sku"
  [ -n "$bootloader_version" ] || die "bootloader version is unavailable"
  case "$current_slot" in
    _a|_b) ;;
    *) die "unexpected current slot: $current_slot" ;;
  esac
  [ "$device_state" = locked ] || die "bootloader device state is '$device_state', not locked"
  [ "$flash_locked" = 1 ] || die "bootloader flash lock property is '$flash_locked', not 1"
  [ "$verified_boot" = green ] || die "verified boot state is '$verified_boot', not green"
  [ "$dynamic_partitions" = true ] || die "dynamic partitions property is '$dynamic_partitions', not true"
}

print_adb_summary() {
  printf 'operation=bootloader-unlock\n'
  printf 'transport=adb\n'
  printf 'serial=%s\n' "$adb_serial"
  printf 'manufacturer=%s\n' "$manufacturer"
  printf 'model=%s\n' "$model"
  printf 'product=%s\n' "$product"
  printf 'codename=%s\n' "$codename"
  printf 'board=%s\n' "$board"
  printf 'hardware_revision=%s\n' "$hardware_revision"
  printf 'sku=%s\n' "$sku"
  printf 'bootloader_version=%s\n' "$bootloader_version"
  printf 'current_slot=%s\n' "$current_slot"
  printf 'slot_count=%s\n' "$slot_count"
  printf 'verified_boot=%s\n' "$verified_boot"
  printf 'device_state=%s\n' "$device_state"
  printf 'flash_locked=%s\n' "$flash_locked"
  printf 'dynamic_partitions=%s\n' "$dynamic_partitions"
  printf 'boot_devices=%s\n' "$boot_devices"
  printf 'partition_target=bootloader (fastboot flashing unlock)\n'
  printf 'input_image_sha256=none (unlock-only operation)\n'
  printf 'factory_recovery_sha256=%s\n' "$factory_hash"
  printf 'ota_recovery_sha256=%s\n' "$ota_hash"
}

preflight() {
  verify_artifact factory
  verify_artifact ota
  read_all_gates
  read_adb_identity
  print_adb_summary
  printf 'preflight=passed\n'
}

confirm_phrase() {
  phrase=$1
  printf '%s\n' "$2" >&2
  printf 'Type exactly: %s\n> ' "$phrase" >&2
  IFS= read -r confirmation || die "confirmation was not provided"
  [ "$confirmation" = "$phrase" ] || die "confirmation did not match; no mutation performed"
}

recheck_adb_transport() {
  adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
  fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
  adb_count=$(count_rows "$adb_rows")
  fastboot_count=$(count_rows "$fastboot_rows")
  [ "$adb_count" -eq 1 ] && [ "$fastboot_count" -eq 0 ] || {
    die "transport changed before reboot: adb=$adb_count fastboot=$fastboot_count"
  }
  recheck_serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1 }')
  [ "$recheck_serial" = "$adb_serial" ] || {
    die "ADB serial changed before reboot"
  }
  [ "$(adb -s "$adb_serial" get-state </dev/null | tr -d '\r')" = device ] || {
    die "ADB device is no longer in device state"
  }
  [ "$(adb_prop ro.product.name)" = "$expected_product" ] || die "ADB product changed before reboot"
  [ "$(adb_prop ro.product.device)" = "$expected_product" ] || die "ADB codename changed before reboot"
  [ "$(adb_prop ro.product.board)" = "$expected_product" ] || die "ADB board changed before reboot"
  [ "$(adb_prop ro.boot.hardware.revision)" = "$expected_revision" ] || die "ADB hardware revision changed before reboot"
  [ "$(adb_prop ro.boot.hardware.sku)" = "$expected_sku" ] || die "ADB SKU changed before reboot"
  recheck_slot=$(adb_prop ro.boot.slot_suffix)
  case "$recheck_slot" in _a|_b) ;; *) die "ADB current slot changed before reboot: $recheck_slot" ;; esac
  [ "$(adb_prop ro.boot.slot_count)" = 2 ] || die "ADB slot count changed before reboot"
  verify_artifact factory
  verify_artifact ota
}

wait_for_fastboot() {
  attempt=0
  while [ "$attempt" -lt 30 ]; do
    fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
    fastboot_count=$(count_rows "$fastboot_rows")
    if [ "$fastboot_count" -gt 1 ]; then
      die "ambiguous Fastboot transports after reboot: $fastboot_count"
    fi
    if [ "$fastboot_count" -eq 1 ]; then
      fastboot_serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
      [ "$fastboot_serial" = "$adb_serial" ] || {
        die "Fastboot serial does not match the authorized ADB serial"
      }
      return 0
    fi
    attempt=$((attempt + 1))
    sleep 1
  done
  die "the authorized device did not appear in Fastboot within 30 seconds"
}

read_fastboot_identity() {
  fastboot_product=$(fastboot_getvar product)
  fastboot_variant=$(fastboot_getvar variant)
  fastboot_revision=$(fastboot_getvar hw-revision)
  fastboot_slot=$(fastboot_getvar current-slot)
  fastboot_unlocked=$(fastboot_getvar unlocked)
  fastboot_serialno=$(fastboot_getvar serialno)
  fastboot_bootloader=$(fastboot_getvar version-bootloader)

  [ "$fastboot_product" = "$expected_product" ] || {
    die "Fastboot product mismatch: $fastboot_product"
  }
  [ "$fastboot_variant" = "$expected_sku" ] || {
    die "Fastboot SKU/variant mismatch: $fastboot_variant"
  }
  [ "$fastboot_revision" = "$expected_revision" ] || {
    die "Fastboot hardware revision mismatch: $fastboot_revision"
  }
  case "$fastboot_slot" in
    a|b|_a|_b) ;;
    *) die "unexpected Fastboot current slot: $fastboot_slot" ;;
  esac
  [ "$fastboot_unlocked" = no ] || {
    die "Fastboot reports unlocked='$fastboot_unlocked'; refusing a redundant unlock"
  }
  if [ -n "$fastboot_serialno" ] && [ "$fastboot_serialno" != "$adb_serial" ]; then
    die "Fastboot getvar serialno does not match the authorized ADB serial"
  fi
  [ -n "$fastboot_bootloader" ] || die "Fastboot bootloader version is unavailable"

  printf 'transport=fastboot\n'
  printf 'serial=%s\n' "$fastboot_serial"
  printf 'product=%s\n' "$fastboot_product"
  printf 'variant=%s\n' "$fastboot_variant"
  printf 'sku=%s\n' "$fastboot_variant"
  printf 'hardware_revision=%s\n' "$fastboot_revision"
  printf 'current_slot=%s\n' "$fastboot_slot"
  printf 'unlocked=%s\n' "$fastboot_unlocked"
  printf 'serialno=%s\n' "$fastboot_serialno"
  printf 'bootloader_version=%s\n' "$fastboot_bootloader"
  printf 'partition_target=bootloader (fastboot flashing unlock)\n'
  printf 'input_image_sha256=none (unlock-only operation)\n'
  printf 'factory_recovery_sha256=%s\n' "$factory_hash"
  printf 'ota_recovery_sha256=%s\n' "$ota_hash"
  printf 'fastboot_preflight=passed\n'
}

preflight
[ "$mode" = unlock ] || exit 0
require_mutation_gates

confirm_phrase \
  "CONFIRM PIXEL 9A REBOOT AND UNLOCK (WIPES ALL USER DATA)" \
  "The next command reboots the exact device into Fastboot. Unlocking will erase all user data."
recheck_adb_transport
printf 'mutation=adb reboot bootloader\n'
adb -s "$adb_serial" reboot bootloader

wait_for_fastboot
read_fastboot_identity
confirm_phrase \
  "CONFIRM PIXEL 9A BOOTLOADER UNLOCK (WIPES ALL USER DATA)" \
  "The exact Fastboot device and recovery hashes are verified. The next command unlocks the bootloader and wipes user data."
recheck_fastboot_transport() {
  fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
  fastboot_count=$(count_rows "$fastboot_rows")
  [ "$fastboot_count" -eq 1 ] || die "Fastboot transport changed before unlock: found $fastboot_count devices"
  recheck_serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
  [ "$recheck_serial" = "$fastboot_serial" ] || die "Fastboot serial changed before unlock"
  [ "$(fastboot_getvar product)" = "$expected_product" ] || die "Fastboot product changed before unlock"
  [ "$(fastboot_getvar variant)" = "$expected_sku" ] || die "Fastboot SKU/variant changed before unlock"
  [ "$(fastboot_getvar hw-revision)" = "$expected_revision" ] || die "Fastboot hardware revision changed before unlock"
  recheck_slot=$(fastboot_getvar current-slot)
  case "$recheck_slot" in a|b|_a|_b) ;; *) die "Fastboot current slot changed before unlock: $recheck_slot" ;; esac
  [ "$(fastboot_getvar unlocked)" = no ] || die "Fastboot is no longer locked before unlock"
  [ "$(fastboot_getvar serialno)" = "$fastboot_serial" ] || die "Fastboot serialno changed before unlock"
  verify_artifact factory
  verify_artifact ota
}
recheck_fastboot_transport
printf 'partition_target=bootloader (fastboot flashing unlock)\n'
printf 'input_image_sha256=none (unlock-only operation)\n'
printf 'factory_recovery_sha256=%s\n' "$factory_hash"
printf 'ota_recovery_sha256=%s\n' "$ota_hash"
printf 'mutation=fastboot flashing unlock\n'
fastboot -s "$fastboot_serial" flashing unlock

unlocked_after=$(fastboot_getvar unlocked)
printf 'unlocked_after=%s\n' "$unlocked_after"
[ "$unlocked_after" = yes ] || die "unlock command returned, but Fastboot does not report unlocked=yes"
printf 'result=bootloader-unlocked\n'
printf 'device remains in Fastboot; restore the verified factory image before normal use.\n'
