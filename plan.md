# Rust-First Mobile Linux for OnePlus Nord N10 5G

This document is the execution plan and agent handoff for bringing up a Rust-first mobile Linux environment on a OnePlus Nord N10 5G (`BE2029`, device codename `billie`). Place it in the repository root and treat it as the source of truth until the project has working project-specific documentation.

## Mission

Build a mobile operating environment with:

- Mainline Linux as the long-term kernel target.
- A Rust-first userspace for project-owned services and applications.
- A Rust/Smithay Wayland compositor designed for touch.
- An Omarchy-inspired visual system and development experience.
- A portable hardware abstraction that can later target Solana Seeker.
- A reproducible virtual AArch64 phone target for daily development and CI.

The first physical hardware target is the OnePlus Nord N10 5G. The first development target is a generic AArch64 QEMU virtual phone with a Seeker-like product profile. The first physical-device milestone is not a complete phone: it is a recoverable, observable boot into a project-controlled initramfs without damaging the stock installation.

The virtual target is deliberately **not** called a Solana Seeker emulator. It exercises portable product code with phone-shaped display, touch, rotation, battery, charging, connectivity, and telephony mocks; it cannot establish compatibility with Seeker firmware, hardware, secure-element/Seed Vault interfaces, radios, or boot chain.

## Read This First: Safety Contract

This project touches boot-critical phone partitions. A mistake can erase user data or make the device temporarily or permanently unbootable.

### The agent may do without additional approval

- Inspect the repository and development host.
- Install or propose development dependencies that do not modify the phone.
- Build host tools, Rust crates, root filesystems, kernels, DTBs, and boot images.
- Run unit tests, linters, formatters, QEMU, and containerized builds.
- Run read-only `adb` and `fastboot getvar` commands.
- Record non-secret device properties.
- Create scripts that print a destructive command without executing it.
- Update documentation and status matrices.

### The agent must stop and request explicit human approval before

- Running `fastboot flashing unlock`, `fastboot oem unlock`, or an unlock-token operation.
- Flashing, erasing, formatting, repartitioning, or overwriting any phone partition.
- Running `fastboot boot` with an image that has not passed the image-validation checklist.
- Changing the active A/B slot.
- Disabling Android Verified Boot on the device.
- Rooting Android or modifying persistent, calibration, radio, modem, EFS, `persist`, or security partitions.
- Relocking the bootloader.
- Running an unbrick or emergency-download tool.
- Publishing proprietary firmware extracted from the phone.

Never infer approval from the existence of this plan. Before a device-changing action, show the exact command, target partition or temporary boot target, expected effect, recovery path, and data-loss risk.

## Hardware Baseline

Expected device identity:


| Property                 | Expected value                                     |
| ------------------------ | -------------------------------------------------- |
| Marketing name           | OnePlus Nord N10 5G                                |
| Model                    | BE2029                                             |
| Codename                 | `billie`                                           |
| SoC                      | Qualcomm SM6350 / Snapdragon 690 5G                |
| CPU architecture         | AArch64                                            |
| GPU                      | Adreno 619L                                        |
| RAM                      | 6 GB                                               |
| Storage                  | 128 GB UFS 2.1                                     |
| Shipping OS              | OxygenOS based on Android 10                       |
| Downstream kernel family | Linux 4.19                                         |
| Partition scheme         | Treat as A/B until verified on the physical device |


Do not trust this table over measurements from the physical phone. Record all differences in `docs/hardware-inventory.md`.

## Strategic Kernel Decision

Mainline Linux is the destination, but the known downstream OnePlus kernel is permitted as temporary bring-up scaffolding.

Use two kernel tracks:

1. **Bring-up track:** Build the known OnePlus/LineageOS downstream kernel to validate the boot-image toolchain, device tree, display, touch, storage, USB, and firmware assumptions.
2. **Mainline track:** Create and maintain the upstream-oriented SM6350/`billie` device tree and minimal patch series.

