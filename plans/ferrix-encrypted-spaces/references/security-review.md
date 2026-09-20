# Security Review Guide

Read this reference for threat-model changes, authentication and recovery work, notification bridges, logging/support output, or physical-target claims.

## Intended adversary

The initial product addresses an attacker who has the device and pressures or observes the user unlocking it, then manually inspects the ordinary UI for a limited time. The attacker may open apps, settings, files, search, recents, notifications, quick settings, and storage summaries.

Assume the attacker may know Ferrix supports encrypted spaces. Do not require the cover space to prove that no other space exists. The valuable property is that the supplied cover credential opens a coherent environment without granting access to private data.

## Excluded claims

Do not claim resistance to:

- forensic block-device analysis or flash examination;
- an attacker-controlled kernel, initramfs, compositor, session daemon, or update;
- extraction of a software-only device secret;
- provider, server, backup, or traffic records;
- laboratory timing, power, electromagnetic, or memory-remanence analysis;
- prolonged expert inspection correlated with known expected content;
- compelled disclosure regimes or legal outcomes.

These exclusions do not excuse obvious UI, log, mount, process, or notification leakage.

## Review by asset

### Credentials

- Never log, serialize, persist, shell-escape, or include a PIN in an error.
- Avoid exposing PIN length through records or accessibility events.
- Bound input length and KDF resource use to prevent local denial of service.
- Scrub transient buffers where practical, while documenting that compiler, allocator, kernel, and hardware copies may remain.
- Keep authentication and rate limiting in trusted system code, not the launcher.
- Make failure responses generic and apply a global policy so the lock screen does not reveal space count.

### Keys

- Generate data keys with an operating-system CSPRNG through maintained libraries.
- Separate data keys from credential-derived wrapping keys.
- Never expose raw keys to apps, launchers, command lines, environment variables, logs, evidence, or support archives.
- Minimize processes that can access unwrapped keys.
- Revoke kernel/user-space key handles after unmount and on every failed unlock rollback.
- Treat QEMU file-backed keys as test material, not secure storage.

### Plaintext data

Inventory filesystems, swap, zram, hibernation, `/tmp`, `/run`, logs, journal/serial output, crash dumps, screenshots, thumbnails, clipboard, compositor buffers, package caches, and firmware-visible memory.

For each, record whether it is:

- encrypted with the active space;
- ephemeral and cleared on lock/reboot;
- system-global but content-free;
- intentionally shared through a broker; or
- an acknowledged limitation blocking a stronger claim.

### Process and IPC state

- Prove the complete session process tree is gone before unmount.
- Revoke portals and reject new requests as soon as locking starts.
- Authenticate privileged peers with kernel credentials or capabilities; request payload roles are insufficient.
- Prevent stale sockets, queued messages, activation records, and subscription streams from crossing sessions.
- Ensure a restarted shell or session authority returns locked rather than reconstructing an active session from weak signals.

### UI state

- Clear the prior framebuffer and compositor damage history when locking or changing spaces.
- Suppress notification shade contents, recents, app previews, search indexes, share targets, input suggestions, clipboard previews, and accessibility history from inactive spaces.
- Avoid visible labels such as `decoy`, `duress`, `hidden`, `primary`, or the number of configured spaces.
- Keep the cover space genuinely usable and persistent; a conspicuously empty shell defeats the interaction goal.

## Cover-credential safety

The cover credential must perform only an ordinary successful unlock of its own space. Explicitly prohibit:

- wiping or corrupting another space;
- sending an alert, location, message, or network beacon;
- taking a photo or recording audio;
- changing audit verbosity;
- triggering a distinctive reboot, delay, animation, or error;
- placing secret markers into logs or remote services.

Such mechanisms may endanger the user if discovered and are not authorized by this architecture.

## Notifications and communications

Review both content and metadata. An app name, badge count, timestamp, sender category, vibration pattern, network destination, or storage growth can disclose another space during ordinary inspection.

For inactive private/cover spaces, default to no visible signal. A sealed-delivery design must specify:

- who can address the opaque destination;
- what the broker learns;
- ciphertext size and timing leakage;
- replay, expiry, quota, and denial-of-service behavior;
- when and where decryption occurs;
- how queues are deleted or migrated;
- whether backups expose destination relationships.

Calls, SMS, emergency alerts, alarms, and SIM state require separate global-versus-space policy. Do not let convenience defaults accidentally reveal private contacts or message history in the cover space.

## Authentication usability

Security depends on users being able to execute the cover flow under stress:

- Use the same lock screen and success transition for every space credential.
- Do not require selecting a profile first.
- Avoid unusual confirmation steps for the cover credential.
- Ensure reboot, timeout, manual lock, and failed-attempt flows behave consistently.
- Do not assume biometrics select a safe space. Treat their use as a separate threat-model decision.
- Preserve emergency access without exposing space configuration.

Test with realistic populated cover data rather than only empty fixtures.

## Audit records and evidence

Machine-readable records should use event classes and coarse outcomes, for example `space-unlock-failed` or `session-locked`, without credentials, user labels, volume paths exposed to ordinary clients, raw internal IDs, or data-derived values.

Development-only QEMU records may include deterministic fixture handles when required for assertions, but keep them explicitly mock, out of product UI, and out of committed personal evidence. Scrub serial logs and captured images before sharing.

## Physical target claim gate

Before saying a physical target provides hardware-backed encrypted spaces, establish for the exact model and revision:

- where the device secret lives and whether it is exportable;
- who enforces retry limits and whether power cycling resets them;
- authenticated/verified boot coverage and rollback behavior;
- update and recovery trust chains;
- whether the lock UI has a secure input/display path;
- key behavior across suspend, reboot, crash, and recovery mode;
- data exposure through bootloader, debug, DMA, crash dump, and service interfaces;
- backup and credential-recovery consequences.

If any property is unavailable, report a lower protection level and preserve functional behavior rather than inventing a hardware-backed claim.

## Minimum adversarial tests

- Wrong PIN before and after successful unlock.
- Rapid attempts and power-cycle behavior.
- Compositor, session authority, appd, launcher, and application crashes during each transition.
- An app that forks, ignores signals, holds files open, retains sockets, and redraws while lock begins.
- Surface spoofing and lock-screen overlay attempts.
- Requests sent on stale appd/core/portal connections after space switch.
- Clipboard, notifications, recents, search, share, crash, and thumbnail inspection in the cover space.
- Disk-full, corrupt header, failed mount, failed unmount, and interrupted key rotation.
- Reboot while private space is active and while lock cleanup is in progress.
- QEMU snapshot/restore behavior, clearly scoped as mock evidence.
