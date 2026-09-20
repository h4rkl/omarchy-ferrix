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

`qemu-check` uses portable TCG and is the CI contract (`.github/workflows/ci.yml`
runs it unchanged on a Linux arm64 runner). It waits for the persistent
`mobile-core` daemon's `boot-health` record and init's `QEMU`-labelled
`boot-health` that may only follow it, asserts the mock is still unplugged,
sends `set-power charging=true` over the real host-to-guest mock-control
channel, waits for the `power-changed` event the core emits, then sends
`shutdown` and requires a clean QEMU exit. Every failure is a machine-readable
`check-failed` record; there is no fallback that prints success. Inspect the
captured serial output at `out/qemu-aarch64/serial.log`.
`OMARCHY_QEMU_CHECK_TIMEOUT` (seconds, default 90) bounds the whole check.

### Guest services and the mock-control channel

The guest init starts `mobile-core` (crate `crates/mobile-core`) as a
persistent daemon before anything else. It bootstraps through
`mobile-session`, which parses `profile.toml` with the shared strict
`target-profile` parser and selects the `hardware-emulator` adapter set for
target id `qemu-aarch64`; product code (`mobile-core`, `mobile-basic-screen`,
`mobile-shell`) depends only on `hardware-api`, and `just arch-check` fails
if that boundary is crossed. The daemon serves typed state and an event
stream on `/run/omarchy/core.sock` (line-delimited JSON: `{"request":"state"}`,
`{"request":"subscribe"}`, and typed product `{"request":"command",...}`
requests) and prints every event as a JSON record on the
serial console. If the binary is missing, exits, or never binds its socket,
init prints `{"evidence":"QEMU","event":"boot-failed","reason":...}` and, in
check mode, powers off.

Every QEMU session also attaches a `virtio-serial` port named
`omarchy.control`, exposed on the host as a Unix socket
(`out/qemu-aarch64/control.sock`, or `agent/control.sock` for agent sessions;
`OMARCHY_QEMU_CONTROL_SOCKET` overrides). `mobile-core` reads line-delimited
JSON commands from the guest side (`/dev/vport1p1`, auto-detected via sysfs),
applies them through the emulator adapter, publishes the resulting
`HardwareEvent` to serial and to subscribed UI clients, and replies with an
ack that echoes the full state. Drive it from the host:

```sh
just qemu-mock set-power charging=true
just qemu-mock set-power battery_percent=40
just qemu-mock set-wifi state=disconnected
just qemu-mock set-modem registration=searching
just qemu-mock set-sim state=absent
just qemu-mock set-orientation orientation=landscape
just qemu-mock set-brightness percent=30
just qemu-mock set-audio route=headphones
just qemu-mock suspend outcome=resumed
just qemu-mock '{"cmd":"state"}'
just qemu-mock shutdown
```

Invalid or unknown commands are rejected with `"status":"error"`; they never
succeed silently. The client is `scripts/mock_control.py` (python3 standard
library only). This drives *mock* state in the virtual phone and is never a
product interface or a physical-device control.

`mobile-basic-screen` subscribes to the core instead of reading power,
network, or modem state from the profile itself, redraws on each event, and
prints a `ui-state` record, so a transition can be proven end to end:
adapter → core → UI → captured framebuffer (the charging indicator turns
green with a bolt glyph in both the status bar and the battery card).
Its quick-settings controls send portable product commands through the state
socket; they do not mutate UI-only state or expose the host mock protocol.

To open the experimental QEMU Cocoa window and start the guest Wayland compositor, run:

```sh
OMARCHY_QEMU_GRAPHICS=cocoa just qemu-run
```

The guest compositor is `mobile-shell` (crate `crates/mobile-shell`), the Rust
product compositor built on Smithay 0.7: portrait, fullscreen-only xdg-shell
toplevels, one `wl_seat` (keyboard + pointer + touch from libinput), wlr-layer-shell
reserved for status/launcher chrome, and software rendering by default (pixman
into DRM dumb buffers on `virtio-gpu`; no GL is required for correctness). The
window is QEMU's virtual display. Weston remains selectable as the reference
compositor:

```sh
OMARCHY_QEMU_SHELL=weston OMARCHY_QEMU_GRAPHICS=cocoa just qemu-run
OMARCHY_QEMU_SHELL=mobile-shell just qemu-agent-start   # the default
```

