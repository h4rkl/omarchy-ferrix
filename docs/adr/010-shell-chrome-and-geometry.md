# ADR-010: Shell chrome, geometry contract and anti-spoofing

Status: proposed 2026-11-25 for spec 11 deliverable 1
([`plans/todos/11-product-shell.md`](../../plans/todos/11-product-shell.md)),
issues #12 and #23. Owner review required. Subordinate to
`plans/baseline.md`. Nothing here is implemented. Evidence cited is `QEMU`,
`mock`, or `physical:google-pixel-9a-tegu-gxq96-mp1-0` source reading as
labelled; this record makes no new physical claim.

## Context

Today each app draws its own system chrome, and the shell draws almost
none:

- **Apps draw chrome.** `ferrix_ui::widgets::clock_strip` (72 design px at
  y = 0 of the app's buffer) and `gesture_pill` (320x12 design px at
  `height - 60`) are in `crates/ferrix-ui/src/widgets.rs`. Contacts,
  Terminal, Files, Phone, Settings, Messages and Camera call both. Home
  (`ferrix-basic-screen`) has its own `status_bar` with clock, cellular,
  Wi-Fi and battery.
- **The shell draws little.** `crates/ferrix-shell/src/compositor/render.rs`
  draws the clear colour, an opaque backdrop, the power menu
  (`power_menu.rs`), and a solid "system pill". The pill is drawn only
  when the focused window's xdg `app_id` does not start with `org.ferrix.`
  (`layout.rs`, `draws_own_pill`). There is no status bar, clock, battery,
  connectivity, Back, recents or overview in the shell.
- **The trusted bottom zone is 3 %.** `gesture.rs` `START_ZONE = 0.97`.
  Contacts that start there are grabbed and never reach clients
  (`compositor/input.rs`). Tap or short swipe means Home; a long swipe
  (≥ 0.55 of the shorter side) means Close. `docs/app-platform.md`
  "Trusted gestures" says "bottom 8%". The code and the Pixel profile say
  3 %. This drift must be resolved by this ADR.
- **Back is app-owned.** Apps draw it (`widgets::back_rect`) and send
  `Report::YieldForeground` at their root (`docs/app-platform.md`).
- **Geometry.** `layout.rs` `OutputLayout::plan` picks the rotation.
  `client_rect(safe)` is the output minus the profile `SafeArea {top,
  bottom, left, right}`, and the insets rotate with the content.
  `toplevel_configure_size` gives every toplevel one identical size,
  because Waydroid's hwcomposer resizes the whole Android display on any
  configure (ADR-008 §3). The profile `scale` is parsed, but the output is
  `Scale::Integer(1)` (`compositor/udev.rs`, `compositor/winit.rs`).
- **Measured panel values.** The Pixel 9a profile
  (`targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/dev/profile.toml`) has
  1080x2424 and `safe_area = { top = 137, bottom = 72 }`. Its comment
  records values from the factory overlay:
  - hole-punch circle at (540, 86), r 41.5 (rect 484..596 x 0..152);
  - corner radius 115 px, top and bottom;
  - the stock status bar is 52 dp at density 420, which is 137 px.
  The QEMU profile (1080x2340) has no `safe_area`, so the shell pill is
  never drawn there.
- **The keyboard is drawn in the app.** Native apps draw the OSK inside
  their own buffer (`crates/ferrix-ui/src/osk.rs`). The shell has no
  text-input, input-method or virtual-keyboard protocol. Android draws its
  own IME inside its task surface.
- **Waydroid tasks** are one `xdg_toplevel` per task with `app_id`
  `waydroid.<package>` (ADR-008).
  - Android's status and navigation bars get no window.
  - Density comes only from `ro.sf.lcd_density`: 420 on the Pixel
    (`pmos/android/waydroid-properties.ini`) and 400 on QEMU.
  - The repo sets no Android-side inset, overscan or `wm size` override.
  - Whether Android still reserves status/navigation-bar insets inside its
    task display has not been measured.
  - Touch reaches Android as `wl_touch`, offset by `client_area().loc`.
