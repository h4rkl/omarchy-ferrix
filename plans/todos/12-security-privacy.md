# 12 — Lock screen, data protection, sandbox and permissions

Status: open · ADR:
[011](../../docs/adr/011-lock-screen-threat-model-and-encryption.md)
(proposed) · Issues: #31 (lock screen and at-rest protection), #10
(sandbox hardening), #11 (consent portals) · Depends on: 04 (power key),
11 (chrome and notifications) · Related draft:
`plans/ferrix-encrypted-spaces/`

## Goal

Personal data on the phone is protected:
- a trusted lock screen;
- encrypted user data with an honest threat model;
- apps confined to what the user granted;
- runtime consent that apps cannot spoof.

## Deliverables

### Lock screen and at-rest protection (#31)

1. **Threat model ADR.** Cover:
   - locked versus unlocked state;
   - trusted input;
   - credential storage;
   - timeout and power-key lock;
   - notification redaction;
   - recovery;
   - what an unlocked bootloader means: no verified boot, an evil-maid
     risk.
   No claim of hardware-backed keys or verified boot without evidence.
2. **Trusted lock surface in `ferrix-shell`.**
   - It covers native apps and Waydroid tasks.
   - App, shell or core crashes never reveal content; the shell restarts
     into the locked state.
   - PIN/passphrase entry on the trusted keyboard, with attempt
     rate-limiting.
   - Notifications redacted while locked.
   - The emergency-call affordance stays honest (spec 02).
3. **Encryption.**
   - Design user-data encryption that fits the dev and release partition
     layout (spec 13), for example LUKS2 or fscrypt on the home and app
     data. The key is derived from the user credential and unlocked at
     first login.
   - Waydroid data lives inside the encrypted area.
   - Test reboot, suspend/resume, power loss, and upgrade/recovery.

### Sandbox hardening (#10), each increment shippable on its own

4. **Filesystem view:** pivot into a minimal root. Read-only `/usr`, a
   per-app data dir, and user folders only with a grant (Files gets
   `files-home`).
5. **seccomp:** a default-deny-dangerous filter through a maintained crate
   (licence-checked), with a per-runtime escape hatch. Waydroid is not
   covered.
6. **cgroups v2:** a per-app cgroup under a Ferrix slice with `memory.max`,
   `pids.max` and CPU weight. Confirm that OpenRC uses unified mode. This
   also gives spec 10 its uclamp target.
7. **Network namespace:** apps without the `network` grant get
   loopback-only.
8. **Per-app uid:** a distinct subuid per app via the user namespace, with
   data-dir ownership.
   Keep the single process-lifetime spawner thread (`PR_SET_PDEATHSIG`).

### Consent portals (#11)

9. **ADR:** spoofing, clickjacking and background requests. The prompt is
   drawn by the shell. Grants are stored atomically in
   `$XDG_STATE_HOME/ferrix/grants.json` and fail closed when corrupt.
10. **Prompts:** contacts, call/SMS, network control, files-home, camera,
    mic and location (specs 07, 08). Settings > Apps lists grants with
    toggles. Revoking drops live subscriptions immediately.
11. **APKs:** map Android runtime permissions for camera, mic and location
    onto the same Ferrix grants, so one switch controls both.

## Acceptance

- QEMU scenarios:
  - lock, crash, still locked;
  - request → grant → use;
  - deny → typed `unauthorized`;
  - revoke drops a subscription;
  - sandbox negative tests (a forbidden path, a forbidden syscall, no
    network).
- Physical: lock/unlock, reboot, suspend and an upgrade on the Pixel, with
  encrypted data surviving and staying unreadable while locked (as defined
  by the threat model).
- Each protection claim names its threat model.

## Owner-gated

The threat model and its accepted risks, and the encryption layout, since
it changes what a reflash or restore destroys.
