#!/bin/sh
set -eu

# Target-scoped, confirmation-gated Pixel 9a stock full-OTA sideload.
# The preflight is read-only. The execution mode may issue only
# `adb reboot recovery` (when the phone is in Android) and `adb sideload` of
# the pinned, re-hashed OTA package; it verifies the exact device and the
# recovery artifacts before each state-changing command. It never flashes,
# erases, unlocks, changes slots, or touches Fastboot.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target=${FERRIX_PIXEL9A_TARGET_DIR:-"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"}
pins=${FERRIX_PIXEL9A_PINS:-"$target/recovery.pins"}
recovery="$target/recovery"
target_manifest="$target/manifest.toml"
out_root=${FERRIX_OUT_DIR:-"$root/out"}
out="$out_root/pixel-9a/sideload"
sideload_wait_seconds=${FERRIX_PIXEL9A_SIDELOAD_WAIT_SECONDS:-180}

expected_manufacturer=Google
expected_model="Pixel 9a"
expected_product=tegu
expected_revision=MP1.0
expected_sku=GXQ96

usage() {
  cat >&2 <<'EOF'
usage:
  tools/pixel-9a/sideload-ota.sh --preflight
  tools/pixel-9a/sideload-ota.sh --confirm-sideload

--preflight         read-only artifact, transport, and identity checks
--confirm-sideload  repeat the checks, require typed confirmations, then
                    sideload the pinned full OTA through stock recovery

The phone must be attached alone, in Android (USB debugging authorized),
in stock recovery, or already in "Apply update from ADB" (sideload) mode.
A serial-redacted transcript is written under out/pixel-9a/sideload/.
EOF
  exit 2
}

[ "$#" -eq 1 ] || usage
case "$1" in
  --preflight) mode=preflight ;;
  --confirm-sideload) mode=sideload ;;
  *) usage ;;
esac

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing $1; install Android Platform Tools" >&2
    exit 1
  }
}

need adb
need fastboot
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
adb_serial=""
{
  printf '# Google Pixel 9a stock OTA sideload (%s), %s UTC, host %s\n' "$mode" "$stamp" "$(uname -s) $(uname -m)"
  printf '# Serial numbers are replaced by a SHA-256 prefix. Never commit this file.\n'
} > "$transcript"

redact() {
  if [ -n "$adb_serial" ]; then
    redact_literal "$adb_serial" "serial-sha256:$(sha256_prefix "$adb_serial")"
  else
    cat
  fi
}

record() {
  printf '%s\n' "$1" | redact >> "$transcript"
}

# Operator-facing key=value line on stdout, redacted copy in the transcript.
emit() {
  printf '%s=%s\n' "$1" "$2"
  record "$1=$2"
}

# Operator-facing message on stderr, redacted copy in the transcript.
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
    ota_filename=$filename
  fi
}

# Even a full OTA is a destructive state transition (and recovery can fail in
# ways that leave the phone requiring a factory restore).  Report the owner
# backup gate during read-only preflight and enforce it before sending any
# reboot or OTA command.
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
  recovery_procedure_gate=$(gate_value recovery_procedure) || die "target manifest has no [gates].recovery_procedure entry: $target_manifest"
  confirmation_gate=$(gate_value per_run_confirmation) || die "target manifest has no [gates].per_run_confirmation entry: $target_manifest"
  [ -n "$identity_gate" ] && [ -n "$image_compatibility_gate" ] && [ -n "$backup_gate" ] && [ -n "$recovery_procedure_gate" ] && [ -n "$confirmation_gate" ] ||
    die "target manifest has an empty provisioning gate: $target_manifest"
  emit gate_identity "$identity_gate"
  emit gate_image_compatibility "$image_compatibility_gate"
  emit gate_recovery_procedure "$recovery_procedure_gate"
  emit backup_gate "$backup_gate"
  emit per_run_confirmation "$confirmation_gate"
}

require_backup_gate() {
  read_all_gates
  [ "$identity_gate" = passed ] || die "identity gate is '$identity_gate'; exact target identity must be approved before this sideload"
  [ "$confirmation_gate" = required ] || die "per-run confirmation gate is '$confirmation_gate'; manifest must require typed confirmation"
  [ "$backup_gate" = verified ] ||
    die "backup gate is '$backup_gate'; owner must verify a readable backup outside Git before this sideload"
}

require_mutation_gates() {
  require_backup_gate
  case "$image_compatibility_gate" in
    passed|verified) ;;
    *) die "image compatibility gate is '$image_compatibility_gate'; exact target image inputs are not approved for this sideload" ;;
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

enumerate_transports() {
  adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
  fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
  adb_count=$(count_rows "$adb_rows")
  fastboot_count=$(count_rows "$fastboot_rows")
}

