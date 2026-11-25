# 08 — Location and GNSS

Status: open; deliverable 1 feasibility done (NO-GO:
[`gnss-source-evidence.md`](../../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/gnss-source-evidence.md))
· Issue: #28 · Depends on: 00, 12 (location
permission) · Sensors for rotation and proximity belong to spec 04

## Goal

Usable location on the exact device, gated by a user permission, with
honest evidence.

## Deliverables

1. **Feasibility (read-only).** Identify the GNSS chip and transport on
   tegu: UART, SPI, or a modem-integrated GNSS reached through the S5300.
   Also record the kernel binding, firmware, and any proprietary userspace
   Android uses. Write `gnss-source-evidence.md` with a GO/NO-GO.
2. **Service boundary.** Use a maintained Linux stack, gpsd and/or
   GeoClue, behind a typed Ferrix location adapter in
   `ferrix-hardware-api`. Keep device wiring in the board package. If GNSS
   is modem-integrated, coordinate through ModemManager's location
   interface (spec 01).
3. **Permission.** Location is a portal capability (spec 12): grant,
   deny, revoke. Revoking takes effect immediately. Add a Settings >
   Location master switch, and a status-bar indicator while in use.
4. **Physical tests.** Outdoors, record:
   - time to first fix, cold and warm;
   - accuracy and repeatability;
   - location off/on;
   - use with the screen off;
   - resume after suspend.
   Keep GNSS results separate from network location. Coordinates never go
   into Git or the evidence; record the accuracy radius and timing only.
5. **Android apps.** Expose location to Waydroid apps through the same
   permission (a mock-location provider fed by the adapter, or Waydroid's
   own route).
6. **Mock.** A QEMU profile entry and a scenario. Never offer QEMU
   coordinates as physical evidence.

## Acceptance

A recorded GO/NO-GO. If GO, measured fixes on the exact device, and a
permission flow proven on QEMU and the device. If NO-GO, a precise blocker
and a visible unsupported state.

## Owner-gated

Outdoor test sessions; the owner carries the phone.
