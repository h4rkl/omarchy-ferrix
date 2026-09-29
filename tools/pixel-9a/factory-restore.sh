#!/bin/sh
set -eu

# Target-scoped, confirmation-gated Pixel 9a Fastboot factory restore from the
# pinned, re-hashed Google factory archive. The preflight is read-only. The
# execution mode replays exactly the vendor flash-all.sh sequence (bootloader,
# reboot-bootloader, radio, reboot-bootloader, [-w] update) against one
# verified device; it re-checks the transport before every state-changing
# command. It never issues flashing lock/unlock, erase, format, set_active,
# oem, or boot commands, and it never runs the vendor script itself.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
. "$root/tools/pixel-9a/data-safety.sh"
target=${FERRIX_PIXEL9A_TARGET_DIR:-"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"}
pins=${FERRIX_PIXEL9A_PINS:-"$target/recovery.pins"}
recovery="$target/recovery"
target_manifest="$target/manifest.toml"
out_root=${FERRIX_OUT_DIR:-"$root/out"}
out="$out_root/pixel-9a/restore"
inventory=${FERRIX_PIXEL9A_FACTORY_INVENTORY:-"$out_root/pixel-9a/factory-inspection/inventory.json"}
settle_seconds=${FERRIX_PIXEL9A_SETTLE_SECONDS:-5}
fastboot_wait_seconds=${FERRIX_PIXEL9A_FASTBOOT_WAIT_SECONDS:-60}

expected_manufacturer=Google
expected_model="Pixel 9a"
expected_product=tegu
expected_revision=MP1.0
expected_sku=GXQ96

usage() {
  cat >&2 <<'EOF'
usage:
  tools/pixel-9a/factory-restore.sh --preflight [--keep-data] [--from-adb]
  tools/pixel-9a/factory-restore.sh --confirm-restore [--keep-data] [--from-adb]

--preflight        read-only artifact, vendor-sequence, transport, and
                   Fastboot identity/version checks
--confirm-restore  repeat the checks, stage the archive members, require two
                   typed confirmations, then replay the vendor sequence
--keep-data        omit -w from the fastboot update stage (default wipes user data)
--from-adb         the phone is in Android: verify its identity over ADB and,
                   after its own confirmation, reboot it to the bootloader

Requires `just pixel-9a-factory-inspect` to have recorded the pinned archive's
android-info requirements first. Only a serial-redacted transcript remains
under out/pixel-9a/restore/; staged images are removed on exit.
EOF
  exit 2
}

mode=""
wipe=1
from_adb=0
[ "$#" -ge 1 ] || usage
for argument in "$@"; do
  case "$argument" in
    --preflight) [ -z "$mode" ] || usage; mode=preflight ;;
    --confirm-restore) [ -z "$mode" ] || usage; mode=restore ;;
    --keep-data) wipe=0 ;;
    --from-adb) from_adb=1 ;;
    *) usage ;;
  esac
done
[ -n "$mode" ] || usage

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing $1; install Android Platform Tools and Info-ZIP unzip" >&2
    exit 1
  }
}

need adb
need fastboot
need unzip
need awk
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

sha256_prefix() {
  if [ "$hash_tool" = shasum ]; then
    printf '%s' "$1" | shasum -a 256 | cut -c1-12
  else
    printf '%s' "$1" | sha256sum | cut -c1-12
  fi
}

lower() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]'
}

