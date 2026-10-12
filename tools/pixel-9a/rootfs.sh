#!/bin/sh
set -eu

# Device-free native Linux/postmarketOS rootfs workflow for the exact Pixel 9a
# target. It deliberately has no adb, fastboot, flashing, booting, or firmware
# extraction. FERRIX_PIXEL9A_ANDROID=1 opts into the Waydroid package set in
# pmos/android/packages (owner request 2026-10-09, dev route only); the
# Android images and APK are staged later by tools/pixel-9a/dev-compose.py.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
package_dir="$target/pmos/device-google-tegu"
radio_initfs_module_list="$package_dir/modules-initfs-radio"
pmos_dir="$target/pmos"
pmaports_pins="$pmos_dir/pins.env"
channels_cfg="$pmos_dir/channels.cfg"
exynos_package_dir="$pmos_dir/exynos-modem"
modemmanager_package_dir="$pmos_dir/modemmanager-samsung-sit"
out_root=${FERRIX_OUT_DIR:-"$root/out"}
rootfs_out=${FERRIX_PIXEL9A_ROOTFS_OUT:-"$out_root/pixel-9a/rootfs/tegu"}
manifest=${FERRIX_PIXEL9A_ROOTFS_MANIFEST:-"$rootfs_out/manifest.tsv"}
cache_root=${FERRIX_CACHE_DIR:-"$root/.cache/ferrix"}
rootfs_cache_root=${FERRIX_PIXEL9A_ROOTFS_CACHE_DIR:-"$cache_root/pixel-9a"}
profile_path=${FERRIX_PIXEL9A_PROFILE:-${FERRIX_PIXEL9A_RUNTIME_PROFILE:-"$target/dev/profile.toml"}}
pins=${FERRIX_PIXEL9A_PINS:-"$target/recovery.pins"}
stage_allowlist="$target/firmware-stage-allowlist.tsv"
radio_firmware_route="$target/firmware-radio-route.tsv"
radio_kernel_out=${FERRIX_PIXEL9A_RADIO_KERNEL_OUT:-"$out_root/pixel-9a/kernel-radio/tegu-wwan"}
radio_kernel_dist=${FERRIX_PIXEL9A_RADIO_KERNEL_DIST:-"$radio_kernel_out/dist"}
radio_kernel_manifest=${FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST:-"$radio_kernel_out/build-manifest.tsv"}
radio_route="$target/kernel-radio-route.toml"
radio_split_contract="$target/radio-split-artifact-contract.toml"
radio_split_contract_hash=''
build_token=I-understand-this-runs-pmbootstrap
rootfs_image_size_mib=${FERRIX_PIXEL9A_ROOTFS_IMAGE_SIZE_MIB:-2048}
rootfs_uuid=${FERRIX_PIXEL9A_ROOTFS_UUID:-9a9a9a9a-9a9a-49a9-9a9a-9a9a9a9a9a9a}
rootfs_password=${FERRIX_PIXEL9A_ROOTFS_PASSWORD:-}
android=${FERRIX_PIXEL9A_ANDROID:-0}
android_packages="$pmos_dir/android/packages"
# The kernel the rootfs will be composed with. radio-first (default) is the
# selected WWAN/SIT route; kleaf-dev explicitly pairs the rootfs with the
# Google Kleaf GKI dist that tools/pixel-9a/dev-compose.py composes (the
# development flash route). Never an implicit fallback.
kernel_route=${FERRIX_PIXEL9A_ROOTFS_KERNEL_ROUTE:-radio-first}
manifest_status=''
tab=$(printf '\t')

die() {
	printf 'FAIL pixel-9a-rootfs: %s\n' "$*" >&2
	exit 1
}

hash_file() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | awk '{ print $1 }'
	else
		shasum -a 256 "$1" | awk '{ print $1 }'
	fi
}

sha512_file() {
	command -v sha512sum >/dev/null 2>&1 || die 'missing sha512sum for staged APKBUILD checksum rewriting'
	sha512sum "$1" | awk '{ print $1 }'
}

file_size() {
	wc -c <"$1" | tr -d '[:space:]'
}

manifest_begin() {
	manifest_status=$1
	manifest_dir=$(dirname -- "$manifest")
	mkdir -p "$manifest_dir"
	manifest_tmp="$manifest.tmp.$$"
	{
		printf 'schema_version\t1\n'
		printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
		printf 'codename\ttegu\n'
		printf 'hardware_revision\tMP1.0\n'
		printf 'substrate\tpostmarketos\n'
		if [ "$kernel_route" = kleaf-dev ]; then
			printf 'route_id\tdevelopment-google-kleaf-gki\n'
			printf 'kernel_backend\tgoogle-kleaf-cpif\n'
		else
			printf 'route_id\texperimental-linux-google-tegu-s5300-wwan\n'
			printf 'kernel_backend\tlinux-google-tegu-wwan\n'
		fi
		printf 'pmaports_commit\t%s\n' "${FERRIX_PIXEL9A_PMAPORTS_COMMIT:-$(pinned_pmaports_commit)}"
		printf 'pmaports_channel\t%s\n' "$(pinned_pmaports_channel)"
		printf 'pmaports_branch\t%s\n' "$(pinned_pmaports_branch)"
		printf 'initramfs_policy\texternal-kernel-owned-not-generated\n'
		printf 'firmware_mapping\tfirmware-radio-route.tsv\t%s\n' "$(hash_file "$radio_firmware_route")"
		printf 'split_artifact_contract\tradio-split-artifact-contract.toml\t%s\n' "$radio_split_contract_hash"
		printf 'status\t%s\n' "$1"
		printf 'bootable\tfalse\n'
		printf 'firmware_policy\texternal-hash-manifest-no-extraction\n'
	} >"$manifest_tmp"
}

manifest_artifact() {
	name=$1
	path=$2
	[ -f "$path" ] || die "manifest artifact is missing: $path"
	printf 'artifact\t%s\t%s\t%s\n' "$name" "$(file_size "$path")" "$(hash_file "$path")" >>"$manifest_tmp"
}

manifest_input() {
	name=$1
	path=$2
	[ -f "$path" ] || die "manifest input is missing: $path"
	printf 'input\t%s\t%s\t%s\n' "$name" "$(file_size "$path")" "$(hash_file "$path")" >>"$manifest_tmp"
}

manifest_finish() {
	manifest_destination=$manifest
	# A read-only preflight or validation after a successful build must not
	# downgrade the canonical built handoff. Keep its diagnostic result in an
	# adjacent status-named sidecar; the explicit build/finalize path is the
	# only path allowed to replace the canonical manifest.
	if [ "$manifest_status" != built-substrate-only ] && [ -f "$manifest" ]; then
		existing_status=$(awk -F "$tab" '$1 == "status" { print $2; exit }' "$manifest" 2>/dev/null || true)
		if [ "$existing_status" = built-substrate-only ]; then
			manifest_destination="$manifest.$manifest_status.tsv"
		fi
	fi
	mv "$manifest_tmp" "$manifest_destination"
	printf 'manifest: %s\n' "$manifest_destination"
}

required_file() {
	[ -f "$1" ] || die "required target input is missing: $1"
}

pinned_pmaports_commit() {
	# Keep the physical target's substrate pin with its own pmOS package.  The
	# QEMU pin file is a separate target and must never silently select the
	# physical rootfs source revision.
	awk -F '=' '$1 == "PMAPORTS_COMMIT" { value=$2; gsub(/[[:space:]]/, "", value); print value; found=1; exit } END { if (!found) exit 1 }' \
		"$pmaports_pins"
}

pinned_pmaports_channel() {
	awk -F '=' '$1 == "PMAPORTS_CHANNEL" { value=$2; gsub(/[[:space:]]/, "", value); print value; found=1; exit } END { if (!found) exit 1 }' \
		"$pmaports_pins"
}

pinned_pmaports_branch() {
	awk -F '=' '$1 == "PMAPORTS_BRANCH" { value=$2; gsub(/[[:space:]]/, "", value); print value; found=1; exit } END { if (!found) exit 1 }' \
		"$pmaports_pins"
}

# android_package_list: the opt-in Waydroid package set as one comma list.
# Refuses packages that need nftables, which the pinned GKI does not have.
android_package_list() {
	required_file "$android_packages"
	list=$(awk '$0 !~ /^[[:space:]]*(#|$)/ { gsub(/[[:space:]]/, ""); printf "%s%s", (n++ ? "," : ""), $0 }' "$android_packages")
	[ -n "$list" ] || die "Android package set is empty: $android_packages"
	case ",$list," in
		*,nftables,*|*,waydroid-nftrules,*) die 'Android package set must not contain nftables or waydroid-nftrules (the GKI has no nftables)' ;;
		*[!a-z0-9,.+_-]*) die "Android package set has an invalid package name: $list" ;;
	esac
	case ",$list," in
		*,waydroid,*) ;;
		*) die 'Android package set must contain waydroid' ;;
	esac
	printf '%s\n' "$list"
}

# verify_android_rootfs DB: the installed rootfs carries the Android package
# set (by name or apk provides, e.g. dnsmasq from dnsmasq-dnssec-dbus) and not
# waydroid-nftrules, whose nft ruleset the GKI cannot load. nftables itself is
# pulled in by postmarketos-base; without CONFIG_NF_TABLES `nft list ruleset`
# fails and waydroid-net.sh falls back to iptables(-legacy).
verify_android_rootfs() {
	installed=$(cat "$1" 2>/dev/null || sudo -n cat "$1" 2>/dev/null) ||
		die "built rootfs has no readable apk database: $1"
	for package in $(android_package_list | tr ',' ' '); do
		printf '%s\n' "$installed" | grep -qx "P:$package" && continue
		printf '%s\n' "$installed" | grep -Eq "^p:(.* )?$package(=[^ ]*)?( |\$)" && continue
		die "built rootfs is missing Android package: $package"
	done
	if printf '%s\n' "$installed" | grep -qx 'P:waydroid-nftrules'; then
		die 'built rootfs contains waydroid-nftrules, which the GKI cannot use'
	fi
}

