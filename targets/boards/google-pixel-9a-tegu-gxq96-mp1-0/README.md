# Google Pixel 9a — tegu / GXQ96 / MP1.0

This is an exact physical-target identity and stock-recovery package for the
identified device. It is not a claim that Ferrix Linux boots on this phone.
Ferrix product code remains target-neutral; the capability matrix is
conservatively `unsupported` until target-specific Linux evidence exists.

The owner-authorized first stock restore completed on 2026-09-30 with
`result=factory-restore-complete` and exit 0; ignored transcript
`out/pixel-9a/restore/20260930T002730Z.txt`. Post-stage checks observed
bootloader `tegu-17.0-15819938` and baseband
`g5300t-260604-260807-B-16036236`. The pinned stock update wrote slot-A
boot images and shared `super`, selected the already-current slot A, wiped
userdata/metadata, and requested reboot without relocking. Initial discovery
returned `no-device`; the owner then ran the read-only workflow successfully
at `2026-09-30T01:02:38Z`. Saved transcript
`out/pixel-9a/discovery-20260930T010238Z.txt` matches the restore's device
identity and verifies Android 17 / `CP3A.260905.009`, security patch
`2026-09-05`, slot `_a`, and unlocked/orange. Recovery is now `tested` for
this exact device and accepted stock build. Earlier partition capacities
remain historical; no Ferrix boot or capability is established.

After the verified restore, the owner-approved Android-to-bootloader workflow
entered ordinary Fastboot without writing storage. Fresh static A/B capacities
were captured at `2026-09-30T01:44:54Z`. A separate same-device read-only
`userdata` map at `02:02:26Z` correlated Android's `/dev/block/sda34` by-name
entry with a live unslotted physical 117,543,358,464-byte Fastboot partition.
The broader inventory remains `partial`. On native Linux x86_64, a fresh
source-built kernel and boot-input handoff produced a separately verified
eight-partition static bundle (manifest SHA-256
`95b5bcf20f74b83eb00f4159f1a69b6b9e31f0ea143953f6b0f4640bb4ebd78a`).
It is `composed-not-booted` / `bootable=false`; no Ferrix image has been
booted or flashed.

The checked-in `static-install-candidate.toml` pins this bundle, fresh
capacities, same-device discovery, targeted `userdata` map, and tested stock
recovery. Its separate `just pixel-9a-flash-static-check` read-only admission
passed on the exact device in ordinary Fastboot on slot A. The owner approved
one gated temporary boot of the pinned kernel-only `boot.img`; that command
uses stock `init_boot` and `vendor_boot` and cannot validate the full Ferrix
split image set. The `observe-temp` step requires a reviewed kernel capture
and binds its redacted copy to the exact boot transcript. The capture must
contain the complete build banner embedded in the pinned `boot.img`; a stock
tegu kernel banner is refused. If the temporary kernel reaches Android ADB,
set `FERRIX_PIXEL9A_STATIC_TEMP_BOOT_EVIDENCE` to the boot transcript and run
`just pixel-9a-flash-static-capture-temp`. It reads `/proc/version` and the
device-tree model after exact ADB identity checks and writes an ignored,
redacted capture.
This matches the live kernel build banner and device-tree model; it is not
a readback of the loaded image, a full kernel log, or proof of no earlier
kernel errors. `observe-temp` also requires a reviewed human account
of no hang or reboot loop and a fresh ordinary-Fastboot preflight. If ADB does
not return, use a separately captured serial kernel log with the same banner
or stop; do not invent boot evidence. These paths are offline-tested
only; the temporary probe stopped before device action after an optional
transfer-verifier invocation omitted the named output path. No persistent
static flash has been run. Separate offline-tested
`pixel-9a-install-static-check`, `-kernel`, `-rootfs`, and `-activate`
recipes now define three independently confirmed physical stages. The current
target `image_compatibility=partial` gate refuses them, and no observed temporary
boot evidence exists. The eight inactive boot images, shared `userdata`
rootfs, and slot selection each need their own future gate review and per-run
owner approval. The persistent adapter pins the currently installed Fastboot
CLI version `37.0.1-15733141`; Fastboot command success alone cannot verify
on-device bytes. Slot selection is held in a separate pinned
`static-activation-candidate.toml`; the image candidate's no-slot-selection
policy remains in force for both flash stages.

Initial identity was established by the checked-in read-only workflow
`just pixel-9a-discover`:

| Field | Observed value |
| --- | --- |
| Manufacturer / product | Google / Pixel 9a |
| Model number / SKU | GXQ96 |
| Codename / board | `tegu` |
| Hardware revision | `MP1.0` |
| Bootloader | `tegu-16.0-12754673`; locked at discovery, now unlocked |
| Verified boot | `green` before unlock; fresh post-unlock ADB discovery reports `orange` |
| Current slot | `_a` at ADB discovery; `a` in post-unlock Fastboot discovery |
| Dynamic partitions | `true` |
| Stock build | `BD4A.240925.111`, Android 15, 2025-01-05 SPL |