# Replace the device serial as a literal byte string (never as a regex).
redact_literal() {
  REDACT_NEEDLE=$1 REDACT_REPLACEMENT=$2 awk '
    BEGIN {
      needle = ENVIRON["REDACT_NEEDLE"]
      replacement = ENVIRON["REDACT_REPLACEMENT"]
    }
    {
      if (needle == "") {
        print
        next
      }
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

mkdir -p "$out"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
transcript="$out/$stamp.txt"
stage="$out/stage-$stamp"
serial=""
cleanup() {
  rm -rf "$stage"
}
trap cleanup EXIT HUP INT TERM
{
  printf '# Google Pixel 9a Fastboot factory restore (%s), %s UTC, host %s\n' "$mode" "$stamp" "$(uname -s) $(uname -m)"
  printf '# Serial numbers are replaced by a SHA-256 prefix. Never commit this file.\n'
} > "$transcript"

redact() {
  if [ -n "$serial" ]; then
    redact_literal "$serial" "serial-sha256:$(sha256_prefix "$serial")"
  else
    cat
  fi
}

record() {
  printf '%s\n' "$1" | redact >> "$transcript"
}

emit() {
  printf '%s=%s\n' "$1" "$2"
  record "$1=$2"
}

say() {
  printf '%s\n' "$1" >&2
  record "# $1"
}

die() {
  printf 'REFUSING: %s\n' "$*" >&2
  record "REFUSING: $*"
  record "transcript=$transcript"
  exit 1
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

  emit "recovery_${kind}_path" "$path"
  emit "recovery_${kind}_bytes" "$actual_bytes"
  emit "recovery_${kind}_sha256" "$actual_hash"
  if [ "$kind" = factory ]; then
    factory_path=$path
    factory_hash=$actual_hash
  else
    ota_path=$path
    ota_hash=$actual_hash
  fi
}

# A factory restore may wipe all user data (and even --keep-data rewrites the
# boot chain), so it is never allowed until the owner has verified a readable
# backup outside this repository.  The read-only preflight reports the gate;
# restore mode enforces it immediately before the first confirmation.
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
  emit gate_identity "$identity_gate"
  emit gate_image_compatibility "$image_compatibility_gate"
  emit gate_recovery_procedure "$recovery_procedure_gate"
  emit backup_gate "$backup_gate"
  emit data_policy "$data_policy_gate"
  emit per_run_confirmation "$confirmation_gate"
}

require_backup_gate() {
  read_all_gates
  [ "$identity_gate" = passed ] || die "identity gate is '$identity_gate'; exact target identity must be approved before this restore"
  [ "$confirmation_gate" = required ] || die "per-run confirmation gate is '$confirmation_gate'; manifest must require typed confirmation"
  pixel9a_data_safety_check "$backup_gate" "$data_policy_gate" || die "$pixel9a_data_safety_reason"
  emit data_safety_gate "$pixel9a_data_safety_status"
}

require_mutation_gates() {
  require_backup_gate
  case "$image_compatibility_gate" in
    passed|verified) ;;
    *) die "image compatibility gate is '$image_compatibility_gate'; exact target image inputs are not approved for this restore" ;;
  esac
  case "$recovery_procedure_gate" in
    tested|passed|verified) ;;
    *) die "recovery procedure gate is '$recovery_procedure_gate'; exact-target recovery procedure is not tested" ;;
  esac
}

# The archive's android-info.txt lives inside the multi-gigabyte nested image
# zip. The offline Rust inspection already read it without extracting the
# payload; reuse that inventory instead of re-parsing the archive here.
read_inventory_requirements() {
  [ -f "$inventory" ] || {
    die "missing factory inventory $inventory; run 'just pixel-9a-factory-inspect' first"
  }
  [ ! -L "$inventory" ] || die "factory inventory must not be a symlink: $inventory"
  grep -Fq "\"sha256\": \"$factory_hash\"" "$inventory" || {
    die "factory inventory $inventory does not record the pinned archive hash; rerun 'just pixel-9a-factory-inspect'"
  }
  required_board=$(grep -o '"require board=[^"]*"' "$inventory" | head -1 | sed 's/^"require board=//; s/"$//')
  required_bootloader=$(grep -o '"require version-bootloader=[^"]*"' "$inventory" | head -1 | sed 's/^"require version-bootloader=//; s/"$//')
  required_baseband=$(grep -o '"require version-baseband=[^"]*"' "$inventory" | head -1 | sed 's/^"require version-baseband=//; s/"$//')
  [ -n "$required_board" ] || die "factory inventory lacks a 'require board=' line"
  [ -n "$required_bootloader" ] || die "factory inventory lacks a 'require version-bootloader=' line"
  [ -n "$required_baseband" ] || die "factory inventory lacks a 'require version-baseband=' line"
  [ "$required_board" = "$expected_product" ] || die "factory archive requires board '$required_board', not $expected_product"
  emit inventory_path "$inventory"
  emit required_board "$required_board"
  emit required_bootloader "$required_bootloader"
  emit required_baseband "$required_baseband"
}

