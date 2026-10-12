#!/usr/bin/env python3
"""Structural and history checks for the dated evidence log (#119).

Stdlib only (Python >= 3.11). Run: python3 tools/dev/tests/status-log.py

1. Every `docs/status-log/*.md` is named `YYYY-MM.md`, starts with a
   `# Status log: YYYY-MM` header, and after that header contains only dated
   entries: blocks that each start with `## YYYY-MM-DD` or
   `## YYYY-MM-DD (summary)`, carry a real calendar date in that file's month,
   and run newest first. A handful of historical entries were recorded out of
   order before the split; they are listed in OUT_OF_ORDER and moved unchanged.
2. `docs/status.md` holds no dated entries (they belong in the log).
3. History: when the pre-split `docs/status.md` blob is present (any full
   clone), each month's pre-split entries must still be the byte-identical
   tail of that month's log file, in the same order. New entries go on top, so
   this holds forever unless historical evidence is altered or removed; record
   a correction as a new entry instead. Shallow clones (CI's default checkout)
   skip this part and say so.
"""

from __future__ import annotations

import datetime as dt
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
LOG_DIR = ROOT / "docs/status-log"
STATUS = ROOT / "docs/status.md"

# Blob of docs/status.md immediately before the split (origin/main 38e96f2 and
# 6f14e29 carry the same blob).
PRE_SPLIT_BLOB = "09e183edf9df00dd9dc05eed47e653aef0de27b1"

# Exact headings recorded out of date order before the split. They are kept
# where they were; no new entry may be added here.
OUT_OF_ORDER = {
    "## 2026-10-11 (Pixel 9a stock restore attempt; physical:google-pixel-9a-tegu-gxq96-mp1-0)",
}

HEADING = re.compile(r"^## (\d{4})-(\d{2})-(\d{2})(?: \((.+)\))?$")
DATED = re.compile(r"^## \d{4}-\d{2}-\d{2}\b", re.M)

failures: list[str] = []


def fail(msg: str) -> None:
    failures.append(msg)
    print(f"not ok - {msg}")


def ok(msg: str) -> None:
    print(f"ok - {msg}")


def split_blocks(text: str) -> tuple[str, list[str]]:
    """Return (text before the first `## ` line, list of `## ` blocks)."""
    parts = re.split(r"(?m)^(?=## )", text)
    return parts[0], parts[1:]


def month_of(block: str) -> str | None:
    m = HEADING.match(block.split("\n", 1)[0])
    return f"{m[1]}-{m[2]}" if m else None


def check_log() -> dict[str, list[str]]:
    by_month: dict[str, list[str]] = {}
    files = sorted(LOG_DIR.glob("*"))
    if not files:
        fail("docs/status-log/ has no files")
    for path in files:
        name = path.name
        m = re.fullmatch(r"(\d{4})-(\d{2})\.md", name)
        if not m:
            fail(f"{name}: not named YYYY-MM.md")
            continue
        month = f"{m[1]}-{m[2]}"
        text = path.read_text(encoding="utf-8")
        header, blocks = split_blocks(text)
        if not header.startswith(f"# Status log: {month}\n"):
            fail(f"{name}: does not start with '# Status log: {month}'")
        if not blocks:
            fail(f"{name}: has no dated entries")
        prev: dt.date | None = None
        good = True
        for block in blocks:
            line = block.split("\n", 1)[0]
            h = HEADING.match(line)
            if not h:
                fail(f"{name}: malformed entry heading {line!r}")
                good = False
                continue
            try:
                date = dt.date(int(h[1]), int(h[2]), int(h[3]))
            except ValueError:
                fail(f"{name}: invalid date in {line!r}")
                good = False
                continue
            if f"{h[1]}-{h[2]}" != month:
                fail(f"{name}: entry {line!r} is not in {month}")
                good = False
            if line in OUT_OF_ORDER:
                continue
            if prev is not None and date > prev:
                fail(f"{name}: {line!r} is newer than the entry above it (newest first)")
                good = False
            prev = date
        if good:
            ok(f"{name}: {len(blocks)} well-formed entries, newest first")
        by_month[month] = blocks
    return by_month


def check_status_has_no_entries() -> None:
    hits = DATED.findall(STATUS.read_text(encoding="utf-8"))
    if hits:
        fail(f"docs/status.md has {len(hits)} dated entries; move them to docs/status-log/")
    else:
        ok("docs/status.md holds no dated entries")


def check_history(by_month: dict[str, list[str]]) -> None:
    res = subprocess.run(
        ["git", "-C", str(ROOT), "cat-file", "-p", PRE_SPLIT_BLOB],
        capture_output=True,
    )
    if res.returncode != 0:
        print(f"skip - history check: pre-split blob {PRE_SPLIT_BLOB[:12]} not in this clone")
        return
    original = res.stdout.decode("utf-8")
    preamble, blocks = split_blocks(original)
    if preamble != "# Status\n\n":
        fail("history: unexpected pre-split preamble")
    if not STATUS.read_text(encoding="utf-8").startswith(preamble):
        fail("history: docs/status.md no longer starts with the pre-split preamble")
    expected: dict[str, list[str]] = {}
    for block in blocks:
        expected.setdefault(month_of(block) or "?", []).append(block)
    total = 0
    for month, want in expected.items():
        have = by_month.get(month, [])
        tail = have[len(have) - len(want):] if len(have) >= len(want) else have
        if tail != want:
            fail(f"history: {month}.md does not end with the {len(want)} pre-split {month} entries byte-for-byte")
        else:
            total += len(want)
    if total == len(blocks):
        ok(f"history: all {len(blocks)} pre-split entries are present byte-for-byte, in order")


def main() -> int:
    by_month = check_log()
    check_status_has_no_entries()
    check_history(by_month)
    if failures:
        print(f"status-log: {len(failures)} failure(s)", file=sys.stderr)
        return 1
    print("status-log: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