The selection travels to the guest as `omarchy.shell=` on the kernel command
line; the guest init defaults to `mobile-shell`. Readiness stays observable:
`mobile-shell` prints `compositor-ready` only once the Wayland socket is
listening and the DRM output is configured, `client-mapped` when a toplevel
commits its first buffer, and `compositor-failed` (non-zero exit) on errors;
init still prints `wayland-ready` only after the socket exists and
`mobile-basic-screen` reported a committed frame, otherwise `wayland-failed`
with a reason and the compositor log. The Catacomb evaluation that led to the
Smithay decision is `crates/mobile-shell/EVALUATION.md`.

`mobile-shell` runs nested on a Linux host session from the same mock profile
(`just shell-nested`, with `just shell-compositor-nested` retained as an alias;
both start `mobile-core` plus the compositor, `mock` evidence, GLES through winit); the
crate's policy modules (geometry, rotation, focus stack, evidence records) have
unit tests that run on every host, and `--print-config` resolves the geometry
without a display. Live orientation comes from `mobile-core` and reconfigures
the output and connected clients; `F9` remains a compositor-only debug hook.

The host window is sized to fit the desktop. `qemu-run.sh` reads the host's
visible screen height and backing scale (macOS, via `osascript`), scales the
profile's portrait panel to that height in physical pixels (aspect ratio
preserved, never upscaled), and passes the result to the guest as
`omarchy.display=WxH`. `mobile-shell` reads that value and pins its DRM mode to
it; for the Weston fallback the guest init generates `/etc/omarchy/weston.ini`
(connector `Virtual-1`, idle blanking off) with the same mode. Pinning matters
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

`qemu-build` compiles the guest binaries for aarch64 musl inside a pinned `rust` container image; its digest, the exact Rust channel from `rust-toolchain.toml`, and the binary hashes (`mobile_basic_screen_sha256`, `mobile_core_sha256`, `mobile_shell_sha256`) are recorded in `out/qemu-aarch64/manifest.json`. `mobile-shell` is linked dynamically against the guest's Alpine musl libraries (libinput, libudev, libseat, libdrm, pixman, libxkbcommon, libgbm, libgcc_s), which `qemu-build` installs into the initramfs.

On Apple Silicon, `qemu-run` selects HVF automatically; `qemu-check` always uses portable TCG. This smoke-test image uses the pinned Alpine arm64 builder image and its generic `linux-virt` kernel; the postmarketOS/pmbootstrap guest (below) is the full mobile-image route.

