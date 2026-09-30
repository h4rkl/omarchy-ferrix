#!/usr/bin/env python3
"""Persistent exact-target static install stages; no stage runs by default.

This host-only Python adapter reuses the reviewed target's TOML/TSV, image,
recovery, live-layout, and observation gates. It deliberately exposes separate
kernel, shared-data, and activation stages. Product and guest code use Rust.
"""

from __future__ import annotations

import importlib.util
import os
import re
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path


spec = importlib.util.spec_from_file_location(
    "pixel9a_flash_static", Path(__file__).with_name("flash-static.py"))
assert spec is not None and spec.loader is not None
sys.dont_write_bytecode = True
static_flash = importlib.util.module_from_spec(spec)
spec.loader.exec_module(static_flash)

Refusal = static_flash.Refusal
require = static_flash.require
OUT = static_flash.OUT
EVIDENCE_DIR = OUT / "install-static"
PROBE_DIR = OUT / "flash-static"
BOOT_PARTITIONS = static_flash.BOOT_PARTITIONS
TARGET_ID = static_flash.TARGET_ID
UNSIGNED_AVB_TOKEN = "I-accept-unsigned-AVB-on-this-unlocked-Pixel-9a"


def mutation_policy() -> None:
    require(os.environ.get("FERRIX_PIXEL9A_NO_USER_DATA") == static_flash.DATA_TOKEN,
            "exact development-device data policy token is required")
    require(os.environ.get("FERRIX_PIXEL9A_STATIC_UNSIGNED_AVB") == UNSIGNED_AVB_TOKEN,
            "explicit unsigned-AVB token is required for this secure unlocked device")


def admission(require_policy: bool = False) -> dict:
    target_manifest = static_flash.TARGET / "manifest.toml"
    gate_manifest_hash = static_flash.sha(target_manifest)
    candidate_hash = static_flash.sha(static_flash.CANDIDATE)
    candidate, bundle, serial, prefix, _ = static_flash.preflight()
    require(static_flash.sha(target_manifest) == gate_manifest_hash,
            "target gate manifest changed during preflight")
    require(static_flash.sha(static_flash.CANDIDATE) == candidate_hash,
            "static install candidate changed during preflight")
    require(static_flash.gates() in ("passed", "verified"),
            "static image compatibility remains partial; persistent installation is blocked")
    require(static_flash.sha(target_manifest) == gate_manifest_hash,
            "target gate manifest changed during admission")
    version_lines = static_flash.command("fastboot", "--version").splitlines()
    require(bool(version_lines), "host Fastboot version is missing")
    version = version_lines[0]
    require(version == f"fastboot version {candidate['fastboot_cli_version']}",
            "host Fastboot version differs from the reviewed static candidate")
    if require_policy:
        mutation_policy()
    boot_hash = static_flash.sha(bundle / "boot.img")
    boot_record = static_flash.evidence_path(
        os.environ.get("FERRIX_PIXEL9A_STATIC_TEMP_BOOT_EVIDENCE", ""),
        PROBE_DIR, "temporary-boot transcript")
    observation = static_flash.evidence_path(
        os.environ.get("FERRIX_PIXEL9A_STATIC_TEMP_BOOT_OBSERVATION", ""),
        PROBE_DIR, "temporary-boot observation")
    static_flash.boot_evidence(boot_record, prefix, boot_hash)
    static_flash.observation_evidence(observation, boot_record, prefix,
                                      bundle / "boot.img", boot_hash)
    context = {
        "candidate": candidate, "bundle": bundle, "serial": serial, "prefix": prefix,
        "boot_hash": boot_hash, "boot_record_hash": static_flash.sha(boot_record),
        "observation_hash": static_flash.sha(observation),
        "gate_manifest_hash": gate_manifest_hash, "candidate_hash": candidate_hash,
    }
    print(f"persistent_preflight=passed-read-only avb_algorithm=NONE secure=yes "
          f"fastboot_cli_version={candidate['fastboot_cli_version']} "
          f"observation_sha256={context['observation_hash']} "
          f"boot_transcript_sha256={context['boot_record_hash']}")
    return context


def same_admission(expected: dict) -> dict:
    current = admission(require_policy=True)
    for key in ("bundle", "serial", "prefix", "boot_hash", "boot_record_hash",
                "observation_hash", "gate_manifest_hash", "candidate_hash"):
        require(current[key] == expected[key], f"persistent admission {key} changed")
    require(current["candidate"] == expected["candidate"],
            "checked-in static candidate changed")
    return current


