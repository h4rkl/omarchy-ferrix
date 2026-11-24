# 01 — Cellular modem: SIM, registration, data, SMS

Status: open · Issues: #1 (parent), #92 (S2 CP boot proof), #76 (FMT codec),
#109 (teardown design), #110 (partial-start rollback), #116 (CP stays
ONLINE), #117 (loops stay attached) · Depends on: 00 · Feeds: 02, 03, 14

## Goal

On a cold boot with no manual steps, the Samsung S5300 modem appears in
ModemManager. Phone, Messages and Settings > Mobile network then show, through
`ferrix-telephonyd`: SIM state, operator, signal, packet data with the
configured APN, SMS send and receive, and call state. Voice audio and IMS
are spec 02.

## Where it stands

- Route A is the Google CPIF character devices on the Kleaf GKI kernel,
  bridged by the target-local `exynos-modem` daemon and `sit-proxy` into
  ModemManager's SIT path. The radio-first `s5300_modem` kernel route stays
  `experimental-not-adopted`.
- Done (physical): read-only source mounts (#34), the NV metadata-size gate
  (#79), private scratch (#81), package staging (#88), and the offline CPIF
  adapter (PR #82). One manual S2 CP boot on the 1-r3 image held `ONLINE`
  for about 13.9 min with no panic. The 1-r5 pre-mount preflight returned
  `go` on 4 of 4 runs, with the opener positive control detected.
- Done (offline): teardown analysis (PR #137, `s5300-s2-teardown.md`).
- Open: a checked-in, fail-closed active S2 workflow does not exist yet.
  Teardown needs a reboot because the CP stays `ONLINE` after a daemon stop
  (#116) and the role loops stay attached after a mounts stop (#117). "RFS
  served" cannot be observed because `exynos-modem` sets no `output_log`.
- Done (offline, 2026-11-25): S3 ModemManager CPIF binding. A hash-pinned
  fork patch, gated udev tags and a `sit-proxy` re-trigger hand `umts_ipc0/1`
  (proxy sockets only) and `rmnet0` to the SIT plugin as one modem
  ([`s5300-s3-mm-binding.md`](../../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/s5300-s3-mm-binding.md)).
  Expect no IMEI: the plugin has no SIT identity loader, and there is no AT
  port on CPIF.
- Not started on device: S3 (`mmcli -L`, SIM, registration).
  A SIM is in the phone but Ferrix has never read it.
- Ready on QEMU/mock: `ferrix-telephonyd`, `ferrix-telephony-mm`, Phone,
  Messages, the SMS store, and the APN through `FERRIX_TELEPHONY_APN`.

## Deliverables

### Stage S2 — close #92 (fail-closed CP boot and teardown)

1. **#76, offline.** Scope the seven-byte `sipc_fmt_hdr` codec in
   `crates/ferrix-s5300-cpif` to SIPC only. Document that tegu uses
   `PROTOCOL_SIT` (12-byte link header stripped by the kernel), with
   citations to the pinned source functions. Add fixture tests and a guard
   that tegu code cannot select the SIPC codec.
2. **#109, design.** Owner answers to #114 Q1–Q10, then a checked-in design:
   - guest-side bounded observation that survives a host disconnect;
   - STOP criteria;
   - the never-forced teardown order (`sit-proxy` → `exynos-modem` →
     `ferrix-s5300-mounts`), with a reboot fallback;
   - `output_log` capture for `exynos-modem`, pinned by the preflight;
   - a reviewed list of CPIF crash and kernel-log signatures.
3. **#116, decision.** The recommendation is (a): a reboot ends every S2/S3
   run. Write it into the teardown contract. Option (c), a separately armed
   CP-off step, comes later only if reboots get costly. Never use
   `IOCTL_POWER_RESET` as a power-off.
4. **#117.** Read-only characterisation (tiers 1–2 in the teardown doc) to
   find what holds the loop open; Waydroid/LXC mount namespaces are the
   leading suspect. Then add a fail-closed `backing_file` re-check in
   `detach_loop` if approved. Do this together with #110 so the #34 service
   changes only once: one pin bump, one pkgrel, one set of per-role proofs.
5. **The active S2 workflow.** Add it as a `just` recipe:
   - redacted preflight;
   - explicit mounts start and re-verify;
   - provision `/run/exynos-modem`;
   - arm once, then start the daemon;
   - bounded observation with RFS-served evidence from the daemon log;
   - teardown, then reboot.
   Offline negative tests first. Then one exact-device run.

### Stage S3 — ModemManager and SIM (separate owner decision)

6. Start `sit-proxy` and let ModemManager's SIT plugin attach. Prove, in
   order, each with a redacted record:
   1. the modem is enumerated in `mmcli -L`;
   2. SIM state (present / PIN / ready);
   3. registration and operator;
   4. signal quality.
   Never record IMEI, IMSI, ICCID, MSISDN or cell IDs.
7. **Data.** Set the APN in `/etc/ferrix/backends.env` (a non-secret value
   that is not committed for a real carrier). Bring up the bearer through
   NetworkManager / ModemManager, then prove DHCP or a static bearer, the
   default route, DNS, and a reachability check. Show that Wi-Fi to cellular
   fallback works in `ferrix-networkd`.
8. **SMS.** Send and receive between Messages and an owner-controlled test
   number. Check delivery state, the persisted store, and the
   notification/raise path (moves to the spec 11 notification API when
   that exists).
9. **Call state, without audio.** Dial out, ring in, answer, reject and hang
   up through Phone, with typed states. Call audio is spec 03 + 02.
10. **Cold boot.** The whole chain starts from OpenRC with no manual steps,
    `rc-status` is clean, and a modem crash is reported as a typed
    `telephony` failure, not a hang.
11. **Settings > Mobile network.** SIM ready, operator, bars, data toggle
    and APN view, with framebuffer captures.

## Acceptance

- #76, #109, #116, #117 (and #110 if chosen) closed with their recorded
  decisions.
- #92 closed: an accepted, fail-closed S2 workflow that has run once on the
  exact device.
- Physical evidence for every S3 step above on a cold boot, redacted.
- `[telephony]` promoted only for what was shown. Emergency stays
  `unsupported{feature: emergency}` (ADR-001).

## Owner-gated

Every mounts start, every CP boot, the S3 start, any SIM or ModemManager
step, any change to the accepted #34 service, any APN for a real carrier,
and the capability promotion.

## Risks

- ModemManager's Samsung SIT support may not cover tegu's firmware. If S3
  fails at the plugin level, decide between patching the plugin upstream
  and a target-local adapter. Don't reimplement the protocol in product code.
- Each run currently needs a reboot. Budget device time accordingly.
