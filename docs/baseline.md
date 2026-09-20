# Virtual Phone Tooling Baseline

## Purpose

This is the initial, implementation-ready baseline for Ferrum's generic AArch64 QEMU virtual phone. It is the primary development and CI target for a Rust-first mobile shell and its project-owned services.

Ferrum is the ship and product name. Ferrix is the kernel/userspace codename and runtime namespace. Gossan is reserved for a future device image; it does not rename the generic QEMU target or imply that a physical target exists. The project is not a new Linux distribution. It composes an existing mobile-Linux substrate with a deliberately small project layer: the Ferrum visual system, Rust mobile services, and (where justified) a Rust Wayland shell. postmarketOS and its tooling provide the initial guest-image, packaging, and QEMU route. Existing Linux system services remain authoritative for real hardware integration.

The Solana Seeker is the intended eventual device target, subject to a separate, explicitly approved board-support proposal. Nothing in this baseline asserts Seeker compatibility, examines a Seeker, or authorizes device work. The generic emulator is the portability contract until that proposal supplies a recovery plan, capability evidence, and a Seeker-specific adapter package.

This is a generic virtual-phone environment. It must never claim to emulate or validate a physical device's firmware, secure hardware, modem/radio hardware, vendor drivers, Android compatibility layers, verified boot, or boot chain.

## Baseline decisions

| Concern | Initial decision | Reason |
| --- | --- | --- |
| Emulated machine | QEMU `virt`, AArch64 | Stable generic ARM platform; avoids encoding phone-vendor assumptions. |
| Host operating system | macOS on Apple Silicon is supported; Linux is the CI reference host | Matches the current development host while keeping CI close to the guest build environment. |
| QEMU acceleration | HVF on Apple Silicon when available; TCG fallback everywhere | Acceleration improves local iteration but must not be required for correctness. |
| Guest architecture | `aarch64-unknown-linux-gnu` | Matches the architecture intended for future mobile targets. |
| Initial guest boot | Generic arm64 Linux kernel, initramfs, serial console | Smallest observable boot path; no boot-image tooling or physical device required. |
| Root filesystem | Build a postmarketOS rootfs with `pmbootstrap`; permit a stripped serial-only CI image or initramfs overlay | Reuses the distribution's package and image pipeline while preserving a small, observable boot check. |
| UI development | Nested Smithay shell on the host first; QEMU integration second | Fast UI feedback while QEMU validates guest lifecycle and AArch64 artifacts. |
| Hardware behavior | Versioned `hardware-emulator` profile | Deterministic state changes for local tests and CI. |
| CI | Linux runner, TCG-compatible bounded smoke test | No dependency on Apple hardware, display server, or physical phone. |
| Guest substrate | postmarketOS via `pmbootstrap` | Provides established mobile-Linux packaging, image construction, and a QEMU workflow rather than recreating a distribution build system. |
| Project product layer | Ferrum Rust services and mobile shell | Keeps the work differentiated while reusing mature Linux hardware services and distribution infrastructure. |

## Existing-project options and chosen boundary

The project deliberately builds on, rather than replaces, the following projects. Versions, availability, and compatibility must be verified at implementation time; links are maintained here as the architectural starting points.

