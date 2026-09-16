#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${OMARCHY_OUT_DIR:-"$root/out"}/qemu-aarch64
image=alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc
container="omarchy-initramfs-$$"

command -v docker >/dev/null || { echo "missing docker" >&2; exit 2; }
command -v cpio >/dev/null || { echo "missing cpio" >&2; exit 2; }
mkdir -p "$out"
stage=$(mktemp -d "$out/initramfs-root.XXXXXX")
printf 'resolved output: %s\n' "$out"
printf 'builder image: %s\n' "$image"
docker pull --platform linux/arm64 "$image" >/dev/null
docker create --platform linux/arm64 --name "$container" "$image" sh -c 'sleep 600' >/dev/null
docker start "$container" >/dev/null
docker exec "$container" sh -ec 'apk add --no-cache linux-virt weston weston-backend-drm weston-shell-desktop weston-terminal seatd mesa-dri-gallium >/dev/null'
docker export "$container" | tar -xf - -C "$stage"
docker stop "$container" >/dev/null
docker rm "$container" >/dev/null
cp "$stage/boot/vmlinuz-virt" "$out/Image"
cp "$root/targets/qemu-aarch64/initramfs/init" "$stage/init"
cp "$root/targets/qemu-aarch64/initramfs/mobile-layout" "$stage/usr/bin/mobile-layout"
cp "${OMARCHY_QEMU_PROFILE:-$root/targets/qemu-aarch64/profile.toml}" "$stage/profile.toml"
chmod 0755 "$stage/init" "$stage/usr/bin/mobile-layout"
(cd "$stage" && find . -print | cpio -o -H newc 2>/dev/null | gzip -9) > "$out/initramfs.cpio.gz"
image_hash=$(shasum -a 256 "$out/Image" | awk '{print $1}')
initramfs_hash=$(shasum -a 256 "$out/initramfs.cpio.gz" | awk '{print $1}')
printf '{"evidence":"QEMU","builder_image":"%s","kernel":"Alpine linux-virt aarch64","image_sha256":"%s","initramfs_sha256":"%s","profile":"%s"}\n' "$image" "$image_hash" "$initramfs_hash" "${OMARCHY_QEMU_PROFILE:-targets/qemu-aarch64/profile.toml}" > "$out/manifest.json"
printf 'built: %s/Image and %s/initramfs.cpio.gz\n' "$out" "$out"
