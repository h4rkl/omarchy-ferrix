#!/usr/bin/env python3
"""Read-only stock Android boot diagnostics after a Pixel 9a temporary probe.

Collects only boot-reason properties and bounded console pstore summaries. Raw
kernel logs, serials, and other device output are never written to disk.
"""

from __future__ import annotations

import hashlib
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "out/pixel-9a/postprobe"
IDENTITY = {
    "ro.product.manufacturer": "Google",
    "ro.product.model": "Pixel 9a",
    "ro.product.device": "tegu",
    "ro.boot.hardware.revision": "MP1.0",
    "ro.boot.hardware.sku": "GXQ96",
    "ro.boot.slot_suffix": "_a",
    "ro.boot.vbmeta.device_state": "unlocked",
    "ro.build.version.release": "17",
}
REASONS = ("ro.boot.bootreason", "sys.boot.reason", "sys.boot.reason.last")
FAULT = re.compile(rb"Kernel panic|Oops:|BUG:|watchdog.*lockup", re.I)


def run(*args: str) -> bytes:
    result = subprocess.run(args, capture_output=True, timeout=20, check=False)
    if result.returncode:
        raise ValueError(f"{args[0]} {args[-1]} failed with exit {result.returncode}")
    return result.stdout


def prop(serial: str, name: str) -> str:
    value = run("adb", "-s", serial, "shell", "getprop", name).decode("utf-8").strip()
    if "\n" in value or len(value) > 128:
        raise ValueError(f"malformed {name}")
    return value


def main() -> int:
    if len(sys.argv) != 1:
        print("usage: postprobe-diagnose.py", file=sys.stderr)
        return 2
    adb_rows = [line.split() for line in run("adb", "devices", "-l").decode().splitlines()[1:]
                if line.strip()]
    fastboot_rows = [line for line in run("fastboot", "devices", "-l").splitlines() if line.strip()]
    if len(adb_rows) != 1 or len(adb_rows[0]) < 2 or adb_rows[0][1] != "device" or fastboot_rows:
        raise ValueError("requires exactly one authorized ADB device and no Fastboot device")
    serial = adb_rows[0][0]
    for name, expected in IDENTITY.items():
        if prop(serial, name) != expected:
            raise ValueError(f"exact Pixel 9a identity mismatch: {name}")

    lines = ["operation=pixel-9a-postprobe-diagnose", "evidence=physical-read-only",
             "target_id=google-pixel-9a-tegu-gxq96-mp1-0",
             f"serial_sha256_prefix={hashlib.sha256(serial.encode()).hexdigest()[:12]}"]
    for name in REASONS:
        value = prop(serial, name)
        # Boot-reason properties are short tokens. Never persist an unexpected value.
        lines.append(f"{name}={value if re.fullmatch(r'[A-Za-z0-9_,. -]{0,128}', value) else 'redacted'}")

    listing_result = subprocess.run(
        ("adb", "-s", serial, "shell", "ls", "-1", "/sys/fs/pstore"),
        capture_output=True, timeout=20, check=False)
    lines.append(f"console_pstore_listing={'available' if listing_result.returncode == 0 else 'unavailable'}")
    listing = listing_result.stdout.decode("utf-8", errors="replace") if listing_result.returncode == 0 else ""
    names = sorted({name.strip() for name in listing.splitlines()
                    if re.fullmatch(r"console-ramoops(?:-[0-9]+)?", name.strip())})
    lines.append(f"console_pstore_entries={len(names)}")
    for index, name in enumerate(names):
        content_result = subprocess.run(
            ("adb", "-s", serial, "shell", "cat", f"/sys/fs/pstore/{name}"),
            capture_output=True, timeout=20, check=False)
        content = content_result.stdout
        if content_result.returncode or not content or len(content) > 2_000_000:
            lines.append(f"console_pstore_{index}=unavailable-or-oversize")
            continue
        lines.append(f"console_pstore_{index}_sha256={hashlib.sha256(content).hexdigest()}")
        lines.append(f"console_pstore_{index}_fault_marker={str(bool(FAULT.search(content))).lower()}")
    lines.append("interpretation=boot-reason-and-pstore-cannot-prove-candidate-kernel-boot")

    OUT.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    path = OUT / f"postprobe-{stamp}.txt"
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))
    print(f"transcript={path}")
    print(f"transcript_sha256={hashlib.sha256(path.read_bytes()).hexdigest()}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, UnicodeError, subprocess.TimeoutExpired, FileNotFoundError) as error:
        print(f"refused: {error}", file=sys.stderr)
        raise SystemExit(2) from error
