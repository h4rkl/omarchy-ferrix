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
5. For development, `just pixel-9a-enter-fastboot --preflight` `[ro]` then
   `--confirm-enter` `[confirm]` may soft-reboot the exact Android device into
   ordinary bootloader Fastboot through the checked-in target workflow. Hard
   button entry is only a fallback when ADB is unavailable. This workflow never
   enters Fastbootd and never changes storage, slots, or bootloader state.
6. `just pixel-9a-recovery-download` `[ro]` (or `device-recovery-download <id>` `[opt-in]`); `just pixel-9a-factory-inspect` `[ro]`.
7. Human makes and verifies a backup outside Git.
8. `just pixel-9a-unlock-bootloader --preflight` `[ro]`; `--confirm-unlock` `[confirm]` (wipes data).
9. Stock paths when needed: `just pixel-9a-sideload-ota --preflight` `[ro]` / `--confirm-sideload` `[confirm]`; `just pixel-9a-factory-restore --preflight` `[ro]` / `--confirm-restore` `[confirm]`.
10. Build the selected radio-first chain on native Linux `[linux]` `[opt-in]`:
   `pixel-9a-kernel-radio-check/sync/verify/build`,
   `pixel-9a-product-check/build`, firmware
   `check/inventory/stage`, rootfs `check/validate/build`, and
   `pixel-9a-boot-radio-check/build`. The canonical Google Kleaf
   `pixel-9a-kernel-*`/`pixel-9a-boot-*` chain is a separate comparison route;
   never mix its CPIF, modules, DTB, or manifests into this WWAN/SIT route.
   If the exact-target, redacted partition-capacity handoff and all
   source-built split inputs are available, continue with
   `pixel-9a-radio-split-check` and the opt-in `pixel-9a-radio-split-build`.
   This device-free composer fills the reviewed `vendor_kernel_boot`, `dtbo`,
   `vendor_dlkm`, and `system_dlkm` handoff, then generates fresh salted AVB
   descriptors from the final image bytes; external or stock AVB metadata is
   rejected. It still emits
   `composed-not-booted` / `bootable=false`.
   The separate static-only route takes a fresh ordinary-bootloader
   `pixel-9a-partition-capacity-static` handoff for eight physical boot
   partitions. Its device-free `pixel-9a-static-boot-check/build/verify`
   workflow requires a current source-built kernel, matching boot inputs,
   built-in early drivers, no selected loadable modules, and fresh AVB
   descriptors. It cannot consume the ten-partition split handoff and cannot
   authorize a device write. Its separate checked-in static candidate and
   `pixel-9a-flash-static-check` admit an exact target read-only; the
   `pixel-9a-flash-static-boot-temp` workflow can issue only one confirmed
   kernel-only `fastboot boot`. For a named build output, set
   `FERRIX_PIXEL9A_STATIC_BOOT_OUT` when running
   `pixel-9a-static-boot-verify-transfer`. A separate persistent static
   adapter is offline-tested but refuses the current partial image-compatibility
   gate and missing observed boot evidence; do not run its mutations until
   those gates and each new owner confirmation pass.
11. `just pixel-9a-partition-inventory` `[ro]`; when the exact Android
    transport is available, also run `just pixel-9a-partition-capacity` `[ro]`
    to record the ten static/dynamic partition capacities used by the
    radio-first split handoff. If Android hides inactive dynamic mappings,
    the same capacity workflow may be run in an owner-entered Fastbootd
    session with exactly one device and `is-userspace=yes`; it never enters
    Fastbootd itself. This capacity file may succeed while the broader `/proc`
    inventory remains `partial`; it never upgrades that broader inventory gate
    or authorizes a flash. Do not pass either
    `images/tegu-wwan` or `images/tegu-wwan-split` to the canonical
    `pixel-9a-flash-*` commands: they are radio-first handoffs with a separate
    manifest contract. Once `images/tegu-wwan-split/boot-manifest.tsv` is
    complete, use only the checked-in route-specific adapter below; it still
    requires the same identity, backup, recovery, inactive-slot, image-hash,
    and confirmation gates before any physical boot or flash is attempted.
    For the static-only route, `just pixel-9a-userdata-map` `[ro]` in ordinary
    bootloader Fastboot can correlate a post-restore same-device ADB inventory
    with live unslotted physical `userdata` classification, snapshot state,
    and size. Supply `FERRIX_PIXEL9A_PARTITION_INVENTORY` explicitly. This
    targeted handoff does not make the broader inventory complete and does
    not authorize formatting or flashing shared storage.
    For the eight-partition static candidate, run
    `just pixel-9a-flash-static-check` `[ro]` and inspect all exact identity,
    capacity, image-hash, recovery, and partial compatibility output. Only a
    separately owner-approved run may use
    `just pixel-9a-flash-static-boot-temp` `[confirm]` with the accepted
    development-device data-policy token. Manually type the prompted phrase;
    the workflow repeats every read-only gate before `fastboot boot`. This
    probes kernel progress with currently flashed stock `init_boot` and
    `vendor_boot`, not the full Ferrix split image set. With a reviewed text
    kernel capture, set `FERRIX_PIXEL9A_STATIC_TEMP_BOOT_EVIDENCE` to the
    ignored boot transcript and `FERRIX_PIXEL9A_STATIC_TEMP_BOOT_KERNEL_LOG`
    to the capture, then run `just pixel-9a-flash-static-observe-temp`. It
    requires an exact typed attestation and writes a redacted, hash-bound
    observation. Stop on a failed gate, bootloader refusal, hang, or loop.
    A temporary boot command or observation alone is not persistent flash
    authorization or evidence of a Ferrix Linux userspace boot.
    When a later review independently upgrades the exact static image
    compatibility gate, `just pixel-9a-install-static-check` `[ro]` must pass
    with `FERRIX_PIXEL9A_STATIC_TEMP_BOOT_EVIDENCE` and
    `FERRIX_PIXEL9A_STATIC_TEMP_BOOT_OBSERVATION` bound to this candidate.
    It also pins and rechecks the reviewed host Fastboot CLI version before
    each stage/write; a changed version needs a new tool-behavior review.
    The final images have AVB algorithm `NONE` while Fastboot reports
    `secure=yes`; explain the unlocked-device unsigned-AVB consequence and
    obtain explicit owner direction before setting
    `FERRIX_PIXEL9A_STATIC_UNSIGNED_AVB=I-accept-unsigned-AVB-on-this-unlocked-Pixel-9a`.
    With that token, the development-device token, and a separate owner
    go-ahead for each run, `pixel-9a-install-static-kernel` writes only eight
    inactive physical slot-B images; `pixel-9a-install-static-rootfs` then
    overwrites unslotted shared `userdata` after checking the complete
    kernel-stage transcript;
    `pixel-9a-install-static-activate` checks both stages, selects slot B,
    and requests reboot only after validating the separate pinned
    `static-activation-candidate.toml` proposal. The image candidate itself
    continues to forbid slot selection. Every stage reruns exact identity,
    recovery, live capacities, image hashes, and observation gates before
    mutation; the kernel stage rechecks before each write. These workflows
    are stub-tested only, have never run on hardware, and a successful Fastboot command is
    not readback proof or Ferrix boot evidence. Stop on any refusal or partial
    stage; do not retry or switch slots to work around it.
