#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
workflow="$root/tools/pixel-9a/firmware.sh"

[ -x "$workflow" ] || {
  echo 'firmware workflow is not executable' >&2
  exit 1
}
sh -n "$workflow"

# Firmware handling must stay offline and must not contain a device transport
# or a persistent provisioning verb.
if grep -nE '\b(adb|fastboot|flash|erase|format|reboot|sideload|unlock)\b' "$workflow" >/dev/null; then
  echo 'firmware workflow contains an out-of-scope device operation' >&2
  exit 1
fi

"$workflow" preflight
policy="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/firmware-policy.toml"
allowlist="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/firmware-stage-allowlist.tsv"
[ -f "$allowlist" ]
grep -q '^kernel_driver = "bcmdhd4383"$' "$policy"
grep -q '^kernel_driver = "nitrous"$' "$policy"
grep -q '^kernel_driver = "cpif"$' "$policy"
awk -F '\t' '$0 !~ /^[[:space:]]*#/ && NF == 4 { count++ } END { exit(count == 11 ? 0 : 1) }' "$allowlist"
awk -F '\t' '$0 !~ /^[[:space:]]*#/ && NF == 5 && $5 == "bundle" { count++ } END { exit(count == 2 ? 0 : 1) }' "$allowlist"
awk -F '\t' '$0 !~ /^[[:space:]]*#/ && (NF == 4 || (NF == 5 && $5 == "bundle")) { if (seen[$4]++) bad=1 } END { exit(bad ? 1 : 0) }' "$allowlist"
awk -F '\t' '$1 == "bluetooth-broadcom" && $2 == "vendor.img" && $3 == "/firmware/brcm/BCM.hcd" && $4 == "lib/firmware/brcm/BCM.hcd" { found=1 } END { exit(found ? 0 : 1) }' \
  "$allowlist" >/dev/null
awk -F '\t' '$1 == "wifi-bcm4383" && $2 == "vendor.img" && $3 == "/firmware/bcmdhd.cal_4383_a3" && ($4 == "lib/firmware/bcmdhd.cal" || $4 == "lib/firmware/bcmdhd.cal_4383_a3") { count++ } END { exit(count == 2 ? 0 : 1) }' \
  "$allowlist" >/dev/null
awk -F '\t' '$1 == "wifi-bcm4383" && $2 == "vendor.img" && $3 == "/firmware/bcmdhd_clm.blob_4383_a3" && ($4 == "lib/firmware/bcmdhd_clm.blob" || $4 == "lib/firmware/bcmdhd_clm.blob_4383_a3") { count++ } END { exit(count == 2 ? 0 : 1) }' \
  "$allowlist" >/dev/null
awk -F '\t' '$1 == "modem-s5300" && $2 == "vendor.img" && $3 == "/firmware/carrierconfig" && $4 == "lib/firmware/carrierconfig" && $5 == "bundle" { found=1 } END { exit(found ? 0 : 1) }' \
  "$allowlist" >/dev/null
awk -F '\t' '$1 == "modem-s5300" && $2 == "vendor.img" && $3 == "/firmware/uecapconfig" && $4 == "lib/firmware/uecapconfig" && $5 == "bundle" { found=1 } END { exit(found ? 0 : 1) }' \
  "$allowlist" >/dev/null

# A replacement allowlist is still parsed before any archive or host-tool
# work. Malformed rows must fail closed rather than reaching extraction.
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-firmware-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
printf '%s\n' 'wifi-bcm4383	vendor.img	/firmware/fw_bcmdhd.bin' > "$work/malformed.tsv"
if FERRIX_PIXEL9A_FIRMWARE=I-understand-this-extracts-verified-firmware \
  FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST="$work/malformed.tsv" \
  FERRIX_PIXEL9A_FIRMWARE_OUT="$work/out" "$workflow" stage >"$work/stdout" 2>"$work/stderr"; then
  echo 'malformed firmware allowlist was accepted' >&2
  exit 1
fi
grep -q 'firmware stage allowlist must contain four fields' "$work/stderr"
printf '%s\n' \
  'wifi-bcm4383	vendor.img	/firmware/fw_bcmdhd.bin	lib/firmware/fw_bcmdhd.bin' \
  'bluetooth-broadcom	vendor.img	/firmware/brcm/BCM.hcd	lib/firmware/fw_bcmdhd.bin' > "$work/duplicate.tsv"
if FERRIX_PIXEL9A_FIRMWARE=I-understand-this-extracts-verified-firmware \
  FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST="$work/duplicate.tsv" \
  FERRIX_PIXEL9A_FIRMWARE_OUT="$work/out-duplicate" "$workflow" stage >"$work/duplicate-stdout" 2>"$work/duplicate-stderr"; then
  echo 'duplicate firmware runtime path was accepted' >&2
  exit 1