# The console UI's default audio provider is PulseAudio. It installs
# pulseaudio-wireplumber, whose 51-pulseaudio.conf disables WirePlumber's ALSA
# and Bluetooth hardware monitors. Reject that dependency before producing a
# Ferrix image; the target selects the PipeWire provider in pmbootstrap.cfg.
verify_audio_backend_rootfs() {
	installed=$(cat "$1" 2>/dev/null || sudo -n cat "$1" 2>/dev/null) ||
		die "built rootfs has no readable apk database: $1"
	if printf '%s\n' "$installed" | grep -Eq '^P:(postmarketos-base-ui-audio-backend-pulseaudio|pulseaudio-wireplumber)$'; then
		die 'built rootfs contains PulseAudio WirePlumber hardware-monitor override'
	fi
	printf '%s\n' "$installed" | grep -qx 'P:postmarketos-base-ui-audio-backend-pipewire' ||
		die 'built rootfs did not select the PipeWire UI audio backend'
	printf '%s\n' "$installed" | grep -qx 'P:alsa-utils' ||
		die 'built rootfs lacks ALSA mixer/playback/capture diagnostics'
}

check_static_inputs() {
	case "$android" in
		0|1) ;;
		*) die "FERRIX_PIXEL9A_ANDROID must be 0 or 1: $android" ;;
	esac
	case "$kernel_route" in
		radio-first|kleaf-dev) ;;
		*) die "FERRIX_PIXEL9A_ROOTFS_KERNEL_ROUTE must be radio-first or kleaf-dev: $kernel_route" ;;
	esac
	for file in pmbootstrap.cfg channels.cfg; do
		required_file "$pmos_dir/$file"
	done
	required_file "$pmaports_pins"
	[ -d "$package_dir" ] || die "target-local pmOS package is missing: $package_dir"
	for file in APKBUILD deviceinfo kernel-cmdline.conf modules-initfs modules-initfs-radio initramfs-init vendor-bootconfig modules-vendor-dlkm modules-system-dlkm boot-layout.toml firmware-policy.toml firmware-allowlist.tsv s5300-channels.toml s5300-userspace-sources.toml ferrix-backends.env ferrix-openrc.sh networkmanager-ferrix.conf modprobe-zram-gs.conf ferrix-core.initd ferrix-networkd.initd ferrix-telephonyd.initd ferrix-audio.initd ferrix-pipewire.initd ferrix-product.initd ferrix-s5300-mounts.initd ferrix-radio-firmware.initd ferrix-aoc-firmware.initd ferrix-bluetooth-uart.initd ferrix-contactsd.initd exynos-modem.initd sit-proxy.initd 99-google-tegu-sit.rules ucm-aoc-snd-card.conf ucm-tegu-HiFi.conf; do
		required_file "$package_dir/$file"
	done
	[ -d "$package_dir/vendor-ramdisk/ferrix" ] || die "source-owned vendor ramdisk is missing: $package_dir/vendor-ramdisk/ferrix"
	for file in target-id route-id rootfs-partlabel; do
		required_file "$package_dir/vendor-ramdisk/ferrix/$file"
	done
	required_file "$root/tools/pixel-9a/pmbootstrap-rootfs.py"
	required_file "$radio_firmware_route"
	required_file "$radio_route"
	required_file "$radio_split_contract"
	[ ! -L "$radio_split_contract" ] || die "radio split-artifact contract must not be a symlink: $radio_split_contract"
	route_contract=$(awk -F '=' '$1 ~ /^[[:space:]]*split_artifact_contract[[:space:]]*$/ { value=$2; gsub(/[[:space:]"]/, "", value); print value; found=1; exit } END { if (!found) exit 1 }' "$radio_route") ||
		die 'radio route is missing split_artifact_contract'
	[ "$route_contract" = radio-split-artifact-contract.toml ] || die "radio route split-artifact contract is not canonical: $route_contract"
	radio_split_contract_hash=$(awk -F '=' '$1 ~ /^[[:space:]]*split_artifact_contract_sha256[[:space:]]*$/ { value=$2; gsub(/[[:space:]"]/, "", value); print value; found=1; exit } END { if (!found) exit 1 }' "$radio_route") ||
		die 'radio route is missing split_artifact_contract_sha256'
	case "$radio_split_contract_hash" in
		''|*[!0-9a-fA-F]*) die 'radio route split_artifact_contract_sha256 is malformed' ;;
	esac
	[ "${#radio_split_contract_hash}" -eq 64 ] || die 'radio route split_artifact_contract_sha256 must contain 64 hexadecimal characters'
	[ "$(hash_file "$radio_split_contract")" = "$radio_split_contract_hash" ] || die 'radio split-artifact contract hash differs from the radio route'
	for package_path in "$exynos_package_dir/APKBUILD" "$modemmanager_package_dir/APKBUILD" "$modemmanager_package_dir/modemmanager.initd" "$modemmanager_package_dir/modemmanager.rules"; do
		required_file "$package_path"
	done
	target_pmaports_commit=$(pinned_pmaports_commit) || die 'target pmOS package is missing PMAPORTS_COMMIT'
	target_pmaports_channel=$(pinned_pmaports_channel) || die 'target pmOS package is missing PMAPORTS_CHANNEL'
	target_pmaports_branch=$(pinned_pmaports_branch) || die 'target pmOS package is missing PMAPORTS_BRANCH'
	[ "$target_pmaports_channel" = v26.06 ] || die "unsupported target pmaports channel: $target_pmaports_channel"
	[ "$target_pmaports_branch" = v26.06 ] || die "unsupported target pmaports branch: $target_pmaports_branch"
	grep -q '^\[v26\.06\]$' "$channels_cfg" || die 'target channel metadata lacks the pinned v26.06 section'
	grep -q '^branch_pmaports=v26\.06$' "$channels_cfg" || die 'target channel metadata has the wrong pmaports branch'
	grep -q '^branch_aports=3\.24-stable$' "$channels_cfg" || die 'target channel metadata has the wrong Alpine aports branch'
	grep -q '^mirrordir_alpine=v3\.24$' "$channels_cfg" || die 'target channel metadata has the wrong Alpine mirror directory'
	case "$target_pmaports_commit" in
		''|*[!0-9a-fA-F]*) die 'target pmOS PMAPORTS_COMMIT is not a hexadecimal revision' ;;
	esac
	[ "${#target_pmaports_commit}" -eq 40 ] || die 'target pmOS PMAPORTS_COMMIT must contain 40 hexadecimal characters'
	[ ! -e "$pmos_dir/s5300-userspace-sources.toml" ] || die "duplicate S5300 source manifest remains outside the device package"
	required_file "$pins"
	required_file "$stage_allowlist"
	grep -q '^deviceinfo_codename="google-tegu"$' "$package_dir/deviceinfo" || die "deviceinfo codename is not google-tegu"
	grep -q '^deviceinfo_arch="aarch64"$' "$package_dir/deviceinfo" || die "deviceinfo arch is not aarch64"
	grep -q '^deviceinfo_flash_method="none"$' "$package_dir/deviceinfo" || die "deviceinfo must keep flashing disabled"
	grep -Eq '^(export[[:space:]]+)?FERRIX_TARGET=google-pixel-9a-tegu-gxq96-mp1-0$' "$package_dir/ferrix-backends.env" || die "target environment identity mismatch"
	grep -Eq '^(export[[:space:]]+)?FERRIX_EVIDENCE=physical:google-pixel-9a-tegu-gxq96-mp1-0$' "$package_dir/ferrix-backends.env" || die "target evidence label is missing"
	grep -Eq '^(export[[:space:]]+)?FERRIX_BLUETOOTH_BACKEND=bluez$' "$package_dir/ferrix-backends.env" || die "BlueZ backend selection is missing"
	grep -Eq '^(export[[:space:]]+)?FERRIX_S5300_KERNEL_BACKEND=linux-google-tegu-wwan$' "$package_dir/ferrix-backends.env" || die "S5300 userspace/kernel ABI pairing is missing"
	grep -Eq '^(export[[:space:]]+)?FERRIX_POWER_BACKEND=upower$' "$package_dir/ferrix-backends.env" || die "UPower backend selection is missing"
	grep -q '[[:space:]]modules-initfs-radio$' "$package_dir/APKBUILD" || die "device package does not carry the radio-first initfs module list"
	grep -q '^commit = "634e8eb404c3fcf959dce6a0b0d97b33ba0fc346"$' "$package_dir/s5300-userspace-sources.toml" || die "exynos-modem source commit is not pinned"
	grep -q '^commit = "14a0013a28e5d5eec513c65e75d7549678ecc61e"$' "$package_dir/s5300-userspace-sources.toml" || die "SIT ModemManager source commit is not pinned"
	grep -q '^sha256 = "324efde8cdef9c3e108cc92d9248556cdc8c0234e77a8a66218b49b978a9445f"$' "$package_dir/s5300-userspace-sources.toml" || die "exynos-modem archive hash is not pinned"
	grep -q '^sha256 = "6c11c4de4926d2e928e86fd1c5fbcfeebb515c373ddc8a515ace62d35de391b2"$' "$package_dir/s5300-userspace-sources.toml" || die "SIT ModemManager archive hash is not pinned"
	grep -q 'fa081999130a9775a9140fa16901d4dcc9a2733ebd9af1eb55b03717659a952' "$exynos_package_dir/APKBUILD" || die "exynos-modem recipe archive hash is not pinned"
	grep -q '9add05d1a18a662cb0c0f6b9048279355a8d1b2933263df0f74fbb791cb97265' "$modemmanager_package_dir/APKBUILD" || die "SIT ModemManager recipe archive hash is not pinned"
	grep -q '^modem_image_label = "modem_{androidboot.slot_suffix}"$' "$package_dir/s5300-userspace-sources.toml" || die "current-slot modem mount label is not pinned"
	grep -q '^modem_image_label_policy = "exact-current-slot-only"$' "$package_dir/s5300-userspace-sources.toml" || die "current-slot modem mount policy is not fail-closed"
	grep -q '^efs_label = "efs"$' "$package_dir/s5300-userspace-sources.toml" || die "efs mount label is not pinned"
	grep -q '^persist_label = "persist"$' "$package_dir/s5300-userspace-sources.toml" || die "persist mount label is not pinned"
	grep -q '^modem_userdata_label = "modem_userdata"$' "$package_dir/s5300-userspace-sources.toml" || die "modem_userdata mount label is not pinned"
	grep -q 'source_policy = "immutable-source-archives-only"' "$package_dir/s5300-userspace-sources.toml" || die "S5300 source policy is not immutable"
	grep -q 'android_framework = false' "$package_dir/s5300-userspace-sources.toml" || die "Android framework boundary is not disabled"
	grep -q 'android_ril = false' "$package_dir/s5300-userspace-sources.toml" || die "Android RIL boundary is not disabled"
	grep -q 'android_hal = false' "$package_dir/s5300-userspace-sources.toml" || die "Android HAL boundary is not disabled"
	required_file "$profile_path"
	grep -q '^schema_version = 1$' "$profile_path" || die "target runtime profile schema is not 1: $profile_path"
	grep -q '^name = "pixel-9a-native-linux"$' "$profile_path" || die "target runtime profile name is not pixel-9a-native-linux: $profile_path"
}