12. For the canonical Google route only, once a complete, reviewed image set
    exists and the owner authorizes the physical operation:
    `just pixel-9a-flash-check` `[ro]`; `just pixel-9a-flash-boot-temp`
    `[confirm]`; then the explicit inactive-slot flash `[confirm]`. The
    never substitute these canonical commands for the radio-first route. For
    the complete source-built radio-first split handoff, use
    `just pixel-9a-flash-radio-check` `[ro]`, then
    `just pixel-9a-flash-radio-boot-temp` `[confirm]`, and, only after the
    temporary-boot evidence gate passes, the explicit inactive-slot
    `just pixel-9a-flash-radio` `[confirm]`. The route never changes the active
    slot, bootloader lock state, or factory firmware. After a guest is actually
    booted over USB network, use
    `pixel-9a-radios-check/inventory` `[ro]`,
    `pixel-9a-radios-enable all` `[confirm]`,
    `pixel-9a-radios-connect-wifi <SSID>` `[confirm]`, and
    `pixel-9a-radios-evidence` `[ro]`.
13. Record redacted evidence and docs (Phase 8); owner pastes the `capabilities.toml` fragment.

Offline test recipes that must pass before any hardware session:
`just device-tools-test`, `just pixel-9a-tools-test`,
`just pixel-9a-kernel-radio-test`, `just pixel-9a-product-test`,
`just pixel-9a-firmware-test`, `just pixel-9a-rootfs-test`,
`just pixel-9a-enter-fastboot-test`,
`just pixel-9a-boot-radio-test`, `just pixel-9a-radio-split-test`,
`just pixel-9a-static-boot-test`,
`just pixel-9a-flash-static-test`,
`just pixel-9a-install-static-test`,
`just pixel-9a-artifacts-test`, `just pixel-9a-flash-test`,
`just pixel-9a-flash-radio-test`,
`just pixel-9a-partition-capacity-test`, and `just pixel-9a-radios-test`.

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
   - Kernel, firmware staging, rootfs, artifact, boot, and radio-split build
     workflows need native Linux x86_64 (`tools/pixel-9a/env.sh check
     build` and build paths refuse other hosts; static checks/tests may run
     locally). On this Apple Silicon Mac, an ARM64 UTM guest is not a
     compliant builder and an x86_64 UTM guest would use CPU emulation. A
     digest-pinned `linux/amd64` Docker tool container is allowed only when Docker is
     running on a native Linux x86_64 host; it does not make an ARM64 host,
     OrbStack VM, Lima VM, or emulated UTM guest compliant. Keep ADB/Fastboot
     on the host-side physical-device workflow: do not expose USB transport
     or an ADB server socket to the build container. UTM may still be used for
     a separate Linux smoke test or QEMU workflow. On macOS, stop at Phase 5
     and hand the Pixel build off to provisioned native Linux x86_64.
   - The build preflight must also resolve the physical profile's Rust target
     in the pinned toolchain. The repository declaration retains
     `aarch64-unknown-linux-gnu` for QEMU/CI and now also declares
     `aarch64-unknown-linux-musl` for the physical profile. The Pixel workflow
     still verifies that the musl standard library is installed and refuses a
     missing target; do not silently switch to the GNU target or a
     cross-build substitute. A container may provide pinned Linux build tools
     only under the native-host Docker exception above.
   - The same preflight requires an explicit AArch64 musl linker: the
     canonical `aarch64-linux-musl-gcc` command or an equivalent command named
     by `FERRIX_PIXEL9A_RUST_LINKER`. `just pixel-9a-env-check build` and
     `just pixel-9a-product-check` must find that linker before the opt-in
     build; do not let Cargo select the host x86_64 linker or install one
     implicitly through the workflow.
3. Cable and mode: data-capable USB cable; USB debugging on, screen unlocked,
   RSA prompt accepted, or the phone in Fastboot. When ADB is available, the
   LLM may use `just pixel-9a-enter-fastboot --confirm-enter` after its exact
   target preflight; hard-button Fastboot entry is not required for normal
   development. One phone attached at a time.
4. Ask the human for: owner authorization for this device, whether a verified
   backup exists, and the intended end state (stock recovery or Ferrix).

## Phase 0 - Safety contract

Apply these rules for the whole session. They have the force of `AGENTS.md`.

1. Never type a raw `adb` or `fastboot` command that reboots, unlocks, locks,
   erases, formats, flashes, sideloads, boots an image, or changes slots.
   The sole development transport exception is the checked-in
   `tools/pixel-9a/enter-fastboot.sh --confirm-enter` workflow, which may
   issue exactly `adb reboot bootloader` after exact-target checks and its
   typed confirmation. The separate checked-in
   `tools/pixel-9a/boot-inactive.sh confirm-boot` workflow may select an
   already-flashed inactive slot and reboot it after its own evidence, identity,
   recovery, and confirmation gates; that exception never authorizes a raw
   `set_active` or reboot command. Read-only verbs (`adb devices -l`,
   `adb shell getprop`, `fastboot devices -l`, `fastboot getvar`) are what the
   checked-in discovery scripts already run; prefer the scripts over typing
   even those.
