# ADR-012: Updates, rollback and the user-data layout

Status: proposed 2026-11-25 for spec 13
([`plans/todos/13-updates-recovery.md`](../../plans/todos/13-updates-recovery.md)),
issue #30. Owner review required. The release layout, signing-key
ownership and any change to which partitions a workflow writes are
owner-gated. Subordinate to `plans/baseline.md` and the recovery invariant
in `AGENTS.md`. Nothing here is implemented. The layout section is read
from the checked-in workflows. It is not new physical evidence.

## Current dev layout (`google-pixel-9a-tegu-gxq96-mp1-0`)

Sources:

- `tools/pixel-9a/dev-compose.py`
- `tools/pixel-9a/dev-flash.sh`
- `skills/ferrix-dev-loop/scripts/reflash.sh`
- `tools/pixel-9a/factory-restore.sh`
- `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/initramfs-init-kleaf`
- `.../boot-layout.toml`
- `skills/mobile-device-provisioning/SKILL.md`

| Partition | Holds | Written by dev flash | Written by factory restore |
| --- | --- | --- | --- |
| bootloader, radio (slot a and b) | Pinned stock | Slot b only, pinned stock, before `set-active b` | Yes (current slot) |
| `boot_b` | Kleaf GKI kernel | Yes | Yes, if b is current |
| `init_boot_b` | Ferrix initramfs (`/init`) | Yes | Yes, if b is current |
| `vendor_boot_b` | Ferrix cmdline and bootconfig, empty ramdisk. Its tail (48/56 MiB) is the flash logger's pstore/dmesg area | Yes | Yes, if b is current |
| `vendor_kernel_boot_b`, `dtbo_b` | Kleaf output | Yes | Yes, if b is current |
| `vbmeta_b` | `--algorithm NONE --flags 2` (verification disabled) | Yes | Yes, if b is current |
| `vbmeta_system_b`, `vbmeta_vendor_b` | Stock | No (allowed, unused) | Yes, if b is current |
| slot a boot set | Stock Android | No | Yes, if a is current |
| `super` (`system`, `vendor`, `vendor_dlkm`, `system_dlkm`, …) | Stock | **Refused** | Yes |
| `userdata` | **Ferrix rootfs, raw ext4** (fs about 8 GiB of about 117.5 GB) | Yes, sparse image, whole partition | Wiped (`-w`) unless `--keep-data` |
| `metadata` | Stock-format | No (allowed, unused) | Wiped (`-w`) |
| `persist`, `efs`, `devinfo`, `frp`, `misc`, `pvmfw` | Device state | **Refused** | No |

Inside `userdata` today there is no boundary between system and data:

- the OS (`/usr`, `/etc`);
- `system_dlkm` and `vendor_dlkm` modules in `/usr/lib/modules/<release>`;
- firmware;
- Ferrix binaries;
- the Waydroid images (`/etc/waydroid-extra/images/{system,vendor}.img`);
- Waydroid state (`/var/lib/waydroid`);
- Android `/data` (`/home/user/.local/share/waydroid/data`);
- home;
- `/var/lib/ferrix` (contacts, SMS, screen);
- NetworkManager profiles.

**What each action destroys:**

- **Dev flash of `userdata`:** everything, including Bluetooth pairings,
  Wi-Fi profiles, Android app data and home.
- **Dev flash of boot partitions only:** nothing in `userdata`. The kernel
  can then mismatch the modules in the rootfs (see coupling).
- **Factory restore (default):** everything in `userdata` and `metadata`.
  Slot b ends stock.
- **Factory restore `--keep-data`:** stock Android then meets a Linux ext4
  `userdata`. Android is not expected to use it (unverified).
  Treat this as data loss for Ferrix purposes.

**Coupling:**

- **Kernel and modules.** The kernel (`boot_b`, `vendor_kernel_boot_b`,
  `dtbo_b`) must match the modules in the rootfs by `kernel_release`.
