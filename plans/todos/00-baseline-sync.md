# 00 — Baseline: land the GPU stream, re-flash, re-verify

Status: open · Issues: #53 (builder divergence), #8 and #7 (pending device
checks) · Blocks: every other spec

## Why first

`main` and the phone have drifted apart. The device runs stock Android
(restored 2026-11-04). The newest Ferrix product work is on
`origin/pixel-9a-gpu`, which has no PR: Mali on Waydroid, our own LineageOS
23.2 image, task-stream windows, Jelly as the browser, APK icons, Apps-list
fling, and the AoC audio firmware route. Device work on `main` without that
branch would repeat work that has already been done. Device work on the branch
would produce unreviewed physical evidence.

## Deliverables

1. **Verify the stock restore.** Run read-only discovery
   (`skills/mobile-device-provisioning/SKILL.md`) and confirm stock Android 17
   boots on the exact device. Record the result in `docs/status-log/2026-11.md`. The
   2026-11-04 entry still says verification is pending.
2. **Split `origin/pixel-9a-gpu` into reviewed PRs.** Rebase it onto current
   `main` (the branch is 80 commits ahead and `main` has moved about 176
   commits since). Open small, issue-linked PRs, in this order:
   1. Kernel and firmware staging for Mali (`gpu-mali` allowlist rows, the
      `gpu_dvfs_kctx_init` PID-namespace fix, removing `mali_kbase` /
      `mali_pixel` from the module skip list) → #4.
   2. The LineageOS 23.2 Waydroid image build and pins
      (`tools/android/waydroid-lineage-revisions.tsv`, `waydroid-patches/`,
      the Waydroid 1.6.3 aport with the `showApp` backport) → #19.
   3. ADR-008 and the shell and appd changes: task-stream association,
      `wl_surface.enter` for the front window, the opaque backdrop, adoption
      after a shell restart, linux-dmabuf → #19.
   4. Apps list: APK icons and fling → #19 / #21.
   5. The AoC audio firmware route (`pixel-audio`) → #2. Offline only; it
      authorises no playback.
   Each PR passes the full handoff checks. Write the status-log entries from
   the branch's `docs/status.md` into the monthly log format instead of
   carrying over the old single-file layout.
3. **Close #53.** When every builder commit is merged or rejected with a
   reason, record the source-sync path (detached worktree at a reviewed
   `origin/main` commit) and close the issue.
4. **Build and flash a current image** from a reviewed `origin/main` worktree:
   rootfs, `device-google-tegu` 1-r6 or later, Kleaf GKI, and dev-compose with
   `--android`. Use `just pixel-9a-dev-flash` (slot b). Record the source
   commit and the image hashes.
5. **Run the pending device checks** on that image:
   - #8: the five chrony checks from PR #140 (sync within 60 s of
     connectivity, no backwards jump after an offline reboot, a clean
     `rc-status`).
   - #7: join a hidden test SSID, then wrong password → Retry → correct
     password, with framebuffer captures.
6. **Re-baseline the capability evidence.** Re-run the 2026-10-09 checks for
   the five `supported` capabilities on the new image. Update the evidence
   strings only if they changed, and fix the stale "see docs/status.md"
   pointers to name the status-log entry.

## Acceptance

- Stock boot verified and recorded.
- `pixel-9a-gpu` fully landed or rejected item by item. #53 closed.
- One reviewed-source image on the phone, recorded with its source commit
  and hashes, that reaches Home unattended.
- #8 closed. #7 closed, or left with a recorded hardware failure.

## Owner-gated

- How to split the GPU branch: it is the owner's active stream, and #53 says
  the owner decides.
- Nothing else here is gated beyond the standing recovery invariant.

## Risks

- After the restore, the userdata layout changed. The first dev flash
  rebuilds the rootfs and the Wi-Fi profile has to be joined again
  (known behaviour).
- The LineageOS image build is long and runs on the native builder. Keep its
  pins and patches in Git, and its outputs in ignored `out/`.
