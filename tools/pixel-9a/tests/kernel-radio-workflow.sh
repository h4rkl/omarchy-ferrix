#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
workflow="$root/tools/pixel-9a/kernel-radio.sh"
route="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/kernel-radio-route.toml"
work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-kernel-radio-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM

fail() {
	printf 'radio kernel workflow test failed: %s\n' "$*" >&2
	exit 1
}

[ -x "$workflow" ] || fail 'radio kernel workflow is not executable'
[ -f "$route" ] || fail 'radio route metadata is missing'
sh -n "$workflow" || fail 'shell syntax'
grep -q 'experimental-linux-google-tegu-s5300-wwan' "$workflow" || fail 'WWAN route id is not checked'
grep -q 'linux-google-tegu-wwan' "$workflow" || fail 'kernel/userspace backend contract is not checked'
grep -q 'kbuild-make' "$workflow" || fail 'independent Kbuild contract is not checked'
grep -q 'verify_source_archive' "$workflow" || fail 'source archive integrity is not checked'
grep -q 'curl --fail --location' "$workflow" || fail 'source archive download is not pinned'
grep -q '^source_clone_depth=1$' "$workflow" || fail 'radio source clone depth is not pinned shallow'
grep -q '^source_clone_filter=blob:none$' "$workflow" || fail 'radio source clone filter is not pinned'
grep -q -- '--single-branch' "$workflow" || fail 'radio source clone is not single-branch'
grep -q -- '--no-checkout' "$workflow" || fail 'radio source clone does not defer checkout'
grep -q 'rev-parse --is-shallow-repository' "$workflow" || fail 'radio source shallow state is not verified'
grep -q 'rev-list --count HEAD' "$workflow" || fail 'radio source depth is not verified'
grep -q 'remote.origin.promisor' "$workflow" || fail 'radio source promisor remote is not verified'
grep -q 'remote.origin.partialclonefilter' "$workflow" || fail 'radio source partial-clone filter is not verified'
grep -q 'radio kernel source remote mismatch' "$workflow" || fail 'radio source remote is not pinned'
for symbol in CONFIG_NET CONFIG_NETDEVICES CONFIG_INET CONFIG_IPV6 CONFIG_S5300_MODEM CONFIG_SPI CONFIG_GNSS CONFIG_GNSS_KEPLER CONFIG_BLK_DEV_DM CONFIG_BRCMFMAC_PCIE CONFIG_BT_HCIUART_BCM CONFIG_BT_HCIUART_H4 CONFIG_BT_HCIUART_SERDEV CONFIG_SERIAL_DEV_BUS; do
	grep -q "$symbol" "$route" || fail "radio route omits required data/radio symbol: $symbol"
done
grep -q '^extra_config_fragment = "kernel-radio-extra.config"$' "$route" || fail 'radio route does not pin its extra config fragment'
grep -q '^initfs_module_list = "pmos/device-google-tegu/modules-initfs-radio"$' "$route" || fail 'radio route does not pin its route-local initfs module list'
grep -q '^split_artifact_contract = "radio-split-artifact-contract.toml"$' "$route" || fail 'radio route does not pin its split-artifact contract'
grep -q 'split_artifact_contract_sha256' "$route" || fail 'radio route does not hash its split-artifact contract'
grep -Fqx 'gnss_binding = "samsung,kepler"' "$route" || fail 'radio route does not pin the KEPLER binding'
grep -Fqx 'gnss_driver_source = "drivers/gnss/kepler.c"' "$route" || fail 'radio route does not pin the KEPLER driver source'
grep -Fqx 'gnss_runtime_device_glob = "/dev/gnss*"' "$route" || fail 'radio route does not pin the GNSS raw-device contract'
grep -Fqx 'gnss_userspace_status = "raw-device-only"' "$route" || fail 'radio route overclaims GNSS userspace'
grep -Fqx 'nfc_route_status = "not-selected"' "$route" || fail 'radio route selects an NFC backend'
grep -Fqx 'nfc_comparison_modules = "st21nfc.ko,st54spi.ko"' "$route" || fail 'radio route loses the NFC comparison-module boundary'
grep -Fqx 'st54_route_status = "not-selected"' "$route" || fail 'radio route selects an ST54 backend'
grep -q 'check_split_contract' "$workflow" || fail 'radio workflow does not validate its split-artifact contract'
grep -q 'modules_install' "$workflow" || fail 'radio workflow does not materialize source-built module inputs'
grep -q 'dtbo-source-set' "$workflow" || fail 'radio workflow does not preserve source-built DTB/DTBO inputs'
grep -q 'partition-sizes' "$workflow" || fail 'radio workflow does not record split partition build status'
contract="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/radio-split-artifact-contract.toml"
[ -f "$contract" ] || fail 'radio split-artifact contract is missing'
for split_field in 'source_only = true' 'stock_modules_may_be_reused = false' 'android_userspace_required = false' 'boot_header_version = 4' 'dtbo_tool = "mkdtboimg.py"' 'dtbo_page_size = 4096'; do
	grep -Fqx "$split_field" "$contract" || fail "split-artifact contract omits $split_field"
