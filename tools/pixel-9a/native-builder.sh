#!/bin/sh
set -eu

# Run only device-free Pixel 9a build workflows in the checked-in native
# linux/amd64 builder.  Physical discovery, radios runtime SSH, ADB, Fastboot,
# flashing, and temporary boot are intentionally not accepted here.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
image=${FERRIX_PIXEL9A_BUILDER_IMAGE:-ferrix-pixel9a-builder:rust-1.98.1}
builder_dir="$root/containers/pixel-9a-builder"

die() {
	printf 'FAIL pixel-9a-native-builder: %s\n' "$*" >&2
	exit 2
}

safe_image() {
	case "$image" in
		''|*[!A-Za-z0-9_.:/-]*) die 'FERRIX_PIXEL9A_BUILDER_IMAGE contains unsafe characters' ;;
	esac
}

check_docker() {
	command -v docker >/dev/null 2>&1 || die 'Docker-compatible CLI is missing'
	docker_platform=$(docker info --format '{{.OSType}}/{{.Architecture}}' 2>/dev/null || true)
	case "$docker_platform" in
		linux/amd64|linux/x86_64) ;;
		*) die "native Linux x86_64 Docker engine required; detected $docker_platform" ;;
	esac
}

build_image() {
	check_docker
	safe_image
	docker build --platform linux/amd64 --tag "$image" --file "$builder_dir/Dockerfile" "$builder_dir"
	docker image inspect --format 'pixel-9a-builder={{.Id}} {{.Os}}/{{.Architecture}}' "$image"
}

