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

- Compositor restarts (implemented 2026-10-11, device verification
  pending): the warm container must outlive ferrix-shell. The container
  sees the shell through a bound socket *directory* and a symlink at
  Waydroid's socket path, so the hwcomposer's own reconnect reaches a
  restarted shell; `ferrix-appd` keeps Android instances across the gap
  (docs/android-runtime.md §7 "Compositor restarts").

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

## Installed APKs (implementation record, 2026-10-10)

Owner decision (c): any APK a person installs behaves as a native Ferrix app.
Recorded design, within decisions 1–3 above:

- **Listing source: `waydroid app list`**, through the adapter's existing
  bounded CLI wrapper (`AndroidHost::installed_packages`). It is Android's
  PackageManager view over Waydroid's maintained CLI, independent of which
  account owns the session and of its home directory, and the call
  `is_installed` already depends on. The generated
  `waydroid.<pkg>.desktop` entries were rejected as the source: they are a
  side effect of the session-side user monitor, live in the session user's
  XDG data directory (`ferrix-appd` may run as another account), and are
  written only while that monitor runs. Only packages with an
  `android.intent.category.LAUNCHER` activity are apps. The CLI prints
  nothing while the session is stopped, so an empty listing is
  "unavailable", never "everything uninstalled".
- **Id scheme:** `apk.<package>` when the package is already a valid Ferrix
  id (lowercase letters and digits, e.g. `apk.org.example.notes`);
  otherwise `apk.<readable>.x-<fnv1a64 hex>` (packages with uppercase or
  `_`). Only the second form contains `-`, so the forms cannot collide.
  The `apk.` prefix (`ferrix_app_api::INSTALLED_APP_PREFIX`) is reserved:
  a checked-in descriptor using it is rejected, so an installed package
  can never shadow a catalog entry.
- **Dedupe:** a package a checked-in descriptor already launches (same
  runtime `app_key`, the package) is not listed again:
  `org.lineageos.jelly` stays `org.ferrix.browser`, labelled "Browser".
- **Descriptor:** `origin: "installed"` (new optional `AppDescriptor`
  field; absent means a checked-in entry), Android's label (control
  characters dropped, at most 40 characters), the label's first letter as
  glyph, the steel tone of the first-party utility tiles (the rust accent
  stays the Terminal's). **Trust:** always `untrusted`, no declared
  capabilities, no intents, no autostart, and no grants row, so zero
  effective Ferrix capabilities; Android's own permission model governs
  the app inside the container (ADR-004/005 unchanged).
- **Never apps:** Android's shell: `com.android.launcher3`,
  `org.lineageos.trebuchet` (both disabled by provisioning),
  `com.android.settings` (its `FallbackHome` is Android's home once the
  launcher is disabled), `com.android.systemui`, and the setup wizards.
  Their task windows stay withheld.
- **Refresh:** a poller in `ferrix-appd` lists every 30 s, every 10 s
  while the listing is unavailable (Android booting), every 300 s while no
  runtime lists apps (Waydroid absent); a toplevel from the runtime's
  composer with an unknown `app_id` (once per id) and a `not-installed`
  launch wake it early. A change publishes `CatalogChanged`; the home
  surface re-fetches. An app removed while running keeps its entry until
  it ends; a runtime whose listing fails keeps its last entries.
- **Adoption (decision 2 completed):** a composer toplevel no running
  instance claims is adopted when its client pid is one the runtime
  reports right now (`Runtime::shared_host_pids`, the trust anchor) and
  its `app_id` is the window id of a launchable catalog or installed app.
  `ferrix-appd` then attaches (`Runtime::attach`: supervise, never
  relaunch) a new instance that owns the window. It is raised when the app
  in front is itself an Android app (the intent's origin); otherwise it
  waits in the background until the person opens it. Android's later
  raise requests reach it through `xdg_activation_v1` as before.
- **Launcher (revised 2026-10-11):** installed apps are not home grid
  tiles. The grid keeps the first-party entries at the designed tile size
  plus an Apps tile; the app list (Apps tile or an upward swipe on home)
  holds every entry sorted by label, and installed apps carry a quiet
  "Android" line so an APK named like a first-party app (Contacts, Files,
  Camera) stays distinguishable. Appending them to the grid (the first
  cut) shrank all 18 tiles on the Pixel 9a until labels overlapped glyphs.
  They never borrow a first-party glyph or reference page.
- **Unavailable listings (2026-10-11):** the Android runtime answers a
  listing only while the session is `RUNNING` before and after it,
  `sys.boot_completed` is `1` (when `waydroid shell` can tell), and at
  least one launchable package comes back; anything else is an error.
  `ferrix-appd` also treats an empty `Ok` listing as unavailable. Both keep
  the last known entries: the Pixel 9a logged `apps:0 removed:10` during a
  session restart before this.

- **Icons (2026-10-11):** Waydroid's Android side writes one PNG per
  package to Android's `/data/icons/`, which on the host is
  `$XDG_DATA_HOME/waydroid/data/icons/<package>.png`
  (`$XDG_DATA_HOME` defaulting to `~/.local/share`) of the account that ran
  `waydroid session start`. Source, Waydroid 1.6.3:
  `tools/config/__init__.py` (`waydroid_data = xdg_data_home +
  "/waydroid/data"`), `tools/helpers/lxc.py` (binds it as the container's
  `data`), `tools/services/user_manager.py` (`Icon=` of each desktop entry
  is `<waydroid_data>/icons/<package>.png`). The Android adapter reads it
  (`ferrix_runtime_android::icon`): the directory is
  `FERRIX_WAYDROID_ICON_DIR` when set (for an appd account other than the
  session user), else derived from `ferrix-appd`'s own `XDG_DATA_HOME` /
  `HOME` once at start. On the Pixel 9a both `ferrix-appd` and the session
  it starts run as `user` with `HOME=/home/user`, so the path is
  `/home/user/.local/share/waydroid/data/icons/<package>.png`. The files are
  container-written, untrusted input: the name comes from a validated
  package only; the file must be a regular file and not a symlink, with the
  same device and inode before and after opening; at most 1 MiB is read;
  the maintained `png` crate (already in the tree) decodes it with an 8 MiB
  memory limit, images larger than 1024 px per side are refused before
  their pixels are allocated, and a box filter in premultiplied alpha
  reduces it to at most 96x96. Fully transparent results are dropped.
  `ferrix-appd` serves only installed entries (`Request::AppIcon`, any
  role, like `Catalog`); first-party entries, including the Jelly-backed
  Browser, always get `null`. The home fetches the bitmaps on a
  short-lived background Home connection after each catalog update (at
  most 64 per update), caches successes per id, retries failures at the
  next catalog change, and draws the bitmap (bicubic, aspect kept) in the
  same icon box; any failure leaves the letter glyph.

Limitations: icon reads are unverified on the device: whether Waydroid
wrote an icon for every listed package, the files' owner and mode under
`/home/user/.local/share/waydroid/data/icons` (container-written, so
`ferrix-appd` may lack read permission), and how they look at the Pixel's
density are open. An app update that changes its icon shows the old
bitmap until the app leaves the catalog or the home restarts (successes
are cached per id). Android's adaptive-icon shape is whatever Waydroid
rendered; Ferrix applies no mask. Each poll costs one `waydroid app list`
subprocess (event-driven refresh from Waydroid's package-state callback
is follow-up work). Evidence is `mock` only (`cargo test`, host preview);
no physical or QEMU run has exercised installed APKs or their icons.
