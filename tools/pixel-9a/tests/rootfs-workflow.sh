#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script="$root/tools/pixel-9a/rootfs.sh"
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-pixel9a-rootfs.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
tab=$(printf '\t')

fail() {
	echo "FAIL: $*" >&2
	exit 1
}

hash_file() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | awk '{print $1}'
	else
		shasum -a 256 "$1" | awk '{print $1}'
	fi
}

sha512_file() {
	if command -v sha512sum >/dev/null 2>&1; then
		sha512sum "$1" | awk '{print $1}'
	else
		shasum -a 512 "$1" | awk '{print $1}'
	fi
}

sh -n "$script"
if grep -q 'targets/qemu-aarch64/pmos/pins.env' "$script"; then
	fail 'physical rootfs workflow must not inherit the QEMU pmaports pin'
fi
for file in ferrix-core.initd ferrix-networkd.initd ferrix-telephonyd.initd ferrix-audio.initd ferrix-product.initd ferrix-s5300-mounts.initd ferrix-radio-firmware.initd ferrix-aoc-firmware.initd exynos-modem.initd sit-proxy.initd; do
	sh -n "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/$file"
done

package_dir="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu"
pmos_dir="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos"
[ -f "$pmos_dir/pins.env" ] || fail 'target-local pmaports pin file is missing'
grep -Eq '^PMAPORTS_COMMIT=[0-9a-fA-F]{40}$' "$pmos_dir/pins.env" ||
	fail 'target-local pmaports pin is not a full Git revision'
[ -f "$package_dir/10-ignore-aware-nmi.conf" ] || fail 'device package is missing the Tegu aware_nmi NetworkManager guard'
grep -q '^unmanaged-devices=interface-name:aware_nmi\*$' "$package_dir/10-ignore-aware-nmi.conf" ||
	fail 'Tegu NetworkManager guard does not leave aware_nmi* unmanaged'
grep -q '10-ignore-aware-nmi.conf' "$package_dir/APKBUILD" ||
	fail 'device package does not install the Tegu aware_nmi NetworkManager guard'
[ -f "$package_dir/ferrix-radio-firmware.initd" ] || fail 'device package is missing the runtime radio firmware gate'
grep -q 'ferrix-radio-firmware.initd' "$package_dir/APKBUILD" ||
	fail 'device package does not install the runtime radio firmware gate'
grep -q 'need .*ferrix-radio-firmware' "$package_dir/ferrix-core.initd" ||
	fail 'Ferrix core does not require the runtime radio firmware gate'
grep -q 'before networkmanager bluetooth' "$package_dir/ferrix-radio-firmware.initd" ||
	fail 'runtime radio firmware gate does not order before NetworkManager and BlueZ'
grep -q 'ferrix-aoc-firmware.initd' "$package_dir/APKBUILD" ||
	fail 'device package does not install the AoC firmware loader'
grep -q 'for service in .* ferrix-aoc-firmware .*; do' "$package_dir/APKBUILD" ||
	fail 'AoC firmware loader is not in the default runlevel'
