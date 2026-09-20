# Implementation Path and Evidence Gates

Read this reference when converting the encrypted-spaces blueprint into a roadmap or repository changes. Implement narrow vertical slices; do not create empty crates for the full design at once.

## Current-to-future mapping

| Current component | Encrypted-spaces responsibility |
| --- | --- |
| `ferrix-shell` | Add authoritative locked state, trusted lock rendering/input, surface suppression, and locked-frame clearing. |
| `ferrix-session` | Retain target selection and adapter composition; do not turn this library itself into the privileged credential daemon by accident. |
| proposed `ferrix-sessiond` | Own authentication, state machine, keys, mounts, private session runtime, and fail-closed cleanup. |
| `ferrix-appd` | Run as a fresh active-space-scoped service; continue to own app catalog/lifecycle inside that space. |
| `ferrix-runtime-native` / `ferrix-sandbox` | Gain restricted filesystem views, session endpoints, app-private writable storage, and stronger identity/resource isolation. |
| `ferrix-core` | Remain the global hardware/domain service; require authenticated, space-scoped access instead of accepting every reachable socket client. |
| `ferrix-basic-screen` | Remain launcher/home only; read active-space data but never handle PINs, raw keys, mounts, or space enumeration. |
| guest startup | Start the locked compositor and session authority before any space launcher or ordinary application. |
| QEMU target | Provide explicit persistent test media and deterministic mock key protection without claiming secure hardware. |

The current application platform shares a UID, filesystem, and service sockets. Its namespaces provide process/crash containment, not space data isolation. Do not declare an encrypted-spaces milestone complete merely because two launchers render.

## Phase 0: approve the contract

Outcome:

- Record the threat model, non-goals, one-active-space invariant, cover-space semantics, notification default, and target-adapter boundary in authoritative project documentation.
- Decide the initial encrypted-storage substrate through a fit note covering kernel support, tooling, licensing, recovery, key rotation, corruption behavior, and QEMU operation.
- Define machine-readable protection levels without implying hardware support.
- Specify persistent QEMU disk creation and cleanup; generated volumes, credentials, and keys remain ignored.

Evidence:

- Architecture review against `plans/baseline.md`, `plans/plan.md`, `docs/app-platform.md`, and repository boundaries.
- No physical-target or forensic-deniability claim.
- Existing headless QEMU check remains ephemeral and unchanged unless the approved baseline explicitly revises it.

## Phase 1: pure session state machine

Outcome:

- Add target-neutral types for locked, unlocking, active, locking, and error-locked transitions.
- Model operations through fake storage, key-protector, runtime, and clock adapters.
- Guarantee rollback of every partially completed unlock step.
- Guarantee that a second unlock cannot begin while any prior space remains active or incompletely cleaned.
- Redact credentials and internal space identities from errors and evidence.

Evidence:

- Exhaustive transition and invalid-transition unit tests.
- Failure injection after each unlock and lock step.
- Property or table-driven tests that never reach `Active` without mounted storage, scoped endpoints, appd readiness, and launcher readiness.
- Tests that every failure ends locked with no retained fake key/mount/process capability.

## Phase 2: compositor-enforced lock

Outcome:

- Boot graphical sessions into a shell-owned locked surface before `ferrix-appd` starts.
- Route lock input only through trusted compositor code.
- Suppress ordinary surfaces, activation requests, screenshots, and app input while locked.
- Clear the previous frame before or as lock becomes visible.
- Use a private authenticated channel between shell and session authority.

Evidence:

- Host tests for input and surface routing in every state.
- A malicious/untrusted test client cannot cover the lock surface or receive lock PIN events.
- QEMU framebuffer inspection shows boot-locked, wrong-PIN-still-locked, and successful transition frames.
- Serial records report states without PIN length/value, key material, or sensitive space labels.

## Phase 3: two persistent QEMU spaces

Outcome:

- Create two generated encrypted test spaces under resolved ignored QEMU output paths.
- Use checked-in non-secret fixtures only to drive deterministic tests; never commit real credentials, volume contents, recovery keys, or generated images.
- A primary test credential and a cover test credential unlock distinct persistent data and launcher state.
- Only the selected space is mapped and mounted.
- Application-visible paths remain the same in both sessions.
- Lock terminates the complete process domain before unmount and key revocation.

Evidence:

