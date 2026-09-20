# Attached OnePlus board-support proposal

Status: discovery blocked — host tools installed, no USB transport enumerated
(re-checked read-only on 2026-09-21; see "Discovery log").

Owner authorization: read-only device discovery and proposal work approved on
2026-09-20. Destructive provisioning is gated by this proposal. Nothing in
this document generalizes from QEMU, from the mock profile, or from any other
OnePlus model or hardware revision; every row below is about one exact
device and revision that has not been identified yet.

## Exact-target checklist

Every row must be filled from read-only discovery or from cited sources before
the provisioning gate may pass. `pending` means no value has been established.

| Field | Value | Source / evidence | Status |
| --- | --- | --- | --- |
| Manufacturer | OnePlus (owner supplied) | owner statement | partial |
| Manufacturer model number (`ro.product.model` / `ro.product.vendor.model`) | — | `just oneplus-discover` | pending |
| Marketing name | — | owner + discovery | pending |
| Codename / product (`ro.product.device`, `ro.product.vendor.device`, fastboot `product`) | — | `just oneplus-discover` | pending |
| Hardware revision / SKU (`ro.boot.hardware.revision`, `ro.boot.hardware.sku`, fastboot `hw-revision`) | — | `just oneplus-discover` | pending |
| SoC (`ro.hardware`, `ro.hardware.chipname`, `ro.product.board`) | — | `just oneplus-discover` | pending |
| Android build (`ro.build.fingerprint`, `ro.build.version.release`, `ro.build.version.security_patch`) | — | `just oneplus-discover` | pending |
| Bootloader version and state (`ro.boot.bootloader`, `ro.boot.flash.locked`, `ro.boot.verifiedbootstate`, fastboot `unlocked`/`secure`) | — | `just oneplus-discover` | pending |
| Supported boot route (fastboot `boot` of a test image vs. flash; `is-userspace`) | — | discovery + vendor/pmOS documentation | pending |
| A/B slots and current slot (`ro.boot.slot_suffix`, fastboot `current-slot`, `slot-count`) | — | `just oneplus-discover` | pending |
| Dynamic partitions (`ro.boot.dynamic_partitions`) and super-partition layout | — | discovery (read-only) | pending |
| Complete partition inventory (names, sizes, slot semantics) | — | read-only listing to be added to the tooling after identity | pending |
| Kernel source and version | — | vendor/mainline citation | pending |
| Device tree source | — | citation | pending |
| Firmware inventory and handling policy (modem, Wi-Fi/BT, GPU, DSP) | — | citation + policy text | pending |
| Driver sources (display, touch, storage, modem, audio, sensors, charging) | — | citation | pending |
| postmarketOS device package basis (`device-oneplus-<codename>`, kernel package) | — | pmaports citation for the exact codename | pending |
| Stock image source and SHA-256 hashes | — | vendor download + recorded hashes (outside Git) | pending |
| Backup inventory (every partition that will be touched, plus persist/modemst/EFS) | — | stored outside the repository, verified readable | pending |
| Tested recovery procedure (restore from backup, return to stock, MSM/EDL availability) | — | dry run recorded outside Git | pending |
| Image compatibility criteria (boot image format, header version, DTB placement, cmdline, page size) | — | derived from the stock boot image | pending |
| Capability matrix (`ferrix-hardware-api`: supported / unsupported / mocked per capability) | — | filled only from device evidence | pending |
| Evidence and redaction policy (serials, IMEI, IMSI, ICCID, MAC/BSSID, keys never in Git) | tooling redacts serials in transcripts (`out/oneplus/`, ignored) | `tools/oneplus/discover.sh`, `tools/oneplus/tests/` | partial |
| Data-collection policy (what the device may send, what is stored, where) | — | policy text | pending |

No `targets/boards/<target-id>/` directory may be created until these values
identify one stable hardware target. Unique serial numbers stay out of Git.

## Physical validation matrix

Each row is evidence for the exact target and revision named above, gathered
only after the provisioning gate has passed. Nothing here may be inferred
from QEMU or mock runs.

