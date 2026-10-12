# Repository instructions

These rules apply repository-wide. Load only task-relevant detail: `docs/status.md` for implemented state/limitations; `targets/qemu-aarch64/README.md` for QEMU operation; and both `plans/baseline.md` (implementation authority) and `plans/plan.md` (long-term direction) before changing architecture, targets, guest construction, hardware APIs, or the product shell. Baseline wins conflicts. `plans/README.md` indexes the other planning documents and their status. For any attached phone (ADB/Fastboot identification, recovery images, unlock, flashing Ferrix, radio bring-up), follow `skills/mobile-device-provisioning/SKILL.md`; it is the LLM-agnostic playbook and encodes the gates below. For the day-to-day build, deploy, screen-capture (framebuffer and host webcam), input-injection and evidence loop on QEMU and the Pixel 9a, use `skills/ferrix-dev-loop/SKILL.md`.

## Boundaries

- This is an emulator-first, Rust-first mobile Linux environment, not a distribution. Generic AArch64 QEMU `virt` remains the development and CI portability target. Physical devices may be added as separately identified board targets after explicit owner approval of the target work; never generalize evidence from QEMU to hardware or from one hardware revision to another.
- Physical-target work must begin with a checked-in board-support proposal covering the exact manufacturer model number/codename and hardware revision, bootloader state and supported boot route, kernel/device-tree/driver sources, partition map, backup and recovery procedure, firmware handling, capability matrix, evidence plan, and data-collection policy. The repository owner explicitly approves OnePlus target discovery and proposal work as of 2026-09-20; destructive provisioning still requires the proposal's device-identity, image-compatibility, backup, and recovery gates to pass.
- `adb`, `fastboot`, and target-specific image tooling may be used only by checked-in, target-scoped workflows. Discovery commands must be read-only. Any command that unlocks a bootloader, erases or formats storage, changes slots, flashes partitions, or boots an untrusted image must resolve and print the exact device serial, product/codename, hardware revision, current slot, partition target, input image hash, and recovery artifact first; it must require an explicit per-run confirmation and refuse ambiguous or multiple devices.
- Recovery-invariant exception (owner policy 2026-10-09, exact target `google-pixel-9a-tegu-gxq96-mp1-0` only): the sole hard requirement is that the device stays restorable to the pinned stock factory image. Agents may run `just pixel-9a-dev-flash` (flash/boot/set-active), `just pixel-9a-enter-fastboot`, and `just pixel-9a-factory-restore` autonomously, supplying confirm tokens, typed phrases, and the dev-device `FERRIX_PIXEL9A_NO_USER_DATA` token themselves without a per-run human go-ahead. Ferrix image-compatibility, candidate pinning, and boot-observation evidence are advisory for this route, not blockers. Never weaken the recovery invariant itself: exact single device, unlocked bootloader-mode Fastboot, tested restore with a hash-verified pinned factory archive, writes only to partitions the factory image rewrites, no bootloader/radio writes except pinned stock images, no relock/erase/format/oem, and stock firmware on the target slot before any slot switch. A device that does not return to Fastboot needs the human to hold Volume Down + Power; stop and ask.
- For development transport setup, a checked-in target workflow may perform a non-destructive Android-to-bootloader Fastboot reboot after exact identity checks and a typed confirmation; hard-button entry is not required when ADB is available. This exception must never enter Fastbootd or change storage, slots, bootloader state, or user data.
- Keep physical-device output, identifiers, partition dumps, firmware, keys, backups, and generated images out of Git. Evidence committed to the repository must redact unique identifiers and distinguish `QEMU`, `mock`, and the exact physical target.
- postmarketOS/pmbootstrap is the intended full guest substrate. Alpine/initramfs is only the explicitly labelled smoke-test route.
- Product code depends on `ferrix-hardware-api`, not QEMU, kernel, vendor, or board details. Put target selection/adapters in session/bootstrap code; keep target conditionals out of apps, `ferrix-core`, and `ferrix-hardware-api`.
- Put Linux services (NetworkManager, UPower, PipeWire, ModemManager, sensors) behind typed project-owned adapters; do not reimplement their protocols. New project-owned code is Rust unless a documented constraint prevents it.

## Product shell

- Evaluate Catacomb reuse/contribution/fork before a new compositor; document gaps before choosing Smithay directly. Develop nested on the host with the versioned mock profile, then integrate into QEMU.
- Weston and `ferrix-basic-screen` are diagnostic/reference surfaces, not the product UI. Applications are independent processes launched only by `ferrix-appd` (see `docs/app-platform.md`); apps, shells, and the core depend on `ferrix-app-api`/`ferrix-app-protocol`, never on `ferrix-appd` or a runtime implementation. The product shell is minimal, portrait, touch-first, software-rendered by default, and uses focused fullscreen flows rather than desktop window management. Acceleration must not be required for correctness.
- Resolve UX choices in this order: explicit requirements, Omarchy Ferrix's restrained, cohesive, low-noise mobile visual system, then GrapheneOS interaction conventions adapted to that system. Do not copy external branding; document intentional departures.
- Phase 3 includes launcher/status surfaces, gestures, on-screen keyboard, rotation, brightness, quick settings, notifications, and crash recovery.

## QEMU and evidence

- Keep `just qemu-check` headless, TCG-compatible, timed, display-independent, and noninteractive. Serial remains mandatory with graphics.
- Readiness must be observable. Wayland requires a compositor socket plus a connected client; preserve logs and emit a machine-readable failure otherwise. A graphical claim also requires inspection/capture of the actual framebuffer or window, not a process-start log.
- Mock state comes from `targets/qemu-aarch64/profile.toml`; deterministic tests retain at least unplugged-to-charging coverage. Generated files stay ignored under resolved `out/qemu-aarch64/`; manifests retain pinned inputs and hashes.

## Work and validation

- For a fresh developer machine, follow `AGENT_INSTALL.md`; installation is
  explicit and opt-in, while `just host-check` remains read-only.
- For a fresh clone, `sh tools/dev/local-init.sh` creates the ignored
  developer and device notes under `local/` without changing build inputs.
  Pixel 9a contributors start with `docs/pixel-9a-developer-setup.md`.
- Preserve user changes; keep work scoped, reproducible through checked-in scripts/`just` recipes, and do not install host tools automatically (`just host-check` reports prerequisites).
- Prefer popular, maintained libraries and MIT/Apache-2.0-compatible dependencies from existing ecosystems. Hand-roll only with a documented fit reason.
- Run the applicable subset before handoff: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace`, and `just qemu-check`.
- For graphical changes, also run the documented graphical workflow, inspect/capture its output, and record evidence and limitations honestly in `docs/status.md` (`docs/status.md` current state; dated entries in `docs/status-log/`).
