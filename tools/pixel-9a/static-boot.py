#!/usr/bin/env python3
"""Compose the exact Pixel 9a native static-boot route without device access.

This target-local host workflow uses Python because the pinned Android image
tools are Python programs and their variable-width TSV handoffs need strict
parsing. It never invokes ADB, Fastboot, or a device node.
"""

from __future__ import annotations

import hashlib
import os
import platform
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / "targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
ROUTE = TARGET / "kernel-radio-route.toml"
CONTRACT = TARGET / "native-static-artifact-contract.toml"
OUT = Path(os.environ.get("FERRIX_OUT_DIR", ROOT / "out"))
KERNEL_OUT = Path(
    os.environ.get(
        "FERRIX_PIXEL9A_RADIO_KERNEL_OUT", OUT / "pixel-9a/kernel-radio/tegu-wwan"
    )
)
KERNEL_DIST = Path(os.environ.get("FERRIX_PIXEL9A_RADIO_KERNEL_DIST", KERNEL_OUT / "dist"))
KERNEL_MANIFEST = Path(
    os.environ.get("FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST", KERNEL_OUT / "build-manifest.tsv")
)
ROOTFS_OUT = Path(os.environ.get("FERRIX_PIXEL9A_ROOTFS_OUT", OUT / "pixel-9a/rootfs/tegu"))
ROOTFS_MANIFEST = Path(
    os.environ.get("FERRIX_PIXEL9A_ROOTFS_MANIFEST", ROOTFS_OUT / "manifest.tsv")
)
BOOT_INPUTS_OUT = Path(
    os.environ.get("FERRIX_PIXEL9A_BOOT_INPUTS_OUT", OUT / "pixel-9a/boot-inputs/tegu-wwan")
)
BOOT_INPUTS_MANIFEST = Path(
    os.environ.get("FERRIX_PIXEL9A_BOOT_INPUTS_MANIFEST", BOOT_INPUTS_OUT / "manifest.tsv")
)
CAPACITIES = Path(os.environ.get("FERRIX_PIXEL9A_STATIC_PARTITION_SIZES", ""))
IMAGE_OUT = Path(
    os.environ.get("FERRIX_PIXEL9A_STATIC_BOOT_OUT", OUT / "pixel-9a/images/tegu-wwan-static")
)
SOURCE = Path(
    os.environ.get(
        "FERRIX_PIXEL9A_RADIO_SOURCE", ROOT / ".cache/ferrix/pixel-9a/kernel-radio/source"
    )
)
TARGET_ID = "google-pixel-9a-tegu-gxq96-mp1-0"
SOURCE_ROUTE = "experimental-linux-google-tegu-s5300-wwan"
STATIC_ROUTE = "experimental-linux-google-tegu-s5300-wwan-static"
PARTITIONS = (
    "boot",
    "init_boot",
    "vendor_boot",
    "vendor_kernel_boot",
    "dtbo",
    "vbmeta",
    "vbmeta_vendor",
    "vbmeta_system",
)
PAYLOADS = PARTITIONS[:5]
BOOT_INPUT_NAMES = (
    "initramfs.cpio", "vendor-ramdisk.cpio", "vendor-kernel-boot.cpio",
    "vendor-bootconfig", "vendor-cmdline",
)
BUILD_TOKEN = "I-understand-this-composes-source-built-pixel-9a-static-boot-images"


class Refusal(Exception):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise Refusal(message)


def regular(path: Path, label: str) -> Path:
    require(path.is_file() and not path.is_symlink(), f"{label} is missing or a symlink: {path}")
    return path


def digest(path: Path, algorithm: str = "sha256") -> str:
    value = hashlib.new(algorithm)
    with regular(path, "hash input").open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def size(path: Path) -> int:
    return regular(path, "size input").stat().st_size


def render_vendor_cmdline(path: Path) -> str:
    arguments = [line.strip() for line in regular(path, "kernel command line").read_text().splitlines()
                 if line.strip() and not line.lstrip().startswith("#")]
    require(arguments and all(re.fullmatch(r"[A-Za-z0-9._=,/+-]+", arg) for arg in arguments),
            "source-owned vendor command line contains an unsafe argument")
    require({"root=PARTLABEL=userdata", "console=ttySAC0,115200", "bootconfig"}
            <= set(arguments), "source-owned vendor command line lacks boot arguments")
    result = " ".join(arguments)
    require(len(result) < 2048, "source-owned vendor command line exceeds header capacity")
    return result


def require_vendor_layout(path: Path, dtb_bytes: int, cmdline: str) -> None:
    # AOSP vendor_boot_img_hdr_v4: fixed fields end at byte 2128. Checking
    # these few offsets also works on the Mac transfer verifier without adding
    # a second host-side Android image parser dependency.
    with regular(path, "vendor boot image").open("rb") as stream:
        header = stream.read(2128)
    require(len(header) == 2128 and header[:8] == b"VNDRBOOT",
            "vendor boot image lacks a version-4 header")
    version, page_size = struct.unpack_from("<II", header, 8)
    declared_header_size, declared_dtb_size = struct.unpack_from("<II", header, 2096)
    actual_cmdline = header[28:2076].split(b"\0", 1)[0]
    require(version == 4 and declared_header_size == 2128 and page_size == 2048,
            "vendor boot header version or page size differs from reviewed stock layout")
    require(declared_dtb_size == dtb_bytes,
            "vendor boot DTB placement differs from reviewed stock layout")
    require(actual_cmdline == cmdline.encode("ascii"),
            "vendor boot command line differs from source-owned input")


def run(*args: str, capture: bool = False) -> bytes:
    try:
        result = subprocess.run(args, check=True, capture_output=capture)
    except (OSError, subprocess.CalledProcessError) as error:
        raise Refusal(f"command failed: {args[0]}: {error}") from error
    return result.stdout if capture else b""