| Area | What counts as evidence | Status |
| --- | --- | --- |
| Boot and recovery | boot route exercised, return to stock verified | not started |
| Display and touch | panel drives at native mode; touch coordinates verified | not started |
| Storage | rootfs on device storage, fsck clean across reboot | not started |
| Charging | charger detect, current, battery state via `ferrix-hardware-api` | not started |
| Wi-Fi discovery / association / suspend / recovery | scan, join, survive suspend, recover after AP loss | not started |
| Modem discovery and firmware | ModemManager enumerates the modem; firmware policy recorded | not started |
| SIM and registration | SIM detected, PIN flow, home registration | not started |
| SMS | send and receive single and multipart | not started |
| Voice (non-emergency) | incoming and outgoing call with audio both ways | not started |
| Audio routes | earpiece, speaker, wired, Bluetooth | not started |
| Cellular data | bearer up, DNS, TLS | not started |
| Suspend and resume | wake on call/SMS, timers, radios after resume | not started |
| Crash recovery | daemon restart and reconciliation on device | not started |
| Carrier limits | VoLTE/IMS requirements, band support, documented refusals | not started |

Emergency calling is not represented anywhere and needs its own review before
any design (audit §6).

## Host transport

- Host: macOS arm64
- Android Platform Tools: 37.0.1 installed with Homebrew
- `adb`: installed at `/opt/homebrew/bin/adb`; server starts successfully
- `fastboot`: installed at `/opt/homebrew/bin/fastboot`

## Discovery log

Read-only runs of `just oneplus-discover` (the tool queries only `adb devices`,
`adb shell getprop`, `fastboot devices`, `fastboot getvar`; transcripts under
`out/oneplus/discovery-<utc>.txt` replace the serial by a SHA-256 prefix and
are ignored by Git):

| Date (UTC) | Result |
| --- | --- |
| 2026-09-20 | no ADB device, no Fastboot device, no matching macOS USB registry entry |
| 2026-09-21 | `adb_devices=0`, `fastboot_devices=0`, `result=no-device` (exit 2); transcript written, nothing queried |

The next discovery attempt is read-only. In Android, enable Developer options
and USB debugging, unlock the screen, select a data-capable USB mode, and accept
the host RSA prompt. Alternatively, place the phone in its bootloader/fastboot
mode. Then run `just oneplus-discover`.

## Tooling

- `tools/oneplus/discover.sh` (`just oneplus-discover`): read-only identity
  discovery; prints the properties listed in the checklist and writes the
  redacted transcript.
- `tools/oneplus/provision-gate.sh` (`just oneplus-provision-gate --image IMG
  --recovery BACKUP --partition NAME`): the provisioning workflow skeleton. It
  resolves and prints serial, product/codename, hardware revision, current
  slot, partition target, image SHA-256, and recovery artifact path and
  SHA-256; refuses zero or multiple devices; requires the typed phrase
  `I have verified <serial-prefix> <codename> <revision>` on stdin; and then
  exits 3 with `{"event":"provisioning-refused","reason":"no-approved-proposal"}`.
  It contains no unlock, erase, format, flash, boot, slot-change, or reboot
  command.
- `tools/oneplus/tests/provision-gate-is-inert.sh` (`just oneplus-tools-test`):
  proves both scripts stay read-only, statically (allowed adb/fastboot verbs
  only) and dynamically with stubbed `adb`/`fastboot` (zero devices, multiple
  devices, wrong confirmation, exact confirmation → refusal; every stub call
  logged and checked).

## Required implementation evidence

- Upstream or maintained postmarketOS/device-package basis for the exact target
- Kernel, device tree, boot image format, firmware, and driver sources
- Complete partition inventory and slot semantics obtained without mutation
- Stock firmware source plus verified hashes and a tested restore procedure
- Backup inventory stored outside the repository and verified readable
- Non-destructive boot route where supported; otherwise a documented rationale
- Capability matrix for every `ferrix-hardware-api` capability
- Boot, display, touch, storage, charging, networking, audio, modem, sensors,
  suspend, and recovery evidence, each labelled with exact target and revision

## Provisioning gate

No unlock, erase, format, slot change, boot, or flash command may run until all
identity and implementation fields above are complete and reviewed. The
provisioning workflow must bind to one explicit serial, verify product and
revision again immediately before mutation, print partition names and image
hashes, verify recovery artifacts, and require a typed per-run confirmation.
`provision-gate.sh` implements the resolution, printing, and confirmation
steps today and refuses at the end; the mutation step does not exist and may
only be added together with the approved proposal.
