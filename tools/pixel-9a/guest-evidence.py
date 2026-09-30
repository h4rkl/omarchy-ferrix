#!/usr/bin/env python3
"""Read-only host inventory of an activated Pixel 9a Ferrix guest.

The target-local POSIX probe uses only procfs, sysfs, OpenRC status, pidfiles,
and Unix-socket tests. Python is host tooling here; guest/product code remains
Rust. An inventory is deliberately not a display, touch, or radio claim.
"""

from __future__ import annotations

import hashlib
import importlib.util
import os
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
TOOL_DIR = ROOT / "tools/pixel-9a"
TARGET_ID = "google-pixel-9a-tegu-gxq96-mp1-0"
REMOTE = TOOL_DIR / "guest-evidence-remote.sh"
OUT = ROOT / "out/pixel-9a"
INSTALL_DIR = OUT / "install-static"
EXPECTED_KEYS = {
    "dt_model", "dt_compatible", "kernel_release", "os_id", "slot_suffix",
    "service_ferrix_core", "service_ferrix_networkd", "service_ferrix_telephonyd",
    "service_ferrix_audio", "service_ferrix_product", "socket_core", "socket_appd",
    "socket_wayland", "pid_shell", "pid_appd", "drm_connected_count",
    "drm_mode_count", "framebuffer_count", "input_event_count",
    "touch_candidate_count",
}

spec = importlib.util.spec_from_file_location("pixel9a_install_static", TOOL_DIR / "install-static.py")
assert spec is not None and spec.loader is not None
sys.dont_write_bytecode = True
install = importlib.util.module_from_spec(spec)
spec.loader.exec_module(install)
probe = install.static_flash
Refusal = probe.Refusal
require = probe.require


def matching_record(directory: Path, pattern: str, digest: str) -> Path:
    require(re.fullmatch(r"[0-9a-f]{64}", digest) is not None,
            "stage evidence hash is malformed")
    matches = [path for path in directory.glob(pattern)
               if path.is_file() and not path.is_symlink() and probe.sha(path) == digest]
    require(len(matches) == 1, f"exactly one {pattern} stage record is required")
    return matches[0]


def activation_provenance() -> dict[str, str]:
    candidate, bundle, discovery, _ = probe.read_candidate()
    prefix = probe.discovery_prefix(discovery)
    require(probe.gates() in ("passed", "verified"),
            "physical guest inventory requires a reviewed passed image-compatibility gate")
    activation = probe.evidence_path(
        os.environ.get("FERRIX_PIXEL9A_STATIC_ACTIVATION_EVIDENCE", ""),
        INSTALL_DIR, "static activation transcript")
    values = probe.key_values(activation)
    common = {
        "target_id": TARGET_ID,
        "serial_sha256_prefix": prefix,
        "gate_manifest_sha256": probe.sha(probe.TARGET / "manifest.toml"),
        "candidate_sha256": probe.sha(probe.CANDIDATE),
        "manifest_sha256": candidate["manifest_sha256"],
        "boot_img_sha256": probe.sha(bundle / "boot.img"),
    }
    for key, value in common.items():
        require(values.get(key) == value, f"activation transcript {key} differs")
    boot_record = matching_record(OUT / "flash-static", "boot-temp-*.txt",
                                  values.get("boot_transcript_sha256", ""))
    observation = matching_record(OUT / "flash-static", "observe-temp-*.txt",
                                  values.get("observation_sha256", ""))
    probe.boot_evidence(boot_record, prefix, common["boot_img_sha256"])
    probe.observation_evidence(observation, boot_record, prefix,
                               bundle / "boot.img", common["boot_img_sha256"])
    context = {
        "candidate": candidate, "bundle": bundle, "prefix": prefix,
        "boot_hash": common["boot_img_sha256"],
        "boot_record_hash": probe.sha(boot_record),
        "observation_hash": probe.sha(observation),
        "gate_manifest_hash": common["gate_manifest_sha256"],
        "candidate_hash": common["candidate_sha256"],
    }
    kernel_record = matching_record(INSTALL_DIR, "flash-kernel-*.txt",
                                    values.get("kernel_stage_sha256", ""))
    rootfs_record = matching_record(INSTALL_DIR, "flash-rootfs-*.txt",
                                    values.get("rootfs_stage_sha256", ""))
    install.kernel_record(kernel_record, context)
    install.rootfs_record(rootfs_record, context, probe.sha(kernel_record))
    install.activation_record(activation, context, probe.sha(kernel_record),
                              probe.sha(rootfs_record), install.activation_policy(context))
    return {"activation_sha256": probe.sha(activation),
            "manifest_sha256": candidate["manifest_sha256"],
            "serial_sha256_prefix": prefix}