- **Identity.** The shell reports `SurfaceMapped {pid, app_id, title,
  surface}` with the kernel pid (`SO_PEERCRED`). It receives
  `Notification::Activate {pid, app_id: AppId, surface}` from appd
  (`crates/ferrix-app-protocol/src/lib.rs`). The `AppId` is used only for
  evidence. The shell never requests `Catalog` or `AppIcon`.
- **Spoofing holes today:**
  - `WlrLayerShellState` is created with no global filter
    (`compositor/mod.rs`), and any namespace and layer is mapped. Top and
    Overlay layers render above the toplevel and the pill and are
    hit-tested first. No first-party crate uses layer-shell.
  - xdg popups are constrained to the full output rectangle, not the
    client area, so a popup can cover the insets.
  - Toplevels themselves are clamped to the client area.

## Constraints

- Baseline Phase 3: launcher, status surfaces, gestures, OSK, rotation,
  quick settings, notifications, crash recovery; software rendering by
  default; acceleration never required for correctness.
- `AGENTS.md` product shell rules: minimal, portrait, touch-first, focused
  fullscreen flows, no desktop window management. UX order: explicit
  requirements, then `docs/design.md`, then GrapheneOS conventions.
- `docs/design.md`: the status strip shows only time, connectivity and
  power. Only the trusted home indicator is pill-shaped. No translucency,
  so no app content may show behind a system decision.
- Apps depend on `ferrix-app-api` / `ferrix-app-protocol`, never on
  `ferrix-shell` internals. Target values (cutout, corners) come from the
  board profile through session/bootstrap, never from app conditionals.
- Identity is kernel-anchored (`docs/app-platform.md`). A surface title or
  `app_id` is untrusted metadata.
- Spec 12 requires a shell-drawn consent prompt and lock surface that no
  app can draw or cover.

## Options

### A. Shell-drawn opaque header and footer, clients between them

The compositor draws a header band and a footer band on the software path,
outside every client's rectangle. Clients get only the client area. This
is the current `client_rect` model, with the shell filling the insets
instead of leaving them blank.

- **For:** works for native apps and Waydroid with no new client protocol.
  The bands are physically unreachable by client pixels, so they are a
  trusted strip. Matches the "no translucency" rule. It is a small step
  from today's code.
- **Against:** apps cannot go edge-to-edge (a camera viewfinder or a video
  cannot draw under the bands). The bands cost about 9 % of the Pixel
  panel (137 + 72 of 2424 px). A future immersive mode needs an explicit,
  shell-owned exception.

### B. Edge-to-edge clients with communicated insets

Clients fill the panel. The shell tells each client the insets (a
project-owned Wayland protocol, or a field over the appd socket). Chrome
is drawn over content.

- **For:** maximum content area; matches Android's edge-to-edge model.
- **Against:**
  - There is no standard Wayland inset protocol, so every client needs a
    custom one.
  - Waydroid can take only one display size for all tasks, so per-task
    insets do not map.
  - Chrome over content needs translucency or covers content, against
    `docs/design.md`.
  - The chrome band is no longer a region apps cannot draw, which weakens
    anti-spoofing.

### C. Command pill footer (draft, `plans/ferrix-mobile-interaction-plan.md`)

This is A's geometry with different footer behaviour. The footer holds a
persistent command pill:

- tap means Home;
- swipe up opens the launcher and command palette;
- hold opens the command sheet;
- hold, drag and release runs a radial command;
- a horizontal swipe changes workspace;
- an edge swipe means Back.

- **For:** it is Omarchy's command-first model and a distinct product
  identity. Its gesture-arbitration order (emergency, then accessibility,
  then lock/system modal, then edges, then pill, then app) is sound and
  is adopted below regardless of this option.
- **Against:**
  - It is a draft with no prototype.
  - It introduces workspaces, which the baseline does not call for and
    which ADR-008's single-size Android display does not yet support.
  - Radial commands need haptics, which no target has evidenced.
  - It overloads the 72 px band that the Home/Close gestures already use.
  - The draft itself says to validate it before treating it as a contract.

