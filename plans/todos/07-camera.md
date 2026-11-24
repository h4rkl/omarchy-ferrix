# 07 — Camera

Status: open; deliverable 3 Pixel research done (NO-GO:
[`camera-source-evidence.md`](../../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/camera-source-evidence.md))
· Issue: #3 · Depends on: 00 · Release: required
unless the owner narrows the release scope

## Goal

A product-wide camera contract, a Camera app, and, if feasible, a working
Pixel 9a camera. No target has any camera support today. The Camera tile
honestly says there is no camera capability.

## Deliverables

1. **Contract, QEMU/mock first.**
   - Typed camera API in `ferrix-hardware-api`: enumerate, facing and
     orientation, a preview stream, still capture, and later video. The
     default is `Unsupported`.
   - An ADR in `docs/adr/` that sets the Linux boundary: libcamera and/or a
     PipeWire camera node behind a project-owned adapter.
   - A mock backed by synthetic frames from `targets/qemu-aarch64/profile.toml`,
     plus a QEMU scenario.
2. **Camera app.** Preview, shutter, front/back switch, and saving to
   `~/Pictures` with a hand-off to Files. Uses the shared `ferrix-ui`
   widgets and `docs/design.md`. Add the camera permission to spec 12's
   portal.
3. **Pixel research, read-only.**
   - Inventory `/dev/video*` and `/dev/media*`, and the camera, ISP and CSI
     modules in the Kleaf vendor_dlkm set.
   - Find out whether any sensor streams raw Bayer through V4L2 without
     the Tensor ISP, and whether libcamera's simple pipeline plus a software
     ISP could produce usable images.
   - Write `camera-source-evidence.md` with a GO/NO-GO and what a GO needs.
4. **Pixel adapter (only if GO).** Raw capture → libcamera soft ISP →
   preview at a usable frame rate. Stills first, video later.
5. **Android camera.** Decide whether APKs get camera access through
   Waydroid, and how. This depends on 4.
6. **Capability entry.** Add `[camera]` to the Pixel matrix as
   `unsupported` with the reason until there is evidence. Add the mock
   entry for QEMU.

## Acceptance

- The API and mock exist, with unit tests, a QEMU scenario, and an inspected
  preview capture.
- The Pixel research records a clear GO/NO-GO.
- If GO: a physical photo taken in the Camera app and saved, plus a preview
  frame rate measurement.

## Owner-gated

The release-scope decision if the result is NO-GO.

## Risks

- Tensor ISP support on Linux probably doesn't exist. A raw-sensor plus soft
  ISP route may give poor image quality. The realistic outcome may be
  "camera unsupported" plus a narrower release target.
