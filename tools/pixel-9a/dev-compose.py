#!/usr/bin/env python3
"""Compose a Pixel 9a (tegu MP1.0) development image set for dev-flash.

Inputs are a completed Google Kleaf dist (tools/pixel-9a/kernel.sh build, with
its hash manifest) and an already built Ferrix postmarketOS ext4 rootfs. The
output directory holds the images `just pixel-9a-dev-flash` accepts, plus a
manifest:

  boot.img                Kleaf GKI boot image, unchanged
  vendor_kernel_boot.img  Kleaf first-stage modules + DTB, unchanged
  dtbo.img                Kleaf tegu overlays (includes MP), unchanged
  init_boot.img           Ferrix /init (initramfs-init-kleaf) + busybox/musl
  vendor_boot.img         Ferrix vendor cmdline + bootconfig, no DTB
  vbmeta.img              unsigned AVB metadata (bootloader is unlocked)
  userdata.img            base rootfs + Kleaf vendor_dlkm/system_dlkm modules, as
                          an Android sparse image (fastboot cannot stream the raw
                          2 GiB ext4 image past max-download-size)

Only partitions the stock factory image rewrites are produced. vendor_dlkm and
system_dlkm live inside super, which dev-flash never writes, so their modules
are installed into the rootfs and loaded by /init. Runs on the Linux build host
(needs debugfs, e2fsck, and depmod; the initramfs cpio is written directly).
"""

import argparse
import hashlib
import os
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

TARGET_ID = "google-pixel-9a-tegu-gxq96-mp1-0"
TAB = "\t"


def die(message):
    print(f"FAIL pixel-9a-dev-compose: {message}", file=sys.stderr)
    sys.exit(1)


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def run(*command, capture=False, stdin=None):
    result = subprocess.run([str(part) for part in command], input=stdin,
                            capture_output=capture, text=capture)
    if result.returncode != 0:
        detail = (result.stderr or "").strip() if capture else ""
        die(f"command failed ({result.returncode}): {' '.join(map(str, command))} {detail}")
    return result.stdout if capture else None


def read_kernel_manifest(manifest, dist):
    rows = {}
    fields = {}
    for line in manifest.read_text().splitlines():
        cols = line.split(TAB)
        if cols[0] == "artifact" and len(cols) == 4:
            rows[cols[1]] = (int(cols[2]), cols[3])
        elif len(cols) == 2:
            fields[cols[0]] = cols[1]
    if fields.get("target_id") != TARGET_ID:
        die("kernel manifest target_id is not the exact Pixel 9a target")
    if fields.get("build_system") != "google-kleaf-bazel":
        die("kernel manifest was not produced by Google Kleaf")

    def verified(name):
        if name not in rows:
            die(f"kernel manifest lacks artifact {name}")
        path = dist / name
        if path.is_symlink() or not path.is_file():
            die(f"kernel artifact missing or symlinked: {path}")
        size, digest = rows[name]
        if path.stat().st_size != size or sha256(path) != digest:
            die(f"kernel artifact differs from its manifest: {name}")
        return path

    return verified


class Cpio:
    """Minimal deterministic newc writer (root-owned entries, device nodes)."""

    def __init__(self):
        self.data = bytearray()
        self.inode = 1

    def _entry(self, name, mode, body=b"", rdev=(0, 0)):
        name_bytes = name.encode() + b"\0"
        fields = [self.inode, mode, 0, 0, 1, 0, len(body), 0, 0,
                  rdev[0], rdev[1], len(name_bytes), 0]
        self.inode += 1
        self.data += b"070701" + b"".join(b"%08X" % value for value in fields)
        self.data += name_bytes
        self.data += b"\0" * (-len(self.data) % 4)
        self.data += body
        self.data += b"\0" * (-len(self.data) % 4)

    def directory(self, name):
        self._entry(name, stat.S_IFDIR | 0o755)

    def file(self, name, body, mode=0o755):
        self._entry(name, stat.S_IFREG | mode, body)

    def symlink(self, name, target):
        self._entry(name, stat.S_IFLNK | 0o777, target.encode())

    def chardev(self, name, major, minor, mode=0o600):
        self._entry(name, stat.S_IFCHR | mode, rdev=(major, minor))

    def finish(self):
        self._entry("TRAILER!!!", 0)
        self.data += b"\0" * (-len(self.data) % 512)
        return bytes(self.data)


def debugfs_type(image, path):
    """Return the inode type of path ("regular", "symlink", ...) or None."""
    info = subprocess.run(["debugfs", "-R", f"stat {path}", str(image)],
                          capture_output=True, text=True).stdout
    for word in info.split("Type:", 1)[1:]:
        return word.split()[0]
    return None


