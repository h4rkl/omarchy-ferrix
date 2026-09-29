#!/bin/sh
set -eu

# Generic, vendor-agnostic, read-only identity discovery for any Android phone
# attached over ADB or Fastboot. This workflow deliberately has no reboot,
# unlock, boot, erase, format, flash, sideload, or slot-changing operation.
# Its only device verbs are `adb devices -l`, `adb -s S shell getprop P`,
# `fastboot devices -l`, and `fastboot -s S getvar V`.
#
# Stdout includes the exact serial for the operator. The ignored transcript and
# the machine-readable identity file replace it with a SHA-256 prefix. Mutation
# never happens here; it lives only in target-scoped workflows such as
# tools/pixel-9a/ after a checked-in proposal has passed its gates.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/device

die() {
  echo "$*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing $1; install Android Platform Tools"
}

need adb
need fastboot

sha256_prefix() {
  if command -v shasum >/dev/null 2>&1; then
    printf '%s' "$1" | shasum -a 256 | cut -c1-12
  else
    printf '%s' "$1" | sha256sum | cut -c1-12
  fi
}

# Replace the device serial as a literal byte string. Serial values must not be
# interpolated into a sed regular expression: discovery must redact safely for
# every value any vendor's Platform Tools build emits.
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

# Quote a value so `. identity-latest.env` reproduces it exactly in any POSIX
# shell. Single quotes are escaped; values containing a newline are rejected
# (replaced by an empty string) because a property value is one line.
sh_quote() {
  case $1 in
    *"$newline"*) printf "''" ; return 0 ;;
  esac
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}
newline='
'

mkdir -p "$out"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
transcript="$out/discovery-$stamp.txt"
identity="$out/identity-latest.env"
identity_tmp="$identity.tmp"
serial=""
{
  printf '# Generic read-only device discovery, %s UTC, host %s\n' "$stamp" "$(uname -s) $(uname -m)"
  printf '# Serial numbers are replaced by a SHA-256 prefix. Never commit this file.\n'
} > "$transcript"
: > "$identity_tmp"

redacted() {
  if [ -n "$serial" ]; then
    printf '%s\n' "$1" | redact_literal "$serial" "serial-sha256:$(sha256_prefix "$serial")"
  else
    printf '%s\n' "$1"
  fi
}

emit() {
  key=$1
  value=$2
  printf '%s=%s\n' "$key" "$value"
  printf '%s=%s\n' "$key" "$(redacted "$value")" >> "$transcript"
}

# Normalized identity keys are written redacted and sh-quoted. The serial itself
# is never an identity key; only serial_sha256_prefix is.
identity_set() {
  key=$1
  value=$(redacted "$2")
  case $key in
    serial) die "internal error: refusing to store the serial in the identity file" ;;
  esac
  printf '%s=%s\n' "$key" "$(sh_quote "$value")" >> "$identity_tmp"
  printf 'identity.%s=%s\n' "$key" "$value" >> "$transcript"
  printf '%s=%s\n' "$key" "$value"
}

finish_identity() {
  mv "$identity_tmp" "$identity"
  echo "identity (redacted, sh-sourceable): $identity" >&2
  echo "transcript (serial redacted): $transcript" >&2
}

adb_rows=$(adb devices -l | awk 'NR > 1 && NF { print }')
fastboot_rows=$(fastboot devices -l 2>/dev/null | awk 'NF { print }')
adb_count=$(printf '%s\n' "$adb_rows" | awk 'NF { count++ } END { print count + 0 }')
fastboot_count=$(printf '%s\n' "$fastboot_rows" | awk 'NF { count++ } END { print count + 0 }')
total=$((adb_count + fastboot_count))
emit adb_devices "$adb_count"
emit fastboot_devices "$fastboot_count"

if [ "$total" -eq 0 ]; then
  emit result no-device
  rm -f "$identity_tmp"
  echo "No ADB or Fastboot device is visible." >&2
  echo "Use a data-capable cable and either enable USB debugging and accept the RSA prompt, or enter Fastboot mode." >&2
  echo "transcript: $transcript" >&2
  exit 2
fi

if [ "$total" -ne 1 ]; then
  emit result ambiguous
  rm -f "$identity_tmp"
  echo "Refusing ambiguous discovery: $adb_count ADB and $fastboot_count Fastboot devices." >&2
  echo "transcript: $transcript" >&2
  exit 3
fi

identity_set discovered_utc "$stamp"
identity_set discovery_workflow "just device-discover"

