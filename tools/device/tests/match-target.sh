#!/bin/sh
# Offline proof for the generic target matcher. It runs against a temporary
# copy of the boards directory plus synthetic manifests, so it never writes to
# the real tree and never contacts a device.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
match="$root/tools/device/match-target.sh"
fails=0
# shellcheck disable=SC1091
. "$here/lib.sh"

work=$(mktemp -d "${TMPDIR:-/tmp}/device-match-test.XXXXXX")
trap 'rm -rf "$work"' EXIT
boards="$work/boards"
mkdir -p "$boards"
copy_board_metadata "$root/targets/boards" "$boards"
export FERRIX_BOARDS_DIR="$boards" FERRIX_OUT_DIR="$work/out"

# The repository may already have staged or user-owned edits under these
# paths. Capture the porcelain state before the matcher runs and compare it
# afterwards, so this assertion detects mutations made by the test without
# assuming a clean index.
git -C "$root" status --porcelain -- targets/boards docs/board-support > "$work/status-before"

# Synthetic non-Google fixtures.
write_manifest "$boards/oneplus-syn2001-synthetic-op-sku7-evt2" OnePlus SYN2001 synthetic-op SKU7 EVT2
write_manifest "$boards/oneplus-syn2001-synthetic-op-sku7-dvt1" OnePlus SYN2001 synthetic-op SKU7 DVT1
write_manifest "$boards/not-physical-synthetic-op" OnePlus SYN2001 synthetic-op SKU7 PVT9 virtual

run_match() {
  status=0
  sh "$match" "$1" > "$work/stdout" 2> "$work/stderr" || status=$?
}

check "static: matcher never invokes adb/fastboot/curl" sh -c "! grep -nE '^[[:space:]]*[^#]*(adb|fastboot|curl)([[:space:]]|$)' '$match' | grep -vE 'transport.*fastboot|\\[[^]]*fastboot[^]]*\\]'"

# Missing identity.
run_match "$work/does-not-exist.env"
check "refusal: missing identity exits 1" [ "$status" -eq 1 ]

# Default identity path from FERRIX_OUT_DIR.
mkdir -p "$work/out/device"
write_identity "$work/out/device/identity-latest.env" OnePlus SYN2001 synthetic-op SKU7 EVT2
status=0
sh "$match" > "$work/stdout" 2> "$work/stderr" || status=$?
check "default: latest identity under FERRIX_OUT_DIR is used" [ "$status" -eq 0 ]

# Exact synthetic vendor match (case-insensitive manufacturer).
write_identity "$work/id-exact.env" oneplus SYN2001 synthetic-op SKU7 EVT2
run_match "$work/id-exact.env"
check "exact: synthetic OnePlus identity matches" [ "$status" -eq 0 ]
check "exact: target_id and target_dir printed" grep -q "^match=exact target_id=oneplus-syn2001-synthetic-op-sku7-evt2 target_dir=$boards/oneplus-syn2001-synthetic-op-sku7-evt2\$" "$work/stdout"
check "exact: virtual manifest is not scanned" sh -c "! grep -q not-physical '$work/stdout'"

# Same codename, different revision: never generalize.
write_identity "$work/id-rev.env" OnePlus SYN2001 synthetic-op SKU7 PVT3
run_match "$work/id-rev.env"
check "revision: different revision is match=none" [ "$status" -eq 10 ]
check "revision: match=none printed" grep -q '^match=none$' "$work/stdout"
check "revision: partial candidates listed" grep -q '^partial=oneplus-syn2001-synthetic-op-sku7-evt2 reason=same-codename-different-revision' "$work/stdout"
check "revision: explicit do-not-reuse note" grep -Fq 'note=same codename, different hardware revision; do not reuse target evidence' "$work/stdout"
check "revision: no exact line leaked" sh -c "! grep -q '^match=exact' '$work/stdout'"

# Same codename and revision, different SKU present on both sides.
write_identity "$work/id-sku.env" OnePlus SYN2001 synthetic-op SKU8 EVT2
run_match "$work/id-sku.env"
check "sku: different SKU is match=none" [ "$status" -eq 10 ]
check "sku: partial reason names SKU" grep -q 'reason=same-codename-different-sku' "$work/stdout"

# SKU absent on the identity side still matches (rule: only when present in both).
write_identity "$work/id-nosku.env" OnePlus SYN2001 synthetic-op "" EVT2
run_match "$work/id-nosku.env"
check "sku: absent identity SKU does not block an exact match" [ "$status" -eq 0 ]

