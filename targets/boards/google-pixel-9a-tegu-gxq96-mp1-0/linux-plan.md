# Native Linux enablement plan

This plan covers only the exact Google Pixel 9a target identified as
`tegu` / `GXQ96` / `MP1.0`. It does not generalize to another Pixel 9a
revision or another Tensor device.

## Product boundary

Ferrix on this target is intended to be a native Linux system. Its root
filesystem is built from the repository's postmarketOS substrate and, once the
host-side inputs and physical gates are complete, will start the Rust Ferrix
services, compositor, and applications. It does not use the Android framework,
Android system applications, Android HALs, Google RIL, or Waydroid as the
operating environment.

The existing boot ROM and unlocked Google bootloader still define the image
container and partition interfaces. Device firmware remains necessary for
hardware controllers. Those interfaces are isolated in this board package and
must not enter product crates or `ferrix-hardware-api`.

## Checked-in package and evidence status

The checked-in target-local package is an implementation scaffold with
source-pinned native-Linux package inputs for this exact `tegu` / `GXQ96` /
`MP1.0` device. It includes:

- `pmos/device-google-tegu/`, a native-Linux postmarketOS device package with
  target metadata, OpenRC service wiring, conservative early-boot inputs, and
  split-boot metadata. It also carries the immutable external
  `exynos-modem`/`sit-proxy` and SIT ModemManager recipes plus their source
  manifest. Its `deviceinfo` sets `deviceinfo_flash_method="none"`; the
  package is not a bootable image and does not authorize pmbootstrap flashing.
- `adapter/`, the `ferrix-target-pixel9a` Rust composition seam selected by
  session/bootstrap code. It is the only target-local adapter layer; product
  crates receive target-neutral `ferrix-hardware-api` trait objects.
- `tools/pixel-9a/` host-only kernel, firmware, rootfs, split-boot, and
  artifact-manifest workflows. Their generated output is ignored and is not
  evidence merely because it is hash-recorded.
- `crates/ferrix-s5300-cpif/`, a bounded SIPC5/SIT link-frame slice described
  below. The exact tegu DT selects SIT; this crate is not a modem backend and
  is not wired into the Pixel adapter.

The target's data-safety policy is explicit and checked in: this is an owner-
declared new development device with no personal data, represented by
`[gates] backup = "not-required-dev-device"` and
`data_policy = "dev-device-no-user-data"` in `manifest.toml`. This is not a
backup verification claim. Each mutating workflow requires the session-only
`FERRIX_PIXEL9A_NO_USER_DATA=I-understand-this-is-a-new-development-device-with-no-personal-data`
opt-in plus its normal typed per-run confirmation; the stock recovery gate is
still independently required.

The target manifest remains explicit about the current state:
`kernel_build_status = "not-built"` for the comparison Google Kleaf route,
`radio_kernel_build_status = "built-not-booted"`,
`rootfs_build_status = "built-substrate-only"`, and
`boot_status = "not-attempted"`. The package's boot layout is still
`substrate-only-not-bootable`. The native build is a source/package handoff,
not physical evidence: no physical boot, framebuffer/input result, live Linux
service result, firmware runtime result, or target capability evidence has
been captured. The earlier ADB/Fastboot identity, recovery, and
bootloader-unlock observations are admission and recovery history only, not
Linux evidence. The target-local `FERRIX_EVIDENCE=physical:<target-id>` value
is a scope label required by the host profile, not proof of a physical run.

The latest read-only physical gates are recorded without promoting them to
Linux evidence: the exact Fastboot device passed the pinned factory/OTA
preflight on 2026-09-29T21:11:49Z, and the ordinary-bootloader Fastboot
partition inventory completed at 2026-09-29T21:12:06Z. The preflight still
reports `image_compatibility=partial` and
`recovery_procedure=documented-not-tested`; the Android-side inventory remains
partial. The owner-entered Fastbootd diagnostic on 2026-09-30 established
that this mode cannot expose six static partition pairs and reports zero
allocation for both inactive DLKM partitions. Another mode transition alone
does not complete the ten-partition capacity handoff.

### Stock-preserving installation methodology

The owner's 2026-09-30 direction is to preserve the stock recovery route,
bootloader/radio firmware, and active stock slot while preparing an explicit
inactive-slot Ferrix installation. This is not authorization to repartition,
overwrite shared storage, select a slot, or flash without a per-run review.

- Keep the existing bootloader and radio firmware. Fastboot and Fastbootd are
  transports, not replacement firmware. The Ferrix route never writes
  bootloader/radio partitions or relocks the device.
- Preserve every active-slot partition. Inactive boot-related partitions
  still need Ferrix-specific source-built images; leaving all boot images
  untouched cannot deliver the current native Linux route.
- Treat `super`, logical-partition allocation, snapshots, `metadata`, and
  `userdata` as shared-storage concerns, not independent inactive-slot
  storage. The currently unset rootfs mapping stays unset. A `userdata`
  rootfs would overwrite Android's shared data filesystem and does not satisfy
  a promise of an immediately bootable stock fallback.