# One member of the outer archive must match each pattern.
single_member() {
  pattern=$1
  matches=$(printf '%s\n' "$members" | awk -v pattern="$pattern" '
    {
      name = $0
      sub(".*/", "", name)
      if (name ~ pattern) print $0
    }')
  [ "$(printf '%s\n' "$matches" | awk 'NF { count++ } END { print count + 0 }')" -eq 1 ] || {
    die "expected exactly one archive member matching $pattern, got: $(printf '%s' "$matches" | tr '\n' ' ')"
  }
  printf '%s\n' "$matches"
}

read_vendor_sequence() {
  members=$(unzip -Z1 "$factory_path")
  bootloader_member=$(single_member '^bootloader-.*\.img$')
  radio_member=$(single_member '^radio-.*\.img$')
  image_member=$(single_member '^image-tegu-.*\.zip$')
  flash_all_member=$(single_member '^flash-all\.sh$')
  bootloader_name=${bootloader_member##*/}
  radio_name=${radio_member##*/}
  image_name=${image_member##*/}

  vendor_sequence=$(unzip -p "$factory_path" "$flash_all_member" | tr -d '\r' | grep -E '^fastboot ' || true)
  expected_sequence=$(printf 'fastboot flash bootloader %s\nfastboot reboot-bootloader\nfastboot flash radio %s\nfastboot reboot-bootloader\nfastboot -w update %s' \
    "$bootloader_name" "$radio_name" "$image_name")
  [ "$vendor_sequence" = "$expected_sequence" ] || {
    say "vendor flash-all.sh fastboot lines:"
    printf '%s\n' "$vendor_sequence" >&2
    die "vendor flash-all.sh differs from the reviewed bootloader/radio/update sequence; review before use"
  }
  emit vendor_sequence_check matches-reviewed-sequence
  emit bootloader_member "$bootloader_member"
  emit radio_member "$radio_member"
  emit image_member "$image_member"
}

count_rows() {
  printf '%s\n' "$1" | awk 'NF { count++ } END { print count + 0 }'
}

adb_prop() {
  adb -s "$serial" shell getprop "$1" </dev/null | tr -d '\r'
}

fastboot_getvar() {
  variable=$1
  fastboot -s "$serial" getvar "$variable" 2>&1 | awk -v key="$variable" '
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

enumerate_transports() {
  adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
  fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
  adb_count=$(count_rows "$adb_rows")
  fastboot_count=$(count_rows "$fastboot_rows")
}

version_status() {
  if [ "$(lower "$1")" = "$(lower "$2")" ]; then
    printf 'already-current\n'
  else
    printf 'needed\n'
  fi
}

# Identity in Android (--from-adb only): the unlock gate's exact fields minus
# the locked/green requirements.
read_adb_identity() {
  [ "$adb_count" -eq 1 ] && [ "$fastboot_count" -eq 0 ] || {
    die "--from-adb expects exactly one ADB device and no Fastboot device: adb=$adb_count fastboot=$fastboot_count"
  }
  serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1 }')
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
  baseband_version=$(adb_prop gsm.version.baseband)
  current_slot=$(adb_prop ro.boot.slot_suffix)
  slot_count=$(adb_prop ro.boot.slot_count)
  device_state=$(adb_prop ro.boot.vbmeta.device_state)
  dynamic_partitions=$(adb_prop ro.boot.dynamic_partitions)

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
  [ "$dynamic_partitions" = true ] || die "dynamic partitions property is '$dynamic_partitions', not true"

  emit transport adb
  emit serial "$serial"
  emit serial_sha256_prefix "$(sha256_prefix "$serial")"
  emit manufacturer "$manufacturer"
  emit model "$model"
  emit product "$product"
  emit codename "$codename"
  emit board "$board"
  emit hardware_revision "$hardware_revision"
  emit sku "$sku"
  emit bootloader_version "$bootloader_version"
  emit baseband_version "$baseband_version"
  emit current_slot "$current_slot"
  emit slot_count "$slot_count"
  emit device_state "$device_state"
  emit dynamic_partitions "$dynamic_partitions"
  emit bootloader_update "$(version_status "$bootloader_version" "$required_bootloader")"
  if [ -n "$baseband_version" ]; then
    emit radio_update "$(version_status "$baseband_version" "$required_baseband")"
  else
    emit radio_update unknown-until-fastboot
  fi
}

read_fastboot_identity() {
  fastboot_product=$(fastboot_getvar product)
  fastboot_variant=$(fastboot_getvar variant)
  fastboot_revision=$(fastboot_getvar hw-revision)
  fastboot_slot=$(fastboot_getvar current-slot)
  fastboot_slot_count=$(fastboot_getvar slot-count)
  fastboot_unlocked=$(fastboot_getvar unlocked)
  fastboot_serialno=$(fastboot_getvar serialno)
  fastboot_bootloader=$(fastboot_getvar version-bootloader)
  fastboot_baseband=$(fastboot_getvar version-baseband)
  fastboot_userspace=$(fastboot_getvar is-userspace)
  fastboot_secure=$(fastboot_getvar secure)

  [ "$fastboot_product" = "$expected_product" ] || die "Fastboot product mismatch: $fastboot_product"
  [ "$fastboot_variant" = "$expected_sku" ] || die "Fastboot SKU/variant mismatch: $fastboot_variant"
  [ "$fastboot_revision" = "$expected_revision" ] || die "Fastboot hardware revision mismatch: $fastboot_revision"
  case "$fastboot_slot" in
    a|b|_a|_b) ;;
    *) die "unexpected Fastboot current slot: $fastboot_slot" ;;
  esac
  [ "$fastboot_slot_count" = 2 ] || die "Fastboot slot-count is '$fastboot_slot_count', not 2"
  [ "$fastboot_unlocked" = yes ] || die "Fastboot reports unlocked='$fastboot_unlocked'; the factory restore needs an unlocked bootloader"
  [ "$fastboot_userspace" = no ] || die "Fastboot reports is-userspace='$fastboot_userspace'; the bootloader/radio stage needs the bootloader, not fastbootd"
  if [ -n "$fastboot_serialno" ] && [ "$fastboot_serialno" != "$serial" ]; then
    die "Fastboot getvar serialno does not match the enumerated serial"
  fi
  [ -n "$fastboot_bootloader" ] || die "Fastboot bootloader version is unavailable"

  emit transport fastboot
  emit serial "$serial"
  emit serial_sha256_prefix "$(sha256_prefix "$serial")"
  emit product "$fastboot_product"
  emit variant "$fastboot_variant"
  emit sku "$fastboot_variant"
  emit hardware_revision "$fastboot_revision"
  emit current_slot "$fastboot_slot"
  emit slot_count "$fastboot_slot_count"
  emit unlocked "$fastboot_unlocked"
  emit secure "$fastboot_secure"
  emit is_userspace "$fastboot_userspace"
  emit serialno "$fastboot_serialno"
  emit bootloader_version "$fastboot_bootloader"
  emit baseband_version "$fastboot_baseband"
  emit bootloader_update "$(version_status "$fastboot_bootloader" "$required_bootloader")"
  emit radio_update "$(version_status "$fastboot_baseband" "$required_baseband")"
}

print_plan() {
  emit operation factory-restore
  if [ "$wipe" -eq 1 ]; then
    emit user_data "wipe via fastboot -w update"
  else
    emit user_data "keep via fastboot update without -w (--keep-data)"
  fi
  emit flash_slot "vendor-default(current)"
  emit partition_target "bootloader, radio, then all images in $image_name via fastboot update"
  emit input_image_path "$factory_path"
  emit input_image_sha256 "$factory_hash"
  emit recovery_artifact "$ota_path sha256:$ota_hash"
}

preflight() {
  verify_artifact factory
  verify_artifact ota
  read_inventory_requirements
  read_vendor_sequence
  read_all_gates
  enumerate_transports
  emit adb_devices "$adb_count"
  emit fastboot_devices "$fastboot_count"
  if [ "$from_adb" -eq 1 ]; then
    read_adb_identity
    emit fastboot_identity deferred-until-after-reboot
  else
    if [ "$adb_count" -ge 1 ] && [ "$fastboot_count" -eq 0 ]; then
      die "the device is visible over ADB, not Fastboot; reboot it to the bootloader yourself or rerun with --from-adb"
    fi
    [ "$adb_count" -eq 0 ] || die "ambiguous transports: adb=$adb_count fastboot=$fastboot_count"
    [ "$fastboot_count" -eq 1 ] || die "expected exactly one Fastboot device, got $fastboot_count"
    serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
    read_fastboot_identity
  fi
  print_plan
  emit preflight passed
}

confirm_phrase() {
  phrase=$1
  say "$2"
  printf 'Type exactly: %s\n> ' "$phrase" >&2
  IFS= read -r confirmation || die "confirmation was not provided"
  [ "$confirmation" = "$phrase" ] || die "confirmation did not match; no mutation performed"
  record "confirmation=accepted"
}

recheck_adb_transport() {
  # The ADB reboot is itself a mutation. Re-read the exact identity after the
  # prompt and before issuing it; the preflight values are not trusted across
  # an operator delay or cable/device replacement.
  enumerate_transports
  [ "$adb_count" -eq 1 ] && [ "$fastboot_count" -eq 0 ] || {
    die "transport changed before reboot: adb=$adb_count fastboot=$fastboot_count"
  }
  recheck_serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1 }')
  [ "$recheck_serial" = "$serial" ] || die "ADB serial changed before reboot"
  [ "$(adb -s "$serial" get-state </dev/null | tr -d '\r')" = device ] || {
    die "ADB device is no longer in device state"
  }
  recheck_product=$(adb_prop ro.product.name)
  recheck_codename=$(adb_prop ro.product.device)
  recheck_board=$(adb_prop ro.product.board)
  recheck_revision=$(adb_prop ro.boot.hardware.revision)
  recheck_sku=$(adb_prop ro.boot.hardware.sku)
  recheck_slot=$(adb_prop ro.boot.slot_suffix)
  recheck_slot_count=$(adb_prop ro.boot.slot_count)
  [ "$recheck_product" = "$expected_product" ] || die "ADB product changed before reboot: $recheck_product"
  [ "$recheck_codename" = "$expected_product" ] || die "ADB codename changed before reboot: $recheck_codename"
  [ "$recheck_board" = "$expected_product" ] || die "ADB board changed before reboot: $recheck_board"
  [ "$recheck_revision" = "$expected_revision" ] || die "ADB hardware revision changed before reboot: $recheck_revision"
  [ "$recheck_sku" = "$expected_sku" ] || die "ADB SKU changed before reboot: $recheck_sku"
  case "$recheck_slot" in _a|_b) ;; *) die "ADB current slot changed before reboot: $recheck_slot" ;; esac
  [ "$recheck_slot_count" = 2 ] || die "ADB slot count changed before reboot: $recheck_slot_count"
  verify_artifact factory
  verify_artifact ota
}