Do not build project-owned userspace against private downstream interfaces when a stable Linux interface is available. The Rust userspace should see DRM/KMS, evdev/libinput, ALSA/PipeWire, V4L2, IIO, power-supply/UPower, NetworkManager, BlueZ, ModemManager, or project-owned adapter APIs.

## Architecture

```
QEMU AArch64 virtual phone                 OnePlus bootloader / future Seeker boot chain
  -> generic kernel + rootfs                  -> Linux kernel + DTB + initramfs
       -> mock / generic adapters                  -> hardware-specific adapters
                    \                              /
                     -> project init / system supervisor
                         -> project-owned D-Bus service boundary
                             -> Rust mobile core
                                 -> Rust/Smithay compositor
                                     -> launcher, settings, dialer, messages, browser
```

### Rust-first policy

Write all new project-owned system services, tools, and first-party applications in Rust unless a documented hardware constraint makes that impractical.

Existing non-Rust infrastructure is acceptable when replacing it would not advance phone bring-up:

- Linux kernel and existing kernel drivers
- Mesa or vendor graphics components
- PipeWire and WirePlumber
- NetworkManager
- BlueZ
- ModemManager
- D-Bus implementation
- Firefox or Chromium
- Temporary Android/vendor compatibility services

Wrap external infrastructure behind Rust traits and project-owned D-Bus interfaces. UI code must not talk directly to a vendor-specific modem or Android HAL.

## Proposed Repository Layout

Create directories only as they become necessary. Avoid empty scaffolding that implies unimplemented functionality.

```
.
├── ONEPLUS_MOBILE_LINUX_PLAN.md
├── README.md
├── Cargo.toml
├── rust-toolchain.toml
├── justfile
├── crates/
│   ├── mobile-core/          # Domain state and orchestration
│   ├── hardware-api/         # Stable traits and shared types
│   ├── hardware-linux/       # Standard Linux implementations
│   ├── hardware-mock/        # Desktop/QEMU test implementations
│   ├── mobile-session/       # Session startup and lifecycle
│   ├── mobile-shell/         # Smithay compositor and shell
│   └── image-tool/           # Image metadata and validation tooling
├── apps/
│   ├── launcher/
│   ├── settings/
│   ├── dialer/
│   └── messages/
├── device/
│   └── oneplus-billie/
│       ├── README.md
│       ├── config/
│       ├── initramfs/
│       ├── kernel/
│       └── manifests/
├── targets/
│   └── qemu-aarch64/         # Generic virtual phone and Seeker-like mock profile
│       ├── README.md
│       ├── profile.toml
│       └── scripts/
├── image/
│   ├── initramfs/
│   ├── rootfs/
│   └── scripts/
├── tools/
│   ├── host-check/
│   └── device-audit/
├── docs/
│   ├── architecture.md
│   ├── hardware-inventory.md
│   ├── partition-map.md
│   ├── recovery.md
│   ├── status.md
│   └── decisions/
└── tests/
```

## Required Documentation

Keep these files current as facts become known:

- `README.md`: project purpose, current status, safe quick start, and links.
- `docs/hardware-inventory.md`: measured Android, kernel, SoC, peripherals, firmware, and identifiers.
- `docs/partition-map.md`: names, sizes, slots, boot-header versions, hashes, and backup status. Never include secrets or raw unique identifiers.
- `docs/recovery.md`: proven path back to the stock OS, required files, and verification steps.
- `docs/status.md`: hardware support matrix for downstream and mainline kernels.
- `targets/qemu-aarch64/README.md`: QEMU prerequisites, launch command, profile contract, and known fidelity limits.
- `docs/decisions/`: short architecture decision records for consequential choices.
- `device/oneplus-billie/README.md`: device-specific build, package, and boot-image notes.

## Phase 0: Host and Repository Bootstrap

### Goal

