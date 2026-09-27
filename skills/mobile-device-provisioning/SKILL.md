---
name: mobile-device-provisioning
description: Use when a human attaches an Android phone over ADB or Fastboot and wants to identify it, match it to a checked-in Omarchy Ferrix board target, download official stock recovery images, back up and unlock it, restore stock, build and flash Ferrix Linux images, or bring up Wi-Fi, Bluetooth, and the modem and record evidence. Hard rules - never run raw adb or fastboot mutation commands; mutation happens only through checked-in tools/<target>/ workflows; every destructive run needs the human's explicit go-ahead in this session after seeing the printed gate output.
---

# Mobile device provisioning

Playbook for taking a phone from "attached over USB" to "Ferrix Linux running with
radios evidenced", inside the rules of `AGENTS.md`. Read `AGENTS.md` first. This
skill never replaces it. For the current admitted target read
`targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/README.md`.

Honesty markers used below:

- **Implemented** - script and offline stub test exist in the tree.
- **Run on hardware** - executed on the exact physical target; date given.
- **Stub-tested only** - passes offline PATH-stub tests; never run against a
  phone. Run the script with no arguments and read its usage block before use.
- **Not implemented** - do not claim or attempt.

## Full end-to-end run order

Tags: `[ro]` read-only, `[confirm]` needs the human's per-run go-ahead and a
typed phrase, `[linux]` native Linux x86_64 host only, `[opt-in]` needs a
`FERRIX_*=I-understand-...` token the human agreed to in this session.

1. `just host-check` `[ro]`; `just pixel-9a-tools-check` `[ro]` (adb/fastboot present).
2. `just device-discover` `[ro]` then `just device-match` `[ro]`.
3. On `match=none`: `just device-propose` `[ro]`, then STOP for owner approval.
4. `just pixel-9a-discover` `[ro]` (exact-target transcript for a matched Pixel 9a).
5. `just pixel-9a-recovery-download` `[ro]` (or `device-recovery-download <id>` `[opt-in]`); `just pixel-9a-factory-inspect` `[ro]`.
6. Human makes and verifies a backup outside Git.
7. `just pixel-9a-unlock-bootloader --preflight` `[ro]`; `--confirm-unlock` `[confirm]` (wipes data).
8. Stock paths when needed: `just pixel-9a-sideload-ota --preflight` `[ro]` / `--confirm-sideload` `[confirm]`; `just pixel-9a-factory-restore --preflight` `[ro]` / `--confirm-restore` `[confirm]`.
9. Build chain `[linux]` `[opt-in]`: kernel-check, kernel-sync, kernel-verify, kernel-build, firmware-check/inventory/stage, rootfs-check/validate/build, boot-check/build, artifacts-check/manifest.
10. `just pixel-9a-flash-check` `[ro]`; `just pixel-9a-flash-boot-temp` `[confirm]`; `just pixel-9a-flash --stage kernel --slot <a|b>` `[confirm]`.
11. Guest booted over USB network: `just pixel-9a-radios-check` `[ro]`; `just pixel-9a-radios-inventory` `[ro]`; `just pixel-9a-radios-enable all` `[confirm]`; `just pixel-9a-radios-connect-wifi <SSID>` `[confirm]`; `just pixel-9a-radios-evidence` `[ro]`.
12. Record redacted evidence and docs (Phase 8); owner pastes the `capabilities.toml` fragment.

Offline test recipes that must pass before any hardware session:
`just device-tools-test`, `just pixel-9a-tools-test`, `just pixel-9a-flash-test`,
`just pixel-9a-radios-test`.

## Purpose and boundaries

- Ferrix is emulator-first; a phone is a separately admitted board target.
  Never generalize QEMU/mock evidence to hardware or across hardware revisions.
- Product code depends on `ferrix-hardware-api`; board details live under
  `targets/boards/<target-id>/` and `tools/<target>/`. No device conditionals
  in apps, `ferrix-core`, or `ferrix-hardware-api`.
- Discovery and matching are generic and read-only. Every state-changing
  operation is target-scoped, checked in, and gated.
- Firmware, backups, identifiers, keys, dumps, and images never enter Git.

## Prerequisites