def ssh_user() -> str:
    config = (probe.TARGET / "pmos/pmbootstrap.cfg").read_text()
    found = re.findall(r"^user\s*=\s*([A-Za-z_][A-Za-z0-9_-]*)\s*$", config, re.MULTILINE)
    require(len(found) == 1, "pmOS SSH user is missing or ambiguous")
    user = os.environ.get("FERRIX_PIXEL9A_SSH_USER", found[0])
    require(re.fullmatch(r"[A-Za-z_][A-Za-z0-9_-]*", user) is not None,
            "SSH user is malformed")
    return user


def remote_guest() -> str:
    require(REMOTE.is_file() and not REMOTE.is_symlink(), "guest probe script is missing or linked")
    host = os.environ.get("FERRIX_PIXEL9A_SSH_HOST", "172.16.42.1")
    port = os.environ.get("FERRIX_PIXEL9A_SSH_PORT", "22")
    require(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9.:-]*", host) is not None and
            re.fullmatch(r"[0-9]{1,5}", port) is not None and 1 <= int(port) <= 65535,
            "SSH endpoint is malformed")
    args = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
            "-o", "StrictHostKeyChecking=accept-new", "-p", port,
            f"{ssh_user()}@{host}", "sh -s"]
    try:
        completed = subprocess.run(args, input=REMOTE.read_text(), text=True,
                                   capture_output=True, timeout=45, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise Refusal(f"guest SSH inventory unavailable: {error}") from error
    require(completed.returncode == 0, "guest SSH probe or exact device-tree gate refused")
    return completed.stdout


def parse_inventory(raw: str) -> dict[str, str]:
    require(len(raw) <= 16384, "guest inventory exceeds the reviewed size limit")
    values: dict[str, str] = {}
    for line in raw.splitlines():
        require("=" in line, "guest inventory has a malformed line")
        key, value = line.split("=", 1)
        require(key in EXPECTED_KEYS and key not in values and "\x00" not in value,
                "guest inventory has an unknown or duplicate field")
        values[key] = value
    require(set(values) == EXPECTED_KEYS, "guest inventory is incomplete")
    require(values["dt_model"] == "ZUMA PRO TEGU MP on ZUMA PRO" and
            values["dt_compatible"] == "google,ZUMA PRO TEGU",
            "guest device tree differs from the admitted tegu board")
    require(re.fullmatch(r"[A-Za-z0-9._+~-]{1,100}", values["kernel_release"]) is not None,
            "guest kernel release is malformed")
    require(re.fullmatch(r"[A-Za-z0-9._-]{1,50}", values["os_id"]) is not None,
            "guest OS identifier is malformed")
    require(values["slot_suffix"] in ("_b", "_a", "unknown", "conflict"),
            "guest slot suffix is malformed")
    for key in EXPECTED_KEYS:
        value = values[key]
        if key.startswith("service_"):
            require(value in ("running", "stopped", "unknown"), f"{key} is malformed")
        elif key.startswith("socket_"):
            require(value in ("present", "absent"), f"{key} is malformed")
        elif key.startswith("pid_"):
            require(value in ("alive", "stale", "invalid", "absent"), f"{key} is malformed")
        elif key.endswith("_count"):
            require(value.isascii() and value.isdigit() and int(value) <= 1000,
                    f"{key} is malformed")
    return values


def inventory() -> Path:
    provenance = activation_provenance()
    values = parse_inventory(remote_guest())
    status = "slot-b-observed" if values["slot_suffix"] == "_b" else "slot-unverified"
    evidence_dir = OUT / "guest-evidence"
    evidence_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    path = evidence_dir / f"inventory-{stamp}-{os.getpid()}.txt"
    lines = {
        "operation": "pixel-9a-static-guest-inventory",
        "evidence": f"physical:{TARGET_ID}", "source": "ssh-read-only",
        "captured_utc": datetime.now(timezone.utc).isoformat(),
        **provenance, **values, "boot_slot_status": status,
        "wayland_client": "unverified", "display_function": "unobserved",
        "touch_function": "unobserved", "service_function": "unverified",
        "result": "inventory-only",
    }
    with path.open("x", encoding="utf-8") as stream:
        for key, value in lines.items():
            stream.write(f"{key}={value}\n")
    print(f"result=inventory-only boot_slot_status={status} transcript={path} "
          f"sha256={hashlib.sha256(path.read_bytes()).hexdigest()}")
    print("DRM, input, service, process, and socket presence do not establish "
          "a rendered screen, touch event, connected Wayland client, or working service.")
    return path


def main() -> int:
    if sys.argv[1:] != ["inventory"]:
        print("usage: guest-evidence.py inventory", file=sys.stderr)
        return 2
    try:
        inventory()
    except (Refusal, probe.static.Refusal, ValueError) as error:
        print(f"REFUSING pixel-9a guest evidence: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
