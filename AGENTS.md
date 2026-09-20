# Repository instructions

These rules apply repository-wide. Load only task-relevant detail: `docs/status.md` for implemented state/limitations; `targets/qemu-aarch64/README.md` for QEMU operation; and both `docs/baseline.md` (implementation authority) and `docs/plan.md` (long-term direction) before changing architecture, targets, guest construction, hardware APIs, or the product shell. Baseline wins conflicts.

## Boundaries

- This is an emulator-first, Rust-first mobile Linux environment, not a distribution. Generic AArch64 QEMU `virt` is the only supported target; label evidence `mock` or `QEMU`, never physical-device validation.
- Do not inspect or operate phones, add board targets or Android boot tooling, or claim Solana Seeker support without a separately approved board-support proposal.
- postmarketOS/pmbootstrap is the intended full guest substrate. Alpine/initramfs is only the explicitly labelled smoke-test route.
- Product code depends on `hardware-api`, not QEMU, kernel, vendor, or board details. Put target selection/adapters in session/bootstrap code; keep target conditionals out of apps, `mobile-core`, and `hardware-api`.
- Put Linux services (NetworkManager, UPower, PipeWire, ModemManager, sensors) behind typed project-owned adapters; do not reimplement their protocols. New project-owned code is Rust unless a documented constraint prevents it.

## Product shell

- Evaluate Catacomb reuse/contribution/fork before a new compositor; document gaps before choosing Smithay directly. Develop nested on the host with the versioned mock profile, then integrate into QEMU.
- Weston and `mobile-basic-screen` are diagnostic/reference surfaces, not the product UI. The product shell is minimal, portrait, touch-first, software-rendered by default, and uses focused fullscreen flows rather than desktop window management. Acceleration must not be required for correctness.
- Resolve UX choices in this order: explicit requirements, Ferrum's restrained, cohesive, low-noise mobile visual system, then GrapheneOS interaction conventions adapted to that system. Do not copy external branding; document intentional departures.
- Phase 3 includes launcher/status surfaces, gestures, on-screen keyboard, rotation, brightness, quick settings, notifications, and crash recovery.

## QEMU and evidence

- Keep `just qemu-check` headless, TCG-compatible, timed, display-independent, and noninteractive. Serial remains mandatory with graphics.
- Readiness must be observable. Wayland requires a compositor socket plus a connected client; preserve logs and emit a machine-readable failure otherwise. A graphical claim also requires inspection/capture of the actual framebuffer or window, not a process-start log.
- Mock state comes from `targets/qemu-aarch64/profile.toml`; deterministic tests retain at least unplugged-to-charging coverage. Generated files stay ignored under resolved `out/qemu-aarch64/`; manifests retain pinned inputs and hashes.

## Work and validation

- Preserve user changes; keep work scoped, reproducible through checked-in scripts/`just` recipes, and do not install host tools automatically (`just host-check` reports prerequisites).
- Prefer popular, maintained libraries and MIT/Apache-2.0-compatible dependencies from existing ecosystems. Hand-roll only with a documented fit reason.
- Run the applicable subset before handoff: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace`, and `just qemu-check`.
- For graphical changes, also run the documented graphical workflow, inspect/capture its output, and record evidence and limitations honestly in `docs/status.md`.