2. Destructive bootloader/storage mutation runs only through a checked-in
   workflow under `tools/<target>/` (today: `tools/pixel-9a/`). Such a
   workflow prints the exact serial, product/codename, hardware revision,
   current slot, partition target, input image hash, and recovery artifact
   hash, refuses zero or multiple devices, and requires typed per-run
   confirmation on stdin. Guest-only radio state changes use the checked-in
   `tools/pixel-9a/radios.sh` workflow instead: its preflight and
   pre-mutation recheck require the exact pinned device-tree model/compatible,
   it never changes storage, partitions, slots, or the bootloader, and it
   still requires a typed confirmation for each state change.
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

Before a build or flash route, run `just pixel-9a-partition-inventory`
`[ro]` while exactly one ADB or Fastboot transport is visible. It records the
redacted exact-target slot and partition evidence under ignored `out/`. Some
Android builds expose an empty `ro.boot.slot_count` over ADB; the workflow may
derive `slot_count=2` only after a read-only `/dev/block/by-name` listing shows
both `_a`/`.a` and `_b`/`.b` partition names, and records
`slot_count_source=block-by-name`. It never guesses a slot count. Treat this
pre-boot inventory as topology/admission evidence, not as proof that Ferrix
Linux or any radio works; capture a fresh guest inventory for any later rootfs
mapping gate. If Android denies `/proc/partitions` or `/proc/cmdline`, the
workflow records `inventory_completeness=partial`; rootfs flashing requires
`complete`, so stop and obtain a complete read-only guest inventory first.
The separate `just pixel-9a-partition-capacity` workflow reads only the
read-only `/sys/class/block/*/size` values behind both A/B by-name entries and
independent active/inactive dynamic-partition mapper entries. It writes a
serial-free `out/pixel-9a/partition-capacities-latest.tsv` handoff with ten
4096-byte-aligned byte capacities; it does not open block devices, change
storage, or promote the broader inventory from `partial` to `complete`.
If Android does not expose an inactive dynamic mapper, the same workflow may
be run with exactly one owner-entered Fastbootd device; it reads independent
`partition-size:*` getvars, requires `is-userspace=yes`, and refuses the
ordinary bootloader. It never reboots into Fastbootd, changes slots, or writes
storage.

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
   For a native-Linux source-path review of the exact firmware rows, the
   device-free builder also exposes an opt-in `inspect` mode:

   ```sh
   FERRIX_PIXEL9A_FIRMWARE=I-understand-this-extracts-verified-firmware \
     tools/pixel-9a/native-builder.sh run firmware inspect
   ```

   It re-verifies the pinned factory archive, firmware policy, and reviewed
   allowlist, then uses `debugfs` to print source-file metadata (and reviewed
   bundle directory listings) from temporary files under the ignored firmware
   output. It removes those temporary files on exit and does not populate
   `staged/` or create a runtime `firmware-manifest.tsv`; this is provenance
   inspection only, not firmware staging or physical radio evidence. Set
   `FERRIX_PIXEL9A_FACTORY_ARCHIVE` or
   `FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST` only to reviewed, checkout-contained
   paths accepted by the native-builder wrapper; keep the factory archive and
   generated firmware data ignored and out of Git.
4. Hash or size mismatch: stop, do not edit the pin. Re-download once; if it
   still mismatches, report to the owner.
5. New-target pins: record `url`, `sha256`, `bytes`, and the official
   `source-page` for the exact codename and build; verify against the vendor
   page, never a mirror. See `references/new-target-playbook.md`.

## Phase 3 - Backup and bootloader

**Status:** Pixel 9a unlock gate Implemented and Run on hardware
(2026-09-27, `unlocked=yes`). This admitted development device declares the
checked-in `dev-device-no-user-data` policy; `backup` is intentionally
`not-required-dev-device`, never `verified`.

1. Backup/data policy: a normal target requires the human to back up personal
   data outside the repository and verify it is readable. This exact admitted
   development device instead uses the checked-in
   `backup="not-required-dev-device"` plus
   `data_policy="dev-device-no-user-data"` gate. Before a mutation, the owner
   must explicitly authorize that policy in this session by setting
   `FERRIX_PIXEL9A_NO_USER_DATA=I-understand-this-is-a-new-development-device-with-no-personal-data`;
   workflows never relabel it as a verified backup. No checked-in backup
   workflow exists; do not improvise one with `adb pull` or `adb backup`.
2. Unlock preflight (read-only): `just pixel-9a-unlock-bootloader --preflight`
   re-verifies both recovery archives, requires exactly one transport, re-reads
   identity, and prints serial, codename, revision, slot, partition target,
   both recovery hashes, and `preflight=passed`. If the route reaches
   Fastboot, it also requires `variant=GXQ96` as the bootloader SKU invariant.
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