if grep -Eq 'need .*ferrix-aoc-firmware' "$package_dir"/*.initd; then
	fail 'a service needs ferrix-aoc-firmware; an AoC failure must not block the session'
fi
[ -f "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/ubuntu-touch-radio-correlation.md" ] ||
	fail 'target is missing the external Ubuntu Touch radio correlation boundary'
grep -q 'Halium/Android-based' "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/ubuntu-touch-radio-correlation.md" ||
	fail 'Ubuntu Touch radio correlation does not document its Android boundary'
for package in dbus-openrc eudev-openrc networkmanager-openrc modemmanager-samsung-sit modemmanager-samsung-sit-openrc exynos-modem bluez-openrc seatd-openrc pipewire-spa-bluez; do
	grep -Eq "^[[:space:]]*$package[[:space:]]*$" "$package_dir/APKBUILD" ||
		fail "device package is missing runtime dependency: $package"
done
if grep -Eq '^[[:space:]]*modemmanager(-openrc)?[[:space:]]*$' "$package_dir/APKBUILD"; then
	fail 'device package still selects the generic ModemManager provider'
fi
for recipe in "$pmos_dir/exynos-modem/APKBUILD" "$pmos_dir/modemmanager-samsung-sit/APKBUILD"; do
	[ -f "$recipe" ] || fail "missing target-local package recipe: $recipe"
done
grep -q '634e8eb404c3fcf959dce6a0b0d97b33ba0fc346' "$pmos_dir/exynos-modem/APKBUILD" ||
	fail 'exynos-modem recipe is not pinned to the reviewed commit'
grep -q '14a0013a28e5d5eec513c65e75d7549678ecc61e' "$pmos_dir/modemmanager-samsung-sit/APKBUILD" ||
	fail 'ModemManager recipe is not pinned to the reviewed commit'
grep -q 'fa081999130a9775a9140fa16901d4dcc9a2733ebd9af1eb55b03717659a952' "$pmos_dir/exynos-modem/APKBUILD" ||
	fail 'exynos-modem recipe archive hash is missing'
grep -q '9add05d1a18a662cb0c0f6b9048279355a8d1b2933263df0f74fbb791cb97265' "$pmos_dir/modemmanager-samsung-sit/APKBUILD" ||
	fail 'ModemManager recipe archive hash is missing'
[ -f "$pmos_dir/waydroid/APKBUILD" ] || fail 'missing the Waydroid 1.6.3 backport recipe'
grep -q '^pkgver=1\.6\.3$' "$pmos_dir/waydroid/APKBUILD" || fail 'Waydroid backport is not 1.6.3'
grep -q '^_aports=3c2c2436e397ea23073cde73138f60e0d5c537b4$' "$pmos_dir/waydroid/APKBUILD" ||
	fail 'Waydroid backport helper files are not pinned to an aports commit'
if grep -q 'rm -f .cargo/config.toml' "$pmos_dir/exynos-modem/APKBUILD"; then
	fail 'exynos-modem recipe removes the reviewed target configuration'
fi
grep -q 'aarch64-alpine-linux-musl' "$pmos_dir/exynos-modem/APKBUILD" ||
	fail 'exynos-modem recipe does not pin the Alpine target Rust output'
grep -q '[[:space:]]modules-initfs-radio$' "$package_dir/APKBUILD" ||
	fail 'device package does not carry the radio-first initfs module list'
grep -q 'cargo test --frozen --tests --target="\$_cargo_target" --no-run' "$pmos_dir/exynos-modem/APKBUILD" ||
	fail 'exynos-modem cross-builder checks must compile target tests without executing them'
grep -q 'cargo test --frozen --tests --target="\$_host_target"' "$pmos_dir/exynos-modem/APKBUILD" ||
	fail 'exynos-modem recipe must execute tests for the package-builder target'
if grep -q -- '-p samsung-sit\([[:space:]]\|$\)' "$pmos_dir/exynos-modem/APKBUILD"; then
	fail 'exynos-modem recipe names the obsolete samsung-sit workspace package'
fi
grep -q -- '-p samsung-sit-ffi' "$pmos_dir/exynos-modem/APKBUILD" ||
	fail 'exynos-modem recipe must test the pinned samsung-sit-ffi workspace package'
for label in efs persist modem_userdata; do
	grep -q "$label" "$package_dir/s5300-userspace-sources.toml" || fail "S5300 source manifest is missing partition label: $label"
done
grep -q 'modem_image_label = "modem_{androidboot.slot_suffix}"' \
	"$package_dir/s5300-userspace-sources.toml" ||
	fail 'S5300 source manifest must resolve the modem label from the current boot slot'
grep -q 'modem_image_label_policy = "exact-current-slot-only"' \
	"$package_dir/s5300-userspace-sources.toml" ||
	fail 'S5300 source manifest must fail closed on an ambiguous modem slot'
grep -q 'androidboot.slot_suffix' "$package_dir/ferrix-s5300-mounts.initd" ||
	fail 'S5300 mounts must select the modem partition from the bootloader slot'
grep -q 'exactly one androidboot.slot_suffix' "$package_dir/ferrix-s5300-mounts.initd" ||
	fail 'S5300 mounts must reject missing or duplicate slot properties'
grep -q 'modem%s' "$package_dir/ferrix-s5300-mounts.initd" ||
	fail 'S5300 mounts must construct the exact slot-specific modem label'
grep -q 'mounts -> exynos-modem -> sit-proxy -> modemmanager' "$package_dir/s5300-userspace-sources.toml" ||
	fail 'S5300 service order is not recorded'
grep -q 'need ferrix-s5300-mounts' "$package_dir/exynos-modem.initd" ||
	fail 'exynos-modem service does not require read-only source mounts'
grep -q 'after udev exynos-modem' "$package_dir/sit-proxy.initd" ||
	fail 'sit-proxy service does not follow exynos-modem'
grep -q 'need ferrix-s5300-mounts exynos-modem sit-proxy' "$pmos_dir/modemmanager-samsung-sit/modemmanager.initd" ||
	fail 'ModemManager service does not require the S5300 stack'
grep -q 'after udev networkmanager ferrix-s5300-mounts exynos-modem sit-proxy' "$package_dir/ferrix-telephonyd.initd" ||
	fail 'Ferrix telephony adapter ordering is incomplete'
grep -q '^arch="aarch64"$' "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/modemmanager-samsung-sit/APKBUILD" ||
	fail 'SIT ModemManager package must be target-architecture specific'
grep -Eq '^[[:space:]]*need ferrix-s5300-mounts exynos-modem$' \
	"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/sit-proxy.initd" ||
	fail 'SIT proxy must require modem mounts and loader'
grep -q 'FERRIX_S5300_KERNEL_BACKEND=linux-google-tegu-wwan' \
	"$package_dir/ferrix-backends.env" ||
	fail 'target backend env does not select the reviewed WWAN/SIT kernel contract'
grep -q '^FERRIX_TELEPHONY_APN=' "$package_dir/ferrix-backends.env" ||
	fail 'target backend env does not expose the explicit carrier APN setting'
grep -q 'FERRIX_TELEPHONY_APN' "$package_dir/ferrix-product.initd" ||
	fail 'product service does not pass the carrier APN setting to Settings'
if grep -Fq 'export FERRIX_TELEPHONY_APN' "$package_dir/ferrix-product.initd"; then
	fail 'product service exports the carrier APN to the shell environment'
fi
grep -Fq -- '--env FERRIX_TELEPHONY_APN="${FERRIX_TELEPHONY_APN:-}"' \
	"$package_dir/ferrix-product.initd" ||
	fail 'product service does not pass the carrier APN explicitly to appd'
grep -Fq 'export FERRIX_TARGET FERRIX_EVIDENCE FERRIX_NETWORK_BACKEND FERRIX_TELEPHONY_BACKEND FERRIX_TELEPHONY_APN' \
	"$package_dir/ferrix-telephonyd.initd" ||
	fail 'telephonyd service does not receive the target carrier APN fallback'
grep -q '/dev/umts_boot0' "$package_dir/exynos-modem.initd" ||
	fail 'exynos-modem must gate on its pre-ONLINE boot channel'
grep -q 'wait_for_boot_channel' "$package_dir/exynos-modem.initd" ||
	fail 'exynos-modem must tolerate the bounded udev boot-channel race'
grep -q '\[ -c /dev/umts_boot0 \]' "$package_dir/exynos-modem.initd" ||
	fail 'exynos-modem must require a character boot channel, not a regular file'
if grep -q '/sys/class/wwan/wwan0' "$package_dir/exynos-modem.initd"; then
	fail 'exynos-modem must not require post-ONLINE WWAN ports before boot'
fi
grep -q '/etc/ferrix/backends.env' "$package_dir/sit-proxy.initd" ||
	fail 'SIT proxy must load the target backend environment'
grep -Eq '^[[:space:]]*need ferrix-s5300-mounts exynos-modem sit-proxy$' \
	"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/modemmanager-samsung-sit/modemmanager.initd" ||
	fail 'SIT ModemManager must require the complete modem service chain'
if grep -Eq 'for service .*upower|need .*upower|after .*upower' "$package_dir"/*.initd "$package_dir/APKBUILD"; then
	fail 'device package treats D-Bus-activated UPower as an OpenRC service'
fi

out="$work/out"
FERRIX_OUT_DIR="$out" "$script" preflight >/dev/null
manifest="$out/pixel-9a/rootfs/tegu/manifest.tsv"
[ -f "$manifest" ]
awk -F "$tab" '$1 == "status" && $2 == "not-built" { found=1 } END { exit(found ? 0 : 1) }' "$manifest"
awk -F "$tab" '$1 == "split_artifact_contract" && $2 == "radio-split-artifact-contract.toml" && $3 ~ /^[0-9a-f]{64}$/ { found=1 } END { exit(found ? 0 : 1) }' "$manifest"
awk -F "$tab" '$1 == "artifact" && $2 == "deviceinfo" && $3 ~ /^[0-9]+$/ && $4 ~ /^[0-9a-f]{64}$/ { found=1 } END { exit(found ? 0 : 1) }' "$manifest"

# The physical build contract is native Linux x86_64. A Linux AArch64 host
# must be refused before the explicit build token or any host tool is used;
# this prevents an AArch64 UTM/container substitute from entering pmbootstrap.
arch_bin="$work/arch-bin"
mkdir -p "$arch_bin"
{
	printf '%s\n' '#!/bin/sh'
	printf '%s\n' 'case "$1" in'
	printf '%s\n' '  -s) printf "%s\\n" Linux ;;'
	printf '%s\n' '  -m) printf "%s\\n" aarch64 ;;'
	printf '%s\n' '  *) exit 2 ;;'
	printf '%s\n' 'esac'
} >"$arch_bin/uname"
chmod 0755 "$arch_bin/uname"
if PATH="$arch_bin:$PATH" FERRIX_OUT_DIR="$work/aarch64-out" \
	FERRIX_PIXEL9A_ROOTFS_BUILD=I-understand-this-runs-pmbootstrap "$script" build \
	>"$work/aarch64.out" 2>"$work/aarch64.err"; then
	fail 'Linux AArch64 host was accepted for the rootfs build'
fi
grep -q 'rootfs build requires native Linux x86_64; detected Linux/aarch64' "$work/aarch64.err" ||
	fail 'AArch64 refusal reason'
override="$work/custom-manifest.tsv"
FERRIX_OUT_DIR="$out" FERRIX_PIXEL9A_ROOTFS_MANIFEST="$override" "$script" preflight >/dev/null
[ -f "$override" ]

if FERRIX_OUT_DIR="$out" "$script" build >/dev/null 2>&1; then
	echo "FAIL: build accepted missing explicit confirmation and inputs" >&2
	exit 1
fi

kernel="$work/kernel"
mkdir -p "$kernel"
for name in dtbo.img vendor_kernel_boot.img vendor_dlkm.img system_dlkm.img; do
	printf '%s\n' "$name" >"$kernel/$name"
done
while IFS= read -r module; do
	case "$module" in ''|'#'*) continue ;; esac
	printf '%s\n' "$module" >"$kernel/$module.ko"
done <"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/modules-initfs"
{
	printf 'schema_version\t1\n'
	for name in dtbo.img vendor_kernel_boot.img vendor_dlkm.img system_dlkm.img; do
		if command -v sha256sum >/dev/null 2>&1; then
			hash=$(sha256sum "$kernel/$name" | awk '{print $1}')
		else
			hash=$(shasum -a 256 "$kernel/$name" | awk '{print $1}')
		fi
		printf 'artifact\t%s\t%s\t%s\n' "$name" "$(wc -c <"$kernel/$name" | tr -d '[:space:]')" "$hash"
	done
} >"$kernel/../build-manifest.tsv"

# The target package is paired with the independent radio-first kernel route;
# keep a tiny hash-verified Kbuild handoff fixture for rootfs validation.
radio_kernel="$work/kernel-radio"
mkdir -p "$radio_kernel"
printf image >"$radio_kernel/Image"
printf dtb >"$radio_kernel/dtb.img"
mkdir -p "$radio_kernel/split-inputs"
printf 'schema_version\t1\nsource\ttegu.dtb\t3\tfixture\n' >"$radio_kernel/split-inputs/dtbo-source-set.tsv"
printf 'schema_version\t1\ninstall_root\tsplit-inputs/module-install\n' >"$radio_kernel/split-inputs/module-install.tsv"
{
	printf 'schema_version\t1\n'
	for partition in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
		printf 'partition\t%s\tstatus=not-built\n' "$partition"
	done
} >"$radio_kernel/split-inputs/partition-sizes.tsv"
{
	printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
	printf 'route_id\texperimental-linux-google-tegu-s5300-wwan\n'
	printf 'kernel_backend\tlinux-google-tegu-wwan\n'
	printf 'firmware_mapping\tfirmware-radio-route.tsv\t%s\n' "$(hash_file "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/firmware-radio-route.tsv")"
	printf 'status\tbuilt-not-booted\n'
	printf 'bootable\tfalse\n'
	printf 'split_artifact_contract\tradio-split-artifact-contract.toml\t%s\n' "$(hash_file "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/radio-split-artifact-contract.toml")"
	for input in dtbo-source-set module-install partition-sizes; do
		file="$radio_kernel/split-inputs/$input.tsv"
		printf '%s\tsplit-inputs/%s.tsv\t%s\t%s\n' "$(printf '%s' "$input" | tr '-' '_')" "$input" "$(wc -c <"$file" | tr -d '[:space:]')" "$(hash_file "$file")"
	done
	for name in Image dtb.img; do
		hash=$(hash_file "$radio_kernel/$name")
		printf 'artifact\t%s\t%s\t%s\n' "$name" "$(wc -c <"$radio_kernel/$name" | tr -d '[:space:]')" "$hash"
	done
} >"$radio_kernel/build-manifest.tsv"

product_bins='ferrix-core ferrix-shell ferrix-basic-screen ferrix-appd ferrix-sandbox ferrix-terminal
ferrix-telephonyd ferrix-networkd ferrix-contactsd ferrix-phone ferrix-messages ferrix-contacts ferrix-settings ferrix-files'
product_app_descriptors='grants.json org.ferrix.browser.json org.ferrix.camera.json
org.ferrix.contacts.json org.ferrix.files.json org.ferrix.home.json
org.ferrix.messages.json org.ferrix.phone.json org.ferrix.settings.json
org.ferrix.terminal.json'
product="$work/product"
mkdir -p "$product/apps" "$product/contacts"
for name in $product_bins; do
	printf '#!/bin/sh\n' >"$product/$name"
done
for name in $product_app_descriptors; do
	case "$name" in
		grants.json) printf '[package]\n' >"$product/apps/$name" ;;
		*) printf '{"schema_version":1,"name":"%s"}\n' "$name" >"$product/apps/$name" ;;
	esac
done
printf 'schema_version = 1\nname = "pixel-9a-native-linux"\n' >"$product/profile.toml"
printf 'colors = true\n' >"$product/colors.toml"
product_artifact() {
	relative=$1
	file="$product/$relative"
	printf 'artifact\t%s\t%s\t%s\n' "$relative" "$(wc -c <"$file" | tr -d '[:space:]')" "$(hash_file "$file")"
}
{
	printf 'schema_version\t1\n'
	printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
	printf 'evidence\tphysical:google-pixel-9a-tegu-gxq96-mp1-0\n'
	printf 'backend_mode\tnative-linux\n'
	printf 'rust_target\taarch64-unknown-linux-musl\n'
	printf 'rust_linker\taarch64-linux-musl-gcc\n'
	printf 'status\tbuilt\n'
	printf 'bootable\tfalse\n'
	for name in $product_bins; do product_artifact "$name"; done
	product_artifact profile.toml
	product_artifact colors.toml
	for name in $product_app_descriptors; do product_artifact "apps/$name"; done
} >"$product/manifest.tsv"
printf '{"contacts":[]}\n' >"$product/contacts/mock-contacts.json"

firmware="$work/firmware"
mkdir -p "$firmware"
allowlist="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/firmware-allowlist.tsv"
stage_allowlist="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/firmware-stage-allowlist.tsv"
factory_archive="$work/tegu-synthetic-factory.zip"
printf 'synthetic-factory-archive\n' >"$factory_archive"
factory_hash=$(hash_file "$factory_archive")
factory_bytes=$(wc -c <"$factory_archive" | tr -d '[:space:]')
pins_fixture="$work/recovery.pins"
{
	printf '# kind\tbuild\tversion\tfilename\turl\tsha256\tbytes\tsource-page\n'
	printf 'factory\tSYN\t0\ttegu-synthetic-factory.zip\thttps://example.invalid/factory\t%s\t%s\thttps://example.invalid\n' "$factory_hash" "$factory_bytes"
} >"$pins_fixture"
stage_file_paths='lib/firmware/fw_bcmdhd.bin
lib/firmware/bcmdhd.cal
lib/firmware/bcmdhd_clm.blob
lib/firmware/fw_bcmdhd.map
lib/firmware/bcmdhd_txcap.blob
lib/firmware/fw_bcmdhd.bin_4383_a3
lib/firmware/bcmdhd.cal_4383_a3
lib/firmware/bcmdhd_clm.blob_4383_a3
lib/firmware/fw_bcmdhd.map_4383_a3
lib/firmware/bcmdhd_txcap.blob_4383_a3
lib/firmware/brcm/BCM.hcd
lib/firmware/mali_csffw-r52p0.bin
lib/firmware/mali_csffw.bin
lib/firmware/mfc_fw.bin
lib/firmware/aoc.bin
lib/firmware/tismartpa_driver_tuning.bin
lib/firmware/cs40l26.wmfw
lib/firmware/cs40l26.bin
lib/firmware/cs40l26-svc.bin
lib/firmware/cs40l26-dvl.bin
lib/firmware/cs40l26-dbc.bin'
firmware_paths="$stage_file_paths
lib/firmware/carrierconfig/carrier.conf
lib/firmware/carrierconfig/carrier-extra.conf
lib/firmware/uecapconfig/capability.conf"
printf '%s\n' "$stage_file_paths" >"$work/expected-stage-file-paths"
printf '%s\n' "$firmware_paths" >"$work/expected-firmware-paths"
awk -F "$tab" '$0 !~ /^[[:space:]]*#/ && NF == 3 && $1 != "modem-s5300" { print $2 }' "$allowlist" >"$work/allowlist-firmware-paths"
cmp -s "$work/expected-stage-file-paths" "$work/allowlist-firmware-paths" ||
	fail 'checked-in firmware allowlist paths changed without updating this workflow test'
awk -F "$tab" '$0 !~ /^[[:space:]]*#/ && NF == 5 && $1 == "modem-s5300" && $5 == "bundle" { print $3 "\t" $4 }' \
	"$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/firmware-stage-allowlist.tsv" >"$work/allowlist-bundles"
printf '%s\n' \
	"/firmware/carrierconfig${tab}lib/firmware/carrierconfig" \
	"/firmware/uecapconfig${tab}lib/firmware/uecapconfig" >"$work/expected-bundles"
cmp -s "$work/expected-bundles" "$work/allowlist-bundles" ||
	fail 'checked-in firmware bundle allowlist changed without updating this workflow test'
: >"$firmware/firmware-manifest.tsv"
while IFS="$tab" read -r component path required; do
	case "$component" in ''|'#'*) continue ;; esac
	case "$component" in
		bcm4383|bluetooth-broadcom|gpu-mali|video-mfc|audio-aoc|audio-tas25xx|haptics-cs40l26) ;;
		modem-s5300) continue ;;
		*) fail "unexpected firmware component: $component" ;;
	esac
	# The Mali CSF, MFC, AoC audio, and haptics firmware are listed for the manifest
	# but are not radio gate inputs, so they are the optional rows.
	case "$required:$component" in
		1:*|0:gpu-mali|0:video-mfc|0:audio-aoc|0:audio-tas25xx|0:haptics-cs40l26) ;;
		*) fail "firmware fixture is not required: $path" ;;
	esac
	mkdir -p "$firmware/$(dirname -- "$path")"
	printf 'fixture-%s\n' "$path" >"$firmware/$path"
	hash=$(hash_file "$firmware/$path")
	printf 'firmware\t%s\t%s\t%s\n' "$path" "$(wc -c <"$firmware/$path" | tr -d '[:space:]')" "$hash" >>"$firmware/firmware-manifest.tsv"
done <"$allowlist"

if FERRIX_OUT_DIR="$out" FERRIX_PIXEL9A_PINS="$pins_fixture" \
FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" \
FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" \
FERRIX_PIXEL9A_PRODUCT_DIR="$product" \
FERRIX_PIXEL9A_FIRMWARE_DIR="$firmware" \
	FERRIX_PIXEL9A_PROFILE="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.toml" \
	"$script" validate >/dev/null 2>&1; then
	fail 'rootfs validator accepted the legacy firmware manifest without provenance rows'
fi

# Keep the legacy fixture above covered, then exercise the canonical rows used by
# the rootfs workflow. The source path is intentionally different from
# the runtime destination so a validator that ignores destination awareness fails.
{
	printf 'schema_version\t1\n'
	printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
	printf 'source\tvendor.img\t1\t%s\n' "$(printf '%064d' 0)"
	printf 'factory_archive\t%s\t%s\n' "$factory_archive" "$factory_hash"
} >"$firmware/firmware-manifest.tsv"
while IFS="$tab" read -r component source_image source_path path entry_type extra; do
	case "$component" in ''|'#'*) continue ;; esac
	case "$component" in modem-s5300) continue ;; esac
	[ "${entry_type:-file}" = file ] || fail "unexpected non-file firmware staging row: $component $source_path"
	hash=$(hash_file "$firmware/$path")
	case "$component" in
		wifi-bcm4383|bcm4383) manifest_controller=wifi-bcm4383 ;;
		bluetooth-broadcom|gpu-mali|video-mfc|audio-aoc|audio-tas25xx|haptics-cs40l26) manifest_controller=$component ;;
		*) fail "unexpected firmware component: $component" ;;
	esac
	printf 'firmware\t%s\t%s\t%s\t%s\t%s\t%s\n' \
		"$manifest_controller" "$source_image" "$source_path" "$path" \
		"$(wc -c <"$firmware/$path" | tr -d '[:space:]')" "$hash" \
		>>"$firmware/firmware-manifest.tsv"
done <"$stage_allowlist"
for path in lib/firmware/carrierconfig/carrier.conf lib/firmware/carrierconfig/carrier-extra.conf lib/firmware/uecapconfig/capability.conf; do
	mkdir -p "$firmware/$(dirname -- "$path")"
	printf 'fixture-%s\n' "$path" >"$firmware/$path"
	source_path=$(printf '%s\n' "$path" | sed 's#^lib/firmware/#/firmware/#')
	hash=$(hash_file "$firmware/$path")
	printf 'firmware\tmodem-s5300\tvendor.img\t%s\t%s\t%s\t%s\n' \
		"$source_path" "$path" "$(wc -c <"$firmware/$path" | tr -d '[:space:]')" "$hash" \
		>>"$firmware/firmware-manifest.tsv"
done
awk -F "$tab" '
	$1 == "firmware" {
			if (NF != 7 || $2 !~ /^(wifi-bcm4383|bluetooth-broadcom|modem-s5300|gpu-mali|video-mfc|audio-aoc|audio-tas25xx|haptics-cs40l26)$/ || $3 != "vendor.img" ||
			index($4, "/firmware/") != 1 || index($5, "lib/firmware/") != 1 ||
			$4 == $5 || $4 ~ /[[:space:]]/ || $5 ~ /[[:space:]]/ ||
			$6 !~ /^[0-9]+$/ || length($7) != 64 || $7 ~ /[^0-9a-f]/) exit 1
		count++
	}
		END { exit(count == 24 ? 0 : 1) }
' "$firmware/firmware-manifest.tsv"
awk -F "$tab" '$1 == "firmware" { print $5 }' "$firmware/firmware-manifest.tsv" >"$work/manifest-firmware-paths"
cmp -s "$work/expected-firmware-paths" "$work/manifest-firmware-paths" ||
	fail 'destination-aware firmware manifest does not cover the current nested paths'

FERRIX_OUT_DIR="$out" FERRIX_PIXEL9A_PINS="$pins_fixture" \
FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" \
FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" \
FERRIX_PIXEL9A_PRODUCT_DIR="$product" \
FERRIX_PIXEL9A_FIRMWARE_DIR="$firmware" \
FERRIX_PIXEL9A_PROFILE="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.toml" \
		"$script" validate >/dev/null
awk -F "$tab" '$1 == "status" && $2 == "inputs-verified" { found=1 } END { exit(found ? 0 : 1) }' "$manifest"
awk -F "$tab" '$1 == "artifact" && $2 == "firmware-manifest.tsv" && $3 ~ /^[0-9]+$/ && $4 ~ /^[0-9a-f]{64}$/ { found=1 } END { exit(found ? 0 : 1) }' "$manifest"

# A claimed pinned archive hash is not enough: the rootfs handoff must
# re-hash the external archive before accepting the staged firmware. Keep the
# basename pinned while changing the path so this exercises the byte/hash gate
# rather than the filename gate.
wrong_factory_dir="$work/wrong-factory"
mkdir -p "$wrong_factory_dir"
printf '%s\n' 'not-the-pinned-factory-bytes' >"$wrong_factory_dir/tegu-synthetic-factory.zip"
wrong_factory_manifest="$work/wrong-factory-manifest.tsv"
sed "s#^factory_archive${tab}[^${tab}]*${tab}#factory_archive${tab}$wrong_factory_dir/tegu-synthetic-factory.zip${tab}#" \
	"$firmware/firmware-manifest.tsv" >"$wrong_factory_manifest"
wrong_factory="$work/wrong-firmware-factory"
cp -R "$firmware" "$wrong_factory"
cp "$wrong_factory_manifest" "$wrong_factory/firmware-manifest.tsv"
if FERRIX_OUT_DIR="$out" FERRIX_PIXEL9A_PINS="$pins_fixture" FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" FERRIX_PIXEL9A_PRODUCT_DIR="$product" FERRIX_PIXEL9A_FIRMWARE_DIR="$wrong_factory" \
	FERRIX_PIXEL9A_PROFILE="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.toml" "$script" validate >/dev/null 2>&1; then
	echo "FAIL: accepted a factory archive whose bytes do not match the pinned hash" >&2
	exit 1
fi

product_backup="$work/product-ferrix-core"
cp "$product/ferrix-core" "$product_backup"
printf '%s\n' '# tampered after product manifest generation' >>"$product/ferrix-core"
if FERRIX_OUT_DIR="$out" FERRIX_PIXEL9A_PINS="$pins_fixture" \
FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" FERRIX_PIXEL9A_PRODUCT_DIR="$product" \
FERRIX_PIXEL9A_FIRMWARE_DIR="$firmware" \
	FERRIX_PIXEL9A_PROFILE="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.toml" \
	"$script" validate >/dev/null 2>&1; then
	fail 'rootfs validator accepted a product binary hash mismatch'
fi
mv "$product_backup" "$product/ferrix-core"

wrong="$work/wrong-firmware"
mkdir -p "$wrong"
cp -R "$firmware"/. "$wrong"/
awk -F "$tab" -v OFS="$tab" \
	'$1 == "firmware" && $5 == "lib/firmware/fw_bcmdhd.bin" { $7 = "0000000000000000000000000000000000000000000000000000000000000000" } { print }' \
	"$firmware/firmware-manifest.tsv" >"$wrong/firmware-manifest.tsv"
if FERRIX_OUT_DIR="$out" FERRIX_PIXEL9A_PINS="$pins_fixture" FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" FERRIX_PIXEL9A_PRODUCT_DIR="$product" FERRIX_PIXEL9A_FIRMWARE_DIR="$wrong" \
FERRIX_PIXEL9A_PROFILE="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.toml" "$script" validate >/dev/null 2>&1; then
	echo "FAIL: accepted a firmware hash mismatch" >&2
	exit 1
fi

# A same-hash symlink must not turn the external firmware handoff into an
# arbitrary path read. The manifest still names the reviewed bytes, but the
# staged input itself must be a regular file beneath the firmware directory.
symlink_firmware="$work/symlink-firmware"
cp -R "$firmware"/. "$symlink_firmware"/
rm -f "$symlink_firmware/lib/firmware/fw_bcmdhd.bin"
ln -s "$firmware/lib/firmware/fw_bcmdhd.bin" "$symlink_firmware/lib/firmware/fw_bcmdhd.bin"
if FERRIX_OUT_DIR="$out" FERRIX_PIXEL9A_PINS="$pins_fixture" FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" FERRIX_PIXEL9A_PRODUCT_DIR="$product" FERRIX_PIXEL9A_FIRMWARE_DIR="$symlink_firmware" \
FERRIX_PIXEL9A_PROFILE="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.toml" "$script" validate >/dev/null 2>&1; then
	echo "FAIL: accepted a symlinked firmware input" >&2
	exit 1
fi

# Bundle rows are required to contribute at least one regular file, and their
# manifest paths must remain a regular-file handoff rather than symlinks.
empty_bundle="$work/empty-bundle"
cp -R "$firmware"/. "$empty_bundle"/
awk -F "$tab" -v OFS="$tab" \
	'!($1 == "firmware" && $2 == "modem-s5300" && $4 ~ "^/firmware/carrierconfig/") { print }' \
	"$firmware/firmware-manifest.tsv" >"$empty_bundle/firmware-manifest.tsv"
if FERRIX_OUT_DIR="$out" FERRIX_PIXEL9A_PINS="$pins_fixture" FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" FERRIX_PIXEL9A_PRODUCT_DIR="$product" FERRIX_PIXEL9A_FIRMWARE_DIR="$empty_bundle" \
FERRIX_PIXEL9A_PROFILE="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.toml" "$script" validate >/dev/null 2>&1; then
	echo "FAIL: accepted an empty required carrierconfig bundle" >&2
	exit 1
fi
symlink_bundle="$work/symlink-bundle"
cp -R "$firmware"/. "$symlink_bundle"/
rm -f "$symlink_bundle/lib/firmware/carrierconfig/carrier.conf"
ln -s "$firmware/lib/firmware/carrierconfig/carrier.conf" \
	"$symlink_bundle/lib/firmware/carrierconfig/carrier.conf"
if FERRIX_OUT_DIR="$out" FERRIX_PIXEL9A_PINS="$pins_fixture" FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" FERRIX_PIXEL9A_PRODUCT_DIR="$product" FERRIX_PIXEL9A_FIRMWARE_DIR="$symlink_bundle" \
FERRIX_PIXEL9A_PROFILE="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.toml" "$script" validate >/dev/null 2>&1; then
	echo "FAIL: accepted a symlinked carrierconfig bundle file" >&2
	exit 1
fi

# Exercise the staging half with a local pmbootstrap stub. This keeps the test
# device-free while making the rootfs tar, firmware destinations, symlinks, and
# complete product artifact set observable on every host.
fake_bin="$work/fake-bin"
mkdir -p "$fake_bin"
real_tar=$(command -v tar)
radio_initfs_sha512=$(sha512_file "$package_dir/modules-initfs-radio")
{
	printf '%s\n' '#!/bin/sh'
	printf '%s\n' 'case "$1" in'
	printf '%s\n' '  -s) printf "%s\\n" Linux ;;'
	printf '%s\n' '  -m) printf "%s\\n" x86_64 ;;'
	printf '%s\n' '  *) exit 2 ;;'
	printf '%s\n' 'esac'
} >"$fake_bin/uname"
{
	printf '%s\n' '#!/bin/sh' 'set -eu'
	printf '%s\n' '[ "$1" = -c ] && { printf "%s\\n" 8; exit 0; }'
	printf '%s\n' 'case "$1" in */pmbootstrap-rootfs.py) shift; exec "$(dirname "$0")/pmbootstrap" "$@" ;; esac'
	printf '%s\n' 'exit 2'
} >"$fake_bin/python3"
{
	printf '%s\n' '#!/bin/sh' 'set -eu' 'config=' 'action=' 'while [ "$#" -gt 0 ]; do' \
		'  case "$1" in' '    --config) config=$2; shift 2 ;;' '    init|install|shutdown) action=$1; shift ;;' '    *) shift ;;' '  esac' \
		'done' '[ -n "$config" ]' 'build_work=$(sed -n "s/^work = //p" "$config")' \
		'aports=$(sed -n "s/^aports = //p" "$config")' \
		'[ "$action" = shutdown ] && exit 0' \
		'git -C "$aports" rev-parse HEAD >/dev/null' \
		'[ "$(git -C "$aports" remote get-url origin)" = https://gitlab.postmarketos.org/postmarketOS/pmaports.git ]' \
		'[ "${PMB_CHANNELS_CFG##*/}" = channels.cfg ]' \
		'[ -f "$PMB_CHANNELS_CFG" ]' \
		'[ "$action" = init ] || [ "$action" = install ]' \
		'if [ "$action" = shutdown ]; then exit 0; fi' \
		'if [ "$action" = init ]; then mkdir -p "$build_work"; printf "%s\\n" 6 >"$build_work/version"; exit 0; fi' \
		'[ -f "$aports/device/testing/exynos-modem/APKBUILD" ]' \
		'[ -f "$aports/device/testing/modemmanager-samsung-sit/APKBUILD" ]' \
		'[ -f "$aports/device/testing/waydroid/APKBUILD" ]' \
		'grep -q "^pkgver=1.6.3$" "$aports/device/testing/waydroid/APKBUILD"' \
		'[ -f "$aports/device/testing/device-google-tegu/s5300-userspace-sources.toml" ]' \
		'[ -f "$aports/device/testing/device-google-tegu/modules-initfs-radio" ]' \
		'awk '\''! /^[[:space:]]*#/ && NF { bad=1 } END { exit bad }'\'' "$aports/device/testing/device-google-tegu/modules-initfs"' \
		"grep -q \"^$radio_initfs_sha512  modules-initfs\\$\" \"\$aports/device/testing/device-google-tegu/APKBUILD\"" \
		'grep -q "634e8eb404c3fcf959dce6a0b0d97b33ba0fc346" "$aports/device/testing/exynos-modem/APKBUILD"' \
		'grep -q "14a0013a28e5d5eec513c65e75d7549678ecc61e" "$aports/device/testing/modemmanager-samsung-sit/APKBUILD"' \
		'grep -q "^build_pkgs_on_install = True$" "$config"' \
		'mkdir -p "$build_work/chroot_rootfs_test/lib/apk/db"' \
		'extra=$(sed -n "s/^extra_packages = //p" "$config")${FAKE_EXTRA_INSTALLED:+,$FAKE_EXTRA_INSTALLED}' \
		'for package in $(printf "%s" "$extra" | tr , " "); do printf "P:%s\\nV:1-r0\\n\\n" "$package"; done >"$build_work/chroot_rootfs_test/lib/apk/db/installed"'
} >"$fake_bin/pmbootstrap"
{
	printf '%s\n' '#!/bin/sh' 'set -eu' 'while [ "$#" -gt 0 ]; do' \
		'  case "$1" in' \
		'    --sort=name|--mtime=*|--owner=*|--group=*|--numeric-owner) shift ;;' \
		'    *) break ;;' \
		'  esac' 'done'
	printf 'exec "%s" "$@"\n' "$real_tar"
} >"$fake_bin/tar"
{
	printf '%s\n' '#!/bin/sh' 'set -eu' 'output=' \
		'for argument in "$@"; do' \
		'  case "$argument" in' \
		'    *.img.tmp.*) output=$argument ;;' \
		'  esac' \
		'done' '[ -n "$output" ]' 'printf "%s\\n" synthetic-ext4-rootfs >"$output"'
} >"$fake_bin/mke2fs"
chmod 0755 "$fake_bin/uname" "$fake_bin/python3" "$fake_bin/pmbootstrap" "$fake_bin/tar" "$fake_bin/mke2fs"

