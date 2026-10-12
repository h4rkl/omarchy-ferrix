# S5300 CPIF source boundary

This file records what the pinned Google S5300 source proves for the exact
`tegu` target. It is source evidence, not physical-device evidence. The
target capability matrix must stay `unsupported` until a native Ferrix Linux
boot on the exact `GXQ96` / `MP1.0` device produces runtime evidence.

## Document map

Since #119 (2026-10-13) this file is the S5300 **source evidence** record. It
holds the CPIF source boundary (below) and, verbatim and one heading level
deeper, [E0: the device-free CPIF ABI comparison](#s5300-e0-device-free-cpif-abi-comparison-issue-1)
(issue #1, formerly `s5300-cpif-e0.md`; its row T1 is the input to the NV
metadata preflight in the [board README](README.md#s5300-nv-metadata-preflight-issue-79)).
Route and design: [`s5300-route-decision.md`](s5300-route-decision.md#document-map).

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
missing or ambiguous slot, foreign or writable mount. Because the factory image
does not restore `efs` or `persist`, each source's type is first read from its
superblock with util-linux `blkid -p` (no mount, no cache write), and only ext4
(`ro,noload`) or f2fs (`ro,norecovery`) is mounted; plain `ro` would still
replay an ext4 journal or roll f2fs forward. A duplicated partition label, an
unknown or ambivalent type, or a mount whose mountinfo superblock options do
not show `ro` plus the recovery-suppression flag is refused. This is
source-level and offline stub-tested only: the four partitions' actual types,
and whether the pinned kernel displays `noload`/`norecovery` in mountinfo,
still need a physical read-only inventory. UECAP, carrier-config, modem image, and NV inputs
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


## S5300 E0: device-free CPIF ABI comparison (issue #1)

> **Moved here on 2026-10-13 (#119)** verbatim from `s5300-cpif-e0.md`, with every
> heading one level deeper. The old path is a stub that maps each old
> section anchor to its new one.

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.

This is the E0 gate from [`s5300-route-decision.md`](s5300-route-decision.md).
It is source and offline-binary evidence only. No phone, USB, ADB, Fastboot,
builder, or CP was touched, and no capability changes. QEMU evidence does not
apply to this board.

### Decision

**E0 result: conditional GO for route A at the ABI level. It does not by
itself justify a CP boot trial.**

- Every ioctl and `read`/`write` that `exynos-modem` uses on the cold-boot
  path has a GPL-defined argument layout in Google CPIF `a8976934`. The
  encodings match exactly. The factory CP3A `cpif.ko` uses the same numeric
  encodings (checked by disassembly).
- What the GPL source does not define is the std_dl download protocol, the
  meaning of the handover fields, and the modem.bin TOC format. That is
  firmware behaviour, reverse-engineered from the proprietary `cbd`. It is
  identical on route A and route B, because both kernels only carry those
  bytes. So it is not grounds to switch to B.
- The unmodified `exynos-modem` must **not** be run against Google CPIF.
  Three bounded userspace adapters are needed first (rows R1, R2, O1 below).
  Without them, recovery and crash-dump paths fail with `-EINVAL`. The GEMS
  channel would also close and reopen every 100 ms, and Google CPIF drops CP
  frames for a node while it is closed (`rx_demux`). The external notes say
  lost GEMS requests crash the CP.
- A CP boot trial still needs things outside E0: the adapter patch and its
  review, E1, the S0 owner review (#34), and the runtime unknowns below
  (U4 skew, NV file sizes).

**Smallest next read-only step:** run E1 as written. In the same read-only
pass, add the cpif probe `dmesg` lines and record whether `nv_normal.bin` and
`nv_protected.bin` are at least the TOC size (0x80000). Record presence and
size only, never content, and only through the reviewed read-only loop mount
once S0 allows it. After that, the adapter patch (R1, R2, O1 plus CLI paths)
is the only code change route A needs before S2.

### Pinned inputs (all hash- or revision-verified, under ignored `out/`)

| Input | Pin | Verification |
| --- | --- | --- |
| Google CPIF `radio/samsung/s5300` | `a8976934fe7f396627f9900958eb1457a74ebcca` (head of `android-gs-tegu-6.1-android16`, committed 2025-06-07) | git object ID |
| Board DTS `devices/google/zuma` | `42395e8adcb880aa336f0da6bb2a19ed53929c40`; tegu include chain `zumapro-tegu-common.dtsi` → `zumapro-board-cp-s5300-sit.dtsi` → `zuma-cp-s5300-sit.dtsi` (tegu `6276884d`, zumapro `8831630e`) | git object IDs |
| Channel constants `soc/gs` | `204078758737776959ecc85c527a00fd2cabb62f` | git object ID |
| `exynos-modem` | `634e8eb404c3fcf959dce6a0b0d97b33ba0fc346` archive | SHA-256 `324efde8…a9445f` matches `s5300-userspace-sources.toml` |
| Radio-first `linux-google-tegu` | `88335b765d920cc345cdffb9c1a6d77e3a531ff9` archive | SHA-256 `efb166b1…3705af` and 266688197 bytes match `kernel-radio-route.toml` |
| Factory `CP3A.260905.009` | `tegu-cp3a.260905.009-factory-0c2f9053.zip` | SHA-256 `0c2f9053…68d1c0d7` and 4227813715 bytes match `recovery.pins` |

Short references used below:
G = [Google CPIF `a8976934`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/),
D = [`zuma-cp-s5300-sit.dtsi` @ `42395e8a`](https://android.googlesource.com/kernel/devices/google/zuma/+/42395e8adcb880aa336f0da6bb2a19ed53929c40/dts/zuma-cp-s5300-sit.dtsi),
X = [`exynos-modem` `634e8eb4`](https://git.deusch.me/pixel-mainline/exynos-modem/src/commit/634e8eb404c3fcf959dce6a0b0d97b33ba0fc346/crates/) (paths below `crates/`),
R = [`s5300_modem.c` @ `88335b76`](https://github.com/SteffenDE/linux-google-tegu/blob/88335b765d920cc345cdffb9c1a6d77e3a531ff9/drivers/net/wwan/s5300_modem.c),
F = factory `vendor_kernel_boot.img` → `lib/modules/cpif.ko`
(SHA-256 `cac472fcf8ee57ac67da1b44f2ccaf55d078dea6a99cb757372615c64a4c0aa5`).

Evidence levels: **source** = handler read in pinned source; **binary** =
disassembly or strings of the hash-verified factory module; **fw-meta** =
parsed from the factory `modem.bin` TOC; **external** = claim in external
code comments, not verified; **runtime** = cannot be settled offline.

### Operation map

`exynos-modem` passes every boot-node ioctl on `/dev/umts_boot0`. Google
dispatches them in `bootdump_ioctl` (G `bootdump_io_device.c:138`). Anything
unmatched falls through to `shmem_ioctl` (G `link_device.c:2735`), which
returns `-EINVAL` for unknown commands.

| # | Operation | `exynos-modem` | Google CPIF (route A) | Radio-first (route B) | Evidence | Decision |
| --- | --- | --- | --- | --- | --- | --- |
| B1 | `IOCTL_HANDOVER_BLOCK_INFO` `_IO('o',0x57)`, pointer to 161 B | X `exynos-modem/src/boot/common.rs:41,227`; layout `boot/s5300.rs:141-162` | `update_handover_block_info` copies `sizeof(t_handover_block_info)` = 161 (packed, G `modem_prj.h:180`) to cmsg `0x82c` (G `link_device.c:1878-1898`; D L102 `DRAM_V1 2092`) | `s5300_handover_block` → ipc+`0x82c` (R L180, L2591) | source; binary (`0x6f57` in the F jump-table range) | **GO as-is.** Offsets 0/4/8/12/16/20/24/28-36/40/44/48-60/64/80/96 match the Google struct exactly. Field *values* (chosen/plat mapping, `cpsha`) are firmware-defined and RE-derived on both routes: **opaque, route-neutral** |
| B2 | `IOCTL_POWER_ON` `_IO('o',0x19)` | X `boot/common.rs:32,236` | `power_on_cp`: STATE_RESET→OFFLINE, clears ap2cp/cp2ap msg words, **full GPIO power off/on** (G `modem_ctrl_s5100.c:992-1037`, L1030) | publishes control words only; reset is a separate ioctl (R L2378) | source; binary | **GO as-is.** Stronger than R (it cold-cycles the CP), which is what `cbd` expects. It does not clear the handover area, so B1-before-B2 ordering is safe |
| B3 | `IOCTL_LOAD_CP_IMAGE` `_IOW('o',0x40, 28 B)` | `CpImage{u64 binary,u32 size,m_offset,b_offset,mode,len}` packed, `m_offset = TOC BOOT.load`, `mode = 0` (X `boot/common.rs:39,47-55,242-257`) | `struct cp_image` same 28 B packed (G `modem_prj.h:82-90`). PCIe path: `boot_img_offset = round_up(0x3000, 64K) = 0x10000`, copies `len` to `base+0x10000+m_offset` (G `link_device.c:2042-2116`) | same layout; dst `0x10000+m_offset`, warns if `m_offset≠0` (R L511-518, L2408-2452) | source; binary (`0x401c6f40` compared in F `bootdump_ioctl`); fw-meta | **GO as-is.** CP3A TOC: `BOOT off=0x410 load=0x0 size=0x16800`, so `m_offset=0` lands where `set_cp_rom_boot_img` points the ROM (G `modem_ctrl_s5100.c:1274-1305`). Size is within `0x20000` (X `boot/s5300.rs:294`) |
| B4 | `IOCTL_START_CP_BOOTLOADER` `_IOW('o',0x22, 4 B)` | `&i32 mode` (0 normal, 1 dump) (X `boot/common.rs:34,262-272`) | `struct boot_mode{enum cp_boot_mode}` 4 B (G `modem_prj.h:61-75`). NORMAL → `start_normal_boot`: ctrl msgs, `link_start_normal_boot` (BOOT_ALIGNED, DPRAM magic), doorbell, `boot_stage` DONE poll (200×20 ms), PCIe bounce, CP2AP_WAKEUP poll (G `bootdump_io_device.c:250-285`, `modem_ctrl_s5100.c:1335-1412`) | does the same steps in-driver plus LINK_ACK doorbell (R L2454-2589) | source; binary (`0x40046f22`) | **GO as-is.** Same number, same arg, and the handshake is driven by the kernel |
| B5 | std_dl stream: `write()` one frame, `poll`+`read()` 4 B ack | X `boot/common.rs:279-375`; stages 1..7 + `0xa400` (`boot/s5300.rs:333-356`) | `bootdump_write`: one skb, no `ul_buffer_size` split, SIT header `{0xABCD, seq, 0xC000, 12+n, ch 241, ch_seq}`, 8-byte pad (G `bootdump_io_device.c:454-607`, `modem_io_device.c:554-598`), sent to the legacy RAW queue (G `link_device.c:1425-1426`). RX: pad trimmed, 12 B header pulled (G `modem_io_device.c:511-522,360-370`). `poll` is POLLIN in BOOTING (G `bootdump_io_device.c:87-109`) | same header bytes (R L343-350, L2649-2718); blocking read (R L2720) | source | **GO as-is** for the transport (frame bytes match R; bytes 10-11 and pad are uninitialised in G but zero in R; the CP is assumed to ignore them, runtime). Std_dl opcodes and CRC frame are **opaque firmware protocol**, not defined in G or R |
| B6 | `IOCTL_COMPLETE_NORMAL_BOOTUP` `_IO('o',0x23)` | X `boot/common.rs:35,274` | waits `init_cmpl` 15 s (G `modem_ctrl.h:10`, `modem_ctrl_s5100.c:1414-1481`). PHONE_START handler sends INIT_END without userspace for SIT (`rild_ready` → true, G `link_device.c:276-285,526-635`) | 15 s wait (R L491, L2603) | source; binary | **GO as-is** |
| B7 | `IOCTL_GET_CP_STATUS` `_IO('o',0x27)`, return value | expects 0 OFFLINE, 4 ONLINE (X `boot/common.rs:124-131`) | returns `enum modem_state` (OFFLINE=0, BOOTING=3, ONLINE=4; RESET maps to OFFLINE) (G `modem_prj.h:222`, `bootdump_io_device.c:296-318`) | same values (R L494-496) | source; binary | **GO as-is** |
| R1 | CP reset (`ResetMode` Warm 0 / Dump 1 / Cold 2) | `_IO('o',0x21)` = `0x6f21`, **integer** arg (X `boot/common.rs:33,141-155`). Used on startup when state ≠ OFFLINE, on every failed-boot retry (Cold), and after a crash (Warm) (`boot/s5300.rs:387-418`) | `IOCTL_POWER_RESET` `_IOW('o',0x21, boot_mode)` = `0x40046f21`, **pointer** arg. NORMAL → full off/on, DUMP → wreset + DUMP_NOTI high, SILENT → wreset + DUMP_NOTI low (G `modem_prj.h:74`, `bootdump_io_device.c:167-217`, `modem_ctrl_s5100.c:1099-1181`) | `0x6f21` integer (R L525, L2781-2816) | source; binary (F compares `0x40046f21`) | **Bounded adapter.** `0x6f21` hits `shmem_ioctl` → `-EINVAL`. The first cold boot is not affected (no reset when OFFLINE), but any retry loops forever with no CP action. Map Cold→NORMAL(0), Dump→DUMP(1), Warm→SILENT(3) via a pointer, or drop the reset on CPIF, since B2 already cold-cycles |
| R2 | `IOCTL_GET_SRINFO` crash record | `_IOWR('o',0x45,16)`, `{u64 buf,u32 len,u32 rsvd}` (X `boot/common.rs:40,158-178`) | `_IO('o',0x45)`, `struct shmem_srinfo{u32 size; char buf[]}`: data copied **inline** after `size` (G `modem_prj.h:100`, `link_device.c:2729-2759`) | `_IOWR`, `{u64,u32,u32}` (R L543-551, L1157) | source; binary (F `shmem_ioctl` accepts only `cmd-0x6f45 ≤ 3`) | **Bounded adapter or disable.** Diagnostic only. Disable the dump-boot path (`boot/s5300.rs:268-275,311-315`) for the first trial |
| P1 | Device paths | defaults `/dev/umts_boot0`, `/dev/umts_rfs0`, `/dev/umts_oem1` (X `exynos-modem/src/main.rs:37-40`); sit-proxy `/dev/wwan0sit0/1` (X `sit-proxy/src/main.rs:42`) | DT creates `umts_boot0` (ch 241), `umts_rfs0` (41), `umts_ipc0/1` (245/246), `oem_ipc0..7` (129..136), names via `"%s%d"` (D L334-474; G `modem_main.c:541-543`) | `umts_oem1` = ch `0x82` (R L253, L3297) | source | **GO with CLI args only:** `--oem-gems-dev /dev/oem_ipc1` (ch 130 = R's `umts_oem1`) and `sit-proxy --dev /dev/umts_ipc0 --dev /dev/umts_ipc1`. The init script currently refuses the Kleaf backend (`exynos-modem.initd`), which is route A integration work |
| O1 | OEM/GEMS read loop | blocking `read()` with no `poll`; `Ok(0)` is treated as EOF → close, sleep 1 s, reopen (X `exynos-modem/src/oem.rs:31-62`) | `ipc_read` waits 100 ms on an empty queue and then **returns 0** (G `ipc_io_device.c:428-446`); release purges the rxq (L57-70); `rx_demux` drops frames for an unopened iod (G `modem_io_device.c:482-486`) | blocks until data (R L2720-2743) | source | **Bounded adapter required.** Poll for POLLIN before reading, or treat 0 as "no data" on the CPIF backend. As-is, GEMS requests arriving during the 1 s gap are dropped. The external notes say that leads to PAL_QUEUE_FULL crashes |
| O2 | OEM framing | one `write()` per message ≤ 256×2036 B; the kernel reassembles on RX (X `oem.rs:23-26,67-77`) | IPC_FMT: TX split into ≤2 KiB SIT multi-frames (G `ipc_io_device.c:270-399`); RX reassembles via `gather_multi_frame_sit` for `is_oem_ch` (G `modem_io_device.c:249-315,497-499`) | same idea (R L3297-3304) | source | **GO as-is** (G is the stock reference framing) |
| F1 | FMT/SIT control on `umts_ipc0/1` | sit-proxy reads whole or fragmented frames, accumulates and splits on the 8-byte SIT header length (X `sit-proxy/src/port.rs:47,566-577`; `samsung-sit/src/lib.rs:74,355-363`); TX ≤ 2036 B, retries EBUSY/EAGAIN (`port.rs:51,500-521`) | strips the 12 B SIT link header and reassembles multi-frame before queueing one skb (G `modem_io_device.c:317-358`). TX: single frame when `n+12 ≤ 2048` (G `ipc_io_device.c:327-328`); `-EPERM` before ONLINE, `-EAGAIN` before INIT_END (L264-307). POLLHUP on crash/reset states (L83-124) | strips the header; delivers fragments separately (`port.rs:20-25`) | source | **GO as-is for the transport** with `--dev` paths. sit-proxy's length-based reassembly also accepts G's reassembled messages, and POLLHUP leads to a clean reopen. Byte equivalence with `wwan0sit*` on device is **runtime** (U3). The ModemManager plugin's `wwan`-only subsystem watch is unchanged (S3, not E0) |
| F2 | FMT header the kernel imposes | — | `struct sipc_fmt_hdr` (7 B, G `modem_prj.h:265`) is used only on the **SIPC** multi-frame path (G `modem_io_device.c:220`). With `PROTOCOL_SIT` (D L37) the kernel never parses FMT payloads | — | source | **Finding:** the 7-byte envelope in `crates/ferrix-s5300-cpif` (`fmt.rs`) is not the SIT payload format. SIT userspace uses the 8-byte header defined only by the external `samsung-sit`. This is not changed here; it should be noted before any Ferrix code relies on it |
| S1 | RFS | `CHUNK=2012`, frames ≤ `0x14+2012` B, poll then read, read buffer 4096 (X `exynos-modem/src/rfs.rs:30-31,131-184,411-429`) | `umts_rfs0` is IPC_RAW: TX single frame because `2032+12 ≤ 2048`; RX header stripped with **no** multi-frame reassembly (G `modem_io_device.c:360-370,502-503`). Writes refused before ONLINE (G `ipc_io_device.c:264`) | 4 KiB single frame, also refused before ONLINE (R L3279-3288, L3412-3415) | source | **GO as-is** while the CP's RFS frames stay ≤ 2036 B (the daemon requests ≤ 2012 B chunks). CP-originated RFS frames > 2 KiB would arrive as fragments on G: **runtime** |
| S2 | RFS/daemon writes | NV writes go to `/tmp/rfs-nv_*.bin`; crash JSON to `/tmp`; reads use `File::open` only on `/mnt/vendor/{modem_img,efs,persist,modem_userdata}` (X `rfs.rs:45-61,293-308`; `boot/s5300.rs:27-56,213,257`) | — | — | source | **GO for the recovery invariant.** No write path to calibration partitions. Writes go to `/tmp`, not `/var/lib/exynos-modem` as the route doc assumes. CP NV changes are not persisted (external claim: the CP rebuilds NV each boot) |
| T1 | Firmware TOC and inputs | TOC 32×32 B, sections TOC/MAIN/VSS/APM from modem.bin, NV_NORM/NV_PROT from efs, REPLAY padded from `modem_userdata` (X `boot/s5300.rs:42-56,284-350`) | the kernel never parses modem.bin | same (transport only) | fw-meta | **GO offline for CP3A image metadata:** INFO `g5300t-260604-260807-B-16036236` passes the s5300 gate; MAIN/VSS/APM present; NV/REPLAY TOC size 0x80000. Whether the efs files are ≥ 0x80000 is **runtime** (device data). TOC/CRC semantics are **opaque firmware** |
| K1 | Opening nodes | — | for SIT, `shmem_init_comm` returns before doing any work (G `link_device.c:1964-1978`); open/close only adjust the refcount and purge rx queues | `open` is a no-op (R L2634) | source | **Note:** in this revision, opening is not link bring-up. The never-open rule for read-only probes stays in place (skew, U4) |

### Platform-kernel and firmware skew (U4)

| Item | Finding | Evidence |
| --- | --- | --- |
| Factory module identity | `vermagic=6.1.162-android14-11-g622c1358585b-ab15912884`, `scmversion=gfd5221767c57`, `depends=logbuffer,s5910,google_modemctl,shm_ipc,ect_parser,cmupmucal,exynos-pcie-iommu,pcie-exynos-gs,exynos_pm_qos,bts` | binary (`.modinfo`) |
| Factory source revision | `fd5221767c57` is in none of the 557 published commits of the S5300 repo. Pinned `a8976934` is the published tegu head, about 14 months older. The CP3A driver source is unpublished | binary + public repo history |
| Ioctl ABI | F `bootdump_ioctl` compares `0x40046f21`, `0x40046f22`, `0x401c6f40`, `0x40106f53`, `0xc0286f52`, `0x82046f55`, `0x80146f56`, `0x80046f59`, and jump-tables `_IO` `0x6f19`–`0x6f60`. `shmem_ioctl` handles `0x6f45`–`0x6f48`. Same version string `CPIF-20220408R1` and handler log strings as G | binary |
| Function set | all 376 function symbols in F map to identifiers defined in pinned G source (including macro-generated ones). Function bodies can still differ | binary vs source |
| Board DT | factory MP1.0 overlay (`dtbo.img` entry `id=0x60b06`, "MP on ZUMA PRO") `cpif` node matches the pinned include chain (D plus `zumapro-board-cp-s5300-sit.dtsi`) on every compared value: link_attrs `0x37c8`, legacy queue offsets/sizes, srinfo, capabilities 0, ctrl-msg offsets including handover `0x82c`, and all 17 iodevs (name, channel, format, attrs, count). The only extra property is `offset_boot_indicator = 0x90` | binary DT vs source |
| Boot indicator | not read by G or present in any pinned DTS. F reads it in two `bootdump_ioctl` paths only as a `readl()` of the word at that DT offset (from a driver-held base) plus a log line ("cp boot indicator"). No AP-side write was found | binary (disassembly) |
| Residual | whether G `a8976934`, built for the 6.1.124 dev GKI, interoperates at runtime with CP3A firmware (shared-memory behaviour outside these ioctls, PCIe PM, pktproc) | **runtime** (S2) |

### What E0 cannot conclude offline

1. CP3A firmware acceptance of the std_dl stream and handover produced by
   `exynos-modem` on the G driver. This is runtime, and only physical S2
   can show it.
2. NV file sizes and `cpsha`/chosen-plat presence on this device. This is
   device data, gated by S0/E1.
3. U4 runtime skew beyond the ioctl/DT surface.
4. Whether `umts_ipc0/1` payloads are byte-identical to `wwan0sit0/1` (U3).
   Both kernels strip the same 12-byte header, but equivalence needs S3.
5. Whether the dev cpif build actually loads and binds (E1).

### Reproduction (device-free)

All downloads go to ignored `out/e0-src/` and `out/e0-factory/`. Parsers lived
outside those directories and read the archives with `python3 -I`. The steps:
shallow-fetch each pinned git revision; fetch and `shasum -a 256` both external
archives and the factory zip against their pins; extract `vendor_kernel_boot`
(vendor boot v4, LZ4 cpio), `dtbo.img`, and `modem.img` (a tar whose raw bytes
are also a valid ext4; `images/default` is a symlink to the versioned
directory); read `.modinfo`; disassemble with `objdump -d` (Apple LLVM). No
firmware bytes, identifiers, or extracted images are committed.
