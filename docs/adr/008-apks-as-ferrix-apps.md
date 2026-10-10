# ADR-008: APKs as first-class Ferrix apps

Status: accepted by the repository owner on 2026-10-10 for the Android
compatibility runtime on every route that ships it. Supersedes the
single-surface windowing and single-app association in
[ADR-005](005-android-runtime-and-brave.md) and the browser choice in
[ADR-007](007-fulguris-browser-input.md). The runtime choice (Waydroid) and
the private-adapter boundary of ADR-005 still stand.

## Context

Owner goal: people can install any app, including browsers, and APKs feel
like native Ferrix apps, as fast as the hardware allows. With
`persist.waydroid.multi_windows=false` Waydroid showed the whole Android
display as one surface, so the LineageOS setup wizard and Android home
appeared and Ferrix tied the Browser to whichever Android task was in front
(physical `google-pixel-9a-tegu-gxq96-mp1-0`, 2026-10-10: Browser bound to
`waydroid.org.lineageos.setupwizard`; once the wizard closed, Browser had no
window to activate).

Options researched (sources in #19): Waydroid single container; several
Waydroid instances (unmerged upstream `--instance`, waydroid/waydroid#1990);
redroid (headless, remote display); Android Translation Layer (no Android
userspace, limited app compatibility); an AVF/crosvm Android VM (the Pixel
9a exposes protected KVM, but the guest cannot reach Mali, so graphics would
be emulated); ChromeOS ARC++/ARCVM and Sailfish AppSupport as precedents.
Base images: LineageOS (Waydroid's patch target), plain AOSP, GrapheneOS.

## Decision

1. **One warm Android container, one Ferrix window per Android task.**
   Waydroid runs with `persist.waydroid.multi_windows=true` and upstream's
   task streams (`persist.waydroid.task_streams=true`, hardware/waydroid
   `dev/lineage-23.2`): SurfaceFlinger renders each task into its own buffer,
   the hwcomposer posts it on that task's own Wayland connection as an
   `xdg_toplevel` with `app_id` `waydroid.<package>`. Android's home,
   launcher, setup wizard, status and navigation bars never get a window.
   Isolation between apps is Android's own per-app UID sandbox; multiple
   containers add RAM and boot cost without a stronger boundary (they share
   the kernel). Separate "Androids" (work/personal) use Android user profiles
   inside the one container.
2. **Association.** A toplevel belongs to a Ferrix Android app instance only
   when its client pid is a runtime-reported Android pid (trust anchor) and
   its `app_id` is `waydroid.` + the descriptor's package. `app_id` from any
   other client is untrusted metadata. Runtime toplevels that match no
   running Ferrix app are not shown as, or attached to, another app.
3. **The shell owns window policy.** Every Android toplevel is configured at
   the same full app-area size (a size change resizes the whole Android
   display). Close sends `xdg_toplevel.close` (Android removes the task); the
   card in front is the only `activated` one and has keyboard focus (the
   hwcomposer withholds frames for the rest, which keep their last frame);
   Android's raise requests arrive over `xdg_activation_v1`. Activating an
   Android app with no live window relaunches it through the runtime.
4. **Ferrix owns setup and power.** Provisioning marks Android setup
   complete, disables any setup-wizard package, and keeps Android awake
   (unbounded screen timeout, `svc power stayon true`); Android otherwise
   sleeps on its own timeout and newly launched tasks never draw.
5. **Base image: our own LineageOS 23.2 build,** pinned
   (`tools/android/waydroid-lineage-revisions.tsv`), with Ferrix patches
   (`tools/android/waydroid-patches/`), including linear dma-buf export of
   task-stream buffers to compositors without `android_wlegl`. Prebuilt
   Waydroid images are not used: none ships Android 16 for arm64 with task
   streams, and none carries the Mali/AFBC patches. Not GrapheneOS: its value
   is bare-metal (verified boot, hardened kernel), Waydroid's patches target
   LineageOS, and inside a container little of it remains.
6. **Lean on LineageOS apps.** Built-in LineageOS apps are the defaults
   where they fit, starting with Jelly as `org.ferrix.browser` (system
   WebView, no download, no first-run terms). Fulguris is dropped from the
   default image.
7. **Performance path.** Zero-copy linear dma-buf to ferrix-shell (done);
   stock Pixel Codec2 hardware decoders overlaid like the Mali blobs
   (next); Android's cached-app freezer; container kept warm from boot.

## Follow-ups

- Sandboxed Google Play compatibility (GrapheneOS's gmscompat approach,
  MIT/Apache) ported as patches, for apps that need Google services.
  Play Integrity device checks and Widevine L1 remain out of reach on any
  of these routes.
- Android user profiles for multiple personas.
- A VM-isolated Android when the guest can reach Mali with acceptable cost.
- Android Translation Layer as an optional lightweight tier.

## Evidence (physical `google-pixel-9a-tegu-gxq96-mp1-0`, 2026-10-10)

- Task streams active; Jelly, Calculator and Fulguris each mapped as their
  own `waydroid.<package>` toplevel; frames exported as linear dma-buf
  (`android buffer 1080x2215 fourcc=AB24 stride=4352 exported as linear
  dma-buf`), framebuffer captures inspected.
- Jelly scrolling Wikipedia (12 swipes, `dumpsys gfxinfo`): task streams
  p50/p90/p95/p99 12/18/21/27 ms, 1.37% janky; single window 13/20/22/30 ms,
  1.36% janky. Per-task windows cost nothing measurable.
- YouTube (m.youtube.com) in Jelly plays (software `c2.ffmpeg.vp9.decoder`,
  640x360; 97% of the video area changed between captures 3 s apart).

## Claim boundary

This records one physical target and revision. It does not claim general
Android compatibility, Play-services apps, DRM, or any QEMU result.