**Owner-approved first-test exception (2026-09-30):** when this exact target's
recovery gate is `documented-not-tested`, the separate
`--preflight-first-test` / `--confirm-first-test` modes of
`pixel-9a-factory-restore` may test the pinned stock recovery route. This is
an explicit bootstrap contract, not permission to relax normal restore or
Ferrix gates. Require all ordinary identity, pinned archive, stock board and
vendor-sequence checks, the data policy, and the full printed preflight plus
fresh per-run owner authorization. Explain that this stock test updates
bootloader/radio firmware and wipes data; Ferrix itself must leave that
firmware untouched. Execution adds the typed phrase
`CONFIRM PIXEL 9A FIRST STOCK RECOVERY TEST (WIPES ALL USER DATA)` before
the two normal restore-stage phrases. Do not use `--keep-data`. Neither the
preflight nor command completion promotes the gate: require the successful
restore transcript and fresh same-device Android discovery of the accepted
pinned build. The owner-authorized first test completed on hardware on
2026-09-30 with exit 0 / `factory-restore-complete`; ignored transcript
`out/pixel-9a/restore/20260930T002730Z.txt`. Subsequent discovery returned
`no-device`, so device operations paused for owner setup/USB authorization.
The owner subsequently ran read-only discovery in the normal terminal at
`2026-09-30T01:02:38Z`; saved transcript
`out/pixel-9a/discovery-20260930T010238Z.txt` matches the restore's device
identity and verifies stock `CP3A.260905.009`, Android 17, slot A, and
unlocked/orange. The recovery gate is now `tested`. Do not repeat the first
test; that bootstrap mode requires the former `documented-not-tested` state.
Normal restore still requires all its other gates and a fresh per-run approval.

**Status:** both workflows Implemented and stub-tested. Factory restoration
has completed on hardware with same-device stock Android verification;
OTA sideload remains stub-tested only. `manifest.toml` records
`recovery_procedure = "tested"` for the accepted pinned factory restore.
This does not promote Ferrix image compatibility or any physical capability.

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
  `product=tegu`, `variant=GXQ96` (the bootloader SKU invariant),
  `hw-revision=MP1.0`, `unlocked=yes`, `is-userspace=no`;
  `--from-adb` adds a gated `adb reboot bootloader`. Phrases:
  `CONFIRM PIXEL 9A FLASH BOOTLOADER AND RADIO (WIPES ALL USER DATA)`, then
  `CONFIRM PIXEL 9A FASTBOOT UPDATE (WIPES ALL USER DATA)`; with `--keep-data`
  both end `(KEEP USER DATA)` and `-w` is omitted. After each bootloader/radio
  flash it re-reads `version-bootloader`/`version-baseband` and stops with
  `REFUSING:` on mismatch; the phone stays in Fastboot and re-running is safe.
  Never issues standalone lock, erase, format, slot, or oem commands.
  The vendor `-w update` itself selects a slot, writes shared `super`, and
  erases userdata/metadata; those effects were observed in the approved
  first test and are not allowed by Ferrix's separate preserving contract.
  Transcript in
  `out/pixel-9a/restore/`.
- After either route, run `just pixel-9a-discover` and record the observed
  build, slot, and bootloader state (redacted) as recovery evidence. Only
  then may `recovery_procedure` move from `documented-not-tested`.

## Phase 5 - Build Ferrix images

**Status:** Implemented and Stub-tested only. The radio-first source archive and
clean detached checkout were source-synced and hash-verified on a provisioned
native Linux x86_64 builder on 2026-09-29; this does not mean that the kernel,
product, rootfs, or boot artifacts are complete. Their manifests remain the
authority for each stage, and no physical boot or radio capability is claimed.
Native Linux x86_64 is still required. For the current Pixel 9a objective, use
the remote `deba` build machine for Docker, UTM, QEMU, kernel and radio-kernel
builds, product builds, and postmarketOS rootfs construction:

```sh
ssh -i ~/.ssh/id_ed25519 ezy@100.95.82.97
```

Use `/home/ezy/orca/workspaces/omarchy-ferrix/remora` when it is the convenient
checked-out workspace, and synchronize reviewed repository commits through Git
instead of copying ignored caches, firmware, images, or device evidence. Keep
the local Mac exclusively for the Pixel's ADB/Fastboot
operations, device logs, flashing, rebooting, and hardware validation; never
expose its USB transport or ADB server socket to `deba`. If `deba` is
unavailable, stop the physical build handoff and report the blocker rather
than substituting the Mac, emulated UTM, or another unapproved host.

On macOS, run only the read-only `*-check`/`*-test` recipes that do not contact
the device, then hand the target build to `deba`. Do not use an ARM64 UTM
guest, an x86_64 UTM guest under CPU emulation, OrbStack/Lima, or a
cross-compilation substitute; the pinned native kernel source, toolchain, and
`pmbootstrap` are Linux-only by design. A digest-pinned `linux/amd64` tool
container is allowed only when its Docker engine runs on native Linux x86_64;
it cannot substitute for that host, and it must not carry the phone's USB
transport or ADB server.

Load the host profile first: `. tools/pixel-9a/env.sh`, then
`just pixel-9a-env-validate` and `just pixel-9a-env-check build`.

When the native builder is remote, transfer the checked-in source through the
canonical Git repository (for example, push with `gh`/Git and pull the reviewed
commit on the builder) rather than copying ignored caches, firmware, images, or
device evidence. The checked-in `tools/pixel-9a/native-builder.sh` wrapper may
then run only the device-free stages inside the pinned builder image:

```sh
tools/pixel-9a/native-builder.sh image
FERRIX_PIXEL9A_RADIO_SYNC=I-understand-this-downloads-the-radio-first-kernel \
  tools/pixel-9a/native-builder.sh run kernel-radio sync
FERRIX_PIXEL9A_RADIO_BUILD=I-understand-this-runs-the-radio-first-kernel-build \
  tools/pixel-9a/native-builder.sh run kernel-radio build
FERRIX_PIXEL9A_PRODUCT_BUILD=I-understand-this-builds-the-pixel-9a-product \
  tools/pixel-9a/native-builder.sh run product build
```

The wrapper requires Docker to report `linux/amd64` on a native Linux x86_64
engine, mounts only the checkout plus target-scoped ignored cache/output paths,
and refuses discovery, ADB, Fastboot, flashing, temporary boot, and guest-radio
SSH workflows. Source sync, radio-kernel build, and product build opt-ins are
separate authorizations; completing one stage does not authorize another.

For `rootfs build`, the checkout bind remains read-only while the wrapper's
target-scoped `.cache/ferrix/pixel-9a` and `out/pixel-9a` mounts are writable.
The pmbootstrap temporary workspace is created under the cache mount at
`rootfs.XXXXXX`, so a rootfs build does not try to write temporary state into
the read-only source checkout. `FERRIX_PIXEL9A_ROOTFS_CACHE_DIR` may override
that location only with another writable path inside the checkout; the wrapper
rejects external paths. Keep this cache ignored and separate from the product,
kernel, firmware, and final rootfs output directories.