def artifact_plan(context: dict) -> list[tuple[str, Path, int, str]]:
    manifest = static_flash.static.tsv(context["bundle"] / "boot-manifest.tsv")
    plan = []
    for name in BOOT_PARTITIONS:
        row = static_flash.static.named(manifest, "artifact", name, 6)
        require(row[2] == f"{name}.img" and row[3].isdigit() and
                re.fullmatch(r"[0-9a-f]{64}", row[4]) is not None,
                f"static artifact {name} manifest row differs")
        plan.append((f"{name}_b", context["bundle"] / row[2], int(row[3]), row[4]))
    return plan


def rootfs_plan(context: dict) -> tuple[str, Path, int, str]:
    candidate = context["candidate"]
    return ("userdata", context["bundle"] / "rootfs.img",
            candidate["rootfs_image_bytes"], candidate["rootfs_image_sha256"])


def confirm(phrase: str, consequence: str) -> None:
    print(consequence)
    print(f"Type exactly: {phrase}", flush=True)
    require(input("> ") == phrase, "confirmation did not match; no device operation performed")


def record_path(stage: str) -> Path:
    EVIDENCE_DIR.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    return EVIDENCE_DIR / f"{stage}-{stamp}-{os.getpid()}.txt"


def stage_header(stage: str, context: dict) -> str:
    return (f"operation=pixel-9a-static-{stage}\ntarget_id={TARGET_ID}\n"
            f"serial_sha256_prefix={context['prefix']}\n"
            f"gate_manifest_sha256={context['gate_manifest_hash']}\n"
            f"candidate_sha256={context['candidate_hash']}\n"
            f"manifest_sha256={context['candidate']['manifest_sha256']}\n"
            f"boot_img_sha256={context['boot_hash']}\n"
            f"boot_transcript_sha256={context['boot_record_hash']}\n"
            f"observation_sha256={context['observation_hash']}\n")


def write_image(context: dict, partition: str, image: Path, size: int,
                expected_hash: str, record) -> None:
    same_admission(context)
    require(image.is_file() and not image.is_symlink() and
            image.stat().st_size == size and static_flash.sha(image) == expected_hash,
            f"{partition} image changed before write")
    print(f"mutation=fastboot flash exact_serial={context['serial']} "
          f"partition_target={partition} input_image_sha256={expected_hash} bytes={size}",
          flush=True)
    if partition.endswith("_b") and partition[:-2] in BOOT_PARTITIONS:
        args = ("fastboot", "-s", context["serial"], "--slot", "b",
                "flash", partition[:-2], str(image))
    else:
        require(partition == "userdata", f"partition {partition} is outside the static allow-list")
        args = ("fastboot", "-s", context["serial"], "flash", partition, str(image))
    try:
        output = static_flash.command(*args, timeout=900)
        written = re.findall(r"Writing '([^']+)'", output)
        require("FAILED" not in output and output.count("OKAY") >= 2 and
                bool(written) and set(written) == {partition},
                f"bootloader did not confirm successful flash of {partition}")
    except Refusal as error:
        record.write(f"refused_{partition}={str(error).replace(context['serial'], '[redacted]')}\n")
        record.flush()
        raise
    record.write(f"flashed_{partition}={expected_hash}\n")
    record.flush()


def kernel_record(path: Path, context: dict) -> None:
    values = static_flash.key_values(path)
    expected = {
        "operation": "pixel-9a-static-flash-kernel", "target_id": TARGET_ID,
        "serial_sha256_prefix": context["prefix"],
        "gate_manifest_sha256": context["gate_manifest_hash"],
        "candidate_sha256": context["candidate_hash"],
        "manifest_sha256": context["candidate"]["manifest_sha256"],
        "boot_img_sha256": context["boot_hash"],
        "boot_transcript_sha256": context["boot_record_hash"],
        "observation_sha256": context["observation_hash"],
        "slot": "b", "result": "kernel-stage-complete",
    }
    for partition, _, _, digest in artifact_plan(context):
        expected[f"flashed_{partition}"] = digest
    require(values == expected, "inactive kernel stage record is incomplete or differs")


def rootfs_record(path: Path, context: dict, kernel_hash: str) -> None:
    values = static_flash.key_values(path)
    expected = {
        "operation": "pixel-9a-static-flash-rootfs", "target_id": TARGET_ID,
        "serial_sha256_prefix": context["prefix"],
        "gate_manifest_sha256": context["gate_manifest_hash"],
        "candidate_sha256": context["candidate_hash"],
        "manifest_sha256": context["candidate"]["manifest_sha256"],
        "boot_img_sha256": context["boot_hash"],
        "boot_transcript_sha256": context["boot_record_hash"],
        "observation_sha256": context["observation_hash"],
        "kernel_stage_sha256": kernel_hash,
        "flashed_userdata": context["candidate"]["rootfs_image_sha256"],
        "result": "rootfs-stage-complete",
    }
    require(values == expected, "shared rootfs stage record is incomplete or differs")