recheck_fastboot_transport() {
  enumerate_transports
  [ "$adb_count" -eq 0 ] && [ "$fastboot_count" -eq 1 ] || {
    die "transport changed before mutation: adb=$adb_count fastboot=$fastboot_count"
  }
  recheck_serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
  [ "$recheck_serial" = "$serial" ] || die "Fastboot serial changed before mutation"
}

# Re-read all identity fields that are relevant to a destructive Fastboot
# operation. The caller supplies the operation's partition target and image
# hash so the line immediately before the command is self-contained.
recheck_fastboot_identity() {
  operation_target=$1
  operation_hash=$2
  verify_artifact factory
  verify_artifact ota
  recheck_fastboot_transport
  recheck_product=$(fastboot_getvar product)
  recheck_variant=$(fastboot_getvar variant)
  recheck_revision=$(fastboot_getvar hw-revision)
  recheck_slot=$(fastboot_getvar current-slot)
  recheck_slot_count=$(fastboot_getvar slot-count)
  recheck_unlocked=$(fastboot_getvar unlocked)
  recheck_userspace=$(fastboot_getvar is-userspace)
  recheck_serialno=$(fastboot_getvar serialno)
  [ "$recheck_product" = "$expected_product" ] || die "Fastboot product changed before mutation: $recheck_product"
  [ "$recheck_variant" = "$expected_sku" ] || die "Fastboot SKU/variant changed before mutation: $recheck_variant"
  [ "$recheck_revision" = "$expected_revision" ] || die "Fastboot hardware revision changed before mutation: $recheck_revision"
  [ "$recheck_slot" = "$fastboot_slot" ] || die "Fastboot current slot changed before mutation: expected $fastboot_slot, got $recheck_slot"
  [ "$recheck_slot_count" = 2 ] || die "Fastboot slot count changed before mutation: $recheck_slot_count"
  [ "$recheck_unlocked" = yes ] || die "Fastboot unlocked state changed before mutation: $recheck_unlocked"
  [ "$recheck_userspace" = no ] || die "Fastboot is-userspace changed before mutation: $recheck_userspace"
  if [ -n "$recheck_serialno" ] && [ "$recheck_serialno" != "$serial" ]; then
    die "Fastboot serialno changed before mutation: $recheck_serialno"
  fi
  emit mutation_identity "serial_sha256_prefix=$(sha256_prefix "$serial") product=$recheck_product hardware_revision=$recheck_revision current_slot=$recheck_slot slot_count=$recheck_slot_count unlocked=$recheck_unlocked is_userspace=$recheck_userspace partition_target=$operation_target input_image_sha256=$operation_hash recovery_artifact_sha256=$factory_hash"
}