def tsv(path: Path) -> list[tuple[str, ...]]:
    regular(path, "manifest")
    rows = []
    for line in path.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        rows.append(tuple(line.split("\t")))
    require(rows, f"manifest is empty: {path}")
    return rows


def one(rows: list[tuple[str, ...]], key: str, width: int | None = None) -> tuple[str, ...]:
    matches = [row for row in rows if row[0] == key]
    require(len(matches) == 1, f"expected exactly one {key} row, found {len(matches)}")
    row = matches[0]
    require(width is None or len(row) == width, f"{key} row has wrong field count")
    return row


def named(rows: list[tuple[str, ...]], key: str, name: str, width: int) -> tuple[str, ...]:
    matches = [row for row in rows if len(row) > 1 and row[:2] == (key, name)]
    require(len(matches) == 1, f"expected exactly one {key}/{name} row")
    require(len(matches[0]) == width, f"{key}/{name} row has wrong field count")
    return matches[0]


def scalar(rows: list[tuple[str, ...]], key: str, expected: str) -> None:
    require(one(rows, key, 2)[1] == expected, f"{key} differs from reviewed value")


def relative(value: str) -> Path:
    path = Path(value)
    require(
        value and not path.is_absolute() and all(piece not in ("", ".", "..") for piece in value.split("/"))
        and not any(char.isspace() for char in value),
        f"unsafe relative path: {value}",
    )
    return path


def checked(path: Path, expected_size: str, expected_hash: str, label: str) -> Path:
    require(re.fullmatch(r"[0-9]+", expected_size) is not None, f"{label} size is malformed")
    require(re.fullmatch(r"[0-9a-f]{64}", expected_hash) is not None, f"{label} hash is malformed")
    regular(path, label)
    require(size(path) == int(expected_size), f"{label} size differs")
    require(digest(path) == expected_hash, f"{label} SHA-256 differs")
    return path


def artifact(rows: list[tuple[str, ...]], base: Path, name: str) -> Path:
    row = named(rows, "artifact", name, 4)
    require(relative(row[1]) == Path(name), f"{name} artifact path is not canonical")
    return checked(base / row[1], row[2], row[3], name)


def capacity_handoff(path: Path | None = None) -> dict[str, int]:
    path = CAPACITIES if path is None else path
    require(str(path) not in ("", "."), "FERRIX_PIXEL9A_STATIC_PARTITION_SIZES is required")
    content = regular(path, "static capacity handoff").read_text()
    identity_line = (f"# exact-target={TARGET_ID} product=tegu revision=MP1.0 "
                     "sku=GXQ96 current_slot=a is_userspace=no")
    require(identity_line in content and
            "# source=Fastboot getvar partition-size:<partition>_<slot>" in content,
            "static capacities lack exact-target ordinary-bootloader provenance")
    observed = re.findall(r"^# observed_utc=(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ)$",
                          content, re.MULTILINE)
    require(len(observed) == 1, "static capacities lack a singular observation timestamp")
    target_manifest = tomllib.loads(regular(TARGET / "manifest.toml", "target manifest").read_text())
    restored_utc = target_manifest.get("recovery_verified_utc", "")
    require(re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ", restored_utc) is not None and
            observed[0] > restored_utc,
            "static capacities predate the verified stock restore; collect fresh live sizes")
    rows = tsv(path)
    scalar(rows, "schema_version", "2")
    scalar(rows, "target_id", TARGET_ID)
    scalar(rows, "partition_contract", "static-boot-only")
    result: dict[str, int] = {}
    for row in rows:
        if row[0] != "partition":
            require(row[0] in ("schema_version", "target_id", "partition_contract"),
                    f"unknown static capacity row: {row[0]}")
            continue
        require(len(row) == 3 and row[1] in PARTITIONS and row[1] not in result,
                "static capacity has an unknown, duplicate, or malformed partition")
        require(re.fullmatch(r"[0-9]+", row[2]) is not None, f"{row[1]} capacity is malformed")
        bytes_count = int(row[2])
        require(bytes_count > 0 and bytes_count % 4096 == 0,
                f"{row[1]} capacity must be positive and 4096-byte aligned")
        result[row[1]] = bytes_count
    require(tuple(result) == PARTITIONS, "static capacity handoff must name the eight partitions in reviewed order")
    return result


def validate_policy(path: Path, label: str) -> None:
    lines = regular(path, label).read_text().splitlines()
    selected = [line for line in lines if line.strip() and not line.lstrip().startswith("#")]
    require(not selected, f"{label} selects loadable modules")


def no_modules_in_cpio(path: Path, label: str) -> None:
    try:
        completed = subprocess.run(("cpio", "-it"), input=path.read_bytes(), capture_output=True, check=True)
    except (OSError, subprocess.CalledProcessError) as error:
        raise Refusal(f"cannot inspect {label} CPIO: {error}") from error
    entries = completed.stdout.decode("utf-8", "replace").splitlines()
    require(not any(name.endswith(".ko") or ".ko." in name for name in entries),
            f"{label} CPIO contains a loadable module")