preflight() {
	check_static_inputs
	for command_name in awk find grep sed tr wc; do
		command -v "$command_name" >/dev/null 2>&1 || die "missing prerequisite '$command_name'"
	done
	command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || die "missing sha256sum or shasum"
	manifest_begin not-built
	manifest_artifact deviceinfo "$package_dir/deviceinfo"
	manifest_artifact kernel-cmdline.conf "$package_dir/kernel-cmdline.conf"
	manifest_artifact modules-initfs "$package_dir/modules-initfs"
	manifest_artifact modules-initfs-radio "$radio_initfs_module_list"
	manifest_artifact boot-layout.toml "$package_dir/boot-layout.toml"
	manifest_artifact firmware-policy.toml "$package_dir/firmware-policy.toml"
	manifest_artifact s5300-channels.toml "$package_dir/s5300-channels.toml"
	manifest_artifact s5300-userspace-sources.toml "$package_dir/s5300-userspace-sources.toml"
	manifest_artifact firmware-radio-route.tsv "$radio_firmware_route"
	manifest_artifact pmos/pins.env "$pmaports_pins"
	manifest_artifact pmos/channels.cfg "$channels_cfg"
	manifest_artifact exynos-modem/APKBUILD "$exynos_package_dir/APKBUILD"
	manifest_artifact modemmanager-samsung-sit/APKBUILD "$modemmanager_package_dir/APKBUILD"
	manifest_artifact profile.toml "$profile_path"
	manifest_finish
	if [ "$android" = 1 ]; then
		android_package_list >/dev/null
		manifest_artifact android/packages "$android_packages"
	fi
	printf '%s\n' 'PASS pixel-9a-rootfs static package and manifest checks'
}

kernel_manifest_path() {
	dist=$1
	if [ -f "$dist/../build-manifest.tsv" ]; then
		printf '%s\n' "$dist/../build-manifest.tsv"
	elif [ -f "$dist/build-manifest.tsv" ]; then
		printf '%s\n' "$dist/build-manifest.tsv"
	else
		die "kernel manifest is missing beside dist: $dist/../build-manifest.tsv"
	fi
}

