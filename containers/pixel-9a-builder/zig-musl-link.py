#!/usr/bin/env python3
"""Run Zig's pinned lld linker for the Rust Pixel product build.

The Rust ``aarch64-unknown-linux-musl`` target spec adds the GNU ld-only
``--fix-cortex-a53-843419`` workaround to every link. Zig 0.14.1 rejects
that spelling before it reaches lld. The Pixel 9a product is not built for a
Cortex-A53 CPU, and lld does not need this legacy workaround for the target,
so remove only that one target-spec argument before invoking lld. Rust also
supplies its own musl startup objects and libraries, but dynamic musl linking
still needs the target libc and libgcc runtime objects. The builder image
provides those from pinned Alpine aarch64 packages under ``TARGET_SYSROOT``.
Invoking lld directly prevents Zig's C driver from injecting a second
``crt1.o``; all explicit Rust objects, ``-L`` paths, and libraries are retained
while the target sysroot is always selected explicitly.
"""

from __future__ import annotations

import subprocess
import sys


ZIG = "/opt/zig/zig"
TARGET_SYSROOT = "/opt/ferrix/aarch64-linux-musl-sysroot"
LEGACY_CORTEX_A53_FIX = "--fix-cortex-a53-843419"


def normalize_link_args(args: list[str]) -> list[str]:
    """Translate compiler-driver linker flags into direct lld arguments."""

    normalized: list[str] = []
    index = 0
    while index < len(args):
        arg = args[index]
        if arg in ("-nostartfiles", "-nodefaultlibs"):
            index += 1
            continue
        if arg == LEGACY_CORTEX_A53_FIX:
            index += 1
            continue
        if arg.startswith("-Wl,"):
            for linker_arg in arg[4:].split(","):
                if linker_arg and linker_arg != LEGACY_CORTEX_A53_FIX:
                    normalized.append(linker_arg)
            index += 1
            continue
        if (
            arg == "-Xlinker"
            and index + 1 < len(args)
        ):
            linker_arg = args[index + 1]
            if linker_arg != LEGACY_CORTEX_A53_FIX:
                normalized.append(linker_arg)
            index += 2
            continue
        normalized.append(arg)
        index += 1
    return normalized


def main() -> int:
    return subprocess.run(
        [
            ZIG,
            "ld.lld",
            "-m",
            "aarch64elf",
            f"--sysroot={TARGET_SYSROOT}",
            f"-L{TARGET_SYSROOT}/usr/lib",
            *normalize_link_args(sys.argv[1:]),
        ],
        check=False,
    ).returncode


if __name__ == "__main__":
    raise SystemExit(main())