Create a reproducible development environment and a testable Rust workspace without touching the phone.

### Tasks

1. Inspect the host OS, CPU architecture, available disk space, and installed tools.
2. Check for `adb`, `fastboot`, Rust, Cargo, Clang/LLVM, `make`, device-tree compiler, QEMU AArch64, `cpio`, LZ4, and Android boot-image tools.
3. Choose a reproducible dependency strategy. Prefer a checked-in package manifest, dev container, Nix flake, or documented distro packages over undocumented host state.
4. Initialize the Rust workspace with strict linting and formatting.
5. Add a `justfile` or equivalent task runner with discoverable recipes.
6. Add CI for formatting, Clippy, unit tests, and documentation.
7. Create `hardware-api` and `hardware-mock` with one small end-to-end example, such as reading a mocked battery state.
8. Define a versioned `qemu-aarch64` virtual-phone profile. It must specify display dimensions and scale, touch input, rotation, battery/charging behavior, network state, modem state, and a deterministic clock or event source where tests need one.
9. Add reproducible QEMU build and launch recipes. Start with the generic `virt` machine and a serial-console-first root filesystem; use a graphical or generic DRM backend only after the serial path is reliable.
10. Add a CI job that runs the portable service and mock profile without any physical device.

### Minimum Rust policy

- Use stable Rust unless a specific dependency requires nightly.
- Deny unsafe code by default at crate roots. Document every exception.
- Deny warnings in CI, but avoid making local exploratory builds unnecessarily brittle.
- Use typed errors for library boundaries and add context at executable boundaries.
- Use structured tracing rather than ad hoc printing in long-running services.
- Keep hardware adapter traits asynchronous only where the operation is inherently asynchronous.
- Do not introduce a large framework before a concrete requirement exists.

### Acceptance criteria

- `cargo fmt --check` passes.
- `cargo clippy --workspace --all-targets --all-features` passes.
- `cargo test --workspace` passes.
- A mock hardware service starts on the development host and exposes one observable state transition.
- `just qemu-check` (or an equivalently documented command) boots the virtual target to a serial health report and exits or can be stopped deterministically.
- The virtual-phone profile has a test covering at least one state change, such as unplugged-to-charging battery state.
- `just --list` describes the supported workflows.
- No phone partitions have been modified.

## Virtual Development Target: QEMU AArch64 Virtual Phone

### Goal

Make portable mobile software testable before, alongside, and independently of physical-phone bring-up.

### Contract

- Use QEMU's generic AArch64 `virt` machine; do not encode OnePlus or Seeker device-tree, partition, firmware, or boot-image assumptions in this target.
- Provide a phone-oriented framebuffer or generic DRM display, absolute-pointer touch input, and a configurable portrait resolution. Keep display values product configuration, not hardware facts.
- Implement battery, charger, Wi-Fi/network, Bluetooth presence, audio-route, modem, SIM, and suspend behavior through `hardware-mock` or clearly named generic adapters.
- Make mock transitions controllable through a test API, fixture, or command-line input, and keep them deterministic in CI.
- Run the shell nested on the host when that gives faster feedback. QEMU is the integration target for the AArch64 root filesystem and service lifecycle, not a replacement for nested UI tests.

### Non-goals and fidelity limits

This target does not validate Qualcomm drivers, GPU acceleration, panel timing, touch-controller behavior, UFS, cellular radio operation, camera, fingerprint, Android/vendor compatibility, secure element, Seed Vault, verified boot, or any Seeker-specific interface. Label all virtual-target evidence as `mock` or `QEMU`; it must not be used to mark physical hardware support as working.

### Required workflows

- `just qemu-build`: build the AArch64 kernel/rootfs artifacts needed by the virtual target.
- `just qemu-run`: launch an interactive virtual phone with serial logs and the configured display/input backend.
- `just qemu-check`: run a bounded boot-health and mock-state smoke test suitable for CI.
- `just shell-nested`: run the compositor nested on the development desktop for rapid UI iteration.

