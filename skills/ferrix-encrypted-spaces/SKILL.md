---
name: ferrix-encrypted-spaces
description: Design, implement, or review Ferrix trusted locking and encrypted spaces, including cover-PIN behavior for in-person duress, work/private separation, key and mount lifecycle, space-scoped applications, and notification isolation. Use for authentication, lock-screen, multi-space, encrypted-at-rest, or cross-space privacy work; do not use it to claim forensic deniability or physical-device security without target evidence.
---

# Ferrix Encrypted Spaces

Use this skill to preserve one security model across the QEMU target and future admitted physical targets. The product feature is an **encrypted space**, not merely an alternate launcher. A space includes its launcher, applications, app data, settings, permissions, history, and notification state.

## Authority and status

This skill is a forward architecture blueprint. It does not claim that encrypted spaces, verified boot, secure hardware, or a physical target are currently implemented.

Before architecture or implementation work:

1. Read the repository `AGENTS.md`, `docs/baseline.md`, `docs/plan.md`, and `docs/status.md`; baseline wins conflicts.
2. Read `docs/app-platform.md` when changing application identity, lifecycle, sandboxing, sockets, the launcher, or `ferrix-appd`.
3. Read `targets/qemu-aarch64/README.md` when changing guest construction, storage images, QEMU workflows, or evidence.
4. Preserve unrelated user changes and the repository's existing validation requirements.

Persistent encrypted storage expands the current baseline, which defaults to ephemeral disks. Do not silently implement that expansion: record the approved design in the authoritative project documents as part of the change.

## Threat model

Optimize the first product for a short, in-person coercion or inspection event. A user can enter a cover credential that opens a convincing, persistent, ordinary space without decrypting or mounting private spaces.

Protect against manual inspection of apps, settings, files, notifications, recents, search, share surfaces, and ordinary storage summaries. Do not claim protection against forensic media analysis, kernel/root compromise, modified system software, provider records, prolonged expert examination, or an attacker who already knows what content should exist.

Do not describe a cover space as forensically hidden or plausibly deniable. Never attach destructive, alerting, networking, wiping, or other exceptional side effects to a cover credential.

## Non-negotiable invariants

- The compositor enforces locked presentation and trusted input. A launcher or ordinary Wayland client never decides whether the device is locked.
- A small privileged session authority owns authentication, active-space state, key lifecycle, mounts, and session startup. Do not put these responsibilities in the launcher, an application, or PID 1.
- Exactly one space is decrypted and active initially. Do not mount a second until the first is terminated, unmounted, and its key revoked.
- The launcher is space-scoped and starts only after unlock. It has no PIN, key-unwrapping, or mount authority.
- Shared system binaries are read-only; writable application and user state belongs to the active encrypted space.
- Inactive spaces run no applications and reveal no notification content, badges, counts, app names, search entries, recents, or crash details.
- Ordinary apps remain target-neutral and space-neutral. They receive stable paths and scoped service endpoints, not target conditionals, credentials, raw keys, or a public `SpaceId`.
- QEMU validates behavior with software-backed mock protection. Hardware-backed key protection, retry enforcement, verified boot, and rollback resistance require exact-target evidence.
- Use maintained cryptographic and encrypted-storage facilities. Never invent encryption, key derivation, disk formats, or secret comparison code.
- Fail closed: a crash, timeout, mount error, stale process, or key-revocation failure leaves the compositor locked and must not expose another space.

## Apply the blueprint

Read [references/architecture.md](references/architecture.md) for component ownership, state transitions, storage and key boundaries, application/session integration, notification policy, and target portability.

When planning or implementing repository changes, also read [references/implementation-path.md](references/implementation-path.md). It maps the blueprint onto the current Ferrix crates and defines incremental evidence gates.

For threat-model review, authentication changes, notification bridging, logging, recovery, or a physical-target claim, also read [references/security-review.md](references/security-review.md).

For each proposed change, state:

1. Which trusted process owns the state and why.
2. Whether data is system-global, active-space-only, app-private, or an explicitly mediated shared resource.
3. What remains mounted, running, or observable while another space is active.
4. How failure returns to or remains in locked state.
5. Which behavior is portable product policy and which implementation is a QEMU, generic-Linux, or exact-target adapter.
6. What evidence demonstrates the claim and what remains unverified.

## Scope discipline

This skill does not authorize physical-device operations, bootloader changes, destructive provisioning, installation of host tools, or broad storage-format changes. Follow repository and target approval gates. Security-sensitive defaults may be prototyped in QEMU, but a mock result must remain labelled `mock` or `QEMU`.