if [ "$adb_count" -eq 1 ]; then
  serial=$(printf '%s\n' "$adb_rows" | awk '{ print $1 }')
  state=$(printf '%s\n' "$adb_rows" | awk '{ print $2 }')
  emit transport adb
  emit serial "$serial"
  emit serial_sha256_prefix "$(sha256_prefix "$serial")"
  emit state "$state"
  if [ "$state" != "device" ]; then
    emit result adb-not-authorized
    rm -f "$identity_tmp"
    echo "ADB is not authorized/ready; no properties were queried." >&2
    echo "transcript: $transcript" >&2
    exit 4
  fi

  for prop in \
    ro.product.manufacturer \
    ro.product.brand \
    ro.product.model \
    ro.product.name \
    ro.product.device \
    ro.product.board \
    ro.hardware \
    ro.hardware.chipname \
    ro.boot.hardware.revision \
    ro.boot.hardware.sku \
    ro.boot.hardware \
    ro.boot.bootloader \
    ro.boot.slot_suffix \
    ro.boot.slot_count \
    ro.boot.verifiedbootstate \
    ro.boot.vbmeta.device_state \
    ro.boot.flash.locked \
    ro.boot.dynamic_partitions \
    ro.boot.boot_devices \
    ro.build.fingerprint \
    ro.build.version.release \
    ro.build.version.sdk \
    ro.build.version.security_patch \
    ro.vendor.build.version.security_patch \
    ro.soc.manufacturer \
    ro.soc.model \
    ro.oem_unlock_supported \
    sys.oem_unlock_allowed; do
    value=$(adb -s "$serial" shell getprop "$prop" </dev/null | tr -d '\r')
    emit "$prop" "$value"
    # Keep the raw properties needed for normalization in shell variables.
    var=$(printf '%s' "$prop" | tr '.' '_')
    eval "p_$var=\$value"
  done

  codename=${p_ro_product_device:-}
  [ -n "$codename" ] || codename=${p_ro_product_name:-}
  [ -n "$codename" ] || codename=${p_ro_product_board:-}
  bootloader_state=unknown
  case ${p_ro_boot_flash_locked:-} in
    1) bootloader_state=locked ;;
    0) bootloader_state=unlocked ;;
    *)
      case ${p_ro_boot_vbmeta_device_state:-} in
        locked) bootloader_state=locked ;;
        unlocked) bootloader_state=unlocked ;;
      esac
      ;;
  esac

  identity_set transport adb
  identity_set serial_sha256_prefix "$(sha256_prefix "$serial")"
  identity_set manufacturer "${p_ro_product_manufacturer:-}"
  identity_set model "${p_ro_product_model:-}"
  identity_set codename "$codename"
  identity_set board "${p_ro_product_board:-}"
  identity_set hardware_revision "${p_ro_boot_hardware_revision:-}"
  identity_set sku "${p_ro_boot_hardware_sku:-}"
  identity_set bootloader_version "${p_ro_boot_bootloader:-}"
  identity_set bootloader_state "$bootloader_state"
  identity_set verified_boot "${p_ro_boot_verifiedbootstate:-}"
  identity_set current_slot "${p_ro_boot_slot_suffix:-}"
  identity_set slot_count "${p_ro_boot_slot_count:-}"
  identity_set dynamic_partitions "${p_ro_boot_dynamic_partitions:-}"
  identity_set android_release "${p_ro_build_version_release:-}"
  identity_set fingerprint "${p_ro_build_fingerprint:-}"
  emit result ok
  finish_identity
  exit 0
fi

serial=$(printf '%s\n' "$fastboot_rows" | awk '{ print $1 }')
emit transport fastboot
emit serial "$serial"
emit serial_sha256_prefix "$(sha256_prefix "$serial")"
for var in product variant hw-revision current-slot slot-count unlocked secure is-userspace version-bootloader version-baseband serialno; do
  # Fastboot writes getvar responses to stderr on many versions.
  value=$(fastboot -s "$serial" getvar "$var" 2>&1 | awk -v key="$var" '
    index($0, key ":") { sub(".*" key ":[[:space:]]*", ""); print; exit }
  ')
  emit "$var" "$value"
  name=$(printf '%s' "$var" | tr '-' '_')
  eval "v_$name=\$value"
done

bootloader_state=unknown
case ${v_unlocked:-} in
  yes) bootloader_state=unlocked ;;
  no) bootloader_state=locked ;;
esac

# Fastboot does not expose the manufacturer. An operator may supply it through
# FERRIX_DEVICE_MANUFACTURER; the identity file records that it was supplied,
# not observed, so matching stays honest about its evidence.
manufacturer=${FERRIX_DEVICE_MANUFACTURER:-}
identity_set transport fastboot
identity_set serial_sha256_prefix "$(sha256_prefix "$serial")"
identity_set manufacturer "$manufacturer"
identity_set manufacturer_source "$([ -n "$manufacturer" ] && echo operator || echo none)"
identity_set model ""
identity_set codename "${v_product:-}"
identity_set board ""
identity_set hardware_revision "${v_hw_revision:-}"
identity_set sku "${v_variant:-}"
identity_set bootloader_version "${v_version_bootloader:-}"
identity_set bootloader_state "$bootloader_state"
identity_set verified_boot ""
identity_set current_slot "${v_current_slot:-}"
identity_set slot_count "${v_slot_count:-}"
identity_set dynamic_partitions ""
identity_set android_release ""
identity_set fingerprint ""
emit result ok
finish_identity
