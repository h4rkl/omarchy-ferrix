#!/usr/bin/env python3
"""Run Zig's pinned clang frontend with a Kbuild dependency-file shim.

Zig 0.14 accepts GCC's ``-Wp,-MMD,<path>`` spelling but treats it as a
dependency-only invocation, so a Kbuild compile that also asks for ``-S`` or
``-c`` silently omits its requested output.  Kbuild relies on that GCC
spelling.  Compile once without the forwarding flag, then run a preprocessor
only pass to emit the dependency file in the expected location.
"""

from __future__ import annotations

import os
import subprocess
import sys


ZIG = "/opt/zig/zig"


def normalize_compile_args(args: list[str]) -> list[str]:
    # The kernel's arm32 VDSO helper uses the spelling -march=armv8-a.  Zig's
    # LLVM target table accepts the ARMv8-A 32-bit core as `cortex_a32` (the
    # generic ARM target is only ARMv4T, which cannot assemble the barrier
    # instructions used by the compat VDSO). Preserve the intended baseline
    # instead of allowing an otherwise unrelated helper build to fail on the
    # spelling difference.
    normalized: list[str] = []
    needs_el3 = False
    for arg in args:
        if arg == "-march=armv8-a":
            normalized.append("-mcpu=cortex_a32")
        elif arg == "-mcpu=armv8":
            normalized.append("-mcpu=generic")
        else:
            normalized.append(arg)
        if arg == "-Wa,-march=armv8.5-a":
            needs_el3 = True
    # Kbuild's arm64 platform helpers use an inline `smc #0`.  The original
    # assembler dialect flag must remain visible to Zig, while its generic
    # CPU model also needs the explicit EL3 extension or LLVM rejects that
    # privileged instruction. Append this after the original arguments so a
    # later generic -mcpu flag cannot silently remove +el3.
    if needs_el3:
        normalized.append("-mcpu=generic+el3")
    return normalized


def split_dependency_flag(args: list[str]) -> tuple[list[str], str | None]:
    compile_args: list[str] = []
    dependency_path: str | None = None
    for arg in args:
        if arg.startswith("-Wp,-MMD,") or arg.startswith("-Wp,-MD,"):
            dependency_path = arg.rsplit(",", 1)[1]
            continue
        compile_args.append(arg)
    return compile_args, dependency_path


def dependency_args(args: list[str]) -> list[str]:
    result: list[str] = []
    skip_next = False
    for arg in args:
        if skip_next:
            skip_next = False
            continue
        if arg in ("-S", "-c"):
            continue
        if arg == "-o":
            skip_next = True
            continue
        if arg.startswith("-o") and len(arg) > 2:
            continue
        result.append(arg)
    return result


def main() -> int:
    compile_args, dependency_path = split_dependency_flag(sys.argv[1:])
    compile_args = normalize_compile_args(compile_args)
    command = [ZIG, "cc", *compile_args]
    completed = subprocess.run(command, check=False)
    if completed.returncode != 0 or dependency_path is None:
        return completed.returncode

    dependency_command = [
        ZIG,
        "cc",
        *dependency_args(compile_args),
        "-MMD",
        "-MF",
        dependency_path,
        "-E",
        "-o",
        os.devnull,
    ]
    return subprocess.run(
        dependency_command,
        check=False,
        stdout=subprocess.DEVNULL,
    ).returncode


if __name__ == "__main__":
    raise SystemExit(main())
