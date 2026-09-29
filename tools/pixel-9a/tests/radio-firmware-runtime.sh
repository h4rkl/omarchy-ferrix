#!/bin/sh
# Device-free test for the target-local OpenRC firmware gate. It sources the
# same service script against a synthetic filesystem and exercises both the
# positive handoff and fail-closed cases; no host /lib/firmware paths are used.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
service="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/ferrix-radio-firmware.initd"
[ -f "$service" ] || { echo 'radio firmware service is missing' >&2; exit 1; }
sh -n "$service"

work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-radio-firmware-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
guest="$work/guest"
mkdir -p "$guest/lib/firmware/brcm" "$guest/usr/share/ferrix/pixel-9a"

hash_file() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | awk '{ print $1 }'
	else
		shasum -a 256 "$1" | awk '{ print $1 }'
	fi
}

write_file() {
	path=$1
	content=$2
	mkdir -p "$guest/$(dirname -- "$path")"
	printf '%s' "$content" >"$guest/$path"
}

write_file lib/firmware/fw_bcmdhd.bin wifi-bin
write_file lib/firmware/bcmdhd.cal wifi-cal-a3
write_file lib/firmware/bcmdhd.cal_4383_a3 wifi-cal-a3
write_file lib/firmware/bcmdhd_clm.blob wifi-clm-a3
write_file lib/firmware/bcmdhd_clm.blob_4383_a3 wifi-clm-a3
write_file lib/firmware/fw_bcmdhd.map wifi-map
write_file lib/firmware/bcmdhd_txcap.blob wifi-txcap-a3
write_file lib/firmware/bcmdhd_txcap.blob_4383_a3 wifi-txcap-a3
write_file lib/firmware/fw_bcmdhd.bin_4383_a3 wifi-bin-a3
write_file lib/firmware/fw_bcmdhd.map_4383_a3 wifi-map-a3
write_file lib/firmware/brcm/BCM.hcd bluetooth-patchram
write_file lib/firmware/carrierconfig/carrier.conf carrier-config
write_file lib/firmware/carrierconfig/carrier-extra.conf carrier-extra
write_file lib/firmware/uecapconfig/capability.conf uecap-config

manifest="$guest/usr/share/ferrix/pixel-9a/firmware-manifest.tsv"
{
	printf 'schema_version\t1\n'
	printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
	printf 'factory_archive\t/path/to/factory.zip\t0000000000000000000000000000000000000000000000000000000000000000\n'
	printf 'source\tvendor.img\t1\t0000000000000000000000000000000000000000000000000000000000000000\n'
} >"$manifest"

manifest_row() {
	controller=$1
	source_path=$2
	runtime_path=$3
	file="$guest/$runtime_path"
	printf 'firmware\t%s\tvendor.img\t%s\t%s\t%s\t%s\n' \
		"$controller" "$source_path" "$runtime_path" \
		"$(wc -c <"$file" | tr -d '[:space:]')" "$(hash_file "$file")" >>"$manifest"
}

manifest_row wifi-bcm4383 /firmware/fw_bcmdhd.bin lib/firmware/fw_bcmdhd.bin
manifest_row wifi-bcm4383 /firmware/bcmdhd.cal_4383_a3 lib/firmware/bcmdhd.cal
manifest_row wifi-bcm4383 /firmware/bcmdhd_clm.blob_4383_a3 lib/firmware/bcmdhd_clm.blob
manifest_row wifi-bcm4383 /firmware/fw_bcmdhd.map lib/firmware/fw_bcmdhd.map
manifest_row wifi-bcm4383 /firmware/bcmdhd_txcap.blob_4383_a3 lib/firmware/bcmdhd_txcap.blob
manifest_row wifi-bcm4383 /firmware/fw_bcmdhd.bin_4383_a3 lib/firmware/fw_bcmdhd.bin_4383_a3
manifest_row wifi-bcm4383 /firmware/bcmdhd.cal_4383_a3 lib/firmware/bcmdhd.cal_4383_a3
manifest_row wifi-bcm4383 /firmware/bcmdhd_clm.blob_4383_a3 lib/firmware/bcmdhd_clm.blob_4383_a3
manifest_row wifi-bcm4383 /firmware/fw_bcmdhd.map_4383_a3 lib/firmware/fw_bcmdhd.map_4383_a3
manifest_row wifi-bcm4383 /firmware/bcmdhd_txcap.blob_4383_a3 lib/firmware/bcmdhd_txcap.blob_4383_a3
manifest_row bluetooth-broadcom /firmware/brcm/BCM.hcd lib/firmware/brcm/BCM.hcd
manifest_row modem-s5300 /firmware/carrierconfig/carrier.conf lib/firmware/carrierconfig/carrier.conf
manifest_row modem-s5300 /firmware/carrierconfig/carrier-extra.conf lib/firmware/carrierconfig/carrier-extra.conf
manifest_row modem-s5300 /firmware/uecapconfig/capability.conf lib/firmware/uecapconfig/capability.conf

