#!/bin/sh
# Build the postmarketOS guest for generic AArch64 QEMU `virt` with pmbootstrap
# inside the pinned guest-builder container (QEMU/mock evidence only; no
# physical device is involved). Outputs land in
# ${FERRIX_OUT_DIR:-out}/qemu-aarch64/pmos/ with a manifest.json.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64/pmos
product=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64/product
cache=${FERRIX_CACHE_DIR:-"$root/.cache/ferrix"}
pmos_dir=$root/targets/qemu-aarch64/pmos
builder_dir=$root/containers/guest-builder
. "$pmos_dir/pins.env"
tag=${FERRIX_GUEST_BUILDER_TAG:-"ferrix-guest-builder:pmbootstrap-$PMBOOTSTRAP_TAG"}

command -v docker >/dev/null || { echo "missing docker; start Docker Desktop (see just host-check)" >&2; exit 2; }
docker info >/dev/null 2>&1 || { echo "docker daemon is not running; start Docker Desktop" >&2; exit 2; }
"$root/targets/qemu-aarch64/scripts/qemu-build.sh"
for file in mobile-core mobile-shell mobile-basic-screen profile.toml; do
  [ -f "$product/$file" ] || { echo "missing product artifact $product/$file" >&2; exit 1; }
done
mkdir -p "$out"
# pmbootstrap's work directory holds chroots and needs real POSIX semantics
# (ownership, setuid bits, device nodes). Docker Desktop's virtiofs bind
# mounts from macOS provide none of them (verified: chown stays uid 0, 4755
# becomes 755, mknod is EPERM), so the default is a Docker volume inside the
# Docker VM. Linux hosts can bind the cache directory instead.
if [ "${FERRIX_PMOS_WORK_BIND:-0}" = 1 ]; then
  work_mount="$cache/pmbootstrap"
  mkdir -p "$work_mount"
else
  work_mount=${FERRIX_PMOS_WORK_VOLUME:-ferrix-pmbootstrap-work}
  docker volume create "$work_mount" >/dev/null
fi
printf 'resolved output: %s\n' "$out"
printf 'pmbootstrap work: %s\n' "$work_mount"
printf 'guest-builder: %s (base %s, pmbootstrap %s, pmaports %s@%s)\n' "$tag" "$GUEST_BUILDER_BASE" "$PMBOOTSTRAP_TAG" "$PMAPORTS_BRANCH" "$PMAPORTS_COMMIT"

if ! docker build --platform linux/arm64 \
    --build-arg "GUEST_BUILDER_BASE=$GUEST_BUILDER_BASE" \
    --build-arg "PMBOOTSTRAP_REPO=$PMBOOTSTRAP_REPO" \
    --build-arg "PMBOOTSTRAP_TAG=$PMBOOTSTRAP_TAG" \
    --build-arg "PMBOOTSTRAP_COMMIT=$PMBOOTSTRAP_COMMIT" \
    -t "$tag" "$builder_dir" >"$out/builder-image.log" 2>&1; then
  cat "$out/builder-image.log" >&2
  echo "guest-builder image build failed (log: $out/builder-image.log)" >&2
  exit 1
fi
image_id=$(docker image inspect --format '{{.Id}}' "$tag")
project_rev=$(git -C "$root" rev-parse HEAD 2>/dev/null || echo unknown)
if [ -n "$(git -C "$root" status --porcelain 2>/dev/null)" ]; then
  project_rev="$project_rev-dirty"
fi
printf 'guest-builder image: %s\n' "$image_id"

# --privileged is the single flag pmbootstrap needs (mount, chroot, loop
# devices and their dynamically created partition nodes); see the Dockerfile.
# Mounts: pmbootstrap work (rw), project pmOS inputs (ro), output dir (rw).
status_file=$(mktemp "$out/.build-status.XXXXXX")
{
  status=0
  docker run --rm --privileged --platform linux/arm64 \
    -v "$work_mount":/cache/pmbootstrap \
    -v "$pmos_dir":/project:ro \
    -v "$product":/product:ro \
    -v "$out":/out \
    -e "FERRIX_BUILDER_IMAGE_ID=$image_id" \
    -e "FERRIX_BUILDER_IMAGE_TAG=$tag" \
    -e "FERRIX_PROJECT_REV=$project_rev" \
    -e "FERRIX_PMOS_PASSWORD=${FERRIX_PMOS_PASSWORD:-147147}" \
    -e "FERRIX_PMOS_KEEP_CHROOTS=${FERRIX_PMOS_KEEP_CHROOTS:-0}" \
    -e "FERRIX_PMOS_WORK_MOUNT=$work_mount" \
    "$tag" sh /project/build-in-container.sh 2>&1 || status=$?
  echo "$status" > "$status_file"
} | tee "$out/build.log"
status=$(cat "$status_file")
rm -f "$status_file"
case "$status" in ''|*[!0-9]*) status=1 ;; esac
[ "$status" -eq 0 ] || { echo "pmos-build failed (exit $status); log: $out/build.log" >&2; exit "$status"; }
[ -f "$out/manifest.json" ] || { echo "pmos-build finished without manifest.json" >&2; exit 1; }
printf 'built: %s/vmlinuz, %s/initramfs, %s/qemu-aarch64.img (manifest: %s/manifest.json)\n' "$out" "$out" "$out" "$out"
