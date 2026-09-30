#!/usr/bin/env python3
"""Exact-target admission, gated kernel probe, and observation for the static route.

This host-only target workflow reuses static-boot.py's strict TOML/TSV and
hash verifier. Python is limited to target tooling; no guest or product code
depends on it. Only `boot-temp` can change device boot state, after a typed
per-run confirmation. Neither mode writes a partition or selects a slot.
"""

from __future__ import annotations

import hashlib
import importlib.util
import os
import re
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / "targets/boards/google-pixel-9a-tegu-gxq96-mp1-0"
CANDIDATE = TARGET / "static-install-candidate.toml"
OUT = ROOT / "out/pixel-9a"
BOOT_PARTITIONS = (
    "boot", "init_boot", "vendor_boot", "vendor_kernel_boot",
    "dtbo", "vbmeta", "vbmeta_vendor", "vbmeta_system",
)
TARGET_ID = "google-pixel-9a-tegu-gxq96-mp1-0"
ROUTE_ID = "experimental-linux-google-tegu-s5300-wwan-static"
DATA_TOKEN = "I-understand-this-is-a-new-development-device-with-no-personal-data"
KERNEL_FAILURE = re.compile(r"Kernel panic|Oops:|BUG:|watchdog.*lockup|soft lockup|hard LOCKUP",
                            re.IGNORECASE)

spec = importlib.util.spec_from_file_location("pixel9a_static_boot", ROOT / "tools/pixel-9a/static-boot.py")
assert spec is not None and spec.loader is not None
static = importlib.util.module_from_spec(spec)
sys.dont_write_bytecode = True
spec.loader.exec_module(static)


class Refusal(Exception):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise Refusal(message)


def sha(path: Path) -> str:
    require(path.is_file() and not path.is_symlink(), f"missing or linked file: {path}")
    return static.digest(path)


def pinned_path(relative: str, base: Path) -> Path:
    path = ROOT / static.relative(relative)
    require(path.resolve().is_relative_to(base.resolve()), f"path outside reviewed output: {relative}")
    return path


def read_candidate() -> tuple[dict, Path, Path, Path]:
    config = tomllib.loads(CANDIDATE.read_text())
    require(config.get("status") != "probe-unobserved",
            "candidate blocked after unobserved temporary probe; build and review a new attempt")
    expected = {
        "schema_version": 1, "target_id": TARGET_ID, "product": "tegu",
        "sku": "GXQ96", "hardware_revision": "MP1.0", "route_id": ROUTE_ID,
        "status": "composed-not-booted", "stock_slot": "a", "inactive_slot": "b",
        "rootfs_partition": "userdata", "rootfs_image_bytes": 2147483648,
        "recovery_build": "CP3A.260905.009",
        "fastboot_cli_version": "37.0.1-15733141",
    }
    for key, value in expected.items():
        require(config.get(key) == value, f"candidate {key} differs from reviewed value")
    policy = config.get("write_policy", {})
    require(policy == {
        "stock_slot_writes": False, "bootloader_or_radio_writes": False,
        "logical_partition_writes": False, "slot_selection": False,
        "temporary_boot_before_persistent_write": True,
        "userdata_write_requires_separate_confirmation": True,
    }, "candidate write policy differs")
    for key in ("manifest_sha256", "static_capacity_sha256", "adb_discovery_sha256",
                "userdata_map_sha256", "rootfs_image_sha256"):
        require(re.fullmatch(r"[0-9a-f]{64}", config.get(key, "")) is not None,
                f"candidate {key} is malformed")
    bundle = pinned_path(config["bundle"], OUT / "images")
    discovery = pinned_path(config["adb_discovery"], OUT)
    rootfs_map = pinned_path(config["userdata_map"], OUT)
    require(sha(bundle / "boot-manifest.tsv") == config["manifest_sha256"],
            "static bundle manifest differs from checked-in candidate")
    require(sha(discovery) == config["adb_discovery_sha256"],
            "ADB discovery differs from checked-in candidate")
    require(sha(rootfs_map) == config["userdata_map_sha256"],
            "userdata map differs from checked-in candidate")
    try:
        static.verify_bundle(bundle, quiet=True, native_avb=False)
    except static.Refusal as error:
        raise Refusal(f"static bundle transfer verifier refused: {error}") from error
    manifest = static.tsv(bundle / "boot-manifest.tsv")
    capacities = static.one(manifest, "partition_capacities", 4)
    require(capacities[1] == "provenance/partition-capacities.tsv" and
            capacities[3] == config["static_capacity_sha256"],
            "bundle capacity evidence differs from candidate")
    rootfs = static.named(manifest, "artifact", "rootfs_image", 6)
    require(rootfs[3:5] == (str(config["rootfs_image_bytes"]), config["rootfs_image_sha256"]),
            "rootfs image differs from candidate")
    return config, bundle, discovery, rootfs_map