def debugfs_dump(image, sources, destination):
    """Dump the first candidate that is a regular file (debugfs does not follow
    symlinks, and the postmarketOS rootfs uses merged /usr)."""
    for source in sources:
        if debugfs_type(image, source) == "regular":
            run("debugfs", "-R", f"dump -p {source} {destination}", image, capture=True)
            if destination.is_file() and destination.stat().st_size > 0:
                return
    die(f"could not read any of {sources} from the base rootfs")


def make_initramfs(base_rootfs, init_script, work):
    busybox = work / "busybox"
    loader = work / "ld-musl-aarch64.so.1"
    debugfs_dump(base_rootfs, ("/usr/bin/busybox", "/bin/busybox"), busybox)
    debugfs_dump(base_rootfs, ("/usr/lib/ld-musl-aarch64.so.1", "/lib/ld-musl-aarch64.so.1"), loader)
    archive = Cpio()
    for directory in ("bin", "dev", "lib", "newroot", "proc", "sys"):
        archive.directory(directory)
    archive.chardev("dev/console", 5, 1)
    archive.chardev("dev/kmsg", 1, 11, 0o644)
    archive.file("bin/busybox", busybox.read_bytes())
    archive.symlink("bin/sh", "busybox")
    archive.file("lib/ld-musl-aarch64.so.1", loader.read_bytes())
    archive.symlink("lib/libc.musl-aarch64.so.1", "ld-musl-aarch64.so.1")
    archive.file("init", init_script.read_bytes())
    output = work / "initramfs.cpio"
    output.write_bytes(archive.finish())
    return output


def safe_extract(archive, destination):
    with tarfile.open(archive) as tar:
        for member in tar.getmembers():
            target = (destination / member.name).resolve()
            if not str(target).startswith(str(destination.resolve())):
                die(f"unsafe path in {archive}: {member.name}")
            if not (member.isdir() or member.isfile()):
                die(f"unexpected member type in {archive}: {member.name}")
        tar.extractall(destination)


def stage_modules(dist, verified, work):
    """Merge Kleaf system_dlkm + vendor_dlkm module trees into one release tree."""
    stage = work / "modules"
    stage.mkdir()
    for name in ("system_dlkm_staging_archive.tar.gz", "vendor_dlkm_staging_archive.tar.gz"):
        safe_extract(verified(name), stage)
    releases = sorted(p.name for p in (stage / "lib/modules").iterdir() if p.is_dir())
    if len(releases) != 1:
        die(f"expected one kernel release in the dlkm archives, found {releases}")
    release = releases[0]
    tree = stage / "lib/modules" / release
    for partition in ("system_dlkm", "vendor_dlkm"):
        load = verified(f"{partition}.modules.load")
        shutil.copyfile(load, tree / f"{partition}.modules.load")
        for module in load.read_text().split():
            if not (tree / module).is_file():
                die(f"{partition}.modules.load names a module missing from its archive: {module}")
    run("depmod", "-b", stage, release)
    if not (tree / "modules.dep").is_file():
        die("depmod produced no modules.dep")
    return stage, release


