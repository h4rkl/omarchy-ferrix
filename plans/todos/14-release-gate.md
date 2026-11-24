# 14 — Daily-phone release gate

Status: open · Issue: #32 · Depends on: all of 00–13

## Goal

An evidence-backed go/no-go decision for this exact revision, as a
"fully functioning phone" or as an explicitly narrowed target the owner has
approved.

## Deliverables

1. **Feature matrix.** In `plans/pixel-9a-daily-phone.md`, one row per
   required feature with the owning spec and issue, its evidence level
   (`physical` / `mock` / none), the newest evidence entry, and any blocker.
2. **End-to-end suite.** A checked-in, repeatable run with physical steps
   marked as attended. It covers:
   - cold boot to Home;
   - Wi-Fi join and reconnect;
   - Bluetooth headset;
   - browser and APK launch;
   - charging;
   - screen off/on and suspend/resume;
   - camera;
   - location;
   - SIM, data and SMS;
   - incoming and outgoing calls with audio;
   - notifications and the lock screen;
   - an update and a rollback;
   - stock restore.
   Add each step as its spec becomes real.
3. **Endurance.** A multi-hour and multi-day ordinary-use run that records
   the image hashes, the duration, the repeat count, battery, temperature,
   memory, crashes (appd, shell and Android tombstones), reconnects and
   failures.
4. **Blocker review.** These always block release:
   - emergency calling unsupported without an owner-approved exception;
   - any unresolved data-loss path;
   - unsafe charging or thermal behaviour;
   - inability to restore pinned stock;
   - a missing core phone function (calls, SMS, data, audio).
   Every exception is written down with the owner's approval.
5. **Report.** A short go/no-go report: the exact revision, the image
   hashes, the matrix, the measured results, the limits, and links to
   redacted evidence. Add it to `docs/status-log/` and summarise it in
   `docs/status.md`.

## Acceptance

- Every required feature has an exact-device result or an explicit
  unresolved blocker. Closing an issue never stands in for physical proof.
- The suite and the endurance run are recorded.
- The plan, the capability matrix and `docs/status.md` agree.
- The go/no-go decision is explicit.

## Owner-gated

The release scope, any exceptions, and the final decision.
