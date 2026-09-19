# Status

## 2026-09-20

- **QEMU (basic screen):** The ASCII `weston-terminal` reference layout is replaced by `mobile-basic-screen`, a Rust Wayland client (smithay-client-toolkit + tiny-skia + fontdue, software rendering only) that draws an Omarchy-styled (Tokyo Night, JetBrains Mono) basic screen with a GrapheneOS-style status bar, clock, battery/network cards from the mock profile, a six-tile launcher grid, and a gesture pill. Verified by the actual rendered framebuffer at `out/qemu-aarch64/agent/frame.png` after the guest emitted `basic-screen-frame` and `wayland-ready` records on serial; `just qemu-check` still passes headlessly under TCG.
- **Build:** `just qemu-build` now also compiles the guest client for aarch64 musl in a pinned `rust` container (digest and binary hash recorded in the manifest). A host-side `--preview` mode renders the same composition to PPM without QEMU.
- **Limitations:** The screen is static (rendered once at first configure; the clock does not tick), tiles do not react to input yet, and it remains a reference surface — Catacomb evaluation and the Phase 3 touch-first product shell are still outstanding. Evidence is QEMU/mock only; no physical-device claims.

## 2026-09-16

- **mock:** The strict, schema-versioned QEMU profile parser and deterministic unplugged-to-charging transition are covered by a Rust unit test.
- **QEMU:** `just qemu-check` booted the generic AArch64 `virt` guest under TCG and observed serial boot-health plus the deterministic unplugged-to-charging mock event. The guest powers down cleanly after the smoke check.
- **QEMU (agent workflow):** `just qemu-agent-start`, `just qemu-frame`, `just qemu-tap`, and `just qemu-agent-stop` provide local QMP control, mandatory serial capture, and actual rendered-framebuffer evidence at `out/qemu-aarch64/agent/frame.png`. Readiness requires both the Wayland socket and a live, connected reference client. `qemu-frame` emits a machine-readable failure for an entirely blank framebuffer. The virtio display takes its 1080x2340 portrait geometry from the mock profile.

The rendered Weston surface is diagnostic evidence for the QEMU graphics path only. It is not the Rust product shell; Catacomb evaluation and the Phase 3 touch-first shell remain outstanding.

No physical-device testing or claims have been made.
