#!/bin/sh
set -eu

# Offline inspection only: verifies and reads an already-downloaded factory
# archive. It never invokes adb, fastboot, curl, or any device-facing command.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
pins=${FERRIX_PIXEL9A_PINS:-"$target/recovery.pins"}
out_root=${FERRIX_OUT_DIR:-"$root/out"}
out="$out_root/pixel-9a/factory-inspection"

command -v cargo >/dev/null 2>&1 || {
  echo "missing cargo; required to run the Rust factory inspection tool" >&2
  exit 1
}

factory_filename=$(awk -F '\t' '$1 == "factory" { print $4; exit }' "$pins")
[ -n "$factory_filename" ] || {
  echo "factory-inspection-refused: missing factory pin in $pins" >&2
  exit 1
}
archive=${FERRIX_PIXEL9A_FACTORY_ARCHIVE:-"$target/recovery/$factory_filename"}
[ -f "$archive" ] || {
  echo "factory-inspection-refused: missing factory archive: $archive" >&2
  echo "Download it separately with: just pixel-9a-recovery-download factory" >&2
  exit 1
}
[ ! -L "$archive" ] || {
  echo "factory-inspection-refused: archive must not be a symlink: $archive" >&2
  exit 1
}

mkdir -p "$out"
exec cargo run --quiet --release --locked --manifest-path "$root/Cargo.toml" \
  -p ferrix-pixel9a-tool --bin ferrix-pixel9a-tool -- \
  "$pins" "$archive" "$out/inventory.json"
