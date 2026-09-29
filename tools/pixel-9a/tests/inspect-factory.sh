#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-pixel9a-inspect.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM

make_fixture() {
  fixture=$1
  board=$2
  compression=$3
  mkdir -p "$fixture"
  cargo run --quiet --release --locked --manifest-path "$root/Cargo.toml" \
    -p ferrix-pixel9a-tool --bin pixel-9a-fixture -- \
    "$fixture" "$board" "$compression"
}

assert_inventory() {
  inventory=$1
  grep -qF '"schema_version": 1' "$inventory"
  grep -qF '"target_id": "google-pixel-9a-tegu-gxq96-mp1-0"' "$inventory"
  grep -qF '"verified": true' "$inventory"
  grep -qF '"board": "tegu"' "$inventory"
  grep -qF '"flash boot"' "$inventory"
  grep -qF '"vendor_boot.img"' "$inventory"
  grep -qF '"device_identifiers_collected": false' "$inventory"
  grep -qF '"host_paths_recorded": false' "$inventory"
  grep -qF '"payloads_extracted": false' "$inventory"
}

good="$work/good"
make_fixture "$good" tegu stored
FERRIX_PIXEL9A_PINS="$good/recovery.pins" \
FERRIX_PIXEL9A_FACTORY_ARCHIVE="$good/tegu-testbuild-factory-fixture.zip" \
FERRIX_OUT_DIR="$good/out" \
  "$root/tools/pixel-9a/inspect-factory.sh" >/dev/null
assert_inventory "$good/out/pixel-9a/factory-inspection/inventory.json"

bad_hash="$work/bad-hash"
make_fixture "$bad_hash" tegu stored
sed 's/\t[0-9a-f]\{64\}\t/\t0000000000000000000000000000000000000000000000000000000000000000\t/' \
  "$bad_hash/recovery.pins" > "$bad_hash/wrong.pins"
if FERRIX_PIXEL9A_PINS="$bad_hash/wrong.pins" \
  FERRIX_PIXEL9A_FACTORY_ARCHIVE="$bad_hash/tegu-testbuild-factory-fixture.zip" \
  FERRIX_OUT_DIR="$bad_hash/out" \
  "$root/tools/pixel-9a/inspect-factory.sh" >/dev/null 2>&1; then
  echo "FAIL: accepted a factory archive with the wrong pinned hash" >&2
  exit 1
fi

bad_board="$work/bad-board"
make_fixture "$bad_board" komodo stored
if FERRIX_PIXEL9A_PINS="$bad_board/recovery.pins" \
  FERRIX_PIXEL9A_FACTORY_ARCHIVE="$bad_board/tegu-testbuild-factory-fixture.zip" \
  FERRIX_OUT_DIR="$bad_board/out" \
  "$root/tools/pixel-9a/inspect-factory.sh" >/dev/null 2>&1; then
  echo "FAIL: accepted a non-tegu android-info.txt" >&2
  exit 1
fi

compressed="$work/compressed"
make_fixture "$compressed" tegu deflated
if FERRIX_PIXEL9A_PINS="$compressed/recovery.pins" \
  FERRIX_PIXEL9A_FACTORY_ARCHIVE="$compressed/tegu-testbuild-factory-fixture.zip" \
  FERRIX_OUT_DIR="$compressed/out" \
  "$root/tools/pixel-9a/inspect-factory.sh" >/dev/null 2>&1; then
  echo "FAIL: accepted a compressed nested archive requiring large extraction" >&2
  exit 1
fi

if grep -nE '(^|[^[:alnum:]_])(adb|fastboot|curl|wget)([^[:alnum:]_]|$)' "$root/tools/pixel-9a/inspect-factory.sh" \
  | grep -v 'never invokes' >/dev/null; then
  echo "FAIL: offline inspection wrapper contains a device or network command" >&2
  exit 1
fi

printf '%s\n' "pixel-9a factory inspection tests passed"