- **Mali.** The GPU userspace (Mali overlay in the rootfs) must match the
  kernel driver.
- **Android graphics.** The Waydroid LineageOS image must match the
  hwcomposer patches and the Mali/Codec2 overlays.
- **Firmware** is in the rootfs and pinned by the firmware manifest. It is
  never fetched.

**How a boot falls back:** on a mount or boot failure, `/init` holds or
reboots so that the bootloader falls back to stock slot a. There is no
boot counting and nothing marks slot b successful (`reflash.sh` prints
"slot b not marked successful").

**What the manifest records:** the dev-compose `manifest.tsv` lists
SHA-256 for inputs and for each output image. It has no version and no
signature. Some inputs (`product_dir`, `vendor_cmdline`) are recorded
without a hash.

**Consequence:** the dev route has no update path that preserves data.
A/B rollback is impossible while the rootfs is the whole `userdata`
partition.

## Constraints

**Recovery invariant (`AGENTS.md`).** A layout may write only partitions
the factory image rewrites. It must leave stock restore possible and
tested, write no bootloader or radio images except pinned stock, issue no
relock, erase, format or `oem`, and put stock firmware on the target slot
before any slot switch. Partitions that qualify for Ferrix writes:

- the slotted boot set;
- the `vbmeta*` partitions;
- `userdata` and `metadata`, which the factory `-w` rewrites.

**Partitions that are forbidden:**

- `super`, `misc`, `devinfo`, `persist`, `efs` and `frp`;
- any write to the bootloader's A/B slot metadata, other than
  `fastboot set-active` from the checked-in workflow.

**Other constraints:**

- **Substrate.** Baseline: postmarketOS/pmbootstrap is the substrate.
  Generated images stay out of Git. Manifests keep pinned inputs and
  hashes.
- **Encryption.** ADR-011 recommends fscrypt (E2) on an ext4 data
  filesystem, which assumes a separate data filesystem that system
  updates never rewrite.
- **Honest reporting.** No boot failure may be reported as success
  (spec 13).

## Options

### U1. A/B system images inside `userdata`, with boot counting in `/init`

`userdata` becomes one ext4 data filesystem (`-O encrypt`, for ADR-011),
created once:

```text
/ferrix/system/a.erofs        read-only system image (rootfs + modules + firmware + Waydroid images)
/ferrix/system/b.erofs
/ferrix/system/state.json     {"current":"a","previous":"b","pending":null,"tries":0,"schema":1}
/ferrix/system/manifests/     signed release manifests for a and b
/ferrix/data/                 writable: home, /var/lib, /etc overlay upper, Waydroid state
```

**Boot sequence.** `/init` (in `init_boot_b`) does the following:

1. mount `userdata`;
2. read `state.json`;
3. if `pending` is set and `tries < 3`, increment `tries` (fsync), then
   boot `pending`;
4. if `tries` is exhausted, set `pending = null`, record
   `update-rollback`, and boot `current`;
5. verify the chosen image's hash against its manifest;
6. loop-mount it read-only as `/`;
7. bind or overlay the writable paths from `/ferrix/data`;
8. `switch_root`.

**Health check.** A bounded OpenRC service promotes `pending` to `current`
only after the health check passes.

**Kernel.** The kernel stays single, in `boot_b`. Each system image
declares the `kernel_release` it carries modules for. The updater refuses
an image whose `kernel_release` differs from the running kernel unless the
update is a kernel bundle (below).

- **For:**
  - **Invariant.** It writes only `userdata` for routine updates, which
    passes the invariant.
  - **Atomic.** An update is atomic at image granularity. Rollback is one
    field.
  - **Separation.** Data is separate from system, so ADR-011's fscrypt
    roots survive updates.
  - **Reproducible.** The system image is immutable and hash-verifiable.
    The existing pinned-manifest pipeline extends naturally: pmOS still
    builds the rootfs, and dev-compose packs it as erofs instead of
    `userdata.img`.
  - **Space.** It costs 2 × about 8 GiB of the 117.5 GB partition.