def validate() -> dict[str, object]:
    require(platform.system() == "Linux" and platform.machine() == "x86_64",
            "native Linux x86_64 host is required")
    route = tomllib.loads(regular(ROUTE, "radio route").read_text())
    contract = tomllib.loads(regular(CONTRACT, "static contract").read_text())
    require(route.get("target_id") == TARGET_ID and route.get("route_id") == SOURCE_ROUTE and
            route.get("status") == "experimental-not-adopted" and
            route.get("architecture") == "arm64" and
            route.get("compiler_mode") == "llvm",
            "radio source route differs")
    require(route.get("native_static_artifact_contract") == CONTRACT.name and
            route.get("native_static_artifact_contract_sha256") == digest(CONTRACT),
            "static contract hash differs from route pin")
    require(contract.get("target_id") == TARGET_ID and contract.get("route_id") == STATIC_ROUTE,
            "static contract target or route differs")
    require(contract.get("source_only") is True and contract.get("stock_modules_may_be_reused") is False
            and contract.get("android_userspace_required") is False and
            contract.get("source_kernel_backend") == "linux-google-tegu-wwan" and
            contract.get("boot_header_version") == 4 and
            contract.get("boot_page_size") == 4096 and
            contract.get("vendor_boot_page_size") == 2048 and
            contract.get("vendor_dtb_partition") == "vendor_kernel_boot" and
            contract.get("capacity_schema_version") == 2 and
            contract.get("capacity_contract") == "static-boot-only",
            "static contract no longer requires source-only native Linux")
    require(contract.get("partition_contract") == "complete-native-static-builtins" and
            tuple(contract.get("partitions", ())) == PARTITIONS and
            set(contract.get("forbidden_writes", ())) >=
            {"bootloader", "radio", "super", "vendor_dlkm", "system_dlkm", "metadata",
             "persist", "efs", "efs_backup", "modem_userdata"} and
            contract.get("logical_partition_writes") is False and
            contract.get("allocation_changes") is False and
            contract.get("active_slot_writes") is False and
            contract.get("temporary_boot_observation_required") is True and
            contract.get("tested_stock_recovery_required") is True and
            contract.get("per_run_confirmation_required") is True and
            contract.get("rootfs_mapping_status") == "pending-live-map-and-separate-wipe-confirmation",
            "static contract partition boundary differs")
    require(contract.get("builtins", {}).get("config_source") == "extract-ikconfig-from-hash-verified-kernel-Image" and
            contract["builtins"].get("selected_initfs_modules_must_be_empty") is True and
            contract["builtins"].get("selected_vendor_modules_must_be_empty") is True and
            contract["builtins"].get("selected_system_modules_must_be_empty") is True and
            contract["builtins"].get("early_cpio_loadable_modules_forbidden") is True and
            contract.get("avb", {}).get("descriptors") == "fresh-from-final-static-images-only" and
            contract["avb"].get("stock_descriptors_forbidden") is True and
            contract["avb"].get("logical_partition_descriptors_forbidden") is True and
            contract["avb"].get("independent_descriptor_verification_required") is True,
            "static builtin or AVB source policy differs")
    capacities = capacity_handoff()

    kernel = tsv(KERNEL_MANIFEST)
    for key, value in (("schema_version", "1"), ("target_id", TARGET_ID),
                       ("route_id", SOURCE_ROUTE), ("kernel_backend", "linux-google-tegu-wwan"),
                       ("build_system", "kbuild-make"), ("source_revision", route["source_revision"]),
                       ("status", "built-not-booted"), ("bootable", "false")):
        scalar(kernel, key, value)
    require(named(kernel, "split_artifact_contract", route["split_artifact_contract"], 3)[2]
            == route["split_artifact_contract_sha256"], "kernel manifest names a stale split contract")
    for key, path_key, hash_key in (
        ("config_fragment", "config_fragment", "config_sha256"),
        ("extra_config_fragment", "extra_config_fragment", "extra_config_sha256"),
        ("device_tree_source", "device_tree_source", "device_tree_sha256"),
        ("firmware_mapping", "firmware_mapping", "firmware_mapping_sha256"),
    ):
        require(one(kernel, key, 3)[1:] == (route[path_key], route[hash_key]),
                f"kernel {key} differs from pinned route")
    image = artifact(kernel, KERNEL_DIST, "Image")
    dtb = artifact(kernel, KERNEL_DIST, "dtb.img")
    dtbo_row = one(kernel, "dtbo_source_set", 4)
    require(dtbo_row[1] == "split-inputs/dtbo-source-set.tsv", "DTBO source-set path differs")
    dtbo_index = checked(KERNEL_DIST / dtbo_row[1], dtbo_row[2], dtbo_row[3], "DTBO source set")
    dtbo_rows = tsv(dtbo_index)
    scalar(dtbo_rows, "schema_version", "1")
    scalar(dtbo_rows, "source_root", "arch/arm64/boot/dts")
    dtbo_files = []
    seen_sources = set()
    for row in dtbo_rows:
        if row[0] != "source":
            require(row[0] in ("schema_version", "source_root"), "DTBO source set has unknown row")
            continue
        require(len(row) == 4 and row[1] not in seen_sources and
                Path(row[1]).suffix in (".dtb", ".dtbo"), "DTBO source row differs")
        seen_sources.add(row[1])
        source = checked(KERNEL_DIST / "split-inputs/dtbo-source-set" / relative(row[1]),
                         row[2], row[3], f"DTBO source {row[1]}")
        if source.suffix == ".dtbo":
            dtbo_files.append(source)
    require(dtbo_files, "source-built DTBO overlay set is empty")
    module_row = one(kernel, "module_install", 4)
    require(module_row[1] == "split-inputs/module-install.tsv", "module-install path differs")
    checked(KERNEL_DIST / module_row[1], module_row[2], module_row[3], "module-install manifest")
    archive = one(kernel, "source_archive", 5)
    require(Path(archive[1]).name == f"{route['source_revision']}.tar.gz", "kernel archive path differs")
    require(archive[2:] == (str(route["source_archive_bytes"]), route["source_archive_sha256"],
                            route["source_archive_sha512"]),
            "kernel archive pins differ from reviewed source route")
    checked(Path(archive[1]), archive[2], archive[3], "pinned kernel source archive")
    require(digest(Path(archive[1]), "sha512") == archive[4], "kernel source archive SHA-512 differs")
    require(run("git", "-C", str(SOURCE), "rev-parse", "HEAD", capture=True).decode().strip()
            == route["source_revision"], "kernel source checkout revision differs")
    require(not run("git", "-C", str(SOURCE), "status", "--porcelain", "--untracked-files=all", capture=True),
            "kernel source checkout has non-ignored changes")
    extractor = SOURCE / contract["builtins"]["extractor"]
    require(digest(extractor) == contract["builtins"]["extractor_sha256"],
            "IKCONFIG extractor differs from pinned source")
    require(contract["builtins"]["extractor_source_revision"] == route["source_revision"],
            "IKCONFIG extractor source revision differs")
    config = run(str(extractor), str(image), capture=True)
    require(config, "hash-verified kernel Image has no IKCONFIG")
    config_lines = set(config.decode("utf-8").splitlines())
    for symbol in contract["builtins"]["required"]:
        require(symbol in config_lines, f"kernel Image lacks built-in symbol: {symbol}")

    rootfs = tsv(ROOTFS_MANIFEST)
    for key, value in (("schema_version", "1"), ("target_id", TARGET_ID),
                       ("codename", "tegu"), ("hardware_revision", "MP1.0"),
                       ("substrate", "postmarketos"), ("route_id", SOURCE_ROUTE),
                       ("kernel_backend", "linux-google-tegu-wwan"),
                       ("status", "built-substrate-only"), ("bootable", "false")):
        scalar(rootfs, key, value)
    rootfs_tar = artifact(rootfs, ROOTFS_OUT, "rootfs.tar")
    rootfs_img = artifact(rootfs, ROOTFS_OUT, "rootfs.img")
    pins = dict(line.split("=", 1) for line in (TARGET / "pmos/pins.env").read_text().splitlines()
                if line and not line.startswith("#") and "=" in line)
    scalar(rootfs, "pmaports_commit", pins["PMAPORTS_COMMIT"])

    inputs = tsv(BOOT_INPUTS_MANIFEST)
    for key, value in (("schema_version", "1"), ("target_id", TARGET_ID),
                       ("route_id", SOURCE_ROUTE), ("kernel_backend", "linux-google-tegu-wwan"),
                       ("status", "built-source-inputs"), ("bootable", "false")):
        scalar(inputs, key, value)
    for key, path in (("kernel_manifest", KERNEL_MANIFEST), ("rootfs_manifest", ROOTFS_MANIFEST)):
        row = one(inputs, key, 4)
        require(Path(row[1]) == path, f"boot inputs name another {key}")
        checked(path, row[2], row[3], f"boot-input {key}")
    boot_files: dict[str, Path] = {}
    origins = {"initramfs.cpio": "source-built-initramfs",
               "vendor-ramdisk.cpio": "source-built-vendor-ramdisk",
               "vendor-kernel-boot.cpio": "source-built-early-modules",
               "vendor-bootconfig": "source-owned-bootconfig",
               "vendor-cmdline": "source-owned-kernel-cmdline"}
    for name, origin in origins.items():
        row = named(inputs, "artifact", name, 6)
        require(row[2] == name and row[5] == origin, f"boot-input {name} path or source differs")
        boot_files[name] = checked(BOOT_INPUTS_OUT / name, row[3], row[4], name)
    require(boot_files["vendor-cmdline"].read_text().strip() ==
            render_vendor_cmdline(TARGET / "pmos/device-google-tegu/kernel-cmdline.conf"),
            "vendor command line differs from checked-in source")
    source_inputs = {"initramfs_source": "initramfs-init",
                     "vendor_bootconfig_source": "vendor-bootconfig",
                     "kernel_cmdline_source": "kernel-cmdline.conf",
                     "modules_initfs_policy": "modules-initfs-radio",
                     "modules_vendor_policy": "modules-vendor-dlkm",
                     "modules_system_policy": "modules-system-dlkm"}
    for key, filename in source_inputs.items():
        row = named(inputs, "input", key, 5)
        checked(TARGET / "pmos/device-google-tegu" / filename, row[3], row[4], key)
    policies = ("modules-initfs-radio", "modules-vendor-dlkm", "modules-system-dlkm")
    for policy in policies:
        validate_policy(TARGET / "pmos/device-google-tegu" / policy, policy)
    for cpio_name in ("initramfs.cpio", "vendor-ramdisk.cpio", "vendor-kernel-boot.cpio"):
        no_modules_in_cpio(boot_files[cpio_name], cpio_name)
    for label, dirname in (("vendor_dlkm", "vendor-dlkm"), ("system_dlkm", "system-dlkm")):
        row = named(inputs, "module_root", label, 5)
        require(row[2] == dirname and row[4] == "0", f"{label} selected module root differs")
        policy_file = TARGET / "pmos/device-google-tegu" / (
            "modules-vendor-dlkm" if label == "vendor_dlkm" else "modules-system-dlkm"
        )
        require(row[3] == digest(policy_file), f"{label} module policy hash differs")
        module_root = BOOT_INPUTS_OUT / dirname
        require(module_root.is_dir() and not module_root.is_symlink(), f"{label} root is missing")
        require(not any(path.is_symlink() or path.name.endswith(".ko") for path in module_root.rglob("*")),
                f"{label} contains a symlink or loadable module")
        recorded = {Path(entry[2]) for entry in inputs if len(entry) == 5 and entry[:2] == ("tree_file", label)}
        actual = {path.relative_to(module_root) for path in module_root.rglob("*") if path.is_file()}
        require(recorded and recorded == actual, f"{label} file set differs from boot-input manifest")
        for entry in inputs:
            if len(entry) == 5 and entry[:2] == ("tree_file", label):
                checked(module_root / relative(entry[2]), entry[3], entry[4], f"{label}/{entry[2]}")
    vendor_tree = TARGET / "pmos/device-google-tegu/vendor-ramdisk"
    recorded_vendor = [row for row in inputs if row[:2] == ("tree_file", "vendor_ramdisk_source")]
    actual_vendor = {path.relative_to(vendor_tree) for path in vendor_tree.rglob("*") if path.is_file()}
    require({relative(row[2]) for row in recorded_vendor} == actual_vendor,
            "source-owned vendor ramdisk file set differs")
    for row in recorded_vendor:
        require(len(row) == 5, "vendor ramdisk source row is malformed")
        checked(vendor_tree / relative(row[2]), row[3], row[4], f"vendor ramdisk/{row[2]}")
    return {"route": route, "contract": contract, "kernel": kernel, "rootfs": rootfs,
            "inputs": inputs, "capacities": capacities, "image": image, "dtb": dtb,
            "rootfs_tar": rootfs_tar, "rootfs_img": rootfs_img, "boot_files": boot_files,
            "config": config, "dtbo_files": dtbo_files}


