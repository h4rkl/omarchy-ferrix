# 10 — APK runtime performance

Status: open · Issues: #19 (APK/browser path), #4 (Mali) · Authority:
ADR-008 (on `origin/pixel-9a-gpu` until spec 00 lands it) · Depends on: 00

## Goal

Installed APKs feel like native Ferrix apps on the Pixel 9a:
- fast cold and warm launch;
- smooth scrolling;
- hardware video;
- no visible Android.
Every change is measured against a recorded baseline.

## Where it stands

From ADR-008 and the 2026-10-10/11 physical entries on the branch:

- **Done:** Mali-G715 with the stock GLES blobs inside Waydroid, our own
  LineageOS 23.2 image, task streams (one Ferrix window per Android task),
  linear dma-buf export to `ferrix-shell`, keep-awake via
  `wl_surface.enter`, adoption after a shell restart, hardware VP9
  (`c2.exynos.vp9.decoder`), and APK icons.
- **Measured** (`dumpsys gfxinfo`):

  | Case | p50 | p90 | p95 | p99 | Janky |
  | --- | --- | --- | --- | --- | --- |
  | Software path (SwiftShader, LOS20) | 81 ms | 97 ms | 101 ms | 113 ms | 95.4 % |
  | Jelly / Wikipedia (task streams) | 12 ms | 18 ms | 21 ms | 27 ms | 1.37 % |
  | YouTube feed (with autoplay) | 12 ms | 18 ms | 21 ms | 28 ms | 5.7 % |

  Hardware VP9 playback uses about 30 % of 8 CPUs. Android uses about
  1.26 GB PSS; disabling Launcher3 saved 62 MB.
- **Already ruled out:** CPU DVFS. `schedutil` is active, all 8 cores are
  online, and frequencies scale (`gpu-research.md`).
- **Already decided** (ADR-008 §7): the container is kept warm from boot,
  and Android's cached-app freezer is used.
- **Open:** AV1 hardware decode shows edge artifacts (AFBC); Android has no
  audio sink and no Bluetooth; `ferrix-shell` composites in software.

## Deliverables

1. **Benchmark recipe first.** Add a checked-in `just pixel-9a-apk-bench`
   (or a `devtest` script) that records, for a fixed APK set (Jelly, Clock,
   one heavy third-party APK):
   - cold launch (`am start -W` TotalTime, and tap-to-first-frame from the
     shell's records);
   - warm launch;
   - a 12-swipe `dumpsys gfxinfo` scroll;
   - `dumpsys SurfaceFlinger --latency`;
   - CPU busy;
   - PSS.
   Output is a machine-readable record under ignored `out/`. Every later
   item is judged against this baseline.
2. **Compositor path.**
   - Measure what `ferrix-shell` does with a fullscreen Android dma-buf
     today: a pixman composite (a CPU read of a linear buffer) or a direct
     scan-out.
   - If it composites, add direct scan-out of the front fullscreen client's
     buffer to a DRM plane when nothing overlaps it, with the software path
     as the fallback. Acceleration must never be required for correctness.
   - Sync frame pacing to page-flip events and `wl_surface.frame`, replacing
     the 16 ms timer, and redraw only damaged regions.
3. **AFBC.** Linear buffers cost memory bandwidth. Assess AFBC end to end:
   the DPU's scan-out support, the modifier advertised over linux-dmabuf,
   and the software fallback when the shell has to composite. This would
   also explain the AV1 edge artifacts.
4. **ART compilation.**
   - Confirm that the LineageOS build pre-compiles (`WITH_DEXPREOPT`) the
     system apps and the WebView.
   - After any APK install, run `cmd package compile -m speed-profile`
     (or `speed` for the default browser) in the provisioning and install
     path.
   - Measure first-launch time before and after.
5. **Touch and launch boost.** Linux has no Android PowerHAL input boost.
   Prototype a small Rust service, or a `ferrix-core` policy, that raises
   `cpu.uclamp.min` on the focused app's cgroup (native, or the Waydroid
   container's top-app cgroup) for a short window on touch-down, launch and
   app switch. Measure whether launch time and first-frame jank improve.
   Keep it only if they do.
6. **Container warmth and memory.**
   - Verify the ADR-008 warm-from-boot behaviour and the cached-app freezer
     on the device.
   - Remove unused LineageOS packages from the image.
   - Tune lmkd and zram against PSS and the Ferrix native apps' needs.
   - Measure the boot-time cost of the warm container, and the benefit to
     launch time.
7. **Codecs.** Fix or keep excluding AV1 hardware decode (AFBC), confirm
   the H.264/HEVC hardware decoders, and verify that Codec2 selection keeps
   them as the default.
8. **Android ↔ Ferrix integration** (with spec 11):
   - Ferrix on-screen keyboard latency into Android text fields;
   - rotation;
   - back gesture mapped to Android back;
   - notifications from APKs bridged to the Ferrix manager;
   - audio sink (spec 03);
   - insets and density matching the Pixel safe area (#19 step 6).
9. **Panel refresh.** Evaluate 120 Hz (the mode list, then the
   power/smoothness trade-off with spec 05). Keep 60 Hz as the default
   until measured.

## Acceptance

- A checked-in bench with a recorded baseline and a result after each
  change.
- Targets, to be confirmed by the owner:
  - p95 scroll ≤ 16.7 ms and < 2 % janky for Jelly;
  - cold launch ≤ 1.5× stock Android on the same APK;
  - warm launch < 500 ms.
- No regression on QEMU. The software path is still tested in
  `just qemu-check`.

## Owner-gated

Performance targets, the image package removals, and default refresh rate.

## Risks

- Direct scan-out depends on DPU plane formats and modifiers; it may need
  AFBC or a specific stride.
- Stock blob ABI drift if the pinned factory image moves to a new version
  (`tools/pixel-9a/waydroid-mali-abi-check.sh` exists on the branch for
  this).