# Identity in Android: the same exact fields as the unlock gate, without the
# locked/green requirements (the bootloader is now unlocked).
read_android_identity() {
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
  device_state=$(adb_prop ro.boot.vbmeta.device_state)
  dynamic_partitions=$(adb_prop ro.boot.dynamic_partitions)
  build_fingerprint=$(adb_prop ro.build.fingerprint)

  [ "$manufacturer" = "$expected_manufacturer" ] || die "manufacturer mismatch: $manufacturer"
  [ "$model" = "$expected_model" ] || die "model mismatch: $model"
  [ "$product" = "$expected_product" ] || die "product mismatch: $product"
  [ "$codename" = "$expected_product" ] || die "codename mismatch: $codename"
  [ "$board" = "$expected_product" ] || die "board mismatch: $board"
  [ "$hardware_revision" = "$expected_revision" ] || die "hardware revision mismatch: $hardware_revision"
  [ "$sku" = "$expected_sku" ] || die "SKU mismatch: $sku"
  case "$current_slot" in
    _a|_b) ;;
    *) die "unexpected current slot: $current_slot" ;;
  esac
  [ "$dynamic_partitions" = true ] || die "dynamic partitions property is '$dynamic_partitions', not true"

  emit manufacturer "$manufacturer"
  emit model "$model"
  emit product "$product"
  emit codename "$codename"
  emit board "$board"
  emit hardware_revision "$hardware_revision"
  emit sku "$sku"
  emit bootloader_version "$bootloader_version"
  emit current_slot "$current_slot"
  emit slot_count "$slot_count"
  emit device_state "$device_state"
  emit dynamic_partitions "$dynamic_partitions"
  emit build_fingerprint "$build_fingerprint"
  identity_check=verified-in-android
}

# Identity in stock recovery: adbd exposes a subset of properties. An empty
# value is reported as not observable; a visible value must match exactly.
recovery_prop() {
  key=$1
  prop=$2
  expected=$3
  value=$(adb_prop "$prop")
  if [ -z "$value" ]; then
    emit "$key" not-observable-in-recovery
    recovery_partial=1
  else
    [ "$value" = "$expected" ] || die "$key mismatch in recovery: $value"
    emit "$key" "$value"
  fi
}

read_recovery_identity() {
  recovery_partial=0
  product=$(adb_prop ro.product.device)
  hardware_revision=$(adb_prop ro.boot.hardware.revision)
  current_slot=$(adb_prop ro.boot.slot_suffix)
  if [ -n "$product" ]; then
    [ "$product" = "$expected_product" ] || die "codename mismatch in recovery: $product"
    emit codename "$product"
  else
    emit codename not-observable-in-recovery
    recovery_partial=1
  fi
  if [ -n "$hardware_revision" ]; then
    [ "$hardware_revision" = "$expected_revision" ] || die "hardware revision mismatch in recovery: $hardware_revision"
    emit hardware_revision "$hardware_revision"
  else
    emit hardware_revision not-observable-in-recovery
    recovery_partial=1
  fi
  recovery_prop product ro.product.name "$expected_product"
  recovery_prop sku ro.boot.hardware.sku "$expected_sku"
  case "$current_slot" in
    _a|_b) emit current_slot "$current_slot" ;;
    "") emit current_slot not-observable-in-recovery ;;
    *) die "unexpected current slot in recovery: $current_slot" ;;
  esac
  if [ "$recovery_partial" -eq 0 ]; then
    identity_check=verified-in-recovery
  else
    identity_check=partial-in-recovery
  fi
}

print_summary() {
  emit operation ota-sideload
  emit transport adb
  emit serial "$adb_serial"
  emit serial_sha256_prefix "$(sha256_prefix "$adb_serial")"
  emit adb_state "$adb_state"
  emit identity_check "$identity_check"
  emit product_expected "$expected_product"
  emit hardware_revision_expected "$expected_revision"
  emit partition_target "stock A/B OTA (both slots per package)"
  emit input_image_path "$ota_path"
  emit input_image_sha256 "$ota_hash"
  emit recovery_artifact "$factory_path sha256:$factory_hash"
}