1. Run `just host-check` (read-only). Do not install anything automatically;
   installation is opt-in through `AGENT_INSTALL.md` and `just dev-setup`.
   `adb`/`fastboot` are not part of that installer; `just pixel-9a-tools-check`
   reports whether they exist and prints the install hint.
2. Check the host: run `uname -s` and `uname -m`.
   - Discovery, recovery download, inspection, unlock, sideload, restore,
     flash, and radio SSH workflows run on macOS or Linux.
   - Kernel, firmware staging, rootfs, artifact, and boot build workflows need
     native Linux x86_64 (`tools/pixel-9a/env.sh check build` and
     `tools/pixel-9a/kernel.sh preflight` refuse other hosts). On macOS, stop
     at Phase 5 and hand the build off; do not emulate or work around.
3. Cable and mode: data-capable USB cable; USB debugging on, screen unlocked,
   RSA prompt accepted, or the phone in Fastboot. One phone attached at a time.
4. Ask the human for: owner authorization for this device, whether a verified
   backup exists, and the intended end state (stock recovery or Ferrix).

## Phase 0 - Safety contract

Apply these rules for the whole session. They have the force of `AGENTS.md`.

1. Never type a raw `adb` or `fastboot` command that reboots, unlocks, locks,
   erases, formats, flashes, sideloads, boots an image, or changes slots.
   Read-only verbs (`adb devices -l`, `adb shell getprop`, `fastboot devices
   -l`, `fastboot getvar`) are what the checked-in discovery scripts already
   run; prefer the scripts over typing even those.
2. Mutation runs only through a checked-in workflow under `tools/<target>/`
   (today: `tools/pixel-9a/`). Such a workflow prints the exact serial,
   product/codename, hardware revision, current slot, partition target, input
   image hash, and recovery artifact hash, refuses zero or multiple devices,
   and requires typed per-run confirmation on stdin.
3. Per-run human confirmation is mandatory. Before each mutating run: show the
   human the complete preflight/gate output, state the consequences (data
   wipe, brick risk, device left in Fastboot), and get an explicit go-ahead in
   chat for that run. Only then type each confirmation phrase into the
   workflow's stdin. Use `references/gate-checklist.md`.
4. Never pre-fill or script a confirmation phrase, pipe `yes`, or answer a
   prompt from a file. Never set an opt-in `FERRIX_*=I-understand-...` token
   unless the human agreed to that specific action in this session. Never
   pass a `--skip-*`, `--allow-*`, or `--force`-style flag unless the human
   directed it in this session after you explained what it disables, and you
   record why.
5. If a gate refuses, report the refusal verbatim and stop. Do not weaken,
   edit, or bypass the gate, and do not retry with different inputs to make
   it pass.
6. For an unknown device: scaffold a proposal (Phase 1) and STOP. Destructive
   provisioning needs the owner's explicit approval of the proposal plus
   passing identity, image-compatibility, backup, and recovery gates.
7. Never relock a bootloader. No workflow does it; do not propose it until a
   stock factory restore has been verified on that exact device.
8. Redact before you write anything into the repository: serial, IMEI, IMSI,
   ICCID, MAC/BSSID, SSID, PSK, keys, backups, firmware, images, raw dumps.
   Label every piece of evidence `QEMU`, `mock`, or the exact target id.

## Phase 1 - Identify

**Status:** generic tools Implemented, Stub-tested only (`just
device-tools-test`, 175 checks). Pixel 9a discovery Implemented and Run on
hardware (2026-09-27).

All `tools/device/` scripts honour `FERRIX_OUT_DIR`, `FERRIX_BOARDS_DIR`, and
`FERRIX_DOCS_DIR`; leave them unset for a real session.

1. `just device-discover` (`tools/device/discover.sh`): read-only for any
   vendor. Exit `0` ok, `2` no device, `3` ambiguous, `4` ADB unauthorized.
   Prints the exact serial to you; writes redacted
   `out/device/discovery-<stamp>.txt` and sh-sourceable
   `out/device/identity-latest.env` (serial as SHA-256 prefix). Over Fastboot
   the manufacturer is unobservable: set `FERRIX_DEVICE_MANUFACTURER` from
   what the human told you (recorded as `manufacturer_source='operator'`).