- **Against:**
  - **One-time wipe.** Moving to it reformats `userdata` once.
  - **Kernel updates are not A/B.** A kernel update still needs Fastboot
    (dev-flash of `boot_b`) or an owner-approved on-device write.
  - **erofs is unverified.** erofs and loop support in the Kleaf GKI are
    expected but unverified. squashfs or ext4-ro images are fallbacks.
  - **Image-based, not package-based.** Devices do not run `apk upgrade`.
    Package choice stays at build time in pmbootstrap, which is a
    departure from pmOS's usual device model.
  - **Initramfs logic.** The boot-counting logic lives in a shell
    `/init`. It must be small and host-tested (a pure Rust helper binary
    in the initramfs is an option).

### U2. pmOS package upgrades with filesystem snapshots

`userdata` becomes btrfs with `@root` and `@home` subvolumes.
`apk upgrade` runs on the live system after a read-only snapshot of
`@root`. Rollback sets the default subvolume back to the snapshot.

- **For:**
  - It is pmOS's own update model, the closest to "use the substrate".
  - Deltas are small.
  - No image pipeline change.
- **Against:**
  - **btrfs support.** btrfs is not in the Android GKI. The module would
    have to be built from the Kleaf tree and loaded from the initramfs.
    That is unverified, and it is a new kernel dependency.
  - **No fscrypt.** btrfs has no merged fscrypt support for this kernel,
    so ADR-011 would have to switch to LUKS2 (E1) with an initramfs
    unlock UI.
  - **Not atomic in place.** Upgrades are not atomic, and a power cut
    mid-`apk` leaves a half-upgraded root. The snapshot helps but does
    not remove that.
  - **No pinned release.** The system is mutable and drifts from any
    pinned manifest. "Release X" means a package-repository state, which
    is harder to sign as one unit and to reproduce.
  - **Kernel and modules.** Kernel/module coupling still needs a
    kernel-aware hook.

### U3. Slot-based kernel and boot images (use slot a as well)

Ferrix writes its boot set to both slots. Updates flip slots through the
bootloader's A/B retry counter.

- **For:** the bootloader does the boot counting, and kernel updates are
  A/B.
- **Against:**
  - **Fails the invariant as written.** Slot a is the stock fallback
    today.
  - **Slot metadata writes.** Marking a slot successful or adjusting
    retries from Linux writes the bootloader's slot metadata, which the
    dev route refuses and the factory image does not "rewrite" as a
    partition.
  - **Unknown store.** Where Pixel keeps that metadata (`misc`,
    `devinfo` or elsewhere) is unverified here.
  - **Stock firmware rule.** Each switch would need stock bootloader and
    radio on the target slot (already pinned).
  - **The rootfs problem remains.** It solves only the kernel half. The
    rootfs still needs U1 or U2.

### U4. Keep dev flashes and add only backup and restore

- **For:** no layout change.
- **Against:** every update destroys data and must be undone by a restore.
  There is no rollback and no unattended update. It fails spec 13's goal.
  It is listed only as the fallback while U1 is built.

## Recommendation

**U1**, with these rules:

1. **Boot counting and health.**
   - `tries` is limited to 3.
   - `/init` writes the counter before `switch_root`. The hardware
     watchdog module `/init` already loads covers hangs, but whether the
     watchdog reboots a hung Pixel must be measured.
   - Health check: `ferrix-boot-health` runs after `ferrix-product`, with
     a 180 s bound. It requires:
     - the Wayland socket with a connected client (the shell's Home
       surface);
     - an appd `hello`;
     - a `networkd` `hello` (the adapter is alive; connectivity is not
       required).
   - Only then does it promote `pending`.
   - On failure it records `boot-health status:failed` with the failing
     check and reboots. The next boot rolls back.
   - Settings > System update reports "Update to X failed; running Y" on
     the next boot. A rollback is never shown as success.