done
for split_partition in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
	grep -Fqx "[artifact.$split_partition]" "$contract" || fail "split-artifact contract omits $split_partition"
done
[ -f "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/kernel-radio-extra.config" ] || fail 'radio extra config fragment is missing'
[ -f "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/modules-initfs-radio" ] || fail 'radio initfs module list is missing'
if awk '!/^[[:space:]]*(#|$)/ { found = 1 } END { exit found ? 0 : 1 }' \
	"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/modules-initfs-radio"; then
	fail 'radio initfs module list must remain empty until a native build proves a module is needed'
fi
if grep -Eiq '(^|[^[:alnum:]_.-])(bcm47765|st21nfc|st54spi|kepler)([^[:alnum:]_.-]|$)' \
	"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/modules-initfs-radio"; then
	fail 'radio initfs module list carries an Android associated-radio module'
fi
[ -f "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/associated-radio-audit.md" ] || fail 'associated-radio audit is missing'
for associated_radio in 'samsung,kepler' 'st21nfc' 'st54spi' 'AOC_UWB_DRIVER'; do
	grep -q "$associated_radio" "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/associated-radio-audit.md" || fail "associated-radio audit omits $associated_radio"
done
grep -q '^CONFIG_BLK_DEV_DM=y$' "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/kernel-radio-extra.config" || fail 'radio extra config does not make device-mapper built in'
if grep -Eiq '(^|[^[:alnum:]_-])(adb|fastboot)([^[:alnum:]_-]|$)' "$workflow"; then
	fail 'radio kernel workflow contains device transport tooling'
fi

mkdir -p "$work/bin" "$work/cache" "$work/out"
cat >"$work/bin/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) printf '%s\n' Darwin ;;
  -m) printf '%s\n' arm64 ;;
  *) exit 2 ;;
esac
EOF
chmod 0755 "$work/bin/uname"

if PATH="$work/bin:$PATH" FERRIX_CACHE_DIR="$work/cache" FERRIX_OUT_DIR="$work/out" \
	"$workflow" preflight >"$work/darwin.out" 2>"$work/darwin.err"; then
	fail 'non-Linux host was accepted'
fi
grep -q 'native Linux host required' "$work/darwin.err" || fail 'host refusal reason'
[ ! -e "$work/cache/pixel-9a" ] || fail 'host refusal created cache'
[ ! -e "$work/out/pixel-9a" ] || fail 'host refusal created output'

if FERRIX_CACHE_DIR="$work/cache" FERRIX_OUT_DIR="$work/out" \
	"$workflow" verify >"$work/verify.out" 2>"$work/verify.err"; then
	fail 'verify without a source checkout unexpectedly passed'
fi
grep -q 'radio kernel source is not initialized' "$work/verify.err" || fail 'missing-source refusal reason'

printf '%s\n' 'pixel-9a radio kernel workflow tests passed'