wait_for_fastboot() {
  attempt=0
  while [ "$attempt" -lt "$fastboot_wait_seconds" ]; do
    fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
    fastboot_count=$(count_rows "$fastboot_rows")
    [ "$fastboot_count" -le 1 ] || die "ambiguous Fastboot transports after reboot: $fastboot_count"
    if [ "$fastboot_count" -eq 1 ]; then
      seen_serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
      [ "$seen_serial" = "$serial" ] || die "Fastboot serial does not match the verified device"
      return 0
    fi
    attempt=$((attempt + 1))
    sleep 1
  done
  die "the verified device did not reappear in Fastboot within $fastboot_wait_seconds seconds"
}

# After each reboot-bootloader the device must still be the same bootloader
# (not fastbootd) before the next flash command.
recheck_bootloader_stage() {
  stage_product=$(fastboot_getvar product)
  stage_variant=$(fastboot_getvar variant)
  stage_revision=$(fastboot_getvar hw-revision)
  stage_slot=$(fastboot_getvar current-slot)
  stage_slot_count=$(fastboot_getvar slot-count)
  stage_unlocked=$(fastboot_getvar unlocked)
  stage_userspace=$(fastboot_getvar is-userspace)
  [ "$stage_product" = "$expected_product" ] || die "Fastboot product changed after reboot: $stage_product"
  [ "$stage_variant" = "$expected_sku" ] || die "Fastboot SKU/variant changed after reboot: $stage_variant"
  [ "$stage_revision" = "$expected_revision" ] || die "Fastboot hardware revision changed after reboot: $stage_revision"
  [ "$stage_slot" = "$fastboot_slot" ] || die "Fastboot current slot changed after reboot: $stage_slot"
  [ "$stage_slot_count" = 2 ] || die "Fastboot slot count changed after reboot: $stage_slot_count"
  [ "$stage_unlocked" = yes ] || die "Fastboot unlocked state changed after reboot: $stage_unlocked"
  [ "$stage_userspace" = no ] || die "Fastboot reports is-userspace='$stage_userspace' after reboot; expected the bootloader"
}

