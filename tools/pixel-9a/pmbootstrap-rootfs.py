#!/usr/bin/env python3
"""Run pmbootstrap rootfs setup with the external-kernel initfs boundary.

The Pixel 9a route deliberately keeps its Google/WWAN kernel and split-boot
initramfs outside pmbootstrap. pmbootstrap 3.11.1 has no ``--no-initfs``
install option and otherwise fails after installing the complete rootfs simply
because ``/lib/modules`` is empty. Keep the normal package, account, locale,
hostname, and service setup, but skip only that kernel-owned post-install hook.

The one-time guest password arrives in ``FERRIX_PMB_ROOTFS_PASSWORD`` rather
than argv. pmbootstrap 3.11.1 parses ``--password <value>`` with argparse, which
rejects a generated value beginning with ``-``; its ``--password=<value>`` form
parses, but pmbootstrap's argv redaction matches only a separate ``--password``
element and would log the value in plain text. Inject the value into the parsed
arguments after pmbootstrap has logged its argv instead.
"""

import os
import sys

if os.environ.get("FERRIX_PMB_EXTERNAL_KERNEL") != "1":
    raise SystemExit("FERRIX_PMB_EXTERNAL_KERNEL=1 is required")

# Remove the value before pmbootstrap starts so its subprocesses never inherit it.
rootfs_password = os.environ.pop("FERRIX_PMB_ROOTFS_PASSWORD", "")
if not rootfs_password:
    raise SystemExit("FERRIX_PMB_ROOTFS_PASSWORD is required")
if any(arg == "--password" or arg.startswith("--password=") for arg in sys.argv[1:]):
    raise SystemExit("pass the rootfs password in FERRIX_PMB_ROOTFS_PASSWORD, not argv")

import pmb.chroot.initfs  # noqa: E402
import pmb.parse  # noqa: E402
from pmb import main  # noqa: E402


def external_kernel_initfs(_chroot):
    """Leave initramfs creation to the checked-in Pixel split-boot workflow."""

    return None


pmbootstrap_arguments = pmb.parse.arguments


def arguments_with_rootfs_password():
    """Parse and log argv as pmbootstrap does, then supply the password."""

    args = pmbootstrap_arguments()
    if args.action != "install":
        raise SystemExit("the rootfs password wrapper only supports pmbootstrap install")
    args.password = rootfs_password
    return args


pmb.chroot.initfs.build = external_kernel_initfs
pmb.parse.arguments = arguments_with_rootfs_password
sys.argv[0] = "pmbootstrap"
raise SystemExit(main())
