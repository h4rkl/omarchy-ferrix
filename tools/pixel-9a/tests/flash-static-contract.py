#!/usr/bin/env python3
"""Mock transport checks for the static route's exact-device boot probe."""

import builtins
import hashlib
import importlib.util
import os
import tempfile
from pathlib import Path


script = Path(__file__).resolve().parents[1] / "flash-static.py"
spec = importlib.util.spec_from_file_location("pixel9a_flash_static", script)
assert spec is not None and spec.loader is not None
workflow = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workflow)

serial = "TEST-PIXEL-9A"
prefix = hashlib.sha256(serial.encode()).hexdigest()[:12]
capacities = {name: 4096 * (index + 1) for index, name in enumerate(workflow.BOOT_PARTITIONS)}
userdata_capacity = 117543358464
responses = {
    "product": "tegu", "hw-revision": "MP1.0", "current-slot": "a",
    "slot-count": "2", "unlocked": "yes", "secure": "yes", "is-userspace": "no",
    "snapshot-update-status": "none", "variant": "N/A", "serialno": serial,
    "has-slot:userdata": "no", "is-logical:userdata": "no",
    "partition-type:userdata": "raw", "partition-size:userdata": hex(userdata_capacity),
}
for name in workflow.BOOT_PARTITIONS:
    responses[f"has-slot:{name}"] = "yes"
    for slot in ("a", "b"):
        responses[f"is-logical:{name}_{slot}"] = "no"
        responses[f"partition-size:{name}_{slot}"] = hex(capacities[name])

commands = []


def fake_command(*args: str, timeout: int = 30) -> str:
    commands.append(args)
    if args == ("adb", "devices", "-l"):
        return "List of devices attached\n\n"
    if args == ("fastboot", "devices", "-l"):
        return f"{serial} fastboot usb:1\n"
    assert args[:4] == ("fastboot", "-s", serial, "getvar"), args
    value = responses[args[4]]
    return f"(bootloader) {args[4]}: {value}\nFinished. Total time: 0.001s\n"


workflow.command = fake_command
workflow.discovery_prefix = lambda path: prefix
workflow.rootfs_mapping = lambda path, actual_prefix, candidate: userdata_capacity
candidate = {"rootfs_image_bytes": 2147483648}


def admit() -> None:
    commands.clear()
    identity, observed_prefix, size = workflow.live(candidate, Path("ignored"), Path("ignored"), capacities)
    assert (identity, observed_prefix, size) == (serial, prefix, userdata_capacity)
    assert not any("boot" in call or "flash" in call or "set_active" in call for call in commands)


def refuse(change: str, value: str, reason: str) -> None:
    old = responses[change]
    responses[change] = value
    try:
        workflow.live(candidate, Path("ignored"), Path("ignored"), capacities)
    except workflow.Refusal as error:
        assert reason in str(error), (reason, str(error))
    else:
        raise AssertionError(f"live gate accepted changed {change}")
    finally:
        responses[change] = old


admit()
refuse("current-slot", "b", "current-slot")
refuse("hw-revision", "MP2.0", "hw-revision")
refuse("is-logical:vendor_boot_b", "yes", "vendor_boot_b")
refuse("partition-size:boot_b", hex(capacities["boot"] + 4096), "boot_b capacity")
refuse("snapshot-update-status", "merging", "snapshot-update-status")
refuse("partition-size:userdata", "0x0", "userdata capacity")

workflow.command = lambda *args, **kwargs: "(bootloader) boot_b: 1\n(bootloader) boot_b: 2\n"
try:
    workflow.getvar(serial, "boot_b")
except workflow.Refusal as error:
    assert "ambiguous" in str(error)
else:
    raise AssertionError("duplicate Fastboot getvar was accepted")

