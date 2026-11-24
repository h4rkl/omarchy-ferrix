# ADR-009: Camera contract and Linux boundary

Status: proposed 2026-11-24 for issue #3 (spec 07 deliverables 1, 2 and 6),
subordinate to `plans/baseline.md`. QEMU/mock evidence only: no target has a
camera adapter, and nothing here is a physical-device claim.

## Problem

Ferrix had no camera abstraction. The Camera tile was a placeholder that
said so. Applications need one portable contract that a mock can serve now
and that a Linux camera stack can serve later, without an application,
`ferrix-core`, or `ferrix-hardware-api` learning about libcamera, PipeWire,
V4L2, an ISP, or a board.

## Decision

1. **Contract in `ferrix-hardware-api`.** `Capability::Camera` joins the
   matrix. The `Camera` trait enumerates cameras (`CameraInfo`: opaque
   adapter-assigned `CameraId`, `CameraFacing` back/front/external, the
   clockwise `CameraRotation` that turns a sensor frame upright on the
   portrait display, preview and still `FrameSize`, and the `CameraMode`s it
   answers), opens a preview (`open_preview` returns a `CameraStream`, one
   per camera at a time, released on drop), and takes a still
   (`capture_still`). Frames are packed 8-bit RGB (`PixelFormat::Rgb24`) in
   sensor orientation; the consumer rotates (and may mirror a front
   preview). **Video is reserved:** `start_video` defaults to the new typed
   `HardwareError::FeatureUnsupported { capability: camera, feature: video }`.
   Every method defaults to a typed error, and `UnsupportedCamera` is the
   composition for every target without an adapter. Nothing imitates a
   camera.
2. **Mock.** `ferrix-hardware-emulator::MockCamera` builds one synthetic
   camera per `[[camera.devices]]` entry of
   `targets/qemu-aarch64/profile.toml` (strictly parsed by
   `ferrix-target-profile`: facing, rotation, `bars` or `grid` pattern,
   preview and still sizes, a `preview_fps` ceiling). Frames are
   deterministic test patterns from the pattern and sequence number, with a
   pale orientation marker in the sensor's top-left corner so a rendered
   preview shows whether the rotation was applied. The mock caps its rate
   at `preview_fps`, as a sensor has a maximum rate.
3. **Serving.** `ferrix-core` serves the session's `Camera` on its own
   socket, `/run/ferrix/camera.sock` (`--camera-socket`,
   `FERRIX_CAMERA_SOCKET`), not on `core.sock`: `core.sock` enforces no
   per-request authorization today, so a camera grant must never expose it.
   Wire: `ferrix-protocol::camera` (line-delimited JSON, hello first,
   `deny_unknown_fields`, frame pixels as base64). A `preview` request turns
   the connection into a frame stream paced at the requested rate; a reader
   that stalls for 3 s (a frozen app) is dropped and the camera released.
4. **Authorization (ADR-004).** New `AppCapability::Camera` (`camera`). The
   native runtime maps it to `camera.sock` in the private `/run/ferrix`
   view, and the camera server resolves every request's `SO_PEERCRED` pid
   through `ferrix-appd` `Identify` and answers `unauthorized` unless the
   effective grants contain `camera` (`camera-request-denied` record).
   `--appd none` denies everyone. The Camera app declares `camera`;
   `apps/grants.json` grants it to `org.ferrix.camera` only. This is the
   checked-in policy, not consent: the runtime prompt is spec 12's portal
   work (`plans/todos/12-security-privacy.md`, "Consent portals").
5. **Linux boundary for a real adapter (not implemented).** A target camera
   adapter is a project-owned Rust crate behind this trait, composed by the
   session/bootstrap layer, that talks to the existing stack instead of
   reimplementing it:
   - **libcamera** is the device layer: pipeline handlers, sensor control,
     the ISP or the software ISP (`simple` pipeline) on raw sensors. The
     adapter uses libcamera's public API (through maintained Rust bindings
     where they fit, or a small C-API shim with a documented reason) and
     never drives V4L2 media graphs, sensor registers, or an ISP itself.
   - **PipeWire** is the sharing layer when more than one consumer needs
     the camera (the Android runtime, a future portal): the adapter may
     consume libcamera through PipeWire's camera nodes (the libcamera SPA
     plugin, mediated by WirePlumber) instead of opening libcamera
     directly. The choice between direct libcamera and the PipeWire node is
     per target and recorded in that target's evidence.
   - Pixel formats are converted to `Rgb24` inside the adapter; a zero-copy
     path (dmabuf or a PipeWire fd handed to the app) is a later,
     backward-compatible addition to the wire contract, not a reason to
     expose a Linux type here.
   - The same adapter answers `Unsupported`, `FeatureUnsupported`,
     `InvalidValue` (unknown id), and `Failed` (busy, sensor error) with the
     existing typed errors.
6. **Camera app.** `ferrix-camera` (`org.ferrix.camera`) is an independent
   native process launched by `ferrix-appd`. It depends on
   `ferrix-hardware-api`, `ferrix-protocol`, `ferrix-app-api`,
   `ferrix-app-protocol`, and `ferrix-ui` only. Preview with rotation and
   front mirroring, shutter, front/back switch, upright PNG stills in
   `~/Pictures` (`IMG_YYYYMMDD_HHMMSS.png`, written then renamed), and a
   hand-off to Files through the new carry-nothing `show-pictures` intent
   (Files resolves `~/Pictures` in its own home). When the capability is
   unsupported, unauthorized, or unreachable it shows that state and
   nothing else.
7. **Capability matrices.** The QEMU target reports `camera = mocked`
   (uniform). The Pixel 9a matrix gains `[camera] status = "unsupported"`
   with "No Ferrix adapter or physical Linux evidence.", and the session
   composes `UnsupportedCamera` for it.

## Evidence and records

Records carry ids, facings, sizes, rotations, sequence numbers, and an FNV
digest of the pixels, never pixels or file names: `camera-ready`,
`camera-list`, `camera-still`, `camera-preview` (`started` / `stopped` /
`sampled`), `camera-request-denied`, `camera-error` (core);
`camera-app-preview`, `camera-app-saved`, `camera-handoff`, `camera-back`,
`ui-state` (app). Scenario `camera-mock-preview-capture` drives the
capability through the mock-control verbs `camera-list`, `camera-capture`,
`camera-preview`, and `camera-video`.

## Limitations

- Frames cross the socket as base64 JSON: fine for the mock's 480x360
  preview, not a design for full-rate sensor video.
- Grants are the checked-in policy; there is no user prompt (spec 12).
- The app does not stop its preview when backgrounded; a frozen app's
  stream ends by the server's write timeout.
- No Linux adapter, no physical evidence, no video, no Android camera
  bridge (spec 07 deliverables 3 to 5).
