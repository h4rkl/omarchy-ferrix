# Troubleshooting

Every fix below is read-only or uses a checked-in workflow. If the fix you
are tempted to try is a raw `adb`/`fastboot` verb that changes state, stop
and ask the owner instead.

## Nothing enumerates (discover exit 2)

- Cable: many USB-C cables are charge-only. Try a known data cable and a
  direct port (no hub, no dock).
- Android side: Developer options -> USB debugging on; screen unlocked; USB
  mode set to file transfer or "USB debugging" rather than charge-only;
  accept the RSA prompt when it appears.
- Fastboot side: power off, then hold the bootloader key combination the
  vendor documents (Pixel: Power + Volume Down) until the bootloader screen
  shows.
- macOS: no driver is needed for ADB/Fastboot; `system_profiler
  SPUSBDataType` (read-only) shows whether the phone enumerated at all. If
  the phone shows in USB but not in `adb devices -l`, restart the ADB server
  (`adb kill-server` is host-only and allowed).
- Linux: udev rules or the `plugdev` group may be missing; `adb devices -l`
  shows `no permissions`. Fix the udev rule on the host (owner's call,
  host-only); do not run `adb` as root.
- After host tool installation, `just pixel-9a-tools-check` reports the
  `adb`/`fastboot` versions. Installation itself is opt-in (Homebrew cask
  `android-platform-tools` on macOS per the recipe's hint).

## Ambiguous (discover exit 3)

More than one ADB or Fastboot device is visible, or one phone shows on both
transports during a reboot. Detach other phones, stop emulators, wait for a
reboot to settle, re-run. Never disambiguate with `-s` by hand.

## ADB unauthorized (discover exit 4)

`adb devices -l` shows `unauthorized` or `offline`. The human must accept the
RSA prompt on the phone (and may tick "always allow"). If no prompt
appears, revoke USB debugging authorizations in Developer options and
reconnect. `offline` after a reboot usually clears within seconds.

## Fastboot vs fastbootd and `is-userspace`

- The bootloader's Fastboot (`is-userspace=no`) flashes bootloader, radio,
  `boot`, `dtbo`, `vbmeta*`, `vendor_boot`, `init_boot`, `vendor_kernel_boot`.
- fastbootd (`is-userspace=yes`) is a userspace Fastboot inside recovery that
  handles dynamic (`super`) partitions such as `vendor_dlkm`, `system_dlkm`,
  `system`, `vendor`, `product`.
- The workflows hop between them and check `is-userspace` first; the factory
  restore requires `is-userspace=no`. If a gate refuses on `is-userspace`,
  the phone is in the wrong mode. Report; let the human use the phone's own
  menu or the owner decide. Do not run `fastboot reboot fastboot` or
  `fastboot reboot bootloader` by hand.

## Slot confusion

- ADB reports `ro.boot.slot_suffix=_a`; Fastboot reports `current-slot: a`.
  They are the same slot. Workflows normalize both forms.
- Fastboot must report `slot-count=2` on the Pixel 9a. Android may expose an
  empty `ro.boot.slot_count`; `pixel-9a-partition-inventory` then accepts `2`
  only when a read-only `/dev/block/by-name` listing proves both A/B suffixes
  and records `slot_count_source=block-by-name`. If neither source proves two
  slots, the workflow dies rather than guessing.
- The same inventory can finish with `inventory_completeness=partial` when
  Android denies or omits `/proc/partitions` or `/proc/cmdline`. That is usable
  as topology evidence, but rootfs flashing requires `complete`; obtain a
  complete read-only guest inventory before that stage.
- Never change the active slot by hand. A first Ferrix flash must not
  overwrite the active stock slot without the owner's approved inactive-slot
  plan (`linux-plan.md`). State the slot in every checklist.

## Hash or size mismatch

- Recovery archive: a partial download leaves a `.part`; re-run the download
  recipe once. A complete file with a different hash means the vendor
  republished or the pin is wrong: do not edit the pin to match the file;
  report to the owner with the vendor page.
- Build manifests (`build-manifest.tsv`, `manifest.tsv`, `boot-manifest.tsv`):
  a mismatch means an artifact was rebuilt or touched; rebuild into a fresh
  output directory (the workflows refuse to overwrite) rather than editing a
  manifest.

## Device stays in Fastboot after unlock

Expected. `unlock-bootloader.sh` leaves the phone in Fastboot on purpose so
the verified factory archive can be restored. The unlock erased user data.
Next steps are the owner's decision: factory restore (Phase 4) to return to
stock Android, or, once a complete checked-in Ferrix route passes every gate,
a temporary Ferrix boot (Phase 6). The current radio-first composer is only an
unbooted handoff and has no approved temporary-boot or persistent-flash route.
`fastboot reboot` by hand is not permitted; the restore and flash workflows
reboot as part of their confirmed sequence.

## Getting back to stock

| State | Route |
| --- | --- |
| Stock Android boots, bootloader locked or unlocked | Full OTA sideload (`just pixel-9a-sideload-ota`, Stub-tested only): no wipe, no unlock needed |
| Stock recovery reachable, Android broken | Full OTA sideload |
| Unlocked Fastboot, Android or recovery broken, or Ferrix images present | Factory restore (`just pixel-9a-factory-restore`, Stub-tested only; requires `just pixel-9a-factory-inspect` first): wipe unless `--keep-data` |
| Locked bootloader and no working recovery | Host workflows cannot help; owner and vendor service |
| No USB enumeration at all | Hardware/cable/bootloader fault; owner decision |

Both routes verify the archive hashes against `recovery.pins` before touching
the device, and neither has run on hardware yet
(`recovery_procedure = "documented-not-tested"`). If the restore stops between
stages with `REFUSING:` because `version-bootloader`/`version-baseband` do not
match the archive's `require` lines, the phone is still in Fastboot; report,
and re-run only with a fresh go-ahead (bootloader/radio flashing is
idempotent).

## Brick avoidance

- Never run `fastboot flashing lock` / `fastboot oem lock`. Relocking with
  non-stock or unsigned images (Ferrix `vbmeta*` are freshly generated, not
  Google-signed) makes the phone refuse to boot and can prevent recovery. No
  checked-in workflow relocks; do not add one without owner approval and a
  tested stock restore.
- Never flash bootloader or radio partitions outside the factory-restore
  sequence, and never from a different build than the pinned archive.
- Never mix stock Android kernel modules with a Ferrix kernel.
- Never `fastboot erase`/`format` anything; no workflow does.
- Prefer `fastboot boot` (temporary) before any persistent write; the flash
  workflow enforces this with the temp-boot evidence file.

## SSH to the booted guest fails (Phase 7)

- The USB network appears only after the guest brings up its gadget
  interface; check the host for a new interface first (read-only `ifconfig`
  or `ip addr`).
- Default guest address is `172.16.42.1` (`FERRIX_PIXEL9A_SSH_HOST`); user
  from `pmos/pmbootstrap.cfg` (`FERRIX_PIXEL9A_SSH_USER`), port 22
  (`FERRIX_PIXEL9A_SSH_PORT`), extra options in `FERRIX_PIXEL9A_SSH_OPTS`.
  All remote calls use `BatchMode=yes`, so key-based SSH must already work.
- Exit 3: unreachable. Exit 4: device tree does not contain
  `FERRIX_PIXEL9A_DT_COMPATIBLE` (default `google,ZUMA PRO TEGU`); override it
  only after a new pinned source revision is reviewed. Exit 5: a QEMU
  guest (`linux,dummy-virt`) is refused by design; you are connected to the
  wrong thing. Exit 6: confirmation phrase mismatch; nothing was changed.
- `connect-wifi` reads the PSK from `FERRIX_PIXEL9A_WIFI_PSK` or a no-echo
  prompt and pipes it to `nmcli --ask` over stdin; if it fails, do not retry
  by putting the PSK on a command line.

## Build host problems (Phase 5)

- `env.sh check build`, `kernel.sh preflight`, and the radio-first split/build
  preflights require native Linux x86_64. On Apple Silicon, an ARM64 UTM
  guest is the wrong architecture, an x86_64 UTM guest uses rejected CPU
  emulation, and OrbStack/Lima/Docker containers or a cross build are not
  native-host substitutes. Hand the Pixel build off to provisioned native
  Linux x86_64; UTM is still fine for a separate Linux smoke test or QEMU
  workflow.
- `repo` missing or under 80 GiB free: report; do not install or free disk on
  your own.
- `pmbootstrap` needs a clean pmaports checkout at the pinned commit
  (`FERRIX_PIXEL9A_PMAPORTS_DIR`); a dirty or wrong-commit checkout is
  refused.

## Artifact and firmware gates (Phase 5)

- The canonical `pixel-9a-artifacts-manifest` route accepts only the exact
  Google Kleaf kernel/rootfs contracts and a header-v4,
  `composed-not-booted` boot manifest with an explicit AVB mode. It rejects
  unsafe or traversing artifact paths and any size/SHA-256 mismatch. Do not
  edit a manifest to make it pass; rebuild into a fresh ignored output.
- The radio-first split composer is device-free and requires all ten
  exact-target partition capacities plus source-built DTBO/module inputs. It
  generates salted AVB descriptors from the final staged bytes and rejects
  external or stock vbmeta inputs. Its `complete-source-built` manifest is
  still not boot or flash authorization; `pixel-9a-flash-*` has no radio-first
  adapter and must refuse it.
- If `ferrix-radio-firmware` refuses, stop before NetworkManager, BlueZ,
  ModemManager, or Ferrix adapters start. The runtime gate checks every
  manifest destination against the reviewed allowlist (including descendants
  of the modem configuration directories), file size/SHA-256, regular-file
  status, and all four radio aliases. Missing, changed, unreviewed, or
  symlinked rows are a fail-closed condition, not a reason to bypass the gate.

## When to stop and ask the owner

- Any gate refusal, identity mismatch, or hash mismatch.
- `match=none` or a different hardware revision than a checked-in target.
- Backup status unknown or not verified.
- Any request that needs `--skip-*`, `--allow-*`, relock, slot change,
  erase, or a raw device command.
- Any evidence that would move a capability status, before editing
  `capabilities.toml`.
- Anything that would place a serial, IMEI, MAC, SSID, key, firmware, or
  image under version control.
