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
  vbmeta.img              unsigned AVB metadata with verification disabled
                          (flag 2; the bootloader is unlocked)
  userdata.img            base rootfs + Kleaf vendor_dlkm/system_dlkm modules, as
                          an Android sparse image (fastboot cannot stream the raw
                          2 GiB ext4 image past max-download-size)

With --android (FERRIX_PIXEL9A_DEV_ANDROID=1; owner request 2026-10-09) the
rootfs is grown (default 8 GiB) and carries the OPT-IN Waydroid runtime: the
pinned LineageOS system/vendor images and Fulguris APK verified against
tools/android/*.pins, the shared guest bootstrap, and the ferrix-android
service. The base rootfs must come from rootfs.sh with FERRIX_PIXEL9A_ANDROID=1.

Only partitions the stock factory image rewrites are produced. vendor_dlkm and
system_dlkm live inside super, which dev-flash never writes, so their modules
are installed into the rootfs and loaded by /init. Runs on the Linux build host
(needs debugfs, e2fsck, and depmod; the initramfs cpio is written directly).
"""

import argparse
import hashlib
import json
import os
import re
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


def build_reboot_helper(source, work):
    """Compile tools/pixel-9a/reboot-bootloader.rs into a static, libc-free binary."""
    if shutil.which("rustc") is None:
        die("rustc with the aarch64-unknown-linux-musl target is required (run via native-builder)")
    output = work / "reboot-bootloader"
    run("rustc", "--edition", "2021", "--target", "aarch64-unknown-linux-musl",
        "-C", "panic=abort", "-C", "opt-level=s", "-C", "linker=rust-lld",
        "-C", "linker-flavor=ld.lld", "-C", "link-self-contained=no",
        "-C", "relocation-model=static", "-C", "target-feature=+crt-static",
        "-C", "strip=symbols", "-o", output, source)
    return output


def make_initramfs(base_rootfs, init_script, modprobe_script, reboot_helper, work):
    busybox = work / "busybox"
    loader = work / "ld-musl-aarch64.so.1"
    debugfs_dump(base_rootfs, ("/usr/bin/busybox", "/bin/busybox"), busybox)
    debugfs_dump(base_rootfs, ("/usr/lib/ld-musl-aarch64.so.1", "/lib/ld-musl-aarch64.so.1"), loader)
    archive = Cpio()
    for directory in ("bin", "dev", "lib", "newroot", "proc", "sys", "system", "system/bin"):
        archive.directory(directory)
    archive.chardev("dev/console", 5, 1)
    archive.chardev("dev/kmsg", 1, 11, 0o644)
    archive.file("bin/busybox", busybox.read_bytes())
    archive.symlink("bin/sh", "busybox")
    archive.file("bin/reboot-bootloader", reboot_helper.read_bytes())
    archive.file("lib/ld-musl-aarch64.so.1", loader.read_bytes())
    archive.symlink("lib/libc.musl-aarch64.so.1", "ld-musl-aarch64.so.1")
    # CONFIG_MODPROBE_PATH: GKI's pKVM early-module loader runs this before /init.
    archive.file("system/bin/modprobe", modprobe_script.read_bytes())
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


class RootfsEditor:
    """Batches file writes and text patches into the ext4 image via debugfs."""

    def __init__(self, image, work):
        self.image = image
        self.work = work / "rootfs-edit"
        self.work.mkdir()
        self.script = []
        self.count = 0

    def _stage(self, data):
        self.count += 1
        path = self.work / f"f{self.count}"
        path.write_bytes(data)
        return path

    def mkdir_p(self, dest):
        parts = [part for part in dest.split("/") if part]
        for index in range(1, len(parts) + 1):
            path = "/" + "/".join(parts[:index])
            if debugfs_type(self.image, path) is None and f"mkdir {path}" not in self.script:
                self.script.append(f"mkdir {path}")

    def put(self, data, dest, mode):
        self.put_file(self._stage(data), dest, mode)

    def put_file(self, source, dest, mode):
        """Write a host file without reading it into memory (large images)."""
        self.mkdir_p(dest.rsplit("/", 1)[0])
        self.script += [f"rm {dest}", f"write {source} {dest}",
                        f"sif {dest} mode 0{0o100000 | mode:o}",
                        f"sif {dest} uid 0", f"sif {dest} gid 0"]

    def symlink(self, dest, target):
        self.mkdir_p(dest.rsplit("/", 1)[0])
        self.script += [f"rm {dest}", f"symlink {dest} {target}"]

    def remove(self, path):
        if debugfs_type(self.image, path) is not None:
            self.script.append(f"rm {path}")

    def read(self, path):
        target = self.work / f"r{self.count}"
        self.count += 1
        run("debugfs", "-R", f"dump -p {path} {target}", self.image, capture=True)
        return target.read_text() if target.exists() else ""

    def patch(self, path, edit, mode=0o644):
        before = self.read(path)
        after = edit(before)
        if after != before:
            self.put(after.encode(), path, mode)

    def commit(self):
        if self.script:
            run("debugfs", "-w", "-f", "-", self.image, capture=True,
                stdin="\n".join(self.script) + "\n")


def gki_rootfs_patches():
    """Text edits that let the postmarketOS userland run on Google's GKI kernel
    (no devtmpfs, no VT): returns {path: edit-function}."""

    def udev(text):
        # /init provides a tmpfs /dev populated by mdev; eudev runs fine on it.
        return (text
                .replace("if ! grep -qs devtmpfs /proc/filesystems; then",
                         "if ! mountinfo -q -f tmpfs /dev && ! grep -qs devtmpfs /proc/filesystems; then")
                .replace("if ! mountinfo -q -f devtmpfs /dev; then",
                         "if ! mountinfo -q -f devtmpfs /dev && ! mountinfo -q -f tmpfs /dev; then"))

    def seatd(text):
        line = "export SEATD_VTBOUND=0"
        return text if line in text else text.rstrip("\n") + "\n# GKI has no VTs.\n" + line + "\n"

    def group(text):
        out = []
        for line in text.splitlines():
            fields = line.split(":")
            if len(fields) == 4 and fields[0] in ("seat", "input"):
                members = [m for m in fields[3].split(",") if m]
                if "user" not in members:
                    members.append("user")
                fields[3] = ",".join(members)
                line = ":".join(fields)
            out.append(line)
        return "\n".join(out) + "\n"

    def inittab(text):
        # No VTs on GKI; the tty gettys only respawn-fail and leave stray nodes.
        return "\n".join(f"#{line}" if re.match(r"tty[0-9]+::", line) else line
                         for line in text.splitlines()) + "\n"

    def modemmanager(text):
        # The SIT ModemManager chain belongs to the radio-first WWAN kernel; on
        # the Kleaf/cpif route ModemManager runs without it (no modem yet).
        text = re.sub(r"^\s*need ferrix-s5300-mounts exynos-modem sit-proxy\n", "", text,
                      flags=re.M)
        # Same as the package's modemmanager.initd: drop the conf.d option this
        # ModemManager build does not have (catch-up for older base rootfs).
        return text.replace(
            'command_args="${modemmanager_opts}"',
            "command_args=\"$(printf '%s' \"${modemmanager_opts}\" | "
            "sed 's/--test-quick-suspend-resume//')\"")

    def modemmanager_dbus(text):
        # Catch-up for base rootfs images built before the package fix:
        # OpenRC supervises ModemManager; D-Bus must not auto-start it.
        return re.sub(r"^Exec=.*$", "Exec=/bin/false", text, flags=re.M)

    return {"/usr/share/dbus-1/system-services/org.freedesktop.ModemManager1.service":
            modemmanager_dbus,
            "/etc/init.d/udev": udev, "/etc/conf.d/seatd": seatd,
            "/etc/group": group, "/etc/inittab": inittab,
            "/etc/init.d/modemmanager": modemmanager}


# Radio-first (linux-google-tegu WWAN/SIT) services that cannot run on the
# Kleaf/cpif kernel; removed from the default runlevel by the Kleaf composer.
RADIO_ROUTE_SERVICES = ("ferrix-s5300-mounts", "exynos-modem", "sit-proxy")


# Discovery-only ALSA UCM, at the paths the device APKBUILD installs it to.
UCM_FILES = (
    ("ucm-aoc-snd-card.conf", "/usr/share/alsa/ucm2/conf.d/aoc-snd-card/aoc-snd-card.conf"),
    ("ucm-tegu-HiFi.conf", "/usr/share/alsa/ucm2/Google/tegu/HiFi.conf"),
)


# Same destinations rootfs.sh stages the product to.
PRODUCT_BINARIES = (
    "ferrix-core", "ferrix-shell", "ferrix-basic-screen", "ferrix-appd", "ferrix-sandbox",
    "ferrix-terminal", "ferrix-telephonyd", "ferrix-networkd", "ferrix-contactsd",
    "ferrix-phone", "ferrix-messages", "ferrix-contacts", "ferrix-settings",
    "ferrix-files",
)


def apkbuild_default_services(apkbuild):
    """Services the device package links into the default runlevel."""
    match = re.search(r"for service in ([^;]+); do\s+ln -sf /etc/init\.d/\$service "
                      r"\"\$pkgdir\"/etc/runlevels/default/", apkbuild.read_text())
    if match is None:
        die(f"no default-runlevel service list in {apkbuild}")
    return match.group(1).split()


def integrate_rootfs(image, work, package, profile, product, reboot_helper):
    """Bring the base rootfs up to date with this checkout: product binaries,
    profile and app catalog, the device package's service scripts, the
    Fastboot reboot helper, and the GKI userland patches. Returns the files
    written, for the manifest."""
    editor = RootfsEditor(image, work)
    written = []
    # The same helper the initramfs carries, so a booted system can return to
    # bootloader Fastboot (busybox reboot cannot pass a reboot reason).
    editor.put(reboot_helper.read_bytes(), "/usr/sbin/reboot-bootloader", 0o755)
    written.append("/usr/sbin/reboot-bootloader")
    if product is not None:
        for name in PRODUCT_BINARIES:
            editor.put((product / name).read_bytes(), f"/usr/bin/{name}", 0o755)
            written.append(f"/usr/bin/{name}")
        editor.put((product / "colors.toml").read_bytes(), "/etc/ferrix/colors.toml", 0o644)
        for descriptor in sorted((product / "apps").glob("*.json")):
            editor.put(descriptor.read_bytes(), f"/usr/share/ferrix/apps/{descriptor.name}", 0o644)
        written += ["/etc/ferrix/colors.toml", "/usr/share/ferrix/apps/*.json"]
    editor.put(profile.read_bytes(), "/etc/ferrix/profile.toml", 0o644)
    # Device-package files, at the paths the APKBUILD installs them to.
    editor.put((package / "ferrix-openrc.sh").read_bytes(), "/usr/lib/ferrix/openrc.sh", 0o644)
    editor.put((package / "networkmanager-ferrix.conf").read_bytes(),
               "/etc/NetworkManager/conf.d/90-ferrix.conf", 0o644)
    editor.put((package / "modprobe-zram-gs.conf").read_bytes(),
               "/etc/modprobe.d/ferrix-zram-gs.conf", 0o644)
    for name, dest in UCM_FILES:
        editor.put((package / name).read_bytes(), dest, 0o644)
        written.append(dest)
    for script in sorted(package.glob("ferrix-*.initd")):
        dest = f"/etc/init.d/{script.stem}"
        editor.put(script.read_bytes(), dest, 0o755)
        written.append(dest)
    # The APKBUILD's runlevel list is the source of truth; a base rootfs
    # built before a service was added lacks its link.
    for service in apkbuild_default_services(package / "APKBUILD"):
        link = f"/etc/runlevels/default/{service}"
        if service in RADIO_ROUTE_SERVICES:
            editor.remove(link)
            written.append(f"-{link}")
        else:
            editor.symlink(link, f"/etc/init.d/{service}")
            written.append(link)
    # Nothing starts plymouthd on this route, so `plymouth quit` only fails.
    editor.remove("/etc/runlevels/default/kill-plymouth")
    written.append("-/etc/runlevels/default/kill-plymouth")
    for path, edit in gki_rootfs_patches().items():
        editor.patch(path, edit, 0o755 if path.startswith("/etc/init.d/") else 0o644)
        written.append(path)
    editor.commit()
    run("e2fsck", "-fn", image, capture=True)
    return written


# --- OPT-IN Android compatibility runtime (owner request 2026-10-09) --------
# Waydroid + Fulguris per ADR-005/ADR-007. The base rootfs must have been built
# with FERRIX_PIXEL9A_ANDROID=1 (tools/pixel-9a/rootfs.sh installs the package
# set in pmos/android/packages). This composer only stages the pinned images
# and APK the QEMU route already pins (tools/android/*.pins), the shared guest
# bootstrap (tools/android/guest/ferrix-waydroid.sh), the target's renderer
# properties, and the ferrix-android runlevel link.
ANDROID_SERVICE = "ferrix-android"
# nftables itself comes with postmarketos-base and is harmless: without
# CONFIG_NF_TABLES waydroid-net.sh falls back to iptables. Its Waydroid
# ruleset package is refused.
ANDROID_FORBIDDEN_PACKAGES = ("waydroid-nftrules",)
DEFAULT_ANDROID_ROOTFS_MIB = 8192


def read_pins(path):
    """KEY=value pins (never sourced as shell); the first value wins."""
    pins = {}
    for line in path.read_text().splitlines():
        if line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        pins.setdefault(key.strip(), value.strip())
    return pins


def verify_pinned_file(path, size, digest, label):
    if path.is_symlink() or not path.is_file():
        die(f"{label} missing or symlinked: {path}")
    if path.stat().st_size != int(size):
        die(f"{label} size differs from its pin: {path}")
    if sha256(path) != digest.lower():
        die(f"{label} SHA-256 differs from its pin: {path}")


def android_package_set(target_dir):
    lines = (target_dir / "pmos/android/packages").read_text().splitlines()
    return [line.strip() for line in lines if line.strip() and not line.lstrip().startswith("#")]


def verify_android_inputs(repo, images_dir, apk):
    """Check the staged inputs against the shared pins; returns manifest facts."""
    wpins = read_pins(repo / "tools/android/waydroid.pins")
    fpins = read_pins(repo / "tools/android/fulguris.pins")
    images_dir = images_dir.resolve()
    if images_dir.name != wpins["WAYDROID_BUILD"]:
        die(f"Waydroid images dir is not the pinned build {wpins['WAYDROID_BUILD']}: {images_dir}")
    meta_path = images_dir / "images.json"
    if meta_path.is_symlink() or not meta_path.is_file():
        die(f"Waydroid images.json missing (run just waydroid-images-fetch): {meta_path}")
    meta = json.loads(meta_path.read_text())
    if meta.get("build") != wpins["WAYDROID_BUILD"] or meta.get("channel") != wpins["WAYDROID_CHANNEL"]:
        die("Waydroid images.json does not describe the pinned channel/build")
    images = {}
    for kind in ("system", "vendor"):
        prefix = f"WAYDROID_{kind.upper()}"
        archive = images_dir / wpins[f"{prefix}_FILE"]
        verify_pinned_file(archive, wpins[f"{prefix}_SIZE"], wpins[f"{prefix}_SHA256"],
                           f"Waydroid {kind} zip")
        entry = meta.get(kind) or {}
        if entry.get("zip") != archive.name or entry.get("zip_sha256") != wpins[f"{prefix}_SHA256"].lower():
            die(f"Waydroid images.json {kind} entry does not name the pinned zip")
        image = images_dir / f"{kind}.img"
        if image.is_symlink() or not image.is_file() or sha256(image) != entry.get("img_sha256"):
            die(f"Waydroid {kind}.img differs from the image unpacked from the pinned zip: {image}")
        images[kind] = image
    verify_pinned_file(apk, fpins["FULGURIS_SIZE"], fpins["FULGURIS_SHA256"], "Fulguris APK")
    if apk.name != fpins["FULGURIS_ASSET"]:
        die(f"Fulguris APK is not the pinned asset {fpins['FULGURIS_ASSET']}: {apk}")
    return {"build": wpins["WAYDROID_BUILD"], "channel": wpins["WAYDROID_CHANNEL"],
            "images": images, "meta": meta_path, "apk": apk,
            "fulguris_version": fpins["FULGURIS_VERSION"]}


def installed_packages(image, work):
    """Package names and apk provides from the rootfs database (merged /usr aware)."""
    dest = work / "apk-installed"
    debugfs_dump(image, ("/usr/lib/apk/db/installed", "/lib/apk/db/installed"), dest)
    names = set()
    for line in dest.read_text().splitlines():
        if line.startswith("P:"):
            names.add(line[2:])
        elif line.startswith("p:"):
            names.update(token.split("=", 1)[0] for token in line[2:].split())
    return names


def check_android_rootfs(packages, required):
    missing = sorted(set(required) - packages)
    if missing:
        die("base rootfs lacks the Android package set (rebuild it with "
            f"FERRIX_PIXEL9A_ANDROID=1): missing {', '.join(missing)}")
    forbidden = sorted(packages & set(ANDROID_FORBIDDEN_PACKAGES))
    if forbidden:
        die(f"base rootfs contains {', '.join(forbidden)}; the GKI cannot load its nft ruleset")


def free_bytes(image):
    header = run("dumpe2fs", "-h", image, capture=True)
    fields = dict(line.split(":", 1) for line in header.splitlines() if ":" in line)
    return int(fields["Free blocks"]) * int(fields["Block size"])


def grow_rootfs(image, size_mib):
    """Grow the ext4 image file and filesystem to size_mib (never shrinks)."""
    target = size_mib << 20
    if target < image.stat().st_size:
        die(f"rootfs size {size_mib} MiB is smaller than the base image")
    # resize2fs requires a forced check since the last mount; -p repairs only
    # what is safe automatically (exit 1 means it fixed something).
    result = subprocess.run(["e2fsck", "-fp", str(image)], capture_output=True, text=True)
    if result.returncode not in (0, 1):
        die(f"e2fsck before resize failed ({result.returncode}): {result.stderr.strip()}")
    if target > image.stat().st_size:
        os.truncate(image, target)
        run("resize2fs", image, f"{size_mib}M", capture=True)
    run("e2fsck", "-fn", image, capture=True)


def stage_android(image, work, repo, target_dir, inputs, size_mib):
    """Grow the rootfs and stage the Waydroid runtime; returns manifest rows."""
    check_android_rootfs(installed_packages(image, work), android_package_set(target_dir))
    grow_rootfs(image, size_mib)
    helper = repo / "tools/android/guest/ferrix-waydroid.sh"
    props = target_dir / "pmos/android/waydroid-properties.ini"
    staged = [
        (inputs["images"]["system"], "/etc/waydroid-extra/images/system.img", 0o644),
        (inputs["images"]["vendor"], "/etc/waydroid-extra/images/vendor.img", 0o644),
        (inputs["meta"], "/etc/waydroid-extra/images.json", 0o644),
        (inputs["apk"], "/usr/share/ferrix/android/fulguris.apk", 0o644),
        (props, "/etc/ferrix/waydroid-properties.ini", 0o644),
        (helper, "/usr/lib/ferrix/ferrix-waydroid.sh", 0o755),
    ]
    needed = sum(source.stat().st_size for source, _, _ in staged)
    # Keep at least 1 GiB for Android /data, logs, and the dalvik cache.
    if free_bytes(image) < needed + (1 << 30):
        die(f"rootfs of {size_mib} MiB leaves too little space for the Android runtime")
    (work / "android").mkdir()
    editor = RootfsEditor(image, work / "android")
    for source, dest, mode in staged:
        editor.put_file(source, dest, mode)
    editor.mkdir_p("/var/lib/waydroid")
    if debugfs_type(image, f"/etc/init.d/{ANDROID_SERVICE}") != "regular":
        die(f"/etc/init.d/{ANDROID_SERVICE} is missing from the integrated rootfs")
    editor.symlink(f"/etc/runlevels/default/{ANDROID_SERVICE}", f"/etc/init.d/{ANDROID_SERVICE}")
    editor.commit()
    for source, dest, _ in staged:
        info = run("debugfs", "-R", f"stat {dest}", image, capture=True)
        if not re.search(rf"\bSize: {source.stat().st_size}\b", info):
            die(f"{dest} did not land in the rootfs with its full size")
    if debugfs_type(image, f"/etc/runlevels/default/{ANDROID_SERVICE}") != "symlink":
        die("ferrix-android runlevel link did not land in the rootfs")
    run("e2fsck", "-fn", image, capture=True)
    rows = [f"android\twaydroid\t{inputs['channel']}\t{inputs['build']}",
            f"rootfs_size_mib\t{size_mib}"]
    for source, dest, _ in staged:
        rows.append(f"android_file\t{dest}\t{source.stat().st_size}\t{sha256(source)}")
    rows.append(f"android_service\t/etc/runlevels/default/{ANDROID_SERVICE}")
    return rows


def main():
    env = os.environ.get
    repo = Path(__file__).resolve().parents[2]
    kernel_out = Path(env("FERRIX_PIXEL9A_KERNEL_OUT", repo / "out/pixel-9a/kernel/tegu"))
    if not kernel_out.is_absolute():
        kernel_out = repo / kernel_out
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("command", nargs="?", choices=["build"], default="build")
    parser.add_argument("--kernel-dist", type=Path, default=kernel_out / "dist")
    parser.add_argument("--kernel-manifest", type=Path, default=kernel_out / "build-manifest.tsv")
    parser.add_argument("--kernel-source", type=Path,
                        default=Path(env("FERRIX_PIXEL9A_KERNEL_SOURCE",
                                         repo / ".cache/ferrix/pixel-9a/kernel/source")),
                        help="synced Kleaf source root (provides mkbootimg and avbtool)")
    parser.add_argument("--base-rootfs", type=Path, default=env("FERRIX_PIXEL9A_DEV_BASE_ROOTFS"))
    parser.add_argument("--target-dir", type=Path,
                        default=repo / "targets/boards/google-pixel-9a-tegu-gxq96-mp1-0")
    parser.add_argument("--out", type=Path, default=env("FERRIX_PIXEL9A_DEV_OUT"))
    parser.add_argument("--checkpoint", default=env("FERRIX_PIXEL9A_DEV_CHECKPOINT", ""),
                        help="initramfs stage at which /init reboots to bootloader Fastboot")
    parser.add_argument("--checkpoint-action", choices=["fastboot", "delay", "hold"],
                        default=env("FERRIX_PIXEL9A_DEV_CHECKPOINT_ACTION", "fastboot"),
                        help="reboot to Fastboot at the checkpoint, delay 30s (timing signal), "
                             "or hold with the USB debug console up")
    parser.add_argument("--modules-limit", default=env("FERRIX_PIXEL9A_DEV_MODULES_LIMIT", ""),
                        help="load only the first N first-stage modules (bisection)")
    parser.add_argument("--modules-skip", default=env("FERRIX_PIXEL9A_DEV_MODULES_SKIP", ""),
                        help="comma-separated module names /init must not load (bisection)")
    parser.add_argument("--product-dir", type=Path, default=env("FERRIX_PIXEL9A_DEV_PRODUCT_DIR"),
                        help="pixel-9a product build to install over the base rootfs")
    fulguris = read_pins(repo / "tools/android/fulguris.pins")
    parser.add_argument("--android", action="store_true",
                        default=env("FERRIX_PIXEL9A_DEV_ANDROID", "") == "1",
                        help="stage the opt-in Waydroid runtime and Fulguris (base rootfs built "
                             "with FERRIX_PIXEL9A_ANDROID=1)")
    parser.add_argument("--waydroid-images", type=Path,
                        default=Path(env("FERRIX_PIXEL9A_DEV_WAYDROID_IMAGES",
                                         repo / "out/android/waydroid/current")),
                        help="verified tools/android/waydroid-images-fetch.sh output")
    parser.add_argument("--fulguris-apk", type=Path,
                        default=Path(env("FERRIX_PIXEL9A_DEV_FULGURIS_APK",
                                         repo / "out/android/fulguris" / fulguris["FULGURIS_VERSION"]
                                         / fulguris["FULGURIS_ASSET"])),
                        help="verified tools/android/fulguris-fetch.sh output")
    parser.add_argument("--rootfs-size-mib", type=int,
                        default=int(env("FERRIX_PIXEL9A_DEV_ROOTFS_SIZE_MIB", "0") or 0),
                        help=f"grow the rootfs image to this size (--android default "
                             f"{DEFAULT_ANDROID_ROOTFS_MIB})")
    args = parser.parse_args()
    if args.android and not args.rootfs_size_mib:
        args.rootfs_size_mib = DEFAULT_ANDROID_ROOTFS_MIB
    if args.rootfs_size_mib < 0:
        die("--rootfs-size-mib must be positive")
    if args.product_dir is not None and not (args.product_dir / "ferrix-shell").is_file():
        die(f"product dir has no ferrix-shell: {args.product_dir}")
    if args.modules_skip and not all(part.replace("-", "").replace("_", "").isalnum()
                                     for part in args.modules_skip.split(",")):
        die(f"modules skip must be comma-separated module names: {args.modules_skip}")
    if args.modules_limit and not args.modules_limit.isdigit():
        die(f"modules limit must be a number: {args.modules_limit}")
    if args.base_rootfs is None or args.out is None:
        die("--base-rootfs/FERRIX_PIXEL9A_DEV_BASE_ROOTFS and --out/FERRIX_PIXEL9A_DEV_OUT are required")
    if args.checkpoint and not args.checkpoint.isalnum():
        die(f"checkpoint must be a bare stage name: {args.checkpoint}")

    package = args.target_dir / "pmos/device-google-tegu"
    init_script = package / "initramfs-init-kleaf"
    modprobe_script = package / "initramfs-modprobe-kleaf"
    cmdline_file = package / "kernel-cmdline-kleaf.conf"
    bootconfig = package / "vendor-bootconfig-kleaf"
    reboot_source = repo / "tools/pixel-9a/reboot-bootloader.rs"
    for path in (init_script, modprobe_script, cmdline_file, bootconfig, reboot_source,
                 args.base_rootfs, args.kernel_manifest):
        if path.is_symlink() or not path.is_file():
            die(f"required input missing or symlinked: {path}")
    mkbootimg = args.kernel_source / "tools/mkbootimg/mkbootimg.py"
    avbtool = args.kernel_source / "prebuilts/kernel-build-tools/linux-x86/bin/avbtool"
    img2simg = args.kernel_source / "prebuilts/kernel-build-tools/linux-x86/bin/img2simg"
    lz4 = args.kernel_source / "prebuilts/kernel-build-tools/linux-x86/bin/lz4"
    for tool in (mkbootimg, avbtool, img2simg, lz4):
        if not tool.is_file():
            die(f"tool missing from the Kleaf source tree: {tool}")
    for tool in ("debugfs", "e2fsck", "depmod"):
        if shutil.which(tool) is None:
            die(f"host tool missing: {tool}")
    if args.out.exists():
        die(f"refusing to overwrite existing output: {args.out}")

    verified = read_kernel_manifest(args.kernel_manifest, args.kernel_dist)
    android_inputs = None
    if args.android:
        for tool in ("dumpe2fs", "resize2fs"):
            if shutil.which(tool) is None:
                die(f"host tool missing: {tool}")
        android_inputs = verify_android_inputs(repo, args.waydroid_images, args.fulguris_apk)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix=".tegu-dev.", dir=args.out.parent))
    try:
        work = stage / "work"
        work.mkdir()
        for name in ("boot.img", "vendor_kernel_boot.img", "dtbo.img"):
            shutil.copyfile(verified(name), stage / name)

        reboot_helper = build_reboot_helper(reboot_source, work)
        # The bootloader concatenates the vendor_boot, vendor_kernel_boot, and
        # init_boot ramdisks. Kleaf's vendor_kernel_boot ramdisk is lz4-legacy,
        # which has no end marker: the kernel's unlz4 reads whatever follows as
        # another chunk, so every later segment must be lz4-legacy as well or
        # unpacking stops before /init exists (stock ramdisks are all lz4-legacy).
        def lz4_legacy(source):
            output = source.with_suffix(source.suffix + ".lz4")
            run(lz4, "-l", "-12", "--favor-decSpeed", "-q", source, output)
            if output.read_bytes()[:4] != b"\x02\x21\x4c\x18":
                die(f"lz4 did not produce a legacy-format stream: {output}")
            return output

        initramfs = lz4_legacy(make_initramfs(args.base_rootfs, init_script, modprobe_script,
                                              reboot_helper, work))
        run(sys.executable, mkbootimg, "--header_version", "4",
            "--ramdisk", initramfs, "--output", stage / "init_boot.img")

        cmdline = " ".join(line.strip() for line in cmdline_file.read_text().splitlines()
                           if line.strip() and not line.lstrip().startswith("#"))
        if args.checkpoint:
            cmdline += f" ferrix.checkpoint={args.checkpoint}"
            if args.checkpoint_action != "fastboot":
                cmdline += f" ferrix.checkpoint_action={args.checkpoint_action}"
        if args.modules_limit:
            cmdline += f" ferrix.modules_limit={args.modules_limit}"
        if args.modules_skip:
            cmdline += f" ferrix.modules_skip={args.modules_skip}"
        empty = work / "vendor-ramdisk.cpio"
        empty.write_bytes(Cpio().finish())
        empty = lz4_legacy(empty)
        run(sys.executable, mkbootimg, "--header_version", "4", "--pagesize", "2048",
            "--vendor_cmdline", cmdline, "--vendor_ramdisk", empty,
            "--vendor_bootconfig", bootconfig, "--vendor_boot", stage / "vendor_boot.img")

        # Without descriptors libavb loads the requested boot partitions only
        # when the top-level vbmeta carries VERIFICATION_DISABLED (flag 2), the
        # standard unlocked-device GSI recipe. A flag-0 descriptor-less vbmeta
        # leaves the bootloader with no boot images and the slot never starts.
        run(avbtool, "make_vbmeta_image", "--algorithm", "NONE", "--flags", "2",
            "--padding_size", "4096", "--output", stage / "vbmeta.img")

        modules, release = stage_modules(args.kernel_dist, verified, work)
        rootfs = work / "rootfs.img"
        shutil.copyfile(args.base_rootfs, rootfs)
        install_modules(rootfs, modules, release)
        integrated = integrate_rootfs(rootfs, work, package, args.target_dir / "dev/profile.toml",
                                      args.product_dir, reboot_helper)
        if android_inputs is not None:
            android_rows = stage_android(rootfs, work, repo, args.target_dir, android_inputs,
                                         args.rootfs_size_mib)
        else:
            if args.rootfs_size_mib:
                grow_rootfs(rootfs, args.rootfs_size_mib)
            android_rows = ["android\tnone"]
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
            "avb_mode\tNONE-verification-disabled",
            f"input\tkernel_manifest\t{args.kernel_manifest}\t{sha256(args.kernel_manifest)}",
            f"input\tbase_rootfs\t{args.base_rootfs}\t{sha256(args.base_rootfs)}",
            f"input\tinitramfs_init\t{init_script}\t{sha256(init_script)}",
            f"input\tinitramfs_modprobe\t{modprobe_script}\t{sha256(modprobe_script)}",
            f"input\treboot_helper\t{reboot_source}\t{sha256(reboot_source)}",
            f"checkpoint\t{args.checkpoint or 'none'}\t{args.checkpoint_action}",
            f"input\tproduct_dir\t{args.product_dir or 'base-rootfs'}",
            f"rootfs_integrated\t{len(integrated)}",
            f"input\tvendor_cmdline\t{cmdline}",
            f"input\tvendor_bootconfig\t{bootconfig}\t{sha256(bootconfig)}",
        ] + android_rows
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