- A bounded QEMU integration workflow proves primary -> lock -> cover -> lock -> primary persistence.
- Marker data from one space is unreachable from the other through files, app APIs, descriptors, recents, and service sockets.
- Block mappings, mounts, process trees, and keys are inspected after each lock and reported in redacted machine-readable records.
- Power loss during writes has a documented recovery result and never causes fallback into another space.
- The ordinary `just qemu-check` remains headless, TCG-compatible, and noninteractive; add a separate bounded check if persistence would burden the baseline smoke test.

## Phase 4: space-scoped app platform

Outcome:

- Start a fresh `ferrix-appd` after each successful mount and stop it during lock.
- Keep public `AppId` semantics unchanged because only one space is active.
- Store launcher layout, app settings, permissions, data, histories, and crash records inside the space.
- Give native apps a restricted mount view and app-private writable directory.
- Authenticate appd/core/portal clients through kernel identity or inherited capabilities, not request fields or environment variables alone.
- Ensure application-held descriptors cannot prevent bounded teardown indefinitely.

Evidence:

- Apps in cover and private spaces see identical public paths but distinct contents.
- A test app cannot open internal mount roots, inactive volumes, another app's private directory, or system-private sockets.
- A process that forks helpers, holds files open, crashes, or ignores termination is fully removed before unmount.
- Reusing an `AppId` in another space does not reuse lifecycle, notification, permission, or crash state.

## Phase 5: notification and shared-resource policy

Outcome:

- Keep inactive-space notifications absent by default.
- Clear clipboard, recents, share targets, thumbnails, media sessions, notification presentation, and crash UI on lock.
- Define explicit behavior for calls, SMS, alarms, emergency actions, networking, removable media, and shared files.
- If sealed delivery is added, the system broker stores opaque payloads it cannot decrypt and exposes nothing to the cover space.
- Put convenience options behind clear, testable policy; never silently weaken a private/cover space.

Evidence:

- Cross-space leak tests for every system surface and portal.
- No inactive app identifier, icon, sender, content, count, badge, or event appears in the cover space.
- System-global events are tested against the documented routing policy.
- Broker compromise tests demonstrate the intended metadata boundary; document unavoidable metadata honestly.

## Phase 6: recovery and credential lifecycle

Outcome:

- Add credential enrollment, rotation, and optional recovery without rewriting the space data volume.
- Reject duplicate credentials inside an already authenticated management flow.
- Apply global attempt throttling and bounded resource use.
- Define safe behavior for corruption, forgotten credentials, exhausted recovery, and interrupted key rotation.
- Keep cover credential behavior indistinguishable from ordinary successful unlock at the product interaction level.

Evidence:

- Rotation invalidates the old credential and preserves data.
- Failure injection cannot leave an unprotected data key or unusable only copy of key metadata.
- Wrong credentials never mount a volume or reveal whether a particular space exists.
- Logs and support bundles contain no credentials, key material, salts tied to user-visible labels, or sensitive volume contents.

## Phase 7: exact physical target hardening

Begin only under the repository's approved board-support process.

Outcome:

- Wire a target key protector and attempt limiter without changing apps or launcher logic.
- Define verified/authenticated boot, rollback, update, and recovery behavior for the exact target revision.
- Account for swap, hibernation, crash dumps, DMA, secure display/input limitations, flash behavior, and recovery tooling.
- Report whether protection is software-only or hardware-backed and what remains exportable.

Evidence:

- Exact model/codename/revision and boot-chain evidence.
- Hardware-bound key tests that do not expose production secrets.
- Retry enforcement and power-cycle behavior.
- Recovery from failed update, corrupted volume, and interrupted lock without weakening authentication.
- Claims remain target-specific; QEMU evidence is not promoted into hardware evidence.

## Cross-cutting review questions

For every slice, answer:

- Can an ordinary app spoof the trusted caller or retain access after lock begins?
- What plaintext remains outside the encrypted space?
- What identifiers or metadata can the cover space observe?
- What happens if each participating process crashes at each transition?
- Is cleanup bounded, verified, and fail-closed?
- Is the implementation replaceable without changing applications?
- Did a target-specific detail leak into a product crate or public app contract?
- Does evidence distinguish mock, QEMU, generic Linux, and exact physical target?
- Does the UI claim more protection than has been demonstrated?

## Validation

Run the applicable repository gates:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
just qemu-check
```

Graphical lock or transition changes also require the documented graphical workflow, inspection/capture of the actual framebuffer, machine-readable readiness/failure evidence, and an honest `docs/status.md` update. Security-sensitive integration checks must be bounded and noninteractive in CI.
