# Catacomb evaluation and compositor decision

Scope: AGENTS.md requires evaluating Catacomb reuse / contribution / fork before
writing a new compositor, and documenting the gaps before choosing Smithay
directly. This is that record. Evidence labels: everything here is desk review
of upstream sources plus `QEMU`/`mock` runs of the resulting `mobile-shell`;
nothing is physical-device validation.

Sources inspected (2026-09-20): `https://github.com/catacombing/catacomb` at
`d7163b5` ("Catacomb version 1.5.0", 2026-04-16), its `Cargo.toml`, `LICENSE`,
`CHANGELOG.md`, `README.md` and `src/`; Smithay `v0.7.0` (crates.io, MIT,
tagged 2025-06-24) and its `anvil`/`smallvil` examples.

## Catacomb facts

| Topic | Finding |
| --- | --- |
| License | **GPL-3.0-only** (`Cargo.toml`, `LICENSE`). The `catacomb_ipc` sub-crate and `catacomb_common` git dependency are part of the same GPL project. |
| Smithay version | Git pin `smithay` rev `dba255cd…` (no crates.io release), `default-features = false`. Catacomb tracks Smithay `master`; Smithay's last crates.io release is `0.7.0` (June 2025), so the two are not directly interchangeable. |
| Maintenance cadence | Active, single maintainer (Christian Duerr). Releases 1.1.0 (2026-02-09), 1.2.0 (02-23), 1.3.0 (03-08), 1.4.0 (03-19), 1.5.0 (04-16): roughly monthly, fix-heavy, PinePhone (Pro) focused. `rust-version = 1.92`, edition 2024, ~10.3 kLOC. |
| Backends | **udev/DRM only** (`src/udev.rs`: `LibSeatSession`, `Libinput::new_with_udev`, `DrmDevice`, `GbmDevice`, `GbmAllocator`, `GbmFramebufferExporter`). There is no winit/X11/nested backend and no headless backend; `main.rs` only dispatches `udev::run()` or the IPC client. A handful of comments mention "nested compositors" but only to cancel touch gestures. |
| Rendering | **GLES only**: `renderer_gl` feature, `GlesRenderer`/`GlesTexture` throughout `src/drawing.rs` and custom `RenderElement<GlesRenderer>` impls. No pixman path, so correctness requires a working EGL/GBM stack (Mesa) in the guest. |
| Input / gestures | Touch-first: `src/input.rs` implements a bottom "gesture handle" (vertical drag = overview/minimize, horizontal = cycle), configurable user gestures by screen sector (`GestureSector`, `GestureBinding` via IPC), double-tap cycling, drag-and-drop in the overview, keybindings via IPC, and a "display off" input mode. Pointer input is supported but secondary. |
| Virtual keyboard / input method | Yes: `delegate_virtual_keyboard_manager`, `delegate_input_method_manager`, `delegate_text_input_manager` (Smithay's zwp protocols). The OSK itself is a separate client (the Catacomb project ships `Tzompantli`/`Squeekboard`-style clients outside this repo). |
| Orientation / rotation | Yes: `src/orientation/` reads accelerometers via `iio-sensor-proxy` (D-Bus, `zbus`, Polkit rule) or raw udev IIO devices; IPC orientation lock. |
| Protocols | Broad: compositor, xdg-shell/decoration/activation, layer-shell, data control, dmabuf, fractional scale, idle inhibit/notify, screencopy, image-copy-capture, foreign-toplevel-list, keyboard-shortcuts-inhibit, presentation, primary selection, session lock, single pixel buffer, viewporter, virtual keyboard, text input, input method. |
| Phone-shell logic vs compositor core | Heavily fused. `src/windows/{layout,window,surface}.rs` implement Catacomb's own window model: primary/secondary **tiling** layouts, an animated **overview** (`src/overview.rs`), gesture handle chrome and IPC-driven configuration (`catacomb msg`, `initrc`). Roughly two thirds of the code is this product-specific window management and animation; the remaining third is DRM/udev/GLES plumbing that is itself Smithay-specific glue. |

## Gap analysis against this project's requirements

| Requirement (AGENTS.md / docs/baseline.md) | Catacomb | Gap |
| --- | --- | --- |
| MIT/Apache-2.0-compatible dependencies; workspace is `MIT OR Apache-2.0` | GPL-3.0-only | **Blocking for reuse-as-library or fork inside this workspace.** Linking or vendoring Catacomb code would relicense the shell under GPL-3.0. Running it as a separate process would be allowed, but see the next rows. |
| Minimal portrait touch-first shell, focused fullscreen flows, no desktop window management | Tiling (primary/secondary split), overview grid, cycle gestures, drag-and-drop | Opposite product model. Removing tiling/overview means deleting most of `windows/`, `overview.rs`, `input.rs` gesture logic and the IPC surface: little would remain to reuse. |
| Software rendering by default; acceleration optional; correctness must not need GL | GLES/GBM only | **Blocking.** Under QEMU `virtio-gpu-pci` with TCG the guest has no virgl, so Catacomb would depend on Mesa's llvmpipe/kms_swrast through GBM: heavy, slow and outside the "correctness without GL" rule. |
| Host Rust clients via xdg-shell (mobile-basic-screen, future apps) | Yes | None. |
| Status/launcher surfaces (layer-shell) | Yes (`delegate_layer_shell`, exclusive zones via IPC) | None for the protocol; the chrome policy would still be ours. |
| Gestures | Yes, but bound to its own handle/overview model | Would need replacement to match GrapheneOS-derived conventions. |
| On-screen keyboard | Protocol support only | Same as any Smithay compositor; the product OSK is a project client either way. |
| Rotation | iio-sensor-proxy / udev accelerometer | Overkill for QEMU (no IIO device); would need a mock adapter through `hardware-api` anyway. |
| Brightness, notifications, crash recovery | Not compositor features in Catacomb (brightness via `initrc` scripts; no notification daemon; no client supervision) | Project work regardless. |
| Runnable nested on a Linux host for development | **No** nested/winit backend | Would have to be written and upstreamed or carried as a patch. |
| Runnable under QEMU virtio-gpu with TCG | Only through GLES/GBM (Mesa software) | See rendering row. |
| Rust-first, `hardware-api` boundary | Rust; talks to udev/D-Bus directly | Its D-Bus/udev sensor code would violate the adapter boundary if reused as-is. |

## Options

- **Reuse (run Catacomb as the guest compositor):** license permits process-level use, but the product model (tiling/overview), GL requirement and lack of a nested backend mean the QEMU route would not exercise the intended shell at all. Rejected.
- **Contribute upstream:** the missing pieces are a pixman/software render path, a winit backend and a fullscreen-only window model. These are not gaps in Catacomb's own goals (it is a tiling phone compositor for real hardware with GPUs), so upstream would rightly push back; and contributions would carry GPL-3.0. Worth revisiting only for protocol-level bugs found in shared Smithay code, which belong in Smithay (MIT) anyway.
- **Fork:** inherits GPL-3.0 (blocking for the `MIT OR Apache-2.0` workspace) and inherits a codebase where most lines implement the window management we do not want. Rejected.
- **Build on Smithay directly:** Smithay `0.7.0` is MIT, published on crates.io, and already provides every protocol Catacomb uses plus `PixmanRenderer`, `DumbAllocator`, `DrmCompositor` (software scan-out through DRM dumb buffers), libseat/udev/libinput integration and a winit backend. The phone-shell policy left to write is small and matches our product: one fullscreen toplevel, layer-shell chrome, one seat.

## Decision

**Build `mobile-shell` on Smithay 0.7 (MIT) directly.** Catacomb is retained as
a design reference for touch gesture handling (handle/sector model), the
protocol set a phone compositor needs, and its orientation-source adapters; no
code is copied from it (GPL-3.0). Departures from Catacomb are intentional:
fullscreen-only flows instead of tiling/overview, software rendering by
default, a nested winit route for host development, and rotation/brightness
sourced through `hardware-api` adapters instead of D-Bus in the compositor.

### Smithay caveats recorded

- Smithay's crates.io cadence is slow (0.7.0 in June 2025; Catacomb and other
  downstreams pin git revisions). `mobile-shell` pins `0.7.0` through
  `Cargo.lock`; moving to a git rev is a one-line change if a needed fix lands
  only on `master`.
