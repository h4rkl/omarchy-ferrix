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

The host window is sized to fit the desktop. `qemu-run.sh` reads the host's
visible screen height and backing scale (macOS, via `osascript`), scales the
profile's portrait panel to that height in physical pixels (aspect ratio
preserved, never upscaled), and passes the result to the guest as
`omarchy.display=WxH`. The guest init pins Weston to exactly that mode through a
generated `/etc/omarchy/weston.ini` (connector `Virtual-1`, idle blanking off),
because QEMU's Cocoa UI otherwise pushes its own window size into virtio-gpu and
the guest surface ends up at an arbitrary size. On a 1920x1080-point Retina
desktop this opens a fixed 415x900-point window showing an 830x1800 guest panel
at 1:1 host pixels. Set `OMARCHY_QEMU_WINDOW_HEIGHT` (logical points) to
override the automatic height; QEMU's View menu "Zoom To Fit" makes the window
resizable. Headless sessions (`-display none`) keep the full profile panel so
framebuffer evidence stays at the mock geometry, and `just qemu-tap` coordinates
remain profile pixels in both cases. The guest reports the chosen geometry as a
`display-geometry` record on serial. The GTK backend path is unverified on this
macOS host.

The first visible surface is `mobile-basic-screen`, a fullscreen Rust Wayland client (crate `crates/mobile-basic-screen`) that software-renders an Omarchy-styled basic screen — Tokyo Night palette, JetBrains Mono type, GrapheneOS-style status bar, launcher grid, and gesture pill — from the emulator profile's battery, network, and display state. It is built on `smithay-client-toolkit`, `tiny-skia`, and `fontdue`, and is intentionally a reference surface, not the final Rust shell or compositor. Preview it on any host without QEMU:

```sh
cargo run -p mobile-basic-screen -- targets/qemu-aarch64/profile.toml --preview out/preview.ppm
```

`qemu-build` compiles the guest binary for aarch64 musl inside a pinned `rust` container image; its digest and the binary hash are recorded in `out/qemu-aarch64/manifest.json`.

On Apple Silicon, `qemu-run` selects HVF automatically; `qemu-check` always uses portable TCG. The initial serial-only image uses the pinned Alpine arm64 builder image and its generic `linux-virt` kernel; the planned postmarketOS/pmbootstrap guest remains the full mobile-image route.

The first boot contract is a JSON `boot-health` record on serial. The smoke check also requires a deterministic mock transition from unplugged to charging.

## Agent-operated graphical session

Use the checked-in agent session when an agent needs to inspect and drive the QEMU
surface. It retains the mandatory serial console as a file, exposes only a local
QMP Unix socket, and writes all artifacts beneath the resolved ignored output path.
It is QEMU/mock evidence, not physical-device validation.

```sh
just qemu-build
just qemu-agent-start
just qemu-frame
```

`qemu-frame` produces `out/qemu-aarch64/agent/frame.png`: the rendered QEMU
framebuffer. Attach or ask Codex to inspect that exact PNG after the guest has
reported `wayland-ready` in `out/qemu-aarch64/agent/serial.log`. A running QEMU
process by itself is not graphical readiness evidence. The command emits a
machine-readable QEMU record and fails if the framebuffer is entirely blank,
while retaining the PNG for diagnosis.

The agent session is headless by default (`-display none`) and uses portable TCG
unless `OMARCHY_QEMU_ACCEL` is explicitly set, so it remains useful on hosts
without a display server. To simultaneously open a local QEMU window, choose a
host backend:

```sh
OMARCHY_QEMU_GRAPHICS=cocoa just qemu-agent-start
```

For deterministic input, coordinates are portrait framebuffer pixels from
`profile.toml`; they are converted to QEMU's absolute pointer range:

```sh
just qemu-tap 540 1170
just qemu-swipe 540 2250 540 1800
just qemu-key esc
```

The interactive reference surface accepts pointer and Wayland touch input. Tap
any launcher tile to open its QEMU/mock app view, tap the status bar for quick
settings, drag the brightness rail, swipe down from the top for notifications,
swipe right from the left edge for Back, or swipe up from the bottom for Home.
Virtual-tablet input displays a transparent blue touch ring with a center dot
marking the exact hotspot; native Wayland touch contacts do not show a cursor.
These interactions demonstrate the guest input path; this client remains a
reference surface rather than the final product compositor or mobile shell.

### Terminal app

The Terminal tile opens a working shell inside the reference surface: the
client spawns BusyBox `/bin/sh` on a PTY and renders it through
`alacritty_terminal` (Alacritty's VT parser and grid, Apache-2.0) with the
same software renderer as the rest of the screen. Input comes from the
touch keyboard drawn below the grid (letters, `?123` symbols, shift within
symbols for the remaining punctuation, plus an `esc tab ctrl ` \ | ← ↓ ↑ →`
accessory row; shift and ctrl are sticky for one key) or from the virtual
keyboard QEMU attaches as `virtio-keyboard-pci`, so typing in the Cocoa
window works as well. Dragging inside the grid scrolls history. The session
keeps running while you navigate Home and back; when the shell exits, tapping
Enter restarts it. Serial evidence records are `terminal-spawned`,
`terminal-ready` (first line the shell wrote), `keyboard-input`, and
`terminal-exited`.

Drive it deterministically from the host:

```sh
just qemu-tap 852 1236            # open the Terminal tile
just qemu-type 'uname -a'         # virtual keyboard, US layout; \n sends Enter
just qemu-key ret
just qemu-frame                   # inspect out/qemu-aarch64/agent/frame.png
```

Touch-keyboard taps use the same coordinates the client hit-tests; the unit
tests in `crates/mobile-basic-screen/src/osk.rs` pin the layout. Preview the
page on any host without QEMU (canned transcript, no PTY):

```sh
cargo run -p mobile-basic-screen -- targets/qemu-aarch64/profile.toml --preview out/terminal.ppm --page terminal
```

The physical-keyboard path is a small hand-rolled US evdev table because the
static musl guest binary does not link libxkbcommon; the touch keyboard is the
primary phone input.

Inspect session state with `just qemu-agent-status`, and end it cleanly with
`just qemu-agent-stop`. Agent artifacts are:

```text
out/qemu-aarch64/agent/qmp.sock    # local QMP control socket
out/qemu-aarch64/agent/serial.log  # mandatory serial evidence
out/qemu-aarch64/agent/frame.png   # latest rendered framebuffer
out/qemu-aarch64/agent/launch.log  # resolved QEMU command and host errors
```

`python3` is the only additional host prerequisite for this mode. Its standard
library is used by the checked-in QMP helper; no Python packages are installed.

### Prompt handoff

Use an explicit evidence loop in agent prompts:

```text
Start the agent QEMU session if it is not running. Wait for the QEMU
wayland-ready record in out/qemu-aarch64/agent/serial.log, capture the actual
framebuffer with just qemu-frame, inspect out/qemu-aarch64/agent/frame.png,
then make the smallest change. Treat a blank-framebuffer failure as a graphical
failure even when Wayland is ready. Rebuild/relaunch as needed and capture a new
framebuffer before claiming a graphical result. Keep all evidence labelled QEMU
or mock; do not infer physical-device support.
```
