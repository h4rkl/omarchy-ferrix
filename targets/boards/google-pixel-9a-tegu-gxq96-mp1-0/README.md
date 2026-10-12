# Google Pixel 9a — tegu / GXQ96 / MP1.0

> **Current state (2026-10-10):** on the
> [development flash route](#development-flash-route) Ferrix boots from slot b
> and reaches home unattended under required app isolation. Power, display,
> input, network (including WPA3-SAE) and Bluetooth are `supported` in
> [`capabilities.toml`](capabilities.toml); audio, telephony and lifecycle
> stay `unsupported`. See the newest physical entry in
> [`docs/status.md`](../../../docs/status.md) ("Pixel 9a: product reaches home
> unattended under required isolation"). The paragraphs below describe the
> earlier static/split routes and the recovery history, and their
> "unsupported"/"no Ferrix boot" statements apply to those routes only.

This is an exact physical-target identity and stock-recovery package for the
identified device. It is not a claim that Ferrix Linux boots on this phone.
Ferrix product code remains target-neutral; the capability matrix is
conservatively `unsupported` until target-specific Linux evidence exists.
The [fresh static build handoff](dev/README.md#one-fresh-static-build-attempt)
is the selected WWAN/SIT route. The current candidate is blocked after an
unobserved temporary probe. The checked-in `image_compatibility=partial` gate
also refuses persistent installation. Those refusals apply to the static
and split routes only: under the 2026-10-09 owner policy, the
[development flash route](#development-flash-route) is gated solely on keeping
the pinned stock factory restore possible, and agents may run it autonomously.

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
The broader inventory remains `partial`. On native Linux x86_64, fresh
source-built kernel, product, rootfs, and boot-input handoffs produced the
current, separately verified eight-partition static bundle (manifest SHA-256
`78c7d21d01d12cc464c76be98bef60afccd38b4d8cd57e0b6c77ff0e670512ed`).
The bundle is `composed-not-booted` / `bootable=false`. One kernel-only
`fastboot boot` was accepted, but the candidate kernel was not observed;
no partition was flashed.

The checked-in `static-install-candidate.toml` pins this bundle, fresh
capacities, same-device discovery, targeted `userdata` map, and tested stock
recovery. Read-only admission of the earlier candidate passed on the exact
device in ordinary Fastboot on slot A at about 04:36 UTC on 2026-09-30; that
result does not admit this bundle. The earlier one-probe approval named a
different `boot.img` hash and is historical. The current kernel-only
`boot.img` has SHA-256
`f310cae8e6fd2c9d7992d68b3020901c6bb4c59fa5fa1fbf3f0c32d2064c8bfe`.
Its one temporary probe used stock `init_boot`, `vendor_boot`,
`vendor_kernel_boot`, and `dtbo`. The
post-probe Android `/proc/version` differed from the candidate's kernel
banner, so the candidate is now `probe-unobserved` and must not be loaded
again without a new build and compatibility investigation.
The boot-only probe cannot test the candidate device tree and full boot image
set together; the subsequent watchdog reason does not isolate the fault.
The static and split composers now refuse the generic Kbuild DTBO inventory:
the selected experimental source has no source-built tegu MP1.0 overlay, while
the verified stock table identifies MP1.0 as ID `0x60b06` / revision
`0x10000`. A new bundle needs a compatible source-built overlay before image
composition; the historical bundle remains blocked.
The read-only `just pixel-9a-postprobe-diagnose` workflow checks exact Android
identity and records boot-reason properties and bounded console pstore summaries
under ignored `out/pixel-9a/postprobe/`. After this probe, Android reported a
watchdog reboot; ADB could not list console pstore. These values do not show
which component failed or prove a Ferrix kernel boot.

For each new physical loading attempt, rebuild the kernel, product, rootfs,
boot inputs, and static image bundle on a native Linux builder from the
reviewed commit into new attempt-specific output directories. Recheck the complete bundle
after transfer to the Mac with `FERRIX_PIXEL9A_STATIC_BOOT_OUT` set to its
exact path, then update the checked-in candidate hashes and run a fresh
read-only device preflight. An earlier probe approval names the earlier boot
hash; obtain a new per-run go-ahead for a changed image. The temporary probe
and any later staged install must use the same hash-bound bundle.

The `observe-temp` step requires a reviewed kernel capture and binds its
redacted copy to the exact boot transcript. The capture must
contain the complete build banner embedded in the pinned `boot.img`; a stock
tegu kernel banner is refused. If the temporary kernel reaches Android ADB,
set `FERRIX_PIXEL9A_STATIC_TEMP_BOOT_EVIDENCE` to the boot transcript and run
`just pixel-9a-flash-static-capture-temp`. It reads `/proc/version` and the
device-tree model after exact ADB identity checks and writes an ignored,
redacted capture. This matches the live kernel build banner and device-tree
model. If Android denies the device-tree model, it records only a hash-bound
version capture, which cannot pass `observe-temp`; a separate reviewed kernel
log is then required. If `/proc/version` differs from the pinned image, it
records a hash-bound mismatch and refuses the capture. A full capture is not
a readback of the loaded image, a full kernel log, or proof of no earlier
kernel errors. `observe-temp` also requires a reviewed human account
of no hang or reboot loop and a fresh ordinary-Fastboot preflight. If ADB does
not return, use a separately captured serial kernel log with the same banner
or stop; do not invent boot evidence. These paths are offline-tested only.
The earlier candidate's temporary probe stopped before device action after
an optional transfer-verifier invocation omitted its named output path. The
current bundle's exact-path transfer check passed. Its temporary boot
command was accepted, but no candidate kernel boot was observed.
No persistent static flash has been run. Separate offline-tested
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

Fastboot on this exact device may report `variant=N/A`; Fastbootd may report
`variant=MSM USF`. Google's [userspace Fastboot HAL](https://android.googlesource.com/platform/hardware/google/pixel/+/1c8e4019/fastboot/Fastboot_aidl.cpp)
hardcodes `MSM USF`, which does not identify the SKU. The generic matcher and
read-only capacity workflow accept it only with `is-userspace=yes` and complete
same-serial redacted ADB evidence proving Google / Pixel 9a / `tegu` /
`GXQ96` / `MP1.0`. They accept `N/A` only with the same ADB proof. Live
Fastboot product, revision, serial, slot, slot count, unlock state, and mode
are still rechecked; observed variant and verified SKU stay distinct. The
read-only Fastbootd partition inventory uses the same correlation and reports
`inventory_completeness=partial` because it cannot inspect guest `/proc`.
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

When stock Android ADB or Fastbootd is available, the LLM may place the exact
target into ordinary bootloader Fastboot through the checked-in workflow:

```sh
just pixel-9a-enter-fastboot --preflight
just pixel-9a-enter-fastboot --confirm-enter
```

The Android branch verifies the exact Google Pixel 9a / `tegu` / `GXQ96` /
`MP1.0` identity before `adb reboot bootloader` and requires the typed phrase
`CONFIRM PIXEL 9A ENTER FASTBOOT`. The Fastbootd branch requires complete
same-serial Android identity evidence, checks live product/revision/slot and
`is-userspace=yes`, and requires `CONFIRM PIXEL 9A EXIT FASTBOOTD` before
`fastboot reboot bootloader`. Both branches verify the resulting serial,
bootloader mode, and slot, and never change storage, slots, bootloader state,
or user data. This device may report `variant=N/A` from bootloader Fastboot;
the dev workflow treats that field as unavailable only after live or recorded
same-serial ADB identity was checked and product, hardware revision, serial,
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
- S5300 cellular route decision for issue #1 (Kleaf CPIF bridge vs radio-first
  WWAN/SIT; staged gates, not runtime evidence):
  [`s5300-route-decision.md`](s5300-route-decision.md)
- S5300 Kleaf CPIF userspace adapter for issue #1 (hash-pinned `exynos-modem`
  patches behind an explicit `--kernel-backend google-kleaf-cpif`; offline
  tested, not selected by any init script, not runtime evidence):
  [`s5300-cpif-adapter.md`](s5300-cpif-adapter.md)
- S5300 daemon scratch directory for issue #81 (hash-pinned `exynos-modem`
  patch moving NV sinks and crash records from `/tmp` to a private
  `/run/exynos-modem`; offline tested; device backing and permissions
  unverified; not runtime evidence):
  [`s5300-scratch-dir.md`](s5300-scratch-dir.md)

The target-local postmarketOS `device-google-tegu` package and the typed
Ferrix adapter composition are now checked in under `pmos/device-google-tegu/`
and `adapter/`. They are native-Linux implementation inputs, not hardware
evidence: the capability matrix remains conservatively `unsupported` until
this exact MP1.0 device produces subsystem evidence. In particular, the
S5300 CPIF modem is not a standard ModemManager transport. The target package
carries immutable native-Linux `exynos-modem`/`sit-proxy` and SIT ModemManager
recipes behind the public ModemManager D-Bus boundary. Their integrated init
scripts require the separately pinned radio-first `s5300_modem` WWAN kernel
backend, not Google's stock `cpif` character-device surface. The packaged
binaries also carry an explicit Kleaf CPIF backend that nothing selects yet
([`s5300-cpif-adapter.md`](s5300-cpif-adapter.md)). The native Linux radio-first kernel and
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
Issue #55 adds one optional `audio-aoc` row: `vendor.img:/firmware/aoc.bin`
to `lib/firmware/aoc.bin`. The fail-open `ferrix-aoc-firmware` OpenRC
one-shot names it in the AoC `firmware` control only while AoC reads
`Offline`, and only after re-checking the image against its manifest row. It
is ordered before the audio session, and nothing needs it. The stage
workflow refuses persist or `*calib*` rows; per-unit calibration is never
staged. See [`audio-route-decision.md`](audio-route-decision.md). Audio stays
`unsupported`.
Issue #59 adds one optional `audio-tas25xx` row:
`vendor.img:/firmware/tismartpa_driver_tuning.bin` to
`lib/firmware/tismartpa_driver_tuning.bin`. That is the exact name the pinned
`snd-soc-tas25xx` driver requests. The source review classifies it as
generic, chip-revision-selected register and profile tuning, not per-unit
data. The driver loads it itself, so there is no loader service. The TAS
per-unit calibration paths (`smartamp_calib.bin`, `speaker.cal`) and the
Android HAL's `ti_sp_tuning.bin` are not staged. Neither is CS40L26 haptics
firmware. No image with this row has booted. TAS initialisation and an ALSA
card remain unproved.
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

## Development flash route

Owner policy (2026-10-09): the only hard requirement for this exact
development device is that it can always be restored to the pinned stock
factory image. `tools/pixel-9a/dev-flash.sh` (`just pixel-9a-dev-flash`)
therefore ignores Ferrix image-compatibility, candidate, and observation
evidence and enforces only:

- exactly one bootloader-mode Fastboot device (no ADB, not fastbootd) with
  product `tegu`, revision `MP1.0`, SKU `GXQ96`, and `unlocked=yes`;
- `recovery_procedure = tested` and the pinned factory archive present with
  its exact size and SHA-256;
- writes only to partitions the factory image rewrites: `boot`, `init_boot`,
  `vendor_boot`, `vendor_kernel_boot`, `dtbo`, `vbmeta`, `vbmeta_system`,
  `vbmeta_vendor` (either slot), `userdata`, `metadata`;
- no bootloader/radio writes except the pinned stock images, and no relock,
  erase, format, `oem`, logical, or device-unique (`persist`, `efs`,
  `devinfo`, modem calibration) partition writes;
- `set-active` first writes the pinned stock bootloader and radio to the
  target slot, because Pixel bootloaders are A/B and switching onto an older
  bootloader below the anti-rollback index can hard-brick the phone.

Every mutating mode without `--confirm` is a read-only plan that prints the
exact serial, slot, partition targets, image hashes, recovery artifact, and a
`confirm_token` bound to all of them. Agents may pass that token themselves.

```sh
just pixel-9a-dev-flash preflight
just pixel-9a-dev-flash flash boot_b=out/.../boot.img vendor_boot_b=out/.../vendor_boot.img
just pixel-9a-dev-flash flash --confirm DEVFLASH-<token> boot_b=... vendor_boot_b=...
just pixel-9a-dev-flash boot out/.../boot.img            # temporary, no write
just pixel-9a-dev-flash set-active b                     # plan, then --confirm
just pixel-9a-dev-flash reboot
```

Recovery from any state that reaches bootloader Fastboot:
`FERRIX_PIXEL9A_NO_USER_DATA=I-understand-this-is-a-new-development-device-with-no-personal-data just pixel-9a-factory-restore --confirm-restore`
(stock restore no longer depends on the Ferrix `image_compatibility` gate),
then `just pixel-9a-discover`. If the phone does not reach Fastboot by itself,
a human must hold Volume Down + Power; the bootloader partitions are never
touched by this route except with pinned stock images, so that entry remains
available. The workflow is offline stub-tested only and has not been run on
the phone.

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

## Read-only static guest inventory

After a separately approved static installation and slot-B activation, set
`FERRIX_PIXEL9A_STATIC_ACTIVATION_EVIDENCE` to the ignored activation
transcript and run `just pixel-9a-guest-evidence-inventory` when the pmOS
guest is reachable over SSH. The host verifies the pinned candidate, the
temporary-boot observation, both complete flash-stage records, and the
activation record before opening SSH. The guest probe checks the exact tegu
device-tree model and compatible string before reading its slot suffix,
OpenRC statuses, Ferrix sockets and pidfiles, DRM connector counts, and input
device counts. The normalized record stays under ignored
`out/pixel-9a/guest-evidence/` and carries the activation transcript hash.

This is an inventory, even when slot B, a connected DRM connector, a Wayland
socket, and a touch-named input device are present. It cannot prove that a
client is connected, a frame was rendered on the phone, a touch event was
delivered, or a service works. Inspect a real display capture and observe
physical touch before recording those capabilities. The SSH device-tree check
and prior activation record should be correlated with the owner's continuous
USB session; neither is a cryptographic same-device attestation. This
workflow is offline-tested only and has not run on Ferrix hardware.

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
carries intermediate findings. Telephony remains `unsupported` until the
selected WWAN/SIT route produces exact-device ModemManager, Ferrix adapter,
registration, data, SMS, and call evidence; modem enumeration alone is
insufficient. Wi-Fi is promoted only after full
NetworkManager connectivity, an associated IPv4 address, and successful typed
Ferrix network/core state replies plus per-file size/SHA-256 verification from
the guest `firmware-manifest.tsv`; Bluetooth requires a powered controller and
the corresponding `present`/`enabled` state from Ferrix core. This workflow is
stub-tested only
(`just pixel-9a-radios-test`); no radio has been observed working on the
physical device, and it never edits `capabilities.toml`.

## S5300 source partition inventory (issue #34)

`just pixel-9a-s5300-source-inventory` is a read-only check of the four
partitions that `ferrix-s5300-mounts` reads: the active-slot `modem_a` or
`modem_b`, plus `efs`, `persist`, and `modem_userdata`. It reaches the guest
over the CDC-ACM guest probe transport described below (guest stderr
discarded, 60 s and 64 KiB bounds). The guest probe
(`tools/pixel-9a/s5300-source-inventory-remote.sh`) first checks the exact tegu
device tree. It then selects the slot with the same rule as the service and
resolves each `/dev/disk/by-partlabel` link and kernel `PARTNAME` uevent. Next
it reads the superblock type with util-linux
`blkid -c /dev/null -p -o value -s TYPE`, using a non-interactive root, `doas -n`, or
`sudo -n` prefix. Finally it copies any `/proc/self/mountinfo` line for the
device or its `/mnt/vendor` destination. It never mounts, writes, or dumps
partition data, and it does not collect filesystem UUIDs or LABEL values or
device serials. The GPT partition names are recorded because they are the
exact inputs to the mount service.

For each source, the host reports the raw type and resolution. Resolution is
unique, missing, duplicate, device mismatch, or slot unresolved. Type is ext4,
f2fs, no signature, ambivalent, unsupported, or unknown. The host also checks
existing mounts against the service's `verify_mount` contract. A direct
partition mount violates that contract (`direct-partition-mount`), because
the service mounts only a read-only loop. A loop mount at the destination is
reported as `loop-backing-not-inventoried`, because this probe does not read
loop sysfs. The redacted
transcript, labelled `physical:google-pixel-9a-tegu-gxq96-mp1-0` (or `mock`
under `FERRIX_PIXEL9A_S5300_INVENTORY_EVIDENCE=mock`), is written to ignored
`out/pixel-9a/s5300-source-inventory/`. A clean physical run reports
`issue34_safety_gate=owner-review-required`, never passed. Whether ext4
`noload` and f2fs `norecovery` suppress every write on this kernel stays
`unknown`. Whether the kernel shows those flags in mountinfo is also unknown
until an observed mount shows them.

The workflow passed its offline fixtures
(`just pixel-9a-s5300-source-inventory-test`) and ran on the exact device
after a recovery-gated Ferrix boot. The physical preflight found all four
source roles uniquely resolved and unmounted, with no holders; `modem_b` is
ext4 and `efs`, `persist`, and `modem_userdata` are f2fs. It reported
`preflight_problems=none` and left the issue gate at `owner-review-required`.

## S5300 CPIF E1 inventory (issue #1)

`just pixel-9a-s5300-cpif-inventory` runs gate E1 of
[`s5300-route-decision.md`](s5300-route-decision.md) on a booted Ferrix dev
guest. It is read-only and runs over the CDC-ACM guest probe transport (guest
stderr discarded, 60 s and 64 KiB bounds). The guest probe
(`tools/pixel-9a/s5300-cpif-inventory-remote.sh`) first checks the exact tegu
device tree; QEMU and other boards exit 4. It then reads:

- the `/proc/modules` lines for `cpif`, `shm_ipc`, and `cpif_page`, and
  whether `/sys/module` has a built-in copy;
- every platform device whose `of_node/compatible` contains
  `samsung,exynos-cp`, its `driver` link name, and its `modem_state`
  attribute;
- the inode type, symlink flag, `stat(2)` device number, mode, and numeric
  owner of `/dev/umts_boot0`, `umts_ipc0`, `umts_ipc1`, `umts_rfs0`, and
  `oem_ipc0` to `oem_ipc7`, plus each node's `/sys/class/*/<node>` entry,
  its `dev` attribute, and its `subsystem` link;
- the number of `oem_ipc*` names, `/sys/class/wwan` entries, and
  `/sys/class/net/rmnet*` netdevs.

The probe never opens a CPIF character device. Opening `umts_boot0` or
`umts_ipc*` runs the link's `init_comm` hook. The probe touches `/dev` paths
only through shell file tests, `stat`, and a directory glob. It reads no
dmesg, node contents, serials, or SIM/IMEI data. It starts no service and
never boots, powers, or resets the CP. The offline test enforces this
statically. It also runs the real probe against a fixture tree whose nodes
are FIFOs, which would block if anything opened them.

The host refuses malformed, unknown, duplicate, or missing fields, more than
one `/proc/modules` line per module, and a `modem_state` reported without a
single platform device (exit 5). It then reports `e1_result`:

- **`go`** means all of the following were observed: all three modules are
  `Live` loadable modules; exactly one `samsung,exynos-cp` platform device
  has a bound driver; `modem_state` is exactly `OFFLINE`; each of the twelve
  nodes is a character device (not a symlink) with exactly one sysfs class
  entry whose `dev` equals the node's device number and whose subsystem is
  that class; all twelve share one class; and no `wwan` device exists. The
  `rmnet*` count is recorded but never decides the result. The observed
  class is recorded with `node_class_matches_source_claim`, which compares it
  against the source-claimed `cpif` class for information only.
- **`no-go`** means an observed fact contradicts one of these. Examples are a
  missing, built-in, or transitional module; a missing, unbound, or
  ambiguous platform device; `ONLINE`, a booting, crash, or unrecognised
  `modem_state`, or no `modem_state` attribute; a missing node, symlink,
  non-char node, or class/device mismatch; mixed classes; or a WWAN surface.
- **`incomplete`** means a fact could not be read (`/proc/modules`, the
  platform directory, `modem_state`, or `stat`). Nothing in it counts as
  pass.

`e1_gate` repeats the result only for physical runs. Mock runs report
`not-evidence-mock`. Transcripts are labelled
`physical:google-pixel-9a-tegu-gxq96-mp1-0` (or `mock` under
`FERRIX_PIXEL9A_S5300_CPIF_EVIDENCE=mock`). They state `cp_nodes_opened=no`
and are written to ignored `out/pixel-9a/s5300-cpif-inventory/`. Every run
reports `capability_change=none`.

E1 `go` does not show that the CP boots or that opening a node is safe. E0
defined the source-level boot-control ABI; runtime firmware compatibility,
NV file sizes, FMT payload equivalence, RFS service, `rmnet*` after
`ONLINE`, CP crash handling and driver/firmware skew remain unknown. The run
lists these as `e1_unknowns`. E1 is separate from the owner-accepted issue
#34 mount gate; adapter review and read-only NV checks also precede a CP
boot trial. The workflow passes its offline fixtures
(`just pixel-9a-s5300-cpif-inventory-test`). On the exact physical device,
the first read-only run returned `e1_result=go`: three Live modules, one
bound `cp_interface` platform device, `OFFLINE`, all twelve `cpif` class
character nodes with matching device numbers, no WWAN class device, and 30
`rmnet*` netdevs before CP boot. The probe opened no CPIF node; runtime
modem behavior remains untested.

## S5300 CPIF service integration (issue #88)

[`s5300-cpif-integration.md`](s5300-cpif-integration.md) records the
offline-tested OpenRC/package wiring of `google-kleaf-cpif`. Each backend has
an explicit `exynos-modem`/`sit-proxy` CLI. No S5300 service is in any
runlevel, and ModemManager only orders after them. On CPIF, `rc-service
exynos-modem preflight` is read-only and opens no CPIF node; `provision`
creates only `/run/exynos-modem`; and a manual start needs a one-shot `arm`
on tmpfs `/run`. `just pixel-9a-s5300-cpif-services-test` runs the offline
contract. Nothing has run on the device, and no CP boot is authorised by it.

## S5300 source mount proof (issue #34)

This section records source review, not physical evidence. Line numbers refer
to the pinned GKI source (`kernel/common` `a4662a84a15f`, Linux 6.1.124). The
mount service's options alone do not stop every write:

- ext4 `noload` (alias `norecovery`) skips loading the journal, and ext4
  refuses `noload` without `ro` when recovery is needed. Orphan cleanup
  (`fs/ext4/orphan.c` `ext4_orphan_cleanup`) still clears `SB_RDONLY` for a
  while on a read-only mount. With the journal not loaded, those writes go
  straight to the device. Error handling (`ext4_handle_error`) also commits
  the superblock on a read-only mount.
- f2fs `norecovery` requires `ro` and skips roll-forward. Orphan inode
  recovery (`f2fs_recover_orphan_inodes`) still runs on a read-only mount.
  `f2fs_handle_error` and `f2fs_handle_stop` also commit the superblock
  unless the block device itself is read-only.
- Each of those paths is skipped only when `bdev_read_only()` of the mounted
  device is true (`fs/ext4/orphan.c:398`, `fs/ext4/super.c:667,1021`,
  `fs/f2fs/f2fs.h:4486` `f2fs_hw_is_readonly`).
- `BLKROSET` (`blockdev --setro`) on the partition makes `bdev_read_only()`
  true, but it is not a write barrier. The block layer only warns
  (`block/blk-core.c:498-507` `bio_check_ro`: "Trying to write to read-only
  block-device") and then submits the bio. A filesystem bug or a path that
  skips the check would still reach the partition.

A loop device attached read-only is a kernel write barrier for I/O sent
through it (`drivers/block/loop.c`, `CONFIG_BLK_DEV_LOOP=y` in the GKI
`gki_defconfig`; the Ferrix fragment does not change it):

- `loop_configure()` sets `LO_FLAGS_READ_ONLY` when asked (losetup
  `--read-only`), and also whenever the backing file is not open for writing
  (`:1048-1050`). It then calls `set_disk_ro()` with that flag (`:1066`). So
  `bdev_read_only()` of the loop is true, and ext4 and f2fs mounted on it take
  their read-only-device branches. A read-only loop never enables the queue's
  write cache (`:1074`), so empty flushes complete in `submit_bio_noacct`
  without reaching the driver (`block/blk-core.c:754-760`).
- `loop_handle_cmd()` fails every request with `op_is_write()` with `-EIO`
  while `LO_FLAGS_READ_ONLY` is set, before `do_req_filebacked()` runs
  (`:1889-1891`). That covers `REQ_OP_WRITE`, `REQ_OP_DISCARD`, and
  `REQ_OP_WRITE_ZEROES` (`include/linux/blk_types.h:370-378,490-492`). This
  check matters because the loop's discard and write-zeroes path calls the
  backing `fallocate` directly (`:304-322`), and the 6.1 block-device
  `blkdev_fallocate` does not check `FMODE_WRITE` (`block/fops.c:625`).
- The backing writes are also blocked in a second way. Writes would go
  through `vfs_iter_write()`, which fails with `-EBADF` without `FMODE_WRITE`
  (`fs/read_write.c:848`). Block-device `write_iter` fails with `-EPERM` on a
  read-only partition (`block/fops.c:534`). The first holds only if losetup
  opened the backing file read-only. util-linux does that for `--read-only`
  (2.40.2 `lib/loopdev.c` `loopcxt_setup_device`), but the guest's
  util-linux version is not pinned and the kernel does not expose the open
  mode. The second holds because `BLKROSET`
  is set first.
- `LOOP_SET_STATUS` cannot clear the flag (`:1288-1293`;
  `LOOP_SET_STATUS_SETTABLE_FLAGS` is only `AUTOCLEAR|PARTSCAN`).
  `LOOP_CHANGE_FD` needs it set (`:585`). The loop has no
  `set_read_only` operation, and `BLKROSET` 0 on the loop clears only
  `bd_read_only`, not the disk-level flag (`block/ioctl.c:361-378`,
  `include/linux/blkdev.h:787-796`).
- Detaching never clears the disk-level flag (`__loop_clr_fd`, `:1131-1212`).
  An unbound loop can therefore still read `ro=1` from an earlier read-only
  use. The proof uses only an unbound loop whose `ro` reads 0, so `ro=1` after
  attaching can only come from the read-only configuration.
- Rejected requests are still counted in the loop's `stat` (they complete
  with an error; `block/blk-mq.c:977-995`) and trigger `bio_check_ro`'s
  warning for the loop. That makes the loop counters a detector of attempted
  writes. The partition counters detect writes that reached it.

The barrier covers I/O through the loop only. It does not stop other writers
of the same sectors: a second opener of the partition node through a
bio-level path, the whole-disk node (`BLKROSET` on the partition does not
cover `/dev/sda`), device-mapper or md, SCSI/UFS passthrough (SG_IO, BSG,
RPMB), or the UFS device itself. The proof excludes them only by checks that
nothing holds or mounts the partition, that the service and modem are in no
runlevel, and that it runs on a dev boot. It does not exclude them by kernel
enforcement. Partition `stat` counters count non-passthrough requests started
while the queue's `iostats` was on, against the partition the bio named.
Unchanged counters are consistent with no write; they do not prove it. Not
reviewed: Android vendor hooks or GKI modules, and whether the running
`vmlinux` was built exactly from `a4662a84a15f` (matched by release string
only).

`just pixel-9a-s5300-mount-proof` (`tools/pixel-9a/s5300-mount-proof.sh`)
uses the same CDC-ACM transport. With no argument it is a read-only
preflight. It checks:

- the exact device tree and the pinned kernel release;
- that `ferrix-s5300-mounts` and `exynos-modem` are in no runlevel;
- the SHA-256 of the installed `/etc/init.d/ferrix-s5300-mounts`;
- that nothing is mounted under `/mnt/vendor`;
- that util-linux `losetup` and an unbound loop device with `ro` 0 exist;
- for each role: unique partition resolution, the util-linux `blkid -p`
  type against the physical inventory (`modem_<slot>` ext4; `efs`,
  `persist`, `modem_userdata` f2fs), the `ro` flag, queue `iostats`,
  holders, mounts, and `stat`.

The preflight mounts nothing and changes no state.

`just pixel-9a-s5300-mount-proof mount ROLE` is a separate run for one role.
It needs a typed phrase, `FERRIX_PIXEL9A_S5300_MOUNT_PROOF_CONFIRM='mount ROLE
read-only on google-pixel-9a-tegu-gxq96-mp1-0'`. For the per-unit
partitions that the factory image does not rewrite (`efs`, `persist`,
`modem_userdata`), it also needs the recorded owner decision,
`FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION=owner-approved-issue34-per-unit-ro-mount`.
The run then:

1. Runs a fresh preflight and prints the bound partition.
2. Opens a second session, which refuses unless the same `major:minor`,
   name, and type are still unique and unmounted.
3. Writes a marker to the kernel log and applies `blockdev --setro`, then
   checks `/sys/class/block/<dev>/ro`.
4. Picks an unbound loop whose `ro` reads 0, re-checks it, and runs
   `losetup --read-only` onto it. It then checks from sysfs that the loop
   now reads `ro=1` and has major 7. It also checks that the
   loop's backing file is the bound partition node, that offset and
   sizelimit are 0, that the size equals the partition's, and that
   `blkid -p` on the loop reports the same type. Nothing is mounted
   otherwise.
5. Mounts the loop, never the partition, once at
   `/run/ferrix-s5300-mount-proof/ROLE` with the service's exact options. It
   reads the directory tree, waits, unmounts, and detaches the loop.

The host compares the write, discard, and flush counters of the partition
from before `BLKROSET` to after detach, and of the loop from before the mount
to after unmount. It checks that the mountinfo line meets `verify_mount` on
the loop's `major:minor` and scans kernel log lines after the marker. Any of
these stops the run:

- an unverified barrier;
- a partition counter change (a write reached it);
- a loop counter change or a read-only-loop warning (a write attempt that the
  loop failed, which still needs review);
- a read-only-partition warning, or an I/O or filesystem error;
- orphan-cleanup or recovery messages;
- the loop left bound.

The partition's `ro` flag stays set until the next boot.

`ferrix-s5300-mounts` now mounts each source through the same read-only
loop, so a proof run covers the path the service itself uses. For each
source, after the slot, label, and `blkid -p` checks, the service:

1. refuses if the partition is already mounted directly anywhere, if
   `losetup` is not util-linux, or if no unbound loop whose `ro` reads 0
   exists. Nothing has changed state at this point;
2. applies `blockdev --setro` to the partition and requires its sysfs `ro`
   to read 1;
3. re-checks the chosen loop and runs `losetup --read-only` onto it. If
   another process took the loop first, losetup fails and that binding is
   left alone;
4. runs `verify_loop`. This requires the loop to be bound and read `ro=1`,
   with major 7, the partition node as its backing file, offset and
   sizelimit 0, the partition's size, the partition still `ro=1`, and the
   probed type from `blkid -p` on the loop. If any check fails, the service
   detaches the loop it attached;
5. mounts only the loop, with `ro,noload` (ext4) or `ro,norecovery` (f2fs)
   and `nosuid,nodev,noexec`;
6. runs `verify_mount`. The mountinfo line must name that loop's
   `major:minor` and node, with root `/`, the probed type, `ro` on the mount
   and the superblock, and the recovery flag. `verify_loop` must still pass,
   and the partition must not be mounted directly. An unconfirmed mount is
   unmounted (never forced), and its loop is detached only after the
   unmount succeeds.

An existing mount at a destination is accepted only if it passes the same
`verify_mount`, so a direct partition mount left by an older service is
refused. `stop` first verifies every mounted destination the same way and
refuses, before any unmount or detach, anything that is not exactly the
read-only loop over that destination's source. It then unmounts each
destination without forcing and detaches that loop only once nothing mounts
it. It never clears the partitions' `ro` flag. The device package now also
depends on `util-linux-misc` (which ships `/sbin/blockdev` on Alpine) and
`losetup`.

Known limits of the service change:

- Detaching does not clear a loop's `ro=1`, so every start and stop cycle
  uses up four loops whose `ro` reads 0 until the next reboot. With the GKI
  default of 16 loop devices, roughly four cycles leave none, and `start`
  then refuses. A start that fails partway leaves the sources it already
  mounted in place, still verified. OpenRC then keeps the modem services
  stopped.
- The barrier scope is unchanged. It does not cover the whole-disk node,
  passthrough, or another opener of the partition.
- The util-linux version in the guest is not pinned. The service checks
  that `losetup` is util-linux, but cannot observe the backing open mode.
- Nothing here has run on Linux or on the device. The service is covered
  only by the offline stub harness, `just pixel-9a-s5300-mounts-test`
  (`tools/pixel-9a/tests/s5300-mounts-service.sh`). That harness models the
  sysfs state that it expects the kernel to show.

The service stays out of every runlevel on the dev image
(`tools/pixel-9a/dev-compose.py` `RADIO_ROUTE_SERVICES`), and no CP boot is
attempted.

The workflow's gate now follows the actual service contract. On the host it
statically checks that the checked-in service has exactly one mount call and
that this call mounts the attached loop. It also checks for
`blockdev --setro` and `losetup --read-only`, that both new and existing
mounts go through `verify_loop`, that the package depends on `blkid`,
`util-linux-misc`, and `losetup` (and not on a `blockdev` package), and that the APKBUILD `sha512` matches the file.
The guest preflight reports the SHA-256 of its installed
`/etc/init.d/ferrix-s5300-mounts`, and the host compares it with the
checked-in file. If the static check fails, the result is
`issue34_gate_blocker=service-mounts-without-read-only-loop`. If the guest
has no service or a different one, it is
`installed-service-not-the-checked-in-service`. Either one keeps the gate
`blocked`. With both checks met, the best result is
`mount_observation=no-write-observed` with
`issue34_safety_gate=owner-review-required`. No run reports a passed gate,
and the static check does not prove the service correct. Transcripts go to
ignored `out/pixel-9a/s5300-mount-proof/`. The workflow passed its offline
fixtures (`just pixel-9a-s5300-mount-proof-test`). On the exact phone, one
gated proof of each source role completed after the read-only preflight.
The three per-unit proofs had explicit owner approval. Each set the source
read-only, used a verified read-only whole-partition loop, mounted with the
checked-in recovery-suppressed options, read a directory-entry count, then
unmounted and detached without a reported write anomaly or leftover mount.
The owner explicitly accepted the issue #34 source-mount safety gate on
2026-10-12 after reviewing these four results. The workflow still reports
`owner-review-required` by design; the acceptance is a separately recorded
human decision, not a tool-generated pass. The loop barrier does not cover
a separate opener, whole-disk I/O, or UFS passthrough;
the observed stable counters alone cannot establish that none occurred.

## S5300 NV metadata preflight (issue #79)

E0 ([`s5300-cpif-e0.md`](s5300-cpif-e0.md), row T1) found that the external
boot path streams `nv_normal.bin` and `nv_protected.bin` from the root of the
per-unit `efs` partition, and that the CP3A TOC sizes both sections at
`0x80000` bytes. Before any manual CP boot trial,
`just pixel-9a-s5300-nv-metadata` (`tools/pixel-9a/s5300-nv-metadata.sh`)
reports whether those two files exist, are regular files, and are at least
that size. It reports nothing else. It runs on an already booted exact dev
guest over the CDC-ACM bring-up shell. It is a separate workflow from CP boot
and never starts `ferrix-s5300-mounts`, `exynos-modem`, or anything that opens
a modem node.

Before the port is opened, both modes refuse unless three files match the
SHA-256 pinned in the host script:

- the guest body `s5300-nv-metadata-remote.sh`;
- the issue #34 probe `s5300-mount-proof-remote.sh`, whose barrier the guest
  body copies;
- the checked-in `ferrix-s5300-mounts.initd`, which is the service accepted
  for the #34 gate. The APKBUILD `sha512` must also match it.

Changing any of these needs a new review and a new pin.

With no argument, the run is a read-only preflight of `efs` only. It checks
the #34 global state: the exact device tree, the pinned kernel, the service
and `exynos-modem` out of every runlevel, util-linux `blkid`/`losetup`,
`blockdev`, and an unbound loop whose `ro` reads 0. It adds three checks:
`stat -c` and `timeout` are usable, the installed service is the accepted
file, and nothing is mounted under `/mnt/vendor`,
`/run/ferrix-s5300-nv-metadata`, or the #34 proof root. For `efs` it checks
unique partition resolution, f2fs from `blkid -p`, holders, existing mounts,
iostats, and counters.

`just pixel-9a-s5300-nv-metadata mount` is gated per run by
`FERRIX_PIXEL9A_S5300_NV_METADATA_CONFIRM='stat efs NV metadata through one
read-only loop mount on google-pixel-9a-tegu-gxq96-mp1-0'` plus the recorded
per-unit decision,
`FERRIX_PIXEL9A_S5300_PER_UNIT_DECISION=owner-approved-issue34-per-unit-ro-mount`.
The #34 mount phrase is not accepted. The run then:

1. Runs a fresh preflight, which must report no problem, and prints the bound
   partition.
2. Opens a second session, which refuses before any state change unless
   `efs` is the same unique, unmounted, unheld f2fs partition and the
   installed service still has the accepted hash.
3. Applies the #34 contract unchanged. The offline test fails if any of these
   blocks differs from the #34 probe: partition resolution, loop selection,
   `BLKROSET`, `losetup --read-only`, loop verification, the single mount of
   the loop with `ro,norecovery,nosuid,nodev,noexec`, cleanup, and teardown
   exit.
4. While mounted, and only if mountinfo shows the destination as this
   session's loop mount, looks up only the two allowlisted names. For each it
   reports `regular`, `absent`, `symlink`, `directory`, or `other`. For a
   regular file it adds the size from `stat -c %s`, which uses `lstat`. The
   symlink test comes first, so a link is never followed. Nothing on the
   filesystem is opened, read, hashed, walked, or listed.
5. Unmounts, detaches, and reports a post-check: the service hash and no
   mount left under either proof root or `/mnt/vendor`.

The host applies the #34 barrier, counter, mountinfo, and kernel-log checks,
then classifies the NV files last. Exit 7 (teardown not clean) and exit 8 (an
anomaly or a dirty post-check) take precedence over exit 9 (a file that is
absent, not regular, has no size, or is under `0x80000`), and all three
results are always reported. Exit 0 needs both files to meet the minimum with
a clean observation. Mock runs carry `evidence=mock`. Transcripts are mode
0600 under ignored `out/pixel-9a/s5300-nv-metadata/`. Raw guest output is
kept only after every key and value passes the allowlist, so no other file
name, UUID, label, link target, or content is retained.

Limits:

- A size of at least `0x80000` does not show that the CP firmware accepts
  the file. The CP boot remains a separate manual S2 trial.
- The barrier scope is the same as #34. It covers I/O through the loop only.
  Each run uses up one loop whose `ro` reads 0 until reboot, and the `efs`
  partition stays `ro` until reboot.
- `persist` `modem/cpsha` and the device-tree identity values that feed the
  handover block are deferred. Checking `cpsha` would add a second per-unit
  partition, `persist`, to the mount scope, which needs its own reviewed
  action and gate. The device-tree values are per-unit identity data whose
  exact property names are not yet pinned from checked-in sources. Neither is
  read here.
- The guest's `stat` and `timeout` implementations are not pinned. Busybox
  and coreutils both `lstat` for `stat -c %s` without `-L`.
- Covered offline by `just pixel-9a-s5300-nv-metadata-test`
  (`tools/pixel-9a/tests/s5300-nv-metadata.sh`). The test runs the real guest
  body against stubbed sysfs, loop, mount, and stat, and runs the host through
  the real CDC-ACM transport against a fake guest. This is offline evidence.

Physical result, 2026-10-12, exact `google-pixel-9a-tegu-gxq96-mp1-0`:
the read-only preflight passed on the still-booted `674157b` image and pinned
6.1.124 kernel. The accepted mount service matched, `efs` resolved uniquely
to an unmounted, unheld f2fs partition already block-read-only, and both
modem services were absent from runlevels. The owner then approved one new
per-unit metadata mount. That single run verified the read-only loop and
`ro,norecovery,nosuid,nodev,noexec` mount contract. Both allowlisted names
were regular files of exactly 524288 bytes (`0x80000`) and met the TOC
minimum. It unmounted and detached cleanly; post-check and write-counter
checks reported `anomalies=none` and `mount_observation=no-write-observed`.
The ignored transcript is under `out/pixel-9a/s5300-nv-metadata/`. No NV
contents were opened or retained, and no CP node or service was started.
This closes the metadata-size question only. The loop barrier cannot rule
out other writers, and file size does not prove CP firmware acceptance.

## Audio route

Audio stays `unsupported`. [`audio-route-decision.md`](audio-route-decision.md)
records the source review and exact-device physical inventories and temporary
UCM/route probe. It states what the checked-in kernel, firmware, service, and
adapter inputs prove and what they do not. It
assigns the remaining work to target layers (Kleaf modules, firmware policy,
observed-only UCM). It also sets out the read-only exact-device inventory and
its pass/fail criteria. That inventory runs only after the issue #25 restore
and recovery proof.

The device package ships a discovery-only ALSA UCM profile for the AoC card
(issue #2). `ucm-aoc-snd-card.conf` installs as
`/usr/share/alsa/ucm2/conf.d/aoc-snd-card/aoc-snd-card.conf`.
`ucm-tegu-HiFi.conf` installs as `/usr/share/alsa/ucm2/Google/tegu/HiFi.conf`.
`dev-compose.py` stages both at the same paths, and the package now depends on
`alsa-ucm-conf`. The `HiFi` use case is defined only when the device-tree model
is the exact tegu string and the `TAS25XX ASI Switch` control exists. It names
a `Speaker` on PCM 0 (`EP1 playback`) and a `Mic` on PCM 8 (`EP1 capture`). It
writes no mixer control, has no enable/disable/boot sequence, and binds no
hardware volume, so no speaker or microphone route is enabled at session start.
The front ends are DPCM links, so no sink or source is exposed without the
observed back-end mixer routes. The default console UI audio provider also
installed a PulseAudio WirePlumber fragment that disabled hardware audio
monitoring; the target pmbootstrap configuration now selects the PipeWire
provider and `rootfs.sh` rejects that conflicting package. A temporary copy
of the UCM files parsed on the exact device, and a controlled, silent route
trial exposed PipeWire nodes; see the decision record. A fresh image carrying
this package change has not booted, and no playback or capture was proven.

## Read-only audio inventory

`just pixel-9a-audio-inventory` (`tools/pixel-9a/audio-inventory.sh
inventory`) runs the checked-in `tools/pixel-9a/audio-inventory-remote.sh`
probe once over the CDC-ACM guest probe transport described below (guest
stderr merged, 180 s and 1 MiB bounds).
The probe's first action is the exact device-tree gate: model
`ZUMA PRO TEGU MP on ZUMA PRO` and compatible line `google,ZUMA PRO TEGU`.
QEMU exits 5 and any other board exits 4. The host then checks the
identity that comes back before it parses anything. Exit 3 means the
transport failed, and probe output without the completion marker fails. The
probe records:

- ALSA cards, PCMs and `/dev/snd` nodes;
- audio-named modules, device-tree bindings (bound/unbound), deferred devices,
  and `dmesg` probe and firmware-load failures;
- audio-named firmware files;
- the AoC DSP (probe version 2, issue #55): device presence, a classified
  `revision` (`offline`/`online`/`unreadable`/`absent`; the raw text is never
  recorded), `firmware`-control presence (never opened), the staged
  `lib/firmware/aoc.bin`, its manifest row count, and the
  `ferrix-aoc-firmware` service state, as the `aoc-dsp` TSV row;
- UCM data and `alsaucm listcards`;
- whether ALSA and PipeWire tools are installed;
- the PipeWire/WirePlumber graph (`wpctl status`, the adapter's
  `wpctl list audio sinks`, and default sink/source inspection);
- the Ferrix audio backend, the `ferrix-pipewire`/`ferrix-audio`/
  `ferrix-core` service states, and the `/run/ferrix/core.sock` audio state.

A missing command or interface is recorded as `unavailable`, `absent` or
`unknown`, never as success. Playback, capture and mixer tools are only
checked for presence. Nothing opens a PCM, changes a mixer, route or default
sink, loads a module, mounts, writes sysfs, or starts a service.

The redacted transcript and `audio-evidence-latest.tsv` go under ignored
`out/pixel-9a/audio/` and are labelled
`physical: google-pixel-9a-tegu-gxq96-mp1-0`. Redaction reuses the radios
redactor and also covers underscore-form Bluetooth MACs, PipeWire/Pulse
cookies and host names. Every row stays `unsupported`. Enumeration is not
speaker playback, microphone capture, or Ferrix route-change evidence, and
the workflow never edits `capabilities.toml`.

**Physical execution is gated by issue #25.** Do not run it against the phone
until the stock restore and recovery proof are re-established. So far it is
stub-tested only (`just pixel-9a-audio-inventory-test`, labelled mock), and no
audio state has been read from the physical device.

## Microphone capture proof (issue #2)

`just pixel-9a-mic-capture-proof [preflight|capture|restore]`
(`tools/pixel-9a/mic-capture-proof.sh`) is the gated first active audio test.
It uses the CDC-ACM transport below, with a two-line generated header before
the exact `mic-capture-proof-remote.sh` body. `preflight` is read-only and
prints the per-run `FERRIX_PIXEL9A_MIC_CAPTURE_CONFIRM` phrase. That phrase
is bound to the observed card driver, PCM 8 identity, mixer numid, and
kernel. PCM 8 is identified from its one `00-08` line in `/proc/asound/pcm`,
cross-checked with `arecord -l`. This kernel has no per-PCM procfs, so the
state is `not-exposed` and the gate is zero actual openers. `capture` sets only `EP1 TX Mixer INTERNAL_MIC_TX` on, records at
most 2 s of raw S16_LE 48 kHz stereo from `hw:0,8` to
`/run/ferrix-mic-capture-proof/`, restores the route, measures the PCM, and
deletes the file. `restore` (`FERRIX_PIXEL9A_MIC_RESTORE_CONFIRM`) puts that
route back to `off` after a lost session. Exit codes, classification, and
limits are in [`audio-route-decision.md`](audio-route-decision.md#microphone-capture-proof-workflow-issue-2-physical-pcm-capture).
Transcripts hold aggregates only, under ignored
`out/pixel-9a/mic-capture-proof/`. The workflow is offline-tested (`just pixel-9a-mic-capture-proof-test`).
The revised read-only preflight passed on the phone. One owner-approved 2 s
capture returned 384000 PCM bytes: channel 0 varied at very low level and
channel 1 was digital silence. This does not establish acoustic pickup. The
temporary recording was deleted and the mic route restored. No speaker test
exists. Issue #83's source and effective-build review is **NO-GO** for a
speaker trial. Neither the built nor the stock TAS module compiles the
`algo/` protection or calibration code. Stock HAL strings suggest it
configures AoC speaker protection, but the firmware behaviour is unknown
and Linux does not run that HAL. See
[`audio-route-decision.md`](audio-route-decision.md#speaker-protection-source-and-effective-build-decision-issue-83).

### Controlled-tone acoustic proof (issue #80)

`just pixel-9a-mic-acoustic-proof` runs `mic-capture-proof.sh acoustic`. It
uses the same preflight, binding, guest checks, single mic-route write,
bounded 2 s `hw:0,8` capture, cleanup, and `restore`. It has its own typed
phrase (`FERRIX_PIXEL9A_MIC_ACOUSTIC_CONFIRM`, printed by a ready preflight).
During the capture session the host plays a generated, gated 1180 Hz
reference tone on its own speaker: −20 dBFS, 200 ms on / 200 ms off,
`afplay`, `pw-play`, or `paplay`, at the existing device and volume. The
WAV lives under ignored `out/` only while it plays. Before deleting the
recording, the guest folds Goertzel amplitudes at the tone and at 1060/1280
Hz control bins over the gate. The host classifies each channel against the
tone-off baseline and the control bins: `acoustic-tone-detected` (exit 0),
`inconclusive-*` (8), or `capture-failed-*` (9). The phone never plays
anything, and no host volume or device is changed. The physical setup
(built-in speaker, modest volume, about 20 cm, quiet room) cannot be checked
from code and is documented with the thresholds and limits in
[`audio-route-decision.md`](audio-route-decision.md#controlled-tone-acoustic-proof-workflow-issue-80).
Offline tests: `just pixel-9a-mic-acoustic-proof-test`. It has not run on
the phone; a run needs a separate owner decision.

### Bluetooth A2DP output preflight (issue #91)

`just pixel-9a-a2dp-output-preflight`
(`tools/pixel-9a/a2dp-output-preflight.sh`) is a read-only preflight for
proving output on a Bluetooth A2DP sink instead of the NO-GO internal
speakers. It uses the CDC-ACM transport below and sends the exact
`a2dp-output-preflight-remote.sh` body. That body reads the following:

- BlueZ, with `bluetoothctl list/show/devices Paired/info`;
- the PipeWire graph, with `wpctl status/inspect`;
- package versions, from the apk database file;
- OpenRC service status;
- the two internal speaker-path controls, with `amixer cget`;
- open playback PCM fds.

It reports counts, classes, versions, and yes/no matches only: no address,
device name, node name, or serial. The result is `candidate-observed`
(exit 0) or `blocked` (exit 6) with every reason. Every run prints
`active_gate=closed`. No playback mode exists, because the on-image
PipeWire/WirePlumber explicit-target fallback semantics are unverified.
Checks, limits, and the remaining gate are in
[`audio-route-decision.md`](audio-route-decision.md#bluetooth-a2dp-output-preflight-issue-91).
Transcripts go to ignored `out/pixel-9a/a2dp-output-preflight/`. Offline
tests: `just pixel-9a-a2dp-output-preflight-test`. It has not run on the
phone.

## CDC-ACM guest probe transport (issue #52)

The Ferrix dev rootfs is built with `--no-sshd`, so the read-only inventory
workflows above reach the guest through the USB CDC-ACM bring-up shell
(`ttyGS1`; the host node defaults to `/dev/cu.usbmodemferrix_tegu3`, override
with `FERRIX_SHELL`). `tools/pixel-9a/acm-probe.py` is the only transport; it
uses the Python standard library and runs exactly one checked-in probe per
session:

- It sends one short POSIX bootstrap line. The bootstrap turns tty echo and
  NL-to-CRNL translation off, prints a READY marker, and reads exactly the
  announced number of base64 lines of the probe. It decodes them and pipes
  them to the guest `sh`, as `ssh ... 'sh -s'` did, but only if the guest-side
  SHA-256 equals the host hash of the exact file bytes. Otherwise nothing runs.
- Every frame marker carries a per-run 128-bit random nonce. The bootstrap
  assembles the marker at run time, so the echoed command line never contains
  it, and probe output cannot forge it. Only the bytes between BEGIN and
  `END:<status>` are returned; echo, prompts and console noise are dropped.
  The transport exits with the probe's status, so each probe keeps its own
  device-tree gate (exit 4/5).
- `--timeout` and `--max-bytes` bound each run. A timeout, an overrun, a
  silent port, a hash mismatch or a framing error exits 255 with one
  `ferrix-acm: ... reason=<token>` line, and no partial output is returned.
  The wrappers map that to exit 3. After a timeout or overrun the host sends
  Ctrl-C to stop its own probe and restores the saved tty settings.
- The port path and USB serial never appear in errors, transcripts or TSVs.
  Retained records name the transport `cdc-acm` and the probe SHA-256.
- Probes containing NUL bytes, linked probes and non-character-device ports
  are refused. The port is opened `O_NOCTTY`, held exclusive (`TIOCEXCL`)
  where the host supports it, and restored on exit.

Limits: the session needs an idle shell prompt on `ttyGS1`; do not run it
while `tools/pixel-9a/devtest/` helpers hold the port. The guest needs
`stty`, `head`, `base64`, `sha256sum` and `cut` (busybox provides them). The
interactive shell may record the bootstrap line, which holds only the nonce,
hash and line count, in its history. If `stty` is missing, CRLF output is
normalised to LF. The transport is offline-tested against a pty-bridged real
`/bin/sh` (`just pixel-9a-acm-probe-test`, labelled mock). It has not been
run on the physical device.

## Agent playbook

Any LLM agent provisioning this or another phone follows
`skills/mobile-device-provisioning/SKILL.md`, which orders the workflows
above, encodes the confirmation policy, and starts with the generic
`just device-discover` / `just device-match` front door.
