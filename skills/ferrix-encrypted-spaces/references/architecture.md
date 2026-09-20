# Encrypted Spaces Architecture

## Security outcome

Ferrix supports multiple persistent encrypted spaces selected by credentials entered through a compositor-enforced lock surface. A cover credential may open an ordinary, usable space during a short in-person coercion or inspection event. Unlocking one space neither decrypts nor mounts another.

This is not a hidden-volume or forensic-deniability design. The existence of the feature, encrypted containers, storage allocation, network history, or remote accounts may be discoverable outside ordinary UI inspection.

## System shape

```text
                 trusted system plane (always present)
  init/supervisor ─ ferrix-shell locked mode ─ ferrix-sessiond
          |                  |                       |
          |          trusted PIN input       credential/key broker
          |                                          |
   ferrix-core / hardware services          space storage adapter
                                                     |
                                      unlock + mount exactly one space
                                                     |
                       active space plane (replaceable on every unlock)
                  space-scoped ferrix-appd ─ launcher/home
                             |                    |
                        applications       space settings/UI
                             |
                  app-private writable directories

  inactive spaces: encrypted, unmounted, no processes, no visible events
```

`ferrix-sessiond` is a proposed name, not an implemented component or frozen ABI. Preserve the responsibility split if implementation naming changes.

## Trust boundaries and ownership

### Compositor

`ferrix-shell` owns the authoritative visual/input lock:

- Enter locked mode before teardown begins.
- Stop routing input to ordinary clients.
- Hide or discard all ordinary surfaces and previous frame contents.
- Render a minimal built-in lock UI initially, including its own PIN keypad/input handling.
- Deny screen capture, overlays, activation, global shortcuts, and ordinary OSK participation while locked.
- Send credentials only to the session authority over a private authenticated channel.
- Transition to the active space only after the new launcher has produced a valid first frame.

A later trusted lock client is acceptable only when authenticated through an inherited capability or equivalently strong mechanism. Do not trust a self-declared Wayland app ID or the ordinary application socket.

The compositor must not own encrypted volumes or retain PINs. It owns presentation and input routing; the session authority owns authentication and keys.

### Session authority

The privileged session authority owns:

- the locked/unlocking/active/locking state machine;
- credential attempt policy and delays;
- opaque internal space records;
- data-key unwrap and key-protector calls;
- encrypted-volume activation, validation, mounting, and unmounting;
- creation of private runtime directories and scoped service endpoints;
- startup and teardown of the space-scoped application service;
- recovery from partial unlock, crashes, and failed cleanup.

It exposes narrow typed operations such as `unlock(credential)`, `lock(reason)`, `active_state()`, and privileged enrollment/recovery operations. It never returns raw data keys or credential material.

### Launcher and applications

The launcher is the home application for the active space. It may configure that space and request lock, but cannot authenticate, choose mounts, enumerate other spaces, or keep the session open.

Ordinary applications should not need `SpaceId`. Present fixed paths inside the active session, for example:

```text
/home/ferrix
/run/ferrix/session/appd.sock
/run/ferrix/session/core.sock
```

Resolve those paths through private namespaces or bind mounts. This keeps `AppId` stable and prevents space identity from spreading through public application APIs. Shared application binaries and descriptors remain outside space storage; per-space catalog preferences and all writable state live inside it.

Start a fresh `ferrix-appd` for each unlocked session. That retains the current one-instance-per-`AppId` model without turning identity into `(SpaceId, AppId)`. Stop it before unmounting.

### Global services

Power, display, networking, modem, audio routing, and similar hardware state remain system-global. Applications access them through scoped, permission-checked services. The active space must not obtain raw access merely because it is unlocked.

System services must classify every retained datum:

- system-global and safe across spaces;
- active-space-only;
- app-private within a space; or
- explicitly brokered shared data.

Ambiguous data defaults to active-space-only.

## State machine

Use an explicit fail-closed state machine. Conceptual states:

```text
BootLocked
    |
    v
Unlocking ------failure------> Locked
    |
    v
Active
    |
    v
Locking --------cleanup------> Locked

Any system-plane crash or inconsistent state -> Locked/ErrorLocked
```

`ErrorLocked` may expose a generic recovery or power-off action, but never an active-space surface. Do not infer active state from a mounted path or process alone; commit it only after all required steps succeed.

### Unlock transaction

1. The compositor is already locked and owns all input.
2. Apply global attempt throttling before expensive credential work.
3. Pass the credential through a private channel to the session authority.
4. Attempt to unwrap a configured space data key without showing or requesting a space name.
5. Scrub credential buffers after use as far as the platform permits.
6. Activate and validate the encrypted volume.
7. Mount it at a session-private internal path with safe mount options.
8. Create private runtime directories, sockets, and service capabilities.
9. Start a fresh space-scoped `ferrix-appd`, then that space's launcher.
10. Wait for a mapped, rendered launcher surface tied to the expected process lineage.
11. Ask the compositor to commit the transition to `Active`.

On any failure, terminate everything created by the attempt, unmount, revoke keys, clear the frame, normalize the user-visible failure, and remain locked.

### Lock transaction

1. The compositor immediately enters locked mode, blocks input, and clears active content.
2. Revoke session portals, service capabilities, clipboard, capture, and new launches.
3. Stop the launcher, apps, helpers, and the space-scoped `ferrix-appd` as complete process trees.
4. Close session sockets and remove runtime files.
5. Ensure no process or open file descriptor can retain the mount; force termination within a bounded policy.
6. Sync required data, unmount, deactivate the encrypted mapping, and revoke its key.
7. Clear notifications, recents, thumbnails, crash presentation, and other derived active-space state.
8. Commit `Locked` only after cleanup is confirmed.

