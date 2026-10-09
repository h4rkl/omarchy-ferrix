# Pixel 9a native Linux pmOS package

This directory is the target-local postmarketOS device package for the exact
Google Pixel 9a `GXQ96` / `tegu` / `MP1.0` admission. It is deliberately
separate from `targets/qemu-aarch64/pmos`: it selects native Linux services
and does not make any QEMU evidence a physical-device claim. By default it
does not install Android or Waydroid.

Boundary change (owner request, 2026-10-09): the **development route only**
may optionally carry Waydroid as an *application runtime* for the Browser tile
(Fulguris, ADR-005/ADR-007). It is never the operating environment. Opt in
with `FERRIX_PIXEL9A_ANDROID=1` for `tools/pixel-9a/rootfs.sh`, which appends
the package set in `android/packages` (Waydroid, LXC, dnsmasq, iptables plus iptables-legacy;
deliberately no `nftables`/`waydroid-nftrules`, because the
pinned GKI has no `CONFIG_NF_TABLES`) and refuses a result containing
`waydroid-nftrules`. postmarketos-base still installs `nftables`; on the
GKI `nft list ruleset` fails, so `waydroid-net.sh` uses `iptables-legacy`
(Alpine's plain `iptables` is the nft backend and would fail too). `tools/pixel-9a/dev-compose.py --android` then grows the rootfs
(default 8 GiB) and stages the QEMU route's pinned LineageOS Waydroid images
and Fulguris APK (verified against `tools/android/*.pins`), the shared guest
bootstrap `tools/android/guest/ferrix-waydroid.sh`, the renderer properties in
`android/waydroid-properties.ini` (SwiftShader, `gralloc=default`, 400 dpi),
and the `ferrix-android` runlevel link. The package installs
`ferrix-android.initd` but never links it itself. Without the opt-in, images
are unchanged apart from that unlinked script and ferrix-appd's
session-bus address (unused when nothing listens there).

The package is an input to `tools/pixel-9a/rootfs.sh`. It is not itself a
bootable Pixel image. The selected route needs the matching radio-first
Kbuild handoff, split-boot composition, exact firmware staging, and the recovery and
physical-device gates in `../linux-plan.md` before any boot attempt.

## Package contents

- `deviceinfo` describes the aarch64 handset and records the verified
  Android header-v4/split-boot shape without selecting a flash method.
- `kernel-cmdline.conf` and `modules-initfs-radio` provide conservative
  early-boot inputs for the selected radio-first Linux root filesystem on UFS.
  `modules-initfs` remains the separate Google Kleaf comparison-route contract;
  `tools/pixel-9a/rootfs.sh` overlays the radio list into the staged package so
  the incompatible bcmdhd/nitrous/cpif module names are never requested by the
  WWAN/SIT image.
- `initramfs-init`, `vendor-bootconfig`, the two DLKM policy files, and the
  `vendor-ramdisk/ferrix/` markers are source-owned inputs for
  `tools/pixel-9a/boot-inputs.sh`; they are hash-recorded before split-image
  composition and do not contain Android framework or stock module payloads.
- `boot-layout.toml` records the expected split artifacts. It is metadata for a
  later artifact composer, not a flashing instruction.
- The OpenRC service files enable Ferrix core/network/telephony, the native
  NetworkManager/ModemManager/BlueZ/UPower services, seatd, a target-local
  PipeWire/WirePlumber session wrapper, and the Ferrix compositor/app service.
- `ferrix-radio-firmware` runs before NetworkManager, BlueZ, and the S5300
  chain. It re-hashes every staged firmware row, checks the required allowlist,
  and verifies the four radio-first BCM4383 aliases. A missing, changed, or
  symlinked payload stops radio-service startup instead of producing a false
  working-radio surface.
- `device-google-tegu/firmware-policy.toml` and
  `device-google-tegu/firmware-allowlist.tsv` describe the external,
  exact-build firmware contract. Firmware bytes are never checked in and are
  accepted by the rootfs workflow only from a directory with a matching
  `firmware-manifest.tsv`.
- `pins.env` carries the physical target's pmaports repository/channel/commit
  pin. The rootfs workflow records and uses this pin by default; it never
  falls back to the separate QEMU substrate pin.

## Rootfs workflow

The checks are device-free and can run on any host:

```sh
just pixel-9a-rootfs-check
just pixel-9a-rootfs-test
```

Building requires native Linux and an explicitly supplied, clean pmaports
checkout at the target-local pinned commit. It also requires the completed
radio-first Kbuild handoff, the separately built Ferrix product handoff from
the target-scoped native Linux Cargo environment, and external firmware
manifest:

```sh
FERRIX_PIXEL9A_PRODUCT_BUILD=I-understand-this-builds-the-pixel-9a-product \
  just pixel-9a-product-build
just pixel-9a-rootfs-validate
FERRIX_PIXEL9A_ROOTFS_BUILD=I-understand-this-runs-pmbootstrap \
  FERRIX_PIXEL9A_PMAPORTS_DIR=/path/to/pmaports \
  FERRIX_PIXEL9A_RADIO_KERNEL_DIST=out/pixel-9a/kernel-radio/tegu-wwan/dist \
  FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST=out/pixel-9a/kernel-radio/tegu-wwan/build-manifest.tsv \
  FERRIX_PIXEL9A_PRODUCT_DIR=out/pixel-9a/product/tegu \
  FERRIX_PIXEL9A_FIRMWARE_DIR=/path/to/verified-firmware \
  just pixel-9a-rootfs-build
```

The stable default manifest is:

```text
out/pixel-9a/rootfs/tegu/manifest.tsv
```

Set `FERRIX_PIXEL9A_ROOTFS_MANIFEST` to override it. Every produced artifact
has a machine-readable row of the form:

```text
artifact\t<name>\t<bytes>\t<sha256>
```

The workflow writes both a deterministic `rootfs.tar` handoff and a raw ext4
`rootfs.img` (default 2 GiB, configurable with
`FERRIX_PIXEL9A_ROOTFS_IMAGE_SIZE_MIB`) using native Linux `mke2fs`. Both are
hash-recorded; the rootfs manifest remains `built-substrate-only` until the
separate boot artifact validator and physical evidence gates pass. The
workflow never calls `adb`, `fastboot`, `pmbootstrap flasher`, or any image
extraction tool for firmware.

The target product output is required to carry the exact Pixel target id,
physical evidence scope, native-Linux backend mode, and `bootable=false` in
its manifest. QEMU product output is never used by the default path; the
staged `/etc/ferrix/profile.toml` always comes from `../dev/profile.toml`.
Synthetic QEMU radio state is therefore not copied into the physical target
guest.

## Opt-in Android runtime (dev route, owner request 2026-10-09)

Build steps on the Linux build host. Each output directory must be new:

1. Host inputs, the same pins as the QEMU route: run `just waydroid-images-fetch`
   and `sh tools/android/fulguris-fetch.sh` (needs `apksigner`). Or copy an
   already verified `out/android/` tree; `dev-compose` re-verifies size and
   SHA-256 against `tools/android/*.pins`.
2. Base rootfs with the package set:
   `FERRIX_PIXEL9A_ANDROID=1 FERRIX_PIXEL9A_ROOTFS_BUILD=… tools/pixel-9a/native-builder.sh run rootfs build`,
   with the usual product, firmware and pmaports inputs and a fresh
   `FERRIX_PIXEL9A_ROOTFS_OUT`. The rootfs is composed with the Kleaf GKI,
   so select that kernel explicitly: `FERRIX_PIXEL9A_ROOTFS_KERNEL_ROUTE=kleaf-dev`
   plus `FERRIX_PIXEL9A_KERNEL_DIST=<kleaf dist>`. Without it, the default
   `radio-first` route needs a radio kernel built against the current split
   contract, and that kernel currently refuses to build because there is no
   MP1.0 overlay source. The manifest records `route_id
   development-google-kleaf-gki`, the hashed Kleaf kernel manifest, and
   `android_runtime waydroid-packages …`.
3. Image: `FERRIX_PIXEL9A_DEV_ANDROID=1 FERRIX_PIXEL9A_DEV_BASE_ROOTFS=<that rootfs.img>
   FERRIX_PIXEL9A_KERNEL_OUT=… FERRIX_PIXEL9A_DEV_PRODUCT_DIR=… FERRIX_PIXEL9A_DEV_OUT=<new dir>
   tools/pixel-9a/native-builder.sh run dev-compose build`. The ext4
   rootfs grows to `FERRIX_PIXEL9A_DEV_ROOTFS_SIZE_MIB` (default 8192). The
   `userdata.img` sparse image therefore carries about 2 GiB more data, and
   its `manifest.tsv` lists every staged Android file with size and SHA-256.

On first boot, the background `ferrix-android` service does the following
(log: `/var/log/ferrix-android.log`, working files under `/run/ferrix-android/`):

- runs offline `waydroid init` from `/etc/waydroid-extra/images`;
- applies the SwiftShader/400 dpi properties and runs `waydroid upgrade -o`;
- starts `waydroid-container`;
- waits for ferrix-shell's `/run/user/1000/wayland-0`;
- as `user`, starts a session bus at `/run/user/1000/bus` and
  `waydroid session start` (no pulse socket, so Android has no audio yet);
- waits for Android `sys.boot_completed`;
- installs Fulguris as `user`;
- assigns the browser role.

Later boots skip the init and the install. ferrix-appd then launches the
Browser tile through the unchanged `ferrix-runtime-android` adapter.

## S5300 native-Linux package boundary

The device package carries a target-local, source-pinned radio stack:

- `exynos-modem` and `sit-proxy` are built from the immutable
  `634e8eb404c3fcf959dce6a0b0d97b33ba0fc346` archive (GPL-2.0-or-later).
- `modemmanager-samsung-sit` is built from the immutable
  `14a0013a28e5d5eec513c65e75d7549678ecc61e` archive (GPL-2.0-or-later and
  LGPL-2.1-or-later), with the upstream `COPYING`/`COPYING.LIB` installation
  retained by Meson.
- `device-google-tegu/s5300-userspace-sources.toml` repeats both archive URLs,
  SHA-256/SHA-512 pins, SPDX licenses, package names, and the native Linux
  boundary. The recipes fail closed on a changed archive; no external source,
  modem image, NV data, or device identifier is vendored here.

`ferrix-s5300-mounts` resolves only the exact current-slot modem label
(`modem_a` or `modem_b`, from the bootloader's
`androidboot.slot_suffix`) plus the single-label `efs`, `persist`, and
`modem_userdata` partitions, and mounts them read-only at the paths the
reviewed daemon consumes. It refuses a missing, ambiguous, foreign, or
writable mount; it never guesses `_a` when Ferrix is booted from slot B.
OpenRC orders the mounts, `exynos-modem`, `sit-proxy`, and the Samsung SIT
ModemManager provider before Ferrix's telephony adapter. The reviewed factory
layout supplies directory-valued inputs at `vendor.img:/firmware/carrierconfig`
and `vendor.img:/firmware/uecapconfig`; the native-Linux destinations are
`/lib/firmware/carrierconfig` and `/lib/firmware/uecapconfig`, exposed to the
services through the rootfs `/vendor/firmware` compatibility root. These
directories and all per-device NV remain external inputs under the firmware
policy; they are never staged in Git or copied from the Android userspace tree.
The firmware stager now expands the two reviewed directory bundles into
destination-aware per-file manifest rows and fails closed on unsafe or special
entries. This wiring is a build/provisioning route, not physical radio
evidence: CP boot, SIM, registration, data bearer, SMS, voice/IMS, Wi-Fi, and
Bluetooth capabilities remain unsupported until observed on this exact
hardware revision.
