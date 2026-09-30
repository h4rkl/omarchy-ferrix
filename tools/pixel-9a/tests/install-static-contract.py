#!/usr/bin/env python3
"""Offline transport and evidence contract for the static install stages."""

import builtins
import hashlib
import importlib.util
import os
import tempfile
import tomllib
from pathlib import Path


script = Path(__file__).resolve().parents[1] / "install-static.py"
spec = importlib.util.spec_from_file_location("pixel9a_install_static", script)
assert spec is not None and spec.loader is not None
install = importlib.util.module_from_spec(spec)
spec.loader.exec_module(install)
probe = install.static_flash
original_input = builtins.input
serial = "TEST-PIXEL-9A"
prefix = hashlib.sha256(serial.encode()).hexdigest()[:12]
real_candidate = tomllib.loads(probe.CANDIDATE.read_text())
assert install.activation_policy({"candidate": real_candidate}) == probe.sha(
    probe.TARGET / "static-activation-candidate.toml")

with tempfile.TemporaryDirectory(prefix="pixel-9a-install-static-test.") as temporary:
    root = Path(temporary)
    bundle = root / "images"
    bundle.mkdir()
    probe_dir = root / "flash-static"
    probe_dir.mkdir()
    target = root / "target"
    target.mkdir()
    gate_manifest = target / "manifest.toml"
    gate_manifest.write_text(
        f'target_id = "{install.TARGET_ID}"\n'
        'recovery_verified_utc = "2026-09-30T01:02:38Z"\n'
        '[gates]\nidentity = "passed"\nbackup = "not-required-dev-device"\n'
        'data_policy = "dev-device-no-user-data"\n'
        'recovery_procedure = "tested"\nper_run_confirmation = "required"\n'
        'image_compatibility = "partial"\n')
    install.OUT = root
    install.EVIDENCE_DIR = root / "install-static"
    install.PROBE_DIR = probe_dir
    probe.OUT = root
    probe.TARGET = target
    reported = []
    install.print = lambda *values, **kwargs: reported.append(" ".join(map(str, values)))

    banner = ("Linux version 7.2.3-test (builder@test) (clang 19) "
              "#1 SMP PREEMPT Wed Sep 30 01:59:37 UTC 2026")
    rows = []
    image_hashes = {}
    for name in install.BOOT_PARTITIONS:
        image = bundle / f"{name}.img"
        image.write_bytes(f"test image {name}".encode() +
                          (b"\0" + banner.encode() + b"\0" if name == "boot" else b""))
        digest = probe.sha(image)
        image_hashes[name] = digest
        rows.append(f"artifact\t{name}\t{name}.img\t{image.stat().st_size}\t{digest}\tgenerated")
    rootfs = bundle / "rootfs.img"
    rootfs.write_bytes(b"test rootfs image")
    manifest = bundle / "boot-manifest.tsv"
    manifest.write_text("\n".join(rows) + "\n")
    candidate = {
        "manifest_sha256": probe.sha(manifest),
        "rootfs_image_bytes": rootfs.stat().st_size,
        "rootfs_image_sha256": probe.sha(rootfs),
        "fastboot_cli_version": "37.0.1-15733141",
    }
    boot_hash = image_hashes["boot"]
    boot_record = probe_dir / "boot-temp-test.txt"
    boot_record.write_text(
        f"operation=pixel-9a-static-boot-temp\ntarget_id={install.TARGET_ID}\n"
        f"serial_sha256_prefix={prefix}\nboot_img_sha256={boot_hash}\n"
        "boot_exit_status=0\nresult=temporary-boot-issued\noutput=Booting OKAY\n")
    kernel_log = probe_dir / "observe-temp-test-kernel.txt"
    kernel_log.write_text(f"{banner}\nZUMA PRO TEGU\n")
    observation = probe_dir / "observe-temp-test.txt"
    observation.write_text(
        f"operation=pixel-9a-static-observe-temp\ntarget_id={install.TARGET_ID}\n"
        f"serial_sha256_prefix={prefix}\nboot_img_sha256={boot_hash}\n"
        f"boot_transcript_sha256={probe.sha(boot_record)}\n"
        f"kernel_log={kernel_log.name}\nkernel_log_bytes={kernel_log.stat().st_size}\n"
        f"kernel_log_sha256={probe.sha(kernel_log)}\nresult=temporary-boot-observed\n")
    os.environ["FERRIX_PIXEL9A_NO_USER_DATA"] = probe.DATA_TOKEN
    os.environ["FERRIX_PIXEL9A_STATIC_UNSIGNED_AVB"] = install.UNSIGNED_AVB_TOKEN
    os.environ["FERRIX_PIXEL9A_STATIC_TEMP_BOOT_EVIDENCE"] = str(boot_record)
    os.environ["FERRIX_PIXEL9A_STATIC_TEMP_BOOT_OBSERVATION"] = str(observation)
    calls = []
    probe.preflight = lambda: (candidate, bundle, serial, prefix, root)

    def successful_flash(*args, **kwargs):
        if args == ("fastboot", "--version"):
            return "fastboot version 37.0.1-15733141\nInstalled as /mock/fastboot\n"
        calls.append(args)
        target = args[6] + "_b" if args[3:6] == ("--slot", "b", "flash") else args[4]
        return f"Sending '{target}' OKAY\nWriting '{target}' OKAY\n"

    probe.command = successful_flash

    try:
        install.admission()
    except install.Refusal as error:
        assert "compatibility remains partial" in str(error)
    else:
        raise AssertionError("partial image compatibility admitted persistent writes")
    assert calls == []

    gate_manifest.write_text(gate_manifest.read_text().replace(
        'image_compatibility = "partial"', 'image_compatibility = "passed"'))
    probe.command = lambda *args, **kwargs: "fastboot version 0.0.0\n"
    try:
        install.admission()
    except install.Refusal as error:
        assert "Fastboot version differs" in str(error)
    else:
        raise AssertionError("unreviewed host Fastboot was admitted")
    assert calls == []
    probe.command = successful_flash
    context = install.admission()
    assert context["serial"] == serial
    plan = install.artifact_plan(context)
    assert [part for part, _, _, _ in plan] == [f"{name}_b" for name in install.BOOT_PARTITIONS]
    assert not any(part.endswith("_a") for part, _, _, _ in plan)
    os.environ.pop("FERRIX_PIXEL9A_STATIC_UNSIGNED_AVB")
    try:
        install.mutation_policy()
    except install.Refusal as error:
        assert "unsigned-AVB token" in str(error)
    else:
        raise AssertionError("unsigned AVB was accepted without the opt-in token")
    os.environ["FERRIX_PIXEL9A_STATIC_UNSIGNED_AVB"] = install.UNSIGNED_AVB_TOKEN

    builtins.input = lambda prompt: "wrong phrase"
    try:
        install.flash_kernel()
    except install.Refusal as error:
        assert "confirmation did not match" in str(error)
    else:
        raise AssertionError("kernel stage accepted wrong confirmation")
    assert calls == []

    builtins.input = lambda prompt: "CONFIRM PIXEL 9A STATIC FLASH INACTIVE KERNEL STAGE TO SLOT B"
    install.flash_kernel()
    kernel_record = list(install.EVIDENCE_DIR.glob("flash-kernel-*.txt"))[0]
    install.kernel_record(kernel_record, context)
    assert [call[6] for call in calls] == list(install.BOOT_PARTITIONS)
    assert all(call[:6] == ("fastboot", "-s", serial, "--slot", "b", "flash")
               for call in calls)
    assert all(any(f"write_plan=partition_target={name}_b " in line and
                   f"input_image_sha256={image_hashes[name]}" in line
                   for line in reported) for name in install.BOOT_PARTITIONS)
    assert not any("set_active" in call or "reboot" in call for call in calls)

    # A remote failure after the first write leaves an incomplete transcript
    # and cannot silently continue to the other seven partitions.
    calls.clear()
    probe.command = lambda *args, **kwargs: (
        "fastboot version 37.0.1-15733141\n" if args == ("fastboot", "--version") else
        (calls.append(args) or "Sending 'boot_b' OKAY\nWriting 'boot_b' FAILED\n"))
    try:
        install.flash_kernel()
    except install.Refusal as error:
        assert "did not confirm successful flash" in str(error)
    else:
        raise AssertionError("kernel stage accepted a remote flash failure")
    assert len(calls) == 1 and calls[0][6] == "boot"
    calls.clear()
    probe.command = lambda *args, **kwargs: (
        "fastboot version 37.0.1-15733141\n" if args == ("fastboot", "--version") else
        (calls.append(args) or "Sending 'boot_b' OKAY\nWriting 'boot_a' OKAY\n"))
    try:
        install.flash_kernel()
    except install.Refusal as error:
        assert "did not confirm successful flash" in str(error)
    else:
        raise AssertionError("kernel stage accepted an active-slot write report")
    assert len(calls) == 1
    probe.command = successful_flash

    os.environ["FERRIX_PIXEL9A_STATIC_KERNEL_STAGE_EVIDENCE"] = str(kernel_record)
    calls.clear()
    builtins.input = lambda prompt: "CONFIRM PIXEL 9A STATIC FLASH SHARED USERDATA ROOTFS"
    install.flash_rootfs()
    rootfs_record = list(install.EVIDENCE_DIR.glob("flash-rootfs-*.txt"))[0]
    install.rootfs_record(rootfs_record, context, probe.sha(kernel_record))
    assert len(calls) == 1 and calls[0][:4] == ("fastboot", "-s", serial, "flash")
    assert calls[0][4] == "userdata"

    os.environ["FERRIX_PIXEL9A_STATIC_ROOTFS_STAGE_EVIDENCE"] = str(rootfs_record)
    calls.clear()
    selected = {"value": False}
    activation_candidate = target / "static-activation-candidate.toml"
    activation_candidate.write_text(
        f'schema_version = 1\ntarget_id = "{install.TARGET_ID}"\n'
        'status = "proposed-not-run"\n'
        'source_candidate = "static-install-candidate.toml"\n'
        f'source_candidate_sha256 = "{probe.sha(probe.CANDIDATE)}"\n'
        f'manifest_sha256 = "{candidate["manifest_sha256"]}"\n'
        'expected_current_slot = "a"\ntarget_slot = "b"\n'
        'operation = "select-reviewed-inactive-slot-and-reboot-only"\n'
        'kernel_stage_required = true\nrootfs_stage_required = true\n'
        'per_run_confirmation = true\n'
        'stock_recovery = "tested-pinned-full-factory-restore"\n')
    approved_proposal = activation_candidate.read_text()
    activation_candidate.write_text(approved_proposal.replace(
        'target_slot = "b"', 'target_slot = "a"'))
    try:
        install.activation_policy(context)
    except install.Refusal as error:
        assert "activation proposal differs" in str(error)
    else:
        raise AssertionError("active-slot activation proposal was accepted")
    assert calls == []
    activation_candidate.write_text(approved_proposal)

    def activation_command(*args, **kwargs):
        if args == ("fastboot", "--version"):
            return "fastboot version 37.0.1-15733141\n"
        calls.append(args)
        if args[3] == "set_active":
            selected["value"] = True
        return "OKAY\n"

    probe.command = activation_command
    probe.attached_fastboot = lambda: serial
    factory = root / "factory.zip"
    ota = root / "ota.zip"
    factory.write_bytes(b"factory recovery fixture")
    ota.write_bytes(b"ota recovery fixture")
    probe.recovery = lambda: (factory, ota)
    expected_vars = {
        "product": "tegu", "hw-revision": "MP1.0", "slot-count": "2",
        "unlocked": "yes", "secure": "yes", "is-userspace": "no",
        "snapshot-update-status": "none", "serialno": serial, "variant": "N/A",
    }
    probe.getvar = lambda actual_serial, name: (
        "b" if name == "current-slot" and selected["value"] else expected_vars[name])
    builtins.input = lambda prompt: "CONFIRM PIXEL 9A STATIC SELECT SLOT B AND BOOT FERRIX"
    install.activate()
    assert [call[3] for call in calls] == ["set_active", "reboot"]
    assert calls[0][4] == "b"
    activation_record = list(install.EVIDENCE_DIR.glob("activate-*.txt"))[0]
    install.activation_record(activation_record, context, probe.sha(kernel_record),
                              probe.sha(rootfs_record), probe.sha(activation_candidate))
    valid_activation = activation_record.read_text()
    activation_record.write_text(valid_activation.replace(
        "result=slot-b-boot-requested", "result=incomplete"))
    try:
        install.activation_record(activation_record, context, probe.sha(kernel_record),
                                  probe.sha(rootfs_record), probe.sha(activation_candidate))
    except install.Refusal as error:
        assert "activation record is incomplete" in str(error)
    else:
        raise AssertionError("incomplete activation record was accepted")
    activation_record.write_text(valid_activation)

    calls.clear()
    selected["value"] = False
    probe.getvar = lambda actual_serial, name: (
        "a" if name == "current-slot" else expected_vars[name])
    try:
        install.activate()
    except install.Refusal as error:
        assert "post-selection Fastboot current-slot differs" in str(error)
    else:
        raise AssertionError("activation rebooted without verified selected slot")
    assert [call[3] for call in calls] == ["set_active"]

    # A changed stage record cannot authorize activation or a second write.
    calls.clear()
    rootfs_record.write_text(rootfs_record.read_text().replace(
        "result=rootfs-stage-complete", "result=incomplete"))
    try:
        install.rootfs_input(context, probe.sha(kernel_record))
    except install.Refusal as error:
        assert "incomplete or differs" in str(error)
    else:
        raise AssertionError("modified rootfs stage record was accepted")
    assert calls == []

builtins.input = original_input
print('{"event":"pixel-9a-install-static-contract-test","status":"passed"}')
