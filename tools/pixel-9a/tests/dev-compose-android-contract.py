#!/usr/bin/env python3
"""Offline checks for dev-compose's opt-in Android (Waydroid + Fulguris) mode.

Always: pin verification against tools/android/*.pins shapes, package-set
policy (no waydroid-nftrules), and the checked-in target/guest inputs. On a host with
e2fsprogs (mke2fs -d, debugfs, resize2fs, dumpe2fs; the Linux build host or
the native builder) it also stages a fake runtime into a small ext4 image and
checks growth, file sizes, the runlevel link, and the manifest rows.
"""

import hashlib
import importlib.util
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

repo = Path(__file__).resolve().parents[3]
script = repo / "tools/pixel-9a/dev-compose.py"
spec = importlib.util.spec_from_file_location("pixel9a_dev_compose", script)
assert spec is not None and spec.loader is not None
compose = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compose)
target = repo / "targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"


def refused(call, *args):
    try:
        call(*args)
    except SystemExit as exit_:
        return exit_.code != 0
    return False


def digest(data):
    return hashlib.sha256(data).hexdigest()


# Checked-in policy: the package set carries Waydroid but no nftables packages, and
# the guest bootstrap/properties/service the composer stages exist.
packages = compose.android_package_set(target)
assert "waydroid" in packages and "iptables-legacy" in packages, packages
assert "pipewire-pulse" not in packages, "conflicts with the pmOS pulseaudio audio backend"
assert not set(packages) & set(compose.ANDROID_FORBIDDEN_PACKAGES), packages
assert (repo / "tools/android/guest/ferrix-waydroid.sh").is_file()
props = (target / "pmos/android/waydroid-properties.ini").read_text()
assert "ro.hardware.egl=swiftshader" in props and "ro.hardware.gralloc=default" in props
service = (target / "pmos/device-google-tegu/ferrix-android.initd").read_text()
assert "FERRIX_ANDROID_SESSION_USER=user" in service and "before ferrix-product" in service
product = (target / "pmos/device-google-tegu/ferrix-product.initd").read_text()
assert "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus" in product
assert "FERRIX_ANDROID_SESSION_BUS=unix:path=/run/user/1000/bus" in service
apkbuild = (target / "pmos/device-google-tegu/APKBUILD").read_text()
# Every APKBUILD checksum must match: pmbootstrap refuses the package
# otherwise, which blocks the opt-in Android base rootfs build.
sums = apkbuild.split('sha512sums="\n', 1)[1].split('\n"', 1)[0]
assert "ferrix-android.initd" in sums
for line in sums.splitlines():
    expected, name = line.split("  ", 1)
    data = (target / "pmos/device-google-tegu" / name).read_bytes()
    assert hashlib.sha512(data).hexdigest() == expected, f"stale APKBUILD sha512: {name}"
# The package never links the opt-in service into a runlevel itself.
runlevel_line = next(line for line in apkbuild.splitlines() if line.strip().startswith("for service in"))
assert "ferrix-android" not in runlevel_line

assert refused(compose.check_android_rootfs, {"lxc"}, packages)
assert refused(compose.check_android_rootfs, set(packages) | {"waydroid-nftrules"}, packages)
# postmarketos-base pulls nftables; it is tolerated (iptables fallback).
compose.check_android_rootfs(set(packages) | {"nftables"}, packages)
compose.check_android_rootfs(set(packages) | {"busybox"}, packages)