pmaports="$work/pmaports"
mkdir -p "$pmaports"
	git -C "$pmaports" init -q
	git -C "$pmaports" config user.email test@example.invalid
	git -C "$pmaports" config user.name ferrix-rootfs-test
	git -C "$pmaports" commit --allow-empty -qm fixture
	git -C "$pmaports" remote add origin https://gitlab.postmarketos.org/postmarketOS/pmaports.git
	pmaports_commit=$(git -C "$pmaports" rev-parse HEAD)

# A fresh native-Linux checkout has no ignored cache root yet. The build must
# create it before mktemp, then fail at the actionable pinned-pmaports gate;
# otherwise the first failure is an opaque "No such file or directory" from
# mktemp and the documented handoff cannot be reproduced from a clean host.
rm -rf "$work/cache"
if PATH="$fake_bin:$PATH" \
FERRIX_OUT_DIR="$out" \
FERRIX_PIXEL9A_PINS="$pins_fixture" \
FERRIX_CACHE_DIR="$work/cache" \
FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" \
FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" \
FERRIX_PIXEL9A_PRODUCT_DIR="$product" \
FERRIX_PIXEL9A_FIRMWARE_DIR="$firmware" \
FERRIX_PIXEL9A_PROFILE="$product/profile.toml" \
FERRIX_PIXEL9A_ROOTFS_BUILD=I-understand-this-runs-pmbootstrap \
FERRIX_PIXEL9A_ROOTFS_PASSWORD=test-password \
FERRIX_PIXEL9A_PMAPORTS_DIR="$work/missing-pmaports" \
FERRIX_PIXEL9A_PMAPORTS_COMMIT="$pmaports_commit" \
FERRIX_PIXEL9A_ROOTFS_IMAGE_SIZE_MIB=256 \
	"$script" build >"$work/fresh-cache.out" 2>"$work/fresh-cache.err"; then
	fail 'rootfs build accepted a missing pmaports checkout'