# The checked-in Pixel 9a target matches its own identity, with [gates] keys ignored.
write_identity "$work/id-pixel.env" Google "Pixel 9a" tegu GXQ96 MP1.0
run_match "$work/id-pixel.env"
check "pixel: checked-in Pixel 9a manifest matches" [ "$status" -eq 0 ]
check "pixel: target_id is the Pixel 9a directory" grep -q '^match=exact target_id=google-pixel-9a-tegu-gxq96-mp1-0 ' "$work/stdout"
write_identity "$work/id-pixel-rev.env" Google "Pixel 9a" tegu GXQ96 EVT1.0
run_match "$work/id-pixel-rev.env"
check "pixel: other revision of tegu is match=none" [ "$status" -eq 10 ]

# Fastboot variant=N/A is accepted only with a same-serial redacted ADB
# correlation proving the exact Pixel SKU and revision.
mkdir -p "$work/out/pixel-9a"
cat >"$work/out/pixel-9a/discovery-adb.txt" <<'ADB_EVIDENCE'
transport=adb
serial_sha256_prefix=0123456789ab
ro.product.manufacturer=Google
ro.product.model=Pixel 9a
ro.product.device=tegu
ro.boot.hardware.revision=MP1.0
ro.boot.hardware.sku=GXQ96
result=ok
ADB_EVIDENCE
write_identity "$work/id-pixel-fastboot-na.env" Google "" tegu N/A MP1.0 fastboot
run_match "$work/id-pixel-fastboot-na.env"
check "pixel: Fastboot N/A variant uses same-serial ADB correlation" [ "$status" -eq 0 ]
check "pixel: correlation is reported" grep -q '^sku_correlation=adb:' "$work/stdout"
rm -f "$work/out/pixel-9a/discovery-adb.txt"
run_match "$work/id-pixel-fastboot-na.env"
check "pixel: uncorrelated Fastboot N/A variant is refused" [ "$status" -eq 10 ]
check "pixel: uncorrelated N/A refusal explains required evidence" grep -q 'same-serial redacted ADB identity' "$work/stdout"

# Google's Pixel userspace Fastboot reports MSM USF instead of the Android
# model/SKU. Only that mode and a same-serial ADB proof may normalize it.
cat >"$work/out/pixel-9a/discovery-adb.txt" <<'ADB_EVIDENCE'
transport=adb
serial_sha256_prefix=0123456789ab
ro.product.manufacturer=Google
ro.product.model=Pixel 9a
ro.product.device=tegu
ro.boot.hardware.revision=MP1.0
ro.boot.hardware.sku=GXQ96
result=ok
ADB_EVIDENCE
write_identity "$work/id-pixel-fastbootd.env" Google "" tegu 'MSM USF' MP1.0 fastboot
printf "fastboot_userspace='yes'\n" >> "$work/id-pixel-fastbootd.env"
run_match "$work/id-pixel-fastbootd.env"
check "pixel: Fastbootd placeholder uses same-serial ADB correlation" [ "$status" -eq 0 ]
check "pixel: Fastbootd correlation reports verified GXQ96" grep -q '^identity_sku=GXQ96$' "$work/stdout"
printf "fastboot_userspace='no'\n" >> "$work/id-pixel-fastbootd.env"
run_match "$work/id-pixel-fastbootd.env"
check "pixel: bootloader-mode MSM USF is refused" [ "$status" -eq 10 ]
rm -f "$work/out/pixel-9a/discovery-adb.txt"
write_identity "$work/id-pixel-fastbootd-uncorrelated.env" Google "" tegu 'MSM USF' MP1.0 fastboot
printf "fastboot_userspace='yes'\n" >> "$work/id-pixel-fastbootd-uncorrelated.env"
run_match "$work/id-pixel-fastbootd-uncorrelated.env"
check "pixel: uncorrelated Fastbootd placeholder is refused" [ "$status" -eq 10 ]

# Fastboot identity without manufacturer.
write_identity "$work/id-fb.env" "" "" synthetic-op SKU7 EVT2 fastboot
run_match "$work/id-fb.env"
check "fastboot: missing manufacturer is match=none" [ "$status" -eq 10 ]
check "fastboot: note explains missing manufacturer" grep -q '^note=identity lacks manufacturer' "$work/stdout"

# Malformed identity is refused before sourcing.
printf "manufacturer='x'\nevil=\$(touch %s/pwned)\n" "$work" > "$work/id-bad.env"
run_match "$work/id-bad.env"
check "refusal: unquoted identity line is refused" [ "$status" -eq 1 ]
check "refusal: malformed identity was not executed" [ ! -e "$work/pwned" ]

git -C "$root" status --porcelain -- targets/boards docs/board-support > "$work/status-after"
check "isolation: real boards tree untouched" cmp -s "$work/status-before" "$work/status-after"

finish match