stage_member() {
  member=$1
  destination="$stage/${member##*/}"
  unzip -p "$factory_path" "$member" > "$destination" || die "could not extract $member"
  member_hash=$(sha256_of "$destination")
  member_bytes=$(wc -c < "$destination" | tr -d '[:space:]')
  emit "staged_${2}_path" "$destination"
  emit "staged_${2}_bytes" "$member_bytes"
  emit "staged_${2}_sha256" "$member_hash"
  case "$2" in
    bootloader) staged_bootloader_hash=$member_hash; staged_bootloader_bytes=$member_bytes ;;
    radio) staged_radio_hash=$member_hash; staged_radio_bytes=$member_bytes ;;
    image) staged_image_hash=$member_hash; staged_image_bytes=$member_bytes ;;
  esac
}

verify_staged_member() {
  image=$1
  expected_hash=$2
  expected_bytes=$3
  label=$4
  [ -f "$image" ] || die "staged $label image is missing: $image"
  [ ! -L "$image" ] || die "staged $label image must not be a symlink: $image"
  actual_bytes=$(wc -c < "$image" | tr -d '[:space:]')
  actual_hash=$(sha256_of "$image")
  [ "$actual_bytes" = "$expected_bytes" ] || die "staged $label size changed: expected $expected_bytes, got $actual_bytes"
  [ "$actual_hash" = "$expected_hash" ] || die "staged $label hash changed: expected $expected_hash, got $actual_hash"
  printf '%s\n' "$actual_hash"
}