## Phase 1: Non-Destructive Device Audit

### Goal

Confirm the exact device variant, bootloader state, slots, Android build, and kernel without changing device state.

### Preconditions

- User data is backed up independently.
- USB debugging is enabled by the device owner.
- The developer confirms that this is the disposable development phone.
- No command in this phase performs unlocking or flashing.

### Read-only collection

Create `tools/device-audit/` as a script or Rust CLI that runs individual commands, records timestamps, redacts unique identifiers, and writes a Markdown and JSON report.

Candidate commands:

```
adb devices
adb shell getprop ro.product.model
adb shell getprop ro.product.device
adb shell getprop ro.product.name
adb shell getprop ro.boot.hardware.sku
adb shell getprop ro.build.fingerprint
adb shell getprop ro.build.version.release
adb shell uname -a
adb shell cat /proc/cmdline
adb shell cat /proc/partitions
adb reboot bootloader
fastboot devices
fastboot getvar product
fastboot getvar unlocked
fastboot getvar current-slot
fastboot getvar slot-count
fastboot getvar secure
```

Some commands can fail because of Android permissions or bootloader implementation differences. Record failures; do not work around them with rooting.

Do not save or commit serial numbers, IMEIs, MEIDs, MAC addresses, unlock tokens, SIM identifiers, attestation data, or account information. If `fastboot getvar all` is used interactively, redact its output before storing or sharing it.

### Manual observations to request

- Whether Developer Options contains **OEM unlocking**.
- Whether the toggle is enabled, selectable, or greyed out.
- Whether the phone is carrier-branded.
- Whether Fastboot reports an unlock-token requirement.
- Whether the user has a verified stock-firmware recovery package.

### Acceptance criteria

- Device identity is confirmed as `billie` or discrepancies are documented.
- Slot and bootloader state are known.
- Carrier/unlock restrictions are documented.
- The hardware inventory contains no unique identifiers.
- No phone partitions have been modified.

## Gate A: Bootloader and Recovery Decision

Stop after Phase 1 and present a short decision report.

The report must answer:

1. Is bootloader unlocking available for this exact unit?
2. Does unlocking require a carrier or vendor token?
3. What data will unlocking erase?
4. Is a matching stock firmware or recovery path available?
5. Can the phone enter both Fastboot and recovery reliably?
6. Which partitions are A/B slotted?
7. Is temporary `fastboot boot` supported or merely assumed?

Do not unlock the device until the owner explicitly approves after reading this report.

## Phase 2: Reproduce the Known Kernel and Boot Artifacts

### Goal

Build known-good `billie` kernel and boot artifacts on the host without flashing them.

### Sources to study

- LineageOS `android_device_oneplus_billie`
- LineageOS `android_kernel_oneplus_sm6350`
- The current LineageOS `billie` build and installation documentation
- Android boot-image and vendor-boot documentation
- Current mainline Linux SM6350 support
- The postmarketOS `oneplus-billie` device package, if available

Pin source revisions in a manifest. Do not silently track moving branches in reproducible builds.

### Tasks

1. Determine the boot image header version from measured or known-good images.
2. Document the roles of `boot`, `vendor_boot`, `dtbo`, `vbmeta`, and any `vendor_dlkm` partition present on the device.
3. Build the known downstream kernel using its expected toolchain.
4. Locate the effective kernel configuration and preserve it under the device directory.
5. Identify DTB/DTBO packaging and kernel module locations.
6. Extract metadata from a known-good boot image without redistributing proprietary contents.
7. Implement an image-inspection command that reports header version, page size, command line, component sizes, hashes, and partition-size fit.
8. Produce a build report that distinguishes reconstructed open-source components from proprietary inputs.

### Image-validation checklist

Before proposing any temporary boot or flash:

