# Pixel 9a daily-phone todos

Work specs for finishing the exact Pixel 9a `google-pixel-9a-tegu-gxq96-mp1-0`
(tegu / GXQ96 / MP1.0) as a complete phone. They break the gates in
[`../pixel-9a-daily-phone.md`](../pixel-9a-daily-phone.md) into concrete,
reviewable deliverables.

These files add no rules. [`AGENTS.md`](../../AGENTS.md),
[`baseline.md`](../baseline.md) and [`plan.md`](../plan.md) still decide
architecture and safety. GitHub issues own live status and the final
acceptance wording. The board
[capability matrix](../../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/capabilities.toml)
owns capability claims. When a spec here disagrees with one of those, the
other document wins and this spec is corrected.

Written 2026-11-24 from the open issues, `docs/status-log/2026-10.md`,
`docs/status-log/2026-11.md` and the unmerged `origin/pixel-9a-gpu` branch.

## Where the phone stands (2026-11-24)

- The device was restored to pinned stock Android 17 (`CP3A.260905.009`) on
  2026-11-04. Ferrix is **not** on it now. Same-device stock boot
  verification after that restore is still pending.
- Capability matrix: `power`, `display`, `input`, `network` (Wi-Fi) and
  `bluetooth` are `supported`. `audio`, `telephony` and `lifecycle` are
  `unsupported`. There is no camera, location, NFC or fingerprint entry.
- The GPU / Android 16 / per-app APK window work (ADR-008) is on
  `origin/pixel-9a-gpu`: 80 commits, 222 files, not merged into `main`.

## Order

The order follows the evidence dependencies. Offline work in a later file can
run in parallel, but it cannot promote a physical claim.

| # | Spec | Owning issues | Blocks a "complete phone"? |
| --- | --- | --- | --- |
| 00 | [Baseline: land the branch, re-flash, re-verify](00-baseline-sync.md) | #53, #8, #7 (device checks) | Yes: everything else needs a current image |
| 01 | [Cellular modem, SIM, data, SMS](01-cellular-modem.md) | #1, #92, #76, #109, #110, #116, #117 | Yes |
| 02 | [Carrier voice, VoLTE/IMS, emergency](02-voice-ims-emergency.md) | #27 | Yes. Emergency calling is a release blocker |
| 03 | [Audio: speaker, earpiece, mic, call audio](03-audio.md) | #2, #94 | Yes |
| 04 | [Lifecycle: power key, blanking, suspend, sensors](04-lifecycle-suspend-sensors.md) | #9 | Yes |
| 05 | [Charging, thermal, endurance](05-power-thermal-endurance.md) | #26 | Yes |
| 06 | [Wi-Fi, Bluetooth and time hardening](06-wifi-bluetooth-time.md) | #24, #7, #5, #8 | Yes (reliability) |
| 07 | [Camera](07-camera.md) | #3 | Yes, unless the owner narrows the release scope |
| 08 | [Location / GNSS](08-location-gnss.md) | #28 | Yes, unless the owner narrows the release scope |
| 09 | [NFC and fingerprint](09-nfc-fingerprint.md) | #29 | Feasibility only. Owner decides scope |
| 10 | [APK runtime performance](10-apk-runtime-performance.md) | #19, #4 | Yes (responsiveness) |
| 11 | [Product shell completeness](11-product-shell.md) | #12, #21, #22, #23, #14, #20 | Yes |
| 12 | [Lock screen, sandbox, permissions](12-security-privacy.md) | #31, #10, #11 | Yes |
| 13 | [Updates, rollback, data recovery](13-updates-recovery.md) | #30 | Yes |
| 14 | [Release gate](14-release-gate.md) | #32 | It is the release decision |

Critical path: **00 → 04 → 03 → 01 → 02 → 14**. Lifecycle comes early
because suspend, the proximity sensor and screen blanking are prerequisites
for calls and endurance numbers. Audio comes before voice because a
connected call with no audio is not a call.

## Conventions every spec follows

- **Evidence labels:** `QEMU`, `mock`, `offline`, or
  `physical:google-pixel-9a-tegu-gxq96-mp1-0`. Only physical evidence on the
  current image moves the matrix. A service start, a flash exit code or an
  uninspected screenshot is never proof.
- **Owner gates:** each spec has an **Owner-gated** section listing the steps
  an agent must not take without a fresh, specific owner decision. The
  standing gates from `AGENTS.md` and the provisioning skill always apply:
  the recovery invariant, new S5300 mounts or CP boots, SIM/ModemManager
  work, internal speaker/earpiece output, capability promotion and
  Volume Down + Power recovery.
- **Code boundaries:** apps, shell and `ferrix-core` depend only on
  `ferrix-hardware-api` / `ferrix-app-api`. Target code lives in the board
  package and session/bootstrap. Linux services stay behind typed adapters.
  New code is Rust.
- **Before handoff:** `cargo fmt --check`, `cargo clippy --workspace
  --all-targets --all-features -- -D warnings`, `cargo test --workspace`,
  `just qemu-check`, plus `just qemu-scenario-check` and a framebuffer
  capture for scenario or graphical changes.
- **Records:** a dated entry at the top of the current month's
  `docs/status-log/` file. Update the `docs/status.md` tables only with the
  matrix. Keep identifiers, firmware and images out of Git.

When a spec is finished, mark it `done` in its header with the merge commit
and the evidence entry, and keep it as history. Don't delete it.
