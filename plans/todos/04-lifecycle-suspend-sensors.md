# 04 — Lifecycle: power key, screen blanking, suspend/resume, sensors

Status: open · Issue: #9 · Depends on: 00 · Feeds: 02 (proximity), 05
(standby drain), 12 (power-key lock), 14

## Goal

The phone behaves like a phone when idle:
- the power key blanks and wakes the screen;
- an idle timeout blanks it;
- the device suspends and resumes with working wake sources;
- rotation, proximity and brightness respond to real sensors.

`[lifecycle]` is currently `unsupported`. Today the screen never blanks,
the device never suspends, and the power key does nothing.

## Deliverables

1. **Power key.** Identify the input device (PMIC / gpio-keys) under
   libinput on the Kleaf image. `ferrix-shell` treats it as a trusted
   system key that apps never receive:
   - short press: screen off/on;
   - long press: power menu (Restart / Power off) as a trusted shell
     surface;
   - press while locked: wakes into the lock screen (spec 12).
2. **Blanking.**
   - DPMS-off the DRM output through `ferrix-shell` and restore it on wake.
   - Drop touch input while blanked so it never reaches apps.
   - Add an idle timeout setting in Settings > Display, and a wake-lock-like
     inhibit for active calls, video and navigation, owned by `ferrix-core`
     with a typed API.
   - Android: blanking must not leave Waydroid's hwcomposer asleep after
     wake (the `wl_surface.enter` lesson from ADR-008). Verify that the
     front APK redraws after wake.
3. **Suspend.**
   - Find out whether `mem` or `s2idle` works on the GKI (the adapter
     already probes `/sys/power/state`).
   - Test wake sources: power key, RTC alarm, Wi-Fi, Bluetooth, and the
     modem once spec 01 lands.
   - Define a Linux policy: suspend after blank + timeout, with no
     inhibitors.
   - Check that Wi-Fi, Bluetooth, the shell, the Waydroid container
     (freeze and thaw) and the CDC-ACM dev shell survive resume.
   - Put suspend behind a board profile flag until it is stable.
4. **Backlight.** Confirm the Settings brightness rail drives
   `/sys/class/backlight/*` through the adapter, including minimum-level
   safety (never fully dark while on). Use host-webcam evidence.
5. **Sensors, research first.** Find out whether the accelerometer,
   proximity and ambient light sensors are exposed to Linux (IIO) or sit
   behind the AoC/CHRE sensor hub. Write
   `sensors-source-evidence.md` with a GO/NO-GO for each sensor before
   writing code. If they're reachable, use iio-sensor-proxy behind a typed
   adapter for:
   - auto-rotate (the shell already supports output rotation on QEMU);
   - auto-brightness;
   - proximity blanking during calls.
6. **Shutdown and reboot.** Clean OpenRC shutdown from the power menu, with
   no filesystem damage (check the next boot's fsck), and Waydroid stopped
   cleanly.

## Acceptance

- The power key blanks and wakes, with framebuffer and webcam evidence.
- The idle timeout works, and touches are not delivered while blanked.
- Suspend/resume is recorded with its outcome, and its failure reason if it
  fails. If it works, Wi-Fi, the UI and the front APK all come back.
- Sensor GO/NO-GO recorded. Rotation and proximity proven on hardware if GO.
- `[lifecycle]` and the display/brightness evidence updated honestly.

## Owner-gated

The capability promotion. Nothing else beyond the standing invariant.

## Risks

- A suspend that never resumes needs a hard reset. Keep suspend opt-in per
  run until resume works reliably, and make sure the dev shell can turn
  the flag off.
- Without suspend, spec 05's standby numbers will be poor, and that blocks
  release.