stage_archive() {
  mkdir -p "$stage"
  say "Staging archive members under $stage (removed on exit)."
  stage_member "$bootloader_member" bootloader
  stage_bootloader="$destination"
  stage_member "$radio_member" radio
  stage_radio="$destination"
  stage_member "$image_member" image
  stage_image="$destination"

  # The extracted image zip must carry the same requirements the inventory
  # recorded; this catches a stale inventory or a tampered nested archive.
  staged_info=$(unzip -p "$stage_image" android-info.txt | tr -d '\r')
  staged_board=$(printf '%s\n' "$staged_info" | awk -F= '$1 == "require board" { print $2; exit }')
  staged_bootloader=$(printf '%s\n' "$staged_info" | awk -F= '$1 == "require version-bootloader" { print $2; exit }')
  staged_baseband=$(printf '%s\n' "$staged_info" | awk -F= '$1 == "require version-baseband" { print $2; exit }')
  [ "$staged_board" = "$required_board" ] || die "staged android-info board '$staged_board' differs from inventory '$required_board'"
  [ "$staged_bootloader" = "$required_bootloader" ] || die "staged android-info bootloader '$staged_bootloader' differs from inventory '$required_bootloader'"
  [ "$staged_baseband" = "$required_baseband" ] || die "staged android-info baseband '$staged_baseband' differs from inventory '$required_baseband'"
  emit staged_android_info matches-inventory
}

run_fastboot_flash() {
  partition=$1
  image=$2
  image_hash=$3
  image_bytes=$4
  # Recheck both the verified recovery archive and the staged member after the
  # confirmation prompt, immediately before this partition write.
  verify_artifact factory
  verify_staged_member "$image" "$image_hash" "$image_bytes" "$partition" >/dev/null
  recheck_fastboot_identity "$partition" "$image_hash"
  emit mutation "fastboot flash $partition ${image##*/}"
  status_file="$out/.status-$stamp"
  rm -f "$status_file"
  if [ "$partition" = bootloader ]; then
    {
      fastboot -s "$serial" flash bootloader "$image" </dev/null 2>&1 || printf '%s' "$?" > "$status_file"
    } | redact | tee -a "$transcript"
  else
    {
      fastboot -s "$serial" flash radio "$image" </dev/null 2>&1 || printf '%s' "$?" > "$status_file"
    } | redact | tee -a "$transcript"
  fi
  if [ -f "$status_file" ]; then
    flash_status=$(cat "$status_file")
    rm -f "$status_file"
    emit result failed
    die "fastboot flash $partition exited with status $flash_status; the device remains in Fastboot"
  fi
}

reboot_bootloader_and_wait() {
  verify_artifact factory
  recheck_fastboot_identity bootloader-control none
  emit mutation "fastboot reboot-bootloader"
  status_file="$out/.status-$stamp"
  rm -f "$status_file"
  {
    fastboot -s "$serial" reboot-bootloader </dev/null 2>&1 || printf '%s' "$?" > "$status_file"
  } | redact | tee -a "$transcript"
  if [ -f "$status_file" ]; then
    reboot_status=$(cat "$status_file")
    rm -f "$status_file"
    emit result failed
    die "fastboot reboot-bootloader exited with status $reboot_status"
  fi
  sleep "$settle_seconds"
  wait_for_fastboot
  recheck_bootloader_stage
}