def tool(name: str) -> str:
    path = shutil.which(name)
    require(path is not None, f"missing native builder tool: {name}")
    return path


def record_file(rows: list[tuple[str, ...]], key: str, path: Path, base: Path) -> None:
    rows.append((key, str(path.relative_to(base)), str(size(path)), digest(path)))


def build(inputs: dict[str, object]) -> None:
    require(os.environ.get("FERRIX_PIXEL9A_STATIC_BOOT_BUILD") == BUILD_TOKEN,
            f"static composition is opt-in; set FERRIX_PIXEL9A_STATIC_BOOT_BUILD={BUILD_TOKEN}")
    require(not any(os.environ.get(name) for name in
                    ("FERRIX_PIXEL9A_AVB_INPUT_MANIFEST", "FERRIX_PIXEL9A_AVB_INPUT_ROOT",
                     "FERRIX_PIXEL9A_AVB_KEY")),
            "external or stock AVB metadata and unreviewed signing keys are forbidden")
    require(os.environ.get("FERRIX_PIXEL9A_AVB_ALGORITHM", "NONE") == "NONE",
            "static route currently accepts only the reviewed unsigned AVB mode")
    output = IMAGE_OUT.resolve()
    image_parent = (OUT / "pixel-9a/images").resolve()
    require(output.is_relative_to(image_parent) and output != image_parent,
            "static image output must stay under out/pixel-9a/images")
    require(not IMAGE_OUT.exists() and not IMAGE_OUT.is_symlink(),
            f"refusing to overwrite static image output: {IMAGE_OUT}")
    mkbootimg, mkdtboimg, avbtool = (tool(name) for name in
                                    ("mkbootimg", "mkdtboimg.py", "avbtool"))
    image_parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".tegu-static-boot.", dir=image_parent) as temporary:
        stage = Path(temporary)
        provenance = stage / "provenance"
        provenance.mkdir()
        source_files = ((KERNEL_MANIFEST, "kernel-manifest.tsv"),
                        (ROOTFS_MANIFEST, "rootfs-manifest.tsv"),
                        (BOOT_INPUTS_MANIFEST, "boot-inputs-manifest.tsv"),
                        (CAPACITIES, "partition-capacities.tsv"),
                        (CONTRACT, "native-static-artifact-contract.toml"),
                        (ROUTE, "kernel-radio-route.toml"))
        for source, name in source_files:
            shutil.copyfile(source, provenance / name)
        (provenance / "kernel.config").write_bytes(inputs["config"])
        for source, name in ((inputs["image"], "Image"), (inputs["dtb"], "dtb.img"),
                             (inputs["rootfs_tar"], "rootfs.tar"),
                             (inputs["rootfs_img"], "rootfs.img")):
            shutil.copyfile(source, stage / name)
        boot_files = inputs["boot_files"]
        for name in BOOT_INPUT_NAMES:
            shutil.copyfile(boot_files[name], provenance / name)
        run(mkbootimg, "--header_version", "4", "--pagesize", "4096",
            "--kernel", str(stage / "Image"), "--output", str(stage / "boot.img"))
        run(mkbootimg, "--header_version", "4", "--pagesize", "4096",
            "--ramdisk", str(boot_files["initramfs.cpio"]),
            "--output", str(stage / "init_boot.img"))
        run(mkbootimg, "--header_version", "4", "--pagesize", "2048",
            "--vendor_ramdisk", str(boot_files["vendor-ramdisk.cpio"]),
            "--vendor_cmdline", boot_files["vendor-cmdline"].read_text().strip(),
            "--vendor_bootconfig", str(boot_files["vendor-bootconfig"]),
            "--vendor_boot", str(stage / "vendor_boot.img"))
        run(mkbootimg, "--header_version", "4", "--pagesize", "2048",
            "--vendor_ramdisk", str(boot_files["vendor-kernel-boot.cpio"]),
            "--dtb", str(stage / "dtb.img"),
            "--vendor_boot", str(stage / "vendor_kernel_boot.img"))
        run(mkdtboimg, "create", str(stage / "dtbo.img"), "--page_size=4096",
            *(str(path) for path in inputs["dtbo_files"]))
        for partition in ("boot", "init_boot", "vendor_boot", "vendor_kernel_boot"):
            expected_magic = b"ANDROID!" if partition in ("boot", "init_boot") else b"VNDRBOOT"
            require((stage / f"{partition}.img").open("rb").read(8) == expected_magic,
                    f"{partition} has the wrong boot-container magic")
        require_vendor_layout(stage / "vendor_boot.img", 0,
                              boot_files["vendor-cmdline"].read_text().strip())
        require_vendor_layout(stage / "vendor_kernel_boot.img", size(stage / "dtb.img"), "")
        capacities = inputs["capacities"]
        for partition in PAYLOADS:
            image = stage / f"{partition}.img"
            require(size(image) < capacities[partition], f"{partition} payload exceeds live capacity")
            salt = digest(image)
            run(avbtool, "add_hash_footer", "--image", str(image),
                "--partition_name", partition,
                "--partition_size", str(capacities[partition]),
                "--hash_algorithm", "sha256", "--salt", salt,
                "--algorithm", "NONE")
            require(size(image) <= capacities[partition], f"{partition} AVB footer exceeds capacity")
        descriptor_sets = {
            "vbmeta": ("boot", "init_boot"),
            "vbmeta_vendor": ("vendor_boot", "vendor_kernel_boot", "dtbo"),
            "vbmeta_system": (),
        }
        for vbmeta, partitions in descriptor_sets.items():
            output_file = stage / f"{vbmeta}.img"
            command = [avbtool, "make_vbmeta_image", "--algorithm", "NONE",
                       "--output", str(output_file)]
            for partition in partitions:
                command.extend(("--include_descriptors_from_image", str(stage / f"{partition}.img")))
            run(*command)
            require(0 < size(output_file) <= capacities[vbmeta],
                    f"{vbmeta} metadata exceeds live capacity")
            verification = run(avbtool, "verify_image", "--image", str(output_file), capture=True)
            (provenance / f"{vbmeta}-verification.txt").write_bytes(verification)
            info = run(avbtool, "info_image", "--image", str(output_file), capture=True)
            (provenance / f"{vbmeta}-info.txt").write_bytes(info)
            names = re.findall(rb"^\s*Partition Name:\s*(\S+)", info, re.MULTILINE)
            names_text = tuple(name.decode("ascii") for name in names)
            require(len(names_text) == len(partitions) and set(names_text) == set(partitions),
                    f"{vbmeta} AVB descriptors do not match the reviewed static image set")
        rows: list[tuple[str, ...]] = [
            ("schema_version", "1"), ("target_id", TARGET_ID),
            ("codename", "tegu"), ("hardware_revision", "MP1.0"),
            ("route_id", STATIC_ROUTE), ("source_route_id", SOURCE_ROUTE),
            ("kernel_backend", "linux-google-tegu-wwan"),
            ("source_revision", inputs["route"]["source_revision"]),
            ("status", "composed-not-booted"), ("bootable", "false"),
            ("header_version", "4"),
            ("partition_contract", "complete-native-static-builtins"),
            ("rootfs_mapping_status", "pending-live-map-and-separate-wipe-confirmation"),
            ("logical_partition_writes", "false"), ("active_slot_writes", "false"),
            ("avb_mode", "fresh-generated"), ("avb_algorithm", "NONE"),
            ("avb_descriptor_inputs", "fresh-avbtool-hash-footers-from-final-static-images"),
            ("avb_binding_salt", "sha256-of-pre-footer-image-bytes"),
        ]
        for key, filename in (("static_contract", "native-static-artifact-contract.toml"),
                              ("route_metadata", "kernel-radio-route.toml"),
                              ("kernel_manifest", "kernel-manifest.tsv"),
                              ("rootfs_manifest", "rootfs-manifest.tsv"),
                              ("boot_inputs_manifest", "boot-inputs-manifest.tsv"),
                              ("partition_capacities", "partition-capacities.tsv"),
                              ("kernel_config", "kernel.config")):
            record_file(rows, key, provenance / filename, stage)
        for name in BOOT_INPUT_NAMES:
            record_file(rows, "boot_input_" + name.replace(".", "_"),
                        provenance / name, stage)
        for partition, capacity in capacities.items():
            rows.append(("partition_capacity", partition, str(capacity)))
        for vbmeta, names in descriptor_sets.items():
            rows.append(("avb_descriptor_set", vbmeta, ",".join(names) or "empty"))
            record_file(rows, "avb_verification", provenance / f"{vbmeta}-verification.txt", stage)
            record_file(rows, "avb_info", provenance / f"{vbmeta}-info.txt", stage)
        for partition in PAYLOADS:
            image = stage / f"{partition}.img"
            rows.append(("avb_bound_image", partition, image.name, str(size(image)), digest(image)))
        origins = {"Image": "kernel", "dtb.img": "kernel", "rootfs.tar": "rootfs",
                   "rootfs.img": "rootfs"}
        for filename in ("Image", "dtb.img", *(f"{partition}.img" for partition in PARTITIONS),
                         "rootfs.tar", "rootfs.img"):
            file = stage / filename
            origin = origins.get(filename, "generated-avbtool-from-final-images" if
                                 filename.startswith("vbmeta") else
                                 "generated-mkdtboimg" if filename == "dtbo.img" else
                                 "generated-mkbootimg")
            name = filename.removesuffix(".img") if filename not in ("dtb.img", "rootfs.img") else (
                "dtb.img" if filename == "dtb.img" else "rootfs_image")
            if filename == "rootfs.tar":
                name = "rootfs_handoff"
            rows.append(("artifact", name, filename, str(size(file)), digest(file), origin))
        manifest = stage / "boot-manifest.tsv"
        manifest.write_text("".join("\t".join(row) + "\n" for row in rows))
        verify_bundle(stage, quiet=True)
        require(not IMAGE_OUT.exists(), "static output appeared during composition")
        stage.rename(IMAGE_OUT)
    print(f"PASS pixel-9a-static-boot composed, hash-recorded, unbooted images: {IMAGE_OUT}")