The pinned kernel and pmaports source checkouts must be clean before they are
used: no tracked, staged, or untracked changes. The main Ferrix repository
worktree may contain the owner's staged or unstaged edits; do not reset, clean,
or otherwise discard them to satisfy an external-source cleanliness gate.
Keep generated artifacts, firmware, and physical-device evidence in the
ignored output paths.

The selected radio-first route is a separately pinned native Kbuild tree. It
is the only route in this playbook that pairs the `s5300_modem` WWAN driver,
the SIT userspace packages, and the `brcmfmac` firmware-name aliases. The
canonical Google Kleaf route remains useful for comparison, but its CPIF,
module, DTB/DTBO, and firmware contracts are incompatible with the radio-first
route. Use one route end to end.

Chain, in order (each opt-in token needs the human's OK in this session):

```sh
just pixel-9a-kernel-radio-check
FERRIX_PIXEL9A_RADIO_SYNC=I-understand-this-downloads-the-radio-first-kernel \
  just pixel-9a-kernel-radio-sync
just pixel-9a-kernel-radio-verify
FERRIX_PIXEL9A_RADIO_BUILD=I-understand-this-runs-the-radio-first-kernel-build \
  just pixel-9a-kernel-radio-build
just pixel-9a-product-check
FERRIX_PIXEL9A_PRODUCT_BUILD=I-understand-this-builds-the-pixel-9a-product \
  just pixel-9a-product-build
just pixel-9a-firmware-check
just pixel-9a-firmware-inventory
FERRIX_PIXEL9A_FIRMWARE=I-understand-this-extracts-verified-firmware \
  FERRIX_PIXEL9A_FIRMWARE_ALLOWLIST=<reviewed TSV> just pixel-9a-firmware-stage
just pixel-9a-rootfs-check
FERRIX_PIXEL9A_RADIO_KERNEL_DIST=out/pixel-9a/kernel-radio/tegu-wwan/dist \
  FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST=out/pixel-9a/kernel-radio/tegu-wwan/build-manifest.tsv \
  FERRIX_PIXEL9A_PRODUCT_DIR=out/pixel-9a/product/tegu \
  FERRIX_PIXEL9A_FIRMWARE_DIR=out/pixel-9a/firmware/tegu/staged \
  just pixel-9a-rootfs-validate
FERRIX_PIXEL9A_ROOTFS_BUILD=I-understand-this-runs-pmbootstrap \
  FERRIX_PIXEL9A_PMAPORTS_DIR=<pinned pmaports checkout> \
  FERRIX_PIXEL9A_RADIO_KERNEL_DIST=out/pixel-9a/kernel-radio/tegu-wwan/dist \
  FERRIX_PIXEL9A_RADIO_KERNEL_MANIFEST=out/pixel-9a/kernel-radio/tegu-wwan/build-manifest.tsv \
  FERRIX_PIXEL9A_PRODUCT_DIR=out/pixel-9a/product/tegu \
  FERRIX_PIXEL9A_FIRMWARE_DIR=out/pixel-9a/firmware/tegu/staged \
  just pixel-9a-rootfs-build
just pixel-9a-boot-radio-check
FERRIX_PIXEL9A_RADIO_BOOT_BUILD=I-understand-this-composes-pixel-9a-radio-first-boot-artifacts \
  FERRIX_PIXEL9A_INITRAMFS=<cpio> FERRIX_PIXEL9A_VENDOR_RAMDISK=<cpio> \
  FERRIX_PIXEL9A_VENDOR_BOOTCONFIG=<bootconfig> \
  just pixel-9a-boot-radio-build
```

The radio sync step downloads the pinned source archive before creating the
checkout and verifies its recorded byte count plus SHA-256 and SHA-512. The
archive and checkout stay under the target cache; a missing or mismatched
archive fails closed before Kbuild runs.

When the exact-target redacted partition-capacity handoff and source-built
`vendor_kernel_boot`, DTBO, and module inputs are available, run
`just pixel-9a-radio-split-check` followed by the explicitly confirmed
`just pixel-9a-radio-split-build`. The split composer re-hashes those inputs,
requires all ten reviewed partition capacities, and never contacts a device or
reuses stock modules. Its manifest is a handoff only: `partition_contract` is
`complete-source-built`, while `status=composed-not-booted` and
`bootable=false` remain mandatory. AVB metadata is generated only after the
final split images exist: native `avbtool add_hash_footer` creates salted
descriptors from those exact bytes, and `make_vbmeta_image` includes those
descriptors. External or stock vbmeta inputs are rejected. A physical boot
route still requires independent target, recovery, and owner gates.
The read-only capacity workflow's output can be supplied as
`FERRIX_PIXEL9A_RADIO_PARTITION_SIZES=out/pixel-9a/partition-capacities-latest.tsv`
after its exact-target transcript has been reviewed.

The radio-first outputs (all ignored) land under
`out/pixel-9a/kernel-radio/tegu-wwan/`,
`out/pixel-9a/firmware/tegu/`, `out/pixel-9a/product/tegu/`,
`out/pixel-9a/rootfs/tegu/`, and `out/pixel-9a/images/tegu-wwan/` with
manifests; the optional completed split handoff is under
`out/pixel-9a/images/tegu-wwan-split/`. The firmware stage starts from the
official factory archive and
the checked-in allowlist; `firmware-radio-route.tsv` then records the explicit
copy/rename to the candidate `brcmfmac4383-pcie.*` names. For this factory
build, keep the BCM4383 `_4383_a3` files under their full suffixed runtime
names in `staged/` and in the firmware manifest; do not normalize those names
away. The A3 calibration, CLM, and TXCAP sources also populate the unsuffixed
`bcmdhd` paths, while the `brcmfmac` aliases must source the suffixed A3
firmware, calibration, CLM, and TXCAP entries (not their generic counterparts).
Keep both name sets in the reviewed runtime allowlist and route contract. No
firmware bytes, images, or device evidence may enter Git.

The build-time firmware manifest is not runtime evidence. In the guest,
`ferrix-radio-firmware` must pass before NetworkManager, BlueZ, the S5300
mount/loader chain, ModemManager, or Ferrix adapters start. It re-validates the
target/schema rows, validates every manifest destination against the reviewed
firmware allowlist (including descendants of the explicitly allow-listed modem
configuration directories), rechecks every allow-listed file's size and
SHA-256, rejects missing or symlinked files, and verifies all four reviewed
radio-first aliases exactly once each (duplicate or missing alias pairs are
rejected). A missing, changed, unreviewed, or malformed manifest
fails closed; successful staging or a passing host test never proves that a
physical controller loaded.

The radio-first kernel manifest is `built-not-booted` / `bootable false`; the
radio-first boot manifest is `composed-not-booted` / `bootable false`. The
current `pixel-9a-boot-radio-*` composer emits `Image`, `dtb.img`,
`boot.img`, `init_boot.img`, `vendor_boot.img`, `rootfs.tar`, `rootfs.img`,
and `vbmeta.img`, but not the complete `vendor_kernel_boot`, `dtbo`,
`vendor_dlkm`, `system_dlkm`, and slot-specific artifact manifest required by
the physical route. The separate `pixel-9a-radio-split-*` composer fills those
source-built split artifacts when its exact capacity and descriptor handoffs
are present. Its `images/tegu-wwan-split/` manifest remains
`composed-not-booted` / `bootable=false`; it may be consumed only by the
checked-in `pixel-9a-flash-radio-*` adapter after its independent preflight.
Do not boot or flash the handoff by hand. The target's canonical Google
`pixel-9a-boot-*`, `pixel-9a-artifacts-*`, and `pixel-9a-flash-*` workflows
continue to reject both radio-first backends rather than mixing contracts.

The canonical `pixel-9a-artifacts-manifest` route has its own hard gate: it
accepts only exact Pixel 9a Google Kleaf kernel metadata
(`schema_version=1`, `build_system=google-kleaf-bazel`, `kernel_package=@//aosp`),
the exact postmarketOS rootfs identity/substrate, and a header-v4
`composed-not-booted` boot manifest with a non-empty AVB mode. It rejects
unsafe or traversing artifact paths and rechecks every listed size and
SHA-256 before writing its manifest. These checks validate provenance; they do
not turn a manifest into boot or physical radio evidence.

The separate Google Kleaf comparison chain remains available as
`pixel-9a-kernel-check/sync/verify/build` plus `pixel-9a-boot-check/build`,
but it must consume only its own kernel-sources, modules, and rootfs manifest.
Never mix stock Android modules with either Ferrix kernel
(`image-layout.toml` policy).

The target's GrapheneOS source-correlation record may corroborate module names,
device-tree wiring, and load ordering, but the authoritative inputs remain the
pinned Google sources in `kernel-sources.toml`. Do not copy GrapheneOS/AOSP
prebuilts, vendor blobs, Android HAL/RIL/Binder services, or SELinux policy
into the native Linux guest.

## Phase 6 - Flash

**Status:** Canonical Google route and the separately gated radio-first route
are Implemented, Stub-tested only (`just pixel-9a-flash-test` and
`just pixel-9a-flash-radio-test`). Neither has run on hardware;
`boot_status = "not-attempted"`.

The canonical `pixel-9a-flash-*` workflow consumes only the Google Kleaf
split-boot manifest. It continues to refuse both radio-first backends. Never
rename or copy radio-first artifacts into a canonical manifest to get past
this boundary. The complete radio-first handoff has its own
`pixel-9a-flash-radio-*` adapter below.

Every mode re-hashes all composed artifacts against `boot-manifest.tsv`,
verifies the pinned factory archive (refuses if absent: no restore path, no
flash), and requires exactly one Fastboot device with `product=tegu`,
`variant=GXQ96` (the bootloader SKU invariant), `hw-revision=MP1.0`,
`unlocked=yes`, `slot-count=2`.

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
5. Rootfs stage (`--stage rootfs` or `all`) remains fail-closed for the current
   target: although the native rootfs build emits a verified `rootfs.img`,
   `deviceinfo_flash_fastboot_partition_rootfs` is intentionally unset until
   the exact live partition mapping is reviewed. It also requires a complete,
   redacted read-only ADB partition inventory. `rootfs.tar` is only a handoff
   archive and is never a partition image; do not build a partition mapping ad
   hoc or flash the archive.
6. Slot: read-only preflight may display the current slot, but a persistent
   flash requires an explicit `--slot a|b` and refuses the active slot. The
   flash workflow never selects a slot and prints a `slot_warning`. After a
   successful radio-first flash, the separate checked-in
   `just pixel-9a-boot-inactive --preflight --slot <a|b>` then
   `--confirm-boot` workflow is the only route allowed to select the already-
   flashed inactive slot and reboot it. It rechecks the same device and all
   image/recovery evidence; its result is a boot request, not boot evidence.
   Do not infer a slot from a default or type raw `set_active`.
7. Result semantics: success prints `result=flashed-not-booted`. It is not
   boot evidence; `bootable` stays false until a Linux boot is observed.
   Transcript `out/pixel-9a/flash/flash-<stamp>-<pid>.txt`.
8. `--skip-temp-boot-gate` changes the phrase to end in
   `WITHOUT TEMP BOOT GATE`. Owner-directed debugging only.

### Radio-first split route (`flash-radio.sh`)

The separately checked-in route consumes only the exact
`out/pixel-9a/images/tegu-wwan-split/boot-manifest.tsv` produced by
`pixel-9a-radio-split-build`. It is a source-built WWAN/SIT route, not a
Google Kleaf/CPIF route, and it never flashes bootloader or radio firmware.
The route is Implemented and Stub-tested only; no physical boot, flash, or
radio evidence exists yet.

On a transition from an identity-verified bootloader in the same process,
this route may recognize Google's `MSM USF` Fastbootd placeholder only after
rechecking userspace mode, serial, product, revision, slots, unlock/secure
state, and complete same-serial ADB evidence of `GXQ96`. An old ADB transcript
alone is insufficient: the same-session bootloader anchor is mandatory.
The placeholder remains forbidden in bootloader mode; shared SKU helpers
for restoration, unlock, and activation are unchanged.

1. Read-only preflight:
   `just pixel-9a-flash-radio-check [--stage kernel|rootfs|all] [--slot a|b]
   [--allow-unsigned-avb]`. It verifies the radio route and split-contract
   hashes, all 14 artifact rows and seven fresh AVB bindings, the exact ten
   partition capacities, source handoff manifests, the pinned recovery
   artifact, and exact Fastboot identity (`tegu`/`GXQ96`/`MP1.0`, unlocked,
   two slots). It refuses ADB-present or multiple-device states.
2. AVB: the route requires `--allow-unsigned-avb` when the fresh AVB
   manifest says `avb_algorithm=NONE` and Fastboot reports `secure=yes`.
   Explain that unsigned images are usable only with the bootloader unlocked;
   never relock this device. Signed modes still require the reviewed key and
   fresh descriptor provenance from the source-built split composer.
3. Temporary boot first:
   `just pixel-9a-flash-radio-boot-temp [--allow-unsigned-avb]`. The typed
   phrase is `CONFIRM PIXEL 9A RADIO TEMPORARY BOOT boot.img@<first 8 hex>`.
   The workflow re-checks identity and recovery immediately before
   `fastboot boot`, writes a serial-redacted transcript under
   `out/pixel-9a/flash-radio/`, and records `result=temporary-boot-issued`.
   Header-v4 temporary boot still uses the device's currently flashed
   `vendor_boot`, `init_boot`, and `dtbo`; observe the kernel and stop on a
   hang or loop.
4. Persistent kernel stage: after reviewing the temporary-boot transcript,
   set `FERRIX_PIXEL9A_RADIO_TEMP_BOOT_EVIDENCE` to that ignored path and run
   `just pixel-9a-flash-radio --stage kernel --slot <a|b>
   [--allow-unsigned-avb]`. The slot is mandatory and must be inactive; the
   flash workflow never changes the selected slot. The typed phrase is
   `CONFIRM PIXEL 9A RADIO FLASH KERNEL STAGE TO SLOT <A|B>`. The explicit-slot
   preflight checks both slots' live sizes against the reviewed handoff and
   refuses any allocation-size change. After confirmation, an initial
   Fastbootd round trip checks identity, logical classification, sizes, and
   `snapshot-update-status=none` before any image write. It then writes the
   eight inactive bootloader-mode partitions, enters Fastbootd for
   `vendor_dlkm` and `system_dlkm`, returns to bootloader, and re-checks the
   exact device, image hash, and live capacity before every write. These are
   four transport reboots, not slot selection. A failed initial round trip
   can leave the phone in Fastbootd with no images written; later failures
   can leave a partially flashed inactive slot. Success means only
   `result=flashed-not-booted`.
5. Inactive-slot activation/boot: after reviewing the flash transcript, set
   `FERRIX_PIXEL9A_RADIO_FLASH_EVIDENCE` to that ignored path and run
   `just pixel-9a-boot-inactive --preflight --slot <a|b>
   [--allow-unsigned-avb]`. With the owner-authorized data-policy token and
   the same flags, run `--confirm-boot`. This is the only checked-in route
   allowed to issue the reviewed slot-selection/reboot sequence; it rechecks
   all ten flashed hashes, both recovery hashes, the exact serial and
   inactive-slot relationship, and reports only
   `result=slot-activated-boot-requested`.
6. Rootfs stage (`--stage rootfs` or `all`) additionally requires the
   complete, redacted exact-target ADB partition inventory in
   `FERRIX_PIXEL9A_PARTITION_INVENTORY`, including the reviewed `userdata`
   mapping. `rootfs.tar` is never flashed; only the hash-verified
   `rootfs.img` is sent to the allow-listed target partition. The route does
   not invent a mapping when this gate is absent.
7. The route never unlocks/locks, erases, formats, wipes, changes slots,
   flashes bootloader/radio firmware, or accepts the canonical Google
   manifest. `--skip-temp-boot-gate` is an owner-directed exception only;
   explain the risk and require the phrase ending `WITHOUT TEMP BOOT GATE`.

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
   `google,ZUMA PRO TEGU`); override only after a new pinned target source
   revision is reviewed and its exact device-tree string is recorded.
   `linux,dummy-virt` (QEMU) is refused.
