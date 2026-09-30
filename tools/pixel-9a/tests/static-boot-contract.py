#!/usr/bin/env python3
"""Offline regression checks for the static route's capacity admission gate."""

import importlib.util
import tempfile
from pathlib import Path


script = Path(__file__).resolve().parents[1] / "static-boot.py"
spec = importlib.util.spec_from_file_location("pixel9a_static_boot", script)
assert spec is not None and spec.loader is not None
workflow = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workflow)

capacities = {
    "boot": 67108864,
    "init_boot": 8388608,
    "vendor_boot": 67108864,
    "vendor_kernel_boot": 67108864,
    "dtbo": 16777216,
    "vbmeta": 65536,
    "vbmeta_vendor": 65536,
    "vbmeta_system": 65536,
}


def handoff(timestamp: str, items: dict[str, int] = capacities) -> str:
    return "\n".join(
        [
            "# source=Fastboot getvar partition-size:<partition>_<slot>",
            f"# observed_utc={timestamp}",
            "# exact-target=google-pixel-9a-tegu-gxq96-mp1-0 product=tegu revision=MP1.0 sku=GXQ96 current_slot=a is_userspace=no",
            "schema_version\t2",
            "target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0",
            "partition_contract\tstatic-boot-only",
            *(f"partition\t{name}\t{value}" for name, value in items.items()),
            "",
        ]
    )


def expect_refusal(path: Path, text: str, reason: str) -> None:
    path.write_text(text)
    try:
        workflow.capacity_handoff()
    except workflow.Refusal as error:
        assert reason in str(error), (reason, str(error))
    else:
        raise AssertionError(f"capacity handoff unexpectedly accepted: {reason}")


with tempfile.TemporaryDirectory(prefix="ferrix-static-capacity-test.") as temporary:
    path = Path(temporary) / "capacity.tsv"
    workflow.CAPACITIES = path
    expect_refusal(path, handoff("2026-09-30T00:17:38Z"), "predate the verified stock restore")
    path.write_text(handoff("2026-09-30T01:30:00Z"))
    assert workflow.capacity_handoff() == capacities
    expect_refusal(path, handoff("2026-09-30T01:30:00Z") + "partition\tboot\t67108864\n",
                   "duplicate")
    expect_refusal(path, handoff("2026-09-30T01:30:00Z", {**capacities, "vendor_dlkm": 4096}),
                   "unknown")
    expect_refusal(path, handoff("2026-09-30T01:30:00Z").replace("partition\tboot\t67108864",
                                                                       "partition\tboot\t67108863"),
                   "4096-byte aligned")
    expect_refusal(path, handoff("2026-09-30T01:30:00Z").replace("is_userspace=no", "is_userspace=yes"),
                   "ordinary-bootloader provenance")

assert workflow.relative("provenance/kernel-manifest.tsv") == Path("provenance/kernel-manifest.tsv")
for unsafe in ("../kernel-manifest.tsv", "/tmp/kernel-manifest.tsv", "provenance/../manifest.tsv"):
    try:
        workflow.relative(unsafe)
    except workflow.Refusal:
        pass
    else:
        raise AssertionError(f"unsafe relative path accepted: {unsafe}")

print('{"event":"pixel-9a-static-boot-contract-test","status":"passed"}')