original_preflight = workflow.preflight
original_sha = workflow.sha
original_input = builtins.input
with tempfile.TemporaryDirectory(prefix="pixel-9a-static-flash-test.") as temporary:
    workflow.OUT = Path(temporary)
    workflow.preflight = lambda: ({}, Path(temporary), serial, prefix, Path(temporary))
    workflow.sha = lambda path: "a" * 64
    os.environ.pop("FERRIX_PIXEL9A_NO_USER_DATA", None)
    try:
        workflow.boot_temp()
    except workflow.Refusal as error:
        assert "data policy token" in str(error)
    else:
        raise AssertionError("temporary boot accepted missing data policy")
    os.environ["FERRIX_PIXEL9A_NO_USER_DATA"] = workflow.DATA_TOKEN
    builtins.input = lambda prompt: "wrong phrase"
    try:
        workflow.boot_temp()
    except workflow.Refusal as error:
        assert "confirmation did not match" in str(error)
    else:
        raise AssertionError("temporary boot accepted wrong typed confirmation")

    # The authorized path must re-run admission and issue exactly one
    # temporary boot command, with no partition or slot mutation.
    checks = []
    boot_commands = []
    workflow.preflight = lambda: (checks.append("preflight") or
                                  ({}, Path(temporary), serial, prefix, Path(temporary)))
    workflow.command = lambda *args, **kwargs: (boot_commands.append(args) or
                                                "Sending 'boot.img' OKAY\nBooting OKAY\n")
    builtins.input = lambda prompt: "CONFIRM PIXEL 9A STATIC TEMPORARY BOOT boot.img@aaaaaaaaaaaa"
    workflow.boot_temp()
    assert checks == ["preflight", "preflight"], checks
    assert boot_commands == [("fastboot", "-s", serial, "boot", str(Path(temporary) / "boot.img"))]
    assert list(Path(temporary, "flash-static").glob("boot-temp-*.txt"))
    boot_record = list(Path(temporary, "flash-static").glob("boot-temp-*.txt"))[0]
    assert workflow.key_values(boot_record)["boot_exit_status"] == "0"
    os.environ.pop("FERRIX_PIXEL9A_NO_USER_DATA", None)
    builtins.input = original_input
    workflow.preflight = original_preflight
    workflow.sha = original_sha