The boot contract is the pair of JSON `boot-health` records on serial (the core's, then init's). The smoke check also requires a deterministic mock transition from unplugged to charging, driven over the control channel.

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
framebuffer. Inspect that exact PNG after the guest has reported
`wayland-ready` in `out/qemu-aarch64/agent/serial.log`. A running QEMU
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
out/qemu-aarch64/agent/qmp.sock      # local QMP control socket
out/qemu-aarch64/agent/control.sock  # mock-control channel (just qemu-mock)
out/qemu-aarch64/agent/serial.log    # mandatory serial evidence
out/qemu-aarch64/agent/frame.png     # latest rendered framebuffer
out/qemu-aarch64/agent/launch.log    # resolved QEMU command and host errors
```

Run only one session per output directory: `qemu-build`, the check, and the
agent session all share `out/qemu-aarch64/`, so a concurrent rebuild
truncates the initramfs under a booting guest and two guests would fight over
the same control socket. Set `OMARCHY_OUT_DIR` to isolate parallel work.

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

## postmarketOS guest route (pmbootstrap)

This is the intended **full guest substrate** from `docs/baseline.md`; the
Alpine initramfs above stays the explicitly labelled smoke-test route. It is a
**QEMU** target only: generic AArch64 `virt`, never a physical device. The
first route is console-only (postmarketOS UI `console`, OpenRC); the project
shell is not in this image yet.

```sh
just pmos-build    # pinned guest-builder container runs pmbootstrap install
just pmos-check    # bounded, headless, TCG: readiness record + clean poweroff
just pmos-run      # interactive serial console (HVF on Apple Silicon)
```

Prerequisites are those of `just host-check`: Docker Desktop running (arm64
containers), `qemu-system-aarch64` and `qemu-img` from Homebrew's `qemu`, and
`python3`. Nothing is installed on the host; pmbootstrap only ever runs inside
the container.

### How it is built

`containers/guest-builder/Dockerfile` pins the Alpine 3.20 base image by digest
(the same digest `qemu-build.sh` uses), clones pmbootstrap at git tag `3.11.1`
and verifies the resolved commit at image build time (pmbootstrap 3.x is not
published on PyPI, whose latest release is 2.1.0 from 2023), and installs it
with `pip --no-index` so PyPI is never contacted. All pins live in
`targets/qemu-aarch64/pmos/pins.env`, the only file to touch when moving to a
newer channel or pmbootstrap.

`targets/qemu-aarch64/scripts/pmos-build.sh` builds that image, then runs one
container with `--privileged`. pmbootstrap creates chroots, bind-mounts
`/proc`, `/sys` and `/dev`, uses loop devices and partitions disk images; that
needs `CAP_SYS_ADMIN`, `CAP_SYS_CHROOT`, `CAP_MKNOD`, unconfined seccomp and
AppArmor, `/dev/loop-control`, and loop partition nodes such as `/dev/loop0p2`
that only appear after the container starts. There is no udev in the
container, so the entrypoint mounts devtmpfs over `/dev` (restoring `/dev/pts`
and `/dev/shm`) and the kernel creates those nodes itself; without this,
`pmbootstrap install` stops at "File did not appear: /dev/loop0p2" (observed).
`--privileged` is the one Docker flag that provides that set; no host PID or
network namespace and no extra mounts are used. The four mounts are the
pmbootstrap work directory (rw, keeps chroots, the apk cache and the pmaports
clone between runs), the checked-in `targets/qemu-aarch64/pmos/` inputs (ro),
the product binaries/profile produced by `qemu-build` (ro), and the output
directory (rw).

The work directory is a Docker volume (`OMARCHY_PMOS_WORK_VOLUME`, default
`omarchy-pmbootstrap-work`; remove it with `docker volume rm`), not the
`${OMARCHY_CACHE_DIR:-.cache/omarchy}/pmbootstrap` path the rest of the cache
uses. That is a measured deviation: on this macOS host Docker Desktop mounts
host directories with virtiofs (`no_xattr`), and as root inside the container
`chown 1000` left the file at uid 0, `chmod 4755` produced `755`, and `mknod`
failed with `EPERM`. pmbootstrap chroots need all three (ownership, suid
`su`/`sudo`/BusyBox, device nodes) and the rootfs image is copied out of those
chroots, so a bind-mounted work directory would silently corrupt the guest.
The entrypoint probes the three semantics on the mounted work directory and
refuses to continue (`guest-builder-failed`, reason `work-fs-*`) when they are
missing; Linux hosts with a real filesystem can set `OMARCHY_PMOS_WORK_BIND=1`
to bind the cache directory instead. Before that, the entrypoint verifies
loop-control, `mount(2)` and a free loop device, then drops to the
unprivileged `pmos` user because pmbootstrap refuses to run as root (it
escalates through passwordless sudo, as upstream expects).

Inside the container, `build-in-container.sh` replaces the interactive
`pmbootstrap init` with the checked-in `pmbootstrap.cfg` (device
`qemu-aarch64`, kernel flavour `stable`, UI `console`, `service_manager =
openrc`, `build_pkgs_on_install = False` so only binary packages from the
pinned channel are installed and nothing is compiled ad hoc). It clones
pmaports at the pinned branch `v26.06` and resets it to the pinned commit
(fetching `origin/main` as well, because pmbootstrap reads `channels.cfg` from
there), zaps stale chroots, runs `pmbootstrap install --password …`, and hands
the resulting image to `finalize-in-container.sh`. That step mounts the image
through a loop device, installs the project core, compositor, reference client,
profile, startup script, and readiness marker
`targets/qemu-aarch64/pmos/local.d/omarchy-boot-health.start` as
`/etc/local.d/omarchy-boot-health.start` (OpenRC's `local` service is the last
one in the default runlevel, so the record means multi-user is up), copies
`vmlinuz` and `initramfs` off the boot partition, captures the kernel command
line boot-deploy generated, reads installed package versions from the apk
database, and copies the image out. `pmbootstrap qemu` is intentionally not
used: it wants to drive QEMU itself from inside the chroot, whereas the host
runs QEMU directly so HVF and graphics stay available.

postmarketOS defaults the `console` UI to systemd on v26.06; OpenRC is pinned
here on purpose for the first route because the readiness marker is a
`local.d` script. The systemd variant is a documented follow-up, not a gap in
pmbootstrap.

The test password (`OMARCHY_PMOS_PASSWORD`, default `147147`, user `user`) is
handled in plain text by pmbootstrap and is only for this emulator image.

### Outputs and manifest

Everything is generated under the ignored `out/qemu-aarch64/pmos/`
(`OMARCHY_OUT_DIR` relocates `out/`):

```text
out/qemu-aarch64/pmos/vmlinuz            # kernel from the pmOS boot partition
out/qemu-aarch64/pmos/initramfs          # postmarketos-mkinitfs initramfs
out/qemu-aarch64/pmos/qemu-aarch64.img   # raw disk image (pmOS_boot + pmOS_root)
out/qemu-aarch64/pmos/cmdline.txt        # kernel command line boot-deploy generated
out/qemu-aarch64/pmos/manifest.json      # pins, versions, hashes
out/qemu-aarch64/pmos/build.log          # full pmbootstrap output
out/qemu-aarch64/pmos/builder-image.log  # docker build output
out/qemu-aarch64/pmos/check/serial.log   # pmos-check serial evidence
```

`manifest.json` records: the builder base image digest and built image id;
pmbootstrap version, repository, tag and commit; pmaports repository, channel,
branch and commit plus the channel's `branch_aports` and `mirrordir_alpine`
from `channels.cfg`; the Alpine and postmarketOS package mirrors; the guest
configuration (device, kernel flavour, UI, service manager, user, hostname);
`os-release`; versions of the device, kernel, base, mkinitfs, boot-deploy,
OpenRC, BusyBox and musl packages as installed; the kernel command line and
where it came from; partition labels; the readiness marker and product startup
paths and hashes; hashes of all three product binaries; the
project git revision (`-dirty` when the tree has changes) and hashes of
`pins.env`, `pmbootstrap.cfg` and the marker; and `sha256` plus size of every
exported artifact. `pmos-check` re-hashes `vmlinuz`, `initramfs` and the disk
image against the manifest before booting.

### Running and checking

`pmos-run.sh` boots with `-kernel`/`-initrd` from the exported files, the
recorded command line plus `panic=-1 omarchy.mode=<run|check>`, the disk as
`virtio-blk`, user-mode networking (`OMARCHY_PMOS_SSH_PORT=2222` adds an SSH
host forward) and `-nographic` by default, so the serial console is always
present. Set `OMARCHY_QEMU_GRAPHICS=cocoa` (or another QEMU display backend)
to attach virtio GPU/input devices and start `mobile-shell` plus
`mobile-basic-screen`. The
pristine image is never written: QEMU boots a qcow2 overlay
(`out/qemu-aarch64/pmos/run/disk.qcow2`, kept between runs unless
`OMARCHY_PMOS_FRESH=1`). Press `Ctrl-a`, then `x` to quit. `OMARCHY_PMOS_MEMORY`
(MiB, default 2048) and `OMARCHY_PMOS_SMP` (default 2) adjust the machine.

`pmos-check.sh` always uses TCG, a fresh overlay, `-no-reboot`, a bounded
timeout (`OMARCHY_PMOS_CHECK_TIMEOUT`, default 600 s) and captures serial to
`check/serial.log`. It passes only when QEMU exits cleanly after the guest
printed

```json
{"evidence":"QEMU","event":"product-core-ready","socket":"/run/omarchy/core.sock"}
{"evidence":"QEMU","event":"pmos-boot-health","status":"ok","substrate":"postmarketos","init":"openrc","runlevel":"default","mode":"check"}
{"evidence":"QEMU","event":"pmos-check-poweroff"}
```

and powered itself off (the marker calls `poweroff` in check mode). Failures
are reported as `{"evidence":"QEMU","event":"pmos-check-failed","reason":…}`
with the serial log retained. Observed on this Apple Silicon host (QEMU 11.1.1,
label QEMU): the TCG check passes in about 40 s from launch to `reboot: Power
down`; under HVF `pmos-run` reaches the `omarchy login:` prompt in about 15 s.

### Limitations

- The image boots via direct kernel load, not through its own systemd-boot/UEFI
  path; the bootloader on the `pmOS_boot` partition is present but unexercised.
  The command line is the one boot-deploy wrote to the loader entry (including
  `pmos_root_uuid`), so the initramfs resolves the same root it would under
  systemd-boot.
- postmarketOS ships `quiet splash` on that command line, so the serial log
  carries OpenRC service output and the project records but few kernel
  messages; the kernel version is recorded in `manifest.json`, not on serial.
- The pmbootstrap UI selection remains `console`; Omarchy supplies its own
  compositor/client rather than a postmarketOS desktop package. The product
  graphical startup path is present, but only the core has been exercised by
  the bounded pmOS check so far; framebuffer evidence currently comes from the
  Alpine smoke guest.
- The rootfs is what pmbootstrap installs from the pinned binary channel. The
  project binaries, profile, startup script, and readiness marker are injected
  by mounting the image; native postmarketOS packaging is still future work.
- `pmbootstrap install` output (`build.log`) is the record of what was
  downloaded; the binary repository behind a stable channel can still change,
  so the manifest's package versions, not the channel alone, define the build.
- The pmbootstrap work directory lives in a Docker volume rather than under
  `.cache/omarchy` (see above); `just qemu-clean` does not remove it.
- First build downloads several hundred MB of packages; subsequent builds reuse
  the volume's apk cache and pmaports clone.