The latest read-only admission refresh ran at `2026-09-29T21:12:06Z`
(`just pixel-9a-partition-inventory`) in ordinary bootloader Fastboot. It
reproduced the identity and A/B topology and recorded the complete read-only
Fastboot `getvar all` output, including `userdata` capacity. The earlier
Android-side inventory remains `partial` because Android denies
`/proc/partitions` and `/proc/cmdline`; neither transcript is evidence of a
Ferrix Linux boot or radio functionality.

The serial and all other unique identifiers are intentionally absent. The
redacted discovery transcript is generated under ignored `out/pixel-9a/`.

With exactly one transport visible, capture the topology gate before any
build or flash route:

```sh
just pixel-9a-partition-inventory
```

On this device Android returned an empty `ro.boot.slot_count`; the checked-in
workflow therefore accepted A/B only after its read-only `/dev/block/by-name`
listing exposed both slot suffixes and recorded
`slot_count_source=block-by-name`. The 2026-09-28 transcript also records that
Android denied `/proc/partitions` and `/proc/cmdline`, so this is exact-target
and partition-topology admission evidence, not Linux-boot or radio evidence.
Generated output is serial-redacted and ignored.

The separate read-only capacity handoff can be collected once exactly one ADB
transport is visible:

```sh
just pixel-9a-partition-capacity
```

It resolves both A/B static partitions and independently mapped dynamic
`vendor_dlkm`/`system_dlkm` devices, reads their sysfs sector counts, and
writes the ten byte capacities to ignored
`out/pixel-9a/partition-capacities-latest.tsv`. This handoff does not upgrade
the broader inventory from `partial`, and it never authorizes a boot or flash.

If Android hides the inactive dynamic mapper nodes, run the same command only
after an owner-controlled transition to Fastbootd, with exactly one Fastboot
device visible. That branch reads independent A/B `partition-size:*` getvars,
requires `is-userspace=yes`, and refuses the ordinary bootloader; it does not
reboot, change slots, or write a partition.

Fastboot on this exact device may report `variant=N/A`. The checked-in
generic matcher and target workflows accept that value only when the same
serial hash has a complete redacted ADB discovery proving Google / Pixel 9a /
`tegu` / `GXQ96` / `MP1.0`; the live Fastboot product, revision, serial, slot,
slot count, unlock state, and transport are still rechecked independently.

The read-only capacity workflow additionally recognizes `variant=MSM USF`
only after verifying `is-userspace=yes`, exact product/revision, and the A/B
slot fields. Google's [userspace Fastboot HAL](https://android.googlesource.com/platform/hardware/google/pixel/+/1c8e4019/fastboot/Fastboot_aidl.cpp)
hardcodes that placeholder; it does not identify the SKU. The workflow still
requires complete redacted ADB identity for the same serial hash proving
`GXQ96`, and records the observed placeholder separately from the verified
SKU. This capacity-read exception leaves the shared identity gate unchanged.
The radio-first flash adapter separately correlates this placeholder only
during a same-process transition from a fully verified ordinary-bootloader
identity, with all live invariants rechecked and complete same-serial ADB
proof of `GXQ96`. It never accepts the placeholder in bootloader mode.
Restoration, unlock, and activation helpers retain their existing rules.

For an owner-authorized read-only investigation of missing capacities in
Fastbootd, use:

```sh
just pixel-9a-partition-capacity-diagnose
```

This uses the same exact-target identity checks, then records all twenty
independent A/B size responses, including device failures and zero values,
in an ignored, serial-redacted diagnostic transcript. It never publishes a
capacity TSV; `result=diagnostic-only` means the scan finished, not that the
installation gates passed. Normal capacity runs also retain the raw query
responses and only mark capacities observed after publishing a complete
handoff. Do not treat a zero CLI exit as a successful query: the local CLI
has returned exit zero with `FAILED (remote: 'Could not open partition')`.
The diagnostic also records lock/secure state, snapshot-update status,
super-partition name, and logical classification of both DLKM pairs. Missing
metadata is recorded as unavailable, never assumed idle, unlocked, or safe.

The physical diagnostic on 2026-09-30 found that Fastbootd could not open
either slot of `vendor_boot`, `vendor_kernel_boot`, `dtbo`, `vbmeta`,
`vbmeta_vendor`, or `vbmeta_system`. It separately reported zero bytes for
`vendor_dlkm_b` and `system_dlkm_b`. Missing static sizes and zero inactive
logical sizes are distinct blockers; neither may be replaced with guessed
or mirrored values. A reviewed acquisition/layout plan is required before
changing the capacity contract or partition allocation. This diagnostic
workflow performs no reboot, resize, slot selection, or flash.