def key_values(path: Path, wanted: set[str] | None = None) -> dict[str, str]:
    values: dict[str, str] = {}
    for line in path.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        if "=" not in line:
            require(wanted is not None, f"malformed evidence line in {path.name}")
            continue
        key, value = line.split("=", 1)
        if wanted is not None and key not in wanted:
            continue
        require(key not in values, f"duplicate evidence key {key} in {path.name}")
        values[key] = value
    return values


def evidence_path(value: str, directory: Path, label: str) -> Path:
    require(bool(value), f"{label} path is required")
    path = Path(value)
    require(path.is_file() and not path.is_symlink() and
            path.resolve().is_relative_to(directory.resolve()),
            f"{label} must be a regular file under {directory}")
    return path


def boot_evidence(path: Path, prefix: str, boot_hash: str) -> None:
    values = key_values(path)
    require(set(values) == {"operation", "target_id", "serial_sha256_prefix",
                            "boot_img_sha256", "boot_exit_status", "result", "output"},
            "temporary-boot transcript fields differ from reviewed contract")
    expected = {
        "operation": "pixel-9a-static-boot-temp", "target_id": TARGET_ID,
        "serial_sha256_prefix": prefix, "boot_img_sha256": boot_hash,
        "boot_exit_status": "0", "result": "temporary-boot-issued",
    }
    for key, value in expected.items():
        require(values[key] == value, f"temporary-boot transcript {key} differs")


def image_kernel_banner(boot: Path) -> str:
    """Extract the complete built kernel version from the pinned boot image."""
    image = boot.read_bytes()
    banners = {
        match.group().decode("ascii")
        for match in re.finditer(rb"Linux version [^\x00\r\n]{1,400}", image)
        if re.search(rb"\) #[0-9]+ (?:SMP|PREEMPT)", match.group())
    }
    require(len(banners) == 1, "boot image lacks one unambiguous complete kernel banner")
    return next(iter(banners))


def kernel_capture(path: Path, boot: Path) -> str:
    try:
        content = path.read_text(encoding="utf-8")
    except UnicodeError as error:
        raise Refusal("temporary-boot kernel capture must be UTF-8 text") from error
    require(image_kernel_banner(boot) in content,
            "temporary-boot observation lacks the pinned boot image kernel banner")
    require("ZUMA PRO TEGU" in content,
            "temporary-boot observation has no tegu machine record")
    require(KERNEL_FAILURE.search(content) is None,
            "temporary-boot observation contains a kernel failure; stop and review recovery")
    return content


def capture_provenance(content: str, prefix: str, boot_hash: str,
                       transcript_hash: str) -> None:
    if not content.startswith("capture_source="):
        return  # A separately reviewed raw kernel log has no generated header.
    fields = {}
    for line in content.splitlines():
        if line.startswith("Linux version "):
            break
        require("=" in line, "generated kernel capture header is malformed")
        key, value = line.split("=", 1)
        require(key not in fields, "generated kernel capture header has a duplicate field")
        fields[key] = value
    require(fields == {
        "capture_source": "adb-procfs-read-only", "target_id": TARGET_ID,
        "serial_sha256_prefix": prefix, "boot_img_sha256": boot_hash,
        "boot_transcript_sha256": transcript_hash,
    }, "generated kernel capture differs from reviewed boot provenance")