preflight() {
  verify_artifact ota
  verify_artifact factory
  read_all_gates
  enumerate_transports
  emit adb_devices "$adb_count"
  emit fastboot_devices "$fastboot_count"
  [ "$((adb_count + fastboot_count))" -eq 1 ] || {
    die "ambiguous transports before sideload: adb=$adb_count fastboot=$fastboot_count"
  }
  [ "$adb_count" -eq 1 ] || {
    die "expected exactly one ADB device (Android, recovery, or sideload); Fastboot cannot sideload an OTA"
  }
  adb_serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1 }')
  adb_state=$(printf '%s\n' "$adb_rows" | awk '{ print $2 }')
  case "$adb_state" in
    device) read_android_identity ;;
    recovery) read_recovery_identity ;;
    sideload)
      identity_check=unavailable-in-sideload
      emit codename not-observable-in-sideload
      emit hardware_revision not-observable-in-sideload
      emit current_slot unknown
      ;;
    *) die "ADB state is '$adb_state', not device, recovery, or sideload" ;;
  esac
  print_summary
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
  wanted_state=$1
  enumerate_transports
  [ "$adb_count" -eq 1 ] && [ "$fastboot_count" -eq 0 ] || {
    die "transport changed before mutation: adb=$adb_count fastboot=$fastboot_count"
  }
  recheck_serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1 }')
  recheck_state=$(printf '%s\n' "$adb_rows" | awk '{ print $2 }')
  [ "$recheck_serial" = "$adb_serial" ] || die "ADB serial changed before mutation"
  [ "$recheck_state" = "$wanted_state" ] || {
    die "ADB state is '$recheck_state', not '$wanted_state', before mutation"
  }
  [ "$(adb -s "$adb_serial" get-state </dev/null | tr -d '\r')" = "$wanted_state" ] || {
    die "ADB get-state does not report '$wanted_state' before mutation"
  }
}

wait_for_sideload() {
  attempt=0
  while [ "$attempt" -lt "$sideload_wait_seconds" ]; do
    enumerate_transports
    [ "$fastboot_count" -eq 0 ] || die "a Fastboot device appeared while waiting for sideload mode"
    [ "$adb_count" -le 1 ] || die "ambiguous ADB transports while waiting for sideload mode: $adb_count"
    if [ "$adb_count" -eq 1 ]; then
      seen_serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1 }')
      seen_state=$(printf '%s\n' "$adb_rows" | awk '{ print $2 }')
      [ "$seen_serial" = "$adb_serial" ] || die "a different ADB serial appeared while waiting for sideload mode"
      if [ "$seen_state" = sideload ]; then
        adb_state=sideload
        emit adb_state sideload
        return 0
      fi
    fi
    attempt=$((attempt + 1))
    sleep 1
  done
  die "the exact device did not enter sideload mode within $sideload_wait_seconds seconds; no OTA was sent"
}

preflight
say "transcript (serial redacted): $transcript"
[ "$mode" = sideload ] || exit 0
require_mutation_gates

ota_hash_short=$(printf '%s' "$ota_hash" | cut -c1-16)

case "$adb_state" in
  device)
    confirm_phrase \
      "CONFIRM PIXEL 9A REBOOT TO RECOVERY FOR OTA SIDELOAD" \
      "The next command reboots the exact device into stock recovery. Nothing is flashed yet."
    verify_artifact factory
    verify_artifact ota
    recheck_adb_transport device
    emit mutation "adb reboot recovery"
    adb -s "$adb_serial" reboot recovery
    say "On the phone: in stock recovery choose 'Apply update from ADB' (volume keys move, power selects)."
    say "Waiting up to $sideload_wait_seconds seconds for the same serial to report the sideload state."
    wait_for_sideload
    ;;
  recovery)
    say "On the phone: choose 'Apply update from ADB' (volume keys move, power selects)."
    say "Waiting up to $sideload_wait_seconds seconds for the same serial to report the sideload state."
    wait_for_sideload
    ;;
  sideload) ;;
esac

if [ "$identity_check" = verified-in-android ] || [ "$identity_check" = verified-in-recovery ]; then
  phrase="CONFIRM PIXEL 9A OTA SIDELOAD $ota_filename SHA256 $ota_hash_short"
else
  phrase="CONFIRM PIXEL 9A OTA SIDELOAD $ota_filename SHA256 $ota_hash_short IDENTITY $identity_check"
fi
confirm_phrase "$phrase" \
  "The next command sends the verified full OTA ($ota_hash) to the device in sideload mode. Identity check: $identity_check."
verify_artifact factory
verify_artifact ota
recheck_adb_transport sideload

emit mutation "adb sideload $ota_filename"
status_file="$out/.status-$stamp"
rm -f "$status_file"
{
  adb -s "$adb_serial" sideload "$ota_path" </dev/null 2>&1 || printf '%s' "$?" > "$status_file"
} | redact | tee -a "$transcript"
if [ -f "$status_file" ]; then
  sideload_status=$(cat "$status_file")
  rm -f "$status_file"
else
  sideload_status=0
fi
emit sideload_exit_status "$sideload_status"
if [ "$sideload_status" -eq 0 ]; then
  emit result ota-sideload-complete
  say "The phone verifies and installs the package itself; confirm the on-screen result, then choose 'Reboot system now'."
  say "transcript (serial redacted): $transcript"
else
  emit result failed
  say "adb sideload exited with status $sideload_status; the phone screen shows the recovery verdict."
  say "transcript (serial redacted): $transcript"
  exit 1
fi
