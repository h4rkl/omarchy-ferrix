#!/bin/sh
set -eu

# Offline scaffolder for an UNKNOWN attached device. It turns the redacted
# identity written by tools/device/discover.sh into a board-target skeleton and
# a board-support proposal for owner review. It never contacts a device, never
# reads a transcript, and never writes a serial or any raw identifier; the only
# device-unique value carried over is serial_sha256_prefix.
#
# The output is a proposal. It claims no boot or hardware support, and every
# gate except identity is pending. Destructive provisioning stays prohibited
# until the owner approves the proposal and the gates pass.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
boards=${FERRIX_BOARDS_DIR:-"$root/targets/boards"}
docs=${FERRIX_DOCS_DIR:-"$root/docs/board-support"}
identity=${1:-"${FERRIX_OUT_DIR:-"$root/out"}/device/identity-latest.env"}
match="$root/tools/device/match-target.sh"

die() {
  echo "$*" >&2
  exit 1
}

[ -f "$identity" ] || die "missing identity file: $identity (run: just device-discover)"

bad_lines=$(grep -nvE "^[a-z][a-z0-9_]*='([^']|'\\\\'')*'\$" "$identity" | grep -vE '^[0-9]+:[[:space:]]*(#.*)?$' || true)
[ -z "$bad_lines" ] || die "refusing malformed identity file $identity:
$bad_lines"

transport="" manufacturer="" manufacturer_source="" model="" codename="" board=""
hardware_revision="" sku="" bootloader_version="" bootloader_state="" verified_boot=""
current_slot="" slot_count="" dynamic_partitions="" android_release="" fingerprint=""
serial_sha256_prefix="" discovered_utc="" discovery_workflow=""
# shellcheck disable=SC1090
. "$identity"

[ -n "$manufacturer" ] || die "identity lacks manufacturer; rediscover over ADB or set FERRIX_DEVICE_MANUFACTURER before proposing"
[ -n "$codename" ] || die "identity lacks codename; cannot derive a target id"
[ -n "$hardware_revision" ] || die "identity lacks hardware_revision; a proposal must name one exact revision"

# Refuse to scaffold when a checked-in target already matches exactly.
match_status=0
FERRIX_BOARDS_DIR=$boards sh "$match" "$identity" >/dev/null 2>&1 || match_status=$?
[ "$match_status" -ne 0 ] || die "an exact target already matches this identity; run: just device-match"

slug() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | sed -e 's/[^a-z0-9]/-/g' -e 's/--*/-/g' -e 's/^-//' -e 's/-$//'
}

target_id=""
for part in "$manufacturer" "$model" "$codename" "$sku" "$hardware_revision"; do
  s=$(slug "$part")
  [ -n "$s" ] || continue
  target_id="${target_id:+$target_id-}$s"
done
[ -n "$target_id" ] || die "could not derive a target id"
codename_slug=$(slug "$codename")

target_dir="$boards/$target_id"
proposal="$docs/$codename_slug-attached.md"
[ ! -e "$target_dir" ] || die "refusing: target directory already exists: $target_dir"
[ ! -e "$proposal" ] || die "refusing: proposal already exists: $proposal"

# TOML basic-string escaping for values that came from a device.
toml_str() {
  printf '"%s"' "$(printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')"
}

# TOML scalar: bare integer or boolean when the value is exactly that shape
# (matching the checked-in Pixel manifest), otherwise a quoted string.
toml_scalar() {
  case $1 in
    true|false) printf '%s' "$1" ;;
    ''|*[!0-9]*) toml_str "$1" ;;
    *) printf '%s' "$1" ;;
  esac
}

# Markdown table cell: keep it on one line and neutralize pipes.
cell() {
  if [ -n "$1" ]; then
    printf '`%s`' "$(printf '%s' "$1" | sed 's/|/\\|/g')"
  else
    printf -- '—'
  fi
}

[ -n "$discovered_utc" ] || discovered_utc=$(date -u +%Y%m%dT%H%M%SZ)
[ -n "$discovery_workflow" ] || discovery_workflow="just device-discover"
[ -n "$bootloader_state" ] || bootloader_state=unknown

mkdir -p "$target_dir" "$docs"

