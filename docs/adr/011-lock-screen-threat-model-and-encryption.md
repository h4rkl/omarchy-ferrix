# ADR-011: Lock screen, threat model and at-rest encryption

Status: proposed 2026-11-25 for spec 12 deliverables 1–3
([`plans/todos/12-security-privacy.md`](../../plans/todos/12-security-privacy.md)),
issue #31. Owner review required. The threat model, its accepted risks and
the encryption layout are owner-gated by spec 12. Subordinate to
`plans/baseline.md`. Nothing here is implemented. No protection is claimed
for any target until the evidence below exists. No hardware-backed key,
retry counter or verified boot is claimed on any route.

## Context

### What exists

- **Trusted power key** (#162). `crates/ferrix-shell/src/power.rs` takes
  `KEY_POWER` out of the input stream before xkb or any client sees it.
  - A short press toggles the screen.
  - A 600 ms hold opens the compositor-drawn power menu (`power_menu.rs`).
  - While the screen is off no input reaches a client, and wake re-enters
    the front toplevel.
  - On the Pixel 9a the path was exercised with injected uinput
    `KEY_POWER`, not the physical button
    (`physical:google-pixel-9a-tegu-gxq96-mp1-0`, 2026-11-24).
- **Idle timeout and inhibitors** are owned by `ferrix-core`
  (`crates/ferrix-core/src/screen.rs`), with `call`, `video` and
  `navigation` inhibitors. Both dev profiles default to never
  (`idle_timeout_secs = 0`).
- **Suspend.** On the Pixel, `s2idle` and `deep` both suspend and resume
  with RTC wake (2026-11-24 probe). There is no Ferrix suspend policy;
  PR #172 proposes one behind a board flag.
- **Sandbox** (#10, QEMU/mock only).
  - Per-app cgroups.
  - A loopback-only netns without the `network` grant.
  - A `minimal` filesystem view, with `$HOME` only with `files-home`.
  - A seccomp filter that already returns `EPERM` for keyring syscalls.
  - Opt-in per-app uid.
  - Waydroid is not covered.
- **No lock screen, no credential and no encryption exist anywhere.** A
  grep for LUKS, cryptsetup, fscrypt, dm-crypt and keyring finds nothing
  in `crates/`, `targets/` or `tools/`.
- **Kernel config.** The Pixel kernel fragments
  (`kernel-config.ferrix-gki`, `kernel-config.tegu`,
  `kernel-radio-extra.config`) set `CONFIG_BLK_DEV_DM=y`. They say nothing
  about `DM_CRYPT` or `FS_ENCRYPTION`. Whether the Kleaf GKI defconfig
  enables them is unverified.

### What the dev route is (Pixel 9a)

From `tools/pixel-9a/dev-compose.py`, `dev-flash.sh` and
`pmos/device-google-tegu/initramfs-init-kleaf`:

- **Bootloader** `unlocked`, verified-boot state `orange`
  (`targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/manifest.toml`).
- **vbmeta** is built `--algorithm NONE --flags 2`: verification is
  disabled. `image-layout.toml` says `[verified_boot] required = true`,
  which the dev route does not meet.
- **Rootfs.** The rootfs is the raw `userdata` partition, an ext4 image of
  about 8 GiB on a partition of about 117.5 GB, mounted read-write by
  `/init` (`PARTLABEL=userdata`). There is no separate data filesystem.
- **USB console.** The initramfs gives a USB CDC-ACM gadget:
  - `ttyGS0` streams the log;
  - `ttyGS1` gets `getty -n -l /bin/sh`, which is a root shell with no
    login.
- **Where user state lives:**
  - `HOME=/home/user`;
  - `/var/lib/ferrix/contacts/contacts.json`;
  - `/var/lib/ferrix/telephony/sms.json`;
  - `/var/lib/ferrix/screen.json`;
  - NetworkManager keyfiles with Wi-Fi PSKs;
  - Waydroid system state in `/var/lib/waydroid`;
  - Android `/data` in `/home/user/.local/share/waydroid/data`.
- **Secure hardware is out of reach.** Fingerprint matching and Android's
  gatekeeper/keymint run in Trusty. Ferrix has no client for them
  (`nfc-fingerprint-source-evidence.md`). No Ferrix code talks to Titan M2
  or Trusty.

## Threat model

### Assets

- **Personal data:** home, contacts, SMS, call history, Wi-Fi PSKs,
  Android app data, notification content.
- **Session authority:** the ability to use apps and services as the
  user.

### States

- **Before first unlock (BFU):** after boot, before the credential has
  been entered once. Personal data keys are not in memory.
- **Locked (AFU):** the credential was entered since boot, and the screen
  is locked.
  - Keys are in kernel memory.
  - Apps and the Android container may run.
  - The UI and input are withheld.
- **Unlocked:** the user is in front.

### Adversaries (in scope)

1. **Opportunistic.** Someone holds a locked or off phone for minutes:
   theft, a found phone, or a casual look. They use only the touchscreen
   and buttons.
2. **Malicious app.** An app or APK on the device tries to read the
   credential, draw a fake lock or consent surface, or keep content
   visible while locked.
3. **Crash.** Any crash of an app, the shell or the core, during lock or
   unlock.

### Adversaries (out of scope, stated plainly)

- **A USB-connected attacker on a dev image.** The `ttyGS1` root shell
  bypasses everything. While that shell exists, no lock claim holds
  against anyone with a cable.
- **Evil maid.** The bootloader is unlocked and vbmeta is `NONE`. Anyone
  with brief physical access can:
  - `fastboot boot` or flash their own `init_boot`;
  - capture the credential at the next unlock;
  - or read every unencrypted byte of `userdata`.
  Encryption protects data at rest only on a device that has not been
  tampered with since. An unlocked bootloader gives no integrity.
- **Offline guessing.** A PIN cannot resist this without a hardware retry
  counter. Ferrix has none. With a dump of `userdata`, a 6-digit PIN has
  10^6 candidates: at 1 s of Argon2id per guess that is about 12 days on
  one core, and parallel hardware divides it. Only a high-entropy
  passphrase resists offline attack.
- **Cold boot, DMA and RAM forensics in the AFU state.** Keys are in RAM.
- **Kernel or root compromise, and modified system software.**
- **Forensic deniability.** This is a non-goal, as in the encrypted-spaces
  draft.

## Constraints

- Baseline: QEMU evidence is never a hardware claim. Use maintained
  encrypted-storage facilities; never project-owned cryptography or disk
  formats (`plans/ferrix-encrypted-spaces/SKILL.md` invariants).
- `AGENTS.md` recovery invariant: the device must stay restorable to stock.
  Encryption must not need writes outside partitions the factory image
  rewrites. There may be no bootloader relock, `oem`, erase or format
  commands.
- ADR-010: the lock surface is a shell system modal at the top of the
  z-order, drawn in the shell's own buffer.
- ADR-008: the Android container stays warm. It has no lock of its own,
  because its own lock screen never gets a window.
- Spec 13 / ADR-012: the encryption layout determines what an update,
  reflash or restore destroys.

## Lock screen: options

### L1. Lock surface inside `ferrix-shell`, credential checked by a small privileged daemon

- **Shell:** the shell renders the lock like `power_menu.rs`. It has its
  own keypad and passphrase keyboard, drawn with `ferrix-ui`, and takes
  all input.
- **Daemon:** a new `ferrix-authd` (the draft calls it `ferrix-sessiond`)
  runs as root from OpenRC. It:
  - holds the attempt counter;
  - runs the KDF;
  - adds the fscrypt key (see E2);
  - answers the shell over a private socket. Only the shell's pid,
    checked by `SO_PEERCRED` against the process OpenRC started, may
    connect.
- **For:** one trusted input path, and no client sees any keystroke. It
  reuses the power-menu pattern. Credential handling is separate from the
  compositor, as the draft requires.
- **Against:** it is a new daemon. The lock UI ships with the shell.

### L2. `ext-session-lock-v1` with a separate lock client

- **For:** it is the standard protocol. Smithay supports it.
- **Against:**
  - A second trusted client must be authenticated (a pid check for the
    protocol bind).
  - It adds a crash surface: if the lock client dies, the compositor must
    still blank.
  - The OSK must run inside that client.
  - The benefit over L1 is small for a single-shell product.

### L3. PAM/elogind session lock

- **For:** standard Linux plumbing.
- **Against:** elogind's lock signal is advisory. The compositor still has
  to enforce it. PAM PIN handling does not address offline guessing. It
  adds no trusted input.

**Recommendation: L1.** Keep `ext-session-lock-v1` unexposed (like
layer-shell in ADR-010) until a trusted lock client is needed.

### Lock behaviour (L1)

- **Boot.** The shell starts in `Locked` before `ferrix-appd` launches
  Home. Data that needs the key is not available until first unlock
  (E2).
- **Lock triggers:**
  - power-key short press (screen off);
  - idle timeout;
  - before suspend (PR #172 policy runs after lock commits);
  - "Lock now" in the power menu (a new fourth button).
  A grace period (re-wake within N s without credential) is an owner
  decision. The recommendation is 0 s: lock on every screen-off.
- **While locked:**
  - Every client surface, including Waydroid tasks, gets
    `wl_surface.leave`, as screen-off already does.
  - No client frame is composed, the last frame is cleared from the
    output buffer, and no input reaches a client.
  - Header status (time, battery, connectivity) stays visible.
- **Crash rules:**
  - The shell persists nothing that would unlock it.
  - A shell restart (OpenRC respawn) always starts in `Locked`.
  - A crashed `ferrix-authd` leaves the shell locked with a typed
    `auth-unavailable` notice and the emergency affordance.
  - An app crash cannot change lock state.
- **Attempt limiting** is enforced by `ferrix-authd` and persisted in its
  state file:
  - 5 free attempts;
  - then exponential delays (30 s, 1 min, 2 min … capped at 1 h);
  - never a wipe.
  This is software-only. It slows the opportunistic adversary, not an
  offline one, and the UI and docs say so.
- **Notifications while locked** (with #22):
  - **Default:** the app name and icon (from appd identity) plus "New
    notification". No title, body, sender or action.
  - **Optional per app:** full content (owner decision on the default).
  - Heads-up content is redacted the same way.
  - Actions that open an app require unlock first.
- **Emergency.** The lock surface shows an emergency affordance only when
  spec 02 reports emergency calling as supported. Otherwise it shows the
  factual text "Emergency calls unavailable on this build". It never
  shows a dial button that would fail.
- **Allowed while locked:**
  - time and battery;
  - the power menu;
  - brightness;
  - flashlight, if spec 06 adds it;
  - media controls later.
  Everything else (terminal, files, quick-settings network toggles,
  camera) requires unlock. This follows the draft's `security_level`
  column.
- **USB.** A release image must not start the `ttyGS1` shell. A dev image
  keeps it and is labelled "no lock protection against USB" in Settings >
  About and in `docs/status.md`. That is the only honest statement while
  it exists.

### Credential storage

- The credential is never stored. `ferrix-authd` generates a random
  256-bit data key (the fscrypt master key). It wraps that key with a key
  derived from the credential by **Argon2id**, using the RustCrypto
  `argon2` crate (MIT/Apache-2.0). Parameters are tuned per target and
  recorded; the initial target is ≥ 256 MiB and ≥ 1 s on the Pixel.
- The wrapping is AEAD (`chacha20poly1305`, RustCrypto). The result is a
  versioned blob in `/var/lib/ferrix-auth/` (root 0700), on the
  unencrypted part of the data filesystem.
- Changing the credential rewraps the key. It does not re-encrypt data.
- Protection level is reported as a typed value: `mock` on QEMU and
  `software` on the Pixel. `hardware-backed` is reserved for a future
  target with evidence. Ferrix has no Trusty/Titan client, so the Pixel
  is `software`.

## Encryption: options

The layout options assume ADR-012's direction (a read-only system image
and a separate data filesystem in `userdata`). On today's layout (rootfs =
`userdata`) only E2 or E3 can be applied without a pre-boot unlock.

### E1. LUKS2 (dm-crypt) on the whole data area

`userdata` becomes a LUKS2 container. `/init` must unlock it before it can
mount anything, because on today's layout the OS itself is inside.

- **For:** everything is covered (logs, swap, NM keyfiles, `/var`).
  `cryptsetup` is mature and the format is standard.
- **Against:**
  - **Pre-boot unlock.** Unlock happens in the initramfs, before the
    shell, Wayland and the OSK exist. It needs an initramfs unlock UI
    (pmOS's `unl0kr` or `osk-sdl` precedent) on the DRM panel with touch,
    a second trusted input path, plus `cryptsetup` and its dependencies in
    `init_boot`.
  - **Nothing runs before unlock.** No alarm, call, SMS or network at
    boot.
  - **Every dev flash must ship a LUKS container image.** That means
    either a fixed dev passphrase or on-device first-boot encryption.
  - **Lock does not remove the key.** It stays in dm-crypt for the whole
    uptime. `luksSuspend` cannot be used on the root device.
  - `DM_CRYPT` in the Kleaf GKI is unverified.

### E2. fscrypt (ext4 native encryption, v2 policies) on user data directories

The system and the data filesystem stay unencrypted at the block level.
`ferrix-authd` applies fscrypt v2 policies to the user-data roots:

- `/home/user`, which holds Waydroid's Android `/data`;
- `/var/lib/ferrix/user/`: contacts, SMS and settings move here from
  `/var/lib/ferrix/`;
- `/var/lib/waydroid` per-user state.

At first unlock `ferrix-authd` adds the key with
`FS_IOC_ADD_ENCRYPTION_KEY`. Services that need user data
(`contactsd`, `telephonyd` storage, appd's Home launch, the Waydroid
container) start only after that. Device-global services (shell, core,
`networkd`, modem) start at boot.

- **For:**
  - **Boots to a real locked shell.** The device reaches a shell-drawn
    lock with the radio and network up. There is no initramfs UI, and
    there is one trusted input path (L1).
  - **The system image needs no unlock.** It stays read-only and
    hash-verifiable (ADR-012).
  - **Dev flashes of the system image do not touch encrypted data.** This
    holds once ADR-012's split exists.
  - **It fits the encrypted-spaces draft.** Each space is another fscrypt
    policy and key on its own directory tree, which E3 refines.
  - **Standard tooling.** It is the kernel's own facility, the same one
    Android FBE uses. The ioctls are stable.
- **Against:**
  - **Metadata leaks.** File sizes, counts, timestamps and directory
    shape are visible. Names are encrypted.
  - **Decrypted data outside the encrypted roots.** NM keyfiles
    (`/etc/NetworkManager/system-connections`), `/var/log`, the pstore
    copy (`/var/log/ferrix-pstore`) and crash dumps hold data outside
    those roots. Each must be moved under an encrypted root, scoped, or
    accepted as a documented leak.
  - **Lock does not evict the key.** It stays loaded while locked. Removal
    is only reliable at reboot ("lockdown" = reboot).
  - **Kernel support is unverified.** `CONFIG_FS_ENCRYPTION` and the
    ext4 `encrypt` feature in the Kleaf GKI are expected (Android GKI
    uses them for FBE) but not verified here. The rootfs ext4 must be
    created with `-O encrypt`.
  - **Android's own FBE is not used.** The container's `/data` sits
    inside Ferrix's encrypted `/home/user`. The container is protected as
    one unit, not per Android user.

### E3. Per-space encryption (`plans/ferrix-encrypted-spaces/`)

Multiple spaces, each with its own key, launcher and appd, and a cover
credential. Only one space is mounted at a time.

- **For:** the strongest separation between contexts. It is the owner's
  long-term direction.
- **Against:**
  - **Lock must tear down.** A lock must stop every app process, unmount
    and revoke keys before another space opens, and that is not built
    (no fresh appd per session, no process-domain teardown).
  - **Waydroid has to be per space.** That costs about 1.5 GiB PSS per
    container (2026-11-24 benchmark), or one container that restarts on
    every space switch.
  - **Notification policy across spaces** is unresolved.
  - **Too large a first step.** It sits on top of E2 or E1 rather than
    replacing them.

## Recommendation

1. **Lock surface:** L1, with `ferrix-authd`.
2. **Encryption:** E2 (fscrypt v2 on the user-data roots), with a
   directory layout that admits E3 later. Each space would be another
   policy-protected root with its own wrapped key. The single space is
   "space 0".
3. **What a dev image says:**
   - Encryption is `software`.
   - Not protected against evil maid (unlocked bootloader, no verified
     boot).
   - Not protected against a USB cable while `ttyGS1` exists.
   - Not protected against offline guessing of a short PIN.
4. **Default credential:** a passphrase is recommended, and a PIN is
   allowed with a stated warning (owner decision).

### Impact on reflash, restore and suspend

| Action | Today (rootfs = userdata) | With ADR-012 split + E2 |
| --- | --- | --- |
| `just pixel-9a-dev-flash` writing `userdata` | Destroys everything | Not used for routine updates. A layout-changing reflash still destroys everything, including encrypted data and the wrapped key |
| System image update (ADR-012) | n/a | Preserves encrypted data. The new system must accept the existing `ferrix-authd` blob version |
| `just pixel-9a-factory-restore` (`fastboot -w`) | Wipes `userdata` and `metadata` | Same. Encrypted data is unrecoverable by design |
| Forgotten credential | n/a | Data is unrecoverable. Recovery is factory restore or a reflash |
| Reboot | n/a | BFU: user data is unreadable until unlock |
| Suspend (`s2idle`/`deep`) | n/a | Key stays in RAM. Lock commits before suspend, so resume always shows the lock |
| Power loss | n/a | fscrypt on ext4 has journalled metadata. Test power loss during the first encryption of an existing directory (`ferrix-authd` encrypts only empty directories, so migration is copy-into-new-encrypted-dir then rename) |

A recovery key (a second wrapping of the data key, shown once at setup) is
optional. It weakens the coercion story of E3 (owner decision).

## Consequences

- **New code:**
  - crate `ferrix-authd` (daemon) with an `auth-api`/`auth-protocol`
    split under the ADR README conventions;
  - a shell lock state machine and lock surface;
  - OpenRC ordering: user-data services `need ferrix-unlocked`, a virtual
    service that `ferrix-authd` starts at first unlock.
- **Moves:** contacts and SMS stores move under `/var/lib/ferrix/user/`.
  This is a migration (ADR-012), and their paths become configurable.
- **Waydroid** starts after first unlock, not at boot. The first APK
  launch after a reboot waits for unlock (ADR-008 "warm container" becomes
  warm-after-unlock).
- **The image build** creates ext4 with `-O encrypt`. Kernel config
  fragments add `CONFIG_FS_ENCRYPTION` if the GKI lacks it, as was done
  for `IPC_NS`.
- **Release images** drop the `ttyGS1` root shell, or gate it behind an
  explicit developer setting that is off by default and shown in the
  header. This is a board-package change in `initramfs-init-kleaf`.
- **The power menu** gains "Lock now". A later "Lockdown" (reboot to BFU)
  is possible.
- **Accepted risks** must be listed in `docs/status.md` before any lock
  claim: evil maid, USB shell, offline PIN guessing, AFU RAM, metadata.

## Evidence and validation plan

- **mock:**
  - **Lock state machine:**
    - every transition;
    - crash or restart of shell or `ferrix-authd` → locked;
    - rate-limit schedule persisted across restart;
    - no client receives input while locked.
  - **Wrapped-key blob:**
    - round trip;
    - version reject;
    - corrupt blob fails closed.
  - Credential bytes and their length never appear in records.
- **QEMU:** a separate bounded check (not `qemu-check`, which stays
  ephemeral), on an explicit persistent ext4 test image under
  `out/qemu-aarch64/`, ignored. It must show:
  - boot → locked frame;
  - wrong PIN → still locked, with a delay record;
  - right PIN → Home;
  - a marker file under `/home/user` is unreadable (`ENOKEY`) before
    unlock;
  - a power-key lock frame shows no app pixels;
  - kill the shell while locked → it restarts locked;
  - kill an app while locked → still locked;
  - a redacted notification frame.
  Frames are inspected. Evidence is labelled `QEMU`/`mock`, protection
  level `mock`.
- **physical:google-pixel-9a-tegu-gxq96-mp1-0:**
  - read-only check of `/proc/config.gz` for `FS_ENCRYPTION`;
  - lock and unlock with Jelly in front (webcam: no Jelly pixels while
    locked);
  - reboot → BFU marker unreadable;
  - `deep` suspend → resume shows lock;
  - a physical power-button press (needs a human);
  - an ADR-012 system update with encrypted data surviving.
  Labelled with the exact target, protection level `software`.
- Each protection claim in `docs/status.md` names which adversary above it
  covers.

## Owner decisions needed

- Accept the threat model: the in-scope adversaries and the out-of-scope
  list, especially evil maid on an unlocked bootloader, offline PIN
  guessing and the dev USB root shell.
- Approve E2 (fscrypt v2 on user-data roots) over E1 (LUKS2 with an
  initramfs unlock UI). E3 stays a later layer.
- Approve L1 (shell lock surface + `ferrix-authd`) over L2
  (`ext-session-lock-v1` client).
- **Credential:**
  - passphrase required, or PIN allowed with a warning;
  - minimum length;
  - attempt schedule (proposed: 5 free attempts, then exponential up to
    1 h, never wipe).
- Lock grace period after screen-off (proposed 0 s).
- Default notification redaction (proposed: app identity only), and
  whether per-app full content is allowed.
- **Recovery key:**
  - offer one at setup, or
  - make data strictly unrecoverable without the credential.
- Whether release images must remove the `ttyGS1` root shell. If
  dev-only, how a dev image is marked.
- Which leaks stay accepted for now: NM keyfiles with PSKs, `/var/log`,
  the pstore copy. The alternative is moving each under an encrypted
  root.
- Whether Waydroid may start only after first unlock. This delays APK
  availability after a reboot.
