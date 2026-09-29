#!/usr/bin/env python3
"""Run Zig's pinned musl linker driver for the Rust Pixel product build.

The Rust ``aarch64-unknown-linux-musl`` target spec adds the GNU ld-only
``--fix-cortex-a53-843419`` workaround to every link. Zig 0.14.1 rejects
that spelling before it reaches lld. The Pixel 9a product is not built for a
Cortex-A53 CPU, and lld does not need this legacy workaround for the target,
so remove only that one target-spec argument before invoking Zig. Keep all
other linker arguments byte-for-byte intact.
"""

from __future__ import annotations

import subprocess
import sys


ZIG = "/opt/zig/zig"
LEGACY_CORTEX_A53_FIX = "--fix-cortex-a53-843419"


def normalize_link_args(args: list[str]) -> list[str]:
    """Drop only the GNU ld Cortex-A53 workaround in any common spelling."""

    normalized: list[str] = []
    index = 0
    while index < len(args):
        arg = args[index]
        if arg in (LEGACY_CORTEX_A53_FIX, f"-Wl,{LEGACY_CORTEX_A53_FIX}"):
            index += 1
            continue
        if (
            arg == "-Xlinker"
            and index + 1 < len(args)
            and args[index + 1] == LEGACY_CORTEX_A53_FIX
        ):
            index += 2
            continue
        normalized.append(arg)
        index += 1
    return normalized


def main() -> int:
    return subprocess.run(
        [ZIG, "cc", "-target", "aarch64-linux-musl", *normalize_link_args(sys.argv[1:])],
        check=False,
    ).returncode


if __name__ == "__main__":
    raise SystemExit(main())