cat > "$target_dir/manifest.toml" <<MANIFEST
schema_version = 1
target_id = $(toml_str "$target_id")
kind = "physical"
status = "proposed-identity-only"

manufacturer = $(toml_str "$manufacturer")
marketing_name = $(toml_str "$model")
model_number = $(toml_str "${sku:-$model}")
codename = $(toml_str "$codename")
hardware_revision = $(toml_str "$hardware_revision")
sku = $(toml_str "$sku")
board = $(toml_str "$board")

discovered_utc = $(toml_str "$discovered_utc")
discovery_transport = $(toml_str "$transport")
discovery_workflow = "just device-discover"
discovery_manufacturer_source = $(toml_str "${manufacturer_source:-observed}")
serial_sha256_prefix = $(toml_str "$serial_sha256_prefix")

stock_build_fingerprint = $(toml_str "$fingerprint")
stock_android_release = $(toml_str "$android_release")
bootloader_version = $(toml_str "$bootloader_version")
bootloader_state = $(toml_str "$bootloader_state")
verified_boot_state = $(toml_str "$verified_boot")
current_slot = $(toml_str "$current_slot")
slot_count = $(toml_scalar "$slot_count")
dynamic_partitions = $(toml_scalar "$dynamic_partitions")

# This package records one exact physical identity captured by the generic
# read-only discovery workflow. It does not claim that Ferrix Linux boots on
# the device or that any hardware API is implemented. It is a proposal that
# awaits explicit owner approval.
capability_matrix = "capabilities.toml"
recovery_pins = "recovery.pins"
board_support_proposal = "docs/board-support/$codename_slug-attached.md"

[gates]
identity = "passed"
image_compatibility = "pending"
backup = "pending"
bootloader_unlock = "pending"
recovery_procedure = "pending"
owner_approval = "pending"
per_run_confirmation = "required"
MANIFEST

cat > "$target_dir/capabilities.toml" <<CAPABILITIES
schema_version = 1
target_id = $(toml_str "$target_id")

# These are deliberately conservative: read-only Android identity discovery is
# not Ferrix Linux hardware evidence. No QEMU/mock result is reused here.
[power]
status = "unsupported"
evidence = "No Ferrix adapter or physical Linux evidence."

[display]
status = "unsupported"
evidence = "No Ferrix adapter or physical Linux evidence."

[input]
status = "unsupported"
evidence = "No Ferrix adapter or physical Linux evidence."

[network]
status = "unsupported"
evidence = "No Ferrix adapter or physical Linux evidence."

[bluetooth]
status = "unsupported"
evidence = "No Ferrix adapter or physical Linux evidence."

[audio]
status = "unsupported"
evidence = "No Ferrix adapter or physical Linux evidence."

[telephony]
status = "unsupported"
evidence = "No Ferrix adapter or physical Linux evidence."

[lifecycle]
status = "unsupported"
evidence = "No Ferrix adapter or physical Linux evidence."

[mock-control]
status = "unsupported"
evidence = "Physical target; no mock control is claimed."
CAPABILITIES

cat > "$target_dir/recovery.pins" <<'PINS'
# kind<TAB>build<TAB>version<TAB>filename<TAB>url<TAB>sha256<TAB>bytes<TAB>source-page
# Tab-separated pins for official vendor recovery images (kind is normally
# `factory` or `ota`). Every row must pin the exact vendor URL, the SHA-256 of
# the file, and its byte size before `just device-recovery-download` will fetch
# anything; the downloader refuses unpinned or mismatching files. Add rows only
# from the vendor's official image page and cite it in source-page.
PINS

cat > "$target_dir/README.md" <<README
# $manufacturer ${model:-$codename} — $codename / ${sku:-no SKU} / $hardware_revision