2. **Kernel bundle.** A release that changes the kernel ships a boot set
   plus a system image that carries both the old and new
   `/usr/lib/modules/<release>`. The boot set is written by the
   checked-in Fastboot workflow (`just pixel-9a-dev-flash`), not by the
   device.
   - That system image boots with either kernel, so a rollback of the
     system image never strands the kernel.
   - On-device writes to `boot_b` are a later owner decision. They would
     pass the invariant (factory-rewritten partitions), but they create a
     write path outside the checked-in host workflow.
3. **Release manifest.** One `release.json` per release:
   - schema version and release version;
   - `target_id` and `kernel_release`;
   - SHA-256 and size of each image;
   - the dev-compose input pins;
   - firmware manifest hash;
   - Waydroid build and pins;
   - minimum data-schema version;
   - the migrations it runs.
   It is signed with **minisign/Ed25519**, verified on device with a
   maintained crate (`minisign-verify`, MIT). The public key is in the
   system image. An update applies only if its signature verifies, its
   hashes match, and its `target_id` matches. No firmware or image is
   fetched unpinned. `/init` re-checks the image hash at boot. Adding
   dm-verity (a root hash in the signed manifest) is a later step.
4. **Migrations.**
   - Ferrix stores keep their own versioned migrations. Contacts already
     has v0 → v1 (`crates/ferrix-contactsd/src/migrate.rs`) and refuses
     newer schemas. SMS is at schema 1 (`ferrix-telephonyd/src/store.rs`).
     The screen state has no version yet and gets one.
   - **Snapshot before migrating.** Because data is shared between system
     slots, the new system first copies each store it will migrate to
     `<store>.pre-<release>`, then migrates.
   - **Restore on rollback.** On rollback, `/init`'s rollback record makes
     the old system restore those copies before its services start.
   - **Interruption.** An interrupted migration leaves the copy and a
     `.tmp` (existing atomic-rename pattern), so the next attempt starts
     clean.
   - **Waydroid.** Android `/data` is not migrated by Ferrix. Android does
     not support downgrade, so a LineageOS image change is forward-only.
     The updater snapshots Waydroid data (a copy, or an ext4 reflink if
     available) before an Android image change, and rollback restores it
     or refuses.
5. **Backup and restore.**
   - `ferrix-backup export` writes an `age`-encrypted (Rust `age` crate,
     MIT/Apache-2.0) tar of `/ferrix/data` (home, `/var/lib/ferrix/user`,
     Waydroid data, NM profiles) to a host over the USB link or to
     removable storage. Its passphrase is chosen at export.
   - `import` runs on a freshly created data filesystem.
   - This is the only path that survives a factory restore or a layout
     change. Every wipe path in the table above says so in its workflow
     output.
6. **Recovery path for an ordinary user.**
   - An automatic rollback covers a bad system image.
   - A bad boot set is covered by the bootloader's fallback to stock
     slot a (existing behaviour). After that, the user needs a host for
     `pixel-9a-dev-flash` or `pixel-9a-factory-restore`.
   - Stock restore stays tested. Re-run it after the layout change and
     record the result.
7. **Updater.** A small Rust `ferrix-updated` (download from a local or
   host-served channel, verify, write `/ferrix/system/<other>.erofs` as
   `.tmp` + fsync + rename, set `pending`). It is not RAUC or SWUpdate.
   Both are C, assume their own bootloader integration (U-Boot, GRUB,
   EFI) or a custom backend script, and we already own `/init`. This is a
   documented hand-roll reason. It is revisited if RAUC's `custom`
   backend proves a better fit (owner decision).

### Interaction with ADR-011