fi
grep -q 'unique runtime paths' "$work/duplicate-stderr"

# Exercise directory bundles with a fake ext4/debugfs surface. This remains
# device-free while covering regular-file extraction and fail-closed handling
# for symlink, traversal, and empty entries.
confirmation=I-understand-this-extracts-verified-firmware
fake_bin="$work/fake-bin"
mkdir -p "$fake_bin" "$work/archive-root"
printf '%s\n' 'synthetic-vendor-filesystem' >"$work/archive-root/vendor.img"
(cd "$work/archive-root" && zip -q "$work/image-tegu-synthetic.zip" vendor.img)
(cd "$work" && zip -q "$work/factory.zip" image-tegu-synthetic.zip)
if command -v sha256sum >/dev/null 2>&1; then
  factory_hash=$(sha256sum "$work/factory.zip" | awk '{print $1}')
else
  factory_hash=$(shasum -a 256 "$work/factory.zip" | awk '{print $1}')
fi
factory_bytes=$(wc -c <"$work/factory.zip" | tr -d '[:space:]')
printf '# kind\tbuild\tversion\tfilename\turl\tsha256\tbytes\tsource-page\n' >"$work/recovery.pins"
printf 'factory\tSYN\t0\tfactory.zip\thttps://example.invalid/factory\t%s\t%s\thttps://example.invalid\n' \
  "$factory_hash" "$factory_bytes" >>"$work/recovery.pins"
printf '%s\n' 'modem-s5300	vendor.img	/firmware/carrierconfig	lib/firmware/carrierconfig	bundle' >"$work/bundle.tsv"
printf '%s\n' '#!/bin/sh' 'set -eu' 'command=$2' 'case "$command" in' \
  '  "stat /firmware/carrierconfig") printf "%s\n" "Inode: 42 Type: directory" ;;' \
  '  "stat /firmware/missing.cal") printf "%s\n" "/firmware/missing.cal: File not found by ext2_lookup" ;;' \
  '  "ls -p /firmware/carrierconfig")' \
  '    printf "%s\n" "/2/040755/0/0/.//" "/3/040755/0/0/..//"' \
  '    case "$FERRIX_FAKE_BUNDLE_MODE" in' \
  '      regular) printf "%s\n" "/4/100644/0/0/carrier-extra.conf/15/" "/5/100644/0/0/carrier.conf/15/" ;;' \
  '      symlink) printf "%s\n" "/4/120777/0/0/carrier.conf/15/" ;;' \
  '      traversal) printf "%s\n" "/4/100644/0/0/..evil/15/" ;;' \
  '      special) printf "%s\n" "/4/020600/0/0/character-device/0/" ;;' \
  '      empty) ;;' \
  '      *) exit 2 ;;' \
  '    esac ;;' \
  '  "ls -p /firmware") printf "%s\n" "/2/040755/0/0/.//" "/3/040755/0/0/..//" "/6/100644/0/0/fw_bcmdhd.bin/15/" ;;' \
  '  dump*) destination=$(printf "%s\n" "$command" | sed "s#^dump [^ ]* ##"); mkdir -p "$(dirname "$destination")"; printf "%s\n" synthetic-carrier-config >"$destination" ;;' \
  '  *) exit 2 ;;' \
  'esac' >"$fake_bin/debugfs"
chmod 0755 "$fake_bin/debugfs"
FERRIX_FAKE_BUNDLE_MODE=regular \
FERRIX_PIXEL9A_PINS="$work/recovery.pins" \
FERRIX_PIXEL9A_FACTORY_ARCHIVE="$work/factory.zip" \
FERRIX_PIXEL9A_FIRMWARE="$confirmation" \
FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST="$work/bundle.tsv" \
FERRIX_PIXEL9A_FIRMWARE_OUT="$work/bundle-out" \
PATH="$fake_bin:$PATH" "$workflow" stage >/dev/null
[ -f "$work/bundle-out/staged/lib/firmware/carrierconfig/carrier.conf" ] ||
  { echo 'bundle extractor did not stage a regular file' >&2; exit 1; }
[ -f "$work/bundle-out/staged/lib/firmware/carrierconfig/carrier-extra.conf" ] ||
  { echo 'bundle extractor did not stage the second regular file' >&2; exit 1; }
grep -q '^firmware\tmodem-s5300\tvendor.img\t/firmware/carrierconfig/carrier.conf\tlib/firmware/carrierconfig/carrier.conf\t' \
  "$work/bundle-out/firmware-manifest.tsv"
