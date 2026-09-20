# Ferrum: Emulator-First Rust Mobile Linux

This document is the execution plan and source of truth for an emulator-first, Rust-first mobile Linux environment. The generic AArch64 QEMU virtual phone is the only initial target. No physical device is implied, required, or supported by this plan.

## Mission

Build a mobile operating environment with:

- A reproducible AArch64 emulator target for daily development and CI.
- A Rust-first userspace for project-owned services and applications.
- A Rust/Smithay Wayland compositor designed for touch.
- Ferrum's own restrained, cohesive, low-noise visual system and development experience.
- A stable hardware abstraction that lets future device targets be added without changing product logic.

The emulator provides phone-shaped display, touch, rotation, battery, charging, connectivity, audio-route, telephony, and suspend mocks. It validates project-owned software and its integration lifecycle only; it does not establish compatibility with any physical device, firmware, radio, secure hardware, or boot chain.

Ferrum is the product and ship name. Ferrix names its kernel/userspace runtime
surface, including configuration and boot interfaces. Gossan is reserved for a
future device image and does not rename the generic AArch64 QEMU target.

## Scope and safety contract

Initial work is limited to local development, containers, CI, and QEMU. Do not connect to, inspect, unlock, boot, flash, or modify physical hardware under this plan. A future physical-target proposal must define its own hardware inventory, recovery procedure, evidence requirements, and explicit approval gates before any device action.

## Architecture

```
                  product applications
       launcher / settings / dialer / messages
                         |
                 Rust mobile core
                         |
          project-owned, versioned APIs
                         |
                   hardware-api
                    /           \
     emulator adapters       target adapters
          |                       |
 QEMU profile + controls   board-support package
          |                       |
 generic AArch64 QEMU     future physical device
```

Product code depends only on `hardware-api`; it must not depend on QEMU details, a kernel driver, a vendor interface, or a board package. The emulator implements the same traits and emits the same API events expected of later targets. It is the contract-test target for portable behavior.

## Repository layout

Create directories only as implementation requires them.

```text
.
├── README.md
├── plan.md
├── Cargo.toml
├── rust-toolchain.toml
├── justfile
├── crates/
│   ├── mobile-core/          # domain state and orchestration
│   ├── hardware-api/         # stable traits and shared types
│   ├── hardware-emulator/    # QEMU and host-test implementations
│   ├── hardware-linux/       # reusable standard-Linux adapters
│   ├── mobile-session/       # startup and lifecycle
│   ├── mobile-shell/         # Smithay compositor and shell
│   └── image-tool/           # emulator-image metadata tooling
├── apps/
├── targets/
│   ├── qemu-aarch64/         # primary target: profile, kernel config, initramfs
│   └── boards/               # future target packages; absent until supported
├── containers/guest-builder/
├── docs/
│   ├── baseline.md
│   ├── status.md
│   └── targets.md
└── .github/workflows/
```

## Target model and future extension boundary

Every supported execution target has a versioned target manifest. The initial manifest is `targets/qemu-aarch64/profile.toml`. A later physical target is added as an isolated package under `targets/boards/<target-id>/`, never by embedding target conditionals in `mobile-core`, apps, or `hardware-api`.

A board-support package may contain only target-specific configuration and adapter wiring: kernel/config fragments, rootfs overlays, packaging metadata, firmware-discovery rules, and implementations that translate target interfaces into `hardware-api`. Reusable Linux functionality belongs in `hardware-linux` only after it has a demonstrated second consumer.

Before a new target is accepted, it must provide:

1. A stable `<target-id>` and versioned manifest.
2. An explicit capability matrix mapping every `hardware-api` capability to `supported`, `unsupported`, or `mocked`.
3. Evidence for every claimed capability and a documented recovery path for any hardware action.
4. Contract-test results proving that product behavior is unchanged when using the emulator adapter.
5. No target-specific imports or feature flags in product applications.

Target selection belongs in the session/bootstrap layer. Runtime capability discovery is preferred over compile-time target checks. Unsupported capabilities return typed, user-visible errors; they must not silently imitate successful hardware operations.

## Rust-first policy

Write new project-owned services, tools, and first-party applications in Rust unless a documented constraint makes that impractical. Existing Linux infrastructure is acceptable behind `hardware-api` and project-owned versioned interfaces. UI code must not call adapter implementations directly.

Start with synchronous traits for simple state reads and commands; use asynchronous methods only where the operation or event stream requires it. Shared capability types should be small, explicit, serializable where needed, and backward-compatible within a schema version.

## Emulator target

Use QEMU's generic AArch64 `virt` machine with a generic arm64 Linux kernel, minimal initramfs, serial console, and a versioned mock profile. The profile controls display geometry, touch, rotation, power, connectivity, telephony, audio route, and suspend behavior. Its values are test configuration, not hardware facts.

The profile must support deterministic state transitions through a test-only control endpoint, fixture, or explicit mock-control command. CI must be able to change charging, battery percentage, network state, modem registration, SIM state, rotation, and suspend outcome and observe the corresponding project API events.

Required workflows:

- `just host-check`: verify documented host prerequisites without installing them.
- `just qemu-build`: build the AArch64 kernel/rootfs artifacts and manifest.
- `just qemu-run`: start an interactive virtual phone with serial output.
- `just qemu-check`: run a bounded, headless boot-health and mock-state smoke test.
- `just shell-nested`: run the compositor on the development desktop with the same profile.
- `just test`: run Rust tests without requiring QEMU unless an integration test opts in.

`qemu-check` is the CI contract. It runs under TCG, requires no graphical display or physical hardware, asserts a structured boot-health record, injects at least one state transition, and shuts down cleanly. Evidence must be labeled `mock` or `QEMU`.

## Delivery phases

### Phase 0 — workspace and contract

Create the Rust workspace, lint/test configuration, `hardware-api`, `hardware-emulator`, a strict schema-versioned profile parser, and a battery/charger state example. Add `.gitignore`, `.env.example`, `just host-check`, and the QEMU target skeleton.

### Phase 1 — serial-first guest

Build a pinned guest-builder container and minimal arm64 kernel/initramfs. Start the mobile core and emulator adapter in the guest, emit structured boot health, and implement `qemu-build`, `qemu-run`, and bounded `qemu-check`.

### Phase 2 — portable product behavior

Define versioned power, display, input, network, audio, telephony, and lifecycle capabilities in `hardware-api`. Add deterministic adapter integration tests and run them in CI. Keep a nested shell route for fast UI iteration.

### Phase 3 — mobile shell

Build the touch-first shell: full-screen applications, launcher, status surface, navigation gestures, on-screen keyboard integration, rotation, brightness, quick settings, notifications, and crash recovery. Begin with software rendering; graphics acceleration is optional.

### Phase 4 — target readiness

Document the target-package contract, capability matrix format, evidence conventions, and a target-admission checklist. No board package is created until there is a concrete target and explicit authorization for its hardware work.

## Acceptance criteria

- `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features`, and `cargo test --workspace` pass.
- A host test observes a deterministic emulator hardware-state transition.
- `just qemu-check` boots the virtual target to a serial health report and exits deterministically.
- CI runs the QEMU check headlessly under TCG.
- Product crates compile and test against `hardware-api` without importing the emulator implementation.
- `docs/status.md` labels all virtual-target evidence as `mock` or `QEMU`.
- No physical-device support is claimed or attempted.

## Project principle

Optimize for small, observable, reversible successes:

```
build -> emulate -> validate -> record evidence -> extend through a target package
```

Do not treat emulator success as a physical-device claim. A later target earns support through its own evidence while the emulator remains the portability baseline.