fi
grep -q 'pmaports path is not a Git checkout' "$work/fresh-cache.err" ||
	fail 'fresh-cache build did not reach the actionable pmaports refusal'
[ -d "$work/cache" ] || fail 'rootfs build did not create its cache parent before mktemp'
mkdir -p "$work/cache"

# A clean tracked diff must not allow untracked pmaports inputs into the
# archived source tree. The validator should stop before pmbootstrap runs.
printf '%s\n' 'untracked source input' >"$pmaports/UNTRACKED-FIXTURE"
if PATH="$fake_bin:$PATH" \
FERRIX_OUT_DIR="$out" \
FERRIX_PIXEL9A_PINS="$pins_fixture" \
FERRIX_CACHE_DIR="$work/cache" \
FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" \
FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" \
FERRIX_PIXEL9A_PRODUCT_DIR="$product" \
FERRIX_PIXEL9A_FIRMWARE_DIR="$firmware" \
FERRIX_PIXEL9A_PROFILE="$product/profile.toml" \
FERRIX_PIXEL9A_ROOTFS_BUILD=I-understand-this-runs-pmbootstrap \
FERRIX_PIXEL9A_ROOTFS_PASSWORD=test-password \
FERRIX_PIXEL9A_PMAPORTS_DIR="$pmaports" \
FERRIX_PIXEL9A_PMAPORTS_COMMIT="$pmaports_commit" \
FERRIX_PIXEL9A_ROOTFS_IMAGE_SIZE_MIB=256 \
	"$script" build >"$work/untracked.out" 2>"$work/untracked.err"; then
	fail 'rootfs build accepted an untracked pmaports input'