map_workspace_path() {
	value=$1
	case "$value" in
		"$root"/*) printf '/workspace/%s\n' "${value#"$root"/}" ;;
		/*) die "external path must be under the checkout for this wrapper: $value" ;;
		*) printf '%s\n' "$value" ;;
	esac
}

run_workflow() {
	check_docker
	safe_image
	workflow=$1
	verb=$2
	shift 2

	case "$workflow:$verb" in
		env:check|kernel-radio:preflight|kernel-radio:sync|kernel-radio:verify|kernel-radio:build|product:preflight|product:build|firmware:preflight|firmware:inventory|firmware:inspect|firmware:stage|rootfs:preflight|rootfs:validate|rootfs:build|rootfs:finalize|boot-radio:preflight|boot-radio:build|radio-split:preflight|radio-split:build|artifacts:check|artifacts:manifest) ;;
		*) die "unsupported or device-facing workflow: $workflow $verb" ;;
	esac

	case "$workflow" in
		env) script=tools/pixel-9a/env.sh; set -- "$script" "$verb" "$@" ;;
		kernel-radio) script=tools/pixel-9a/kernel-radio.sh; set -- "$script" "$verb" "$@" ;;
		product) script=tools/pixel-9a/product.sh; set -- "$script" "$verb" "$@" ;;
		firmware) script=tools/pixel-9a/firmware.sh; set -- "$script" "$verb" "$@" ;;
		rootfs) script=tools/pixel-9a/rootfs.sh; set -- "$script" "$verb" "$@" ;;
		boot-radio) script=tools/pixel-9a/boot-radio.sh; set -- "$script" "$verb" "$@" ;;
		radio-split) script=tools/pixel-9a/radio-split.sh; set -- "$script" "$verb" "$@" ;;
		artifacts) script=tools/pixel-9a/artifacts.sh; set -- "$script" "$verb" "$@" ;;
	esac

	mkdir -p "$root/.cache/ferrix/pixel-9a" "$root/out/pixel-9a"
	env_file=$(mktemp "${TMPDIR:-/tmp}/pixel-9a-builder-env.XXXXXX")
	trap 'rm -f "$env_file"' EXIT HUP INT TERM
	{
		printf '%s\n' \
			'FERRIX_PIXEL9A_REPO_ROOT=/workspace' \
			'FERRIX_CACHE_DIR=/workspace/.cache/ferrix' \
			'FERRIX_OUT_DIR=/workspace/out'
	} >"$env_file"

	# Pass only build-stage controls and input paths.  No host-wide env-file,
	# USB path, ADB socket, or arbitrary command is forwarded into the image.
	for name in \
		FERRIX_PIXEL9A_RADIO_SYNC FERRIX_PIXEL9A_RADIO_BUILD \
		FERRIX_PIXEL9A_PRODUCT_BUILD FERRIX_PIXEL9A_FIRMWARE \
		FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST FERRIX_PIXEL9A_FIRMWARE_DIR \
		FERRIX_PIXEL9A_ROOTFS_BUILD FERRIX_PIXEL9A_ROOTFS_PASSWORD \
		FERRIX_PIXEL9A_ROOTFS_CACHE_DIR \
		FERRIX_PIXEL9A_RADIO_BOOT_BUILD FERRIX_PIXEL9A_RADIO_SPLIT_BUILD \
		FERRIX_PIXEL9A_AVB_ALGORITHM FERRIX_PIXEL9A_AVB_KEY \
		FERRIX_PIXEL9A_PMAPORTS_DIR FERRIX_PIXEL9A_PMAPORTS_COMMIT \
		FERRIX_PIXEL9A_FACTORY_ARCHIVE FERRIX_PIXEL9A_RADIO_PARTITION_SIZES \
		FERRIX_PIXEL9A_VENDOR_KERNEL_BOOT_CPIO FERRIX_PIXEL9A_VENDOR_DLKM_ROOT \
		FERRIX_PIXEL9A_SYSTEM_DLKM_ROOT FERRIX_PIXEL9A_INITRAMFS \
		FERRIX_PIXEL9A_VENDOR_RAMDISK FERRIX_PIXEL9A_VENDOR_BOOTCONFIG; do
		value=$(printenv "$name" 2>/dev/null || true)
		[ -n "$value" ] || continue
		case "$name" in
			*_DIR|*_ARCHIVE|*_SIZES|*_CPIO|*_ROOT|*_INITRAMFS|*_RAMDISK|*_BOOTCONFIG|FERRIX_PIXEL9A_AVB_KEY|FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST)
				value=$(map_workspace_path "$value") ;;
		esac
		case "$value" in
			*'\n'*) die "$name contains a newline" ;;
		esac
		printf '%s=%s\n' "$name" "$value" >>"$env_file"
	done

	if [ "$workflow:$verb" = rootfs:build ]; then
		printf '%s\n' 'FERRIX_PIXEL9A_PRIVILEGED=1' >>"$env_file"
		exec docker run --rm --platform linux/amd64 --init --privileged \
			--mount "type=bind,src=$root,dst=/workspace,readonly" \
			--mount "type=bind,src=$root/.cache/ferrix/pixel-9a,dst=/workspace/.cache/ferrix/pixel-9a" \
			--mount "type=bind,src=$root/out/pixel-9a,dst=/workspace/out/pixel-9a" \
			--env-file "$env_file" "$image" sh -c 'cd /workspace && exec "$@"' -- "$@"
	fi
	exec docker run --rm --platform linux/amd64 --init \
		--mount "type=bind,src=$root,dst=/workspace,readonly" \
		--mount "type=bind,src=$root/.cache/ferrix/pixel-9a,dst=/workspace/.cache/ferrix/pixel-9a" \
		--mount "type=bind,src=$root/out/pixel-9a,dst=/workspace/out/pixel-9a" \
		--env-file "$env_file" "$image" sh -c 'cd /workspace && exec "$@"' -- "$@"
}

usage() {
	cat >&2 <<'EOF'
usage:
  tools/pixel-9a/native-builder.sh image
  tools/pixel-9a/native-builder.sh run <workflow> <verb>

Allowed workflows are device-free env, kernel-radio, product, firmware,
rootfs, boot-radio, radio-split, and artifacts stages.  A native Linux
x86_64 Docker engine is mandatory; ADB/Fastboot and USB are never mounted.
EOF
	exit 2
}

case "${1:-}" in
	image) [ "$#" -eq 1 ] || usage; build_image ;;
	run) [ "$#" -ge 3 ] || usage; shift; run_workflow "$@" ;;
	*) usage ;;
esac