- `DrmCompositor` is gated on Smithay's `backend_gbm` feature, so the guest
  binary links `libgbm` even though it is passed `None` and never allocates
  through GBM; rendering is pixman into DRM dumb buffers (`DumbAllocator`),
  exported as dmabufs for the pixman `Bind<Dmabuf>` path.
- The winit backend is EGL/GLES only. Nested host development therefore uses
  GLES (acceleration is available on a developer desktop); the QEMU/product
  path never requires GL. A wl_shm-presenting nested backend is possible later.
- Dynamically linked system libraries in the guest: libinput (MIT), libudev
  from eudev (LGPL-2.1), libseat (MIT), libdrm (MIT), pixman (MIT),
  libxkbcommon (MIT), libgbm from Mesa (MIT). All are Alpine packages; none is
  statically linked or vendored.

### Requirement coverage in this implementation (Phase 3 baseline)

| Requirement | State in `mobile-shell` |
| --- | --- |
| Portrait, fullscreen-only xdg toplevels | Implemented (`layout::WindowStack`, `XdgShellHandler`: newest mapped toplevel fills the output; only the Fullscreen wm capability is advertised). |
| Software rendering by default | Implemented on the `udev` backend (`PixmanRenderer` + `DumbAllocator` + `DrmCompositor`, overlay/cursor planes disabled). |
| wl_seat keyboard + pointer + touch from libinput | Implemented (virtio-keyboard, virtio-tablet as absolute pointer; touch path wired for real touchscreens). |
| Output pinned to `omarchy.display=WxH` / profile | Implemented (`config::ShellConfig::resolve`; connector mode match, else synthesised 60 Hz mode). |
| wlr-layer-shell reserved for status/launcher | Implemented (`WlrLayerShellHandler`, Smithay `LayerMap`; rendered above/below the toplevel by layer). |
| Nested winit backend from the mock profile | Implemented (`--nested targets/qemu-aarch64/profile.toml`, Linux host only; `mock` evidence). |
| Machine-readable readiness | Implemented (`compositor-ready`, `client-mapped`, `client-unmapped`, `compositor-frame`, `compositor-failed`, `input-device`, `drm-output`, `display-geometry`). |
| Rotation | Output transform plumbing and re-configure on plan change implemented; no gesture/sensor trigger yet. |
| Gestures, OSK, brightness, quick settings, notifications, crash recovery | Not yet; layer-shell and the seat model are the hooks. Crash recovery of clients belongs to the session layer (`mobile-session`). |