fi
grep -q 'pmaports checkout has untracked changes' "$work/untracked.err" ||
	fail 'untracked pmaports refusal reason'
rm -f "$pmaports/UNTRACKED-FIXTURE"

PATH="$fake_bin:$PATH" \
FERRIX_OUT_DIR="$out" \
FERRIX_PIXEL9A_PINS="$pins_fixture" \
FERRIX_CACHE_DIR="$work/cache" \
FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" \
FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" \
FERRIX_PIXEL9A_PRODUCT_DIR="$product" \
FERRIX_PIXEL9A_FIRMWARE_DIR="$firmware" \
FERRIX_PIXEL9A_PROFILE="$product/profile.toml" \
FERRIX_PIXEL9A_ROOTFS_BUILD=I-understand-this-runs-pmbootstrap \
FERRIX_PIXEL9A_ROOTFS_PASSWORD=test-password \
FERRIX_PIXEL9A_PMAPORTS_DIR="$pmaports" \
FERRIX_PIXEL9A_PMAPORTS_COMMIT="$pmaports_commit" \
FERRIX_PIXEL9A_ROOTFS_IMAGE_SIZE_MIB=256 \
	"$script" build >/dev/null

rootfs_artifact="$out/pixel-9a/rootfs/tegu/rootfs.tar"
rootfs_image="$out/pixel-9a/rootfs/tegu/rootfs.img"
built_firmware_manifest="$out/pixel-9a/rootfs/tegu/firmware-manifest.tsv"
[ -f "$rootfs_artifact" ] || fail 'rootfs build did not produce rootfs.tar'
[ -f "$rootfs_image" ] || fail 'rootfs build did not produce rootfs.img'
[ -f "$built_firmware_manifest" ] || fail 'rootfs build did not copy firmware-manifest.tsv'
cmp -s "$firmware/firmware-manifest.tsv" "$built_firmware_manifest" ||
	fail 'rootfs build changed the destination-aware firmware manifest'
