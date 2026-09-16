# Generic AArch64 QEMU virtual phone

This is a **QEMU/mock** target, never a physical-device claim. It uses QEMU `virt`, a serial console, and the versioned mock profile in `profile.toml`.

## Install (macOS / Apple Silicon)

Install the host tools once:

```sh
xcode-select --install
brew install qemu just cpio
brew install --cask docker
rustup target add aarch64-unknown-linux-gnu
```

Open Docker Desktop and wait until it reports that the engine is running. Then, from the repository root, verify the environment:

```sh
just host-check
```

It should report QEMU, Docker, the Rust AArch64 target, and `cpio` as available. The host-side AArch64 linker is optional for this initial container-built guest.

## Build and run

Build the generated artifacts (kernel, initramfs, manifest) with:

```sh
just qemu-build
```

Start the interactive serial virtual phone:

```sh
just qemu-run
```

The guest starts a BusyBox shell on the serial console. Press `Ctrl-a`, then `x` to terminate QEMU cleanly from the terminal. To run the bounded CI-style smoke check instead, use:

```sh
just qemu-check
```

`qemu-check` uses portable TCG, asserts the `boot-health` JSON record and an unplugged-to-charging mock event, then powers the guest down cleanly. Inspect the captured serial output at `out/qemu-aarch64/serial.log`.

To open the experimental QEMU Cocoa window and start the guest Wayland compositor, run:

```sh
OMARCHY_QEMU_GRAPHICS=cocoa just qemu-run
```

This is the first graphical integration step: Weston runs inside the QEMU guest and the window is QEMU's virtual display. The project Rust shell is the next replacement for this reference compositor.

On Apple Silicon, `qemu-run` selects HVF automatically; `qemu-check` always uses portable TCG. The initial serial-only image uses the pinned Alpine arm64 builder image and its generic `linux-virt` kernel; the planned postmarketOS/pmbootstrap guest remains the full mobile-image route.

The first boot contract is a JSON `boot-health` record on serial. The smoke check also requires a deterministic mock transition from unplugged to charging.
