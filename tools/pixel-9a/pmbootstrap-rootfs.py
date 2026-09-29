#!/usr/bin/env python3
"""Run pmbootstrap rootfs setup with the external-kernel initfs boundary.

The Pixel 9a route deliberately keeps its Google/WWAN kernel and split-boot
initramfs outside pmbootstrap. pmbootstrap 3.11.1 has no ``--no-initfs``
install option and otherwise fails after installing the complete rootfs simply
because ``/lib/modules`` is empty. Keep the normal package, account, locale,
hostname, and service setup, but skip only that kernel-owned post-install hook.
"""

import os
import sys

if os.environ.get("FERRIX_PMB_EXTERNAL_KERNEL") != "1":
    raise SystemExit("FERRIX_PMB_EXTERNAL_KERNEL=1 is required")

import pmb.chroot.initfs  # noqa: E402
from pmb import main  # noqa: E402


def external_kernel_initfs(_chroot):
    """Leave initramfs creation to the checked-in Pixel split-boot workflow."""

    return None


pmb.chroot.initfs.build = external_kernel_initfs
sys.argv[0] = "pmbootstrap"
raise SystemExit(main())