def verify_bundle(bundle: Path = IMAGE_OUT, quiet: bool = False,
                  native_avb: bool = True) -> None:
    """Recheck a transferred static bundle and optionally run native AVB tools."""
    require(bundle.is_dir() and not bundle.is_symlink(), "static image bundle is missing or a symlink")
    manifest = tsv(bundle / "boot-manifest.tsv")
    expected_scalars = {
        "schema_version": "1", "target_id": TARGET_ID, "codename": "tegu",
        "hardware_revision": "MP1.0", "route_id": STATIC_ROUTE,
        "source_route_id": SOURCE_ROUTE, "kernel_backend": "linux-google-tegu-wwan",
        "status": "composed-not-booted", "bootable": "false", "header_version": "4",
        "partition_contract": "complete-native-static-builtins",
        "rootfs_mapping_status": "pending-live-map-and-separate-wipe-confirmation",
        "logical_partition_writes": "false", "active_slot_writes": "false",
        "avb_mode": "fresh-generated", "avb_algorithm": "NONE",
        "avb_descriptor_inputs": "fresh-avbtool-hash-footers-from-final-static-images",
        "avb_binding_salt": "sha256-of-pre-footer-image-bytes",
    }
    for key, value in expected_scalars.items():
        scalar(manifest, key, value)
    route = tomllib.loads(regular(ROUTE, "radio route").read_text())
    require(route.get("native_static_artifact_contract_sha256") == digest(CONTRACT),
            "checked-in static contract hash differs from route pin")
    scalar(manifest, "source_revision", route["source_revision"])
    provenance_names = {
        "static_contract": "native-static-artifact-contract.toml",
        "route_metadata": "kernel-radio-route.toml",
        "kernel_manifest": "kernel-manifest.tsv",
        "rootfs_manifest": "rootfs-manifest.tsv",
        "boot_inputs_manifest": "boot-inputs-manifest.tsv",
        "partition_capacities": "partition-capacities.tsv",
        "kernel_config": "kernel.config",
    }
    for key, filename in provenance_names.items():
        row = one(manifest, key, 4)
        require(row[1] == f"provenance/{filename}", f"{key} provenance path differs")
        checked(bundle / row[1], row[2], row[3], key)
    for name in BOOT_INPUT_NAMES:
        key = "boot_input_" + name.replace(".", "_")
        row = one(manifest, key, 4)
        require(row[1] == f"provenance/{name}", f"{name} provenance path differs")
        checked(bundle / row[1], row[2], row[3], name)
    require((bundle / "provenance/vendor-cmdline").read_text().strip() ==
            render_vendor_cmdline(TARGET / "pmos/device-google-tegu/kernel-cmdline.conf"),
            "bundled vendor command line differs from checked-in source")
    require(digest(bundle / "provenance/native-static-artifact-contract.toml") == digest(CONTRACT),
            "bundle static contract differs from checked-in contract")
    require(digest(bundle / "provenance/kernel-radio-route.toml") == digest(ROUTE),
            "bundle radio route differs from checked-in route")
    contract = tomllib.loads(regular(CONTRACT, "static contract").read_text())
    config = (bundle / "provenance/kernel.config").read_text()
    config_lines = set(config.splitlines())
    for symbol in contract["builtins"]["required"]:
        require(symbol in config_lines, f"bundled kernel config lacks built-in symbol: {symbol}")
    kernel = tsv(bundle / "provenance/kernel-manifest.tsv")
    for key, value in (("schema_version", "1"), ("target_id", TARGET_ID),
                       ("route_id", SOURCE_ROUTE), ("build_system", "kbuild-make"),
                       ("compiler_mode", "llvm"),
                       ("source_revision", route["source_revision"]),
                       ("kernel_backend", "linux-google-tegu-wwan"),
                       ("status", "built-not-booted"), ("bootable", "false")):
        scalar(kernel, key, value)
    require(named(kernel, "split_artifact_contract", route["split_artifact_contract"], 3)[2]
            == route["split_artifact_contract_sha256"], "bundled kernel manifest names a stale split contract")
    archive = one(kernel, "source_archive", 5)
    require(archive[2:] == (str(route["source_archive_bytes"]), route["source_archive_sha256"],
                            route["source_archive_sha512"]), "bundled source archive pins differ")
    rootfs = tsv(bundle / "provenance/rootfs-manifest.tsv")
    for key, value in (("schema_version", "1"), ("target_id", TARGET_ID),
                       ("codename", "tegu"), ("hardware_revision", "MP1.0"),
                       ("substrate", "postmarketos"), ("route_id", SOURCE_ROUTE),
                       ("kernel_backend", "linux-google-tegu-wwan"),
                       ("status", "built-substrate-only"), ("bootable", "false")):
        scalar(rootfs, key, value)
    boot_inputs = tsv(bundle / "provenance/boot-inputs-manifest.tsv")
    for key, value in (("schema_version", "1"), ("target_id", TARGET_ID),
                       ("route_id", SOURCE_ROUTE),
                       ("kernel_backend", "linux-google-tegu-wwan"),
                       ("status", "built-source-inputs"), ("bootable", "false")):
        scalar(boot_inputs, key, value)
    for key, filename in (("kernel_manifest", "kernel-manifest.tsv"),
                          ("rootfs_manifest", "rootfs-manifest.tsv")):
        row = one(boot_inputs, key, 4)
        checked(bundle / "provenance" / filename, row[2], row[3], f"boot-input {key}")
    for name in BOOT_INPUT_NAMES:
        source = named(boot_inputs, "artifact", name, 6)
        built = one(manifest, "boot_input_" + name.replace(".", "_"), 4)
        require(source[3:5] == built[2:4], f"bundled {name} differs from boot-input handoff")
        if name.endswith(".cpio"):
            no_modules_in_cpio(bundle / "provenance" / name, name)
    capacities = capacity_handoff(bundle / "provenance/partition-capacities.tsv")
    capacity_rows = [row for row in manifest if row[0] == "partition_capacity"]
    require(len(capacity_rows) == len(PARTITIONS), "static manifest has wrong capacity row count")
    for partition in PARTITIONS:
        row = named(manifest, "partition_capacity", partition, 3)
        require(row[2] == str(capacities[partition]), f"{partition} manifest capacity differs")
    filenames = {"Image": "Image", "dtb.img": "dtb.img",
                 **{partition: f"{partition}.img" for partition in PARTITIONS},
                 "rootfs_handoff": "rootfs.tar", "rootfs_image": "rootfs.img"}
    origins = {"Image": "kernel", "dtb.img": "kernel", "rootfs_handoff": "rootfs",
               "rootfs_image": "rootfs", "dtbo": "generated-mkdtboimg",
               "vbmeta": "generated-avbtool-from-final-images",
               "vbmeta_vendor": "generated-avbtool-from-final-images",
               "vbmeta_system": "generated-avbtool-from-final-images"}
    artifact_rows = [row for row in manifest if row[0] == "artifact"]
    require(len(artifact_rows) == len(filenames), "static manifest has wrong artifact row count")
    for name, filename in filenames.items():
        row = named(manifest, "artifact", name, 6)
        require(row[2] == filename and row[5] == origins.get(name, "generated-mkbootimg"),
                f"{name} artifact path or origin differs")
        checked(bundle / filename, row[3], row[4], name)
        if name in PARTITIONS:
            require(size(bundle / filename) <= capacities[name], f"{name} image exceeds live capacity")
    for partition in ("boot", "init_boot", "vendor_boot", "vendor_kernel_boot"):
        expected_magic = b"ANDROID!" if partition in ("boot", "init_boot") else b"VNDRBOOT"
        require((bundle / f"{partition}.img").open("rb").read(8) == expected_magic,
                f"{partition} has the wrong boot-container magic")
    require_vendor_layout(bundle / "vendor_boot.img", 0,
                          (bundle / "provenance/vendor-cmdline").read_text().strip())
    require_vendor_layout(bundle / "vendor_kernel_boot.img", size(bundle / "dtb.img"), "")
    for source_rows, source_name, final_name in ((kernel, "Image", "Image"),
                                                (kernel, "dtb.img", "dtb.img"),
                                                (rootfs, "rootfs.tar", "rootfs_handoff"),
                                                (rootfs, "rootfs.img", "rootfs_image")):
        source = named(source_rows, "artifact", source_name, 4)
        final = named(manifest, "artifact", final_name, 6)
        require(source[2:4] == final[3:5], f"{final_name} differs from source handoff")
    bound_rows = [row for row in manifest if row[0] == "avb_bound_image"]
    require(len(bound_rows) == len(PAYLOADS), "static manifest has wrong AVB binding row count")
    for name in PAYLOADS:
        binding = named(manifest, "avb_bound_image", name, 5)
        image = named(manifest, "artifact", name, 6)
        require(binding[2:] == image[2:5], f"{name} AVB binding differs from final artifact")
    descriptor_sets = {"vbmeta": {"boot", "init_boot"},
                       "vbmeta_vendor": {"vendor_boot", "vendor_kernel_boot", "dtbo"},
                       "vbmeta_system": set()}
    require(len([row for row in manifest if row[0] == "avb_descriptor_set"]) == 3,
            "static manifest has wrong AVB descriptor-set count")
    avbtool = tool("avbtool") if native_avb else None
    for vbmeta, expected in descriptor_sets.items():
        row = named(manifest, "avb_descriptor_set", vbmeta, 3)
        require(row[2] == (",".join(sorted(expected, key=PARTITIONS.index)) or "empty"),
                f"{vbmeta} manifest descriptor set differs")
        image = bundle / f"{vbmeta}.img"
        for key, suffix in (("avb_verification", "verification"), ("avb_info", "info")):
            record = named(manifest, key, f"provenance/{vbmeta}-{suffix}.txt", 4)
            checked(bundle / record[1], record[2], record[3], f"{vbmeta} {suffix}")
        if avbtool is not None:
            run(avbtool, "verify_image", "--image", str(image), capture=True)
            info = run(avbtool, "info_image", "--image", str(image), capture=True)
        else:
            info = (bundle / f"provenance/{vbmeta}-info.txt").read_bytes()
        names = re.findall(rb"^\s*Partition Name:\s*(\S+)", info, re.MULTILINE)
        decoded = tuple(name.decode("ascii") for name in names)
        require(len(decoded) == len(expected) and set(decoded) == expected,
                f"{vbmeta} actual AVB descriptors differ from final static images")
    if not quiet:
        scope = "bundle and AVB payloads" if native_avb else "transferred bundle hashes and recorded AVB metadata"
        print(f"PASS pixel-9a-static-boot verified {scope}: {bundle}")


def main() -> int:
    if len(sys.argv) != 2 or sys.argv[1] not in ("preflight", "build", "verify", "verify-transfer"):
        print("usage: static-boot.py preflight|build|verify|verify-transfer", file=sys.stderr)
        return 2
    try:
        if sys.argv[1] == "verify":
            verify_bundle()
        elif sys.argv[1] == "verify-transfer":
            verify_bundle(native_avb=False)
        else:
            inputs = validate()
            if sys.argv[1] == "preflight":
                print("PASS pixel-9a-static-boot preflight: native source, built-ins, empty selected module sets, and eight capacities verified")
            else:
                build(inputs)
    except Refusal as error:
        print(f"FAIL pixel-9a-static-boot: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
