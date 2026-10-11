# S5300 CPIF source boundary

This file records what the pinned Google S5300 source proves for the exact
`tegu` target. It is source evidence, not physical-device evidence. The
target capability matrix must stay `unsupported` until a native Ferrix Linux
boot on the exact `GXQ96` / `MP1.0` device produces runtime evidence.

## Pinned inputs

- Driver repository: [`google-modules/radio/samsung/s5300`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/)
- Driver revision: `a8976934fe7f396627f9900958eb1457a74ebcca`
- Board DTS repository: [`kernel/devices/google/zuma`](https://android.googlesource.com/kernel/devices/google/zuma/)
- Board DTS revision: `42395e8adcb880aa336f0da6bb2a19ed53929c40`
- Board protocol include: `dts/zuma-cp-s5300-sit.dtsi`
- Exact tegu config fragment: [`tegu_defconfig`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/tegu_defconfig)

The pinned board include selects `PROTOCOL_SIT`. The checked-in
`pmos/device-google-tegu/s5300-channels.toml` records the complete DTS
I/O-device catalog and the two expanded `umts_ipc` endpoints
(`/dev/umts_ipc0` and `/dev/umts_ipc1`) without treating those names as proof
that the nodes exist on a booted phone. Their source constants are kept
distinct (`EXYNOS_CH_ID_FMT_0` = 245 and `EXYNOS_CH_ID_FMT_1` = 246), matching
the per-channel definitions in Google's constants header. It also records the
30 source-defined `rmnet` network channels and the eight `oem_ipc` expansions
as symbolic metadata only.

The target service passes `/dev/umts_oem1` to the pinned upstream
`exynos-modem` CLI for `--oem-gems-dev`. This is the CLI's reviewed legacy
device-node contract; the `oem_ipc0`–`oem_ipc7` entries above are the
source-derived channel expansion and are not an unreviewed udev alias. A
booted Tegu must still confirm that the node is created and usable before any
telephony capability is promoted.

The pinned tegu Kleaf source also supplies the checked-in
[`kernel-config.tegu`](kernel-config.tegu) fragment. Its S5300 entries select
`EXYNOS_MODEM_IF=m`, `SEC_MODEM_S5100=m`, `SHM_IPC=m`, packet processing,
PCIe IOCC/IOMMU, CP thermal support, and the voice-call suspend hook. The
kernel workflow compares that fragment with the exact
`private/devices/google/tegu/tegu_defconfig` from the pinned source checkout;
it does not infer configuration from an Android userspace image.

The pinned S5300 [`BUILD.bazel`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/BUILD.bazel)
emits `cpif.ko`, `cpif_page.ko`, `shm_ipc.ko`, and `cp_thermal_zone.ko`.
The target firmware policy records `cpif` as the primary module basename;
the CP PBL/MAIN/NV payload is not a firmware-class file and is deliberately
not copied from `modem.img` by the firmware staging workflow.

## Factory carrier and UECAP directory inputs

The exact `CP3A.260905.009` factory package's `vendor.img` contains the
directory-valued S5300 configuration inputs at
`/firmware/carrierconfig/` and `/firmware/uecapconfig/`. Public Tegu extraction
metadata independently lists those members
([`proprietary-files-vendor.txt`](https://gitlab.com/CalyxOS/device_google_tegu/-/raw/android17/tegu/proprietary-files-vendor.txt)).
This corroborates the source paths; the pinned factory archive and its local
hash remain the authority for the target build.

The native-Linux package stages the directories as
`/lib/firmware/carrierconfig/` and `/lib/firmware/uecapconfig/`. The rootfs
workflow's `/vendor/firmware` symlink then satisfies the reviewed daemon and
udev paths without importing Android userspace. The checked-in firmware
allowlist/stager recursively expands both bundles into deterministic,
destination-aware per-file manifest rows and rejects symlinks, special files,
unsafe paths, and empty bundles. This is still source/staging evidence only:
the modem package has not been built or observed to boot the CP on the exact
device. No `modem.img`, Android service, RIL, HAL, Binder contract, or private
NV data is made part of this path.

## SIT channel and FMT envelope

The authoritative numeric channel constants are pinned from Google's
[`exynos-cpif.h`](https://android.googlesource.com/kernel/google-modules/soc/gs/+/204078758737776959ecc85c527a00fd2cabb62f/include/dt-bindings/soc/google/exynos-cpif.h)
at revision `204078758737776959ecc85c527a00fd2cabb62f`. The target-local
`s5300-channels.toml` keeps both the symbolic DT expression and its numeric
value so source review cannot accidentally substitute legacy SIPC5 tables for
the tegu SIT map:

| DTS device/function | SIT channel value |
| --- | ---: |
| `umts_ipc0` / FMT0 | 245 |
| `umts_ipc1` / FMT1 | 246 |
| `umts_rfs0` | 41 |
| `umts_router` / BT-DUN raw | 21 |
| `umts_dm0` / CPLOG | 81 |
| `umts_loopback` | 82 |
| `umts_rcs0`, `umts_rcs1` | 91, 92 |
| `umts_wfc0`, `umts_wfc1` | 93, 94 |
| `oem_ipc0`–`oem_ipc7` | 129–136 |
| `rmnet0`–`rmnet29` | 181–210 |
| `umts_dummy` / extended PDP30 | 211 |
| `gnss_dump`, `gnss_boot` | 239, 240 |
| `umts_boot0`, dump | 241, 242 |

The open [`modem_prj.h`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/modem_prj.h)
defines the userspace FMT envelope as a packed seven-byte header: little-endian
total length (`u16`), then `msg_seq`, `ack_seq`, `main_cmd`, `sub_cmd`, and
`cmd_type`. The Rust `ferrix-s5300-cpif` crate now bounds and round-trips this
envelope and can wrap an explicitly supplied IPC payload transport. It keeps
the command tuple opaque because the open kernel source does not define the
firmware-specific command vocabulary. This is transport evidence only; it is
not SIM, registration, SMS, APN/PDP, call, IMS, or ModemManager support.

The old SIPC5 numeric tables commonly used by unrelated Samsung devices (for
example FMT 235–244 and RFS 245–254) are not valid for this tegu SIT map. The
`rmnet*` entries are kernel raw-IP interfaces, not a userspace modem-control
protocol, and their presence cannot establish packet-data capability.

## Non-Android Linux research candidate

An independently maintained [`linux-google-tegu`](https://github.com/SteffenDE/linux-google-tegu)
branch (`tegu-v7.2`, observed commit
`88335b765d920cc345cdffb9c1a6d77e3a531ff9`) demonstrates a possible native-Linux
direction: PCIe/doorbell/MSI/shared-memory S5300 transport, SIT WWAN plumbing,
BCM4383 PCIe, and Tegu Bluetooth GPIO/serdev support. Ferrix pins this tree as
the separate radio-first kernel backend; it is not a replacement for the
Google Kleaf comparison baseline, has not been reproduced in this workspace,
and supplies no exact-device evidence.

The candidate requires the board-specific CP payloads in the read-only
current-slot modem image (`modem_a` or `modem_b`), per-device NV/calibration inputs, and a userspace `std_dl`
boot helper before `/dev/umts_boot0` can bring the modem online. Its DTS
declares `firmware-name = "tegu/cp_pbl.bin"`, but the pinned
`s5300_modem` driver is a transport: it does not call the Linux firmware-class
loader. The pinned `exynos-modem` helper instead reads the `BOOT`/PBL section
from the TOC of `/mnt/vendor/modem_img/images/default/modem.bin`, then streams
the MAIN/VSS/APM sections and the read-only EFS/replay inputs. Ferrix must not
invent or stage a separate `cp_pbl.bin` alias until a built kernel proves that
the property is consumed.

The helper builds the 161-byte handover block at runtime from the exact
device-tree identity values, IMEIs, and `/mnt/vendor/persist/modem/cpsha`.
`/lib/firmware/tegu/cp_handover.bin` is only an optional bring-up comparison
blob in that source and is deliberately not required or staged: it can contain
device identifiers. Its userspace also reports a further proprietary SIT/GEMS
command and carrier-protobuf layer for attach, data, SMS, voice, and VoLTE;
those reports are external engineering notes, not Ferrix evidence or a protocol
specification. The Rust crate therefore remains a bounded framing layer and
keeps all modem capabilities unsupported until payload provenance, protocol
corpus, licensing, and exact-target runtime evidence are complete.

The checked-in inventory of the exact official factory archive records separate
`modem.img` and `pbl.img` members, but no `cp_pbl.bin` or `cp_handover.bin`
member. That inventory fact is not permission to reinterpret `pbl.img` as a
Linux firmware-class file: the pinned userspace contract reads `modem.bin` from
the mounted current-slot modem partition, extracts its TOC `BOOT` section, and supplies
that buffer to the transport ioctl. A future kernel or loader that consumes
`pbl.img` would need its own source, hash, compatibility, and runtime-consumer
evidence before the firmware policy could map it; no such alias is currently
staged.

### Radio-first kernel ABI audit (2026-09-28)

The pinned Google Kleaf S5300 module and the community userspace packages do
not share a usable device ABI. Google's module registers a `cpif` class and
creates `/dev/umts_ipc*`; it does not register Linux `wwan_port` SIT devices.
The reviewed `exynos-modem` loader and SIT ModemManager plugin instead target
the `s5300_modem` WWAN driver and `/dev/wwan0sit*`. The ModemManager plugin
manager only watches its declared kernel subsystems, so changing a udev path or
the `sit-proxy` arguments cannot bridge this mismatch. The loader's CP reset
and status ioctls also have different argument encodings between the two
drivers.

For the requested radio-first route, Ferrix therefore uses the
`linux-google-tegu` candidate backend. Its `s5300_modem` driver exports the
WWAN SIT ports consumed by the existing source-pinned loader/proxy/ModemManager
boundary, and its Tegu device tree contains the BCM4383 PCIe Wi-Fi plus UART
Bluetooth plumbing. This is a source selection, not runtime evidence: it still
needs a reproducible kernel build, the read-only modem/EFS/persist payload
contract described above, explicit Wi-Fi/Bluetooth firmware-name mapping, and
exact-device boot and radio tests. The Google Kleaf route remains available for
comparison but must not be described as a working cellular route with the
current userspace packages.

The public Google Android device sources reinforce that boundary rather than
providing a reusable Linux modem ABI. The `gs-common` `modem_svc_sit` change
installs a vendor `/vendor/bin/shared_modem_platform` service and an optional
Binder AIDL `ISharedModemPlatform` compatibility entry; its accompanying
policy grants the service access to `rild`, radio character devices, vendor
firmware, and modem-userdata files ([source change](https://android.googlesource.com/device/google/gs-common/+/8112ee64cc93db0c674bec442b2419e9ea6249a4)).
The published material is policy and compatibility metadata, not the service
implementation or a documented SIT/GEMS command schema. Ferrix therefore must
not copy that Android service, RIL, Binder contract, or SELinux policy into the
native Linux guest; it is evidence that the open CPIF kernel module alone does
not establish telephony or data support.

That candidate also uses different Wi-Fi firmware names (`brcmfmac4383-pcie.*`)
from the pinned Google `bcmdhd` contract (`fw_bcmdhd.bin` and `bcmdhd_*`). The
two sets must not be mixed or silently renamed by the firmware staging policy.

## What the source establishes

The driver source is a kernel CPIF implementation, not a userspace modem
protocol:

1. `ipc_io_device.c` exposes status/opened-state controls and forwards
   unrecognised ioctls to the link driver. Its character-device `open(2)` path
   invokes the link's `init_comm` hook, and its read/write path presents
   payloads after/before the kernel's link framing. Opening an IPC node is
   therefore an active bring-up operation, not a read-only probe.
2. `modem_main.c` creates IPC devices from device-tree I/O-device entries and
   connects them to the modem controller and link device.
3. `modem_ctrl_s5100.c` performs CP boot/reset through the kernel's PCIe/shared
   memory link operations. The source does not define a stable userspace
   boot-and-ring ABI that a Rust daemon can safely infer.
4. `cp_btl.c` exposes a separate boot-log dump-size ioctl; it is diagnostic
   plumbing, not a CP boot or telephony interface.

Consequently, SIPC5/SIT frame codecs and an explicitly supplied IPC payload
transport are useful bounded primitives, but they do not implement FMT/RFS,
SIM, SMS, packet data, voice, IMS, or the CP boot sequence. A standard
ModemManager instance cannot be declared sufficient merely because the
`umts_ipc` nodes are present; an S5300-specific backend or an upstreamed
ModemManager integration still has to map a proven userspace ABI to the
typed Ferrix telephony contract.

## ModemManager compatibility check

The upstream ModemManager device model requires at least one modem control
port; a data-only network interface is explicitly not enough for modem
management ([WWAN device types](https://modemmanager.org/docs/modemmanager/wwan-device-types/)).
The tegu CPIF DTS instead exposes `rmnet*` as raw-IP `IODEV_NET` interfaces
and `umts_ipc*` as vendor-specific `IODEV_IPC` character devices. Neither
surface is a standard AT, QMI, MBIM, QRTR, or ModemManager plugin port. The
presence of an `rmnet` netdev therefore cannot make `mmcli -L` enumerate a
modem, and the existing `ferrix-telephony-mm` adapter must remain an observer
of any independently provided ModemManager object rather than pretending to
own the CPIF protocol.

The open Google header also declares mutable CP-control ioctls such as
`IOCTL_START_CP_BOOTLOADER`, `IOCTL_COMPLETE_NORMAL_BOOTUP`, and
`IOCTL_LOAD_CP_IMAGE`, alongside power/reset and crash operations
([`modem_prj.h`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/modem_prj.h)).
Those ioctl declarations are not a complete modem userspace ABI: the image
pointer/offset structure, secure boot hand-off, PCIe MSI/doorbell sequence,
CP PBL/MAIN/NV contents, and per-device handover/NV provenance still have to
be established for this exact board. An inventory or Rust adapter must never
call them speculatively; even opening `umts_boot0` or `umts_ipc*` can invoke
kernel communication initialization.

## External userspace source adoption policy

The reviewed `pixel-mainline/exynos-modem` and SIT-oriented ModemManager
sources have now passed the source gate for a target-local external build: their
exact repository URLs, immutable commits, archive hashes, licenses, and build
dependencies are recorded in
`pmos/device-google-tegu/s5300-userspace-sources.toml` and the two local
APKBUILD recipes. A source with no explicit redistribution license remains
audit-only: it must not be copied into this tree, vendored into a package, or
distributed in a Ferrix image. The target package must preserve the complete
license notices and corresponding source offer for any GPL kernel or
ModemManager component it eventually builds.

The exact external pins reviewed for this target are:

| Component | Repository / commit | Source archive SHA-256 | License evidence | Ferrix status |
| --- | --- | --- | --- | --- |
| S5300 userspace loader/transport | [`pixel-mainline/exynos-modem`](https://git.deusch.me/pixel-mainline/exynos-modem) @ `634e8eb404c3fcf959dce6a0b0d97b33ba0fc346` ([archive](https://git.deusch.me/pixel-mainline/exynos-modem/archive/634e8eb404c3fcf959dce6a0b0d97b33ba0fc346.tar.gz)) | `324efde8cdef9c3e108cc92d9248556cdc8c0234e77a8a66218b49b978a9445f` | `GPL-2.0-or-later` in repository manifests/headers | Target-local external package recipe; source is not vendored, not a Cargo dependency, and not physical evidence |
| SIT ModemManager integration | [`pixel-mainline/ModemManager`](https://git.deusch.me/pixel-mainline/ModemManager) @ `14a0013a28e5d5eec513c65e75d7549678ecc61e` ([archive](https://git.deusch.me/pixel-mainline/ModemManager/archive/14a0013a28e5d5eec513c65e75d7549678ecc61e.tar.gz)) | `6c11c4de4926d2e928e86fd1c5fbcfeebb515c373ddc8a515ace62d35de391b2` | `GPL-2.0-or-later` in manifests/headers; `COPYING` and `COPYING.LIB` present | Target-local external package/plugin recipe; source is not vendored, not a Ferrix Rust link, and not physical evidence |

These commits and archives remain external package inputs, not checked-in source
or proof that the Pixel 9a CP boots, enumerates in ModemManager, registers,
establishes a bearer, or supports SMS/voice. The rootfs workflow re-fetches only
these hashes, preserves the repository license files/source offer, and records
the package recipes as the complete local build boundary. There are no submodule
or patch inputs in this reviewed package route.

The target-local recipes prefer an external build over a Cargo dependency for
the GPL ModemManager process boundary. The checked-in manifest pins the source
URLs and commits, source archive SHA-256/SHA-512, SPDX expressions, compiler
target, expected module/plugin/helper names, and the read-only partition
contract. The build runs only on the explicit native-Linux target route and
fails closed on a changed source or missing license metadata. Generated
sources, binaries, CP firmware, NV data, handover data, and device identifiers
remain outside Git.

The external component must be a Linux process or target package boundary. A
GPL ModemManager daemon/plugin communicates with Ferrix through its public
D-Bus API; it is not linked into the MIT/Apache Ferrix Rust crates. The existing
`ferrix-telephony-mm` adapter can consume a modem object only after the external
component creates one. It must not be changed into a guessed SIT client, and a
Rust daemon must not reimplement the modem command vocabulary merely to avoid a
license or process boundary.

No Android RIL, HAL, Binder/AIDL service, `rild` dependency, Android SELinux
policy, or `/vendor` service may be imported. A helper that loads CP PBL/MAIN
or per-device NV/handover data needs a separately reviewed target-local ABI,
strict device-node/state-directory permissions, explicit firmware provenance,
and recovery gates. It must not be hidden inside `ferrix-core` or an app.

The intended OpenRC dependency shape is:

```text
dbus + udev
    -> CPIF kernel modules and reviewed target modem-loader (if any)
    -> ModemManager/SIT plugin
    -> ferrix-telephonyd and ferrix-networkd
    -> ferrix-core and the product shell
```

The concrete target package expands that shape to
`ferrix-s5300-mounts -> exynos-modem -> sit-proxy -> modemmanager`, then
`ferrix-telephonyd`/`ferrix-networkd` and the product services. The four source
partitions are mounted read-only from the exact current-slot modem label
(`modem_a` or `modem_b`, selected from the bootloader's
`androidboot.slot_suffix`), plus `efs`, `persist`, and `modem_userdata`, at `/mnt/vendor/modem_img`, `/mnt/vendor/efs`,
`/mnt/vendor/persist`, and `/mnt/vendor/modem_userdata`; the service refuses a
missing or ambiguous slot, foreign or writable mount. UECAP, carrier-config, modem image, and NV inputs
remain external firmware/partition data and are not checked in.

Service ordering alone is not modem readiness. The loader/plugin must expose a
readiness result only after the kernel control surface and ModemManager object
are observed; `ferrix-telephonyd` and the product shell must continue to start
with an absent or failed modem. Until a cold boot, SIM, registration, bearer,
SMS, voice/IMS, and recovery corpus exists on this exact target, the capability
matrix remains `unsupported`.

## Next implementation gate

The next safe step is a native-Linux kernel/rootfs build followed by a
read-only exact-target inventory of CPIF driver binding, IPC nodes, network
interfaces, and modem-controller state. Only after that evidence exists may
we choose between:

- a Rust S5300 userspace backend over a documented, stable kernel ABI; or
- a target-local kernel/ModemManager integration whose ABI and licensing are
  reviewed and pinned.

Issue #1's route comparison, recommended first route, and staged stop/go
gates are in [`s5300-route-decision.md`](s5300-route-decision.md).

Neither route may import Android framework, Google RIL, Android HAL, or
Waydroid into Ferrix. The stock bootloader and controller firmware remain
hardware interfaces only; the operating environment stays native Linux.

The Rust crate now provides `ferrix_s5300_cpif::discover_at` for this
read-only sysfs observation. It requires the exact compatible string,
preserves unknown kernel states, and reports controller `ONLINE` separately
from any telephony capability. The Pixel target adapter does not promote its
capability matrix from this observation alone.