This is a proposed exact physical-target identity package generated by the
generic read-only workflow \`just device-discover\` followed by
\`just device-propose\`. It is a proposal awaiting owner approval. It is not a
claim that Ferrix Linux boots on this phone, that any hardware API is
implemented, or that any boot route is supported. Ferrix product code remains
target-neutral; the capability matrix is conservatively \`unsupported\` until
target-specific Linux evidence exists.

| Field | Observed value |
| --- | --- |
| Manufacturer | $(cell "$manufacturer") |
| Model / marketing name | $(cell "$model") |
| Codename / product | $(cell "$codename") |
| Board | $(cell "$board") |
| Hardware revision | $(cell "$hardware_revision") |
| SKU / model number | $(cell "$sku") |
| Bootloader version / state | $(cell "$bootloader_version") / $(cell "$bootloader_state") |
| Verified boot | $(cell "$verified_boot") |
| Current slot / slot count | $(cell "$current_slot") / $(cell "$slot_count") |
| Dynamic partitions | $(cell "$dynamic_partitions") |
| Android release | $(cell "$android_release") |
| Stock build fingerprint | $(cell "$fingerprint") |
| Discovery transport / time | $(cell "$transport") / $(cell "$discovered_utc") |
| Serial | redacted; SHA-256 prefix $(cell "$serial_sha256_prefix") only |

The serial and all other unique identifiers are intentionally absent. The
redacted discovery transcript is generated under ignored \`out/device/\`.

## Support boundary

- No boot, flash, unlock, erase, slot change, or sideload has been run, and
  none may be run from this package until the owner approves the proposal in
  \`docs/board-support/$codename_slug-attached.md\` and its gates pass.
- Evidence from QEMU, the mock profile, other devices, other models, or other
  hardware revisions of this model does not apply here.
- Mutation belongs only in a checked-in, target-scoped workflow that prints
  the exact device identity, partition target, image hash, and recovery
  artifact and requires per-run confirmation.

## Sources

- Identity: \`just device-discover\` (read-only ADB/Fastboot properties).
- Kernel, device-tree, driver, partition-map, and firmware sources: pending;
  record them in the proposal before any provisioning work.
- Recovery images: none pinned yet; see \`recovery.pins\`.
README

cat > "$proposal" <<PROPOSAL
# $manufacturer ${model:-$codename} board-support proposal

Status: identity captured by the generic read-only workflow; nothing else is
established. This proposal is NOT approved.

## Owner authorization

This proposal is NOT approved. It was generated by \`just device-propose\` from
the redacted identity written by \`just device-discover\` and exists only for
owner review. Read-only discovery is the sole device interaction that has
happened. Destructive provisioning (bootloader unlock, erase, format, slot
change, flash, boot of an untrusted image, sideload) requires explicit owner
approval recorded in this section plus passing identity, image-compatibility,
backup, and recovery gates, and it must run only through a checked-in,
target-scoped workflow with per-run confirmation.

This proposal is for one exact physical device. It must not generalize from
QEMU, mock state, another hardware revision, or another model.

## Exact manufacturer model number, codename, and hardware revision

| Field | Value | Source / evidence | Status |
| --- | --- | --- | --- |
| Manufacturer | $(cell "$manufacturer") | ${transport:-adb} read-only discovery${manufacturer_source:+ ($manufacturer_source)} | passed |
| Model / marketing name | $(cell "$model") | read-only discovery | $([ -n "$model" ] && echo passed || echo pending) |
| Manufacturer model number / SKU | $(cell "$sku") | read-only discovery | $([ -n "$sku" ] && echo passed || echo pending) |
| Codename / product | $(cell "$codename") | read-only discovery | passed |
| Board / SoC | $(cell "$board") | read-only discovery | $([ -n "$board" ] && echo partial || echo pending) |
| Hardware revision | $(cell "$hardware_revision") | read-only discovery | passed |
| Android build | $(cell "$fingerprint") | read-only discovery | $([ -n "$fingerprint" ] && echo passed || echo pending) |
| Serial | redacted; SHA-256 prefix $(cell "$serial_sha256_prefix") | never committed | passed |

## Bootloader state and supported boot route

| Field | Value | Source / evidence | Status |
| --- | --- | --- | --- |
| Bootloader version | $(cell "$bootloader_version") | read-only discovery | $([ -n "$bootloader_version" ] && echo passed || echo pending) |
| Bootloader state | $(cell "$bootloader_state") | read-only discovery | $([ "$bootloader_state" != unknown ] && echo passed || echo pending) |
| Verified boot state | $(cell "$verified_boot") | read-only discovery | $([ -n "$verified_boot" ] && echo passed || echo pending) |
| A/B slots and current slot | $(cell "$current_slot") / $(cell "$slot_count") | read-only discovery | $([ -n "$slot_count" ] && echo passed || echo pending) |
| Dynamic partitions | $(cell "$dynamic_partitions") | read-only discovery | $([ -n "$dynamic_partitions" ] && echo passed || echo pending) |
| Supported boot route | — | vendor documentation + postmarketOS device wiki | pending |

## Kernel, device-tree, and driver sources

| Field | Value | Source / evidence | Status |
| --- | --- | --- | --- |
| Kernel source and version | — | vendor kernel source release | pending |
| Device-tree source | — | vendor or mainline source | pending |
| Driver sources | — | vendor or mainline source | pending |

## Partition map

| Field | Value | Source / evidence | Status |
| --- | --- | --- | --- |
| Complete partition inventory | — | read-only Fastboot/ADB evidence, redacted before any commit | pending |
| Super / dynamic partition layout | — | read-only evidence | pending |

## Backup and recovery procedure

| Field | Value | Source / evidence | Status |
| --- | --- | --- | --- |
| Backup inventory | — | backup artifacts stored outside Git and verified readable | pending |
| Official recovery images pinned | — | \`recovery.pins\` with URL, SHA-256, and byte size | pending |
| Tested recovery procedure | — | exact-target restore evidence | pending |

## Firmware handling

| Field | Value | Source / evidence | Status |
| --- | --- | --- | --- |
| Firmware inventory and policy | — | official vendor images only; no firmware committed | pending |

## Capability matrix

| Field | Value | Source / evidence | Status |
| --- | --- | --- | --- |
| Capability matrix | all Ferrix capabilities conservative \`unsupported\` | \`targets/boards/$target_id/capabilities.toml\` | passed |

## Evidence plan

| Field | Value | Source / evidence | Status |
| --- | --- | --- | --- |
| Evidence and redaction policy | serials, IMEI/IMSI/ICCID, MAC/BSSID, keys, user data, and raw dumps never enter Git | this proposal + ignored output paths | partial |
| Physical evidence labelling | every committed result must name this exact target, never QEMU or mock | this proposal | partial |

## Data-collection policy

| Field | Value | Source / evidence | Status |
| --- | --- | --- | --- |
| Data-collection policy | selected identity/build properties only; no user-data pull or personal backup | this proposal | partial |

## Discovery log

| Date (UTC) | Event |
| --- | --- |
| $discovered_utc | \`just device-discover\` captured read-only identity over $(cell "$transport"); no device mutation performed. |
PROPOSAL

printf 'created=%s\n' "$target_dir/manifest.toml"
printf 'created=%s\n' "$target_dir/capabilities.toml"
printf 'created=%s\n' "$target_dir/recovery.pins"
printf 'created=%s\n' "$target_dir/README.md"
printf 'created=%s\n' "$proposal"
printf 'target_id=%s\n' "$target_id"
cat <<REMINDER

REMINDER: this is a proposal only. It is NOT approved. No ADB or Fastboot
mutation (unlock, erase, format, slot change, flash, boot, sideload) may run
against this device until the owner approves the proposal and the gates pass.

Add these lines to .gitignore so device-unique artifacts for the new target
never enter Git:

/targets/boards/$target_id/recovery/
/targets/boards/$target_id/backups/
/targets/boards/$target_id/evidence/
/targets/boards/$target_id/identifiers/
/targets/boards/$target_id/keys/
/targets/boards/$target_id/partition-dumps/
/targets/boards/$target_id/firmware/
/targets/boards/$target_id/**/*.img
/targets/boards/$target_id/**/*.zip
/targets/boards/$target_id/**/*.bin
/targets/boards/$target_id/**/*.raw
/targets/boards/$target_id/**/*.dump
/targets/boards/$target_id/**/*.log
/targets/boards/$target_id/**/*.serial
/targets/boards/$target_id/**/*.imei
/targets/boards/$target_id/**/*.imsi
/targets/boards/$target_id/**/*.iccid
/targets/boards/$target_id/**/*.mac
/targets/boards/$target_id/**/*.adbkey
/targets/boards/$target_id/**/*.key
/targets/boards/$target_id/**/*.pem
/targets/boards/$target_id/**/*.p12
/targets/boards/$target_id/**/*.jks
REMINDER
