#!/usr/bin/env python3
"""Offline refusal and reporting checks for the read-only physical guest probe."""

import importlib.util
import tempfile
from pathlib import Path
from types import SimpleNamespace


script = Path(__file__).resolve().parents[1] / "guest-evidence.py"
spec = importlib.util.spec_from_file_location("pixel9a_guest_evidence", script)
assert spec is not None and spec.loader is not None
guest = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guest)

remote_script = guest.REMOTE.read_text()
assert "ZUMA PRO TEGU MP on ZUMA PRO" in remote_script
assert "google,ZUMA PRO TEGU" in remote_script
assert "rc-service \"$service\" status" in remote_script
assert "/sys/class/drm/" in remote_script and "/sys/class/input/" in remote_script
assert not any(f" {verb} " in remote_script for verb in
               ("flash", "erase", "reboot", "set_active", "mkfs", "dd"))

sample = {
    "dt_model": "ZUMA PRO TEGU MP on ZUMA PRO",
    "dt_compatible": "google,ZUMA PRO TEGU", "kernel_release": "7.2.3-test",
    "os_id": "postmarketOS", "slot_suffix": "_b",
    "service_ferrix_core": "running", "service_ferrix_networkd": "running",
    "service_ferrix_telephonyd": "running", "service_ferrix_audio": "running",
    "service_ferrix_product": "running", "socket_core": "present",
    "socket_appd": "present", "socket_wayland": "present",
    "pid_shell": "alive", "pid_appd": "alive", "drm_connected_count": "1",
    "drm_mode_count": "1", "framebuffer_count": "0", "input_event_count": "4",
    "touch_candidate_count": "1",
}


def raw(values: dict[str, str]) -> str:
    return "".join(f"{key}={value}\n" for key, value in values.items())


assert guest.parse_inventory(raw(sample)) == sample
calls = []
original_run = guest.subprocess.run


def fake_ssh(args, **kwargs):
    calls.append((args, kwargs))
    return SimpleNamespace(returncode=0, stdout=raw(sample), stderr="")


guest.subprocess.run = fake_ssh
assert guest.remote_guest() == raw(sample)
assert len(calls) == 1 and calls[0][0][0] == "ssh"
assert calls[0][0][-2:] == ["user@172.16.42.1", "sh -s"]
assert calls[0][1]["input"] == remote_script
guest.subprocess.run = lambda *args, **kwargs: SimpleNamespace(
    returncode=4, stdout="dt_model=QEMU Virtual Machine\n", stderr="wrong board")
try:
    guest.remote_guest()
except guest.Refusal as error:
    assert "exact device-tree gate refused" in str(error)
    assert "QEMU Virtual Machine" not in str(error)
else:
    raise AssertionError("wrong guest identity was accepted")
guest.subprocess.run = original_run
for field, value in (("dt_model", "QEMU Virtual Machine"),
                     ("dt_compatible", "linux,dummy-virt"),
                     ("drm_connected_count", "1; reboot"),
                     ("socket_wayland", "maybe")):
    altered = {**sample, field: value}
    try:
        guest.parse_inventory(raw(altered))
    except guest.Refusal:
        pass
    else:
        raise AssertionError(f"guest parser accepted invalid {field}")
try:
    guest.parse_inventory(raw(sample) + "slot_suffix=_a\n")
except guest.Refusal:
    pass
else:
    raise AssertionError("guest parser accepted duplicate slot suffix")

with tempfile.TemporaryDirectory(prefix="pixel-9a-guest-evidence-test.") as temporary:
    guest.OUT = Path(temporary)
    guest.activation_provenance = lambda: {
        "activation_sha256": "a" * 64,
        "manifest_sha256": "b" * 64,
        "serial_sha256_prefix": "c" * 12,
    }
    guest.remote_guest = lambda: raw(sample)
    record = guest.inventory()
    values = guest.probe.key_values(record)
    assert values["boot_slot_status"] == "slot-b-observed"
    assert values["display_function"] == "unobserved"
    assert values["touch_function"] == "unobserved"
    assert values["wayland_client"] == "unverified"
    assert values["result"] == "inventory-only"
    guest.remote_guest = lambda: (_ for _ in ()).throw(
        AssertionError("SSH must not run before activation evidence passes"))
    guest.activation_provenance = lambda: (_ for _ in ()).throw(
        guest.Refusal("activation evidence is missing"))
    try:
        guest.inventory()
    except guest.Refusal as error:
        assert "activation evidence" in str(error)
    else:
        raise AssertionError("guest inventory ran without activation evidence")

print('{"event":"pixel-9a-guest-evidence-contract-test","status":"passed"}')