awk -F "$tab" '$1 == "status" && $2 == "built-substrate-only" { found=1 } END { exit(found ? 0 : 1) }' "$manifest"
awk -F "$tab" -v expected="$pmaports_commit" '$1 == "pmaports_commit" && $2 == expected { found=1 } END { exit(found ? 0 : 1) }' "$manifest" ||
	fail 'rootfs manifest does not record the pinned pmaports revision'
product_manifest_hash=$(hash_file "$product/manifest.tsv")
product_manifest_bytes=$(wc -c <"$product/manifest.tsv" | tr -d '[:space:]')
awk -F "$tab" -v bytes="$product_manifest_bytes" -v hash="$product_manifest_hash" \
	'$1 == "input" && $2 == "product_manifest" && $3 == bytes && $4 == hash { found=1 } END { exit(found ? 0 : 1) }' \
	"$manifest" || fail 'rootfs manifest does not hash the product manifest handoff'
awk -F "$tab" '$1 == "split_artifact_contract" && $2 == "radio-split-artifact-contract.toml" && $3 ~ /^[0-9a-f]{64}$/ { found=1 } END { exit(found ? 0 : 1) }' "$manifest"
awk -F "$tab" '$1 == "artifact" && $2 == "rootfs.tar" && $3 ~ /^[0-9]+$/ && $4 ~ /^[0-9a-f]{64}$/ { found=1 } END { exit(found ? 0 : 1) }' "$manifest"
awk -F "$tab" '$1 == "artifact" && $2 == "rootfs.img" && $3 ~ /^[0-9]+$/ && $4 ~ /^[0-9a-f]{64}$/ { found=1 } END { exit(found ? 0 : 1) }' "$manifest"
awk -F "$tab" '$1 == "android_runtime" && $2 == "none" { found=1 } END { exit(found ? 0 : 1) }' "$manifest" ||
	fail 'default rootfs manifest does not record android_runtime none'

