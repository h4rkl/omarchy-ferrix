#!/bin/sh
# Runs as root inside containers/guest-builder (via sudo from
# build-in-container.sh). Mounts the pmbootstrap-generated disk image through
# a loop device, installs the project readiness marker into the root
# filesystem, exports the boot files QEMU needs, records image facts and
# copies the image to the output directory.
set -eu
img=$1
project=${FERRIX_PMOS_PROJECT:-/project}
out=${FERRIX_PMOS_OUT:-/out}
marker_src="$project/local.d/ferrix-boot-health.start"
marker_dst=/etc/local.d/ferrix-boot-health.start
product_src="$project/local.d/00-ferrix-product.start"
product_dst=/etc/local.d/00-ferrix-product.start

log() { printf '[pmos-finalize] %s\n' "$*"; }
fail() {
  printf '{"evidence":"QEMU","event":"pmos-build-failed","reason":"%s","detail":"%s"}\n' "$1" "$2" >&2
  exit 1
}

mnt=$(mktemp -d)
loop=""
boot_mounted=0
root_mounted=0
cleanup() {
  [ "$boot_mounted" = 1 ] && umount "$mnt/boot" 2>/dev/null || true
  [ "$root_mounted" = 1 ] && umount "$mnt" 2>/dev/null || true
  [ -n "$loop" ] && losetup -d "$loop" 2>/dev/null || true
  rmdir "$mnt" 2>/dev/null || true
}
trap cleanup EXIT

loop=$(losetup --find --show --partscan "$img")
loopname=${loop#/dev/}
# No udev in the container: create partition nodes from sysfs when missing.
for sys in /sys/block/"$loopname"/"$loopname"p*; do
  [ -d "$sys" ] || continue
  name=${sys##*/}
  if [ ! -e "/dev/$name" ]; then
    major=$(cut -d: -f1 "$sys/dev")
    minor=$(cut -d: -f2 "$sys/dev")
    mknod "/dev/$name" b "$major" "$minor"
  fi
done
rootdev=""
bootdev=""
partitions=""
for dev in /dev/"$loopname"p*; do
  [ -e "$dev" ] || continue
  label=$(blkid -o value -s LABEL "$dev" 2>/dev/null || true)
  fstype=$(blkid -o value -s TYPE "$dev" 2>/dev/null || true)
  partitions="$partitions${partitions:+,}{\"index\":${dev##*p},\"label\":\"$label\",\"fstype\":\"$fstype\"}"
  case "$label" in
    pmOS_root) rootdev=$dev ;;
    pmOS_boot) bootdev=$dev ;;
  esac
done
[ -n "$rootdev" ] || fail "no-root-partition" "no partition labelled pmOS_root in $img"
[ -n "$bootdev" ] || fail "no-boot-partition" "no partition labelled pmOS_boot in $img"
log "root=$rootdev boot=$bootdev"

mount "$rootdev" "$mnt"
root_mounted=1
mkdir -p "$mnt/boot"
mount -o ro "$bootdev" "$mnt/boot"
boot_mounted=1

# Readiness marker (OpenRC local.d, default runlevel).
install -Dm755 "$marker_src" "$mnt$marker_dst"
install -Dm755 "$product_src" "$mnt$product_dst"
install -Dm755 /product/mobile-core "$mnt/usr/bin/mobile-core"
install -Dm755 /product/mobile-shell "$mnt/usr/bin/mobile-shell"
install -Dm755 /product/mobile-basic-screen "$mnt/usr/bin/mobile-basic-screen"
install -Dm644 /product/profile.toml "$mnt/etc/ferrix/profile.toml"
local_enabled=true
# -L as well as -e: the runlevel entry is a symlink to /etc/init.d/local that
# dangles when resolved against the container's own root.
if [ ! -L "$mnt/etc/runlevels/default/local" ] && [ ! -e "$mnt/etc/runlevels/default/local" ]; then
  local_enabled=false
  [ -x "$mnt/etc/init.d/local" ] || fail "no-openrc-local" "OpenRC local service missing in rootfs"
  ln -s /etc/init.d/local "$mnt/etc/runlevels/default/local"
  log "enabled OpenRC local service in default runlevel"
fi

# Boot files: boot-deploy places them on the boot partition.
mkdir -p "$out"
exported=""
copy_out() {
  cp -f "$1" "$out/$2"
  exported="$exported${exported:+,}\"$2\""
}
kernel=""
for candidate in "$mnt/boot/vmlinuz" "$mnt/boot"/vmlinuz-*; do
  [ -f "$candidate" ] && { kernel=$candidate; break; }