with tempfile.TemporaryDirectory(prefix="pixel-9a-static-observe-test.") as temporary:
    root = Path(temporary)
    evidence_dir = root / "flash-static"
    evidence_dir.mkdir()
    bundle = root / "images"
    bundle.mkdir()
    banner = ("Linux version 7.2.3-test (builder@test) (clang 19) "
              "#1 SMP PREEMPT Wed Sep 30 01:59:37 UTC 2026")
    (bundle / "boot.img").write_bytes(b"test boot image\0" + banner.encode() + b"\0")
    boot_hash = workflow.sha(bundle / "boot.img")
    transcript = evidence_dir / "boot-temp-test.txt"
    transcript.write_text(
        f"operation=pixel-9a-static-boot-temp\ntarget_id={workflow.TARGET_ID}\n"
        f"serial_sha256_prefix={prefix}\nboot_img_sha256={boot_hash}\n"
        "boot_exit_status=0\nresult=temporary-boot-issued\noutput=Booting OKAY\n")
    capture = root / "kernel-capture.txt"
    capture.write_text(f"{banner}\nZUMA PRO TEGU\nserial={serial}\n")
    workflow.OUT = root
    checks = []
    workflow.preflight = lambda: (checks.append("preflight") or
                                  ({}, bundle, serial, prefix, root))
    workflow.command = lambda *args, **kwargs: (_ for _ in ()).throw(
        AssertionError("observation must not issue a device command"))
    os.environ["FERRIX_PIXEL9A_STATIC_TEMP_BOOT_EVIDENCE"] = str(transcript)
    os.environ["FERRIX_PIXEL9A_STATIC_TEMP_BOOT_KERNEL_LOG"] = str(capture)
    builtins.input = lambda prompt: (
        f"CONFIRM PIXEL 9A STATIC KERNEL PROGRESS OBSERVED boot.img@{boot_hash[:12]}")
    workflow.observe_temp()
    assert checks == ["preflight", "preflight"], checks
    observation = next(path for path in evidence_dir.glob("observe-temp-*.txt")
                       if not path.name.endswith("-kernel.txt"))
    workflow.observation_evidence(observation, transcript, prefix,
                                  bundle / "boot.img", boot_hash)
    assert serial not in (evidence_dir / workflow.key_values(observation)["kernel_log"]).read_text()
    original_transcript = transcript.read_text()
    transcript.write_text(original_transcript.replace("boot_exit_status=0", "boot_exit_status=1"))
    try:
        workflow.boot_evidence(transcript, prefix, boot_hash)
    except workflow.Refusal as error:
        assert "boot_exit_status" in str(error)
    else:
        raise AssertionError("failed boot transcript was accepted")
    try:
        workflow.observation_evidence(observation, transcript, prefix,
                                      bundle / "boot.img", boot_hash)
    except workflow.Refusal as error:
        assert "boot_transcript_sha256" in str(error)
    else:
        raise AssertionError("changed boot transcript was accepted")
    transcript.write_text(original_transcript)
    capture.write_text(f"{banner}\nZUMA PRO TEGU\nKernel panic\n")
    try:
        workflow.observe_temp()
    except workflow.Refusal as error:
        assert "kernel failure" in str(error)
    else:
        raise AssertionError("kernel panic was accepted as boot progress")
    capture.write_text("Linux version 6.1-stock (stock@google) #1 SMP\nZUMA PRO TEGU\n")
    try:
        workflow.observe_temp()
    except workflow.Refusal as error:
        assert "pinned boot image kernel banner" in str(error)
    else:
        raise AssertionError("stock tegu kernel log was accepted as Ferrix boot progress")
    observed_properties = {
        "ro.product.manufacturer": "Google", "ro.product.model": "Pixel 9a",
        "ro.product.device": "tegu", "ro.boot.hardware.revision": "MP1.0",
        "ro.boot.hardware.sku": "GXQ96", "ro.boot.slot_suffix": "_a",
        "ro.boot.vbmeta.device_state": "unlocked", "ro.build.version.release": "17",
    }
    workflow.read_candidate = lambda: ({}, bundle, root / "discovery.txt", root)
    workflow.discovery_prefix = lambda path: prefix
    commands = []

    def adb_capture_command(*args: str, **kwargs) -> str:
        commands.append(args)
        if args == ("adb", "devices", "-l"):
            return f"List of devices attached\n{serial} device usb:1\n"
        if args == ("fastboot", "devices", "-l"):
            return ""
        assert args[:4] == ("adb", "-s", serial, "shell"), args
        if args[4] == "getprop":
            return observed_properties[args[5]] + "\n"
        if args[4:] == ("cat", "/proc/version"):
            return banner + "\n"
        if args[4:] == ("cat", "/proc/device-tree/model"):
            return "ZUMA PRO TEGU MP on ZUMA PRO\x00"
        raise AssertionError(args)

    workflow.command = adb_capture_command
    workflow.capture_temp()
    assert all("boot" not in call and "flash" not in call and "set_active" not in call
               for call in commands)
    generated_capture = next(evidence_dir.glob("capture-temp-*.txt"))
    assert workflow.kernel_capture(generated_capture, bundle / "boot.img")
    workflow.capture_provenance(generated_capture.read_text(), prefix, boot_hash,
                                workflow.sha(transcript))
    try:
        workflow.capture_provenance(generated_capture.read_text(), prefix, boot_hash,
                                    "0" * 64)
    except workflow.Refusal as error:
        assert "boot provenance" in str(error)
    else:
        raise AssertionError("capture from another temporary boot was accepted")
    assert serial not in generated_capture.read_text()
    observed_properties["ro.boot.hardware.revision"] = "MP2.0"
    try:
        workflow.capture_temp()
    except workflow.Refusal as error:
        assert "hardware.revision" in str(error)
    else:
        raise AssertionError("wrong hardware revision was accepted for capture")
    os.environ.pop("FERRIX_PIXEL9A_STATIC_TEMP_BOOT_EVIDENCE", None)
    os.environ.pop("FERRIX_PIXEL9A_STATIC_TEMP_BOOT_KERNEL_LOG", None)
    builtins.input = original_input
    workflow.preflight = original_preflight

print('{"event":"pixel-9a-flash-static-contract-test","status":"passed"}')
