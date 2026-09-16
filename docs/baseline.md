# Virtual Phone Tooling Baseline

## Purpose

This is the initial, implementation-ready baseline for the project's generic AArch64 QEMU virtual phone. It is the primary development and CI target for portable Rust services, the root filesystem, and the mobile shell. Physical-device support is a later, separate board-support effort.

This is a generic virtual-phone environment. It must never claim to emulate or validate a physical device's firmware, secure hardware, modem/radio hardware, vendor drivers, Android compatibility layers, verified boot, or boot chain.

## Baseline decisions

| Concern | Initial decision | Reason |
| --- | --- | --- |
| Emulated machine | QEMU `virt`, AArch64 | Stable generic ARM platform; avoids encoding phone-vendor assumptions. |
| Host operating system | macOS on Apple Silicon is supported; Linux is the CI reference host | Matches the current development host while keeping CI close to the guest build environment. |
| QEMU acceleration | HVF on Apple Silicon when available; TCG fallback everywhere | Acceleration improves local iteration but must not be required for correctness. |
| Guest architecture | `aarch64-unknown-linux-gnu` | Matches the architecture intended for future mobile targets. |
| Initial guest boot | Generic arm64 Linux kernel, initramfs, serial console | Smallest observable boot path; no boot-image tooling or physical device required. |
| Root filesystem | Build in a pinned Linux container; begin with a minimal BusyBox-based initramfs | Reproducible across macOS and Linux; avoids a host-specific rootfs build. |
| UI development | Nested Smithay shell on the host first; QEMU integration second | Fast UI feedback while QEMU validates guest lifecycle and AArch64 artifacts. |
| Hardware behavior | Versioned `hardware-emulator` profile | Deterministic state changes for local tests and CI. |
| CI | Linux runner, TCG-compatible bounded smoke test | No dependency on Apple hardware, display server, or physical phone. |

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
| `OMARCHY_OUT_DIR` | `out` | Generated, ignored build artifacts. |
| `OMARCHY_CACHE_DIR` | `.cache/omarchy` | Download and compiler cache; ignored. |
| `OMARCHY_QEMU_ACCEL` | `auto` | `hvf`, `kvm`, `tcg`, or `auto`; `auto` resolves per host. |
| `OMARCHY_QEMU_GRAPHICS` | `off` | `off`, `cocoa`, `gtk`, or another documented backend. Serial remains mandatory. |
| `OMARCHY_QEMU_PROFILE` | `targets/qemu-aarch64/profile.toml` | Mock product profile path. |
| `OMARCHY_CONTAINER_RUNTIME` | `docker` | Container command used by image-build recipes. |

Never put credentials, personal identifiers, or personal backups in `.env`, `out/`, or Git.

## Target extension contract

`targets/qemu-aarch64/` is the initial and only supported target. It defines the portable behavior contract for the rest of the project.

When a concrete additional target is approved, add it as an isolated package at `targets/boards/<target-id>/`. Keep target selection and adapter wiring in the session/bootstrap layer. Product crates, applications, and `hardware-api` must not import a target package or contain target-specific conditionals.

Each target package must include a versioned manifest, a capability matrix for every `hardware-api` capability (`supported`, `unsupported`, or `mocked`), evidence for each claimed capability, and an explicit recovery procedure for hardware actions. Translate target behavior through a dedicated adapter; promote code into `hardware-linux` only after a demonstrated second consumer.

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

Build all Linux guest artifacts inside the pinned builder container. Pin the container base image by digest and pin kernel/rootfs inputs by immutable revision and checksum. The host may run QEMU directly so graphical and HVF support remain available.

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
2. Add the Rust workspace, `hardware-api`, `hardware-emulator`, and a small emulator battery/charger state example.
3. Add `.gitignore`, `.env.example`, and the `targets/qemu-aarch64/` skeleton. Keep the profile parser strict and schema-versioned.
4. Add `just host-check`; do not install tools automatically. Report missing prerequisites and their documented commands.
5. Create the pinned Linux guest-builder definition and a minimal serial-first arm64 kernel/initramfs build.
6. Add `just qemu-build`, `qemu-run`, and bounded headless `qemu-check`.
7. Cross-compile the mobile core for `aarch64-unknown-linux-gnu`, launch it in the guest, and record boot health.
8. Add deterministic mock-control integration tests, then wire `qemu-check` into Linux CI.
9. Add the nested Smithay shell path and then an optional QEMU graphical/input backend.
10. Document every command, expected output, and known host limitation in `targets/qemu-aarch64/README.md` and `docs/status.md`.

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