2. `just device-match` (`tools/device/match-target.sh [identity.env]`):
   `match=exact target_id=... target_dir=...` (exit 0) or `match=none`
   (exit 10) with `partial=` rows and, for a same-codename different-revision
   device, the note `do not reuse target evidence`.
3. Decide:
   - `match=exact` -> continue with that target's `tools/<target>/` workflows.
     For Pixel 9a also run `just pixel-9a-discover` so the exact-target
     transcript exists under `out/pixel-9a/`.
   - `match=none` -> `just device-propose`
     (`tools/device/propose-board.sh [identity.env]`): refuses if a target
     already matches or paths exist; scaffolds
     `targets/boards/<id>/{manifest.toml,capabilities.toml,recovery.pins,README.md}`
     and `docs/board-support/<codename>-attached.md` (all `unsupported`, gates
     `pending`, "NOT approved" section); prints the `.gitignore` block to add.
     Fill what read-only discovery supports, then STOP and report; owner
     approval is required first. Follow `references/new-target-playbook.md`.
   - Same codename, different revision -> treat as `none`.

## Phase 2 - Recovery images

**Status:** Pixel 9a download and offline inspection Implemented and Run
(2026-09-28, `CP3A.260905.009`, factory + full OTA verified). Generic
`device-recovery-download` Implemented, Stub-tested only.

1. Read `targets/boards/<id>/recovery.pins` (TSV: `kind build version
   filename url sha256 bytes source-page`). Only official vendor URLs are
   acceptable; the Pixel script refuses anything outside
   `https://dl.google.com/dl/android/aosp/`, and the generic script accepts
   `https://` only (`file://` exists solely for tests behind
   `FERRIX_RECOVERY_ALLOW_FILE_URL=1`).
2. Download and verify. Pixel 9a: `just pixel-9a-recovery-download` (size and
   SHA-256 checked; files land in the ignored
   `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/recovery/` with a
   `download-manifest.txt`). Generic:
   `just device-recovery-download <id> [factory|ota|all]`
   (`tools/device/recovery-download.sh --target <id> [--kind ...]`), resumable
   through a `.part` file, opt-in token
   `FERRIX_RECOVERY_DOWNLOAD=I-understand-this-downloads-vendor-firmware`;
   ask the human before setting it (multi-gigabyte vendor download).
3. Inspect offline. Pixel 9a: `just pixel-9a-factory-inspect` re-verifies the
   archive and writes `out/pixel-9a/factory-inspection/inventory.json`
   (`android-info.txt` requirements, `fastboot-info.txt`, image members). No
   device contact. The factory-restore workflow requires this file.
4. Hash or size mismatch: stop, do not edit the pin. Re-download once; if it
   still mismatches, report to the owner.
5. New-target pins: record `url`, `sha256`, `bytes`, and the official
   `source-page` for the exact codename and build; verify against the vendor
   page, never a mirror. See `references/new-target-playbook.md`.

## Phase 3 - Backup and bootloader

**Status:** Pixel 9a unlock gate Implemented and Run on hardware
(2026-09-27, `unlocked=yes`). The backup gate is `pending` in
`manifest.toml` and `linux-plan.md`.

1. Backup: the human backs up personal data outside the repository and
   verifies it is readable. No checked-in backup workflow exists; do not
   improvise one with `adb pull` or `adb backup`. Record only "backup verified
   by owner on <date>, stored outside Git".
2. Unlock preflight (read-only): `just pixel-9a-unlock-bootloader --preflight`
   re-verifies both recovery archives, requires exactly one transport, re-reads
   identity, and prints serial, codename, revision, slot, partition target,
   both recovery hashes, and `preflight=passed`.
3. Unlock (wipes all user data): after the checklist and the human's
   go-ahead, `just pixel-9a-unlock-bootloader --confirm-unlock`. Phrases:
   `CONFIRM PIXEL 9A REBOOT AND UNLOCK (WIPES ALL USER DATA)`, then after the
   Fastboot identity re-check
   `CONFIRM PIXEL 9A BOOTLOADER UNLOCK (WIPES ALL USER DATA)`; the phone then
   shows its own prompt for the human. Only `adb reboot bootloader` and
   `fastboot flashing unlock` are issued. Expected tail: `unlocked_after=yes`,
   `result=bootloader-unlocked`, device stays in Fastboot.
