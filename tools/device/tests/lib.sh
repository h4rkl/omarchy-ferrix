# Shared helpers for tools/device tests. Sourced, not executed.

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

q() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

# Write a synthetic, redacted identity file in the exact shape discover.sh
# produces. Arguments: path manufacturer model codename sku revision [transport]
write_identity() {
  cat > "$1" <<IDENTITY
discovered_utc='20260928T000000Z'
discovery_workflow='just device-discover'
transport='${7:-adb}'
serial_sha256_prefix='0123456789ab'
manufacturer=$(q "$2")
model=$(q "$3")
codename=$(q "$4")
board='synthboard'
hardware_revision=$(q "$6")
sku=$(q "$5")
bootloader_version='synthetic-bl-1'
bootloader_state='locked'
verified_boot='green'
current_slot='_b'
slot_count='2'
dynamic_partitions='true'
android_release='14'
fingerprint='$2/$4/$4:14/SYN.001/1:user/release-keys'
IDENTITY
}

# Minimal physical manifest in the checked-in shape.
write_manifest() {
  dir=$1
  mkdir -p "$dir"
  cat > "$dir/manifest.toml" <<MANIFEST
schema_version = 1
target_id = "$(basename -- "$dir")"
kind = "${7:-physical}"
status = "proposed-identity-only"

manufacturer = "$2"
marketing_name = "$3"
model_number = "$5"
codename = "$4"
hardware_revision = "$6"
sku = "$5"

[gates]
identity = "passed"
codename = "shadow-must-not-be-read"
MANIFEST
}

# Copy only checked-in board metadata (never the ignored recovery/backups
# folders, which may hold multi-gigabyte vendor archives) into a temp boards dir.
copy_board_metadata() {
  src=$1
  dst=$2
  for d in "$src"/*/; do
    [ -f "$d/manifest.toml" ] || continue
    name=$(basename -- "$d")
    mkdir -p "$dst/$name"
    for f in manifest.toml capabilities.toml recovery.pins; do
      [ -f "$d/$f" ] && cp "$d/$f" "$dst/$name/$f"
    done
  done
}

finish() {
  suite=$1
  if [ "$fails" -eq 0 ]; then
    printf '{"event":"device-tools-test","suite":"%s","status":"passed"}\n' "$suite"
  else
    printf '{"event":"device-tools-test","suite":"%s","status":"failed","failures":%s}\n' "$suite" "$fails"
    exit 1
  fi
}