2. `just pixel-9a-radios-inventory` (read-only): `rfkill`, NetworkManager,
   BlueZ, ModemManager, modules, `dmesg` firmware lines, the guest
   `firmware-manifest.tsv` with per-file size/SHA-256 verification, OpenRC
   service state, and the Ferrix adapter path
   (`ferrix-networkd` readiness record or socket reply; `ferrix-core` socket
   state). For the selected radio-first backend, the inventory must also
   distinguish the candidate `brcmfmac`/`btbcm` drivers from the canonical
   Google `bcmdhd` path and record the WWAN/SIT readiness surface (`s5300`
   driver, `/dev/wwan0sit*`/AT ports, `rmnet*`, and ModemManager D-Bus).
   Writes `out/pixel-9a/radios/evidence-latest.tsv` with columns `subsystem
   status progress evidence`.
   It also runs a read-only associated-radio probe for GNSS/KEPLER, NFC/ST21,
   ST54, and UWB sysfs/device-node surfaces. Those rows remain unsupported and
   are written separately to `associated-evidence-latest.tsv`; use
   `just pixel-9a-radios-associated` for the focused probe without the broad
   NetworkManager/BlueZ inventory.
3. `just pixel-9a-radios-enable [wifi|bluetooth|modem|all]`: phrase
   `CONFIRM PIXEL 9A ENABLE RADIOS: <list>`; then `rfkill unblock`, `nmcli
   radio wifi on`, `bluetoothctl power on`, `mmcli -m any --enable`. Modem is
   skipped with a printed note when `mmcli -L` lists none. Changes radio
   state only, never storage.
