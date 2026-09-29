# Google Pixel 9a board-support proposal

Status: identity established; bootloader unlocked through the gated workflow;
official recovery artifacts verified; factory restore not tested.

Owner authorization: the 2026-09-28 requests authorize read-only Pixel 9a
discovery, proposal work, official stock-recovery downloads, target-local
native Linux implementation, and a conditional bootloader unlock after the
recovery gate passes. The gated unlock completed after exact-identity checks
and explicit per-run confirmations. The later native-Linux request authorizes
building the target and working toward physical boot/provisioning, but does
not waive the checked-in backup, compatibility, recovery, exact-identity, and
per-run confirmation gates. Factory flashing, erasing beyond the completed
unlock reset, formatting, changing slots, booting an untrusted image, and
relocking remain prohibited until their applicable gates pass.

This proposal is for one exact physical device. It must not generalize from
QEMU, mock state, another Pixel 9a hardware revision, or another Google model.
The target package cannot claim hardware support until the identity,
compatibility, backup, recovery, and evidence gates below are complete.

## Exact-target checklist

Every value must come from the attached device through the checked-in
read-only workflow or from an authoritative source cited here. Unique device
identifiers remain outside Git.

| Field | Value | Source / evidence | Status |
| --- | --- | --- | --- |
| Manufacturer | Google | ADB `ro.product.manufacturer` | passed |
| Manufacturer model number | GXQ96 | ADB `ro.boot.hardware.sku`; Google Pixel 9a specifications | passed |
| Marketing name | Pixel 9a (owner supplied) | owner request | partial |
| Codename / product | tegu | ADB `ro.product.device`, `ro.product.name`, and `ro.product.board` | passed |
| Hardware revision / SKU | MP1.0 / GXQ96 | ADB `ro.boot.hardware.revision` / `ro.boot.hardware.sku`; Google regulatory guide | passed |
| SoC / board | Tensor G4 platform; board `tegu` | Google Pixel 9a specifications + ADB board properties | partial |
| Android build | `google/tegu/tegu:15/BD4A.240925.111/12859276:user/release-keys` | ADB read-only properties | passed |
| Bootloader version and state | `tegu-16.0-12754673`; locked/green at discovery; Fastboot now `unlocked=yes` | ADB and post-unlock Fastboot read-only properties | passed |
| Supported boot route | Stock recovery full OTA (`sideload-ota.sh`); gated Fastboot unlock/factory archive (`factory-restore.sh`); gated Ferrix temporary boot and slot-explicit flash (`flash.sh`) | Google OTA/factory-image instructions; checked-in target workflows, stub-tested only | documented |
| A/B slots and current slot | A/B; `_a` at ADB discovery, `a` in Fastboot; slot count `2` | ADB `ro.boot.slot_suffix`; Fastboot `current-slot`/`slot-count` | passed |
| Dynamic partitions and super layout | dynamic partitions `true`; UFS boot device `13200000.ufs`; factory metadata lists split boot and dynamic-system images | ADB properties + offline factory inspection | partial |
| Complete partition inventory | ADB `/dev/block/by-name` map observed; A/B topology resolved from both `_a` and `_b` names; Android denied `/proc/partitions` and `/proc/cmdline` | `just pixel-9a-partition-inventory` at `2026-09-28T05:57:22Z`, read-only transcript under ignored `out/pixel-9a/` (serial-redacted) | partial |
| Kernel source and version | Google `android-gs-tegu-6.1-android16`, `android14-6.1` GKI; source pins recorded in target plan | Google Pixel-kernel guide + AOSP source | documented |
| Device-tree source | pinned `devices/google/tegu` + `devices/google/zumapro`; includes the MP-family DTBO | AOSP source + target Linux plan | documented |
| Driver sources | pinned Google sources for panel, Synaptics touch, BCM4383 Wi-Fi, Broadcom Bluetooth, S5300 CPIF, audio/AoC, BMS and SoC modules | AOSP kernel superproject + target Linux plan | documented |
| Firmware inventory and handling | Official Google factory archive only; no firmware committed | factory archive manifest and policy below | partial |
| postmarketOS device package basis | Target-local `pmos/device-google-tegu` package and native-Linux Ferrix adapter composition are checked in; substrate is not built or bootable | repository target package + target Linux plan | implementation-input |
| Stock factory image and OTA hashes | CP3A.260905.009 pinned; files downloaded and verified in ignored target folder | official Google image manifests | passed |
| Backup inventory | — | backup artifacts stored outside Git and verified readable | pending |
| Tested recovery procedure | Sideload and factory-restore workflows checked in and stub-tested (49 and 69 offline checks); neither run on the device; unlock reset completed | future exact-target restore evidence | pending |
| Image compatibility criteria | factory metadata requires `tegu`, bootloader `tegu-17.0-15819938`, baseband `g5300t-260604-260807-B-16036236`; AVB/slot/output-size checks remain required | pinned archive + offline factory inspection | partial |
| Capability matrix | all Ferrix capabilities conservative `unsupported` pending Linux evidence | target `capabilities.toml` | passed |
| Evidence and redaction policy | serials, IMEI/IMSI/ICCID, MAC/BSSID, keys, user data, and raw dumps never enter Git | this proposal + ignored output path | partial |
| Data-collection policy | selected identity/build properties only; no user-data pull or personal backup | this proposal | partial |