- Keep tested recovery and exact image compatibility as independent gates.
  The owner reports a stock restore, but the recorded preflight alone does
  not establish its completion. The pinned recovery build `CP3A.260905.009`
  differs from the observed stock build `BD4A.240925.111`; restoring the
  pinned archive must not be described as restoring that exact old build.
  The owner explicitly accepted the pinned newer `CP3A.260905.009` build as
  the recovery destination on 2026-09-30. This resolves the destination
  choice only, not the completion-evidence or per-run confirmation gates.
  Do not upgrade bootloader firmware or claim downgrade compatibility to
  resolve this discrepancy.

The current radio-first workflow enforces an **existing-allocation-only**
policy. The reason is visible in [AOSP's Fastboot implementation](https://android.googlesource.com/platform/system/core/+/3f439a7a887736cb3cf26d6523defc19ff61ec7b/fastboot/fastboot.cpp):
an ordinary logical-partition flash can invoke a resize to the image's
expanded size. Merely omitting an explicit resize command is insufficient.
This source observation is not proof of a particular host binary's behavior.

For an explicitly selected inactive slot, the read-only preflight now checks
both slots' live capacities against all ten reviewed capacities. Missing,
zero, ambiguous, unaligned, or changed values fail closed. Logical images
must equal the reviewed allocation, not merely fit within it. Temporary boot
and observation remain separate, non-persistent operations.

After per-run confirmation, the kernel flash workflow first rechecks identity
and capacities, visits Fastbootd, verifies exact identity, logical partition
classification, existing sizes, and `snapshot-update-status=none`, then
returns to the bootloader and rechecks before writing any image. The normal
inactive boot writes and later Fastbootd DLKM writes follow; each write has
another live capacity check. A failure during the initial round trip leaves
images untouched, though the phone may remain in Fastbootd. Later failures
can still leave a partially flashed inactive slot: no atomicity or automatic
rollback is claimed. The shared mutation SKU gate is unchanged. The
radio-first route now has a narrower Fastbootd-only correlation described
below; the placeholder remains forbidden in bootloader mode.

Next acquisition/layout review, before enabling any allocation change:

1. Collect fresh static A/B capacities in owner-entered bootloader Fastboot
   with the existing read-only inventory workflow. Any future cross-mode
   handoff must bind exact identity, current slot, capture times, and hashes;
   old and new transcripts must not be silently merged.
2. Obtain exact live logical metadata, group limits, free extents, and
   snapshot state through a reviewed read-only workflow. Zero inactive DLKM
   allocation is an observation, not proof of spare space or permission to
   copy the active size. No allocation/resize workflow is approved here.
3. Choose a rootfs storage route consistent with the desired stock fallback,
   establish tested recovery to the accepted pinned build, then review the complete
   image and storage plan. Only a new, specific mutation preflight and owner
   confirmation can authorize physical installation.

### Remaining-contract review (implementation approved; physical runs separate)

The owner approved the static-only and first-recovery-test tooling changes
on 2026-09-30. This does not authorize a device write. The source-only
`native-static-artifact-contract.toml` now declares the independent eight
partition, built-in-driver and AVB requirements. Its composer and flash
adapter remain to be implemented; the existing ten-partition route cannot
consume it.

Read-only inspection of deba at source commit
`26fc9f9beb1d1faf041a8d4df71fe6142353c133` found empty selected initfs/vendor/
system module policies and empty generated DLKM `modules.dep`/`modules.load`
files. The built kernel configuration has SHA-256
`a644e47fac409e47d2c132dfb1dc02839f645812ce0d10a7bc2e93634c766b2d`.
It sets Exynos UFS, ext4, devtmpfs, Exynos DWC3, the display driver,
BCM4383 Wi-Fi, Bluetooth UART, and S5300 WWAN to built-in (`=y`). The
source-owned initramfs mounts `PARTLABEL=userdata` directly, with no DLKM
mount. This is source/build evidence, not an observed boot or proof that a
revised AVB chain will be accepted by the Pixel bootloader.

Proposed resolution instead of reallocating shared `super`:

- Introduce a separately named native static-boot artifact contract. Keep
  the existing ten-partition contract intact; do not add a skip-DLKM switch
  or feed an eight-partition capacity file to the existing composer.
- Bind the final built kernel configuration to its kernel manifest, verify
  all required early drivers are built in, and reject nonempty selected
  module policies or loadable payloads in the proposed route. Revalidate
  boot inputs, rootfs, source revision, and all hashes on the native builder.
- Collect fresh static capacities in ordinary bootloader Fastboot. Compose
  only the eight inactive boot-related images with fresh, source-derived AVB
  metadata referencing the intended static images, not stock DLKM contents.
  Verify the new AVB chain independently. Do not write, resize, delete, or
  reformat `super` or either slot's logical partitions.
- Give that route its own strict manifest acceptance and flash adapter;
  retain identity, inactive-slot, live capacity, image hash, data policy,
  tested recovery, temporary-boot observation, and per-run confirmation gates.
  The existing ten-partition adapter must reject the new contract.
- Keep rootfs provisioning separately gated. The current native rootfs needs
  `userdata`; writing it is a shared-data wipe, not a dual-boot installation.
  Recovering stock would require the accepted pinned factory restore. Exact
  live mapping and size evidence and explicit per-run approval remain required.

The other proposal is an explicit first-stock-recovery-test workflow. Normal
restore and Ferrix gates must not silently admit an untested recovery route.
A separately authorized first test would require exact live identity, both
pinned official archives, their own board/image requirements, the reviewed
vendor sequence, the existing data policy, and an additional typed
acknowledgement that this is an unproven recovery test. It would execute the
stock vendor restore sequence only; never Ferrix images. Successful command
completion plus fresh same-device Android discovery of the accepted stock
build is required before promoting recovery evidence. The target's general
Ferrix `image_compatibility=partial` is not evidence about the official stock
archive. The explicit `--preflight-first-test` / `--confirm-first-test` modes
are now implemented and stub-tested. The live read-only preflight passed on
2026-09-30; no restore execution or gate promotion is implied. Normal restore
still requires its existing tested-recovery gate. First-test mode always
uses the full wipe sequence and adds a separate risk acknowledgement before
any mutation.

The separate `pixel-9a-partition-capacity-static` read-only workflow now
requires ordinary bootloader mode, verifies physical classification and both
slots' independent equal positive sizes for the eight allow-listed targets,
and emits schema 2 / `partition_contract=static-boot-only` to a separate
ignored output. The live capture at `2026-09-30T00:17:38Z` passed; it neither
queries DLKM sizes nor upgrades the broader partial inventory. Fresh evidence
must be collected after any restore that changes the layout.

The kernel's embedded IKCONFIG was extracted read-only on deba using the
pinned upstream helper and matched the recorded `.config` digest exactly.
The new contract binds the extractor digest and requires future composition
to repeat this extraction from the verified `Image`, not trust an unrelated
configuration file. No generated image or extracted config enters Git.

The current radio-first Fastbootd SKU correction is independent of those
proposals: `MSM USF` is recognized only after this process has established a
complete ordinary-bootloader identity, then rechecked the same serial,
product, revision, slot, slot count, unlock/secure state, and userspace mode.
Complete matching-serial ADB discovery must also prove `GXQ96`. Missing proof,
bootloader-mode placeholders, and other variants are refused. The observed
placeholder and evidence source are logged separately. Shared restoration,
unlock, and activation helpers remain unchanged.

## Kernel decision and pinned source baseline

Mainline Linux and postmarketOS do not currently provide a Pixel 9a target.
The initial board kernel must therefore use Google's published downstream
Pixel 9a Linux 6.1 stack. Google's Pixel-kernel documentation maps `tegu` to
`android-gs-tegu-6.1-android16` with the `android14-6.1` GKI base:

<https://source.android.com/docs/setup/build/building-pixel-kernels>

The source baseline inspected on 2026-09-28 is pinned below. A build workflow
must resolve these exact objects and fail if any object differs.

The pinned `dts/zumapro-tegu-mp.dts` root overlay identifies this board as
model `ZUMA PRO TEGU MP on ZUMA PRO` with compatible strings
`google,ZUMA PRO TEGU` and `google,ZUMA PRO`. The Rust target adapter and the
post-boot radio workflow admit those exact strings; they do not use the
lowercase marketing codename as a hardware identity check.

| Component | Git object |
| --- | --- |
| Kernel manifest | `893fbfd2192bc1c4a921f5daf23ea8543c09670d` |
| Kernel superproject | `ef85063e6f3c68fbd580eb7e41809b8b64f935ed` |
| GKI common | `a4662a84a15f38b99cf597354edccf75988852de` |
| `devices/google/tegu` | `6276884d9940694b25a6934671918ec8ac412ee8` |
| `devices/google/zuma` | `42395e8adcb880aa336f0da6bb2a19ed53929c40` |
| `devices/google/zumapro` | `8831630eee2ad095b85ed452944e6353940966a5` |
| Google SoC modules | `204078758737776959ecc85c527a00fd2cabb62f` |
| BCM4383 Wi-Fi | `3526a8cb8e13b7a72fca642c7b6411cd69e70d87` |
| Broadcom Bluetooth | `d7bd4dd23cc52fedd2e36c6e45c12f0c2a595ae0` |
| Samsung S5300 CPIF | `a8976934fe7f396627f9900958eb1457a74ebcca` |
| Display common | `e34756e6205a16083c5c7c4e5cde2c899674a120` |
| Tegu panel drivers | `e3a801fb038fdd1bbaec36d387f27bd99f10757a` |
| Amplifiers | `33976d763bc0ace11603e7ecb207fc2b6e3f2608` |
| AoC / AoC IPC | `ac69293d11343b47f0b2b758b0df9f936360dbd4` / `1fe9b9cf3a2954a4a0c0f9868aa04c9d88c902b9` |
| BMS | `183509aff980610401869deca9f6ce8836980fe7` |
| Touch common / Synaptics | `340d76ec988f2960427647e9ac4852f072a86c88` / `a5d225cc4034f3e98c6f7f0c0c7e182c254a517f` |

The local verified `CP3A.260905.009` factory package contains a
`6.1.162-android14-11-g2ec90535fa34-ab15810641` stock kernel. That observation
does not make the published Android 16 branch byte-compatible with the stock
Android 17 artifacts. Ferrix must build the kernel, DTBOs, and modules as one
coherent source set; it must not mix stock modules with a differently built
kernel.

The target package preserves Google's exact `tegu_defconfig` fragment in
[`kernel-config.tegu`](kernel-config.tegu). It includes the S5300
`EXYNOS_MODEM_IF`/`SEC_MODEM_S5100`/`SHM_IPC` path, packet processing, PCIe
IOCC/IOMMU, CP thermal support, and the display/touch fragments. The pinned
kernel verification step compares this file byte-for-byte with the checked-out
`private/devices/google/tegu/tegu_defconfig`; a mismatch stops before Kleaf
build. This is a Linux kernel input, not Android userspace.

### External native-Linux kernel candidate (radio-first backend)

Read-only research found an independent native-Linux candidate in
[`SteffenDE/linux-google-tegu`](https://github.com/SteffenDE/linux-google-tegu),
branch `tegu-v7.2`, observed at commit
`88335b765d920cc345cdffb9c1a6d77e3a531ff9`. Its public history includes the
Zumapro PCIe/doorbell/MSI/shared-memory S5300 transport, SIT WWAN plumbing,
BCM4383 PCIe support, and Tegu Bluetooth GPIO/serdev changes. Ferrix keeps the
Google Kleaf tree as a comparison baseline, but the radio-first build backend
pins this candidate separately. It has an independent source archive,
configuration, device tree, and normal Linux `make` build contract; it has not
yet been reproduced here and has no exact-device runtime evidence.

The associated-radio audit pins the candidate's built-in `samsung,kepler`
GNSS path (`CONFIG_SPI`, `CONFIG_GNSS`, and `CONFIG_GNSS_KEPLER`) into the
radio-first route. It deliberately does not claim NFC/ST54 or UWB: the Google
Kleaf source has Android-only comparison modules/DT for NFC, while the
radio-first candidate has no reviewed native NFC binding and no enabled UWB
driver. See [`associated-radio-audit.md`](associated-radio-audit.md) for the
source links and promotion gates.

The route metadata records KEPLER as `samsung,kepler` with a
`drivers/gnss/kepler.c` source path and `raw-device-only` userspace status.
Its preflight rejects Android comparison modules (`st21nfc`, `st54spi`, and
`bcm47765`) in the radio-first initfs. This keeps the source-built GNSS kernel
path available without claiming a location service, while NFC/ST54 remain
unselected until a native Linux binding and secure-element boundary are
reviewed.

That candidate also makes the remaining modem gate explicit. Its DTS declares
`tegu/cp_pbl.bin`, but the pinned transport driver does not request that file
through the kernel firmware class. The paired `exynos-modem` helper extracts the
`BOOT`/PBL section from the exact read-only current-slot (`modem_a` or
`modem_b`) `modem.bin`, streams the
MAIN/VSS/APM sections, and reads the EFS/replay inputs; it constructs handover
bytes at runtime from the device tree, IMEIs, and persist `cpsha`. Ferrix must
not manufacture a `cp_pbl.bin` or `cp_handover.bin` firmware alias, and must
never commit the per-device handover data. The public kernel tree still does
not provide a complete Ferrix-safe attach/data/SMS/voice protocol schema. The
current Rust crate therefore remains transport/framing-only, and the target
capability matrix stays `unsupported`. Adopting this candidate would require an
owner-approved source proposal, license review, exact read-only partition and
payload provenance, and a redacted SIT protocol corpus before changing the
build baseline.

The candidate's BCM4383 path uses `brcmfmac` firmware names
(`brcmfmac4383-pcie.bin`, `.txt`, and `.clm_blob`), while the pinned Google
`bcmdhd` path uses five base request names plus factory `_4383_a3` variants
for revision-aware lookup. These are alternative kernel contracts, not
interchangeable files. The radio-first firmware allowlist explicitly maps the
reviewed A3 factory members to the `brcmfmac` runtime names; the default Google
allowlist preserves the base and suffixed names and does not silently mix the
contracts.

The ABI audit is decisive for cellular integration: the Google Kleaf CPIF
surface (`cpif`/`umts_ipc*`) cannot feed the current `s5300_modem` WWAN SIT
userspace by changing only init scripts or udev rules. The radio-first backend
is therefore the checked-in route that preserves the reviewed
`exynos-modem`/SIT ModemManager package boundary without importing Android RIL,
HAL, Binder, or vendor services. Its native Linux x86_64 workflow has now
produced a hash-verified kernel handoff and rootfs substrate; both remain
unbooted and provide no physical radio evidence.

The radio-first build handoff is explicit and device-free:

```sh
just pixel-9a-kernel-radio-check
FERRIX_PIXEL9A_RADIO_SYNC=I-understand-this-downloads-the-radio-first-kernel \
  just pixel-9a-kernel-radio-sync
just pixel-9a-kernel-radio-verify
FERRIX_PIXEL9A_RADIO_BUILD=I-understand-this-runs-the-radio-first-kernel-build \
  just pixel-9a-kernel-radio-build
```

The radio sync workflow downloads the exact pinned source archive and verifies
its byte count and SHA-256/SHA-512 before using the detached revision. The
archive, checkout, and build output remain in the target cache/output roots;
Kbuild is refused when the archive is missing, altered, or the checkout is
not clean.

The route pins the independent `linux-google-tegu` revision, config/DTS
digests, the IP/WWAN/rmnet data path, Wi-Fi/Bluetooth/serdev symbols, and the
explicit factory firmware-name mapping in `kernel-radio-route.toml` and
`firmware-radio-route.tsv`. The candidate creates `wwan0sit0`/`wwan0sit1`, an
AT WWAN port, and the `rmnet0`-`rmnet29` raw-IP data netdev range only after CP
ONLINE; those are runtime gates, not build claims. The workflow never invokes
ADB/Fastboot and its manifest stays `built-not-booted` / `bootable=false` until
exact-device evidence exists.

After a native build, the radio workflow also records hash-verified
source-built DTB/DTBO inputs, the `modules_install` tree, and a partition-size
status file. These handoffs are consumed by the rootfs and radio boot
validators, but they are not `dtbo`, `vendor_dlkm`, or `system_dlkm` images and
cannot authorize flashing until a split-image builder produces those exact
partitions.

The radio boot preflight now validates those handoffs as source-built inputs:
the DTB/DTBO manifest must contain at least one real `.dtbo` overlay and every
listed source file is rechecked for path safety, size, and SHA-256; the
`modules_install` manifest is likewise rechecked against its materialized
files. A DTB-only or stale/tampered handoff is refused before any boot
container is composed. This still does not create the missing split partition
images or establish boot evidence.

The candidate's radio, Bluetooth, Wi-Fi, UFS, and filesystem paths are built in
by the merged config. `kernel-radio-extra.config` pins device-mapper built in
as well, because the radio-first image has no Android module partition. Its
route-local `modules-initfs-radio` list is intentionally empty until a native
Linux build proves a required `.ko`; the Google Kleaf `modules-initfs` list is
kept separate and is never passed to the WWAN/SIT rootfs.

## Boot composition

The verified factory package declares this target's split boot layout:

- `boot`: GKI kernel;
- `init_boot`: generic ramdisk;
- `vendor_boot`: vendor ramdisk and device tree data;
- `vendor_kernel_boot`: early vendor kernel modules;
- `dtbo`: board overlays, including the `zumapro-tegu-mp` family;
- `vendor_dlkm` and `system_dlkm`: remaining kernel modules;
- `vbmeta`, `vbmeta_vendor`, and `vbmeta_system`: verified-boot metadata.

The first Ferrix artifact set must retain that separation and include a
manifest containing every input revision, output size, and SHA-256. It must
not run Google's `flash-all.sh`, update bootloader/radio firmware, wipe dynamic
partitions, or overwrite the active slot.

The canonical Google comparison workflow remains `tools/pixel-9a/boot.sh`:
`just pixel-9a-boot-check` validates the exact target layout, pinned Kleaf
manifest, native rootfs manifest, Linux initramfs/vendor-ramdisk/bootconfig
inputs, `mkbootimg`, and `avbtool`; the explicitly confirmed
`just pixel-9a-boot-build` route then copies only the hash-verified Google
kernel artifacts, constructs fresh header-v4 `init_boot.img` and
`vendor_boot.img`, creates fresh `vbmeta.img`, `vbmeta_vendor.img`, and
`vbmeta_system.img`, and hands off both `rootfs.tar` and the generated ext4
`rootfs.img`. It deliberately rejects the radio-first rootfs backend so the
two incompatible kernel/userspace ABIs cannot be mixed.

The selected radio-first route separates source-owned boot-input construction
from complete split-image composition. First run:

```sh
just pixel-9a-boot-inputs-test
just pixel-9a-boot-inputs-check
FERRIX_PIXEL9A_BOOT_INPUTS_BUILD=I-understand-this-builds-source-owned-pixel-9a-boot-inputs \
  just pixel-9a-boot-inputs-build
```

The complete split handoff then requires the exact-target ten-partition
capacity file from the owner-controlled Fastbootd read-only gate:

```sh
just pixel-9a-radio-split-test
just pixel-9a-radio-split-check
FERRIX_PIXEL9A_RADIO_SPLIT_BUILD=I-understand-this-composes-the-radio-first-split-artifacts \
  just pixel-9a-radio-split-build
```

`boot-inputs.sh` hashes the source-built rootfs BusyBox, checked-in Ferrix
initramfs/vendor-ramdisk/bootconfig inputs, and source-built module roots.
`radio-split.sh` consumes only that manifest, generates the missing persistent
split artifacts and fresh AVB metadata, and carries relative provenance files
inside the output so the Mac-side verifier does not depend on native-builder
paths. The output remains `composed-not-booted` / `bootable=false`;
temporary-boot command success does not prove a Ferrix Linux boot on the
exact device.

`boot-radio.sh` remains a diagnostic/incomplete handoff and is not a flashable
route.
`just pixel-9a-artifacts-manifest` checks the resulting kernel/rootfs/boot
manifests and the pinned factory archive without contacting a device. It does
not establish that any generated artifact is bootable or that the candidate
rootfs partition mapping is safe to flash.

Before a boot attempt, a checked-in target workflow must prove whether this
bootloader accepts a temporary `fastboot boot` artifact without persistent
partition mutation. If it does not, an inactive-slot plan needs separate owner
approval after backup and tested recovery gates pass. Either route must print
and confirm the exact serial, product, hardware revision, current slot,
partition targets, input hashes, and recovery hashes immediately before use.
Once the radio-first kernel stage has been written to the inactive slot, the
checked-in `just pixel-9a-boot-inactive` workflow is the only route that may
select and reboot that slot. It consumes same-device temporary-boot and
persistent-flash evidence, plus the operator-reviewed kernel capture recorded
by `just pixel-9a-observe-temp-boot`. Both flashing and activation bind that
capture to the temporary-boot transcript and current `boot.img` hash; command
success alone is insufficient. The activation workflow re-hashes the complete split manifest and both
recovery archives, rechecks identity after selection, and reports only
`slot-activated-boot-requested`; it does not claim a Linux boot. Raw
`set_active`/reboot commands remain prohibited.

## Target-local root filesystem and adapter composition

The target image uses postmarketOS for the Linux root filesystem and package
manager. `pmos/device-google-tegu/APKBUILD` is a target-local package input,
not a final image: it declares the native Linux dependencies (D-Bus, udev,
NetworkManager, ModemManager, BlueZ, PipeWire/WirePlumber, UPower, seatd, and
the pmOS base), installs the Ferrix OpenRC entry points, and enables the
production service set. Its `ferrix-backends.env` selects NetworkManager,
ModemManager, BlueZ, WirePlumber, and UPower. Those selections describe the
intended composition; they do not claim that any service has run on this
device.

The target-local Rust adapter is selected only in session/bootstrap and returns
a target-neutral `HardwareBundle`. Its checked-in Linux composition is:

| `ferrix-hardware-api` capability | Target-local implementation input |
| --- | --- |
| Power | `ferrix-power-upower` over UPower's aggregate display device |
| Display | local DRM mode discovery and kernel backlight sysfs access |
| Input | local input-device and IIO-presence discovery |
| Network | `ferrix-network-nm` / NetworkManager, folded into the coarse network contract |
| Bluetooth | `ferrix-bluetooth-bluez` over BlueZ system D-Bus |
| Audio | `ferrix-audio-wireplumber` through WirePlumber's `wpctl` surface |
| Telephony | `ferrix-telephony-mm` / ModemManager, only for interfaces it can expose |
| Lifecycle | Linux `/sys/power/state` suspend integration (OpenRC-safe) |

An internal event hub bridges service-adapter events to the target-neutral
event source. Product applications, `ferrix-core`, and `ferrix-hardware-api`
do not import this target package or gain Google, Pixel, Tensor, Android-image,
partition, or S5300 conditionals. The target adapter deliberately reports
`unsupported` for every capability until exact-target Linux evidence exists.

The device-free rootfs workflow is separate from split-boot composition. Its
firmware input is the canonical manifest emitted by
`pixel-9a-firmware-stage`: it carries the pinned factory archive hash plus
every allow-listed source image/path and Linux runtime destination. Runtime-only
or legacy rows are rejected.

1. `just pixel-9a-rootfs-check` and `just pixel-9a-rootfs-test` check the
   target package and write a `not-built` metadata manifest.
2. `just pixel-9a-rootfs-validate` requires a pinned, clean pmaports checkout,
   the completed radio-first Kbuild dist from
   `just pixel-9a-kernel-radio-build`, the Ferrix product artifact directory,
   and an external provenance-verified firmware directory. The rootfs builder
   copies the target-local S5300 recipes into the clean pmaports staging tree;
   the package sources are fetched and built there, never vendored into this
   repository. The canonical Google Kleaf dist is a separate comparison route
   and is rejected by this selected radio-first rootfs contract.
3. The explicitly confirmed `just pixel-9a-rootfs-build` route runs
   pmbootstrap on native Linux, stages the Ferrix binaries/descriptors,
   `dev/profile.toml`, service configuration, and the allow-listed firmware,
   then writes a deterministic `rootfs.tar`, a raw ext4 `rootfs.img`, and
   `firmware-manifest.tsv` under ignored `out/pixel-9a/rootfs/tegu/`. The
   resulting manifest is `built-substrate-only` and `bootable=false`.

The product handoff is built directly by Cargo on a provisioned native Linux
x86_64 host, using the target's pinned Rust toolchain and target-side Linux
development inputs (`libudev`, `libseat`, DRM, libinput, and the software
renderer). The physical-device playbook rejects macOS, containerized builders,
and cross-build substitutes for product, kernel, firmware, rootfs, boot, and
artifact stages. The Google Kleaf kernel and pmbootstrap rootfs remain separate
privileged stages, each with distinct caches and outputs.

The rootfs workflow never calls ADB, Fastboot, a pmbootstrap flasher, or a
firmware extraction route. QEMU output contributes only cross-compiled product
artifacts and descriptors; its synthetic radio, battery, network, or other
mock state is not copied into this target profile.

## Firmware policy

Firmware and calibration data may be obtained only from an owner-supplied,
size- and SHA-256-verified official Google factory package for the exact `tegu`
build selected by `recovery.pins`. `firmware-policy.toml` requires the exact
factory hash, local extraction, no unlisted paths, and no checked-in firmware
bytes or device identifiers. Firmware, calibration, radio images, keys,
partition dumps, and generated images remain ignored and must not be committed
or redistributed.

The device-free workflow is deliberately two-stage. `just
pixel-9a-firmware-inventory` verifies the pinned factory archive and records an
offline source inventory. The explicitly confirmed `just
pixel-9a-firmware-stage` route accepts a reviewed controller/image/path
allowlist, reads only allow-listed files from `vendor.img` or `vendor_dlkm.img`,
and emits an external `firmware-manifest.tsv` containing source member/path,
runtime path, byte count, and SHA-256. The rootfs workflow accepts only that
manifest and rechecks every file; it never extracts firmware itself or copies
the Android `/vendor` tree wholesale. The stage helper rejects `modem.img` as
a raw radio payload. The checked-in allowlist now includes the directory
inputs `vendor.img:/firmware/carrierconfig` and
`vendor.img:/firmware/uecapconfig` required by the S5300 userspace package.
The stager recursively expands each reviewed bundle into deterministic,
destination-aware per-file rows and rejects symlinks, special files, unsafe
paths, and empty bundles before the modem service can start.

The default target-scoped allowlist is
`firmware-stage-allowlist.tsv`. It records explicit BCM4383 source/runtime
mappings, including preserved A3 variants, the factory-selected
`vendor.img:/firmware/brcm/BCM.hcd` path, and the two reviewed S5300 directory
bundles. A replacement allowlist remains an explicit override and is still
subject to the same factory-hash and confirmation gates.

The checked-in required allowlist names the five base BCM4383 Wi-Fi files and
their available A3-suffixed runtime variants, the factory-selected Broadcom
`BCM.hcd` destination, and the two S5300 directory bundles. Its source
member/path/size/hash provenance is recorded in the
external firmware manifest; the binary and bundle contents remain external. A
physical controller result and complete Linux HCI/Nitrous integration are
still required before Bluetooth is supported. The S5300 package and bundle
staging are implementation inputs, not runtime evidence: CP boot, modem
protocol, SIM, registration, and bearer evidence remain outstanding. The
exact factory directory paths are recorded in `s5300-source-evidence.md`; no
modem image or Android userspace is copied.

The stock bootloader and radio firmware may remain installed while a future
gated Ferrix boot is evaluated. That is not use of the Android operating
system, and it is not evidence that a Linux controller works. Ferrix must not
depend on Android framework or HAL processes to claim a Linux capability.

## Bounded S5300 CPIF / SIT and SIPC5 slice

The checked-in `crates/ferrix-s5300-cpif` crate is intentionally a small,
source-pinned protocol slice based on the S5300 kernel source object
`a8976934fe7f396627f9900958eb1457a74ebcca`. The exact pinned tegu DTS sets
`mif,protocol = PROTOCOL_SIT`; the crate therefore includes a bounded Exynos
SIT codec as well as a generic SIPC5 codec. Both are link-format primitives,
not modem protocols. The crate also provides a payload-only transport for an
explicitly supplied IPC node:

| Exynos SIT field | Representation |
| --- | --- |
| sync | little-endian `0xabcd` |
| header | 12 bytes, including the two-byte reserved gap |
| sequence / length | little-endian `u16` |
| channel / channel sequence | one byte each |
| fragment config | bounded packet/frame flags |

The userspace payload inside that link frame is the packed FMT envelope from
the pinned S5300 `modem_prj.h`: a seven-byte header containing a little-endian
`u16` total length followed by `msg_seq`, `ack_seq`, `main_cmd`, `sub_cmd`, and
`cmd_type`. `ferrix-s5300-cpif::FmtCodec` bounds and validates this envelope,
and `FmtTransport` applies it only to an explicitly supplied payload
transport. The command tuple remains opaque; no Android RIL, HAL, or
firmware-specific modem command map is brought into Ferrix.

The target-local `pmos/device-google-tegu/s5300-channels.toml` records the
complete source-derived I/O-device catalog, including the `umts_ipc0`/`umts_ipc1`
node names, indexed FMT binding, and exact SIT numeric channel values from the
pinned `exynos-cpif.h`. It is not runtime
evidence and is not a telephony configuration. The Rust CPIF crate's
read-only sysfs status reader can observe the exact `samsung,exynos-cp`
platform node and `modem_state`; it never opens an IPC or boot node.

| SIPC5 layout | Header and length |
| --- | --- |
| ordinary (`0b000`) | 4-byte header, little-endian `u16` total length |
| multi-frame (`0b011`) | 5-byte header, little-endian `u16` plus the packed id/more control byte |
| extended (`0b010`) | 6-byte header, little-endian `u32` total length |

The slice handles the driver's eight-byte padding rule, strict single-frame
decoding and stream-prefix parsing, a default 16 MiB allocation bound, and the
reserved channel values `0`, `5`, `6`, `27`, and `255`. `ChannelDevice` keeps a
target-supplied device path and channel as metadata. The framed transport
remains typed-unsupported, while the explicit payload transport opens only a
caller-supplied IPC node and relies on the kernel to add/remove the selected
SIT or SIPC5 headers; it does not guess a `/dev` node, implement the CPIF
shared-memory/PCIe rings or doorbells, discover device-tree channel bindings,
or contain Samsung modem commands or ModemManager mapping.

## Source-level S5300 channel metadata

The target-local `pmos/device-google-tegu/s5300-channels.toml` records the
complete pinned Zuma DTS I/O-device catalog and exact evidence for
`PROTOCOL_SIT`: the `umts_ipc` I/O device uses
`EXYNOS_CH_ID_FMT_0` with two indexed channels, represented as
`/dev/umts_ipc0` and `/dev/umts_ipc1`. This is source evidence from
`dts/zuma-cp-s5300-sit.dtsi`, not physical runtime evidence. It does not show
that either node exists on the device, that the CPIF driver probed, that the
modem link or firmware works, or that any modem protocol is usable. The
read-only `ferrix_s5300_cpif::discover_at` seam observes `modem_state` without
opening those nodes; `ONLINE` alone never promotes telephony support.

This crate and the channel metadata are not connected to
`ferrix-target-pixel9a`; the checked-in Pixel composition currently connects
`ferrix-telephony-mm` to ModemManager. A real tegu-specific S5300 userspace
backend or ModemManager plugin, physical channel/runtime validation,
controller firmware evidence, modem protocol mapping, SIM/data/SMS/voice/IMS
integration, and physical validation are all outstanding. SIPC5 codec tests
and this source record therefore demonstrate only bounded source/codec
information, not a modem or telephony capability.

## Enablement and evidence order

Each step starts unsupported and becomes supported only after exact-target
evidence is captured and redacted. None of the physical Linux steps below has
been completed; host compilation, mapping tests, package checks, and generated
manifests do not substitute for physical evidence:

1. bounded serial/USB console and orderly shutdown;
2. UFS discovery and a disposable or explicitly allocated Linux rootfs;
3. DRM framebuffer plus inspected capture, then touch/input;
4. battery, charging limits, thermal readings, and safe shutdown;
5. BCM4383 Wi-Fi firmware, scan, WPA association, DHCP, DNS, and traffic;
6. Broadcom Bluetooth firmware, BlueZ adapter power, discovery, pairing, and
   an audio/input profile as applicable;
7. ALSA/PipeWire playback, capture, earpiece, speaker, and microphone routing;
8. S5300 CPIF enumeration, SIM state, network registration, packet data, SMS,
   and only then voice calls and call audio;
9. repeated suspend/resume with charger, Wi-Fi, Bluetooth, and modem wake
   cases.

Cellular transport source is available, and the target package now carries a
source-pinned native-Linux S5300 loader plus SIT ModemManager integration. No
native build, modem object, registration, or bearer has yet been observed on
this exact device. Modem enumeration alone is not phone support. Calls,
VoLTE/IMS, emergency calling, carrier certification, and call audio remain
unsupported until independently implemented and tested; no Android RIL/HAL or
Binder service is part of this route.

## Gates before physical boot or provisioning

The checked-in manifest records some admission/recovery prerequisites, but
those observations do not pass a physical Ferrix boot or capability gate. All
physical Ferrix boot, provisioning, and capability gates remain outstanding.

Recorded prerequisites (not physical Linux evidence):

- [x] exact identity is established;
- [x] official factory and OTA recovery inputs are pinned and verified;
- [x] bootloader is unlocked through the gated workflow;
- [x] factory metadata and partition image inventory are inspectable offline;

Outstanding physical gates (all):

- [x] the checked-in dev-device/no-user-data policy gate is present; this does
  not claim a personal-data backup was verified;
- [ ] stock factory recovery is tested on this exact device;
- [ ] the complete live partition inventory is captured read-only and
  redacted;
- [ ] the complete coherent kernel/module/DTBO split-boot build succeeds from
  pinned source;
- [x] a native postmarketOS rootfs substrate is built with the exact product,
  firmware, pmaports, and target package inputs; bootability remains unproven;
- [ ] the Ferrix artifact manifest passes image-layout and size checks;
- [x] the checked-in inactive-slot activation/boot workflow is implemented and
  stub-tested; it has not run on the physical target;
- [ ] a temporary boot route is proven supported and the inactive-slot plan is
  explicitly authorized for a physical run;
- [ ] exact Bluetooth firmware member/path/hash evidence and live Bluetooth
  controller behavior are established;
- [x] the source-pinned S5300 userspace packages build on native Linux and
  their protocol test passes;
- [ ] the exact tegu channel/device-tree binding and carrierconfig/UECAP
  directory inputs are exercised on the device, with CP boot, modem protocol,
  SIM, registration, and bearer evidence captured;
- [ ] the target-scoped command passes its identity, hash, backup, recovery,
  ambiguity, and per-run confirmation checks.

Until every applicable gate passes, no Ferrix image may be booted or flashed
on the physical target.