# Opt-in Android package set (owner request 2026-10-09): appended to the
# staged pmbootstrap config, verified in the built apk database, recorded in
# the manifest; waydroid-nftrules in the result is refused (the GKI lacks
# nftables; postmarketos-base's own nftables package is tolerated).
android_build() {
	PATH="$fake_bin:$PATH" FERRIX_OUT_DIR="$1" FERRIX_PIXEL9A_PINS="$pins_fixture" \
	FERRIX_CACHE_DIR="$work/cache" FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" \
	FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" \
	FERRIX_PIXEL9A_PRODUCT_DIR="$product" FERRIX_PIXEL9A_FIRMWARE_DIR="$firmware" \
	FERRIX_PIXEL9A_PROFILE="$product/profile.toml" \
	FERRIX_PIXEL9A_ROOTFS_BUILD=I-understand-this-runs-pmbootstrap \
	FERRIX_PIXEL9A_ROOTFS_PASSWORD=test-password \
	FERRIX_PIXEL9A_PMAPORTS_DIR="$pmaports" FERRIX_PIXEL9A_PMAPORTS_COMMIT="$pmaports_commit" \
	FERRIX_PIXEL9A_ROOTFS_IMAGE_SIZE_MIB=256 FERRIX_PIXEL9A_ANDROID=1 \
	FERRIX_PIXEL9A_ROOTFS_KERNEL_ROUTE="${ROUTE:-radio-first}" FERRIX_PIXEL9A_KERNEL_DIST="$kernel" "$script" build
}
# The development flash route composes with the Kleaf GKI, selected explicitly.
# GKI builds dm_mod in; modules.builtin (kbuild '-' names) satisfies the list.
rm -f "$kernel/dm_mod.ko"
printf '%s\n' kernel/drivers/md/dm-mod.ko >"$kernel/modules.builtin"
ROUTE=kleaf-dev android_build "$work/android-out" >"$work/android.out" 2>"$work/android.err" ||
	{ cat "$work/android.err" >&2; fail 'opt-in Android rootfs build failed'; }
android_manifest="$work/android-out/pixel-9a/rootfs/tegu/manifest.tsv"
awk -F "$tab" '$1 == "android_runtime" && $2 == "waydroid-packages" && $3 ~ /(^|,)waydroid(,|$)/ { found=1 } END { exit(found ? 0 : 1) }' \
	"$android_manifest" || fail 'Android rootfs manifest does not record the package set'
awk -F "$tab" '$1 == "artifact" && $2 == "android/packages" && $4 ~ /^[0-9a-f]{64}$/ { found=1 } END { exit(found ? 0 : 1) }' \
	"$android_manifest" || fail 'Android rootfs manifest does not hash the package set'
grep -q 'opt-in Android runtime packages: waydroid,' "$work/android.out" || fail 'Android packages were not appended'
grep -q '(audio backend: pipewire)' "$work/android.out" || fail 'Android build did not select the PipeWire audio backend'
awk -F "$tab" '$1 == "route_id" && $2 == "development-google-kleaf-gki" { found=1 } END { exit(found ? 0 : 1) }' \
	"$android_manifest" || fail 'kleaf-dev rootfs manifest does not record the development route'
awk -F "$tab" '$1 == "input" && $2 == "kleaf_kernel_manifest" && $4 ~ /^[0-9a-f]{64}$/ { found=1 } END { exit(found ? 0 : 1) }' \
	"$android_manifest" || fail 'kleaf-dev rootfs manifest does not hash the Kleaf kernel manifest'
if ROUTE=guess android_build "$work/android-route-out" >/dev/null 2>"$work/android-route.err"; then
	fail 'unknown rootfs kernel route was accepted'
fi
grep -q 'FERRIX_PIXEL9A_ROOTFS_KERNEL_ROUTE must be radio-first or kleaf-dev' "$work/android-route.err" ||
	fail 'unknown kernel route reason'
