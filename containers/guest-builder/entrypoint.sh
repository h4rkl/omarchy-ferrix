#!/bin/sh
# Entrypoint for the guest-builder container. Runs as root, verifies the
# privileges and work-filesystem semantics pmbootstrap needs, prepares the
# cache mount, then drops to the unprivileged `pmos` user (pmbootstrap refuses
# to run as root; it escalates through passwordless sudo). Failures are
# machine-readable QEMU-route records.
set -eu
cache=${OMARCHY_PMBOOTSTRAP_CACHE:-/cache/pmbootstrap}

fail() {
  printf '{"evidence":"QEMU","event":"guest-builder-failed","reason":"%s","detail":"%s"}\n' "$1" "$2" >&2
  exit 2
}

[ -e /dev/loop-control ] || fail "no-loop-control" "/dev/loop-control is missing; run the container with --privileged"
probe=$(mktemp -d)
if ! mount -t tmpfs tmpfs "$probe" 2>/dev/null; then
  rmdir "$probe"
  fail "mount-denied" "mount(2) is not permitted; run the container with --privileged"
fi
umount "$probe"
rmdir "$probe"
losetup -f >/dev/null 2>&1 || fail "no-free-loop-device" "losetup -f found no free loop device"

# pmbootstrap waits for partition nodes such as /dev/loop0p2 after
# `losetup -P`. Docker gives the container a static /dev and there is no udev
# here, so let the kernel populate /dev itself: mount devtmpfs over it and
# restore the pseudo-terminal and shm mounts that were layered on the old /dev.
mount -t devtmpfs devtmpfs /dev || fail "devtmpfs-denied" "cannot mount devtmpfs on /dev; run the container with --privileged"
mkdir -p /dev/pts /dev/shm
mount -t devpts -o newinstance,ptmxmode=0666,gid=5,mode=620 devpts /dev/pts || fail "devpts-denied" "cannot mount devpts"
mount -t tmpfs -o nosuid,nodev shm /dev/shm || fail "shm-denied" "cannot mount /dev/shm"

# pmbootstrap chroots need real POSIX semantics on the work filesystem:
# ownership changes, setuid bits (su, sudo, BusyBox) and device nodes. Docker
# Desktop's virtiofs bind mounts from macOS provide none of these, so the
# work directory must be a Docker volume there (pmos-build.sh's default).
mkdir -p "$cache"
fs_probe="$cache/.fs-probe.$$"
fs_detail="use a Docker volume for the pmbootstrap work directory (OMARCHY_PMOS_WORK_VOLUME) instead of a macOS bind mount"
touch "$fs_probe"
chown 1000:1000 "$fs_probe"
[ "$(stat -c %u "$fs_probe")" = 1000 ] || { rm -f "$fs_probe"; fail "work-fs-no-chown" "$cache does not preserve file ownership; $fs_detail"; }
chmod 4755 "$fs_probe"
[ "$(stat -c %a "$fs_probe")" = 4755 ] || { rm -f "$fs_probe"; fail "work-fs-no-setuid" "$cache drops setuid bits; $fs_detail"; }
rm -f "$fs_probe"
if ! mknod "$fs_probe" c 1 3 2>/dev/null; then
  fail "work-fs-no-mknod" "$cache refuses device nodes; $fs_detail"
fi
rm -f "$fs_probe"

# Only the top level: the work dir below is large and pmbootstrap owns its
# contents (a mix of pmos- and root-owned files is expected).
chown pmos:pmos "$cache"
mkdir -p /home/pmos/.config
chown -R pmos:pmos /home/pmos

exec su-exec pmos "$@"