def observation_evidence(path: Path, transcript: Path, prefix: str,
                         boot: Path, boot_hash: str) -> None:
    values = key_values(path)
    require(set(values) == {"operation", "target_id", "serial_sha256_prefix",
                            "boot_img_sha256", "boot_transcript_sha256",
                            "kernel_log", "kernel_log_bytes", "kernel_log_sha256", "result"},
            "temporary-boot observation fields differ from reviewed contract")
    expected = {
        "operation": "pixel-9a-static-observe-temp", "target_id": TARGET_ID,
        "serial_sha256_prefix": prefix, "boot_img_sha256": boot_hash,
        "boot_transcript_sha256": sha(transcript),
        "result": "temporary-boot-observed",
    }
    for key, value in expected.items():
        require(values[key] == value, f"temporary-boot observation {key} differs")
    name = values["kernel_log"]
    require(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", name) is not None,
            "temporary-boot observation kernel log filename is unsafe")
    log = evidence_path(str(path.parent / name), path.parent, "observation kernel log")
    require(values["kernel_log_bytes"].isdigit() and
            log.stat().st_size == int(values["kernel_log_bytes"]) and
            sha(log) == values["kernel_log_sha256"],
            "temporary-boot observation kernel log size or hash differs")
    require(sha(boot) == boot_hash, "observation boot image hash differs")
    capture_provenance(kernel_capture(log, boot), prefix, boot_hash, sha(transcript))


def discovery_prefix(path: Path) -> str:
    values = key_values(path)
    expected = {
        "transport": "adb", "result": "ok", "ro.product.manufacturer": "Google",
        "ro.product.model": "Pixel 9a", "ro.product.device": "tegu",
        "ro.boot.hardware.revision": "MP1.0", "ro.boot.hardware.sku": "GXQ96",
        "ro.boot.slot_suffix": "_a", "ro.boot.vbmeta.device_state": "unlocked",
        "ro.build.version.release": "17",
    }
    for key, value in expected.items():
        require(values.get(key) == value, f"ADB discovery {key} differs")
    prefix = values.get("serial_sha256_prefix", "")
    require(re.fullmatch(r"[0-9a-f]{12}", prefix) is not None,
            "ADB discovery has no redacted serial hash")
    return prefix


def rootfs_mapping(path: Path, serial_prefix: str, candidate: dict) -> int:
    rows = static.tsv(path)
    for key, value in (("schema_version", "1"), ("target_id", TARGET_ID),
                       ("partition_contract", "rootfs-userdata-only"),
                       ("serial_sha256_prefix", serial_prefix),
                       ("current_slot", "a"), ("rootfs_partlabel", "userdata")):
        require(static.one(rows, key, 2)[1] == value, f"userdata map {key} differs")
    partition = static.one(rows, "partition", 4)
    require(partition[1] == "userdata" and partition[3] == "/dev/block/sda34" and
            partition[2].isdigit() and int(partition[2]) > candidate["rootfs_image_bytes"],
            "userdata map partition differs from reviewed mapping")
    inventory = static.one(rows, "adb_inventory", 4)
    inventory_path = pinned_path(inventory[1], OUT)
    require(inventory_path.stat().st_size == int(inventory[2]) and
            sha(inventory_path) == inventory[3], "map source ADB inventory differs")
    values = key_values(inventory_path, {"target_id", "transport", "serial_sha256_prefix", "result"})
    require(values.get("target_id") == TARGET_ID and values.get("transport") == "adb" and
            values.get("serial_sha256_prefix") == serial_prefix and
            values.get("result") == "ok", "map source ADB inventory identity differs")
    return int(partition[2])