allowlist="$guest/usr/share/ferrix/pixel-9a/firmware-allowlist.tsv"
cat >"$allowlist" <<'EOF'
# component	runtime destination	required
bcm4383	lib/firmware/fw_bcmdhd.bin	1
bcm4383	lib/firmware/bcmdhd.cal	1
bcm4383	lib/firmware/bcmdhd_clm.blob	1
bcm4383	lib/firmware/fw_bcmdhd.map	1
bcm4383	lib/firmware/bcmdhd_txcap.blob	1
bcm4383	lib/firmware/fw_bcmdhd.bin_4383_a3	1
bcm4383	lib/firmware/bcmdhd.cal_4383_a3	1
bcm4383	lib/firmware/bcmdhd_clm.blob_4383_a3	1
bcm4383	lib/firmware/fw_bcmdhd.map_4383_a3	1
bcm4383	lib/firmware/bcmdhd_txcap.blob_4383_a3	1
bluetooth-broadcom	lib/firmware/brcm/BCM.hcd	1
modem-s5300	lib/firmware/carrierconfig	1
modem-s5300	lib/firmware/uecapconfig	1
EOF

for mapping in \
	'lib/firmware/fw_bcmdhd.bin_4383_a3 lib/firmware/brcm/brcmfmac4383-pcie.bin' \
	'lib/firmware/bcmdhd.cal_4383_a3 lib/firmware/brcm/brcmfmac4383-pcie.txt' \
	'lib/firmware/bcmdhd_clm.blob_4383_a3 lib/firmware/brcm/brcmfmac4383-pcie.clm_blob' \
	'lib/firmware/bcmdhd_txcap.blob_4383_a3 lib/firmware/brcm/brcmfmac4383-pcie.txcap_blob'; do
	set -- $mapping
	cp "$guest/$1" "$guest/$2"
done

radio_manifest="$guest/usr/share/ferrix/pixel-9a/radio-firmware-manifest.tsv"
write_radio_manifest() {
	{
		printf 'schema_version\t1\n'
		printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
		printf 'route_id\texperimental-linux-google-tegu-s5300-wwan\n'
		printf 'kernel_backend\tlinux-google-tegu-wwan\n'
		printf 'source_policy\tcanonical-factory-manifest-explicit-aliases\n'
	} >"$radio_manifest"
	for mapping in \
		'lib/firmware/fw_bcmdhd.bin_4383_a3 lib/firmware/brcm/brcmfmac4383-pcie.bin' \
		'lib/firmware/bcmdhd.cal_4383_a3 lib/firmware/brcm/brcmfmac4383-pcie.txt' \
		'lib/firmware/bcmdhd_clm.blob_4383_a3 lib/firmware/brcm/brcmfmac4383-pcie.clm_blob' \
		'lib/firmware/bcmdhd_txcap.blob_4383_a3 lib/firmware/brcm/brcmfmac4383-pcie.txcap_blob'; do
		set -- $mapping
		bytes=$(wc -c <"$guest/$2" | tr -d '[:space:]')
		hash=$(hash_file "$guest/$2")
		printf 'alias\t%s\t%s\t%s\t%s\n' "$1" "$2" "$bytes" "$hash" >>"$radio_manifest"
	done
}
write_radio_manifest