- **U1 + fscrypt is the recommended pair.**
  - The system image is unencrypted and verifiable.
  - `/ferrix/data` is ext4 with fscrypt roots.
  - `/init` needs no unlock UI.
  - Updates never touch encrypted data.
  - The wrapped key blob (`/var/lib/ferrix-auth`) lives in
    `/ferrix/data`, outside the encrypted roots.
- **Migrations that read encrypted stores** run in the new system after
  first unlock, not at boot. The health check therefore must not depend
  on unlock, and the migration step has its own record and rollback rule.
- **Choosing U2 would force LUKS2** (ADR-011 E1). With LUKS2, `/init` must
  unlock before it can read `state.json`, unless boot state moves to the
  `metadata` partition (allowed by the invariant, wiped by `-w`).

## Consequences

- **One more `userdata` wipe** to move to the new layout. It is allowed
  under `data_policy = dev-device-no-user-data`.
- **dev-compose** produces `system.erofs` and `release.json` instead of
  `userdata.img`.
- **dev-flash** gains a mode that pushes a system image into the data
  filesystem (over the USB link from a running Ferrix) without writing
  `userdata` raw. The raw `userdata` write stays for first-time layout
  creation only.
- **`/init` changes:** state file, try counter, hash check, loop mount,
  overlay setup.
- **Writable paths must be enumerated.** `/etc` needs an overlay or a
  move of the few writable files (NM, `machine-id`, `/etc/ferrix`
  profile).
- **QEMU gains** a persistent-disk check for the same A/B logic (separate
  from `qemu-check`, which stays ephemeral and headless). The pure
  boot-state logic is host-tested.
- **Docs:** `docs/status.md` must stop implying any update path until U1
  evidence exists.

## Evidence and validation plan

- **mock:** host tests of the boot-state machine:
  - pending → tried → promoted;
  - pending → exhausted → rollback;
  - corrupt `state.json` → boot `current` and record it;
  - power loss between write and fsync;
  - manifest signature, hash and `target_id` reject paths;
  - migration snapshot and restore.
- **QEMU** (a separate bounded check, persistent image under ignored
  `out/qemu-aarch64/`):
  - version → version upgrade;
  - an injected failing health check → automatic rollback with a
    truthful record;
  - an update killed mid-write → the old system boots;
  - a contacts migration with rollback restore.
- **physical:google-pixel-9a-tegu-gxq96-mp1-0**, on test data:
  - **Upgrade and interruption:**
    - one upgrade;
    - one interrupted upgrade (power cut mid-write; needs a human for the
      power key, or a forced reboot);
    - one rollback forced by a failing health check;
    - one data migration.
  - **Kernel bundle:** one kernel bundle through Fastboot, with the
    previous system image still booting.
  - **Encrypted data:** encrypted data (ADR-011) survives all of the
    above.
  - **Stock restore:** stock restore re-run and verified after the layout
    change.
  - **Record:** which paths kept data and which wiped it.

## Owner decisions needed

- Approve U1 (A/B system images in `userdata` with boot counting in
  `/init`), and reject U2 and U3 for now.
- **Signing key:**
  - who owns it, and where the private key lives (off-device, offline);
  - minisign/Ed25519 as the format;
  - the rotation and revocation procedure.
- Whether a device may ever write `boot_b` itself (an on-device kernel
  update), or whether kernel updates stay host/Fastboot-only.
- **Hand-rolled updater:** accept the small Rust `ferrix-updated` and
  `/init` logic, or require a RAUC/SWUpdate fit evaluation first.
- The release cadence and security-response owner for kernel, firmware,
  pmOS packages and the LineageOS/Waydroid image, and the pin-bump
  procedure (spec 13 deliverable 8).
- **Waydroid downgrades:**
  - Android-image updates are forward-only, with a data snapshot, or
  - rollback across an Android image is refused.
- Backup format (`age`) and the default destination (host over USB, or
  removable storage).
- Approve one more dev `userdata` wipe to create the new layout.
