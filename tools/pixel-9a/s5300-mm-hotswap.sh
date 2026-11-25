#!/bin/sh
# Build the checked-in modemmanager-samsung-sit recipe natively for the Pixel 9a
# userspace (Alpine 3.24 aarch64) and, optionally, run the mock S3 emulation.
#
#   sh tools/pixel-9a/s5300-mm-hotswap.sh build   # apks + files -> out/s3-device/build/
#   sh tools/pixel-9a/s5300-mm-hotswap.sh sim     # mock run of those apks -> .../build/sim/
#
# Opt-in host tool: it needs Docker with linux/arm64 (native on Apple Silicon,
# emulated elsewhere) and network access for the pinned source archives and
# Alpine packages. It never contacts a device. `build` runs abuild against the
# recipe as checked in, so every patch is sha512-verified. `sim` overmounts
# /sys inside a privileged throwaway container only, and its sit-proxy
# stand-ins never answer, so it emulates no SIT protocol. Its output is mock
# evidence, never device evidence; see s5300-s3-mm-binding.md.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
board="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
scripts="$root/tools/pixel-9a/s5300-mm-hotswap"
out="$root/out/s3-device/build"
image=alpine:3.24

command -v docker >/dev/null 2>&1 || { echo 's5300-mm-hotswap: docker is required' >&2; exit 2; }
case "${1:-}" in
	build)
		mkdir -p "$out"
		docker run --rm --platform linux/arm64 \
			-v "$board/pmos:/pkg:ro" -v "$scripts:/in:ro" -v "$out:/out" \
			"$image" sh /in/abuild.sh
		;;
	sim)
		[ -f "$out/apk/modemmanager-samsung-sit-$(sed -n 's/^pkgver=//p' "$board/pmos/modemmanager-samsung-sit/APKBUILD")-r$(sed -n 's/^pkgrel=//p' "$board/pmos/modemmanager-samsung-sit/APKBUILD").apk" ] ||
			{ echo 's5300-mm-hotswap: run "build" first (no apk for the current pkgrel)' >&2; exit 2; }
		mkdir -p "$out/sim"
		docker run --rm --privileged --platform linux/arm64 -e SIM_SECONDS="${SIM_SECONDS:-150}" \
			-v "$out/apk:/apk:ro" -v "$scripts:/in:ro" -v "$out/sim:/out" \
			"$image" sh /in/sim.sh
		;;
	*)
		echo 'usage: s5300-mm-hotswap.sh build|sim' >&2
		exit 2
		;;
esac
