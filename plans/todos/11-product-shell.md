# 11 — Product shell completeness

Status: open · ADR:
[010](../../docs/adr/010-shell-chrome-and-geometry.md) (proposed) ·
Issues: #12 (recents, gestures, chrome, supervision), #21 (All apps
drawer), #22 (notifications), #23 (global header/footer across native
and Android), #14 (typography), #20 (icons; reconcile) · Depends on: 00
(the branch's Apps list and ADR-008 shell changes) · Read
`plans/baseline.md` Phase 3 and `plan.md` before starting

## Goal

A complete Phase 3 shell on QEMU and the Pixel:
- launcher and an all-apps drawer;
- compositor-owned status bar and navigation;
- a recents overview with interactive gestures;
- a notification manager and shade;
- quick settings;
- correct text in every script;
- recovery from core and shell crashes.

Native apps and APKs get the same treatment.

## Deliverables

1. **Chrome ADR (#12 / #23).** Decide what the shell draws (time,
   connectivity, battery, the notification entry, Back / Home / Recents)
   and what stays with the app. Define one geometry contract: physical safe
   area + system chrome + client area, in portrait and landscape, with the
   keyboard shown or hidden. App names and icons in chrome come from
   `ferrix-appd` identity, never from a surface title. Do this first,
   because 2–5 depend on it.
2. **Global header and footer (#23).** Draw them in `ferrix-shell` above
   every fullscreen toplevel, including Waydroid tasks. Remove the
   duplicate `clock_strip` and `gesture_pill` from first-party apps.
   Configure Android insets and density so APK content sits between the
   bars, with 1:1 touch mapping. Verify on the Pixel cutout and corners.
3. **Gestures and recents (#12).**
   - Drag-following feedback and a threshold for the bottom-edge Home
     gesture.
   - Swipe-and-hold opens a recents overview with last-frame thumbnails
     taken from the compositor; Android tasks use their task-stream buffer.
   - Dismissing a card stops the app through `ferrix-appd`.
   - A Back gesture or button that maps to Android back for APKs.
4. **All apps drawer (#21).** Upward swipe from Home content (above the
   bottom 8% gesture zone). Alphabetical, live from the `ferrix-appd`
   catalog including APKs, with search using the on-screen keyboard and
   fling scrolling (shared with the branch's Apps list). Home keeps a
   sparse shortcut set, with a documented policy.
5. **Notifications (#22).**
   - A versioned notification contract in `ferrix-app-protocol`: ID,
     authenticated source, category, title and body, urgency, actions,
     replace, expire.
   - A manager that survives restarts.
   - A top-edge shade on Home and over apps, heads-up for urgent items,
     per-app and per-category controls, and size/rate limits.
   - Notification actions go through appd intents.
   - Bridges: a freedesktop D-Bus bridge for Linux apps, and Android
     notifications from Waydroid.
   - Replace the forced raises for SMS, missed calls and pairing with
     notifications.
6. **Quick settings.** Wi-Fi, Bluetooth, mobile data, airplane mode,
   brightness, rotation lock, flashlight (if the hardware allows) and
   Do Not Disturb, in the shade.
7. **Supervision (#12).** Scenarios that kill `ferrix-core` and
   `ferrix-shell` and check that the UI returns with its state reconciled.
   Android windows are already re-adopted after a shell restart (ADR-008);
   extend this to the core.
8. **Typography (#14).** IBM Plex Sans Condensed and Plex Mono, with Noto
   fallback loaded from guest fonts. Shaping and bidi through `cosmic-text`
   (preferred) or rustybuzz + unicode-bidi. Terminal wide-character
   support. QEMU frames showing Latin, Arabic, Devanagari and CJK. Keep
   within a documented `qemu-check` time budget.
9. **Icons (#20).** The Remix Icon v4.7.0 set landed on `main` in
   2026-10. Reconcile it with #20's Material Symbols title: close the
   issue or retitle it with the remaining gaps.

## Acceptance

- Each item verified on QEMU with inspected frames and scenarios, then on
  the Pixel with `tools/pixel-9a/devtest` captures, including an APK in
  front.
- No app can draw or spoof system chrome or the consent prompt (spec 12).
- `just qemu-check` and `just qemu-scenario-check` pass.

## Owner-gated

The chrome ADR and design choices (the command-pill proposal in
`plans/ferrix-mobile-interaction-plan.md` is a draft and needs the owner's
decision before it shapes this work).