4. `just pixel-9a-radios-connect-wifi <SSID>`: PSK from
   `FERRIX_PIXEL9A_WIFI_PSK` or a no-echo prompt, piped to `nmcli --ask` over
   stdin; it never appears in argv, history, chat, or transcripts. SSID is
   stored only as a hash. Result `wifi_connected=yes|no`.
5. Cellular data operations use the typed `ferrix-telephonyd` requests
   `connect-data` and `disconnect-data` under the separate `telephony-data`
   grant; do not substitute an ad-hoc ModemManager or AT command. The host
   radio workflow does not bypass appd authorization from an SSH shell: issue
   these requests through an authorized Ferrix Settings/app client, and use
   the workflow's inventory only to record the resulting state and bearer
   evidence. The
   Samsung-SIT backend requires an APN. An explicit `DataProfile.apn` wins;
   otherwise the non-secret `FERRIX_TELEPHONY_APN` target value is used as a
   daemon-only fallback. The checked-in target leaves it empty rather than
   guessing a carrier profile, and malformed APNs fail closed. The backend
   calls ModemManager `Modem.Simple.Connect`/`Disconnect`; data evidence must
   include the resulting bearer, `rmnet*` interface, and Ferrix adapter/service
   readiness in addition to modem/SIM/registration checks. An APN accepted by
   the backend or modem enumeration alone is not evidence; APNs and
   credentials do not belong in evidence records.
