# Repository instructions

These instructions apply to the entire repository. Read `docs/baseline.md` and `plan.md` before changing architecture, targets, guest-image construction, hardware APIs, or the mobile shell. If they conflict, `docs/baseline.md` is the implementation baseline and `plan.md` supplies the longer-term product direction.

## Product boundary

- This is an emulator-first, Rust-first mobile Linux environment, not a new distribution.
- The only supported target is generic AArch64 QEMU `virt`. Describe evidence as `mock` or `QEMU`; never present it as physical-device validation.
- Do not create a board target, inspect or operate a physical phone, add Android boot tooling, or claim Solana Seeker support without a separately approved board-support proposal.
- Use postmarketOS/pmbootstrap as the intended full guest substrate. A small Alpine/initramfs guest is acceptable only as the explicitly labelled serial/graphical smoke-test route, not as a replacement product distribution.

## Architecture

- Product behavior depends on `hardware-api`, never directly on QEMU, kernel, vendor, or board-specific details.
- Target selection and adapter wiring belong in session/bootstrap code. Keep target conditionals out of applications, `mobile-core`, and `hardware-api`.
- Keep Linux services such as NetworkManager, UPower, PipeWire, ModemManager, and sensor services behind typed project-owned adapters; do not reimplement their device protocols.
- New project-owned services, tools, applications, and UI are Rust unless a documented constraint makes that impractical.

## Mobile shell

- Evaluate Catacomb for reuse, contribution, or maintained fork before starting a compositor from scratch. If it cannot satisfy the requirements, document the gap before choosing Smithay directly.
- Develop the product shell nested on the host first, using the same versioned mock profile; integrate it into QEMU second.
- Weston, BusyBox, terminal layouts, and other third-party surfaces are diagnostic/reference tools only. Never call a terminal mock, Weston desktop, or plain test client the product mobile UI.
- The product shell must be touch-first and portrait-oriented. Phase 3 scope includes fullscreen applications, launcher, status surface, navigation gestures, on-screen keyboard integration, rotation, brightness, quick settings, notifications, and crash recovery.
- Begin with software rendering. Graphics acceleration is optional and must not become a correctness requirement.
- A graphical completion claim requires visual evidence from the actual rendered framebuffer or window plus a successful compositor/client readiness signal. A process-start log alone is insufficient.

## Emulator and evidence

- Keep `just qemu-check` headless, TCG-compatible, bounded by a timeout, and independent of human input or a display server.
- Do not emit readiness before the underlying condition is observable. For Wayland, require the compositor socket and a successfully connected client; on failure, emit a machine-readable failure record and preserve the relevant log.
- Keep the serial console mandatory even when graphical output is enabled.
- Use the versioned `targets/qemu-aarch64/profile.toml` for mock state. Deterministic tests must cover at least unplugged-to-charging and later the remaining profile transitions.
- Generated artifacts belong under the resolved `out/qemu-aarch64/` path and remain ignored. Manifests record pinned inputs and hashes.

## Required checks

Before handing off relevant changes, run the applicable subset of:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
just qemu-check
```

For graphical changes, additionally launch the documented graphical command, capture the QEMU framebuffer or inspect the visible window, and verify that it matches the intended mobile surface. Record limitations honestly in `docs/status.md`.

## Working practices

- Reuse popular, maintained libraries first; hand-roll only where popularity is low or existing packages are not fit for purpose, and document that reasoning. Example: Wayland client plumbing comes from `smithay-client-toolkit` and CPU 2D rendering from `tiny-skia`, while the strict profile parser stays hand-rolled to keep the schema contract explicit.
- Check new dependencies for license compatibility (MIT/Apache-2.0 preferred) and prefer crates from ecosystems the plan already commits to (for example Smithay).
- Preserve user changes and keep edits scoped.
- Do not install host tools automatically; `just host-check` reports prerequisites and installation commands.
- Keep commands reproducible through checked-in scripts or `just` recipes rather than machine-specific steps.
- Prefer small, observable, reversible progress: build, emulate, validate, record evidence, then extend.