preflight
say "transcript (serial redacted): $transcript"
[ "$mode" = restore ] || exit 0
require_mutation_gates

if [ "$from_adb" -eq 1 ]; then
  confirm_phrase \
    "CONFIRM PIXEL 9A REBOOT TO BOOTLOADER FOR FACTORY RESTORE" \
    "The next command reboots the exact device into Fastboot. Nothing is flashed yet."
  recheck_adb_transport
  emit mutation "adb reboot bootloader"
  adb -s "$serial" reboot bootloader
  sleep "$settle_seconds"
  wait_for_fastboot
  read_fastboot_identity
fi

stage_archive

if [ "$wipe" -eq 1 ]; then
  wipe_text="WIPES ALL USER DATA"
else
  wipe_text="KEEP USER DATA"
fi
if [ "$wipe" -eq 1 ]; then
  stage_two_text="Stage 2, fastboot -w update, will wipe all user data."
else
  stage_two_text="Stage 2, fastboot update without -w, keeps user data."
fi
confirm_phrase \
  "CONFIRM PIXEL 9A FLASH BOOTLOADER AND RADIO ($wipe_text)" \
  "Stage 1 flashes bootloader ($required_bootloader) and radio ($required_baseband) with a reboot-bootloader after each. $stage_two_text"
run_fastboot_flash bootloader "$stage_bootloader" "$staged_bootloader_hash" "$staged_bootloader_bytes"
reboot_bootloader_and_wait
bootloader_after=$(fastboot_getvar version-bootloader)
emit bootloader_version_after "$bootloader_after"
[ "$(lower "$bootloader_after")" = "$(lower "$required_bootloader")" ] || {
  die "bootloader version after flash is '$bootloader_after', not '$required_bootloader'; stopping before radio"
}

run_fastboot_flash radio "$stage_radio" "$staged_radio_hash" "$staged_radio_bytes"
reboot_bootloader_and_wait
baseband_after=$(fastboot_getvar version-baseband)
emit baseband_version_after "$baseband_after"
[ "$(lower "$baseband_after")" = "$(lower "$required_baseband")" ] || {
  die "baseband version after flash is '$baseband_after', not '$required_baseband'; stopping before update"
}

confirm_phrase \
  "CONFIRM PIXEL 9A FASTBOOT UPDATE ($wipe_text)" \
  "Stage 2 runs fastboot update with $image_name. The device reboots into fastbootd and back by itself and finally boots Android."
verify_artifact factory
verify_staged_member "$stage_image" "$staged_image_hash" "$staged_image_bytes" update >/dev/null
recheck_fastboot_identity fastboot-update "$staged_image_hash"
status_file="$out/.status-$stamp"
rm -f "$status_file"
if [ "$wipe" -eq 1 ]; then
  emit mutation "fastboot -w update $image_name"
  {
    fastboot -s "$serial" -w update "$stage_image" </dev/null 2>&1 || printf '%s' "$?" > "$status_file"
  } | redact | tee -a "$transcript"
else
  emit mutation "fastboot update $image_name"
  {
    fastboot -s "$serial" update "$stage_image" </dev/null 2>&1 || printf '%s' "$?" > "$status_file"
  } | redact | tee -a "$transcript"
fi
if [ -f "$status_file" ]; then
  update_status=$(cat "$status_file")
  rm -f "$status_file"
else
  update_status=0
fi
emit update_exit_status "$update_status"
if [ "$update_status" -eq 0 ]; then
  emit result factory-restore-complete
  say "fastboot update finished; the device reboots into stock Android by itself (vendor default)."
  say "transcript (serial redacted): $transcript"
else
  emit result failed
  say "fastboot update exited with status $update_status; inspect the device state before retrying."
  say "transcript (serial redacted): $transcript"
  exit 1
fi