- Confirm the image targets `billie`/SM6350.
- Confirm boot header version.
- Confirm page size and load metadata.
- Confirm DTB and ramdisk placement.
- Confirm compression formats.
- Confirm final image size is below the measured target partition size.
- Confirm the image does not contain host secrets or SSH private keys.
- Confirm an early console, USB gadget, or another debug channel is configured.
- Confirm the initramfs has a deterministic emergency reboot path.
- Record SHA-256 hashes.
- Document the exact recovery action if it fails.

### Acceptance criteria

- The known kernel builds reproducibly.
- Boot-image inspection works on known-good artifacts.
- A candidate diagnostic boot image can be produced entirely on the host.
- No candidate image has been executed on the phone.

## Phase 3: Project-Controlled Diagnostic Initramfs

### Goal

Build the smallest environment capable of proving that the project controls early userspace.

### Required behavior

The initramfs must:

- Mount `/proc`, `/sys`, and `/dev`.
- Capture kernel and init logs.
- Identify storage and expected partitions without mounting them read-write.
- Start a documented USB debug transport when supported.
- Expose a minimal shell or Rust diagnostic service.
- Provide explicit `reboot` and `poweroff` paths.
- Default all discovered Android partitions to read-only.
- Time out or fail safely instead of modifying storage.

Start with the simplest reliable init. A Rust PID 1 is a project goal, not a requirement for the first boot. If BusyBox is used initially, document the replacement plan.

### Test order

1. Unit-test initramfs generation.
2. Boot the initramfs under QEMU AArch64 where applicable.
3. Validate archive contents and permissions.
4. Validate boot-image size and metadata.
5. Present the exact temporary-boot command and recovery plan.
6. Wait for explicit human approval.
7. Attempt temporary boot only if the bootloader is known to support it.

### Acceptance criteria

- The phone runs project-controlled `/init`.
- A developer can retrieve logs without relying on the display.
- Storage is visible and remains unmodified.
- Reboot returns to the stock OS.
- The attempt is documented whether it succeeds or fails.

## Phase 4: Minimal Root Filesystem and Rust System Service

### Goal

Boot a minimal AArch64 Linux root filesystem and start the Rust mobile core.

### Tasks

1. Define a reproducible root-filesystem build.
2. Keep the initial package set intentionally small.
3. Start D-Bus, device management, logging, and the Rust system service.
4. Mount stock Android partitions read-only only when needed for firmware discovery.
5. Implement mockable hardware traits for:
  - Battery and charging
  - Display brightness
  - Network state
  - Audio route
  - Modem state
  - Suspend request
6. Expose a versioned project-owned D-Bus API.
7. Add structured boot health reporting.

### Acceptance criteria

- Root filesystem reaches its intended target reliably.
- Rust mobile core starts and logs its version.
- Hardware enumeration produces a machine-readable report.
- A failed optional peripheral does not prevent a debug login.

## Phase 5: Mainline Linux Bring-Up

### Goal

Boot an upstream-oriented kernel and progressively replace the downstream kernel.

### Rules

- Keep mainline patches small and organized by subsystem.
- Prefer upstream bindings and existing SM6350 drivers.
- Do not copy downstream device-tree properties blindly. Validate bindings and hardware meaning.
- Send generally useful fixes upstream when they are ready.
- Track downstream-only dependencies explicitly.

### Bring-up order

1. Boot CPU, interrupt controller, timers, reserved memory, and serial/debug transport.
2. UFS storage in read-only testing.
3. Regulators, clocks, pin control, and remote processors.
4. USB and charging observation.
5. Display controller and panel.
6. Touchscreen and hardware keys.
7. GPU acceleration.
8. Wi-Fi and Bluetooth.
9. Battery, thermal management, and charging policy.
10. Audio.
11. Sensors.
12. Modem data, SMS, and voice.
13. Cameras and fingerprint reader.

### Hardware status vocabulary