def kernel_input(context: dict) -> tuple[Path, str]:
    path = static_flash.evidence_path(
        os.environ.get("FERRIX_PIXEL9A_STATIC_KERNEL_STAGE_EVIDENCE", ""),
        EVIDENCE_DIR, "inactive kernel stage transcript")
    kernel_record(path, context)
    return path, static_flash.sha(path)


def rootfs_input(context: dict, kernel_hash: str) -> tuple[Path, str]:
    path = static_flash.evidence_path(
        os.environ.get("FERRIX_PIXEL9A_STATIC_ROOTFS_STAGE_EVIDENCE", ""),
        EVIDENCE_DIR, "shared rootfs stage transcript")
    rootfs_record(path, context, kernel_hash)
    return path, static_flash.sha(path)


def activation_policy(context: dict) -> str:
    path = static_flash.TARGET / "static-activation-candidate.toml"
    digest = static_flash.sha(path)
    values = tomllib.loads(path.read_text())
    require(values == {
        "schema_version": 1, "target_id": TARGET_ID,
        "status": "proposed-not-run",
        "source_candidate": "static-install-candidate.toml",
        "source_candidate_sha256": static_flash.sha(static_flash.CANDIDATE),
        "manifest_sha256": context["candidate"]["manifest_sha256"],
        "expected_current_slot": "a", "target_slot": "b",
        "operation": "select-reviewed-inactive-slot-and-reboot-only",
        "kernel_stage_required": True, "rootfs_stage_required": True,
        "per_run_confirmation": True,
        "stock_recovery": "tested-pinned-full-factory-restore",
    }, "separate static activation proposal differs from reviewed slot policy")
    require(static_flash.sha(path) == digest, "activation proposal changed while reading")
    return digest


def activation_record(path: Path, context: dict, kernel_hash: str,
                      rootfs_hash: str, proposal_hash: str) -> None:
    expected = {
        "operation": "pixel-9a-static-activate", "target_id": TARGET_ID,
        "serial_sha256_prefix": context["prefix"],
        "gate_manifest_sha256": context["gate_manifest_hash"],
        "candidate_sha256": context["candidate_hash"],
        "manifest_sha256": context["candidate"]["manifest_sha256"],
        "boot_img_sha256": context["boot_hash"],
        "boot_transcript_sha256": context["boot_record_hash"],
        "observation_sha256": context["observation_hash"],
        "kernel_stage_sha256": kernel_hash,
        "rootfs_stage_sha256": rootfs_hash,
        "activation_proposal_sha256": proposal_hash,
        "slot_selection": "issued-b", "result": "slot-b-boot-requested",
    }
    require(static_flash.key_values(path) == expected,
            "static activation record is incomplete or differs")


def flash_kernel() -> None:
    context = admission()
    mutation_policy()
    plan = artifact_plan(context)
    for partition, _, size, digest in plan:
        print(f"write_plan=partition_target={partition} input_image_sha256={digest} bytes={size}")
    confirm("CONFIRM PIXEL 9A STATIC FLASH INACTIVE KERNEL STAGE TO SLOT B",
            "This writes eight physical boot partitions on inactive slot B. "
            "AVB algorithm NONE is unsigned on this unlocked device. "
            "It does not select a slot or overwrite stock slot A; a failure may leave B partial.")
    same_admission(context)
    record = record_path("flash-kernel")
    with record.open("x") as stream:
        stream.write(stage_header("flash-kernel", context) + "slot=b\n")
        for partition, image, size, digest in plan:
            write_image(context, partition, image, size, digest, stream)
        stream.write("result=kernel-stage-complete\n")
    kernel_record(record, context)
    print(f"result=kernel-stage-complete transcript={record}")


def flash_rootfs() -> None:
    context = admission()
    mutation_policy()
    _, kernel_hash = kernel_input(context)
    partition, image, size, digest = rootfs_plan(context)
    print(f"write_plan=partition_target={partition} input_image_sha256={digest} "
          f"bytes={size} shared_data_wipe=yes")
    confirm("CONFIRM PIXEL 9A STATIC FLASH SHARED USERDATA ROOTFS",
            "This overwrites shared userdata with the Ferrix rootfs. Android data is erased; "
            "stock recovery requires the tested full factory restore. "
            "The selected boot images use unsigned AVB on this unlocked device.")
    same_admission(context)
    _, checked_kernel_hash = kernel_input(context)
    require(checked_kernel_hash == kernel_hash, "kernel stage evidence changed")
    record = record_path("flash-rootfs")
    with record.open("x") as stream:
        stream.write(stage_header("flash-rootfs", context) +
                     f"kernel_stage_sha256={kernel_hash}\n")
        write_image(context, partition, image, size, digest, stream)
        stream.write("result=rootfs-stage-complete\n")
    rootfs_record(record, context, kernel_hash)
    print(f"result=rootfs-stage-complete transcript={record}")