4. Already unlocked: skip step 3; the preflight remains the identity check.

## Phase 4 - Stock recovery paths

**Status:** both workflows Implemented, Stub-tested only (`just
pixel-9a-tools-test`: sideload 49 checks, factory restore 69 checks). Neither
has run on hardware; `manifest.toml` records
`recovery_procedure = "documented-not-tested"`.

Choose by device state:

| Device state | Route | Workflow |
| --- | --- | --- |
| Stock Android or stock recovery boots; want stock back without a wipe | Full OTA sideload | `just pixel-9a-sideload-ota --preflight`, then `--confirm-sideload` |
| Unlocked Fastboot, stock damaged or Ferrix images present; accept a wipe | Factory restore | `just pixel-9a-factory-restore --preflight [--keep-data] [--from-adb]`, then `--confirm-restore` with the same flags |
| Neither ADB nor Fastboot visible | none | host-side workflows cannot help; report to owner |

- Sideload: one ADB device in `device`, `recovery`, or `sideload` state, no
  Fastboot device present. Identity is fully checked in Android, partially in
  recovery, and unobservable in sideload mode, where the phrase must include
  `IDENTITY unavailable-in-sideload`. Phrases:
  `CONFIRM PIXEL 9A REBOOT TO RECOVERY FOR OTA SIDELOAD` (the human then picks
  "Apply update from ADB"; the workflow waits up to 180 s for the same serial),
  then `CONFIRM PIXEL 9A OTA SIDELOAD <file> SHA256 <first 16 hex>`. Only
  `adb reboot recovery` and `adb sideload` are issued. Result
  `result=ota-sideload-complete|failed`; transcript in `out/pixel-9a/sideload/`.
- Factory restore: needs `just pixel-9a-factory-inspect` output first; refuses
  unless the archive's own `flash-all.sh` equals the reviewed sequence (flash
  bootloader, reboot-bootloader, flash radio, reboot-bootloader,
  `fastboot -w update <image zip>`). Requires one Fastboot device with
  `product=tegu`, `hw-revision=MP1.0`, `unlocked=yes`, `is-userspace=no`;
  `--from-adb` adds a gated `adb reboot bootloader`. Phrases:
  `CONFIRM PIXEL 9A FLASH BOOTLOADER AND RADIO (WIPES ALL USER DATA)`, then
  `CONFIRM PIXEL 9A FASTBOOT UPDATE (WIPES ALL USER DATA)`; with `--keep-data`
  both end `(KEEP USER DATA)` and `-w` is omitted. After each bootloader/radio
  flash it re-reads `version-bootloader`/`version-baseband` and stops with
  `REFUSING:` on mismatch; the phone stays in Fastboot and re-running is safe.
  Never issues lock, erase, format, slot, or oem. Transcript in
  `out/pixel-9a/restore/`.
- After either route, run `just pixel-9a-discover` and record the observed
  build, slot, and bootloader state (redacted) as recovery evidence. Only
  then may `recovery_procedure` move from `documented-not-tested`.

## Phase 5 - Build Ferrix images

**Status:** Implemented and Stub-tested only; nothing has been built
(`kernel_build_status`, `rootfs_build_status` = `not-built`). Native Linux
x86_64 only. On macOS: run the `*-check`/`*-test` recipes if useful, then
stop and hand off with the exact commands below. Do not try Docker or
cross-compilation substitutes; the pinned Google prebuilts and
`pmbootstrap` are Linux-only by design.

Load the host profile first: `. tools/pixel-9a/env.sh`, then
`just pixel-9a-env-validate` and `just pixel-9a-env-check build`.

