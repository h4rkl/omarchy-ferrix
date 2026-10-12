#!/usr/bin/env python3
"""The docs/status.md "Current state" tables must equal the capability matrices.

Python >= 3.11 (tomllib). Run: python3 tools/dev/tests/status-current-state.py

- Every `targets/**/capabilities.toml` target has a `### \\`<target_id>\\``
  table whose capability rows are exactly the TOML's capability sections, with
  the Status cell equal to `status` and the Evidence cell equal to `evidence`,
  verbatim.
- `qemu-aarch64` has no TOML; its matrix is
  `CapabilityMatrix::uniform(CapabilityStatus::Mocked)` in
  ferrix-hardware-emulator. Its table must list every capability named in
  ferrix-hardware-api as `mocked`, and the emulator source must still build the
  matrix that way (otherwise this test and the table need updating together).
- No other target tables exist.
"""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
STATUS = ROOT / "docs/status.md"
HW_API = ROOT / "crates/ferrix-hardware-api/src/lib.rs"
EMULATOR = ROOT / "crates/ferrix-hardware-emulator/src/lib.rs"
QEMU_TARGET = "qemu-aarch64"

failures: list[str] = []


def fail(msg: str) -> None:
    failures.append(msg)
    print(f"not ok - {msg}")


def ok(msg: str) -> None:
    print(f"ok - {msg}")


def unquote(cell: str) -> str:
    cell = cell.strip()
    if len(cell) >= 2 and cell[0] == cell[-1] == "`":
        return cell[1:-1]
    return cell


def parse_current_state() -> dict[str, list[dict[str, str]]]:
    text = STATUS.read_text(encoding="utf-8")
    m = re.search(r"(?ms)^## Current state\n(.*?)(?=^## |\Z)", text)
    if not m:
        fail("docs/status.md has no '## Current state' section")
        return {}
    tables: dict[str, list[dict[str, str]]] = {}
    target = None
    header: list[str] | None = None
    for line in m.group(1).splitlines():
        h = re.match(r"^### `([^`]+)`\s*$", line)
        if h:
            target = h.group(1)
            if target in tables:
                fail(f"duplicate table for {target}")
            tables[target] = []
            header = None
            continue
        if target is None or not line.startswith("|"):
            header = None
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if header is None:
            header = cells
            continue
        if all(set(c) <= set("-: ") for c in cells):
            continue
        if len(cells) != len(header):
            fail(f"{target}: row has {len(cells)} cells, header has {len(header)}: {line[:80]}")
            continue
        tables[target].append(dict(zip(header, cells)))
    return tables


def hardware_api_capabilities() -> list[str]:
    names = re.findall(r'Capability::\w+ => "([a-z-]+)"', HW_API.read_text(encoding="utf-8"))
    if not names:
        fail("could not read capability names from ferrix-hardware-api")
    return names


def check_toml_target(path: Path, rows: list[dict[str, str]]) -> None:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    target = data["target_id"]
    matrix = {k: v for k, v in data.items() if isinstance(v, dict)}
    seen = {}
    for row in rows:
        cap = unquote(row.get("Capability", ""))
        if cap in seen:
            fail(f"{target}: capability {cap!r} listed twice")
        seen[cap] = row
    if set(seen) != set(matrix):
        fail(f"{target}: table capabilities {sorted(seen)} != matrix {sorted(matrix)}")
    bad = 0
    for cap, entry in matrix.items():
        row = seen.get(cap)
        if row is None:
            continue
        if unquote(row.get("Status", "")) != entry["status"]:
            fail(f"{target}/{cap}: Status {row.get('Status')!r} != matrix {entry['status']!r}")
            bad += 1
        if "Evidence" not in row:
            fail(f"{target}: table has no Evidence column")
            bad += 1
        elif row["Evidence"] != entry["evidence"]:
            fail(f"{target}/{cap}: Evidence differs from capabilities.toml")
            bad += 1
    if not bad and set(seen) == set(matrix):
        ok(f"{target}: {len(matrix)} capabilities match {path.relative_to(ROOT)}")


def check_qemu(rows: list[dict[str, str]]) -> None:
    src = EMULATOR.read_text(encoding="utf-8")
    m = re.search(r"pub fn capability_matrix\(\) -> CapabilityMatrix \{\s*(.*?)\s*\}", src, re.S)
    if not m or m.group(1) != "CapabilityMatrix::uniform(CapabilityStatus::Mocked)":
        fail("ferrix-hardware-emulator capability_matrix() is no longer uniform Mocked; update the qemu-aarch64 table and this test")
        return
    caps = hardware_api_capabilities()
    got = {unquote(r.get("Capability", "")): unquote(r.get("Status", "")) for r in rows}
    if sorted(got) != sorted(caps):
        fail(f"{QEMU_TARGET}: table capabilities {sorted(got)} != hardware API {sorted(caps)}")
    wrong = {c: s for c, s in got.items() if s != "mocked"}
    if wrong:
        fail(f"{QEMU_TARGET}: non-mocked rows {wrong}")
    if sorted(got) == sorted(caps) and not wrong:
        ok(f"{QEMU_TARGET}: {len(caps)} capabilities are mocked, matching ferrix-hardware-emulator")


def main() -> int:
    tables = parse_current_state()
    tomls = sorted((ROOT / "targets").rglob("capabilities.toml"))
    expected = {QEMU_TARGET}
    for path in tomls:
        target = tomllib.loads(path.read_text(encoding="utf-8"))["target_id"]
        expected.add(target)
        if target not in tables:
            fail(f"docs/status.md Current state has no table for {target}")
            continue
        check_toml_target(path, tables[target])
    if QEMU_TARGET in tables:
        check_qemu(tables[QEMU_TARGET])
    else:
        fail(f"docs/status.md Current state has no table for {QEMU_TARGET}")
    extra = set(tables) - expected
    if extra:
        fail(f"Current state has tables for unknown targets: {sorted(extra)}")
    if failures:
        print(f"status-current-state: {len(failures)} failure(s)", file=sys.stderr)
        return 1
    print("status-current-state: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
