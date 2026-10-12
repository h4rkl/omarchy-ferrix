# Pixel 9a daily-phone delivery plan

## Goal and authority

Deliver Omarchy Ferrix as a dependable, fully functioning **native Linux phone**
on the exact Google Pixel 9a `tegu` / `GXQ96` / `MP1.0`, using postmarketOS and
pmbootstrap for the root filesystem and packaging. Ferrix owns its Rust
services, Wayland shell, applications, and typed hardware contracts. Waydroid
is an optional Android **application** runtime, not the phone's operating
environment. Generic AArch64 QEMU `virt` stays the portable development and
CI contract; a QEMU or mock pass never proves Pixel hardware.

Read this file when resuming this goal. `plans/baseline.md` is the
implementation authority and wins conflicts with `plans/plan.md`, the
long-term direction. `AGENTS.md`, the exact [Pixel board package](../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/README.md),
`docs/status.md`, and the live GitHub issues are required context before
planning a tranche of work. The dated status log contains superseded entries:
use the newest relevant physical evidence and reconcile contradictions before
promoting a claim. For attached-phone work, follow
`skills/mobile-device-provisioning/SKILL.md`; for build, deploy, capture, and
input, follow `skills/ferrix-dev-loop/SKILL.md`.

This plan is the persistent project brief. The [GitHub project](https://github.com/users/h4rkl/projects/1)
tracks live status, priority, assignee, and linked PRs. GitHub issues own
implementation and acceptance detail. `docs/status-log/` owns dated evidence;
the board [capability matrix](../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/capabilities.toml)
owns the current exact-device capability claim. If they disagree, stop the
claim, inspect the newest evidence, and correct the stale document.

## What “fully functioning phone” means here

The release gate is a phone that cold boots without operator intervention and
reliably supports power, charging, thermal protection, screen and touch,
Wi-Fi, Bluetooth, speaker and microphone, camera, location, SIM and carrier
registration, mobile data, SMS, incoming and outgoing calls with two-way
audio, screen lock, and update/recovery without undisclosed data loss. It must
remain usable across app restarts, network loss, suspend/resume, and repeated
days of ordinary use. The default browser and core Ferrix applications must
be responsive enough for that use. Measured results and remaining limits
belong in an exact-revision go/no-go report, not in an unqualified feature
list. postmarketOS's own [main-device criteria](https://docs.postmarketos.org/pmaports/main/packaging/device-categorization.html)
provide a useful external completeness check; they do not certify this port.

**Emergency calling is a release blocker.** Until an approved, independent
verification establishes it, Ferrix must say it is unsupported; ordinary
voice calls, modem enumeration, or an Android stock result do not prove it.
No agent should place an uncoordinated live emergency call. NFC, fingerprint,
hardware GPU acceleration, and broad Android app compatibility require
independent feasibility and evidence. If any remains unavailable, the owner
must decide whether a narrower, explicitly named release target is acceptable;
the project must not silently call that a fully functioning Pixel 9a.

## Evidence snapshot — 2026-10-12 (reconciled after the S2 CP boot)

Labels follow `docs/status.md`: `physical` means the exact
`google-pixel-9a-tegu-gxq96-mp1-0`; `mock`, `offline` and `QEMU` results
never promote a board capability. The [capability matrix](../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/capabilities.toml)
is unchanged by this snapshot: power, display, input, network (Wi-Fi) and
Bluetooth discovery are `supported`; **audio, telephony and lifecycle are
`unsupported`**.

- **Recovery (physical).** The pinned `CP3A.260905.009` factory restore
  completed on the exact device and same-device discovery then verified stock
  Android 17 on slot B, unlocked ([status entry](../docs/status-log/2026-10.md#2026-10-12-pixel-9a-pinned-stock-recovery-verified-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0);
  [#25](https://github.com/h4rkl/omarchy-ferrix/issues/25) closed via
  [PR #36](https://github.com/h4rkl/omarchy-ferrix/pull/36) and
  [PR #47](https://github.com/h4rkl/omarchy-ferrix/pull/47)). The earlier
  out-of-disk attempt and the owner-run `fastboot -w` are superseded history,
  not current blockers. Recheck the recovery invariant before every image
  write. The main checkout carries uncommitted user changes in
  `docs/status.md` and the factory-restore files; preserve them.
- **Current image (physical).** The 1-r3 Kleaf development bundle
  (`tegu-kleaf-dev-20261012T062930Z`; rootfs from `0032a6b`,
  `device-google-tegu` 1-r3, `exynos-modem` and `modemmanager-samsung-sit`
  r3) is on slot B. It was flashed through `just pixel-9a-dev-flash` and
  boots Ferrix on the pinned 6.1.124 kernel. After the S2 run and a reboot,
  the S5300 services are stopped and the CP is `OFFLINE`
  ([1-r3 entry](../docs/status-log/2026-10.md#2026-10-12-1-r3-image-and-s5300-s2-pre-mount-go-issue-92-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0)).
  The checked-in recipes are now `device-google-tegu` 1-r5, so the
  preflight reports image skew until a 1-r5 image is flashed. No 1-r4
  image exists: abuild refused its packaged `/mnt/vendor`.
  The earlier `f49a460` boot showed the `ferrix-basic-screen` reference
  surface, not the product shell; #12 and related shell issues remain open.
- **Modem (physical, S2 CP boot observed; S3 not started).** [#34](https://github.com/h4rkl/omarchy-ferrix/issues/34)
  is closed: one `modem_b` and three per-unit read-only loop mount proofs
  passed and the owner accepted the source-mount gate
  ([record](https://github.com/h4rkl/omarchy-ferrix/issues/34#issuecomment-6116464948)).
  E0 (PR #75), E1 `go` (PR #78), the target-local CPIF adapter (PR #82, offline),
  the #79 NV size gate, #81 private scratch and #88 package/service staging
  are complete. [#92](https://github.com/h4rkl/omarchy-ferrix/issues/92):
  the read-only S2 preflight merged in [PR #104](https://github.com/h4rkl/omarchy-ferrix/pull/104);
  its one physical run on the 1-r2 image returned `no-go` **solely** for
  checked-in 1-r3 versus installed 1-r2 source/image skew, with no device
  blocker ([S2 entry](../docs/status-log/2026-10.md#2026-10-12-s5300-s2-read-only-preflight-issue-92-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0),
  [board record](../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/s5300-s2-preflight.md)).
  The rebuilt 1-r3 image (rootfs attempt `20261012T062930Z` from `0032a6b`,
  same pinned Kleaf kernel) was flashed to slot B and booted. The preflight
  then returned a **pre-mount `go`** with no image skew or blocker
  ([1-r3 entry](../docs/status-log/2026-10.md#2026-10-12-1-r3-image-and-s5300-s2-pre-mount-go-issue-92-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0)).
  The owner then approved one manual S2 run on that image. The lead
  performed it by hand; no checked-in active workflow exists.
  - The first mounts start failed before any mount, because `/mnt/vendor`
    was missing. The first fix (`02eebc7`, PR #113, 1-r4) packaged the
    directory, which abuild refuses under `/mnt`. 1-r5 creates it from
    the package install scripts instead.
  - After a reboot and a one-off directory creation, all four sources
    mounted read-only, and the post-mount preflight returned `go`.
  - The CP stayed `ONLINE` for about 13.9 minutes of samples on one AP
    boot, with no panic, which meets the S2 "ONLINE ≥10 min" criterion.
  - "RFS served" was not observable, because the service keeps no daemon
    output.
  - Teardown findings: stopping the daemon left the CP `ONLINE`, and
    stopping the mounts left the role loops attached. A reboot cleared
    both.

  The record is in the
  [S2 entry](../docs/status-log/2026-10.md#2026-10-12-s5300-s2-active-cp-boot-issue-92-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0)
  and the
  [board record](../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/s5300-s2-preflight.md#exact-device-s2-active-cp-boot-on-the-1-r3-image-2026-10-12).
  No SIM, carrier, ModemManager or `sit-proxy` step has run. An offline review
  ([blockers](../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/s5300-s2-active-blockers.md))
  found that a checked-in active S2 workflow cannot yet be made fail-closed. Its follow-ups
  are [#107](https://github.com/h4rkl/omarchy-ferrix/issues/107) (opener-scan
  positive control; offline fix merged in PR #113, device check pending a
  1-r5 image), [#108](https://github.com/h4rkl/omarchy-ferrix/issues/108)
  (per-unit and per-boot decision binding),
  [#109](https://github.com/h4rkl/omarchy-ferrix/issues/109) (OpenRC teardown
  and disconnect-safe observation design) and
  [#110](https://github.com/h4rkl/omarchy-ferrix/issues/110) (partial-start
  rollback). A partial `ferrix-s5300-mounts` start
  currently requires a reboot. A SIM is physically installed but has not been
  recognised by Ferrix. [#76](https://github.com/h4rkl/omarchy-ferrix/issues/76)
  must correct the FMT codec boundary before tegu control traffic.
- **Audio (physical).** The AoC ALSA card binds. One controlled-tone
  acoustic proof ([#80](https://github.com/h4rkl/omarchy-ferrix/issues/80))
  detected the tone on **channel 0 only**; channel 1 was digital zero. That
  is one pickup observation, not calibrated sensitivity or call audio
  ([entry](../docs/status-log/2026-10.md#2026-10-12-controlled-tone-microphone-pickup-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0)).
  Internal TAS speaker and earpiece playback is **NO-GO** because the
  protection route is unknown (#83, #90 closed;
  [#94](https://github.com/h4rkl/omarchy-ferrix/issues/94) open;
  [research](../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/aoc-speaker-protection-research.md)).
  [#91](https://github.com/h4rkl/omarchy-ferrix/issues/91): read-only
  preflight v3 (PR #101) observed one connected LDAC A2DP sink as
  `candidate-observed`, with `active_gate=closed`; no tone has played
  ([entry](../docs/status-log/2026-10.md#2026-10-12-a2dp-route-source-review-and-preflight-v3-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0),
  [route decision](../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/audio-route-decision.md)).
  The bounded A2DP playback workflow is offline, under review and not merged.
- **Not yet evidenced (physical).** Camera, power-key blanking,
  suspend/resume, real Bluetooth pairing, an SAE-only AP, GNSS, NFC,
  fingerprint, safe charging/thermal behaviour, release-grade updates and
  long-run radio reliability. Mali acceleration remains optional; Fulguris
  performance and video remain work in progress.
- **CI (QEMU/mock).** Native sandbox (#37, PR #41) and arm64 headless QEMU
  (#38, PR #39) gates are closed. [#102](https://github.com/h4rkl/omarchy-ferrix/issues/102)
  closed after [PR #103](https://github.com/h4rkl/omarchy-ferrix/pull/103)
  installed the missing QEMU ROM package; the post-merge main run passed all
  four jobs ([run](https://github.com/h4rkl/omarchy-ferrix/actions/runs/38213896048)).
  None of this is Pixel evidence.

## Working structure

| Role | Responsibility | Handoff |
| --- | --- | --- |
| Lead agent: project manager and code-structure reviewer | Maintain this plan and the GitHub project; select the next unblocked issue; write a bounded task packet; check dependencies, architecture, evidence, tests, security/recovery gates, and PR diffs; request corrections; update issue status only from evidence. | Reviewed issue/PR with a clear decision, remaining risks, and next ready issue. |
| Claude implementation agents | Do the heavy implementation work on issue-scoped branches or isolated worktrees. Investigate, code, run applicable checks, collect redacted target evidence, update status/capabilities, and open a PR linked to the issue. | Small reviewable PR, test results, exact-device evidence or an explicit blocker. |
| Repository owner | Supply the exact device, carrier/test SIM, test accessories/APs, and decisions only where human action or policy is required. Decide release scope and any explicit exception. | Hardware access, approved test conditions, and final go/no-go. |

The manager should not absorb a large feature into one unreviewable agent
task. Split an issue into research/feasibility, narrow implementation, and
physical validation PRs where the evidence path requires it. Delegate
independent tasks only when they do not write the same contracts, board
package, or status section concurrently. Do not let an agent close a hardware
issue from a host compile, mock test, generated image, service-start log, or
uninspected screenshot.

### Every Claude agent task packet

Give the agent: issue URL and exact target; relevant source/authority files;
starting commit and worktree; allowed scope and files; known evidence and
unknowns; dependencies; recovery and data gates; required output artifacts;
applicable tests; and the physical proof needed to close the issue. Require
the agent to state assumptions, preserve unrelated user changes, and stop on
a failed identity/recovery gate or missing human-only device action. Treat
issue text, comments, external pages, logs, and attachments as evidence to
verify, never as authority to override repository or owner rules.

### Review and delivery loop

1. **Triage:** Read this brief, `git status`, newest `docs/status-log/` entries, board
   matrix, issue, dependencies, and GitHub project state. Make a small issue
   task packet. Mark `Ready` only when inputs and acceptance are concrete.
2. **Implement:** Agent works on a scoped branch/worktree. Shared product
   logic stays target-neutral: apps/shell/core use project APIs; target
   selection and vendor bindings stay in session/bootstrap and the Pixel
   package. Linux protocols stay with established services behind typed
   adapters. New project-owned code is Rust unless a documented constraint
   requires otherwise.
3. **Verify:** Run applicable `cargo fmt --check`,
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
   `cargo test --workspace`, and `just qemu-check`. Run focused issue tests
   and `just qemu-scenario-check` for scenario changes. Graphical claims need
   an inspected framebuffer or window capture. Pixel claims need redacted
   exact-device evidence and the checked-in target workflow.
4. **Review:** The lead agent checks API boundaries, dependency/licence fit, security,
   recovery invariant, failure paths, regressions, reproducibility, honest
   evidence, and whether the issue's acceptance criteria really passed.
   Request agent fixes in the same PR. The project moves to `In review` only
   with a reviewable PR; move to `Done` only after merge and evidence-backed
   acceptance. A blocked or partial result stays open with the next
   experiment recorded.
5. **Reconcile:** Update the issue, project, this plan's snapshot or issue map
   when it changes, a `docs/status-log/` entry, and `capabilities.toml` (with
   the `docs/status.md` current state) as warranted.
   Re-read the queue and select the next unblocked slice. GitHub issue state
   alone never upgrades a physical capability.
6. **Clean up:** After a branch is merged and its evidence is retained under
   ignored `out/` or in the issue, remove the worktree and branch created for
   that delivery. Preserve unrelated or user-created worktrees and uncommitted
   changes.

The GitHub project uses `Backlog → Ready → In progress → In review → Done`.
Use `P0` for recovery and core-phone blockers, `P1` for completion/reliability,
and `P2` for conditional enhancements. Keep only a few issues `Ready`; a
physical validation task is not ready merely because its implementation
compiles. Link PRs to issues and record blocked dependencies in the issue
body/comment, rather than relying on board order as a dependency graph.

## Ordered work and issue map

The order follows evidence dependencies, not numeric issue order. Offline
work can proceed while a physical gate is pending, but cannot promote a
physical claim.

| Gate | Existing issues | New issues and required result |
| --- | --- | --- |
| 0. Known-good device and recovery | Existing Pixel workflows and board proposal | [#25](https://github.com/h4rkl/omarchy-ferrix/issues/25) closed: pinned stock restore and same-device Android 17 boot verified. Recheck the recovery invariant before every image write. |
| 1. Basic phone services | [#1](https://github.com/h4rkl/omarchy-ferrix/issues/1) S5300 modem/SIM/data/SMS/call state; [#2](https://github.com/h4rkl/omarchy-ferrix/issues/2) playback/capture/routing | Completed prerequisites: #34 source mounts, #79 NV sizes, #81 scratch, #88 staging; #55/#59 AoC/TAS binding, #80 mic pickup, #83/#90 speaker NO-GO. Open: [#92](https://github.com/h4rkl/omarchy-ferrix/issues/92) bounded manual CP boot and teardown (S2 met by one manual run on 1-r3: ONLINE about 13.9 min, no panic; RFS service not observed; new follow-ups for CP left `ONLINE` after a daemon stop and role loops left attached after a mounts stop; a checked-in active workflow is still blocked on #107 on-device control (1-r5), #108 decision binding, #109 teardown/observation design (PR #114); optional #110 rollback; fresh owner decision per mounts start and CP boot; S3 not started); [#76](https://github.com/h4rkl/omarchy-ferrix/issues/76) FMT codec for tegu SIT; [#91](https://github.com/h4rkl/omarchy-ferrix/issues/91) A2DP output without TAS; [#94](https://github.com/h4rkl/omarchy-ferrix/issues/94) speaker protection and gain contract; [#27](https://github.com/h4rkl/omarchy-ferrix/issues/27) ordinary voice, VoLTE/IMS, call audio and separate emergency readiness. #27 end-to-end proof depends on #1 and #2. |
| 2. Power and radios | [#9](https://github.com/h4rkl/omarchy-ferrix/issues/9) power key, blanking, suspend/wake, sensors; [#24](https://github.com/h4rkl/omarchy-ferrix/issues/24) Wi-Fi hardening; [#5](https://github.com/h4rkl/omarchy-ferrix/issues/5) Bluetooth pairing; [#7](https://github.com/h4rkl/omarchy-ferrix/issues/7) hidden Wi-Fi; [#8](https://github.com/h4rkl/omarchy-ferrix/issues/8) time/RTC | [#26](https://github.com/h4rkl/omarchy-ferrix/issues/26): charge/thermal/endurance, with screen-off drain dependent on #9. |
| 3. Camera, location and other hardware | [#3](https://github.com/h4rkl/omarchy-ferrix/issues/3) camera; #9 owns orientation/proximity/ambient sensors | [#28](https://github.com/h4rkl/omarchy-ferrix/issues/28) GNSS/location; [#29](https://github.com/h4rkl/omarchy-ferrix/issues/29) NFC/fingerprint feasibility and proof. |
| 4. App and shell usability | [#19](https://github.com/h4rkl/omarchy-ferrix/issues/19) browser performance/video/screen fit; [#4](https://github.com/h4rkl/omarchy-ferrix/issues/4) optional GPU; [#12](https://github.com/h4rkl/omarchy-ferrix/issues/12), [#21](https://github.com/h4rkl/omarchy-ferrix/issues/21), [#22](https://github.com/h4rkl/omarchy-ferrix/issues/22), [#23](https://github.com/h4rkl/omarchy-ferrix/issues/23) shell/navigation/notifications; [#14](https://github.com/h4rkl/omarchy-ferrix/issues/14) text; [#16](https://github.com/h4rkl/omarchy-ferrix/issues/16) pmOS QEMU graphics | No duplicate browser or shell issue. Keep browser usability evidence distinct from hardware GPU support. |
| 5. Personal data and maintenance | [#10](https://github.com/h4rkl/omarchy-ferrix/issues/10) sandbox; [#11](https://github.com/h4rkl/omarchy-ferrix/issues/11) permissions | [#30](https://github.com/h4rkl/omarchy-ferrix/issues/30) upgrade/rollback/data recovery; [#31](https://github.com/h4rkl/omarchy-ferrix/issues/31) lock screen/data protection. Do not infer Android verified boot on this native route. |
| 6. Daily-phone decision | All required issues above | [#32](https://github.com/h4rkl/omarchy-ferrix/issues/32): repeatable end-to-end and endurance suite, exact-revision go/no-go and explicit remaining limits. |

Issue #20 remains a UI/icon follow-up; its open state must be reconciled with
the newer icon evidence in `docs/status-log/` before scheduling more icon work.
[#53](https://github.com/h4rkl/omarchy-ferrix/issues/53) owns review of
unpublished builder commits through separate PRs; do not merge or reset that
checkout wholesale. [#99](https://github.com/h4rkl/omarchy-ferrix/issues/99)
tracks a host rootfs-build password-quoting fix; it adds no phone evidence.
Closed issues #6, #13, #15, and #17 are historical inputs, not proof of Pixel
daily-phone acceptance. Do not open duplicates for work already owned above.

Closed CI and tooling gates — #37, #38, #42, #46, #50, #52 and #102 — keep
other PR checks meaningful but satisfy no phone capability. A new red main
check blocks merges until it is diagnosed in its own issue.

## Physical-device invariant and evidence rules

The one hard Pixel development-route gate is restorable stock firmware. Follow
the exact exception in `AGENTS.md` and the mobile provisioning skill: one
identified `tegu` / `GXQ96` / `MP1.0` device, unlocked ordinary Fastboot,
hash-verified pinned factory archive, tested restore, writes only to
factory-rewritten partitions, no relock/erase/format/OEM operation, and stock
firmware on the target slot before switching slots. Agents may use the
checked-in `pixel-9a-dev-flash`, `pixel-9a-enter-fastboot`, and
`pixel-9a-factory-restore` workflows under that policy. If the phone does not
return to Fastboot, the human must use Volume Down + Power; stop and ask.
Never bypass a failed invariant to keep a sprint moving.

Keep serials, subscriber data, phone numbers, addresses, credentials, raw
partition dumps, firmware, backups, and generated images out of Git. Use
redacted, reproducible records under ignored `out/`; committed evidence must
name `QEMU`, `mock`, or `physical:google-pixel-9a-tegu-gxq96-mp1-0` and
include the image/build identity, observed behaviour, test conditions, and
limitations. A real service process, simulated UI, and a successful flash
command are each weaker than a working end-to-end feature. Capture and
inspect pixels for graphical claims; use the host webcam when display,
charging LED, touch, or physical panel behaviour needs external observation.

## Resume checklist

On every new work session: read this goal and authority section; check
`git status` and preserve user edits; fetch the live GitHub project/issues;
read the newest exact-device status and capability matrix; confirm the
recovery invariant still holds; identify one small unblocked issue; send
Claude an issue-scoped task packet; review its PR and evidence; then
reconcile issue state, plan, status and matrix.

Next actionable step from this snapshot: #92's S2 CP boot was observed
once, by hand. First, build and flash a 1-r5 image. Its install scripts
create `/mnt/vendor` (the 1-r4 packaged directory failed abuild's `/mnt`
rule), and it carries the #107 opener-scan control. Then repeat the pre-mount
preflight until it reports `opener_positive_control=detected`. File the two
teardown follow-ups (the CP stays `ONLINE` after a daemon stop; role loops
stay attached after a mounts stop), and feed them into the #109 design
(PR #114). S3 planning (`sit-proxy`, ModemManager, SIM) waits on the owner
review of that design. #108 stays a read-only, offline-first slice. #110 is
needed only if the owner wants recovery without a reboot. Before any audible
run, review #91's revised A2DP playback workflow and merge it, then obtain a
fresh, single-run owner decision. Continue #94 and #76 off-device. The standing goal remains a fully
functioning postmarketOS-based Pixel 9a phone until the owner changes it or
#32 records an evidence-backed release decision.