The [stock-preserving installation methodology](linux-plan.md#stock-preserving-installation-methodology)
keeps bootloader/radio firmware and the active stock slot out of the Ferrix
write set. The radio-first flash workflow now checks live A/B sizes against
the reviewed handoff and requires logical images to match existing allocation
exactly. After confirmation, it verifies Fastbootd identity, logical sizes,
and idle snapshot state in an initial transport round trip before any image
write, and rechecks at write time. This is offline-tested safety behavior,
not permission to resize the zero-capacity inactive partitions. The targeted
read-only `userdata` map does not authorize a shared-data wipe.
The tested pinned factory restore is the owner's selected stock recovery
route. Persistent static flash remains physically unattempted and gated by
image compatibility, a reviewed temporary-boot observation, and separate
per-run confirmations.

This is an owner-declared new development device with no personal data. The
checked-in gate records `backup = "not-required-dev-device"` and
`data_policy = "dev-device-no-user-data"`; it deliberately does not claim a
verified backup. Before any destructive or boot-state mutation, the owner
must explicitly authorize the session policy with
`FERRIX_PIXEL9A_NO_USER_DATA=I-understand-this-is-a-new-development-device-with-no-personal-data`
and answer the workflow's exact per-run confirmation prompts. The checked-in
stock recovery command and matching post-restore Android verification have
completed. Other provisioning gates still apply independently.

## Recovery files

The checked-in pins select Google's current public Pixel 9a build recorded on
2026-09-28: `CP3A.260905.009` / Android 17.0.0.

Run:

```sh
just pixel-9a-recovery-download
```

The workflow downloads and SHA-256 verifies both archives under:

```text
targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/recovery/
```

That exact folder is ignored by Git. The factory archive is the Fastboot
recovery input. The full OTA archive is the preferred ADB recovery input.
Firmware bytes remain local; the checked-in `recovery.pins` file records the
official URLs and hashes.

After the factory archive has been downloaded, inspect its compatibility
metadata and image-member inventory entirely offline:

```sh
just pixel-9a-factory-inspect
```

This re-verifies the pinned size and SHA-256, reads `android-info.txt` and
`fastboot-info.txt` from the stored nested image archive without extracting
its multi-gigabyte payload, and writes a redacted machine-readable inventory
to ignored `out/pixel-9a/factory-inspection/inventory.json`. It does not invoke
ADB, Fastboot, a downloader, or any device-facing command.

## Bootloader unlock gate

The recovery gate is satisfied for the expected ADB/Fastboot failure modes,
but unlocking is destructive: Google will erase all user data. Make a
verified backup outside this repository before proceeding. The checked-in
workflow re-verifies both recovery archives and the exact device identity,
refuses ambiguous transports, prints the serial/product/revision/slot and
artifact hashes, and stops on any mismatch.

Run the read-only gate first:

```sh
just pixel-9a-unlock-bootloader --preflight
```

After reviewing that output and accepting the data wipe, the execution mode
requires two exact typed confirmations and the phone's own bootloader prompt:

```sh
just pixel-9a-unlock-bootloader --confirm-unlock
```

The workflow may issue only `adb reboot bootloader` and then
`fastboot flashing unlock`. It does not flash a ROM or change slots. If the
unlock succeeds, the phone remains in Fastboot so the verified factory archive
can be restored. Do not continue if the preflight or either identity check
fails.

This gated workflow completed successfully on 2026-09-27 UTC. Post-unlock
Fastboot discovery reported the exact device as `unlocked=yes`; no factory
restore or slot change had been run at that point. The later 2026-09-30
stock restore is recorded above.

## Development Fastboot entry

When stock Android is running and ADB is authorized, the LLM may place the
exact target into ordinary bootloader Fastboot through the checked-in,
non-destructive workflow:

```sh
just pixel-9a-enter-fastboot --preflight
just pixel-9a-enter-fastboot --confirm-enter
```

The workflow verifies the exact Google Pixel 9a / `tegu` / `GXQ96` / `MP1.0`
identity before and after issuing `adb reboot bootloader`, requires the typed
phrase `CONFIRM PIXEL 9A ENTER FASTBOOT`, and never changes storage, slots,
the bootloader, or user data. Hard-button entry is only a fallback when ADB is
unavailable. This device may report `variant=N/A` from bootloader Fastboot;
the dev workflow treats that field as unavailable only after the exact ADB
identity was checked before reboot and product, hardware revision, serial,
slot count, and `is-userspace=no` are rechecked afterward. This is ordinary
bootloader Fastboot; the separate capacity workflow still refuses to enter
Fastbootd on its own.

## Recovery choices

The owner approved an explicit first-stock-recovery-test contract on
2026-09-30, accepting pinned stock `CP3A.260905.009` as the recovery
destination. The following bootstrap commands were used while recovery was
`documented-not-tested`; they are no longer appropriate now that recovery is
`tested`:

```sh
just pixel-9a-factory-restore --preflight-first-test
# Only after exact preflight review and fresh owner authorization:
just pixel-9a-factory-restore --confirm-first-test
```

The development-device data-policy token is still required for execution.
This mode verifies the same official archives, board requirements, vendor
sequence, unique device, and image hashes; it permits only the explicit
`documented-not-tested` bootstrap state and always wipes data. It adds
`CONFIRM PIXEL 9A FIRST STOCK RECOVERY TEST (WIPES ALL USER DATA)` before
the existing bootloader/radio and update confirmations. It replaces stock
bootloader/radio firmware as part of Google's sequence; this is distinct
from Ferrix's firmware-preserving installation route. `--keep-data` is
refused. Command completion does not prove a successful Android boot, and
the workflow never edits recovery gates. Capture fresh same-device discovery
of the expected stock build before recording recovery as tested.

Back up personal data first. Google documents full OTA sideload as the safer
route: it normally does not require an unlocked bootloader or a data wipe. The
device was locked during initial discovery and is now unlocked; the OTA route
remains preferred while stock recovery is intact.

Both stock recovery routes now have checked-in, target-scoped workflows.
Both are stub-tested offline (`just pixel-9a-tools-test`). The first-test
factory workflow and matching post-restore Android verification completed
on the physical device on 2026-09-30. OTA sideload remains untested
on hardware.

For an OTA sideload, `just pixel-9a-sideload-ota --preflight` re-hashes the
pinned full OTA and factory archives, accepts exactly one ADB device in
Android, stock recovery, or "Apply update from ADB" (sideload) mode, and
re-checks the exact identity as far as that transport permits (sideload mode
exposes none, and its confirmation phrase must acknowledge that).
`--confirm-sideload` may issue only `adb reboot recovery` and `adb sideload`
after two typed confirmations.

For Fastboot restoration, `just pixel-9a-factory-restore --preflight` requires
the offline factory inventory from `just pixel-9a-factory-inspect`, verifies
that the archive's own `flash-all.sh` matches the reviewed bootloader, radio,
then `fastboot -w update` sequence, and requires one unlocked `tegu`/`MP1.0`
bootloader-mode Fastboot device (`--from-adb` adds a gated
`adb reboot bootloader`). `--confirm-restore` stages only the three needed
archive members, requires two typed confirmations naming the wipe
(`--keep-data` omits `-w`), stops if the bootloader or baseband version does
not match the archive requirement after each stage, and never issues lock,
standalone erase, format, slot, or oem commands. The vendor update itself
can select a slot, write shared `super`, and erase userdata/metadata, as
observed in the approved first test. Only serial-redacted transcripts remain
under `out/pixel-9a/{sideload,restore}/`. Do not run the vendor script
directly. Command completion alone does not establish a tested restore path;
fresh same-device Android verification was also captured for the recorded test.

The read-only factory preflight passed on `2026-09-29T21:11:49Z`: both pinned
official recovery archives, the offline factory inventory, the required
bootloader/baseband versions, and the reviewed vendor sequence matched. This
did not test stock recovery or change the then-current
`recovery_procedure=documented-not-tested` gate. The later completed restore
and stock Android discovery, not this preflight, established `tested`.

For expected failure states, use the route matching the transport: stock
Android or stock recovery can use the verified full OTA with `adb sideload`;
an unlocked Fastboot device can use the verified factory archive and its
reviewed Fastboot commands. A hardware, cable, or bootloader failure that
leaves neither ADB nor Fastboot visible cannot be repaired by a host-side
workflow alone.

## Source and support boundary

- Google factory images: <https://developers.google.com/android/images>
- Google full OTA images: <https://developers.google.com/android/ota>
- Google Pixel 9a specifications/model list: <https://support.google.com/pixelphone/answer/7158570>
- AOSP device repository: <https://android.googlesource.com/device/google/tegu/>
- AOSP Android GS kernel basis: <https://android.googlesource.com/device/google/tegu-kernels/>
- AOSP device kernel tree: <https://android.googlesource.com/kernel/devices/google/tegu/>
- External radio correlation (not Ferrix runtime evidence):
  [`ubuntu-touch-radio-correlation.md`](ubuntu-touch-radio-correlation.md)
- Associated GNSS/NFC/UWB route audit (source comparison, not runtime evidence):
  [`associated-radio-audit.md`](associated-radio-audit.md)

The target-local postmarketOS `device-google-tegu` package and the typed
Ferrix adapter composition are now checked in under `pmos/device-google-tegu/`
and `adapter/`. They are native-Linux implementation inputs, not hardware
evidence: the capability matrix remains conservatively `unsupported` until
this exact MP1.0 device produces subsystem evidence. In particular, the
S5300 CPIF modem is not a standard ModemManager transport. The target package
carries immutable native-Linux `exynos-modem`/`sit-proxy` and SIT ModemManager
recipes behind the public ModemManager D-Bus boundary; their ABI requires the
separately pinned radio-first `s5300_modem` WWAN kernel backend, not Google's
stock `cpif` character-device surface. The native Linux radio-first kernel and
target-local userspace packages have now been built on the authorized x86_64
builder, but the route remains unbooted and has not been observed to create a
modem on this device. The Rust
CPIF crate remains a bounded SIT link and seven-byte userspace FMT envelope
codec, keeps the firmware-specific command tuple opaque, and does not import
Android RIL/HAL code. The package records the exact source-level SIT channel
metadata in `pmos/device-google-tegu/s5300-channels.toml` (`umts_ipc0` and
`umts_ipc1`); that file is not runtime evidence and does not enable modem
traffic by itself. The source-level ABI boundary, package pins, and required
runtime gate are recorded in
[`s5300-source-evidence.md`](s5300-source-evidence.md).

The Samsung-SIT data path requires a carrier-specific APN. The target package
therefore carries an empty `FERRIX_TELEPHONY_APN` runtime setting rather than
guessing one; set that non-secret value in the guest's
`/etc/ferrix/backends.env` before enabling Cellular data. The target's
`ferrix-telephonyd` service receives that value as a daemon-only fallback for
default `connect-data` requests, while the product service also passes it only
to the Settings client, which sends it through the typed `telephony-data`
request. An explicit request APN wins; malformed configured values fail closed.
No APN value is recorded in evidence or logs.

The S5300 package also needs two directory-valued inputs from the exact
factory `vendor.img`: `/firmware/carrierconfig` and
`/firmware/uecapconfig`. They must be staged as
`/lib/firmware/carrierconfig` and `/lib/firmware/uecapconfig`; the target
rootfs's `/vendor/firmware` compatibility root then satisfies the daemon and
udev paths. The checked-in firmware allowlist/stager recursively expands these
bundles into destination-aware per-file manifest rows and fails closed on
unsafe or special entries. Public Tegu extraction metadata corroborates the
two factory paths
([`proprietary-files-vendor.txt`](https://gitlab.com/CalyxOS/device_google_tegu/-/raw/android17/tegu/proprietary-files-vendor.txt));
it is a path reference, not permission to copy Android userspace or commit
proprietary bytes. The package and staged inputs have now passed the native
Linux rootfs build and hash-manifest handoff; exact-device boot is still
required before they provide runtime evidence.

Build the physical product handoff separately from QEMU:

```sh
just pixel-9a-product-check
FERRIX_PIXEL9A_PRODUCT_BUILD=I-understand-this-builds-the-pixel-9a-product \
  just pixel-9a-product-build
just pixel-9a-product-test
```

The workflow requires native Linux x86_64, stages the target profile, physical
app catalog, colors, and production radio feature paths under
`out/pixel-9a/product/tegu`, and refuses to build without the typed opt-in.
Product compilation is deliberately isolated in its own target-scoped Cargo
target/cache/output roots. It is separate from the Google Kleaf kernel,
pmOS/rootfs, firmware, recovery, and QEMU environments; the selected target
and backend inputs are recorded in `manifest.tsv`.

Do not infer display, input, audio, modem, Wi-Fi/Bluetooth, sensors, suspend,
or Linux boot support from stock Android or from QEMU.

## Separate Pixel development environment

The QEMU profile remains `targets/qemu-aarch64/profile.toml`. Pixel host
tooling has its own canonical environment and output roots:

```sh
. tools/pixel-9a/env.sh
just pixel-9a-env-validate
just pixel-9a-env-print
just pixel-9a-env-test
```

Source the loader in the shell that runs the native build recipes. It exports
the canonical product, kernel, rootfs, firmware-staging, split-boot, and
artifact-manifest paths as `FERRIX_PIXEL9A_*` variables; the compatibility
`FERRIX_CACHE_DIR`/`FERRIX_OUT_DIR` values remain at the repository parents so
QEMU continues to resolve `out/qemu-aarch64/`. `just pixel-9a-env-check build`
also verifies the native Linux `x86_64` host toolchain and the configured Rust
target standard library without creating output. It also requires the physical
profile's `aarch64-unknown-linux-musl` linker; provision an
`aarch64-linux-musl-gcc`-compatible command explicitly on the native Linux host
(or set `FERRIX_PIXEL9A_RUST_LINKER`). Repository workflows do not install host
tools.

The target runtime profile staged into a native guest is
`dev/profile.toml`; it contains only hardware-shaped display/input defaults.
Production Linux adapters, not that profile, provide live power, network,
Bluetooth, audio, and modem state.

The checked-in [native Linux enablement plan](linux-plan.md) records the
Linux-first product boundary, the comparison Google downstream baseline and
radio-first kernel backend, split-boot composition, firmware policy, service
composition, evidence order, and remaining gates. It is a plan and source
record, not hardware evidence.

### UTM and the build host

UTM is not currently a compliant Pixel build host on this Apple Silicon Mac.
The checked-in workflows require native Linux `x86_64` because the pinned
Kleaf host tools and the postmarketOS/product handoff execute Linux host
tooling directly. The local UTM VM is an ARM64 guest and is stopped; an
`x86_64` UTM guest here would rely on CPU emulation, which the mobile-device
provisioning playbook explicitly rejects. `just pixel-9a-env-check build`,
`just pixel-9a-kernel-radio-check`, and the physical-build checks therefore
refuse this host before creating build output. Static target checks and
device-free workflow tests may still run locally. UTM may still be used for a separate
Linux smoke-test or QEMU workflow, but the Pixel kernel, firmware, rootfs,
product, and boot handoff must run on a provisioned native Linux `x86_64`
machine; the target is now built-but-not-booted rather than `not-built`.

## Pinned downstream kernel build (comparison route)

Google documents Pixel 9a (`tegu`) on the
`android-gs-tegu-6.1-android16` Pixel kernel branch with the `android14-6.1`
GKI family. The device uses Kleaf/Bazel, and Google's `build_tegu.sh` resolves
to the `//private/devices/google/tegu:zumapro_tegu_dist` target. Ferrix builds
that target with `--kernel_package=@//aosp`, so the GKI kernel comes from the
checked-out pinned source rather than a prebuilt GKI binary.

The board package preserves Google's exact `private/devices/google/tegu/
tegu_defconfig` as [`kernel-config.tegu`](kernel-config.tegu). It enables the
S5300 CPIF S5100/PCIe/shared-memory/packet-processing path, CP thermal support,
and the target display/touch fragments. `pixel-9a-kernel-verify` compares the
checked-out source fragment byte-for-byte before any build; this is a Linux
kernel configuration contract and does not add Android userspace.

The workflow is host-only and never contacts a phone:

```sh
just pixel-9a-kernel-check
FERRIX_PIXEL9A_KERNEL_SYNC=I-understand-this-downloads-large-kernel-sources \
  just pixel-9a-kernel-sync
just pixel-9a-kernel-verify
FERRIX_PIXEL9A_KERNEL_BUILD=I-understand-this-runs-a-large-kleaf-build \
  just pixel-9a-kernel-build
```

`pixel-9a-kernel-check` is read-only and installs nothing. The pinned Google
host prebuilts require a native Linux x86_64 host; macOS fails with an
actionable refusal rather than attempting a build. The check also requires
Google's `repo` launcher and at least 80 GiB free by default. Sync is explicitly
opt-in and stores the large checkout beneath ignored
`.cache/ferrix/pixel-9a/kernel/` (or `FERRIX_CACHE_DIR`). It initializes the
manifest at the exact commit in `kernel-sources.toml`, retains the exact Google
superproject commit for verification, and refuses any listed component whose
checkout differs from its pin.

Build output goes to ignored `out/pixel-9a/kernel/tegu/dist/` (or the explicit
output override). A successful build writes `build-manifest.tsv` containing
the target, branch, manifest/superproject/component revisions, the pinned
`tegu_defconfig` digest, and the size and SHA-256 of every dist artifact. It
refuses a nonempty dist directory rather than overwriting an earlier build.
The shell code is orchestration only:
Google's `repo` performs source synchronization and Google's checked-out
Kleaf/Bazel target performs compilation. It does not implement a kernel build
system, construct a boot image, invoke physical-device tools, or provision any
partition.

Offline refusal and static safety tests run with:

```sh
just pixel-9a-kernel-test
```

The radio-first route selected by the target package is the independent
native-Linux Kbuild candidate, not the Google CPIF comparison route above. It
is pinned in `kernel-radio-route.toml` and pairs with the `s5300_modem` WWAN
userspace ABI:

```sh
just pixel-9a-kernel-radio-check
FERRIX_PIXEL9A_RADIO_SYNC=I-understand-this-downloads-the-radio-first-kernel \
  just pixel-9a-kernel-radio-sync
just pixel-9a-kernel-radio-verify
FERRIX_PIXEL9A_RADIO_BUILD=I-understand-this-runs-the-radio-first-kernel-build \
  just pixel-9a-kernel-radio-build
just pixel-9a-kernel-radio-test
```

The sync step downloads the pinned source archive into the target cache and
verifies its recorded byte count plus SHA-256/SHA-512 before creating or
building the clean detached checkout. New checkouts use a depth-1,
single-branch, `blob:none` promisor clone, so Git can fetch only the blobs
needed by verification and Kbuild instead of transferring the complete
history. The workflow verifies that clone shape, the exact source revision,
the archive hashes, the pinned remote, and a clean checkout before Kbuild; a
pre-existing full clone must be replaced with a fresh source path. A missing
or mismatched archive stops the route before Kbuild.

This route requires the same native Linux x86_64 build host, records the exact
candidate revision/config/DTS hashes, checks the IP/WWAN/rmnet data path and
the Wi-Fi, Bluetooth, and serdev symbols, and emits only a hash-verified
`Image`/`dtb.img` handoff. At runtime the candidate is expected to create
`wwan0sit0`/`wwan0sit1`, an AT WWAN port, and `rmnet0`-`rmnet29` only after CP
ONLINE; those names must be captured in the physical evidence transcript.
The route is still `built-not-booted` / `bootable=false`; no physical radio
claim is made until the kernel and rootfs have been booted on this exact MP1.0
device.

## Native Linux package, firmware, and split boot

The package workflow stages the complete Ferrix product set and target runtime
profile into an unbooted pmOS rootfs substrate. Firmware is an external,
reviewed, hash-verified input; it is never committed to Git:

```sh
just pixel-9a-firmware-check
just pixel-9a-firmware-inventory
just pixel-9a-rootfs-check
just pixel-9a-rootfs-test
```

The authorized native Linux x86_64 build completed these handoffs on
2026-09-30: the radio-first kernel is `built-not-booted`, the product is
`built`, and the pmOS rootfs manifest is `built-substrate-only` with
`bootable=false`. The generated outputs remain ignored on the builder under
`out/pixel-9a/`; their hashes are recorded in `docs/status.md`. This confirms
the source, package, firmware-manifest, and image-construction handoff only;
it does not confirm a boot, display, input, service, or radio capability on
the phone.

### Broadcom Bluetooth / Nitrous firmware provenance

The exact selected Broadcom patchram blob is now recorded in the target-level
policy as `BCM.hcd`. Its provenance, byte size and SHA-256, the two present but
unselected HCD variants, the factory configuration that selects it, and the
Google Nitrous/UART source evidence are documented in
[`bluetooth-firmware-source-evidence.md`](bluetooth-firmware-source-evidence.md).
The evidence is from the hash-verified official `tegu` factory package; no
firmware bytes are checked in.

This does not yet claim Linux Bluetooth works on the physical device. The
target package's pmOS runtime allowlist now includes the exact
`lib/firmware/brcm/BCM.hcd` destination; the external staging manifest must
still provide that file from the reviewed factory path. The selected
radio-first kernel uses its Tegu `hci_bcm` power/wake/BLE-DBO path and must not
be combined with the canonical Google `nitrous.ko` module; the Rust OS still
needs physical boot evidence before Bluetooth can be marked supported. Do not
substitute `BTFW_B.hcd` or `BTFW_D.hcd`: the factory configuration does not
select either one for this target.

The default target-scoped extraction allowlist is
`firmware-stage-allowlist.tsv`. It maps the factory's generic BCM4383 firmware
and map files, their `_4383_a3` variants, and the available A3 calibration,
CLM, and TXCAP variants to explicit Linux runtime names. A3 calibration/CLM/
TXCAP bytes are retained under their suffixed names and exposed under the base
driver request names as fallback copies. The allowlist also records the exact
`BCM.hcd` source path and two S5300 factory directory bundles (`carrierconfig`
and `uecapconfig`). Bundle rows are expanded to destination-aware per-file
manifest rows; symlinks, special files, unsafe components, and empty bundles
fail closed. `FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST` may point to a separately
reviewed replacement, but extraction still requires the explicit firmware
confirmation and the exact factory hash.
The pinned BCM4383 module name, firmware request basenames, and platform
variant lookup contract are recorded in
[`wifi-firmware-source-evidence.md`](wifi-firmware-source-evidence.md);
the radio-first route maps the reviewed A3 factory variants to the candidate's
`brcmfmac4383-pcie.*` names only through the explicit, hash-pinned
`firmware-radio-route.tsv` copy/rename manifest. The canonical Google route
keeps the `bcmdhd` request names and variant suffixes; neither route silently
treats the two contracts as interchangeable.

The canonical Google comparison composer (`tools/pixel-9a/boot.sh`) validates
the Kleaf contract and deliberately rejects a radio-first rootfs. Its offline
test/build commands are:

```sh
just pixel-9a-boot-test
just pixel-9a-boot-check
FERRIX_PIXEL9A_BOOT_BUILD=I-understand-this-composes-pixel-9a-boot-artifacts \
  just pixel-9a-boot-build
```

Its manifest deliberately remains `composed-not-booted` / `bootable=false`.
For the selected radio route, build the source-owned boot inputs first:

```sh
just pixel-9a-boot-inputs-test
just pixel-9a-boot-inputs-check
FERRIX_PIXEL9A_BOOT_INPUTS_BUILD=I-understand-this-builds-source-owned-pixel-9a-boot-inputs \
  just pixel-9a-boot-inputs-build
```

The complete split output is then composed only after the redacted,
exact-target ten-partition capacity handoff is available:

```sh
just pixel-9a-radio-split-test
just pixel-9a-radio-split-check
FERRIX_PIXEL9A_RADIO_SPLIT_BUILD=I-understand-this-composes-the-radio-first-split-artifacts \
  just pixel-9a-radio-split-build
```

`tools/pixel-9a/boot-inputs.sh` hashes the source-built rootfs BusyBox,
checked-in Ferrix initramfs/vendor-ramdisk/bootconfig inputs, and source-built
module roots. `tools/pixel-9a/radio-split.sh` generates the missing
`vendor_kernel_boot`, `dtbo`, `vendor_dlkm`, `system_dlkm`, and fresh AVB
metadata, then carries the kernel, rootfs, capacity, and boot-input provenance
inside the output directory. The manifest remains `composed-not-booted` /
`bootable=false`; temporary-boot command success does not prove a Ferrix Linux
boot on the exact device. The older
`just pixel-9a-boot-radio-*` route remains a diagnostic incomplete handoff and
is not accepted by the flash workflow. No physical boot, flash, slot change,
or factory restore is implied by host-side builds.

The radio-first candidate builds its Wi-Fi, Bluetooth, S5300 WWAN, UFS, and
filesystem paths in-kernel. The rootfs workflow therefore overlays the
route-local `pmos/device-google-tegu/modules-initfs-radio` list and never
requests the Google Kleaf `bcmdhd4383`/`nitrous`/`cpif` module list from the
incompatible comparison route. That list remains empty until a native Linux
Kbuild output proves a required module and records its hash. A completed
native build also records the source-built DTB/DTBO set, `modules_install`
tree, and explicit not-built split-partition status; those rows are provenance
handoffs, not flashable partition artifacts.
The radio boot preflight re-hashes every listed DTB/DTBO and installed-module
file and requires at least one source-built `.dtbo` overlay, so a DTB-only or
stale handoff cannot silently advance into boot composition.

## Gated Ferrix flash

`tools/pixel-9a/flash.sh` is the gated route from a complete canonical split
artifact set to the exact device. `just pixel-9a-flash-check` is read-only. `just
pixel-9a-flash-boot-temp` issues a one-time `fastboot boot` of the manifest
`boot.img` and writes a redacted evidence transcript under
`out/pixel-9a/flash/`. `just pixel-9a-flash --stage kernel --slot b`
requires that transcript (`FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE`, enforcing the
`image-layout.toml` temporary-boot-first policy), a verified factory archive,
exact identity, `unlocked=yes`, and a typed phrase naming the explicitly
selected inactive slot before writing slot-suffixed partitions; omitting
`--slot` or naming the active slot is refused. Dynamic `*_dlkm` partitions
are written from fastbootd. With `avb_mode=NONE` on a `secure=yes` device it
additionally requires `--allow-unsigned-avb`, because unsigned images boot
only while the bootloader stays unlocked. A header-v4 temporary boot uses the
device's current `vendor_boot`/`init_boot`/`dtbo`, so it is only a kernel smoke
check.
The rootfs stage can consume the generated `rootfs.img`, but remains gated
until the target package explicitly selects `userdata` and a complete,
serial-redacted read-only ADB partition inventory matches the same device,
product, revision, SKU, current slot, and slot count. The owner must still
pass the normal backup/recovery/confirmation gates. This workflow is stub-tested only (`just
pixel-9a-flash-test`) and has not been run on the phone; the manifest stays
`bootable=false` until an observed Linux boot.

## Inactive-slot activation and boot

The radio-first flash route intentionally writes only an explicitly selected
inactive slot and never selects it. After its temporary-boot evidence and
`result=flashed-not-booted` transcript have been reviewed, use the separate
checked-in workflow below; do not type `fastboot set_active` or `fastboot
reboot` directly:

```sh
export FERRIX_PIXEL9A_RADIO_TEMP_BOOT_EVIDENCE=out/pixel-9a/flash-radio/<boot-temp-transcript>
export FERRIX_PIXEL9A_RADIO_TEMP_BOOT_OBSERVATION=out/pixel-9a/flash-radio/<observe-temp-transcript>
export FERRIX_PIXEL9A_RADIO_FLASH_EVIDENCE=out/pixel-9a/flash-radio/<flash-transcript>
just pixel-9a-boot-inactive --preflight --slot b --allow-unsigned-avb
FERRIX_PIXEL9A_NO_USER_DATA=I-understand-this-is-a-new-development-device-with-no-personal-data \
  just pixel-9a-boot-inactive --confirm-boot --slot b --allow-unsigned-avb
```

The workflow re-hashes the complete split manifest and both pinned recovery
archives, proves that all ten reviewed partition writes succeeded on the same
device and inactive slot, rechecks `tegu` / `MP1.0` / serial / slot / ordinary
Fastboot immediately before and after selection, then issues the checked-in
`set_active` and reboot sequence. Its result is only
`slot-activated-boot-requested`; it is not Linux or capability evidence. It is
stub-tested (`just pixel-9a-boot-inactive-test`) and has not run on hardware.

Both persistent flashing and slot activation require the separate
`pixel-9a-observe-temp-boot` record described in [the development workflow](dev/README.md#radio-first-physical-handoff).
It binds the operator-reviewed kernel capture to the same temporary-boot
transcript, image hash, and device. A successful Fastboot command alone does
not satisfy the observation requirement.

## Radios on a booted guest

Once a Ferrix/pmOS guest is booted and reachable over the USB network
(`172.16.42.1`, SSH user from `pmos/pmbootstrap.cfg`),
`tools/pixel-9a/radios.sh` verifies the guest's device tree contains
`google,ZUMA PRO TEGU` (the exact root compatible string from the pinned
`zumapro-tegu-mp.dts`) and the exact model `ZUMA PRO TEGU MP on ZUMA PRO`;
override `FERRIX_PIXEL9A_DT_COMPATIBLE` only after a new pinned source revision
is reviewed; QEMU is refused. Inventory also reads the CPIF platform/driver
surface and the source-derived `umts_ipc0`/`umts_ipc1` and `rmnet*` node
presence without opening or mutating them. It then offers
`just pixel-9a-radios-check`, a read-only `just pixel-9a-radios-inventory`,
confirmation-gated `just pixel-9a-radios-enable [wifi|bluetooth|modem|all]`,
`just pixel-9a-radios-connect-wifi <SSID>` (PSK via `FERRIX_PIXEL9A_WIFI_PSK`
or a no-echo prompt, piped to `nmcli --ask`, never logged), and
`just pixel-9a-radios-evidence`, which prints a proposed `capabilities.toml`
fragment for the owner to paste. Transcripts under `out/pixel-9a/radios/`
redact MAC, IMEI/IMSI/ICCID, serial, SSID, and IPv6 values; every evidence
line is labelled `physical: google-pixel-9a-tegu-gxq96-mp1-0`. `status`
follows `CapabilityStatus` (`supported|unsupported`); a `progress` column
carries intermediate findings, and telephony stays `unsupported` because the
S5300 CPIF exposes no ModemManager modem. Wi-Fi is promoted only after full
NetworkManager connectivity, an associated IPv4 address, and successful typed
Ferrix network/core state replies plus per-file size/SHA-256 verification from
the guest `firmware-manifest.tsv`; Bluetooth requires a powered controller and
the corresponding `present`/`enabled` state from Ferrix core. This workflow is
stub-tested only
(`just pixel-9a-radios-test`); no radio has been observed working on the
physical device, and it never edits `capabilities.toml`.

## Agent playbook

Any LLM agent provisioning this or another phone follows
`skills/mobile-device-provisioning/SKILL.md`, which orders the workflows
above, encodes the confirmation policy, and starts with the generic
`just device-discover` / `just device-match` front door.