verify_radio_manifest_input() {
	key=$1
	label=$2
	row=$(awk -F "$tab" -v key="$key" '$1 == key { print; found=1; exit } END { if (!found) exit 1 }' "$radio_kernel_manifest") ||
		die "$label is missing the $key split-input row"
	relative=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $2 }')
	expected_size=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $3 }')
	expected_hash=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $4 }')
	case "$relative" in
		''|/*|*..*|*' '*) die "$label has an unsafe $key path: $relative" ;;
	esac
	case "$expected_size" in ''|*[!0-9]*) die "$label has an invalid $key byte count" ;; esac
	case "$expected_hash" in ''|*[!0-9a-f]*) die "$label has an invalid $key hash" ;; esac
	[ "${#expected_hash}" -eq 64 ] || die "$label has an invalid $key hash length"
	file="$radio_kernel_dist/$relative"
	[ -f "$file" ] || die "$label split input is missing: $file"
	[ ! -L "$file" ] || die "$label split input must not be a symlink: $file"
	[ "$(file_size "$file")" = "$expected_size" ] || die "$label $key byte count differs from its manifest"
	[ "$(hash_file "$file")" = "$expected_hash" ] || die "$label $key hash differs from its manifest"
}

verify_radio_partition_sizes() {
	row=$(awk -F "$tab" '$1 == "partition_sizes" { print; found=1; exit } END { if (!found) exit 1 }' "$radio_kernel_manifest") ||
		die 'radio-first kernel manifest is missing the partition_sizes split-input row'
	relative=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $2 }')
	case "$relative" in
		''|/*|*..*|*' '*) die "radio-first kernel manifest has an unsafe partition_sizes path: $relative" ;;
	esac
	file="$radio_kernel_dist/$relative"
	[ -f "$file" ] || die "radio-first kernel partition_sizes split input is missing: $file"
	[ ! -L "$file" ] || die 'radio-first kernel partition_sizes split input must not be a symlink'
	schema_count=$(awk -F "$tab" '$1 == "schema_version" && $2 == "1" { count++ } END { print count + 0 }' "$file")
	[ "$schema_count" -eq 1 ] || die 'radio-first kernel partition_sizes must contain exactly one schema_version=1 row'
	seen=''
	count=0
	while IFS="$tab" read -r kind partition status extra; do
		case "$kind" in
			''|'#'*) continue ;;
			schema_version)
				[ "$partition" = 1 ] && [ -z "${status:-}" ] && [ -z "${extra:-}" ] ||
					die 'radio-first kernel partition_sizes has a malformed schema_version row'
				;;
			partition)
				[ -n "$partition" ] && [ "$status" = status=not-built ] && [ -z "${extra:-}" ] ||
					die 'radio-first kernel partition_sizes has a malformed partition row'
				case "$partition" in
					boot|init_boot|vendor_boot|vendor_kernel_boot|dtbo|vendor_dlkm|system_dlkm|vbmeta|vbmeta_vendor|vbmeta_system) ;;
					*) die "radio-first kernel partition_sizes names an unexpected partition: $partition" ;;
				esac
				case " $seen " in
					*" $partition "*) die "radio-first kernel partition_sizes repeats partition: $partition" ;;
				esac
				seen="$seen $partition"
				count=$((count + 1))
				;;
			*) die "radio-first kernel partition_sizes has an unknown row: $kind" ;;
		esac
	done <"$file"
	[ "$count" -eq 10 ] || die 'radio-first kernel partition_sizes must contain all 10 split partitions'
	for partition in boot init_boot vendor_boot vendor_kernel_boot dtbo vendor_dlkm system_dlkm vbmeta vbmeta_vendor vbmeta_system; do
		case " $seen " in
			*" $partition "*) ;;
			*) die "radio-first kernel partition_sizes is missing partition: $partition" ;;
		esac
	done
}

required_kernel_modules() {

	[ -f "$package_dir/modules-initfs" ] || die "target initfs module list is missing: $package_dir/modules-initfs"
	while IFS= read -r module; do
		case "$module" in
			''|'#'*) continue ;;
		esac
		find "$1" -type f -name "$module.ko" -print -quit | grep -q . && continue
		# A GKI dist records built-in drivers in modules.builtin (kbuild names
		# use '-' where module names use '_'); those need no .ko file.
		if [ -f "$1/modules.builtin" ] && awk -v want="$(printf '%s' "$module" | tr '-' '_').ko" '
			{ name = $0; sub(/.*\//, "", name); gsub(/-/, "_", name); if (name == want) { found = 1; exit } }
			END { exit(found ? 0 : 1) }' "$1/modules.builtin"; then
			continue
		fi
		die "kernel dist is missing required tegu module: $module.ko"
	done < "$package_dir/modules-initfs"
}

validate_google_kernel_dist() {
	dist=${FERRIX_PIXEL9A_KERNEL_DIST:-"$out_root/pixel-9a/kernel/tegu/dist"}
	[ -d "$dist" ] || die "missing Google Kleaf dist: $dist; run the pinned kernel build first"
	find "$dist" -type f -print -quit | grep -q . || die "Google Kleaf dist is empty: $dist"
	kmanifest=$(kernel_manifest_path "$dist")
	for name in dtbo.img vendor_kernel_boot.img vendor_dlkm.img system_dlkm.img; do
		awk -F '\t' -v name="$name" '$1 == "artifact" && $2 == name { found=1 } END { exit(found ? 0 : 1) }' "$kmanifest" || die "kernel manifest lacks required artifact row: $name"
		[ -f "$dist/$name" ] || die "kernel dist lacks manifest artifact: $dist/$name"
	done
	required_kernel_modules "$dist"
}

validate_radio_kernel_dist() {
	[ -d "$radio_kernel_dist" ] ||
		die "missing radio-first Kbuild dist: $radio_kernel_dist; run just pixel-9a-kernel-radio-build first"
	[ -f "$radio_kernel_manifest" ] ||
		die "missing radio-first kernel manifest: $radio_kernel_manifest"
	[ "$(awk -F "$tab" '$1 == "target_id" { print $2; exit }' "$radio_kernel_manifest")" = google-pixel-9a-tegu-gxq96-mp1-0 ] ||
		die 'radio-first kernel manifest target_id is not the exact Pixel 9a target'
	[ "$(awk -F "$tab" '$1 == "route_id" { print $2; exit }' "$radio_kernel_manifest")" = experimental-linux-google-tegu-s5300-wwan ] ||
		die 'radio-first kernel manifest route_id is not the selected radio route'
	[ "$(awk -F "$tab" '$1 == "kernel_backend" { print $2; exit }' "$radio_kernel_manifest")" = linux-google-tegu-wwan ] ||
		die 'radio-first kernel manifest does not select linux-google-tegu-wwan'
	contract_row=$(awk -F "$tab" '$1 == "split_artifact_contract" { print; found=1; exit } END { if (!found) exit 1 }' "$radio_kernel_manifest") ||
		die 'radio-first kernel manifest lacks the split-artifact contract row'
	[ "$(printf '%s\n' "$contract_row" | awk -F "$tab" '{ print $2 }')" = radio-split-artifact-contract.toml ] ||
		die 'radio-first kernel manifest split-artifact contract path is not canonical'
	[ "$(printf '%s\n' "$contract_row" | awk -F "$tab" '{ print $3 }')" = "$(awk -F '=' '$1 ~ /^[[:space:]]*split_artifact_contract_sha256[[:space:]]*$/ { value=$2; gsub(/[[:space:]"]/, "", value); print value; found=1; exit } END { if (!found) exit 1 }' "$radio_route")" ] ||
		die 'radio-first kernel manifest split-artifact contract hash differs from the radio route'
	mapping_row=$(awk -F "$tab" '$1 == "firmware_mapping" { print; found=1; exit } END { if (!found) exit 1 }' "$radio_kernel_manifest") ||
		die 'radio-first kernel manifest lacks the firmware mapping row'
	[ "$(printf '%s\n' "$mapping_row" | awk -F "$tab" '{ print $2 }')" = firmware-radio-route.tsv ] ||
		die 'radio-first kernel manifest firmware mapping path is not canonical'
	[ "$(printf '%s\n' "$mapping_row" | awk -F "$tab" '{ print $3 }')" = "$(hash_file "$radio_firmware_route")" ] ||
		die 'radio-first kernel manifest firmware mapping hash differs from the checked-in route'
	[ "$(awk -F "$tab" '$1 == "status" { print $2; exit }' "$radio_kernel_manifest")" = built-not-booted ] ||
		die 'radio-first kernel manifest is not a completed build handoff'
	[ "$(awk -F "$tab" '$1 == "bootable" { print $2; exit }' "$radio_kernel_manifest")" = false ] ||
		die 'radio-first kernel manifest must remain bootable=false before evidence'
	for input in dtbo_source_set module_install partition_sizes; do
		verify_radio_manifest_input "$input" 'radio-first kernel manifest'
	done
	verify_radio_partition_sizes
	for name in Image dtb.img; do
		row=$(awk -F "$tab" -v name="$name" '$1 == "artifact" && $2 == name { print; found=1; exit } END { if (!found) exit 1 }' "$radio_kernel_manifest") ||
			die "radio-first kernel manifest lacks required artifact row: $name"
		relative=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $2 }')
		expected_bytes=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $3 }')
		expected_hash=$(printf '%s\n' "$row" | awk -F "$tab" '{ print $4 }')
		[ "$relative" = "$name" ] || die "radio-first kernel artifact path is not canonical: $name"
		file="$radio_kernel_dist/$relative"
		[ -f "$file" ] || die "radio-first kernel artifact is missing: $file"
		[ ! -L "$file" ] || die "radio-first kernel artifact must not be a symlink: $file"
		[ "$(file_size "$file")" = "$expected_bytes" ] || die "radio-first kernel artifact size mismatch: $name"
		[ "$(hash_file "$file")" = "$expected_hash" ] || die "radio-first kernel artifact hash mismatch: $name"
	done
}

validate_kernel_dist() {
	# The selected target package is paired with the radio-first WWAN/SIT
	# backend. The Google Kleaf/cpif validator is used only when the caller
	# explicitly selects the development route's kernel, never as a fallback.
	case "$kernel_route" in
		radio-first) validate_radio_kernel_dist ;;
		kleaf-dev) validate_google_kernel_dist ;;
	esac
}

validate_product_dir() {
	product=${FERRIX_PIXEL9A_PRODUCT_DIR:-"$out_root/pixel-9a/product/tegu"}
	product_manifest="$product/manifest.tsv"
	[ -f "$product_manifest" ] || die "missing native Pixel product manifest $product_manifest; run just pixel-9a-product-build"
	awk -F '\t' '
		$1 == "artifact" && NF != 4 { bad=1 }
		$1 == "artifact" && $3 !~ /^[0-9]+$/ { bad=1 }
		$1 == "artifact" && $4 !~ /^[0-9a-f]{64}$/ { bad=1 }
		END { exit bad }
	' "$product_manifest" || die "native Pixel product manifest has malformed artifact rows"
	for required in \
		'target_id	google-pixel-9a-tegu-gxq96-mp1-0' \
		'evidence	physical:google-pixel-9a-tegu-gxq96-mp1-0' \
		'backend_mode	native-linux' \
		'rust_target	aarch64-unknown-linux-musl' \
		'status	built' \
		'bootable	false'; do
		key=${required%%"$tab"*}
		value=${required#*"$tab"}
		awk -F '\t' -v key="$key" -v value="$value" \
			'$1 == key && $2 == value { found=1 } END { exit(found ? 0 : 1) }' \
			"$product_manifest" || die "native Pixel product manifest is missing $key=$value"
	done
	awk -F '\t' '$1 == "rust_linker" && $2 != "" { found=1 } END { exit(found ? 0 : 1) }' "$product_manifest" ||
		die 'native Pixel product manifest is missing its target Rust linker'
	for name in ferrix-core ferrix-shell ferrix-basic-screen ferrix-appd ferrix-sandbox ferrix-terminal \
		ferrix-telephonyd ferrix-networkd ferrix-contactsd ferrix-phone ferrix-messages ferrix-contacts ferrix-settings ferrix-files \
		profile.toml colors.toml; do
		[ -f "$product/$name" ] || die "missing product input $product/$name"
	done
	[ -d "$product/apps" ] || die "missing product app catalog $product/apps"
	for descriptor in "$product"/apps/*.json; do
		[ -f "$descriptor" ] || die "missing product app descriptor under $product/apps"
	done
	for name in ferrix-core ferrix-shell ferrix-basic-screen ferrix-appd ferrix-sandbox ferrix-terminal \
		ferrix-telephonyd ferrix-networkd ferrix-contactsd ferrix-phone ferrix-messages ferrix-contacts ferrix-settings ferrix-files \
		profile.toml colors.toml; do
		verify_product_artifact "$name"
	done
	for descriptor in "$product"/apps/*.json; do
		verify_product_artifact "apps/$(basename -- "$descriptor")"
	done
}

verify_product_artifact() {
	relative=$1
	case "$relative" in
		''|/*|.|..|../*|*/../*|*/..) die "product manifest contains an unsafe artifact path: $relative" ;;
	esac
	row=$(awk -F '\t' -v name="$relative" '$1 == "artifact" && $2 == name { print $0; found=1; exit } END { if (!found) exit 1 }' "$product_manifest") ||
		die "native Pixel product manifest is missing artifact: $relative"
	expected_bytes=$(printf '%s\n' "$row" | awk -F '\t' '{ print $3 }')
	expected_hash=$(printf '%s\n' "$row" | awk -F '\t' '{ print $4 }')
	file="$product/$relative"
	[ -f "$file" ] || die "native Pixel product artifact is missing: $file"
	[ ! -L "$file" ] || die "native Pixel product artifact must not be a symlink: $file"
	[ "$(file_size "$file")" = "$expected_bytes" ] || die "native Pixel product artifact size mismatch: $relative"
	[ "$(hash_file "$file")" = "$expected_hash" ] || die "native Pixel product artifact hash mismatch: $relative"
}

allowlisted() {
	path=$1
	awk -F '\t' -v path="$path" '
		$0 !~ /^[[:space:]]*#/ && NF >= 3 &&
		($2 == path || (index(path, $2 "/") == 1 && $2 !~ /\/$/)) {
			print $3
			found=1
		}
		END { exit(found ? 0 : 1) }
	' "$package_dir/firmware-allowlist.tsv"
}

factory_pin() {
	awk -F '\t' -v number="$1" '$1 == "factory" { print $number; exit }' "$pins"
}