FERRIX_FAKE_BUNDLE_MODE=regular \
FERRIX_PIXEL9A_PINS="$work/recovery.pins" \
FERRIX_PIXEL9A_FACTORY_ARCHIVE="$work/factory.zip" \
FERRIX_PIXEL9A_FIRMWARE="$confirmation" \
FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST="$work/bundle.tsv" \
FERRIX_PIXEL9A_FIRMWARE_OUT="$work/source-inspect-out" \
PATH="$fake_bin:$PATH" "$workflow" inspect >"$work/source-inspect.out"
grep -q '== modem-s5300: vendor.img:/firmware/carrierconfig -> lib/firmware/carrierconfig (bundle) ==' \
  "$work/source-inspect.out"
grep -q 'Inode: 42 Type: directory' "$work/source-inspect.out"
printf 'wifi-bcm4383\tvendor.img\t/firmware/missing.cal\tlib/firmware/missing.cal\n' >"$work/missing-source.tsv"
FERRIX_PIXEL9A_PINS="$work/recovery.pins" \
FERRIX_PIXEL9A_FACTORY_ARCHIVE="$work/factory.zip" \
FERRIX_PIXEL9A_FIRMWARE="$confirmation" \
FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST="$work/missing-source.tsv" \
FERRIX_PIXEL9A_FIRMWARE_OUT="$work/missing-source-out" \
PATH="$fake_bin:$PATH" "$workflow" inspect >"$work/missing-source.out"
grep -q 'source entry is unresolved; listing parent /firmware' "$work/missing-source.out"
grep -q 'fw_bcmdhd.bin' "$work/missing-source.out"
mkdir -p "$work/overwrite-out/staged/lib/firmware/carrierconfig"
ln -s "$work/archive-root/vendor.img" \
  "$work/overwrite-out/staged/lib/firmware/carrierconfig/carrier.conf"
if FERRIX_FAKE_BUNDLE_MODE=regular \
  FERRIX_PIXEL9A_PINS="$work/recovery.pins" \
  FERRIX_PIXEL9A_FACTORY_ARCHIVE="$work/factory.zip" \
  FERRIX_PIXEL9A_FIRMWARE="$confirmation" \
  FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST="$work/bundle.tsv" \
  FERRIX_PIXEL9A_FIRMWARE_OUT="$work/overwrite-out" \
  PATH="$fake_bin:$PATH" "$workflow" stage >"$work/overwrite.stdout" 2>"$work/overwrite.stderr"; then
  echo 'bundle extractor overwrote an existing destination symlink' >&2
  exit 1
fi
rm -f "$work/overwrite-out/staged/lib/firmware/carrierconfig/carrier.conf"
printf '%s\n' pre-existing >"$work/overwrite-out/staged/lib/firmware/carrierconfig/carrier.conf"
if FERRIX_FAKE_BUNDLE_MODE=regular \
  FERRIX_PIXEL9A_PINS="$work/recovery.pins" \
  FERRIX_PIXEL9A_FACTORY_ARCHIVE="$work/factory.zip" \
  FERRIX_PIXEL9A_FIRMWARE="$confirmation" \
  FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST="$work/bundle.tsv" \
  FERRIX_PIXEL9A_FIRMWARE_OUT="$work/overwrite-out" \
  PATH="$fake_bin:$PATH" "$workflow" stage >"$work/overwrite-file.stdout" 2>"$work/overwrite-file.stderr"; then
  echo 'bundle extractor overwrote an existing destination file' >&2
  exit 1
fi
grep -q 'already exists' "$work/overwrite-file.stderr"
for mode in symlink traversal empty special; do
  if FERRIX_FAKE_BUNDLE_MODE="$mode" \
    FERRIX_PIXEL9A_PINS="$work/recovery.pins" \
    FERRIX_PIXEL9A_FACTORY_ARCHIVE="$work/factory.zip" \
    FERRIX_PIXEL9A_FIRMWARE="$confirmation" \
    FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST="$work/bundle.tsv" \
    FERRIX_PIXEL9A_FIRMWARE_OUT="$work/bundle-out-$mode" \
    PATH="$fake_bin:$PATH" "$workflow" stage >"$work/$mode.stdout" 2>"$work/$mode.stderr"; then
    echo "bundle extractor accepted $mode input" >&2
    exit 1
  fi
done
grep -q 'symlink' "$work/symlink.stderr"
grep -q 'unsafe path component' "$work/traversal.stderr"
grep -q 'empty' "$work/empty.stderr"
grep -q 'non-regular, non-directory entry' "$work/special.stderr"
printf '%s\n' 'pixel-9a firmware workflow tests passed'