Chain, in order (each token needs the human's OK in this session):

```sh
just pixel-9a-kernel-check
FERRIX_PIXEL9A_KERNEL_SYNC=I-understand-this-downloads-large-kernel-sources just pixel-9a-kernel-sync
just pixel-9a-kernel-verify
FERRIX_PIXEL9A_KERNEL_BUILD=I-understand-this-runs-a-large-kleaf-build just pixel-9a-kernel-build
just pixel-9a-firmware-check
just pixel-9a-firmware-inventory
FERRIX_PIXEL9A_FIRMWARE=I-understand-this-extracts-verified-firmware \
  FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST=<reviewed TSV> just pixel-9a-firmware-stage
just pixel-9a-rootfs-check
just pixel-9a-rootfs-validate
FERRIX_PIXEL9A_ROOTFS_BUILD=I-understand-this-runs-pmbootstrap \
  FERRIX_PIXEL9A_PMAPORTS_DIR=<pinned pmaports checkout> \
  FERRIX_PIXEL9A_KERNEL_DIST=out/pixel-9a/kernel/tegu/dist \
  FERRIX_PIXEL9A_FIRMWARE_DIR=<staged firmware dir with firmware-manifest.tsv> \
  just pixel-9a-rootfs-build
just pixel-9a-boot-check
FERRIX_PIXEL9A_BOOT_BUILD=I-understand-this-composes-pixel-9a-boot-artifacts \
  FERRIX_PIXEL9A_INITRAMFS=<cpio> FERRIX_PIXEL9A_VENDOR_RAMDISK=<cpio> \
  FERRIX_PIXEL9A_VENDOR_BOOTCONFIG=<bootconfig> \
  just pixel-9a-boot-build
just pixel-9a-artifacts-check
just pixel-9a-artifacts-manifest
```

Outputs (all ignored) land under `out/pixel-9a/{kernel,firmware,rootfs,images}/tegu/`
with a manifest each; the boot manifest is `composed-not-booted` /
`bootable false`. Kernel sync needs Google's `repo` and about 80 GiB under
`.cache/ferrix/pixel-9a/kernel/`; build refuses a non-empty dist. Never mix
stock Android modules with a Ferrix kernel (`image-layout.toml` policy).

## Phase 6 - Flash

**Status:** Implemented, Stub-tested only (`just pixel-9a-flash-test`, 64
checks). Not run on hardware; `boot_status = "not-attempted"`.

Every mode re-hashes all composed artifacts against `boot-manifest.tsv`,
verifies the pinned factory archive (refuses if absent: no restore path, no
flash), and requires exactly one Fastboot device with `product=tegu`,
`hw-revision=MP1.0`, `unlocked=yes`, `slot-count=2`.

1. `just pixel-9a-flash-check [--stage kernel|rootfs|all] [--slot a|b]`
   (read-only): prints serial, current and target slot, `is_userspace=`, one
   `plan=... partition_target=<p>_<slot> via=bootloader|fastbootd
   input_image_sha256=...` line per artifact, and `recovery_artifact=`.
2. AVB: the composer defaults to `avb_mode=NONE` (unsigned vbmeta) and every
   retail Pixel reports `secure=yes`, so the workflow refuses unless
   `--allow-unsigned-avb` is passed. Explain to the human that unsigned images
   boot only while the bootloader stays unlocked and that relocking would
   leave the phone unbootable; pass the flag only after their explicit OK in
   this session, and record it.
3. Temporary boot first (`image-layout.toml` policy):
   `just pixel-9a-flash-boot-temp [--allow-unsigned-avb]`. Phrase:
   `CONFIRM PIXEL 9A TEMPORARY BOOT boot.img@<first 8 hex of its sha256>`.
   One `fastboot boot <boot.img>`, then `result=temporary-boot-issued` and
   `out/pixel-9a/flash/boot-temp-<stamp>-<pid>.txt`. Header-v4 limitation: the
   phone's currently flashed vendor_boot/init_boot/dtbo are used, so this is a
   kernel smoke check, not a Linux boot. Capture console evidence; a hang or
   loop is a stop-and-report.
4. Persistent flash (device in bootloader mode, `is-userspace=no`):
   `FERRIX_PIXEL9A_TEMP_BOOT_EVIDENCE=<boot-temp transcript>
   just pixel-9a-flash --stage kernel --slot <a|b> [--allow-unsigned-avb]`.
   The evidence must record the same boot.img hash, exit 0, and the same
   device, or the workflow refuses. Phrase:
   `CONFIRM PIXEL 9A FLASH KERNEL STAGE TO SLOT <A|B>`. Kernel stage writes
   boot, init_boot, vendor_boot, vendor_kernel_boot, dtbo, and the three vbmeta
   images with the slot suffix, hops to fastbootd for `vendor_dlkm` and
   `system_dlkm`, and returns to the bootloader. Each write re-hashes its image
   and re-checks transport, serial, and product.
5. Rootfs stage (`--stage rootfs` or `all`) currently REFUSES: the rootfs
   manifest provides `rootfs.tar`, a handoff archive, not a flashable
   `rootfs.img`. Do not build one ad hoc; report the refusal.
6. Slot: default is the current slot; `--slot` overrides. The workflow never
   issues `set_active`, so images written to the inactive slot cannot be
   booted by it, and it prints a `slot_warning` either way. `linux-plan.md`
   says the first artifact set must not overwrite the active slot; the flash
   default disagrees. Put that choice to the owner before the run; do not
   resolve it yourself.
7. Result semantics: success prints `result=flashed-not-booted`. It is not
   boot evidence; `bootable` stays false until a Linux boot is observed.
   Transcript `out/pixel-9a/flash/flash-<stamp>-<pid>.txt`.
8. `--skip-temp-boot-gate` changes the phrase to end in
   `WITHOUT TEMP BOOT GATE`. Owner-directed debugging only.

## Phase 7 - Radios

**Status:** Implemented, Stub-tested only (`just pixel-9a-radios-test`, 70
checks against a stubbed `ssh`). Adapters exist as crates
(`ferrix-network-nm`, `ferrix-bluetooth-bluez`, `ferrix-telephony-mm`,
`ferrix-power-upower`, `ferrix-audio-wireplumber`) with host tests only. No
radio has been observed working on hardware.

Route: SSH into the booted guest over the postmarketOS USB network
(`FERRIX_PIXEL9A_SSH_HOST` default `172.16.42.1`; `_SSH_USER` from
`pmos/pmbootstrap.cfg`; `_SSH_PORT`; `_SSH_OPTS`). Exit codes: `2` usage, `3`
unreachable, `4` device-tree mismatch, `5` QEMU refused, `6` confirmation
mismatch. Transcripts: `out/pixel-9a/radios/<command>-<stamp>.txt`.

1. `just pixel-9a-radios-check`: reads `/proc/device-tree/compatible`,
   hostname, kernel, OS. Requires `FERRIX_PIXEL9A_DT_COMPATIBLE` (default
   `google,tegu`); override only after the first physical transcript shows
   the exact string. `linux,dummy-virt` (QEMU) is refused.
2. `just pixel-9a-radios-inventory` (read-only): `rfkill`, NetworkManager,
   BlueZ, ModemManager, modules, `dmesg` firmware lines, allow-listed firmware
   presence, OpenRC service state, and the Ferrix adapter path
   (`ferrix-networkd` readiness record or socket reply; `ferrix-core` socket
   state). Writes `out/pixel-9a/radios/evidence-latest.tsv` with columns
   `subsystem status progress evidence`.
3. `just pixel-9a-radios-enable [wifi|bluetooth|modem|all]`: phrase
   `CONFIRM PIXEL 9A ENABLE RADIOS: <list>`; then `rfkill unblock`, `nmcli
   radio wifi on`, `bluetoothctl power on`, `mmcli -m any --enable`. Modem is
   skipped with a printed note when `mmcli -L` lists none. Changes radio
   state only, never storage.
4. `just pixel-9a-radios-connect-wifi <SSID>`: PSK from
   `FERRIX_PIXEL9A_WIFI_PSK` or a no-echo prompt, piped to `nmcli --ask` over
   stdin; it never appears in argv, history, chat, or transcripts. SSID is
   stored only as a hash. Result `wifi_connected=yes|no`.
5. `just pixel-9a-radios-evidence`: runs inventory, prints a
   `capabilities.toml` fragment labelled
   `physical: google-pixel-9a-tegu-gxq96-mp1-0`. The OWNER pastes it; the
   workflow never edits `capabilities.toml` and neither do you.
6. Vocabulary: `status` is only `supported` or `unsupported`
   (`CapabilityStatus` has no `Partial`; `mocked` is never used for a
   physical target). Nuance lives in `progress` and `evidence`. `supported`
   requires the Ferrix adapter path observed, not just Linux tools working.
7. Telephony never becomes `supported` from this workflow: the Samsung S5300
   CPIF modem exposes no ModemManager modem and no CPIF backend exists;
   `ferrix-s5300-cpif` is only a SIPC5 frame codec. Modem enumeration is not
   phone support. See `references/radios-and-evidence.md`.

## Phase 8 - Record evidence and docs

After any hardware session, update only what the evidence supports:

- `docs/status.md`: dated entry; every claim labelled with the exact target
  id; say what was not done.
- `targets/boards/<id>/README.md`: observed state, dates, and a limitation
  sentence per unproven capability.
- `targets/boards/<id>/manifest.toml`: `[gates]`, `[linux]` status fields,
  and `status`.
- `docs/board-support/<codename>-attached.md`: checklist statuses and a
  discovery-log row per session.
- `targets/boards/<id>/linux-plan.md`: tick a gate only with evidence.
- `capabilities.toml`: owner-only, from the radios fragment.

Redaction checklist before `git add`: no serial (prefix hash only), no IMEI,
IMSI, ICCID, MAC/BSSID, SSID, PSK, no `*.img`/`*.zip`/`*.bin`/dumps/logs from
the device, no backups or keys, no vendor firmware bytes, no paths that embed
a serial. Everything generated stays under ignored `out/` or the ignored
target subfolders listed in `.gitignore`. Run `git status` and read the diff.

## Failure handling

| Signal | Action |
| --- | --- |
| discover exit `2` (no device) | Check cable, USB mode, debugging; ask human to unlock the screen; do not retry in a loop |
| discover exit `3` (ambiguous) | Ask the human to detach other phones/emulators; never pick one with `-s` |
| discover exit `4` (unauthorized) | Human accepts the RSA prompt on the phone; re-run |
| match exit `10` (none) | Propose and STOP; owner approval needed |
| Hash or size mismatch | Stop; do not edit pins; re-download once; report |
| Any gate `die`/`REFUSING`/`provisioning-refused` | Report verbatim; stop; no workaround |
| Restore stops between stages (version mismatch) | Device is still in Fastboot; report; re-run only with a fresh go-ahead |
| Wrong `is-userspace` for the step | Let the workflow hop; never `fastboot reboot ...` by hand |
| Flash refuses without temp-boot evidence | Run `pixel-9a-flash-boot-temp` first; never use `--skip-temp-boot-gate` on your own |
| Device stays in Fastboot after unlock | Expected; next step is stock restore or a gated boot, decided by owner |
| Temp boot hangs or loops | Power-cycle per owner instruction; restore stock via Phase 4; record as failed boot evidence |
| radios exit `3`/`4`/`5` | Wrong host, wrong device tree, or a QEMU guest; you are not talking to the phone; stop |
| Build host is macOS | Stop at Phase 5 and hand off; do not emulate |
| Asked to relock | Refuse until a verified stock restore exists on this exact device |

More in `references/troubleshooting.md`.

## Reporting template

Summarize every session in chat with this shape (redacted):

```text
Target: <target id or "unknown; proposal scaffolded at ...">  Host: <uname -s/-m>
Identity: codename=<..> model=<..> revision=<..> slot=<..> bootloader=<locked|unlocked> serial=sha256:<12>
Steps run: <recipe> -> <result/exit>; ...
Gates: passed=<...> refused=<verbatim reason> not-attempted=<...>
Human confirmations given this session: <n>, for: <operations>
Evidence files (ignored paths): <out/...>
Repo edits: <files>, all redacted; nothing generated staged
Unproven: <what remains not run on hardware>
Next step needing owner decision: <...>
```

## References

- `references/workflow-map.md` - every recipe, host, gate, output, status.
- `references/gate-checklist.md` - pre-mutation checklist and confirmation wording.
- `references/new-target-playbook.md` - unknown device to approved target.
- `references/radios-and-evidence.md` - radio stack, evidence vocabulary, redaction.
- `references/troubleshooting.md` - cables, authorization, fastbootd, slots, recovery.