eerror() { printf '%s\n' "$*" >&2; }
einfo() { :; }
need() { :; }
before() { :; }
export FERRIX_PIXEL9A_FIRMWARE_ROOT="$guest"
export FERRIX_PIXEL9A_FIRMWARE_MANIFEST="$manifest"
export FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST="$allowlist"
export FERRIX_PIXEL9A_RADIO_FIRMWARE_MANIFEST="$radio_manifest"
# The service is an OpenRC script; sourcing it exposes start_pre without
# trying to start a host service.
. "$service"

run_gate() {
	start_pre >"$work/gate.out" 2>"$work/gate.err"
}

run_gate

# A hash-preserving alias with a different destination must still be rejected:
# runtime validation binds the manifest to the reviewed radio route, not just
# to four arbitrary copies.
cp "$guest/lib/firmware/brcm/brcmfmac4383-pcie.bin" \
	"$guest/lib/firmware/brcm/not-reviewed.bin"
cp "$radio_manifest" "$work/good-radio-manifest"
sed 's#brcmfmac4383-pcie\.bin#not-reviewed.bin#' "$radio_manifest" >"$work/wrong-radio-manifest"
cp "$work/wrong-radio-manifest" "$radio_manifest"
if run_gate; then
	echo 'FAIL: accepted a radio alias outside the reviewed source/destination route' >&2
	exit 1
fi
cp "$work/good-radio-manifest" "$radio_manifest"

# Four rows are not sufficient by themselves: a duplicated reviewed pair could
# otherwise hide a missing firmware alias. The runtime gate must require one
# row for every checked-in source/destination mapping.
awk -F '\t' -v OFS='\t' '
	$1 == "alias" && $2 == "lib/firmware/bcmdhd.cal_4383_a3" { next }
	{ print }
' "$radio_manifest" >"$work/missing-radio-manifest"
cp "$work/missing-radio-manifest" "$radio_manifest"
if run_gate; then
	echo 'FAIL: accepted a radio alias manifest with a missing reviewed pair' >&2
	exit 1
fi
write_radio_manifest

awk -F '\t' -v OFS='\t' '
	{ print }
	$1 == "alias" && $2 == "lib/firmware/fw_bcmdhd.bin_4383_a3" { print }
' "$radio_manifest" >"$work/duplicate-radio-manifest"
cp "$work/duplicate-radio-manifest" "$radio_manifest"
if run_gate; then
	echo 'FAIL: accepted a radio alias manifest with a duplicated reviewed pair' >&2
	exit 1
fi
write_radio_manifest

cp "$manifest" "$work/good-manifest"
write_file lib/firmware/unreviewed.bin unreviewed-payload
manifest_row unreviewed-controller /firmware/unreviewed.bin lib/firmware/unreviewed.bin
if run_gate; then
	echo 'FAIL: accepted a firmware row outside the reviewed allowlist' >&2
	exit 1
fi
cp "$work/good-manifest" "$manifest"

printf '%s\n' 'tampered-hcd' >"$guest/lib/firmware/brcm/BCM.hcd"
if run_gate; then
	echo 'FAIL: accepted a changed BCM.hcd payload' >&2
	exit 1
fi
printf '%s' bluetooth-patchram >"$guest/lib/firmware/brcm/BCM.hcd"

rm -f "$radio_manifest"
if run_gate; then
	echo 'FAIL: accepted a missing radio alias manifest' >&2
	exit 1
fi
write_radio_manifest

sed 's/google-pixel-9a-tegu-gxq96-mp1-0/wrong-target/' "$manifest" >"$work/wrong-manifest"
cp "$work/wrong-manifest" "$manifest"
if run_gate; then
	echo 'FAIL: accepted a firmware manifest for another target' >&2
	exit 1
fi

printf '%s\n' 'pixel-9a runtime radio firmware gate tests passed'