def post_select(serial: str) -> None:
    require(static_flash.attached_fastboot() == serial,
            "Fastboot transport changed after slot selection")
    for name, expected in (("product", "tegu"), ("hw-revision", "MP1.0"),
                           ("current-slot", "b"), ("slot-count", "2"),
                           ("unlocked", "yes"), ("secure", "yes"),
                           ("is-userspace", "no"),
                           ("snapshot-update-status", "none"),
                           ("serialno", serial)):
        require(static_flash.getvar(serial, name) == expected,
                f"post-selection Fastboot {name} differs")
    require(static_flash.getvar(serial, "variant") in
            ("GXQ96", "N/A", "NA", "unknown", "unavailable"),
            "post-selection Fastboot variant differs")


def activate() -> None:
    context = admission()
    mutation_policy()
    _, kernel_hash = kernel_input(context)
    _, rootfs_hash = rootfs_input(context, kernel_hash)
    activation_hash = activation_policy(context)
    confirm("CONFIRM PIXEL 9A STATIC SELECT SLOT B AND BOOT FERRIX",
            "This selects the written slot B and requests one reboot. Shared userdata has "
            "been overwritten; stock requires the tested full factory restore. "
            "The selected images use unsigned AVB; boot success is unverified.")
    same_admission(context)
    _, checked_kernel_hash = kernel_input(context)
    _, checked_rootfs_hash = rootfs_input(context, kernel_hash)
    require((checked_kernel_hash, checked_rootfs_hash) == (kernel_hash, rootfs_hash),
            "install stage evidence changed before activation")
    require(activation_policy(context) == activation_hash,
            "activation proposal changed before slot selection")
    record = record_path("activate")
    with record.open("x") as stream:
        stream.write(stage_header("activate", context) +
                     f"kernel_stage_sha256={kernel_hash}\n"
                     f"rootfs_stage_sha256={rootfs_hash}\n"
                     f"activation_proposal_sha256={activation_hash}\n")
        print(f"mutation=fastboot set_active exact_serial={context['serial']} "
              "partition_target=slot_b input_image_sha256=none", flush=True)
        output = static_flash.command("fastboot", "-s", context["serial"],
                                      "set_active", "b", timeout=60)
        require("FAILED" not in output and "OKAY" in output,
                "bootloader did not confirm selecting slot B")
        stream.write("slot_selection=issued-b\n")
        stream.flush()
        post_select(context["serial"])
        require(activation_policy(context) == activation_hash,
                "activation proposal changed after slot selection")
        factory, ota = static_flash.recovery()
        print(f"post_selection=exact_serial={context['serial']} product=tegu "
              "hardware_revision=MP1.0 current_slot=b")
        print(f"recovery_factory={factory.name} sha256={static_flash.sha(factory)}")
        print(f"recovery_ota={ota.name} sha256={static_flash.sha(ota)}")
        print(f"mutation=fastboot reboot exact_serial={context['serial']} "
              f"boot_target=slot_b partition_target=none "
              f"input_image_sha256={context['boot_hash']}", flush=True)
        output = static_flash.command("fastboot", "-s", context["serial"],
                                      "reboot", timeout=60)
        require("FAILED" not in output and "OKAY" in output,
                "bootloader did not confirm reboot request")
        stream.write("result=slot-b-boot-requested\n")
    activation_record(record, context, kernel_hash, rootfs_hash, activation_hash)
    print(f"result=slot-b-boot-requested transcript={record}")


def main() -> int:
    if len(sys.argv) != 2 or sys.argv[1] not in (
            "preflight", "flash-kernel", "flash-rootfs", "activate"):
        print("usage: install-static.py preflight|flash-kernel|flash-rootfs|activate",
              file=sys.stderr)
        return 2
    try:
        mode = sys.argv[1]
        if mode == "preflight":
            admission()
        elif mode == "flash-kernel":
            flash_kernel()
        elif mode == "flash-rootfs":
            flash_rootfs()
        else:
            activate()
    except (Refusal, static_flash.static.Refusal, ValueError, EOFError) as error:
        print(f"REFUSING pixel-9a static install: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