Use only these states in `docs/status.md`:

- `unknown`
- `not-started`
- `blocked`
- `boots`
- `detected`
- `partial`
- `working`
- `working-with-caveats`
- `regressed`

Every state other than `unknown` and `not-started` must link to evidence: a log, test procedure, commit, or issue.

### First mainline acceptance criteria

- Mainline kernel reaches project-controlled initramfs.
- Debug transport works.
- UFS is detected without corruption.
- Reboot returns to the stock OS.

## Phase 6: Mobile Graphics and Shell

### Goal

Run a touch-first Rust Wayland shell on the phone.

### Development sequence

1. Run the shell nested on a desktop Wayland compositor.
2. Run against the defined `qemu-aarch64` virtual-phone profile and its generic DRM or framebuffer backend.
3. Run with software rendering on the phone.
4. Add GPU acceleration after basic correctness.

### Initial shell scope

- Full-screen application presentation
- Launcher
- Status bar
- Back/home/app-switching gestures
- On-screen keyboard integration
- Rotation
- Brightness control
- Lock surface without claiming production security
- Notification surface
- Quick settings
- Crash recovery to a debug session

Do not begin with elaborate animation, theme marketplaces, or a Hyprland compatibility layer. First establish correct input, lifecycle, scaling, and recovery behavior.

### Omarchy-inspired experience

After the shell is stable, add:

- Theme files and palette switching
- Strong typography and dark defaults
- Command palette
- Workspace model
- Keyboard-driven navigation when a keyboard is present
- Developer-oriented terminal and tools
- Tiling or split-screen behavior where screen size permits
- Optional Hyprland session for docked desktop mode

Hyprland is not the initial handheld compositor.

## Phase 7: Connectivity and Phone Functions

### Goal

Implement basic phone functionality behind portable Rust APIs.

### Order

1. Wi-Fi
2. Bluetooth
3. Speaker and microphone
4. Cellular modem discovery
5. SIM state and network registration
6. Cellular data
7. SMS receive
8. SMS send
9. Incoming call indication
10. Outgoing and answered calls
11. Audio routing during calls
12. Suspend/wake for calls and messages
13. VoLTE/IMS investigation

### Adapter boundary

Design telephony around a Rust trait with multiple possible backends:

```
Telephony API
  -> ModemManager/QMI backend
  -> Android RIL compatibility backend
  -> Mock backend for host tests
```

Do not couple the dialer or messages app to a particular backend. Treat reliable VoLTE as a separate risk: data and SMS success do not prove that carrier voice will work.

### Acceptance criteria for the first phone MVP

- Boots without Android userspace.
- Displays the mobile shell.
- Touch, brightness, charging state, and suspend/resume work.
- Connects to Wi-Fi.
- Runs a usable browser.
- Connects to cellular data where supported.
- Sends and receives SMS.
- Makes and receives calls where modem and carrier IMS support permit.

## Phase 8: Portability and Solana Seeker Preparation

### Goal

Prove that product logic is not tied to OnePlus hardware.

### Tasks

- Run all project-owned services with mock adapters in CI.
- Treat the `qemu-aarch64` profile as the contract test for portable product behavior; keep it free of Seeker-only assumptions.
- Document every `billie`-specific assumption.
- Keep firmware discovery and packaging under the device directory.
- Move reusable Qualcomm logic into clearly named shared components only after a second consumer exists.
- Define the minimum board-support interface for a second phone.
- Add Solana functionality only above the hardware abstraction boundary.
- Treat secure-element or Seed Vault access as unavailable until documented and proven on a physical Seeker device.
- When Seeker access is available, add a separate board-support target and evidence-backed hardware matrix. Do not promote QEMU results to Seeker compatibility claims.

## Immediate Agent Assignment

An LLM beginning work from this file should execute the following assignment:

