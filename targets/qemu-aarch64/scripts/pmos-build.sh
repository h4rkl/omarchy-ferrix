#!/bin/sh
# Build the postmarketOS guest for generic AArch64 QEMU `virt` with pmbootstrap
# inside the pinned guest-builder container (QEMU/mock evidence only; no
# physical device is involved). Outputs land in
# ${FERRIX_OUT_DIR:-out}/qemu-aarch64/pmos/ with a manifest.json.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64/pmos
product=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64/product
# OPT-IN Android compatibility runtime (Waydroid). FERRIX_PMOS_ANDROID=1
# builds a separate image under .../pmos-android/ from
# pmbootstrap-android.cfg with the pinned container images fetched by
# `just waydroid-images-fetch`; the default build, pmos-check, and CI are
# unchanged. QEMU evidence only; claim level none until demonstrated.
android=${FERRIX_PMOS_ANDROID:-0}
case "$android" in 0|1) ;; *) echo "invalid FERRIX_PMOS_ANDROID: $android (0|1)" >&2; exit 2 ;; esac
android_mounts=""
if [ "$android" = 1 ]; then
  out="$out-android"
  images_dir=${FERRIX_WAYDROID_IMAGES:-"${FERRIX_OUT_DIR:-$root/out}/android/waydroid/current"}
  images_dir=$(CDPATH= cd -P -- "$images_dir" 2>/dev/null && pwd) || { echo "missing Waydroid images at $images_dir; run: just waydroid-images-fetch" >&2; exit 2; }
  for f in system.img vendor.img images.json; do
    [ -f "$images_dir/$f" ] || { echo "missing $images_dir/$f; run: just waydroid-images-fetch" >&2; exit 2; }
  done
  sh "$root/tools/android/waydroid-images-fetch.sh" --verify-only >/dev/null || { echo "Waydroid images failed verification; re-run: just waydroid-images-fetch" >&2; exit 2; }
  android_mounts="-v $images_dir:/android/images:ro"
  # Fulguris is part of the Android product image: refuse to build that
  # variant unless the pinned F-Droid APK exists and passes verification.
  fulguris_pins="$root/tools/android/fulguris.pins"
  fulguris_version=$(sed -n 's/^FULGURIS_VERSION=//p' "$fulguris_pins" | head -1)
  fulguris_asset=$(sed -n 's/^FULGURIS_ASSET=//p' "$fulguris_pins" | head -1)
  fulguris_apk=${FERRIX_ANDROID_FULGURIS_APK:-"${FERRIX_OUT_DIR:-$root/out}/android/fulguris/$fulguris_version/$fulguris_asset"}
  [ -f "$fulguris_apk" ] || {
    echo "missing pinned Fulguris APK at $fulguris_apk; run: sh tools/android/fulguris-fetch.sh" >&2
    exit 2
  }
  sh "$root/tools/android/fulguris-verify.sh" "$fulguris_apk" --pins "$fulguris_pins" >/dev/null \
    || { echo "pinned Fulguris APK failed verification: $fulguris_apk" >&2; exit 2; }
  android_mounts="$android_mounts -v $fulguris_apk:/android/fulguris.apk:ro"
  printf 'fulguris apk: %s (verified)\n' "$fulguris_apk"
fi
cache=${FERRIX_CACHE_DIR:-"$root/.cache/ferrix"}
pmos_dir=$root/targets/qemu-aarch64/pmos
builder_dir=$root/containers/guest-builder
. "$pmos_dir/pins.env"
tag=${FERRIX_GUEST_BUILDER_TAG:-"ferrix-guest-builder:pmbootstrap-$PMBOOTSTRAP_TAG"}

command -v docker >/dev/null || { echo "missing Docker-compatible container CLI; start OrbStack or the configured container engine (see just host-check)" >&2; exit 2; }
docker info >/dev/null 2>&1 || { echo "container engine is not running; start OrbStack or the configured container engine" >&2; exit 2; }
"$root/targets/qemu-aarch64/scripts/qemu-build.sh"
# Every product artifact the finalize step installs, including the service
# daemons, the apps, and the ADR-004 grants policy (without it ferrix-appd
# starts with an empty policy and the home surface has no capabilities).
for file in ferrix-core ferrix-shell ferrix-basic-screen ferrix-appd ferrix-sandbox ferrix-terminal \
    ferrix-telephonyd ferrix-networkd ferrix-contactsd ferrix-phone ferrix-messages ferrix-contacts ferrix-settings \
    profile.toml colors.toml apps/org.ferrix.home.json apps/grants.json; do
  [ -f "$product/$file" ] || { echo "missing product artifact $product/$file" >&2; exit 1; }
done
mkdir -p "$out"
# pmbootstrap's work directory holds chroots and needs real POSIX semantics
# (ownership, setuid bits, device nodes). macOS host bind mounts may not
# provide all of them, so the default is a named container volume inside the
# container engine's Linux VM. Linux hosts can bind the cache directory instead.
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
    -e "FERRIX_PMOS_ANDROID=$android" \
    $android_mounts \
    "$tag" sh /project/build-in-container.sh 2>&1 || status=$?
  echo "$status" > "$status_file"
} | tee "$out/build.log"
status=$(cat "$status_file")
rm -f "$status_file"
case "$status" in ''|*[!0-9]*) status=1 ;; esac
[ "$status" -eq 0 ] || { echo "pmos-build failed (exit $status); log: $out/build.log" >&2; exit "$status"; }
[ -f "$out/manifest.json" ] || { echo "pmos-build finished without manifest.json" >&2; exit 1; }
printf 'built: %s/vmlinuz, %s/initramfs, %s/qemu-aarch64.img (manifest: %s/manifest.json)\n' "$out" "$out" "$out" "$out"
