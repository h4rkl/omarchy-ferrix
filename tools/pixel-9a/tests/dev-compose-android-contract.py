#!/usr/bin/env python3
"""Offline checks for dev-compose's opt-in Android (Waydroid) mode; the Browser
(LineageOS Jelly) ships inside the Android image, so no APK is staged.

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
# Android audio rides pipewire-pulse; rootfs.sh selects the PipeWire backend
# provider rather than listing a backend package (that would conflict).
assert "pipewire-pulse" in packages, packages
assert not any(p.startswith("postmarketos-base-ui-audio-backend") for p in packages), packages
assert not set(packages) & set(compose.ANDROID_FORBIDDEN_PACKAGES), packages
assert (repo / "tools/android/guest/ferrix-waydroid.sh").is_file()
props = (target / "pmos/android/waydroid-properties.ini").read_text()
assert "ro.hardware.egl=swiftshader" in props and "ro.hardware.gralloc=default" in props
# Android apps are per-task windows (owner decision 2026-10-10).
assert "persist.waydroid.multi_windows=true" in props.splitlines(), props
assert "persist.waydroid.task_streams=true" in props.splitlines(), props
service = (target / "pmos/device-google-tegu/ferrix-android.initd").read_text()
assert "FERRIX_ANDROID_SESSION_USER=user" in service and "before ferrix-product" in service
product = (target / "pmos/device-google-tegu/ferrix-product.initd").read_text()
assert "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus" in product
# The container binds the extra socket's directory (ferrix-waydroid.sh
# FERRIX_ANDROID_WAYLAND_DIR default), so Android survives a shell restart.
assert "--extra-socket ferrix-android-wayland/wayland-0" in product
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
    # The Browser (LineageOS Jelly) is part of the Android image: no APK
    # input exists any more.
    inputs = compose.verify_android_inputs(fake_repo, tmp / "out/android/waydroid/current")
    assert inputs["build"] == "20260403" and inputs["images"]["system"].name == "system.img"
    assert "apk" not in inputs

    # A tampered image and a non-pinned build are refused.
    (build / "vendor.img").write_bytes(b"tampered")
    assert refused(compose.verify_android_inputs, fake_repo, build)
    (build / "vendor.img").write_bytes(imgs["vendor"])
    other = tmp / "out/android/waydroid/20990101"
    shutil.copytree(build, other)
    assert refused(compose.verify_android_inputs, fake_repo, other)

    # Stock vendor overlays (Mali renderer, hardware Codec2): files and the
    # property file must match the manifest; hardware codecs need both their
    # overlay and the Mali renderer.
    def fake_overlay(kind, files):
        out = tmp / f"overlay-{kind}"
        rows = ["schema_version\t1"]
        for rel, data in files.items():
            path = out / "overlay" / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            rows.append(f"file\t{rel}\t{len(data)}\t{digest(data)}")
        prop = out / f"waydroid-{kind}.prop"
        prop.write_text("[properties]\nmedia.c2.hal.selection=aidl\n")
        rows.append(f"prop\t{prop.name}\t{digest(prop.read_bytes())}")
        (out / "manifest.tsv").write_text("\n".join(rows) + "\n")
        return out

    codec2 = fake_overlay("codec2", {
        "vendor/bin/hw/samsung.hardware.media.c2@1.2-service": b"\x7fELF-service",
        "vendor/lib64/ferrix-codec2/libcodec2_vndk.so": b"\x7fELF-lib",
        "vendor/etc/media_codecs_c2.xml": b"<MediaCodecs/>",
    })
    staged_codec2 = compose.verify_codec2_overlay(codec2)
    dests = {dest for _, dest in staged_codec2}
    assert "/usr/share/ferrix/android/codec2/overlay/vendor/lib64/ferrix-codec2/libcodec2_vndk.so" in dests
    assert "/usr/share/ferrix/android/codec2/waydroid-codec2.prop" in dests
    assert "/usr/share/ferrix/android/codec2/manifest.tsv" in dests
    # The Mali verifier refuses a Codec2 output (wrong property file).
    assert refused(compose.verify_mali_overlay, codec2)
    (codec2 / "overlay/vendor/etc/media_codecs_c2.xml").write_bytes(b"<tampered/>")
    assert refused(compose.verify_codec2_overlay, codec2)
    (codec2 / "overlay/vendor/etc/media_codecs_c2.xml").write_bytes(b"<MediaCodecs/>")
    (codec2 / "waydroid-codec2.prop").write_text("[properties]\n")
    assert refused(compose.verify_codec2_overlay, codec2)
    (codec2 / "waydroid-codec2.prop").write_text("[properties]\nmedia.c2.hal.selection=aidl\n")
    escape = fake_overlay("codec2", {"vendor/../etc/passwd": b"x"})
    assert refused(compose.verify_codec2_overlay, escape)
    codec2 = fake_overlay("codec2", {
        "vendor/lib64/ferrix-codec2/libcodec2_vndk.so": b"\x7fELF-lib",
    })
    compose.check_codecs("software", "swiftshader", False)
    compose.check_codecs("hardware", "mali", True)
    assert refused(compose.check_codecs, "hardware", "mali", False)
    assert refused(compose.check_codecs, "hardware", "swiftshader", True)

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
        assert not any(row.startswith("android_file\t/usr/share/ferrix/android/")
                       for row in rows)
        assert any(row.startswith("android_file\t/usr/lib/ferrix/ferrix-waydroid.sh\t")
                   for row in rows)
        assert compose.debugfs_type(image, "/etc/runlevels/default/ferrix-android") == "symlink"
        assert compose.debugfs_type(image, "/etc/waydroid-extra/images/system.img") == "regular"
        assert compose.debugfs_type(image, "/var/lib/waydroid") == "directory"
        assert "android_codecs\tsoftware" in rows
        # Hardware codecs: the Codec2 overlay lands under its share dir and
        # the selection file says hardware.
        hw = dict(inputs, codec2=compose.verify_codec2_overlay(codec2),
                  mali=compose.verify_mali_overlay(fake_overlay("mali", {
                      "vendor/lib64/egl/libGLES_mali.so": b"\x7fELF-mali"})),
                  renderer="mali", codecs="hardware")
        image_hw = tmp / "rootfs-hw.img"
        subprocess.run(["mke2fs", "-q", "-t", "ext4", "-F", "-d", str(root), str(image_hw), "64M"],
                       check=True)
        work_hw = tmp / "work-hw"
        work_hw.mkdir()
        rows = compose.stage_android(image_hw, work_hw, fake_repo, target, hw, 1200)
        assert "android_codecs\thardware" in rows and "android_renderer\tmali" in rows
        lib = "/usr/share/ferrix/android/codec2/overlay/vendor/lib64/ferrix-codec2/libcodec2_vndk.so"
        assert compose.debugfs_type(image_hw, lib) == "regular"
        assert compose.debugfs_type(image_hw, "/usr/share/ferrix/android/codec2/waydroid-codec2.prop") == "regular"
        selected = subprocess.run(["debugfs", "-R", "cat /etc/ferrix/waydroid-codecs", str(image_hw)],
                                  capture_output=True, text=True, check=True).stdout
        assert selected.strip() == "hardware", selected
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