1. Inspect the current repository and report what already exists.
2. Do not overwrite user work or assume the repository is empty.
3. Create a short implementation plan for Phase 0 only.
4. Bootstrap the minimal Rust workspace, documentation skeleton, lint/test configuration, and mock battery example.
5. Add the documented `qemu-aarch64` virtual-phone profile, launch/check recipes, and one deterministic mock-state smoke test.
6. Add a non-destructive device-audit tool that defaults to redacting unique identifiers.
7. Do not connect to or modify the phone unless the human explicitly asks during the active session.
8. Run formatting, linting, and tests.
9. Update `docs/status.md` with evidence-backed results, labeling virtual-target results as `mock` or `QEMU`.
10. End with:
  - Files changed
  - Commands run
  - Test results
  - Assumptions
  - Blockers
  - Next safe step
  - Any action awaiting human approval

## Definition of Done for Phase 0

Phase 0 is complete when a new contributor or agent can clone the repository, install documented prerequisites, run one command to validate the workspace, execute the mock hardware example, and understand that no phone-changing action is authorized.

## Known Risks


| Risk                                   | Consequence                                       | Mitigation                                                |
| -------------------------------------- | ------------------------------------------------- | --------------------------------------------------------- |
| Carrier bootloader restriction         | Cannot run custom images                          | Verify exact BE2029 unit before hardware work             |
| Missing complete `billie` mainline DTS | Significant kernel porting work                   | Use downstream kernel only as bring-up scaffolding        |
| Proprietary firmware                   | Some hardware cannot be redistributed or replaced | Extract locally; store hashes/manifests, not blobs        |
| Virtual-target fidelity                | False confidence in physical-device compatibility | Keep QEMU generic; label mock evidence; validate hardware on devices |
| GPU/display dependencies               | No accelerated UI                                 | Start with logs and software rendering                    |
| Modem/IMS dependency                   | Calls may fail despite data/SMS                   | Isolate telephony backends; track VoLTE separately        |
| Audio routing complexity               | Calls connect but are unusable                    | Test ALSA/PipeWire routes before dialer polish            |
| Suspend defects                        | Battery drain or missed calls                     | Make suspend a gated, measured milestone                  |
| A/B slot mistakes                      | Device fails to boot                              | Record slot state; never switch or flash without approval |
| Accidental secret capture              | Privacy/security exposure                         | Redact audit outputs and keep unique IDs out of Git       |


## Upstream References

- OnePlus `billie` device tree: [https://github.com/LineageOS/android_device_oneplus_billie](https://github.com/LineageOS/android_device_oneplus_billie)
- OnePlus SM6350 kernel: [https://github.com/LineageOS/android_kernel_oneplus_sm6350](https://github.com/LineageOS/android_kernel_oneplus_sm6350)
- LineageOS `billie` install guide: [https://wiki.lineageos.org/devices/billie/install/](https://wiki.lineageos.org/devices/billie/install/)
- Android bootloader overview: [https://source.android.com/docs/core/architecture/bootloader](https://source.android.com/docs/core/architecture/bootloader)
- Android vendor boot partitions: [https://source.android.com/docs/core/architecture/partitions/vendor-boot-partitions](https://source.android.com/docs/core/architecture/partitions/vendor-boot-partitions)
- Mainline Linux: [https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/)
- Rust for Linux: [https://rust-for-linux.com/](https://rust-for-linux.com/)
- Smithay: [https://github.com/Smithay/smithay](https://github.com/Smithay/smithay)
- ModemManager: [https://modemmanager.org/docs/modemmanager/](https://modemmanager.org/docs/modemmanager/)

## Project Principle

Optimize for a sequence of small, observable, reversible successes:

```
build -> emulate -> validate -> inspect -> request approval -> temporarily boot -> collect evidence
```

Do not optimize for the appearance of progress by generating large amounts of untested scaffolding. On phone bring-up, a reliable log from a minimal initramfs is more valuable than a polished shell that cannot boot.