If cleanup cannot complete, remain visually locked and do not unlock another space. Emit a redacted machine-readable failure and take the configured safe recovery action; never fall back to exposing the prior session.

## Space and credential model

A space has an opaque random internal ID and owns:

- one encrypted data domain;
- launcher configuration;
- application data roots;
- space settings and permissions;
- notification/history state;
- optional recovery metadata;
- credential-wrapped access to its data key.

Do not derive the data-encryption key directly from a PIN. Generate a high-entropy random data key and protect it with credential and device protection layers:

```text
random space data key
        |
        +-- wrapped for PIN-derived unlock
        |
        +-- optionally wrapped for recovery credential
        |
        `-- bound to device key protection where available
```

Use maintained facilities such as LUKS2/dm-crypt or fscrypt after a documented fit evaluation. Prefer a standard format and audited implementation over project-owned cryptography. If PIN processing exists outside the storage facility, use a maintained memory-hard password derivation implementation and secret-memory wrappers; verify choices and parameters at implementation time.

A short PIN cannot independently resist offline guessing. Meaningful physical-device resistance requires a non-exportable device secret and enforced retry policy. Report protection accurately through a typed level such as `mock`, `software`, or `hardware-backed`; naming is illustrative.

### Credential routing

The lock screen never lists spaces. A successful credential unwrap identifies the space internally. Avoid a plaintext PIN-to-space map.

Normalize obvious UI differences between successful credentials: layout, transition, messages, and animation should be the same. Use global rather than per-visible-space failure counters. Do not overstate timing resistance: the first threat model is interactive observation, not laboratory side-channel analysis.

Space enrollment must reject duplicate credentials without exposing other spaces outside a privileged authenticated flow. Space management belongs inside an authorized primary/private space or a separately designed recovery environment, never on the public lock screen.

### Cover space

A cover space is an ordinary persistent space, not a mode flag or synthetic screen. It should support normal apps, settings, files, and believable ongoing use. Its credential must follow the same unlock path as any other valid credential.

Never cause wipe, alert, outbound message, special logging, reboot, or covert network activity when the cover credential is used. Those behaviors create risk and are outside this blueprint.

## Storage and process isolation

Encryption at rest protects inactive storage; it does not isolate mutually distrustful apps while a space is active. Extend the native runtime independently with:

- restricted filesystem views and private writable directories;
- distinct app identities or equivalently enforced user namespaces;
- permission-checked service endpoints;
- no raw access to inactive-space block devices or mount roots;
- bounded resources, seccomp, and network policy where justified;
- prevention of app-held file descriptors blocking secure lock teardown.

Do not treat a changed `HOME` variable as isolation. The mount namespace and permissions must make other data unreachable.

Account for decrypted data outside the main filesystem: swap, hibernation, crash dumps, logs, temporary files, thumbnails, compositor buffers, DMA-capable devices, and application caches. Disable, encrypt, or scope each mechanism before claiming at-rest coverage.

## Notifications and shared events

Default policy: inactive spaces do not run applications and reveal no notification signal. This is the least surprising policy for the cover-space threat model.

Possible later mechanisms, from strongest to weakest:

1. **No background delivery:** process events only after unlock.
2. **Sealed delivery:** a system broker stores an opaque envelope it cannot decrypt, then hands it to the intended space after unlock.
3. **Redacted metadata:** show an opt-in generic indicator with no app or content identity; this still leaks that another space exists or is active.
4. **Resident background access:** keeps keys or services available and materially weakens the guarantee; do not enable silently.

The cover space must never show another space's app name, icon, sender, text, count, badge, crash, or pending-event indicator.

Calls, SMS, alarms, and emergency behavior use device-global resources and need explicit policy. Keep emergency calling in the trusted system plane. Do not infer that modem-global data is safe to expose to every space. Contact resolution, call history, voicemail, and message content should be stored or presented under a documented space policy.

## Biometrics and recovery

Biometrics do not naturally select among spaces. Keep the PIN/passphrase as the authoritative selector until a separate design proves that biometrics preserve cover-space behavior. Do not map different fingers to spaces by assumption.

Recovery credentials may reveal space existence and weaken coercion behavior. Define who can recover what, whether recovery is hardware-bound, what metadata becomes visible, and how credential rotation rewraps keys without rewriting the entire data volume.

## Target portability

Keep portable policy separate from implementation adapters. Conceptual internal contracts may include:

```text
SpaceStore       create / activate / validate / mount / unmount / destroy
KeyProtector     seal / unseal / rotate / protection_level
AttemptLimiter   check / record_failure / record_success
SessionRuntime   create endpoints / start appd / stop process domain
```

These names and method shapes are not frozen public APIs. Apps and public app protocols must not depend on them.

Implementation layers:

- **QEMU:** deterministic software-backed test keys and explicit persistent test images; evidence labelled `mock` or `QEMU`.
- **Generic Linux:** maintained kernel/userspace encrypted-storage and keyring facilities behind project adapters.
- **Physical target:** target package wires any TPM, TEE, secure element, rollback counter, or hardware retry facility and provides exact-device evidence.

Do not put board, vendor, QEMU, cryptsetup command, device-node, or partition conditionals into the launcher, applications, `ferrix-app-api`, or product policy. Target packages select adapters through session/bootstrap wiring.

## Integrity dependency

Encryption does not protect a PIN entered into attacker-modified system software. A physical-target security claim requires a target-specific authenticated/verified boot and update story, rollback behavior, and recovery chain. QEMU can validate state transitions and failure handling but cannot establish those properties.