with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    fake_repo = tmp / "repo"
    (fake_repo / "tools/android/guest").mkdir(parents=True)
    shutil.copyfile(repo / "tools/android/guest/ferrix-waydroid.sh",
                    fake_repo / "tools/android/guest/ferrix-waydroid.sh")
    build = tmp / "out/android/waydroid/20260403"
    build.mkdir(parents=True)
    (tmp / "out/android/waydroid/current").symlink_to("20260403")
    zips = {"system": b"system-zip", "vendor": b"vendor-zip"}
    imgs = {"system": b"S" * 4096, "vendor": b"V" * 2048}
    names = {kind: f"lineage-{kind}.zip" for kind in zips}
    pins = ["WAYDROID_CHANNEL=waydroid_arm64_only", "WAYDROID_BUILD=20260403"]
    for kind in zips:
        (build / names[kind]).write_bytes(zips[kind])
        (build / f"{kind}.img").write_bytes(imgs[kind])
        prefix = f"WAYDROID_{kind.upper()}"
        pins += [f"{prefix}_FILE={names[kind]}", f"{prefix}_SIZE={len(zips[kind])}",
                 f"{prefix}_SHA256={digest(zips[kind])}"]
    (fake_repo / "tools/android/waydroid.pins").write_text("# test\n" + "\n".join(pins) + "\n")
    meta = {"schema_version": 1, "channel": "waydroid_arm64_only", "build": "20260403"}
    for kind in zips:
        meta[kind] = {"zip": names[kind], "zip_sha256": digest(zips[kind]),
                      "img_sha256": digest(imgs[kind])}
    (build / "images.json").write_text(json.dumps(meta))
    apk_bytes = b"PK" + b"\0" * 1000
    apk = tmp / "out/android/fulguris/2.0.10/fulguris_261.apk"
    apk.parent.mkdir(parents=True)
    apk.write_bytes(apk_bytes)
    (fake_repo / "tools/android/fulguris.pins").write_text(
        "FULGURIS_VERSION=2.0.10\nFULGURIS_ASSET=fulguris_261.apk\n"
        f"FULGURIS_SIZE={len(apk_bytes)}\nFULGURIS_SHA256={digest(apk_bytes)}\n")

    inputs = compose.verify_android_inputs(fake_repo, tmp / "out/android/waydroid/current", apk)
    assert inputs["build"] == "20260403" and inputs["images"]["system"].name == "system.img"

    # A tampered image, a wrong APK, and a non-pinned build are refused.
    (build / "vendor.img").write_bytes(b"tampered")
    assert refused(compose.verify_android_inputs, fake_repo, build, apk)
    (build / "vendor.img").write_bytes(imgs["vendor"])
    apk.write_bytes(apk_bytes + b"x")
    assert refused(compose.verify_android_inputs, fake_repo, build, apk)
    apk.write_bytes(apk_bytes)
    other = tmp / "out/android/waydroid/20990101"
    shutil.copytree(build, other)
    assert refused(compose.verify_android_inputs, fake_repo, other, apk)

    tools = ("mke2fs", "debugfs", "resize2fs", "dumpe2fs", "e2fsck")
    if all(shutil.which(tool) for tool in tools):
        root = tmp / "rootfs"
        for directory in ("usr/lib/apk/db", "etc/init.d", "etc/runlevels/default", "etc/ferrix"):
            (root / directory).mkdir(parents=True)
        (root / "lib").symlink_to("usr/lib")
        # dnsmasq arrives through an apk provides line, as on the real image.
        (root / "usr/lib/apk/db/installed").write_text(
            "".join(f"P:{name}\nV:1-r0\n\n" for name in packages + ["busybox", "nftables"]
                    if name != "dnsmasq")
            + "P:dnsmasq-dnssec-dbus\nV:2.91-r0\np:dnsmasq=2.91-r0 cmd:dnsmasq=2.91-r0\n\n")
        (root / "etc/init.d/ferrix-android").write_text(service)
        image = tmp / "rootfs.img"
        subprocess.run(["mke2fs", "-q", "-t", "ext4", "-F", "-d", str(root), str(image), "64M"],
                       check=True)
        work = tmp / "work"
        work.mkdir()
        assert refused(compose.stage_android, image, work, fake_repo, target, inputs, 32)
        work2 = tmp / "work2"
        work2.mkdir()
        rows = compose.stage_android(image, work2, fake_repo, target, inputs, 1200)
        assert image.stat().st_size == 1200 << 20
        assert "rootfs_size_mib\t1200" in rows
        assert any(row.startswith("android_file\t/usr/share/ferrix/android/fulguris.apk\t")
                   for row in rows)
        assert compose.debugfs_type(image, "/etc/runlevels/default/ferrix-android") == "symlink"
        assert compose.debugfs_type(image, "/etc/waydroid-extra/images/system.img") == "regular"
        assert compose.debugfs_type(image, "/var/lib/waydroid") == "directory"
        # A base rootfs without the package set is refused before any write.
        (root / "usr/lib/apk/db/installed").write_text("P:busybox\nV:1\n\n")
        bare = tmp / "bare.img"
        subprocess.run(["mke2fs", "-q", "-t", "ext4", "-F", "-d", str(root), str(bare), "64M"],
                       check=True)
        work3 = tmp / "work3"
        work3.mkdir()
        assert refused(compose.stage_android, bare, work3, fake_repo, target, inputs, 1200)
        assert bare.stat().st_size == 64 << 20
        print("PASS dev-compose android contract (including ext4 staging)")
    else:
        print("PASS dev-compose android contract (ext4 staging skipped: e2fsprogs not on PATH)")
