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
    os.environ.pop("FERRIX_PIXEL9A_NO_USER_DATA", None)
    builtins.input = original_input
    workflow.preflight = original_preflight
    workflow.sha = original_sha

print('{"event":"pixel-9a-flash-static-contract-test","status":"passed"}')