6. The guest's `ferrix-s5300-mounts` service reads exactly one
   `androidboot.slot_suffix` (`_a` or `_b`) and maps it to the matching
   `modem_a` or `modem_b` partition. It mounts that modem image plus `efs`,
   `persist`, and `modem_userdata` read-only, and refuses a missing, duplicate,
   unknown, foreign, or writable source; it never guesses `_a` for slot B.
   `exynos-modem` consequently consumes only the current-slot `modem.bin` and
   exact read-only calibration inputs. This pairing is a runtime safety gate,
   not physical radio evidence; persistent flash still requires the explicit
   inactive-slot rule in Phase 6.
7. `just pixel-9a-radios-evidence`: runs inventory, prints a
   `capabilities.toml` fragment labelled
   `physical: google-pixel-9a-tegu-gxq96-mp1-0`. The OWNER pastes it; the
   workflow never edits `capabilities.toml` and neither do you.
8. Vocabulary: `status` is only `supported` or `unsupported`
   (`CapabilityStatus` has no `Partial`; `mocked` is never used for a
   physical target). Nuance lives in `progress` and `evidence`. `supported`
   requires the Ferrix adapter path observed, not just Linux tools working.
9. Physical evidence boundary: source correlations, offline manifests,
   native builds, firmware staging/runtime-gate success, driver or CP ONLINE
   lines, modem enumeration, and host/stub tests are prerequisites or
   implementation evidence only. A physical capability requires the exact
   target guest boot plus the Linux service and Ferrix adapter path observed
   repeatedly in redacted records; QEMU/mock output never promotes it. With
   the checked-in target baseline, telephony remains `unsupported`: the
   Google Kleaf Samsung S5300 CPIF modem exposes no ModemManager modem because
   its `cpif`/`umts_ipc*` ABI is not the WWAN SIT ABI expected by the reviewed
   loader/plugin. The radio-first backend pins the community `s5300_modem`
   WWAN driver so that package boundary can be tested as one coherent kernel
   and userspace set. The Rust `ferrix-s5300-cpif` crate remains only a SIPC5
   frame codec, while the reviewed external loader/plugin is a target-local
   package boundary.
   The reviewed `exynos-modem` and SIT ModemManager sources are now fetched by
   target-local, source-pinned native-Linux package recipes; they remain outside
   Ferrix Rust crates and are not physical evidence. Runtime adoption still
   requires a successful native build, exact factory carrierconfig/UECAP
   inputs, exact-target runtime evidence, and a public ModemManager D-Bus
   readiness result. The route must not import Android RIL/HAL/Binder/SELinux/
   vendor services. Modem enumeration alone is not phone support. See
   `references/radios-and-evidence.md`.

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
- Target source-correlation records (including the S5300 external-source and
  GrapheneOS records): update only reviewed pins, licensing boundaries, and
  implementation limits; they never promote a physical capability.
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
| Flash refuses without temp-boot evidence | Run the matching route's `pixel-9a-flash-boot-temp` or `pixel-9a-flash-radio-boot-temp` first; never use `--skip-temp-boot-gate` on your own |
| Device stays in Fastboot after unlock | Expected; next step is stock restore or a gated boot, decided by owner |
| Development needs ordinary bootloader Fastboot while Android is running | Use `just pixel-9a-enter-fastboot --preflight`, then `--confirm-enter`; do not require hard buttons or type raw `adb reboot bootloader` |
| Temp boot hangs or loops | Power-cycle per owner instruction; restore stock via Phase 4; record as failed boot evidence |
| Radio-first flash refuses a canonical manifest | Keep the route separation; feed only `images/tegu-wwan-split/boot-manifest.tsv` to `pixel-9a-flash-radio-*` and only the Google Kleaf manifest to `pixel-9a-flash-*` |
| Radio-first flash refuses rootfs inventory | Stop; obtain a complete redacted exact-target ADB partition inventory and do not invent a `userdata` mapping |
| radios exit `3`/`4`/`5` | Wrong host, wrong device tree, or a QEMU guest; you are not talking to the phone; stop |
| External modem source/archive/license metadata changes or cannot be verified | Fail the target-local package build; do not fetch, build, or package the source until the exact native-Linux source and license gates are restored |
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
- `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/s5300-source-evidence.md`
  and `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/grapheneos-radio-correlation.md`
  - reviewed source boundaries and reuse limits; neither is physical radio
  evidence.