## Recovery and firmware policy

- Only official Google factory images and full OTA packages for the exact
  `tegu`/Pixel 9a target may be retained by the download workflow. The pinned
  public build is `CP3A.260905.009` (Android 17.0.0, September 2026).
- Recovery files are generated under the ignored repository-local target folder
  `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/recovery/`. The checked-in
  pins record URLs, filenames, and hashes; both the factory and full OTA
  archives were downloaded and verified locally. Firmware bytes do not enter
  the repository.
- The preferred non-destructive route is stock recovery plus `adb sideload`
  of a verified full OTA package. It requires the phone to remain on a
  compatible stock build and does not replace a verified backup.
- Fastboot factory restoration is a separate, destructive procedure. It may
  require an unlocked bootloader and normally erases user data. This proposal
  was not invoked by the unlock workflow. The checked-in unlock workflow
  requires the verified recovery artifacts, exact device identity, two typed
  per-run confirmations, and the phone's own confirmation before it issues
  only `adb reboot bootloader` and `fastboot flashing unlock`; it stops in
  Fastboot afterward. That unlock workflow completed successfully; no factory
  flash, slot change, or relock has been run.
- No `fastboot boot`, `fastboot flash`, `fastboot update`, `fastboot erase`,
  `fastboot set_active`, `fastboot reboot`, or `adb sideload` command has been
  run. Discovery/download/preflight are read-only; the gated unlock workflow
  is the sole state-changing exception and issued only `adb reboot bootloader`
  and `fastboot flashing unlock`.

## Evidence plan

Evidence must name the exact model, codename, and hardware revision while
redacting serial numbers and other unique identifiers. It must distinguish
`QEMU`, `mock`, and this exact physical target. The initial evidence is limited
to read-only identity and official recovery artifact verification. Hardware
capability claims require separate, target-specific tests and remain
`unsupported` or `unverified` until demonstrated.

## Discovery log

| Date (UTC) | Result |
| --- | --- |
| 2026-09-28 | Proposal package recorded after the exact-target read-only discovery; discovery had completed on 2026-09-27. |
| 2026-09-27 | ADB discovery established Google Pixel 9a / GXQ96 / tegu / MP1.0; bootloader locked, verified boot green, slot `_a`; serial redacted. |
| 2026-09-27 | Gated unlock completed after recovery-hash and Fastboot identity checks; post-unlock discovery reported `unlocked=yes`, slot `a`; no factory flash or slot change. |
| 2026-09-28 | Official CP3A.260905.009 factory and full OTA archives downloaded and SHA-256 verified under the ignored recovery folder; no device mutation performed. |
| 2026-09-28 | Offline factory inspection verified `tegu`, bootloader/baseband requirements, and the split boot/dynamic partition image inventory without extracting payloads or contacting the device. |
| 2026-09-28 | Read-only ADB discovery at `05:57:17Z` and partition inventory at `05:57:22Z` matched the exact target and recorded the live `/dev/block/by-name` map, current slot `_a`, and two-slot topology. Android permissions denied `/proc/partitions` and `/proc/cmdline`; the redacted transcripts remain ignored and are admission/topology evidence only. |
| 2026-09-28 | Native Linux plan pinned Google's published downstream 6.1 kernel/module baseline and recorded subsystem evidence gates; no Linux image was booted or flashed. |

The discovery command was `just pixel-9a-discover`. It queried one
unambiguous ADB transport, then later one Fastboot transport, and wrote
serial-redacted transcripts to `out/pixel-9a/`. Discovery itself did not
reboot or otherwise change the phone; the separate gated unlock workflow did
the documented reboot and unlock only.