| Project | What it already provides | Role in this project |
| --- | --- | --- |
| [postmarketOS](https://postmarketos.org/) / [pmbootstrap](https://docs.postmarketos.org/pmbootstrap/main/usage.html) | Mobile-Linux distribution tooling, package recipes, image builds, and QEMU image execution | **Chosen initial substrate.** Use its generic AArch64 QEMU support and packaging rather than building a rootfs distribution pipeline. |
| [Mobian](https://wiki.debian.org/Mobian/) | Debian adapted for mobile devices | Debian-based alternative for package and service compatibility research; not the initial build substrate. |
| [Droidian](https://droidian.org/) | Debian-derived mobile Linux for Android phones, using libhybris and Halium | Potential future reference if a target requires Android-derived hardware enablement; outside the emulator-first, mainline-oriented baseline. |
| [Ubuntu Touch / UBports](https://docs.ubports.com/en/latest/) | A complete touch-oriented mobile operating system and Lomiri environment | Product and convergence reference; not selected because this project needs a Rust-first project layer and direct standard-Linux service adapters. |
| [Maemo Leste](https://maemo-leste.github.io/) | Devuan/Debian-derived free mobile environment | Historical and device-porting reference; not a project dependency by default. |
| [Phosh](https://phosh.mobi/) and [Phoc](https://gitlab.gnome.org/World/Phosh/phoc) | Mature GTK/GNOME phone shell and compositor, including standard mobile UI integration | Reference implementation and fallback UI for validating the guest and system-service integration. Not the Rust-first product shell. |
| [Plasma Mobile](https://plasma-mobile.org/) | KDE-based mobile workspace and applications | Alternative reference UI; useful for compatibility and UX comparisons, not a project dependency by default. |
| [Sxmo](https://sxmo.org/) | Scriptable, minimal mobile user environment | Alternative reference UI for low-resource/mobile interaction patterns. |
| [Catacomb](https://catacombing.org/) | Rust/Smithay Wayland compositor and mobile-focused applications | Closest existing Rust mobile compositor. Evaluate reuse, contribution, or a maintained fork before writing a compositor from first principles. |
| [Smithay](https://github.com/Smithay/smithay) | Rust building blocks for Wayland compositors | Chosen compositor foundation if Catacomb cannot meet the product requirements. It is a library, not a complete phone shell. |
| [ModemManager](https://modemmanager.org/), NetworkManager, UPower, PipeWire, and iio-sensor-proxy | Standard Linux integrations for cellular, networking, power, audio, and sensors | Keep behind project-owned Rust interfaces. Do not reimplement their device protocols. |

The initial decision is therefore: **postmarketOS/pmbootstrap for the guest substrate; a Rust project layer for product behavior; Catacomb evaluation before a custom Smithay shell; and existing Linux services behind typed adapters.**

## Supported host profile: macOS / Apple Silicon

### Required installations

Install these with Homebrew unless the team later supplies a pinned development container or Nix environment:

```sh
xcode-select --install
brew install qemu rustup-init just dtc cpio lz4 xz pkg-config coreutils gnu-tar make ninja rsync git
```

Then install Rust stable and the guest target:

```sh
rustup-init -y --profile minimal --default-toolchain stable
rustup target add aarch64-unknown-linux-gnu
rustup component add rustfmt clippy
```

Install exactly one Linux-container runtime for building guest images. Docker Desktop is the documented initial option; OrbStack or Colima may be supported later only after the same checks pass:

```sh
brew install --cask docker
```

Start the container runtime and confirm `docker run --rm hello-world` succeeds before attempting guest-image work.

### Required verification

Run and record only tool versions, never device identifiers:

```sh
uname -m
qemu-system-aarch64 --version
rustc --version
cargo --version
docker version
just --version
dtc --version
```

Expected host architecture is `arm64`. QEMU must run with `-accel hvf` when supported, but every project check must also work with `-accel tcg` so it remains portable to CI.

## Linux / CI reference profile

Use Ubuntu LTS or another supported Linux image with these equivalents installed: QEMU system AArch64, Rust stable plus `aarch64-unknown-linux-gnu`, Docker or a compatible container engine, `just`, `dtc`, `cpio`, `lz4`, `xz`, `pkg-config`, `clang`, `make`, `ninja`, `rsync`, and Git.

The CI workflow must not require a graphical display, KVM, HVF, USB access, `adb`, `fastboot`, proprietary firmware, or a physical phone. It runs the same QEMU smoke test with TCG and serial output.

## Repository environment contract

Do not require global paths or untracked machine-specific scripts. Add these files as implementation begins:

```text
.env.example                    # documented, non-secret optional overrides
.gitignore                      # excludes .env, .cache/, out/, and generated images
containers/guest-builder/       # pinned Linux image and build entrypoint
targets/qemu-aarch64/
  README.md                     # prerequisites and supported launch modes
  profile.toml                  # versioned emulator mock configuration
  scripts/                      # small, checked-in launch helpers if needed
  kernel/                       # config fragments, not generated kernel binaries
  initramfs/                    # init and rootfs manifest
```

Use these environment variables only as documented overrides. Defaults must work without them:

| Variable | Default | Meaning |
| --- | --- | --- |
| `FERRIX_OUT_DIR` | `out` | Generated, ignored build artifacts. |
| `FERRIX_CACHE_DIR` | `.cache/ferrix` | Download and compiler cache; ignored. |
| `FERRIX_QEMU_ACCEL` | `auto` | `hvf`, `kvm`, `tcg`, or `auto`; `auto` resolves per host. |
| `FERRIX_QEMU_GRAPHICS` | `off` | `off`, `cocoa`, `gtk`, or another documented backend. Serial remains mandatory. |
| `FERRIX_QEMU_PROFILE` | `targets/qemu-aarch64/profile.toml` | Mock product profile path. |
| `FERRIX_CONTAINER_RUNTIME` | `docker` | Container command used by image-build recipes. |

Never put credentials, personal identifiers, or personal backups in `.env`, `out/`, or Git.

## Target extension contract

`targets/qemu-aarch64/` is the initial and only supported target. It defines the portable behavior contract for the rest of the project and is built from the selected postmarketOS/pmbootstrap guest substrate where practical.

The first prospective additional target is `targets/boards/solana-seeker/`, but this directory must remain absent until hardware work is separately approved. Keep target selection and adapter wiring in the session/bootstrap layer. Product crates, applications, and `hardware-api` must not import a target package or contain target-specific conditionals.

Each target package must include a versioned manifest, a capability matrix for every `hardware-api` capability (`supported`, `unsupported`, or `mocked`), evidence for each claimed capability, and an explicit recovery procedure for hardware actions. A future Solana Seeker proposal must additionally establish its supported boot route, kernel and driver sources, partition/recovery procedure, modem and audio integration, and a no-device-data collection policy before any device action. Translate target behavior through a dedicated adapter; promote code into `hardware-linux` only after a demonstrated second consumer.

## First virtual-phone profile

Create `targets/qemu-aarch64/profile.toml` with a schema version and values such as these. Values are emulator-test configuration, not claims about physical hardware:

```toml
schema_version = 1
name = "emulator-dev"

[display]
width = 1080
height = 2340
scale = 1.0
orientation = "portrait"

[input]
touch = true
rotation = true

[power]
battery_percent = 75
charging = false

[connectivity]
wifi = "connected"
bluetooth_present = true
modem = "registered"
sim = "present"
```

The profile needs a controlled mechanism for test transitions: for example a JSON fixture passed at boot, a Unix-socket test endpoint available only in test builds, or an explicit mock-control command. Tests must be able to change charging, battery percentage, network state, modem registration, SIM state, rotation, and suspend outcome deterministically.

## Artifacts and build pipeline

The first implementation should produce the following generated artifacts under `out/qemu-aarch64/`:

```text
Image                         # generic arm64 Linux kernel
initramfs.cpio.gz             # minimal root filesystem and /init
qemu-virt.dtb                 # only if the selected kernel flow needs an explicit DTB
mobile-core                   # AArch64 project service
manifest.json                 # input revisions, hashes, profile, and command metadata
serial.log                    # launch output; scrub before attaching externally
```

Build or invoke the postmarketOS guest-image flow in a pinned builder environment. Pin the builder base image by digest and record the `pmbootstrap`, package-repository, kernel, and project input revisions in the manifest. The host may run QEMU directly so graphical and HVF support remain available.

Initial `/init` responsibilities:

1. Mount `/proc`, `/sys`, `/dev`, and a temporary runtime directory.
2. Start structured logging to the serial console.
3. Load the QEMU profile and start the emulator adapter service.
4. Start the Rust mobile core.
5. Emit a machine-readable boot-health record and keep a debug shell or orderly shutdown path available.

Do not add a bootloader, Android boot image, AVB, vendor partitions, or firmware extraction to this target.

## Required task-runner interface

Implement the following `justfile` recipes. Each must print the resolved paths and commands, fail with actionable prerequisite checks, and avoid destructive operations:

| Recipe | Responsibility | Completion signal |
| --- | --- | --- |
| `just host-check` | Verify host tools, Rust target, container runtime, and QEMU acceleration availability | Clear pass/fail table with install hints. |
| `just qemu-build` | Build/pull the pinned builder and produce guest artifacts | Manifest with hashes under `out/qemu-aarch64/`. |
| `just qemu-run` | Start an interactive virtual phone | Serial console plus optional display/touch window. |
| `just qemu-check` | Run a bounded headless smoke test | Exit 0 only after boot-health and mock state transition assertions. |
| `just qemu-clean` | Remove only resolved generated QEMU artifacts beneath `out/qemu-aarch64/` | Printed target path and successful removal. |
| `just shell-nested` | Run the compositor nested on the desktop with the same mock profile | Shell starts and reports profile/version. |
| `just test` | Run Rust unit/integration tests without QEMU unless explicitly needed | Standard Cargo test result. |

`qemu-check` is the CI contract. It must enforce a timeout, capture serial output, assert a boot-health record, inject at least one mock state transition (initially unplugged → charging), and shut QEMU down cleanly. It must not hang waiting for a graphical display or human input.

## Implementation order for Codex

1. Inspect the current workspace and preserve all user changes.
2. Establish a reproducible postmarketOS/pmbootstrap generic AArch64 QEMU guest route; retain a minimal serial-first path for bounded CI checks.
3. Add the Rust workspace, `hardware-api`, `hardware-emulator`, and a small emulator battery/charger state example.
4. Add `.gitignore`, `.env.example`, and the `targets/qemu-aarch64/` skeleton. Keep the profile parser strict and schema-versioned.
5. Add `just host-check`; do not install tools automatically. Report missing prerequisites and their documented commands.
6. Add `just qemu-build`, `qemu-run`, and bounded headless `qemu-check` around the selected guest route.
7. Cross-compile the mobile core for `aarch64-unknown-linux-gnu`, launch it in the guest, and record boot health.
8. Add typed adapters for existing Linux services and deterministic mock-control integration tests, then wire `qemu-check` into Linux CI.
9. Evaluate Catacomb against the product requirements; use it, contribute upstream, or document the reason to build the nested Smithay shell path. Then add an optional QEMU graphical/input backend.
10. Document every command, expected output, existing-project decision, and known host limitation in `targets/qemu-aarch64/README.md` and `docs/status.md`.

At every step, QEMU evidence must be tagged `mock` or `QEMU`. No physical-device action is authorized by this baseline.

## Acceptance gates

### Gate A — host ready

- `just host-check` passes on the documented macOS host profile.
- Rust formatting, Clippy, and unit tests pass.
- The builder container starts and can write only to the designated output/cache directories.

### Gate B — serial guest boot

- `just qemu-build` creates a manifest and the kernel/initramfs artifacts.
- `just qemu-check` boots an AArch64 guest under TCG and finds a structured boot-health record before its timeout.
- The guest starts the mock adapter and Rust mobile core.

### Gate C — portable product behavior

- CI runs `qemu-check` headlessly.
- A test changes battery charging state and observes the corresponding project-owned API event.
- The same profile works in host-nested shell tests and QEMU guest integration tests where applicable.

### Gate D — optional graphical path

- `just qemu-run` presents a configured portrait display and accepts testable absolute-pointer input.
- Rotation, lifecycle, and crash recovery are demonstrated without reclassifying any physical hardware support.

## Explicit exclusions for this baseline

- Connecting to, auditing, unlocking, booting, flashing, or modifying a physical phone.
- Building an image for, or claiming support for, a physical device.
- Creating `targets/boards/solana-seeker/`, connecting to a Solana Seeker, or claiming Solana Seeker support. These require a separate board-support proposal and explicit authorization.
- Android Studio/AVD as the Linux-phone emulator. It may be useful for Android-app work later, but it does not validate this Linux rootfs, init, or Wayland shell.
- Modem, SMS, calling, real Wi-Fi/Bluetooth, secure hardware, camera, fingerprint, GPU, or vendor-firmware validation.
- Persistent storage images by default. Use ephemeral disks until a storage test has an explicit persistence and cleanup design.

## Initial risks and mitigations

| Risk | Mitigation |
| --- | --- |
| QEMU graphical backends differ by host | Keep serial and headless TCG checks authoritative; make graphics optional. |
| Cross-linker/toolchain mismatch | Build guest Linux artifacts in the pinned container and fail early in `host-check`. |
| Long emulated boot time in CI | Keep initramfs minimal, avoid graphics, use a strict timeout, and cache immutable inputs. |
| Mock behavior drifts from product APIs | Keep the profile schema versioned and exercise it in both unit and integration tests. |
| QEMU is mistaken for physical-device validation | Use the terms `generic virtual phone`, `mock`, and `QEMU` in commands, logs, and status evidence. |