def command(*args: str, timeout: int = 30) -> str:
    try:
        done = subprocess.run(args, capture_output=True, text=True, timeout=timeout, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise Refusal(f"command unavailable or timed out: {args[0]}: {error}") from error
    require(done.returncode == 0, f"{args[0]} command failed with exit {done.returncode}: {done.stderr.strip()}")
    return done.stdout + done.stderr


def attached_fastboot() -> str:
    adb = command("adb", "devices", "-l")
    adb_rows = [line for line in adb.splitlines() if line.strip() and not line.startswith("List of devices")]
    fastboot = command("fastboot", "devices", "-l")
    fastboot_rows = [line.split() for line in fastboot.splitlines() if line.strip()]
    require(not adb_rows and len(fastboot_rows) == 1 and len(fastboot_rows[0]) >= 2 and
            fastboot_rows[0][1] == "fastboot", "requires exactly one Fastboot device and no ADB device")
    return fastboot_rows[0][0]


def getvar(serial: str, name: str) -> str:
    output = command("fastboot", "-s", serial, "getvar", name)
    require("FAILED" not in output, f"Fastboot reported failure for {name}")
    pattern = re.compile(rf"^(?:\(bootloader\)\s*)?{re.escape(name)}:\s*(.*?)\s*$")
    values = [match.group(1) for line in output.splitlines() if (match := pattern.fullmatch(line.strip()))]
    require(len(values) == 1 and bool(values[0]), f"Fastboot {name} is missing or ambiguous")
    return values[0]


def recovery() -> tuple[Path, Path]:
    pins = [line.split("\t") for line in (TARGET / "recovery.pins").read_text().splitlines()
            if line and not line.startswith("#")]
    require(len(pins) == 2 and {row[0] for row in pins} == {"factory", "ota"},
            "stock recovery pins differ")
    result: dict[str, Path] = {}
    for row in pins:
        require(len(row) == 8 and row[1] == "CP3A.260905.009" and
                re.fullmatch(r"[0-9a-f]{64}", row[5]) is not None and row[6].isdigit(),
                "stock recovery pin is malformed")
        path = TARGET / "recovery" / static.relative(row[3])
        require(path.is_file() and not path.is_symlink() and
                path.stat().st_size == int(row[6]) and sha(path) == row[5],
                f"stock {row[0]} recovery artifact differs")
        result[row[0]] = path
    return result["factory"], result["ota"]


def gates() -> str:
    target = tomllib.loads((TARGET / "manifest.toml").read_text())
    require(target.get("target_id") == TARGET_ID and
            target.get("recovery_verified_utc") == "2026-09-30T01:02:38Z",
            "target or tested stock restore differs")
    values = target["gates"]
    for key, expected in (("identity", "passed"), ("backup", "not-required-dev-device"),
                          ("data_policy", "dev-device-no-user-data"),
                          ("recovery_procedure", "tested"),
                          ("per_run_confirmation", "required")):
        require(values.get(key) == expected, f"target {key} gate differs")
    require(values.get("image_compatibility") in ("partial", "passed", "verified"),
            "target image compatibility gate is not a reviewed value")
    return values["image_compatibility"]


def live(candidate: dict, discovery: Path, rootfs_map: Path,
         capacities: dict[str, int]) -> tuple[str, str, int]:
    serial = attached_fastboot()
    prefix = hashlib.sha256(serial.encode()).hexdigest()[:12]
    require(discovery_prefix(discovery) == prefix, "live Fastboot serial lacks matching post-restore Android identity")
    expected = {
        "product": "tegu", "hw-revision": "MP1.0", "current-slot": "a",
        "slot-count": "2", "unlocked": "yes", "secure": "yes",
        "is-userspace": "no", "snapshot-update-status": "none",
    }
    for name, value in expected.items():
        require(getvar(serial, name) == value, f"live Fastboot {name} differs from reviewed value")
    variant = getvar(serial, "variant")
    require(variant in ("GXQ96", "N/A", "NA", "unknown", "unavailable"),
            "Fastboot variant differs from GXQ96 or reviewed unavailable values")
    serialno = getvar(serial, "serialno")
    require(serialno == serial, "Fastboot serialno differs from transport serial")
    for partition in BOOT_PARTITIONS:
        require(getvar(serial, f"has-slot:{partition}") == "yes",
                f"{partition} is not an A/B partition")
        for slot in ("a", "b"):
            target = f"{partition}_{slot}"
            require(getvar(serial, f"is-logical:{target}") == "no",
                    f"{target} is not verified physical")
            observed = int(getvar(serial, f"partition-size:{target}"), 0)
            require(observed == capacities[partition], f"{target} capacity changed")
    mapped_size = rootfs_mapping(rootfs_map, prefix, candidate)
    require(getvar(serial, "has-slot:userdata") == "no" and
            getvar(serial, "is-logical:userdata") == "no" and
            getvar(serial, "partition-type:userdata") == "raw",
            "live userdata is not verified unslotted physical storage")
    require(int(getvar(serial, "partition-size:userdata"), 0) == mapped_size,
            "live userdata capacity differs from targeted map")
    return serial, prefix, mapped_size


def preflight() -> tuple[dict, Path, str, str, Path]:
    candidate, bundle, discovery, rootfs_map = read_candidate()
    manifest = static.tsv(bundle / "boot-manifest.tsv")
    capacities = {name: int(static.named(manifest, "partition_capacity", name, 3)[2])
                  for name in BOOT_PARTITIONS}
    compatibility = gates()
    factory, ota = recovery()
    serial, prefix, userdata_bytes = live(candidate, discovery, rootfs_map, capacities)
    lines: list[str] = []

    def emit(line: str) -> None:
        lines.append(line)
        print(line)

    emit(f"transport=fastboot serial={serial} serial_sha256_prefix={prefix}")
    emit("manufacturer=Google model=Pixel 9a product=tegu sku=GXQ96 hardware_revision=MP1.0")
    emit("current_slot=a target_inactive_slot=b is_userspace=no unlocked=yes")
    emit(f"static_manifest_sha256={candidate['manifest_sha256']} image_compatibility={compatibility}")
    emit(f"recovery_factory={factory.name} sha256={sha(factory)}")
    emit(f"recovery_ota={ota.name} sha256={sha(ota)}")
    for name in BOOT_PARTITIONS:
        row = static.named(manifest, "artifact", name, 6)
        emit(f"plan=partition_target={name}_b input_image_sha256={row[4]} bytes={row[3]}")
    emit(f"plan=partition_target=userdata input_image_sha256={candidate['rootfs_image_sha256']} "
         f"bytes={candidate['rootfs_image_bytes']} live_capacity={userdata_bytes} separate_confirmation=required")
    emit("plan=partition_target=none input_image_sha256=none operation=temporary-kernel-probe-only")
    emit("preflight=passed-read-only bootable=false persistent_flash=not-authorized")
    evidence_dir = OUT / "flash-static"
    evidence_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    transcript = evidence_dir / f"preflight-{stamp}-{os.getpid()}.txt"
    with transcript.open("x") as stream:
        stream.write("# Pixel 9a static route read-only preflight; serial redacted. Never commit.\n")
        stream.write("\n".join(line.replace(serial, f"serial-sha256:{prefix}") for line in lines) + "\n")
    print(f"transcript={transcript}")
    return candidate, bundle, serial, prefix, factory


def boot_temp() -> None:
    _, bundle, serial, prefix, _ = preflight()
    require(os.environ.get("FERRIX_PIXEL9A_NO_USER_DATA") == DATA_TOKEN,
            "exact development-device data policy token is required for a temporary boot")
    boot = bundle / "boot.img"
    boot_hash = sha(boot)
    phrase = f"CONFIRM PIXEL 9A STATIC TEMPORARY BOOT boot.img@{boot_hash[:12]}"
    print("The next command sends this exact kernel-only image to the device; no partition or slot is changed.")
    print("This probes kernel progress with the current stock init_boot/vendor_boot and does not validate the full Ferrix image set.")
    print(f"Type exactly: {phrase}", flush=True)
    entered = input("> ")
    require(entered == phrase, "confirmation did not match; no device operation performed")
    # Repeat every read-only device, capacity, image and recovery gate after
    # confirmation, before sending any bytes to the bootloader.
    _, checked_bundle, checked_serial, checked_prefix, _ = preflight()
    require(checked_bundle == bundle and checked_serial == serial and
            checked_prefix == prefix and sha(boot) == boot_hash,
            "identity or boot image changed before temporary boot")
    print(f"mutation=fastboot boot exact_serial={serial} partition_target=none input_image_sha256={boot_hash}", flush=True)
    output = command("fastboot", "-s", serial, "boot", str(boot), timeout=180)
    require("FAILED" not in output, "bootloader reported a temporary-boot failure")
    transcript_dir = OUT / "flash-static"
    transcript_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    transcript = transcript_dir / f"boot-temp-{stamp}-{os.getpid()}.txt"
    redacted_output = output.replace(serial, "[redacted]").strip().replace("\n", "\\n")
    with transcript.open("x") as stream:
        stream.write(f"operation=pixel-9a-static-boot-temp\ntarget_id={TARGET_ID}\n"
                     f"serial_sha256_prefix={prefix}\nboot_img_sha256={boot_hash}\n"
                     f"boot_exit_status=0\nresult=temporary-boot-issued\n"
                     f"output={redacted_output}\n")
    print(f"result=temporary-boot-issued transcript={transcript}")


def capture_temp() -> None:
    """Read live Android procfs after a temporary kernel boot, if ADB returns."""
    _, bundle, discovery, _ = read_candidate()
    prefix = discovery_prefix(discovery)
    boot = bundle / "boot.img"
    boot_hash = sha(boot)
    evidence_dir = OUT / "flash-static"
    transcript = evidence_path(
        os.environ.get("FERRIX_PIXEL9A_STATIC_TEMP_BOOT_EVIDENCE", ""),
        evidence_dir, "temporary-boot transcript")
    boot_evidence(transcript, prefix, boot_hash)
    transcript_hash = sha(transcript)
    adb = command("adb", "devices", "-l")
    rows = [line.split() for line in adb.splitlines()
            if line.strip() and not line.startswith("List of devices")]
    fastboot = command("fastboot", "devices", "-l")
    require(len(rows) == 1 and len(rows[0]) >= 2 and rows[0][1] == "device" and
            not fastboot.strip(), "requires exactly one authorized ADB device and no Fastboot device")
    serial = rows[0][0]
    require(hashlib.sha256(serial.encode()).hexdigest()[:12] == prefix,
            "ADB serial differs from the post-restore discovery and temporary-boot transcript")
    expected = {
        "ro.product.manufacturer": "Google", "ro.product.model": "Pixel 9a",
        "ro.product.device": "tegu", "ro.boot.hardware.revision": "MP1.0",
        "ro.boot.hardware.sku": "GXQ96", "ro.boot.slot_suffix": "_a",
        "ro.boot.vbmeta.device_state": "unlocked", "ro.build.version.release": "17",
    }
    for property_name, value in expected.items():
        observed = command("adb", "-s", serial, "shell", "getprop", property_name).strip()
        require(observed == value, f"post-probe ADB {property_name} differs from reviewed identity")
    version = command("adb", "-s", serial, "shell", "cat", "/proc/version").strip()
    require("\n" not in version and 0 < len(version) <= 400,
            "live /proc/version is missing or malformed")
    expected_version = image_kernel_banner(boot)
    if version != expected_version:
        require(sha(boot) == boot_hash and sha(transcript) == transcript_hash,
                "image or boot transcript changed while collecting mismatch evidence")
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
        mismatch = evidence_dir / f"capture-mismatch-{stamp}-{os.getpid()}.txt"
        with mismatch.open("x", encoding="utf-8") as stream:
            stream.write(f"capture_source=adb-procfs-read-only\n"
                         f"target_id={TARGET_ID}\nserial_sha256_prefix={prefix}\n"
                         f"boot_img_sha256={boot_hash}\n"
                         f"boot_transcript_sha256={transcript_hash}\n"
                         f"expected_banner_sha256={hashlib.sha256(expected_version.encode()).hexdigest()}\n"
                         f"observed_banner_sha256={hashlib.sha256(version.encode()).hexdigest()}\n"
                         "result=kernel-banner-mismatch\n")
        raise Refusal(f"live /proc/version differs from the pinned boot image kernel; "
                      f"diagnostic={mismatch}")
    try:
        model = command("adb", "-s", serial, "shell", "cat", "/proc/device-tree/model").strip("\x00\r\n ")
    except Refusal as error:
        if "Permission denied" not in str(error):
            raise
        model = None
    require(sha(boot) == boot_hash and sha(transcript) == transcript_hash and
            hashlib.sha256(serial.encode()).hexdigest()[:12] == prefix,
            "image or identity changed while collecting temporary-boot evidence")
    if model is None:
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
        partial = evidence_dir / f"capture-partial-{stamp}-{os.getpid()}.txt"
        with partial.open("x", encoding="utf-8") as stream:
            stream.write(f"capture_source=adb-procfs-version-only\n"
                         f"target_id={TARGET_ID}\nserial_sha256_prefix={prefix}\n"
                         f"boot_img_sha256={boot_hash}\n"
                         f"boot_transcript_sha256={transcript_hash}\n"
                         "device_tree_model=unavailable-permission-denied\n"
                         f"{version}\n")
        print(f"result=live-kernel-banner-matched-model-unavailable "
              f"capture={partial} sha256={sha(partial)}")
        print("This partial capture cannot satisfy the temporary-boot observation gate.")
        return
    require(model == "ZUMA PRO TEGU MP on ZUMA PRO",
            "live device tree model differs from pinned tegu board")
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    capture = evidence_dir / f"capture-temp-{stamp}-{os.getpid()}.txt"
    with capture.open("x", encoding="utf-8") as stream:
        stream.write(f"capture_source=adb-procfs-read-only\n"
                     f"target_id={TARGET_ID}\nserial_sha256_prefix={prefix}\n"
                     f"boot_img_sha256={boot_hash}\n"
                     f"boot_transcript_sha256={transcript_hash}\n"
                     f"{version}\n{model}\n")
    kernel_capture(capture, boot)
    print(f"result=live-kernel-banner-matched source=adb-procfs-read-only "
          f"capture={capture} sha256={sha(capture)}")
    print("The running kernel's build banner and device-tree model match the pinned candidate. "
          "This does not prove every loaded image byte, Ferrix userspace, absence of earlier "
          "kernel errors, or hardware capability.")


def observe_temp() -> None:
    _, bundle, serial, prefix, _ = preflight()
    boot_hash = sha(bundle / "boot.img")
    evidence_dir = OUT / "flash-static"
    transcript = evidence_path(
        os.environ.get("FERRIX_PIXEL9A_STATIC_TEMP_BOOT_EVIDENCE", ""),
        evidence_dir, "temporary-boot transcript")
    capture = evidence_path(
        os.environ.get("FERRIX_PIXEL9A_STATIC_TEMP_BOOT_KERNEL_LOG", ""),
        OUT, "temporary-boot kernel capture")
    boot_evidence(transcript, prefix, boot_hash)
    transcript_hash = sha(transcript)
    content = kernel_capture(capture, bundle / "boot.img")
    capture_provenance(content, prefix, boot_hash, transcript_hash)
    capture_hash = sha(capture)
    phrase = f"CONFIRM PIXEL 9A STATIC KERNEL PROGRESS OBSERVED boot.img@{boot_hash[:12]}"
    print("Confirm this capture came from the displayed device and temporary boot, "
          "shows kernel progress, and has no hang or reboot loop.")
    print("This records kernel progress only; it does not establish Ferrix userspace or radios.")
    print(f"Type exactly: {phrase}", flush=True)
    require(input("> ") == phrase, "observation confirmation did not match")
    _, checked_bundle, checked_serial, checked_prefix, _ = preflight()
    require(checked_bundle == bundle and checked_serial == serial and checked_prefix == prefix and
            sha(bundle / "boot.img") == boot_hash and sha(transcript) == transcript_hash and
            sha(capture) == capture_hash,
            "device, boot image, or observation input changed before recording")
    boot_evidence(transcript, prefix, boot_hash)
    capture_provenance(kernel_capture(capture, bundle / "boot.img"),
                       prefix, boot_hash, transcript_hash)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    redacted_log = evidence_dir / f"observe-temp-{stamp}-{os.getpid()}-kernel.txt"
    redacted_log.write_text(content.replace(serial, f"serial-sha256:{prefix}"),
                            encoding="utf-8")
    observation = evidence_dir / f"observe-temp-{stamp}-{os.getpid()}.txt"
    with observation.open("x") as stream:
        stream.write(f"operation=pixel-9a-static-observe-temp\ntarget_id={TARGET_ID}\n"
                     f"serial_sha256_prefix={prefix}\nboot_img_sha256={boot_hash}\n"
                     f"boot_transcript_sha256={transcript_hash}\n"
                     f"kernel_log={redacted_log.name}\n"
                     f"kernel_log_bytes={redacted_log.stat().st_size}\n"
                     f"kernel_log_sha256={sha(redacted_log)}\n"
                     "result=temporary-boot-observed\n")
    observation_evidence(observation, transcript, prefix, bundle / "boot.img", boot_hash)
    print(f"result=temporary-boot-observed transcript={observation}")


def main() -> int:
    if len(sys.argv) != 2 or sys.argv[1] not in ("preflight", "boot-temp", "capture-temp", "observe-temp"):
        print("usage: flash-static.py preflight|boot-temp|capture-temp|observe-temp", file=sys.stderr)
        return 2
    try:
        if sys.argv[1] == "preflight":
            preflight()
        elif sys.argv[1] == "boot-temp":
            boot_temp()
        elif sys.argv[1] == "capture-temp":
            capture_temp()
        else:
            observe_temp()
    except (Refusal, static.Refusal, ValueError, EOFError) as error:
        print(f"REFUSING pixel-9a static physical workflow: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