stage_allowlisted() {
	controller=$1
	source_image=$2
	source_path=$3
	runtime_path=$4
	awk -F '\t' -v controller="$controller" -v source_image="$source_image" \
		-v source_path="$source_path" -v runtime_path="$runtime_path" \
		'
			$0 !~ /^[[:space:]]*#/ && $1 == controller && $2 == source_image {
				if (NF == 4 && $3 == source_path && $4 == runtime_path) {
					found=1
				}
				if (NF == 5 && $5 == "bundle" && index(source_path, $3 "/") == 1 && index(runtime_path, $4 "/") == 1) {
					source_suffix=substr(source_path, length($3) + 2)
					runtime_suffix=substr(runtime_path, length($4) + 2)
					if (source_suffix != "" && source_suffix == runtime_suffix) {
						found=1
					}
				}
			}
			END { exit(found ? 0 : 1) }
		' \
		"$stage_allowlist"
}

validate_firmware_dir() {
	firmware=${FERRIX_PIXEL9A_FIRMWARE_DIR:-}
	[ -n "$firmware" ] || die 'FERRIX_PIXEL9A_FIRMWARE_DIR is required; firmware is never extracted by this workflow'
	[ -d "$firmware" ] || die "firmware directory is missing: $firmware"
	[ ! -f "$firmware" ] || die "firmware input must be a directory, not an archive: $firmware"
	fmanifest="$firmware/firmware-manifest.tsv"
	[ -f "$fmanifest" ] || die "missing external firmware manifest: $fmanifest"
	awk -F '\t' '
		$0 !~ /^[[:space:]]*#/ && NF > 0 &&
			(($1 == "firmware" && NF != 7) || ($1 == "factory_archive" && NF != 3) ||
			 ($1 == "source" && NF != 4) || ($1 == "schema_version" && NF != 2) ||
			 ($1 == "target_id" && NF != 2) ||
			 ($1 != "firmware" && $1 != "factory_archive" && $1 != "source" &&
			  $1 != "schema_version" && $1 != "target_id")) { bad=1 }
		END { exit bad }
	' "$fmanifest" || die 'external firmware manifest is not the canonical firmware workflow format'
	awk -F '\t' '
		$0 !~ /^[[:space:]]*#/ && NF > 0 &&
		(NF != 4 && NF != 5 || (NF == 5 && $5 != "bundle")) { bad=1 }
		END { exit bad }
	' "$stage_allowlist" || die "target firmware stage allowlist is malformed: $stage_allowlist"
	stage_file_count=$(awk -F '\t' '$0 !~ /^[[:space:]]*#/ && NF == 4 { count++ } END { print count + 0 }' "$stage_allowlist")
	stage_bundle_count=$(awk -F '\t' '$0 !~ /^[[:space:]]*#/ && NF == 5 && $5 == "bundle" { count++ } END { print count + 0 }' "$stage_allowlist")
	stage_count=$((stage_file_count + stage_bundle_count))
	[ "$stage_count" -gt 0 ] || die "target firmware stage allowlist is empty or malformed: $stage_allowlist"
	awk -F '\t' '$0 !~ /^[[:space:]]*#/ && (NF == 4 || (NF == 5 && $5 == "bundle")) { if (seen[$4]++) { duplicate=1 } } END { exit(duplicate ? 1 : 0) }' "$stage_allowlist" ||
		die "target firmware stage allowlist contains duplicate runtime paths: $stage_allowlist"
	factory_name=$(factory_pin 4)
	factory_hash=$(factory_pin 6)
	[ -n "$factory_name" ] && [ -n "$factory_hash" ] || die "factory recovery pin is missing from $pins"
	awk -F '\t' '
		$1 == "firmware" {
			if (seen[$5]++) { duplicate = 1 }
		}
		END { exit(duplicate ? 1 : 0) }
	' "$fmanifest" || die 'external firmware manifest contains duplicate runtime paths'
	factory_count=0
	firmware_count=0
	while IFS="$tab" read -r kind field2 field3 field4 field5 field6 field7; do
		case "$kind" in
			''|'#'*) continue ;;
			schema_version)
				[ "$field2" = 1 ] || die "external firmware manifest schema_version is not 1"
				[ -z "${field3:-}" ] || die 'external firmware manifest schema_version row has extra fields'
				continue
				;;
			target_id)
				[ "$field2" = google-pixel-9a-tegu-gxq96-mp1-0 ] || die 'external firmware manifest target_id is not the exact Pixel 9a target'
				[ -z "${field3:-}" ] || die 'external firmware manifest target_id row has extra fields'
				continue
				;;
			source)
				case "$field2" in
					vendor.img|vendor_dlkm.img) ;;
					*) die "external firmware manifest has an unsupported source image row: $field2" ;;
				 esac
				case "$field3" in ''|*[!0-9]*) die "invalid source image byte count: $field2" ;; esac
				case "$field4" in ''|*[!0-9a-fA-F]*) die "invalid source image sha256: $field2" ;; esac
				[ "${#field4}" -eq 64 ] || die "invalid source image sha256 length: $field2"
				continue
				;;
			factory_archive)
				factory_count=$((factory_count + 1))
				[ "$factory_count" -eq 1 ] || die 'external firmware manifest contains multiple factory_archive rows'
				[ -n "${field2:-}" ] && [ -n "${field3:-}" ] || die 'external firmware manifest factory_archive row is incomplete'
				[ -z "${field4:-}" ] && [ -z "${field5:-}" ] && [ -z "${field6:-}" ] && [ -z "${field7:-}" ] || die 'external firmware manifest factory_archive row has extra fields'
				[ "${field2##*/}" = "$factory_name" ] || die "external firmware manifest factory archive is not the pinned file: $field2"
				[ "$field3" = "$factory_hash" ] || die 'external firmware manifest factory archive hash is not the pinned factory hash'
				[ -f "$field2" ] || die "external firmware manifest factory archive is missing: $field2"
				[ ! -L "$field2" ] || die "external firmware manifest factory archive must not be a symlink: $field2"
				# The manifest is an external handoff, so its claimed digest is
				# not evidence until the archive bytes are checked again here.
				# Otherwise a forged path/hash row could authorize firmware that
				# did not come from the pinned official factory package.
				factory_bytes=$(factory_pin 7)
				[ -n "$factory_bytes" ] || die "factory recovery pin is missing the pinned byte count"
				actual_factory_bytes=$(file_size "$field2")
				actual_factory_hash=$(hash_file "$field2")
				[ "$actual_factory_bytes" = "$factory_bytes" ] || die 'external firmware factory archive byte count does not match the pinned archive'
				[ "$actual_factory_hash" = "$factory_hash" ] || die 'external firmware factory archive hash does not match the pinned archive'
				continue
				;;
			firmware)
				# Canonical destination-aware row emitted by firmware.sh:
				# firmware controller source-image source-path runtime-path bytes sha256
				controller=$field2
				source_image=$field3
				source_path=$field4
				runtime_path=$field5
				bytes=$field6
				expected=$field7
				firmware_count=$((firmware_count + 1))
				stage_allowlisted "$controller" "$source_image" "$source_path" "$runtime_path" ||
					die "firmware source/controller/path is not in the checked-in stage allowlist: $controller $source_image $source_path $runtime_path"
				case "$source_path" in
					/*) ;;
					*) die "firmware source path must be absolute in the manifest: $source_path" ;;
				esac
				path=$source_path
				;;
			*) die "invalid firmware manifest row kind: $kind" ;;
		esac
		case "$path" in
			*..*|*' '*) die "unsafe firmware path in manifest: $path" ;;
		esac
		case "$runtime_path" in
			''|/*|*..*|*' '*) die "unsafe firmware runtime path in manifest: $runtime_path" ;;
		esac
		allow=$(allowlisted "$runtime_path") || die "firmware runtime path is not in the checked-in allowlist: $runtime_path"
		case "$bytes" in ''|*[!0-9]*) die "invalid firmware byte count for $path" ;; esac
		case "$expected" in ''|*[!0-9a-fA-F]*) die "invalid firmware sha256 for $path" ;; esac
		[ "${#expected}" -eq 64 ] || die "invalid firmware sha256 length for $path"
		file="$firmware/$runtime_path"
		[ -f "$file" ] || die "firmware file is missing: $file"
		[ ! -L "$file" ] || die "firmware file must not be a symlink: $file"
		[ "$(file_size "$file")" = "$bytes" ] || die "firmware byte count mismatch: $runtime_path"
		[ "$(hash_file "$file")" = "$expected" ] || die "firmware hash mismatch: $runtime_path"
	done <"$fmanifest"
	[ "$factory_count" -eq 1 ] || die 'external firmware manifest must contain exactly one pinned factory_archive row'
	[ "$firmware_count" -ge "$stage_count" ] ||
		die "external firmware manifest contains $firmware_count firmware rows, expected at least $stage_count"
	while IFS="$tab" read -r controller source_image source_path runtime_path entry_type extra; do
		case "$controller" in
			''|'#'*) continue ;;
		esac
		case "${entry_type:-file}" in
			file)
				awk -F '\t' -v controller="$controller" -v source_image="$source_image" \
					-v source_path="$source_path" -v runtime_path="$runtime_path" \
					'$1 == "firmware" && $2 == controller && $3 == source_image && $4 == source_path && $5 == runtime_path { found=1 } END { exit(found ? 0 : 1) }' \
					"$fmanifest" || die "external firmware manifest is missing the allowlisted source row: $controller $source_image $source_path $runtime_path"
				;;
			bundle)
				awk -F '\t' -v controller="$controller" -v source_image="$source_image" \
					-v source_path="$source_path" -v runtime_path="$runtime_path" '
					$1 == "firmware" && $2 == controller && $3 == source_image && index($4, source_path "/") == 1 && index($5, runtime_path "/") == 1 {
						source_suffix=substr($4, length(source_path) + 2)
						runtime_suffix=substr($5, length(runtime_path) + 2)
						if (source_suffix != "" && source_suffix == runtime_suffix) { found=1 }
					}
					END { exit(found ? 0 : 1) }
				' "$fmanifest" ||
					die "external firmware manifest is missing entries for the allowlisted bundle: $controller $source_image $source_path $runtime_path"
				;;
			*) die "target firmware stage allowlist has an unknown entry type: $entry_type" ;;
		esac
	done <"$stage_allowlist"
	required_entries=0
	while IFS="$tab" read -r component runtime_path required extra; do
		case "$component" in
			''|'#'*) continue ;;
		esac
		[ "$required" = 1 ] || continue
		required_entries=$((required_entries + 1))
		awk -F '\t' -v wanted="$runtime_path" '
			$1 == "firmware" &&
			($5 == wanted || (index($5, wanted "/") == 1 && wanted !~ /\/$/)) { found = 1 }
			END { exit(found ? 0 : 1) }
		' "$fmanifest" || die "external firmware manifest is missing required allowlisted entry: $runtime_path"
	done <"$package_dir/firmware-allowlist.tsv"
	[ "$required_entries" -gt 0 ] || die 'checked-in firmware allowlist has no required entries'
}

validate_radio_firmware_route() {
	[ -f "$radio_firmware_route" ] || die "radio-first firmware mapping is missing: $radio_firmware_route"
	count=0
	destinations=""
	while IFS="$tab" read -r source_path destination_path extra; do
		case "$source_path" in
			''|'#'*) continue ;;
		esac
		[ -z "${extra:-}" ] || die "radio-first firmware mapping has extra fields: $source_path"
		case "$source_path" in
			/*|*..*|*' '*) die "unsafe radio-first firmware source path: $source_path" ;;
		esac
		case "$destination_path" in
			''|/*|*..*|*' '*) die "unsafe radio-first firmware destination path: $destination_path" ;;
		esac
		[ "$source_path" != "$destination_path" ] || die "radio-first firmware mapping must not copy a path onto itself: $source_path"
		allowlisted "$source_path" >/dev/null || die "radio-first firmware source is not in the canonical allowlist: $source_path"
		awk -F "$tab" -v wanted="$source_path" '$1 == "firmware" && $5 == wanted { found=1 } END { exit(found ? 0 : 1) }' \
			"$FERRIX_PIXEL9A_FIRMWARE_DIR/firmware-manifest.tsv" ||
			die "radio-first firmware source is missing from the verified manifest: $source_path"
		case " $destinations " in
			*" $destination_path "*) die "radio-first firmware mapping has duplicate destination: $destination_path" ;;
		esac
		destinations="$destinations $destination_path"
		count=$((count + 1))
	done <"$radio_firmware_route"
	[ "$count" -gt 0 ] || die 'radio-first firmware mapping is empty'
}

validate_inputs() {
	check_static_inputs
	validate_kernel_dist
	validate_product_dir
	validate_firmware_dir
	validate_radio_firmware_route
	manifest_begin inputs-verified
	manifest_artifact deviceinfo "$package_dir/deviceinfo"
	manifest_artifact kernel-cmdline.conf "$package_dir/kernel-cmdline.conf"
	manifest_artifact modules-initfs "$package_dir/modules-initfs"
	manifest_artifact modules-initfs-radio "$radio_initfs_module_list"
	manifest_artifact boot-layout.toml "$package_dir/boot-layout.toml"
	manifest_artifact firmware-policy.toml "$package_dir/firmware-policy.toml"
	manifest_artifact s5300-channels.toml "$package_dir/s5300-channels.toml"
	manifest_artifact s5300-userspace-sources.toml "$package_dir/s5300-userspace-sources.toml"
	manifest_artifact exynos-modem/APKBUILD "$exynos_package_dir/APKBUILD"
	manifest_artifact modemmanager-samsung-sit/APKBUILD "$modemmanager_package_dir/APKBUILD"
	manifest_artifact pmos/pins.env "$pmaports_pins"
	manifest_artifact pmos/channels.cfg "$channels_cfg"
	manifest_artifact profile.toml "$profile_path"
	manifest_artifact firmware-manifest.tsv "$FERRIX_PIXEL9A_FIRMWARE_DIR/firmware-manifest.tsv"
	manifest_artifact firmware-radio-route.tsv "$radio_firmware_route"
	manifest_finish
	printf '%s\n' 'PASS pixel-9a-rootfs external inputs and hashes'
}

require_linux_build_host() {
	host_os=$(uname -s)
	host_arch=$(uname -m)
	[ "$host_os" = Linux ] && [ "$host_arch" = x86_64 ] ||
		die "rootfs build requires native Linux x86_64; detected $host_os/$host_arch"
	for command_name in git mke2fs pmbootstrap python3 tar; do
		command -v "$command_name" >/dev/null 2>&1 || die "missing native Linux prerequisite '$command_name'"
	done
}

validate_rootfs_image_settings() {
	case "$rootfs_image_size_mib" in
		''|*[!0-9]*) die "FERRIX_PIXEL9A_ROOTFS_IMAGE_SIZE_MIB must be an integer MiB count" ;;
	esac
	[ "$rootfs_image_size_mib" -ge 256 ] ||
		die "FERRIX_PIXEL9A_ROOTFS_IMAGE_SIZE_MIB must be at least 256 MiB"
	case "$rootfs_uuid" in
		[0-9a-fA-F][0-9a-fA-F]*-[0-9a-fA-F][0-9a-fA-F]*-[0-9a-fA-F][0-9a-fA-F]*-[0-9a-fA-F][0-9a-fA-F]*-[0-9a-fA-F][0-9a-fA-F]*) ;;
		*) die "FERRIX_PIXEL9A_ROOTFS_UUID must be a UUID" ;;
	esac
}

rootfs_mkdir() {
	for directory do
		if mkdir -p -- "$directory" 2>/dev/null; then
			continue
		fi
		sudo -n mkdir -p -- "$directory"
	done
}

stage_file() {
	src=$1
	dst=$2
	rootfs_mkdir "$(dirname -- "$dst")"
	if cp "$src" "$dst" 2>/dev/null; then
		return 0
	fi
	sudo -n cp "$src" "$dst"
}

rootfs_chmod() {
	mode=$1
	path=$2
	if chmod "$mode" "$path" 2>/dev/null; then
		return 0
	fi
	sudo -n chmod "$mode" "$path"
}

rootfs_symlink() {
	target=$1
	destination=$2
	if ln -sfn -- "$target" "$destination" 2>/dev/null; then
		return 0
	fi
	sudo -n ln -sfn -- "$target" "$destination"
}

ensure_output_dir() {
	directory=$1
	mkdir -p -- "$directory" 2>/dev/null || sudo -n mkdir -p -- "$directory"
	if [ -w "$directory" ]; then
		return 0
	fi
	# pmbootstrap and its privileged fallback can leave only this exact output
	# directory root-owned. Reclaim the handoff directory before writing the
	# immutable artifacts and manifest; never broaden this to the output root.
	sudo -n chown "$(id -u):$(id -g)" "$directory"
	[ -w "$directory" ] || die "rootfs output directory is not writable: $directory"
}

stage_radio_firmware_aliases() {
	route_manifest="$rootfs_dir/usr/share/ferrix/pixel-9a/radio-firmware-manifest.tsv"
	route_manifest_tmp="$build_dir/radio-firmware-manifest.tsv.tmp.$$"
	{
		printf 'schema_version\t1\n'
		printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
		printf 'route_id\texperimental-linux-google-tegu-s5300-wwan\n'
		printf 'kernel_backend\tlinux-google-tegu-wwan\n'
		printf 'source_policy\tcanonical-factory-manifest-explicit-aliases\n'
	} >"$route_manifest_tmp"
	while IFS="$tab" read -r source_path destination_path extra; do
		case "$source_path" in
			''|'#'*) continue ;;
		esac
		stage_file "$FERRIX_PIXEL9A_FIRMWARE_DIR/$source_path" "$rootfs_dir/$destination_path"
		file="$rootfs_dir/$destination_path"
		printf 'alias\t%s\t%s\t%s\t%s\n' "$source_path" "$destination_path" "$(file_size "$file")" "$(hash_file "$file")" >>"$route_manifest_tmp"
	done <"$radio_firmware_route"
	stage_file "$route_manifest_tmp" "$route_manifest"
	rm -f "$route_manifest_tmp"
}

write_built_manifest() {
	product=${FERRIX_PIXEL9A_PRODUCT_DIR:-"$out_root/pixel-9a/product/tegu"}
	rootfs_artifact="$rootfs_out/rootfs.tar"
	rootfs_image="$rootfs_out/rootfs.img"
	[ -f "$rootfs_artifact" ] || die "rootfs artifact is missing: $rootfs_artifact"
	[ -f "$rootfs_image" ] || die "rootfs image is missing: $rootfs_image"
	[ -f "$rootfs_out/firmware-manifest.tsv" ] ||
		die "rootfs firmware manifest is missing: $rootfs_out/firmware-manifest.tsv"
	[ -f "$rootfs_out/radio-firmware-manifest.tsv" ] ||
		die "rootfs radio firmware manifest is missing: $rootfs_out/radio-firmware-manifest.tsv"
	manifest_begin built-substrate-only
	if [ "$android" = 1 ]; then
		printf 'android_runtime\twaydroid-packages\t%s\n' "$(android_package_list)" >>"$manifest_tmp"
		manifest_artifact android/packages "$android_packages"
	else
		printf 'android_runtime\tnone\n' >>"$manifest_tmp"
	fi
	manifest_input product_manifest "$product/manifest.tsv"
	if [ "$kernel_route" = kleaf-dev ]; then
		manifest_input kleaf_kernel_manifest \
			"$(kernel_manifest_path "${FERRIX_PIXEL9A_KERNEL_DIST:-"$out_root/pixel-9a/kernel/tegu/dist"}")"
	fi
	manifest_artifact deviceinfo "$package_dir/deviceinfo"
	manifest_artifact kernel-cmdline.conf "$package_dir/kernel-cmdline.conf"
	manifest_artifact modules-initfs "$package_dir/modules-initfs"
	manifest_artifact modules-initfs-radio "$radio_initfs_module_list"
	manifest_artifact boot-layout.toml "$package_dir/boot-layout.toml"
	manifest_artifact firmware-policy.toml "$package_dir/firmware-policy.toml"
	manifest_artifact s5300-channels.toml "$package_dir/s5300-channels.toml"
	manifest_artifact s5300-userspace-sources.toml "$package_dir/s5300-userspace-sources.toml"
	manifest_artifact exynos-modem/APKBUILD "$exynos_package_dir/APKBUILD"
	manifest_artifact modemmanager-samsung-sit/APKBUILD "$modemmanager_package_dir/APKBUILD"
	manifest_artifact profile.toml "$profile_path"
	manifest_artifact firmware-manifest.tsv "$rootfs_out/firmware-manifest.tsv"
	manifest_artifact radio-firmware-manifest.tsv "$rootfs_out/radio-firmware-manifest.tsv"
	manifest_artifact firmware-radio-route.tsv "$radio_firmware_route"
	manifest_artifact pmos/channels.cfg "$channels_cfg"
	manifest_artifact rootfs.tar "$rootfs_artifact"
	manifest_artifact rootfs.img "$rootfs_image"
	manifest_finish
}

build_rootfs() {
	preflight >/dev/null
	require_linux_build_host
	[ "${FERRIX_PIXEL9A_ROOTFS_BUILD:-}" = "$build_token" ] || die "rootfs build is opt-in; set FERRIX_PIXEL9A_ROOTFS_BUILD=$build_token"
	[ -n "$rootfs_password" ] || die 'FERRIX_PIXEL9A_ROOTFS_PASSWORD is required; use a one-time non-empty password for the generated guest user'
	validate_kernel_dist
	validate_product_dir
	validate_firmware_dir
	validate_radio_firmware_route
	# The native builder mounts only the target-specific Pixel cache writable.
	# Keep pmbootstrap's temporary workspace inside that mount so builds work
	# with the repository bind mounted read-only.
	mkdir -p "$rootfs_cache_root"
	pmaports=${FERRIX_PIXEL9A_PMAPORTS_DIR:-}
	[ -n "$pmaports" ] || die 'FERRIX_PIXEL9A_PMAPORTS_DIR must point to a pinned pmaports checkout'
	git -C "$pmaports" rev-parse --git-dir >/dev/null 2>&1 || die "pmaports path is not a Git checkout: $pmaports"
	pmaports_commit=${FERRIX_PIXEL9A_PMAPORTS_COMMIT:-$(pinned_pmaports_commit)}
	[ -n "$pmaports_commit" ] || die 'could not resolve the pinned pmaports commit'
	case "$pmaports_commit" in
		''|*[!0-9a-fA-F]*) die 'pmaports revision must be a hexadecimal Git revision' ;;
	esac
	[ "${#pmaports_commit}" -eq 40 ] || die 'pmaports revision must contain 40 hexadecimal characters'
	actual=$(git -C "$pmaports" rev-parse HEAD) || die 'pmaports checkout is unreadable'
	[ "$actual" = "$pmaports_commit" ] || die "pmaports revision mismatch: expected $pmaports_commit, found $actual"
	git -C "$pmaports" diff --quiet --ignore-submodules -- || die 'pmaports checkout has unstaged changes'
	git -C "$pmaports" diff --cached --quiet --ignore-submodules -- || die 'pmaports checkout has staged changes'
	# A clean tracked diff is insufficient for a source archive: untracked
	# files can still be consumed by pmbootstrap and change the resulting
	# package set. Reject them before archiving the checkout.
	if git -C "$pmaports" status --porcelain --untracked-files=all | grep -q .; then
		die 'pmaports checkout has untracked changes'
	fi
	[ -d "$pmaports/.git" ] || die 'pmaports checkout must have a directory Git metadata root'
	pmaports_remote=$(git -C "$pmaports" remote get-url origin 2>/dev/null || true)
	case "$pmaports_remote" in
		https://gitlab.postmarketos.org/postmarketOS/pmaports.git|git@gitlab.postmarketos.org:postmarketOS/pmaports.git) ;;
		*) die "pmaports origin is not the canonical postmarketOS repository: $pmaports_remote" ;;
	esac

	# A failed privileged pmbootstrap run can leave root-owned chroot files in
	# the target cache. Reuse its own config to detach those mounts before
	# removing only the generated rootfs.* directories. The native builder
	# grants this build user passwordless sudo because pmbootstrap creates
	# root-owned chroot content; keep the fallback limited to this exact
	# generated directory.
	remove_generated_dir() {
		target_dir=$1
		rm -rf -- "$target_dir" 2>/dev/null || true
		[ ! -e "$target_dir" ] && return 0
		if command -v sudo >/dev/null 2>&1; then
			sudo -n rm -rf -- "$target_dir" 2>/dev/null || true
		fi
		[ ! -e "$target_dir" ]
	}

	for stale_dir in "$rootfs_cache_root"/rootfs.*; do
		[ -d "$stale_dir" ] || continue
		stale_config="$stale_dir/pmbootstrap.cfg"
		if [ -f "$stale_config" ]; then
			PMB_CHANNELS_CFG="$channels_cfg" pmbootstrap --config "$stale_config" -y shutdown >/dev/null 2>&1 || true
		fi
		remove_generated_dir "$stale_dir" || die "could not remove stale pmbootstrap build directory: $stale_dir"
	done

	build_dir=$(mktemp -d "$rootfs_cache_root/rootfs.XXXXXX")
	rootfs_image_tmp=''
	config=''
	cleanup() {
		# pmbootstrap owns root-created chroot files and mount points. Detach
		# them before the container removes this temporary build directory;
		# otherwise a failed package build can strand a privileged cache tree.
		if [ -n "$config" ]; then
			PMB_CHANNELS_CFG="$channels_cfg" pmbootstrap --config "$config" -y shutdown >/dev/null 2>&1 || true
		fi
		remove_generated_dir "$build_dir" ||
			printf '%s\n' "WARN pixel-9a-rootfs: could not remove temporary pmbootstrap build directory: $build_dir" >&2
		[ -z "$rootfs_image_tmp" ] || rm -f "$rootfs_image_tmp"
	}
	trap cleanup EXIT HUP INT TERM
	validate_rootfs_image_settings
	aports="$build_dir/aports"
	mkdir -p "$aports"
	pmaports_archive="$build_dir/pmaports.tar"
	git -C "$pmaports" archive HEAD >"$pmaports_archive" || die 'could not archive the pinned pmaports checkout'
	tar -xf "$pmaports_archive" -C "$aports"
	# pmbootstrap identifies the staged tree as pmaports through its canonical
	# Git remote. Keep the deterministic archive as the working tree, but carry
	# over the already-verified metadata so the package manager can resolve that
	# identity without treating the cache checkout itself as mutable input.
	cp -R "$pmaports/.git" "$aports/.git"
	git -C "$aports" rev-parse HEAD >/dev/null 2>&1 || die 'staged pmaports metadata is not a valid Git repository'
	mkdir -p "$aports/device/testing"
	cp -R "$package_dir" "$aports/device/testing/device-google-tegu"
	staged_device_package="$aports/device/testing/device-google-tegu"
	# The checked-in package keeps Google's Kleaf module list for the comparison
	# route. The selected radio-first candidate has a different ABI and builds
	# its radio/storage paths in-kernel, so overlay the explicit route-local list
	# before pmbootstrap consumes the device package. Its APKBUILD checksum must
	# follow that deliberate route selection rather than the comparison list.
	cp "$radio_initfs_module_list" "$staged_device_package/modules-initfs"
	radio_initfs_module_sha512=$(sha512_file "$radio_initfs_module_list")
	staged_apkbuild_tmp="$staged_device_package/APKBUILD.tmp.$$"
	if ! awk -v radio_hash="$radio_initfs_module_sha512" \
		'$2 == "modules-initfs" { sub(/^[^[:space:]]+/, radio_hash); found=1 } { print } END { if (!found) exit 1 }' \
		"$staged_device_package/APKBUILD" >"$staged_apkbuild_tmp"; then
		rm -f "$staged_apkbuild_tmp"
		die 'staged device package APKBUILD is missing the modules-initfs checksum'
	fi
	mv "$staged_apkbuild_tmp" "$staged_device_package/APKBUILD"
	# These are target-local external package recipes. Keep them alongside the
	# copied device package so pmbootstrap cannot silently resolve generic
	# ModemManager or an unreviewed modem helper from the host checkout.
	rm -rf "$aports/device/testing/exynos-modem" "$aports/device/testing/modemmanager-samsung-sit"
	cp -R "$exynos_package_dir" "$aports/device/testing/exynos-modem"
	cp -R "$modemmanager_package_dir" "$aports/device/testing/modemmanager-samsung-sit"
	pmb_work="$build_dir/pmbootstrap"
	config="$build_dir/pmbootstrap.cfg"
	sed -e "s#^work = .*#work = $pmb_work#" -e "s#^aports = .*#aports = $aports#" "$pmos_dir/pmbootstrap.cfg" >"$config"
	if [ "$android" = 1 ]; then
		android_list=$(android_package_list)
		config_tmp="$config.tmp.$$"
		awk -v extra="$android_list" '/^extra_packages = / { $0 = $0 "," extra; found=1 } { print } END { exit(found ? 0 : 1) }' \
			"$config" >"$config_tmp" || die 'target pmbootstrap.cfg has no extra_packages line for the Android package set'
		mv "$config_tmp" "$config"
		printf '%s\n' "opt-in Android runtime packages: $android_list (audio backend: pipewire)"
	fi
	printf '%s\n' "running pmbootstrap rootfs build with config $config"
	# The target package set includes two local, source-pinned aports. Keep
	# local package installation enabled so pmbootstrap builds and installs
	# those recipes from the staged aports tree.
	# pmbootstrap init's v3.11.1 systemd warning assumes every device package
	# carries a Linux subpackage. This target deliberately keeps its external
	# Google/WWAN kernel outside pmbootstrap, so initialize only the work-folder
	# state that init would create and run install from the fully checked-in
	# config. This is the same noninteractive config boundary used by the QEMU
	# postmarketOS workflow.
	pmbootstrap_work_version=$(python3 -c 'import pmb.config; print(pmb.config.work_version)')
	mkdir -p "$pmb_work/cache_git"
	chmod 0700 "$pmb_work"
	[ -f "$pmb_work/version" ] || printf '%s\n' "$pmbootstrap_work_version" >"$pmb_work/version"
	printf '%s\n' 'running pmbootstrap rootfs install with external-kernel initramfs boundary'
	FERRIX_PMB_EXTERNAL_KERNEL=1 PMB_CHANNELS_CFG="$channels_cfg" python3 \
		"$root/tools/pixel-9a/pmbootstrap-rootfs.py" --details-to-stdout --config "$config" -y install \
		--password "$rootfs_password" --no-image --no-fde --no-sshd --no-firewall --no-recommends
	# pmbootstrap leaves bind mounts for /dev, /proc, and /sys in the chroot
	# after install. Detach its generated mounts before staging or archiving so
	# the guest image contains only the rootfs tree, never live host pseudo-FS.
	PMB_CHANNELS_CFG="$channels_cfg" pmbootstrap --config "$config" -y shutdown >/dev/null 2>&1 ||
		die 'pmbootstrap could not detach the generated rootfs mounts'
	# pmbootstrap names this directory from the target's deviceinfo. Prefer a
	# shell glob here: some minimal native builder images provide a find
	# implementation that does not reliably traverse the root-owned chroot
	# after pmbootstrap returns. Keep the explicit postcondition and print the
	# workspace contents when it fails so a substrate problem is diagnosable.
	rootfs_dir=''
	for candidate in "$pmb_work"/chroot_rootfs_*; do
		[ -d "$candidate" ] || continue
		rootfs_dir=$candidate
		break
	done
	if [ -z "$rootfs_dir" ]; then
		printf '%s\n' "pmbootstrap workspace contents under $pmb_work:" >&2
		find "$pmb_work" -maxdepth 2 -print >&2 2>/dev/null || true
		die "pmbootstrap did not produce a rootfs chroot under $pmb_work"
	fi
	verify_audio_backend_rootfs "$rootfs_dir/lib/apk/db/installed"
	[ "$android" != 1 ] || verify_android_rootfs "$rootfs_dir/lib/apk/db/installed"
	product=${FERRIX_PIXEL9A_PRODUCT_DIR:-"$out_root/pixel-9a/product/tegu"}
	for name in ferrix-core ferrix-shell ferrix-basic-screen ferrix-appd ferrix-sandbox ferrix-terminal \
		ferrix-telephonyd ferrix-networkd ferrix-contactsd ferrix-phone ferrix-messages ferrix-contacts ferrix-settings ferrix-files; do
		stage_file "$product/$name" "$rootfs_dir/usr/bin/$name"
		rootfs_chmod 0755 "$rootfs_dir/usr/bin/$name"
	done
	stage_file "$profile_path" "$rootfs_dir/etc/ferrix/profile.toml"
	stage_file "$product/colors.toml" "$rootfs_dir/etc/ferrix/colors.toml"
	rootfs_mkdir "$rootfs_dir/usr/share/ferrix/apps"
	for descriptor in "$product"/apps/*.json; do
		stage_file "$descriptor" "$rootfs_dir/usr/share/ferrix/apps/$(basename -- "$descriptor")"
	done
	if [ -d "$product/contacts" ]; then
		for fixture in "$product"/contacts/*; do
			[ -f "$fixture" ] || continue
			stage_file "$fixture" "$rootfs_dir/usr/share/ferrix/contacts/$(basename -- "$fixture")"
		done
	fi
	stage_file "$package_dir/ferrix-backends.env" "$rootfs_dir/etc/ferrix/backends.env"
	stage_file "$package_dir/s5300-channels.toml" "$rootfs_dir/usr/share/ferrix/pixel-9a/s5300-channels.toml"
	stage_file "$package_dir/s5300-userspace-sources.toml" "$rootfs_dir/usr/share/ferrix/pixel-9a/s5300-userspace-sources.toml"
	rootfs_mkdir "$rootfs_dir/var/lib/ferrix/telephony" "$rootfs_dir/var/lib/ferrix/contacts"
	while IFS="$tab" read -r kind field2 field3 field4 field5 field6 field7; do
		case "$kind" in
			firmware)
				if [ -n "${field7:-}" ]; then
					runtime_path=$field5
				else
					runtime_path=$field2
				fi
				stage_file "$FERRIX_PIXEL9A_FIRMWARE_DIR/$runtime_path" "$rootfs_dir/$runtime_path"
				;;
		esac
		done <"$FERRIX_PIXEL9A_FIRMWARE_DIR/firmware-manifest.tsv"
	stage_radio_firmware_aliases
	# Carry the validated destination-aware handoff into the guest so the
	# physical radio inventory can re-hash every staged file instead of treating
	# directory presence as firmware evidence.
	stage_file "$FERRIX_PIXEL9A_FIRMWARE_DIR/firmware-manifest.tsv" \
		"$rootfs_dir/usr/share/ferrix/pixel-9a/firmware-manifest.tsv"
	# The Google BCM4383 module has build variants that request the same
	# allow-listed files through Android-era absolute paths. These symlinks are
	# only filesystem compatibility; no Android framework or userspace is used.
	rootfs_mkdir "$rootfs_dir/vendor" "$rootfs_dir/etc"
	rootfs_symlink /lib/firmware "$rootfs_dir/vendor/firmware"
	rootfs_symlink /lib/firmware "$rootfs_dir/etc/wifi"

	ensure_output_dir "$rootfs_out"
	rootfs_artifact="$rootfs_out/rootfs.tar"
	# Keep temporary files in the user-owned build directory. The final output
	# directory may have been touched by a privileged pmbootstrap fallback.
	tmp_tar="$build_dir/rootfs.tar.tmp.$$"
	if ! tar --sort=name --mtime='UTC 1970-01-01' --owner=0 --group=0 --numeric-owner -C "$rootfs_dir" -cf "$tmp_tar" .; then
		sudo -n rm -f -- "$tmp_tar"
		sudo -n tar --sort=name --mtime='UTC 1970-01-01' --owner=0 --group=0 --numeric-owner \
			-C "$rootfs_dir" -cf "$tmp_tar" .
		sudo -n chown "$(id -u):$(id -g)" "$tmp_tar"
	fi
	mv "$tmp_tar" "$rootfs_artifact"
	rootfs_image="$rootfs_out/rootfs.img"
	rootfs_image_tmp="$build_dir/rootfs.img.tmp.$$"
	printf '%s\n' "creating ext4 rootfs image (${rootfs_image_size_mib} MiB) from $rootfs_dir"
	if ! mke2fs -q -t ext4 -F -L ferrix-tegu -U "$rootfs_uuid" -d "$rootfs_dir" \
		"$rootfs_image_tmp" "${rootfs_image_size_mib}M"; then
		sudo -n rm -f -- "$rootfs_image_tmp"
		sudo -n mke2fs -q -t ext4 -F -L ferrix-tegu -U "$rootfs_uuid" -d "$rootfs_dir" \
			"$rootfs_image_tmp" "${rootfs_image_size_mib}M"
		sudo -n chown "$(id -u):$(id -g)" "$rootfs_image_tmp"
	fi
	mv "$rootfs_image_tmp" "$rootfs_image"
	rootfs_image_tmp=''
	cp "$FERRIX_PIXEL9A_FIRMWARE_DIR/firmware-manifest.tsv" "$rootfs_out/firmware-manifest.tsv"
	cp "$rootfs_dir/usr/share/ferrix/pixel-9a/radio-firmware-manifest.tsv" "$rootfs_out/radio-firmware-manifest.tsv"
	write_built_manifest
	printf '%s\n' 'PASS pixel-9a-rootfs substrate built; bootable=false until split artifacts are validated'
}

finalize_rootfs_manifest() {
	validate_inputs >/dev/null
	write_built_manifest
	printf '%s\n' 'PASS pixel-9a-rootfs built manifest refreshed from existing artifacts'
}

usage() {
	printf '%s\n' 'usage: tools/pixel-9a/rootfs.sh preflight|validate|build|finalize' >&2
	exit 2
}

case "${1:-}" in
	preflight) [ "$#" -eq 1 ] || usage; preflight ;;
	validate) [ "$#" -eq 1 ] || usage; validate_inputs ;;
	build) [ "$#" -eq 1 ] || usage; build_rootfs ;;
	finalize) [ "$#" -eq 1 ] || usage; finalize_rootfs_manifest ;;
	*) usage ;;
esac
