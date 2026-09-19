# Status

## 2026-09-16

- **mock:** The strict, schema-versioned QEMU profile parser and deterministic unplugged-to-charging transition are covered by a Rust unit test.
- **QEMU:** `just qemu-check` booted the generic AArch64 `virt` guest under TCG and observed serial boot-health plus the deterministic unplugged-to-charging mock event. The guest powers down cleanly after the smoke check.
- **QEMU (agent workflow):** `just qemu-agent-start`, `just qemu-frame`, `just qemu-tap`, and `just qemu-agent-stop` provide local QMP control, mandatory serial capture, and actual rendered-framebuffer evidence at `out/qemu-aarch64/agent/frame.png`. Readiness requires both the Wayland socket and a live, connected reference client. `qemu-frame` emits a machine-readable failure for an entirely blank framebuffer. The virtio display takes its 1080x2340 portrait geometry from the mock profile.

The rendered Weston surface is diagnostic evidence for the QEMU graphics path only. It is not the Rust product shell; Catacomb evaluation and the Phase 3 touch-first shell remain outstanding.

No physical-device testing or claims have been made.