def install_modules(image, stage, release):
    """Write the staged module tree into the ext4 rootfs without mounting it."""
    root = stage / "lib/modules" / release
    # /lib is a symlink to usr/lib in the merged-/usr rootfs; write through the
    # real directory so debugfs (which does not follow symlinks) can resolve it.
    base = "/usr/lib/modules" if debugfs_type(image, "/lib") == "symlink" else "/lib/modules"
    script = [] if debugfs_type(image, base) == "directory" else [f"mkdir {base}"]
    script.append(f"mkdir {base}/{release}")
    owners = [base, f"{base}/{release}"]
    for path in sorted(root.rglob("*")):
        target = f"{base}/{release}/{path.relative_to(root)}"
        if path.is_dir():
            script.append(f"mkdir {target}")
        else:
            script.append(f"write {path} {target}")
        owners.append(target)
    for target in owners:
        script.append(f"sif {target} uid 0")
        script.append(f"sif {target} gid 0")
    commands = "\n".join(script) + "\n"
    run("debugfs", "-w", "-f", "-", image, capture=True, stdin=commands)
    listing = run("debugfs", "-R", f"ls {base}/{release}", image, capture=True)
    if "vendor_dlkm.modules.load" not in listing or "modules.dep" not in listing:
        die("module tree did not land in the rootfs image")
    run("e2fsck", "-fn", image, capture=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--kernel-dist", required=True, type=Path)
    parser.add_argument("--kernel-manifest", required=True, type=Path)
    parser.add_argument("--kernel-source", required=True, type=Path,
                        help="synced Kleaf source root (provides mkbootimg and avbtool)")
    parser.add_argument("--base-rootfs", required=True, type=Path)
    parser.add_argument("--target-dir", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()

    package = args.target_dir / "pmos/device-google-tegu"
    init_script = package / "initramfs-init-kleaf"
    cmdline_file = package / "kernel-cmdline-kleaf.conf"
    bootconfig = package / "vendor-bootconfig-kleaf"
    for path in (init_script, cmdline_file, bootconfig, args.base_rootfs, args.kernel_manifest):
        if path.is_symlink() or not path.is_file():
            die(f"required input missing or symlinked: {path}")
    mkbootimg = args.kernel_source / "tools/mkbootimg/mkbootimg.py"
    avbtool = args.kernel_source / "prebuilts/kernel-build-tools/linux-x86/bin/avbtool"
    img2simg = args.kernel_source / "prebuilts/kernel-build-tools/linux-x86/bin/img2simg"
    for tool in (mkbootimg, avbtool, img2simg):
        if not tool.is_file():
            die(f"tool missing from the Kleaf source tree: {tool}")
    for tool in ("debugfs", "e2fsck", "depmod"):
        if shutil.which(tool) is None:
            die(f"host tool missing: {tool}")
    if args.out.exists():
        die(f"refusing to overwrite existing output: {args.out}")

    verified = read_kernel_manifest(args.kernel_manifest, args.kernel_dist)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix=".tegu-dev.", dir=args.out.parent))
    try:
        work = stage / "work"
        work.mkdir()
        for name in ("boot.img", "vendor_kernel_boot.img", "dtbo.img"):
            shutil.copyfile(verified(name), stage / name)

        initramfs = make_initramfs(args.base_rootfs, init_script, work)
        run(sys.executable, mkbootimg, "--header_version", "4",
            "--ramdisk", initramfs, "--output", stage / "init_boot.img")

        cmdline = " ".join(line.strip() for line in cmdline_file.read_text().splitlines()
                           if line.strip() and not line.lstrip().startswith("#"))
        empty = work / "vendor-ramdisk.cpio"
        empty.write_bytes(Cpio().finish())
        run(sys.executable, mkbootimg, "--header_version", "4", "--pagesize", "2048",
            "--vendor_cmdline", cmdline, "--vendor_ramdisk", empty,
            "--vendor_bootconfig", bootconfig, "--vendor_boot", stage / "vendor_boot.img")

        run(avbtool, "make_vbmeta_image", "--algorithm", "NONE",
            "--output", stage / "vbmeta.img")

        modules, release = stage_modules(args.kernel_dist, verified, work)
        rootfs = work / "rootfs.img"
        shutil.copyfile(args.base_rootfs, rootfs)
        install_modules(rootfs, modules, release)
        run(img2simg, rootfs, stage / "userdata.img")
        shutil.rmtree(work)

        partitions = {
            "boot": "boot.img", "init_boot": "init_boot.img", "vendor_boot": "vendor_boot.img",
            "vendor_kernel_boot": "vendor_kernel_boot.img", "dtbo": "dtbo.img",
            "vbmeta": "vbmeta.img", "userdata": "userdata.img",
        }
        lines = [
            "schema_version\t1",
            f"target_id\t{TARGET_ID}",
            "route_id\tdevelopment-google-kleaf-gki",
            "kernel_backend\tgoogle-kleaf-cpif",
            f"kernel_release\t{release}",
            "status\tcomposed-not-booted",
            "avb_mode\tNONE",
            f"input\tkernel_manifest\t{args.kernel_manifest}\t{sha256(args.kernel_manifest)}",
            f"input\tbase_rootfs\t{args.base_rootfs}\t{sha256(args.base_rootfs)}",
            f"input\tinitramfs_init\t{init_script}\t{sha256(init_script)}",
            f"input\tvendor_cmdline\t{cmdline}",
            f"input\tvendor_bootconfig\t{bootconfig}\t{sha256(bootconfig)}",
        ]
        for partition, name in partitions.items():
            path = stage / name
            lines.append(f"artifact\t{partition}\t{name}\t{path.stat().st_size}\t{sha256(path)}")
        (stage / "manifest.tsv").write_text("\n".join(lines) + "\n")
        os.rename(stage, args.out)
    except BaseException:
        shutil.rmtree(stage, ignore_errors=True)
        raise
    print(f"PASS pixel-9a-dev-compose: {args.out} (kernel {release})")
    print("NOTE: flash with just pixel-9a-dev-flash flash PART_b=IMG ... ; images are unbooted")


if __name__ == "__main__":
    main()