if FAKE_EXTRA_INSTALLED=waydroid-nftrules ROUTE=kleaf-dev android_build "$work/android-nft-out" >/dev/null 2>"$work/android-nft.err"; then
	fail 'Android rootfs with waydroid-nftrules installed was accepted'
fi
grep -q 'contains waydroid-nftrules, which the GKI cannot use' "$work/android-nft.err" || { cat "$work/android-nft.err" >&2; fail 'nftables refusal reason'; }
if grep -Eqx '(nftables|waydroid-nftrules)' "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/android/packages"; then
	fail 'Android package set lists an nftables package'
fi
if FERRIX_OUT_DIR="$work/android-bad" FERRIX_PIXEL9A_ANDROID=yes "$script" preflight >/dev/null 2>"$work/android-bad.err"; then
	fail 'invalid FERRIX_PIXEL9A_ANDROID was accepted'
fi
grep -q 'FERRIX_PIXEL9A_ANDROID must be 0 or 1' "$work/android-bad.err" || fail 'invalid Android flag reason'

# Read-only status checks must not clobber a completed built handoff. They
# write status-named diagnostics beside the canonical manifest instead.
built_manifest_hash=$(hash_file "$manifest")
PATH="$fake_bin:$PATH" FERRIX_OUT_DIR="$out" FERRIX_PIXEL9A_PINS="$pins_fixture" \
FERRIX_CACHE_DIR="$work/cache" FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" \
FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" \
FERRIX_PIXEL9A_PRODUCT_DIR="$product" FERRIX_PIXEL9A_FIRMWARE_DIR="$firmware" \
FERRIX_PIXEL9A_PROFILE="$product/profile.toml" "$script" preflight >/dev/null
[ "$(hash_file "$manifest")" = "$built_manifest_hash" ] ||
	fail 'rootfs preflight clobbered the built manifest'
awk -F "$tab" '$1 == "status" && $2 == "built-substrate-only" { found=1 } END { exit(found ? 0 : 1) }' "$manifest"
awk -F "$tab" '$1 == "status" && $2 == "not-built" { found=1 } END { exit(found ? 0 : 1) }' "$manifest.not-built.tsv"

PATH="$fake_bin:$PATH" FERRIX_OUT_DIR="$out" FERRIX_PIXEL9A_PINS="$pins_fixture" \
FERRIX_CACHE_DIR="$work/cache" FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" \
FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" \
FERRIX_PIXEL9A_PRODUCT_DIR="$product" FERRIX_PIXEL9A_FIRMWARE_DIR="$firmware" \
FERRIX_PIXEL9A_PROFILE="$product/profile.toml" "$script" validate >/dev/null
[ "$(hash_file "$manifest")" = "$built_manifest_hash" ] ||
	fail 'rootfs validation clobbered the built manifest'
awk -F "$tab" '$1 == "status" && $2 == "inputs-verified" { found=1 } END { exit(found ? 0 : 1) }' "$manifest.inputs-verified.tsv"

PATH="$fake_bin:$PATH" FERRIX_OUT_DIR="$out" FERRIX_PIXEL9A_PINS="$pins_fixture" \
FERRIX_CACHE_DIR="$work/cache" FERRIX_PIXEL9A_RADIO_KERNEL_DIST="$radio_kernel" \
FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST="$radio_kernel/build-manifest.tsv" \
FERRIX_PIXEL9A_PRODUCT_DIR="$product" FERRIX_PIXEL9A_FIRMWARE_DIR="$firmware" \
FERRIX_PIXEL9A_PROFILE="$product/profile.toml" "$script" finalize >/dev/null
awk -F "$tab" '$1 == "status" && $2 == "built-substrate-only" { found=1 } END { exit(found ? 0 : 1) }' "$manifest"

staged="$work/staged"
mkdir -p "$staged"
PATH="$fake_bin:$PATH" tar -xf "$rootfs_artifact" -C "$staged"
for path in $firmware_paths; do
	[ -f "$staged/$path" ] || fail "missing nested staged firmware: $path"
	cmp -s "$firmware/$path" "$staged/$path" || fail "staged firmware changed: $path"
done
[ -L "$staged/vendor/firmware" ] || fail 'missing vendor/firmware compatibility symlink'
[ "$(readlink "$staged/vendor/firmware")" = /lib/firmware ] || fail 'vendor/firmware symlink target changed'
[ -L "$staged/etc/wifi" ] || fail 'missing etc/wifi compatibility symlink'
[ "$(readlink "$staged/etc/wifi")" = /lib/firmware ] || fail 'etc/wifi symlink target changed'

expected_product_files="$work/expected-product-files"
{
	for name in $product_bins; do printf 'usr/bin/%s\n' "$name"; done
	printf '%s\n' etc/ferrix/profile.toml etc/ferrix/colors.toml
	for name in $product_app_descriptors; do printf 'usr/share/ferrix/apps/%s\n' "$name"; done
	printf '%s\n' usr/share/ferrix/contacts/mock-contacts.json
} | LC_ALL=C sort >"$expected_product_files"
actual_product_files="$work/actual-product-files"
{
	find "$staged/usr/bin" -type f -print
	find "$staged/usr/share/ferrix/apps" -type f -print
	find "$staged/usr/share/ferrix/contacts" -type f -print
	printf '%s\n' "$staged/etc/ferrix/profile.toml" "$staged/etc/ferrix/colors.toml"
} | sed "s#^$staged/##" | LC_ALL=C sort >"$actual_product_files"
cmp -s "$expected_product_files" "$actual_product_files" || fail 'product staging file list changed'
for name in $product_bins; do
	[ -x "$staged/usr/bin/$name" ] || fail "staged product binary is not executable: $name"
	cmp -s "$product/$name" "$staged/usr/bin/$name" || fail "staged product binary changed: $name"
done
cmp -s "$product/profile.toml" "$staged/etc/ferrix/profile.toml" || fail 'staged product profile changed'
cmp -s "$product/colors.toml" "$staged/etc/ferrix/colors.toml" || fail 'staged product colors changed'
for name in $product_app_descriptors; do
	cmp -s "$product/apps/$name" "$staged/usr/share/ferrix/apps/$name" || fail "staged app descriptor changed: $name"
done
cmp -s "$product/contacts/mock-contacts.json" "$staged/usr/share/ferrix/contacts/mock-contacts.json" ||
	fail 'staged contacts fixture changed'
[ -f "$staged/etc/ferrix/backends.env" ] || fail 'missing staged backend environment'
cmp -s "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/ferrix-backends.env" \
	"$staged/etc/ferrix/backends.env" || fail 'staged backend environment changed'
cmp -s "$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/s5300-channels.toml" \
	"$staged/usr/share/ferrix/pixel-9a/s5300-channels.toml" || fail 'staged S5300 channel metadata changed'
cmp -s "$package_dir/s5300-userspace-sources.toml" \
	"$staged/usr/share/ferrix/pixel-9a/s5300-userspace-sources.toml" || fail 'staged S5300 source metadata changed'

if grep -En '^[[:space:]]*(adb|fastboot|pmbootstrap[[:space:]]+flasher)([[:space:]]|$)' "$script" >/dev/null; then
	echo "FAIL: rootfs workflow contains device or flashing commands" >&2
	exit 1
fi

printf '%s\n' 'pixel-9a rootfs workflow tests passed'