### D. Keep app-drawn chrome

- **Against:** apps can spoof it, it duplicates across seven apps, and
  Waydroid tasks get none. This contradicts #12 and #23. It is listed only
  as the baseline we are leaving.

## Recommendation

Adopt **A**. Keep the footer as today's gesture model, with shell-owned
Back. Keep **C** as a later footer-behaviour experiment inside A's
geometry, gated on a prototype (see owner decisions). Reject B and D.

### What the shell draws (header)

- **Left:** time, as `HH:MM` in the 24 h or 12 h locale setting.
- **Right:**
  - cellular: signal and registration from `telephonyd`;
  - Wi-Fi: state from `networkd`;
  - battery: percent and glyph, with charging shown, from `ferrix-core`.
- **The whole header is the notification entry.** A downward drag that
  starts in the header opens the shade (#22) on Home and over any app. A
  tap on the header opens quick settings. The shell itself has no
  separate notification icon. When unread notifications exist it shows
  one compact dot, left of the cutout, with no count or app identity
  while locked (ADR-011).
- The header never shows a surface title. When the shell names an app (in
  recents cards, a consent prompt or a crash notice), the label and icon
  come from appd `Request::Catalog` / `Request::AppIcon`, keyed by the
  `AppId` in `Notification::Activate`. That `AppId` is resolved from the
  kernel pid, never from `app_id` or a title.
- The header sources its data as a `system`-role client of `ferrix-core`,
  `networkd` and `telephonyd`. If a source is unreachable, it shows a
  typed "unavailable" glyph rather than a stale value.

### What the shell draws (footer) and navigation

- **Footer:** an opaque band with the pill-shaped home indicator, always
  drawn, for every app including `org.ferrix.*`. The `draws_own_pill`
  exception is removed.
- **Home:** tap or short swipe up from the footer (unchanged).
- **Close:** long swipe up from the footer (unchanged, until recents
  ships).
- **Recents:** swipe up and hold from the footer (spec 11 deliverable 3).
  Cards use appd identity and the compositor's last-frame thumbnails.
- **Back becomes a compositor gesture.** It is an inward swipe that starts
  in a narrow side-edge strip of the client area (proposed 24 design px
  each side; owner decision below).
  - The shell keeps that contact and does not deliver it.
  - For a native app, the shell sends a new `Notification::Back
    {surface}` over the app socket. The app navigates or yields
    (`YieldForeground`) as it does today.
  - For an Android task, the shell sends `KEY_BACK` to that task's
    surface as a `wl_keyboard` key. Whether Waydroid maps it to Android
    Back must be measured (evidence plan).
  - The app-drawn header back control (`back_rect`) stays an in-app
    affordance. It is not system chrome.
- **Optional three-button footer:** Back, Home, Recents, for
  accessibility (GrapheneOS offers both). It is a setting, not the
  default. It uses the same footer band and the same actions.

### What stays with the app

- Page titles, in-app back affordances, browser address bars and
  controls, tabs, and app toolbars.
- Each app's own scroll and gesture handling in the client area, except
  the reserved side-edge Back strips.
- First-party apps stop calling `clock_strip` and `gesture_pill`
  (deliverable 2). `page_header` stays.

### Geometry contract

One pure function in `ferrix-shell`, `Geometry::resolve(panel,
board_shape, orientation, keyboard)`, replaces `client_rect(safe)`. It is
host-tested and logged as a `geometry` record. It produces four nested
rectangles in output coordinates:

1. **Panel:** the physical pixels.
2. **Physical safe area:** the panel minus the cutout and the corner
   radii. It comes from a new board-profile `[display.shape]` (cutout
   rect or circle, corner radii). The existing `safe_area` becomes a
   derived minimum, not the only input.
3. **System chrome:** header and footer bands.
   - Header height = max(the safe-area top in this orientation, the
     design minimum of 72 design px scaled).
   - Footer height = max(the safe-area bottom, the gesture zone).
   - Status content is laid out around the cutout rectangle and never
     inside it. On the Pixel, time goes left of x = 484 and icons right
     of x = 596.
4. **Client area:** what remains. It is the only rectangle a client
   surface, popup or subsurface may cover.

Orientation rules:

- **Portrait:** header on the top edge, footer on the bottom edge.
- **Landscape:** the header stays on the content's top edge, shortened to
  the design minimum. The cutout moves to a side edge and becomes a side
  inset of the client area on that side only. The footer stays on the
  content's bottom edge at the gesture-zone height. Both corner-radius
  insets apply. Landscape is a reflow (`docs/design.md`); clients get a
  configure with the landscape client size.
- **Rotation lock and the F9 debug rotation** go through the same
  function.

Keyboard rules:

- **Native apps, now:** the app-drawn OSK lives inside the client area.
  The geometry does not change.
- **Native apps, later:** when the OSK moves into the shell
  (`text-input-v3` + `input-method-v2`), the keyboard becomes a fourth
  band above the footer. The client area shrinks and the shell re-sends
  configure.
- **Android tasks:** keep Android's own IME. Reconfiguring would resize
  every Android task (ADR-008), and Android already handles
  `windowSoftInputMode` inside its display.

The gesture zone is defined once, as a fraction plus a minimum in pixels.
`docs/app-platform.md` is corrected to the chosen value.

### Waydroid tasks: insets, density, touch

- The Android display size equals the client area (already true through
  configure). The client area already excludes the cutout and the bands,
  so Android needs no cutout and no system-bar insets of its own.
- Ship a Ferrix runtime resource overlay (RRO) in the pinned LineageOS
  build (`tools/android/waydroid-patches/`) to stop Android reserving
  insets:
  - `status_bar_height` 0;
  - `navigation_bar_height` 0;
  - `config_showNavigationBar` false;
  - no display cutout;
  - gesture navigation insets 0.
  Then content sits exactly between the Ferrix bands with no double
  padding.
- Density stays `ro.sf.lcd_density` from the board's
  `waydroid-properties.ini`: 420 on the Pixel, to match stock dp sizing.
  It is recorded in the geometry record.
- **Touch is 1:1.** The surface is at scale 1 and placed at the client
  area origin. A touch at output (x, y) reaches Android at (x - cx,
  y - cy) with no scaling. The shell never scales an Android buffer.
- A bottom-edge contact never reaches Android, so Android's own gesture
  navigation must be disabled in the overlay as well.

### Anti-spoofing

- **Layer-shell.** Filter the `wlr-layer-shell` global with Smithay's
  `WlrLayerShellState::new_with_filter`. Only clients whose kernel pid appd
  identifies as a trusted system component may bind it. Today that is
  none. Until a trusted client exists, no client sees the global.
- **Popups and subsurfaces.** Constrain them to the client area, not
  `output_rect()`. Anything outside is clipped.
- **Z-order is fixed.** From bottom to top:
  1. client area;
  2. header and footer;
  3. system modals: the power menu, consent prompts, the lock surface and
     the crash notice.
  System modals take all input. They are opaque and drawn by the shell in
  its own buffer, like `power_menu.rs`.
- **Identity on prompts.** Consent prompts and other system modals show
  their requester's identity in the header band, from appd. An app cannot
  draw in the header band, so a fake prompt inside the client area always
  lacks the trusted header line. The design rule: a system decision is
  never drawn only inside the client area.
- **Tap-jacking.** A prompt action accepts only a contact that started
  after the prompt was fully drawn. It also ignores contacts during the
  first 500 ms (proposed). A prompt never appears under a finger that is
  already down.
- **Reserved inputs.** The power key, the footer gestures and the
  side-edge Back strips are taken before any client. An app cannot opt
  out of them. Arbitration order (from the draft): emergency/security
  hardware, accessibility, lock/system modal, system edges, footer, app.

## Consequences

- **Shell work:**
  - a status data path to three daemons;
  - header and footer rendering;
  - a geometry function plus `[display.shape]` parsing in
    `ferrix-target-profile`;
  - the layer-shell filter;
  - popup confinement;
  - `Notification::Back`, which needs an app-protocol schema bump.
- **Apps:** seven apps drop `clock_strip` and `gesture_pill`. Home drops
  its own `status_bar`. Each gains up to 72 design px of client height on
  QEMU, where today it pays for its own strip.
- **QEMU:** QEMU gets real header and footer bands. Its client area
  shrinks from 1080x2340. Existing frame captures and scenario coordinates
  that assume full-panel clients change. Add an optional mock
  `[display.shape]` (a fake cutout) to the QEMU profile so the cutout
  geometry is tested in CI, labelled `mock`.
- **Android:** the RRO is a LineageOS image change. It needs a rebuild
  and a new pin (`waydroid-lineage-revisions.tsv`).
- **Docs:** `docs/design.md` gains header/footer components and the
  "system decisions are never client-area only" rule.
  `docs/app-platform.md` "Trusted gestures" gets the corrected zone
  fraction and shell-owned Back.
- **Command pill (C)** is not foreclosed. It would change footer
  behaviour only, not geometry.

## Evidence and validation plan

- **mock:** host tests for `Geometry::resolve`:
  - portrait and landscape, each rotation;
  - cutout on each edge, corner radii, keyboard band;
  - the invariant that header, footer and client area are disjoint and
    cover the panel's safe region;
  - a property test that no client, popup or subsurface rectangle leaves
    the client area.
- **mock:** layer-shell tests.
  - A test client from an untrusted pid cannot bind layer-shell.
  - A popup at (0, 0) is clipped to the client area.
  - The side-edge Back strip never delivers a touch to the client.
- **QEMU:**
  - Agent-session frames of Home, Settings, Terminal and Camera with the
    shell header and footer and no duplicate strip.
  - Landscape frames.
  - A mock-cutout frame.
  - A scenario that taps a known client-area coordinate and asserts the
    client received the same local coordinate.
  - A scenario: Back gesture → `back` record → app navigates.
  - A malicious test client that tries layer-shell → `denied` record.
  - Frames are inspected, not only logged.
- **physical:google-pixel-9a-tegu-gxq96-mp1-0:**
  - `tools/pixel-9a/devtest` captures, portrait and landscape, of a
    native app and Jelly in front;
  - header content clear of the cutout (484..596);
  - no Android status or navigation bar and no double padding;
  - Android `dumpsys window` insets reading 0 after the RRO;
  - `KEY_BACK` reaching Android;
  - a touch-mapping probe: a tap at known panel coordinates against a
    test APK that reports `MotionEvent` coordinates, with 0 px expected
    offset;
  - webcam frames to confirm corners.
- Until those runs exist, nothing about Pixel fit is claimed beyond the
  profile values already recorded.

## Owner decisions needed

- Approve option A (shell-drawn opaque bands) and reject edge-to-edge (B)
  for now.
- **Command pill (C):**
  - Defer it to a prototype inside A's footer, or
  - make it the default footer now, which also means accepting
    workspaces into Phase 3 scope.
- **Gesture zone size:**
  - keep 3 % (72 px on the Pixel, as coded), or
  - adopt the 8 % that `docs/app-platform.md` states.
  The doc is corrected either way.
- **Back:**
  - make Back a shell-owned side-edge gesture (recommended), with a strip
    width (proposed 24 design px), or
  - keep Back app-owned.
- Whether a three-button footer is offered as a setting in Phase 3.
- Whether the header ever shows the foreground app's name. The
  recommendation is no: identity appears in recents, prompts and crash
  notices only.
- Approve the Ferrix RRO in the pinned LineageOS build to zero Android
  system-bar insets.
- Whether an immersive or fullscreen exception (video, camera viewfinder)
  is allowed in Phase 3, and who may request it. The recommendation is
  not in Phase 3.
- The tap-jacking delay for prompts (proposed 500 ms).