done
[ -n "$kernel" ] || fail "no-kernel" "no vmlinuz on the boot partition"
copy_out "$kernel" vmlinuz
initramfs=""
for candidate in "$mnt/boot/initramfs" "$mnt/boot"/initramfs-*; do
  case "$candidate" in *-extra*) continue ;; esac
  [ -f "$candidate" ] && { initramfs=$candidate; break; }
done
[ -n "$initramfs" ] || fail "no-initramfs" "no initramfs on the boot partition"
copy_out "$initramfs" initramfs
[ -f "$mnt/boot/initramfs-extra" ] && copy_out "$mnt/boot/initramfs-extra" initramfs-extra

# Kernel command line: the systemd-boot loader entry written by boot-deploy is
# authoritative when present; otherwise compose it exactly as boot-deploy
# does, from /usr/lib/kernel-cmdline.d and /etc/kernel-cmdline.d.
cmdline=""
cmdline_source=""
for entry in "$mnt"/boot/loader/entries/*.conf; do
  [ -f "$entry" ] || continue
  cmdline=$(sed -n 's/^options[[:space:]]*//p' "$entry" | head -1)
  [ -n "$cmdline" ] && { cmdline_source="boot/loader/entries/${entry##*/}"; break; }
done
if [ -z "$cmdline" ]; then
  cmdline=$(for f in "$mnt"/usr/lib/kernel-cmdline.d/*.conf "$mnt"/etc/kernel-cmdline.d/*.conf; do
    [ -f "$f" ] && cat "$f"
  done | grep -v '^[[:space:]]*#' | tr '\n' ' ' | tr -s ' ')
  cmdline_source="usr/lib/kernel-cmdline.d + etc/kernel-cmdline.d"
fi
cmdline=$(printf '%s' "$cmdline" | sed 's/^ *//; s/ *$//')
[ -n "$cmdline" ] || fail "no-cmdline" "could not determine the kernel command line"
printf '%s\n' "$cmdline" > "$out/cmdline.txt"
exported="$exported,\"cmdline.txt\""

# Facts from the installed rootfs.
os_release=$(sed -n 's/^PRETTY_NAME="\{0,1\}\([^"]*\)"\{0,1\}$/\1/p' "$mnt/etc/os-release" | head -1)
os_version=$(sed -n 's/^VERSION_ID="\{0,1\}\([^"]*\)"\{0,1\}$/\1/p' "$mnt/etc/os-release" | head -1)
packages=$(awk -v want=" device-qemu-aarch64 device-qemu-aarch64-kernel-stable linux-postmarketos-stable postmarketos-base postmarketos-base-openrc postmarketos-mkinitfs boot-deploy systemd-boot openrc busybox musl alpine-base networkmanager " '
  /^P:/ { name = substr($0, 3) }
  /^V:/ { if (index(want, " " name " ")) printf "%s\"%s\":\"%s\"", (n++ ? "," : ""), name, substr($0, 3) }
' "$mnt/lib/apk/db/installed")
marker_sha=$(sha256sum "$mnt$marker_dst" | cut -d' ' -f1)
product_marker_sha=$(sha256sum "$mnt$product_dst" | cut -d' ' -f1)
core_sha=$(sha256sum "$mnt/usr/bin/mobile-core" | cut -d' ' -f1)
shell_sha=$(sha256sum "$mnt/usr/bin/mobile-shell" | cut -d' ' -f1)
screen_sha=$(sha256sum "$mnt/usr/bin/mobile-basic-screen" | cut -d' ' -f1)
cat > "$out/image-facts.json" <<JSON
{
  "os_release": {"pretty_name": "$os_release", "version_id": "$os_version"},
  "packages": {$packages},
  "kernel_cmdline": "$cmdline",
  "kernel_cmdline_source": "$cmdline_source",
  "partitions": [$partitions],
  "readiness_marker": {"path": "$marker_dst", "sha256": "$marker_sha", "service": "openrc local", "local_service_was_enabled": $local_enabled},
  "product": {"startup_path": "$product_dst", "startup_sha256": "$product_marker_sha", "mobile_core_sha256": "$core_sha", "mobile_shell_sha256": "$shell_sha", "mobile_basic_screen_sha256": "$screen_sha"},
  "exported": [$exported,"qemu-aarch64.img"]
}
JSON

sync
umount "$mnt/boot"; boot_mounted=0
umount "$mnt"; root_mounted=0
losetup -d "$loop"; loop=""

log "copying disk image to $out/qemu-aarch64.img"
cp --sparse=always "$img" "$out/qemu-aarch64.img"
log "exported: vmlinuz initramfs cmdline.txt qemu-aarch64.img"
