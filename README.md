# Ferrum

An emulator-first, Rust-first mobile Linux environment. The only supported
target is a generic AArch64 QEMU `virt` virtual phone; every result in this
repository is labelled `QEMU` or `mock` and none of it is a physical-device
claim. This is not a Linux distribution: postmarketOS (via pmbootstrap) is the
guest substrate, and the project adds a small Rust layer on top.

The naming model is deliberate: **Ferrum** is the product and ship name,
**Ferrix** is the kernel/userspace codename and runtime namespace, and
**Gossan** is reserved for a future device image. Three names, one metal.
Runtime paths, environment variables, kernel parameters, hostnames, and service
identifiers therefore use `ferrix`. The current emulator remains the generic
`qemu-aarch64` target and does not use the Gossan name.

Read `AGENTS.md` before contributing. `docs/baseline.md` is the implementation
authority and `docs/plan.md` the long-term direction.

## Current status

This repository now has a working emulator foundation rather than a disconnected
UI prototype:

- `mobile-core` owns persistent hardware state and portable product commands.
- `mobile-shell` is the default QEMU Wayland compositor, using Smithay, DRM,
  libinput, and pixman software rendering.
- `appd` is the application service: it owns the catalog (`apps/*.json`),
  launches every application as its own supervised process inside Linux
  namespaces through `ferrix-sandbox`, tracks lifecycle and crashes, and
  associates compositor surfaces with instances. See `docs/app-platform.md`.
- `mobile-basic-screen` is the home surface: a catalog-driven launcher that
  asks `appd` to start apps, plus connected quick settings whose Wi-Fi,
  Bluetooth, and brightness controls operate through `mobile-core`.
- `ferrum-terminal` is the first independent application: a separate Wayland
  client in its own PID/mount/IPC/UTS namespaces. Crashing it leaves the
  compositor, core, service, and home surface running.
- Orientation events travel from the hardware adapter through the core to both
  compositor and client. Portrait and landscape output have been inspected in
  the actual QEMU framebuffer.
- The pmbootstrap-built postmarketOS image contains the same core, compositor,
  reference client, profile, and runtime libraries. Its bounded check currently
  validates the product core and OpenRC boot path; its graphical route still
  needs separate framebuffer validation.

All evidence remains `QEMU` or `mock`. This is a usable development foundation,
not a finished mobile OS or a physical-device port.

## Architecture

```
home surface / apps ──▶ app-protocol ──▶ appd ──▶ app-runtime-api::Runtime ──▶ runtime-native ──▶ ferrix-sandbox ──▶ app
        ▲                                 ▲
  mobile-shell ── surfaces/focus/gesture ─┘

apps / shell ──▶ mobile-protocol ──▶ mobile-core ──▶ hardware-api
                                               ▲              ▲
                                      mobile-session ──────────┘
                                               │
                                      hardware-emulator
                                      (QEMU mock adapter)
```

| Crate | Role |
| --- | --- |
| `crates/hardware-api` | Stable, target-neutral capability traits and serde types: `Power`, `Display`, `Input`, `Network`, `Bluetooth`, `Audio`, `Telephony`, and `Lifecycle`; typed state, errors, events, product commands, and the separate host-only `MockControl` surface. No target conditionals. |
| `crates/target-profile` | The one strict, schema-versioned parser for `targets/qemu-aarch64/profile.toml` (`toml` + `serde`, unknown keys rejected, ranges validated). |
| `crates/hardware-emulator` | Deterministic implementation of every capability from the profile, with mutation methods that emit events. Every capability reports `mocked`. |
| `crates/mobile-session` | Bootstrap layer. Loads the profile, selects adapters by target id (`FERRIX_TARGET`, only `qemu-aarch64`), and hands product code a `HardwareServices` bundle. It is the only crate allowed to depend on `hardware-emulator`; `tests/architecture.rs` enforces that (`just arch-check`). |
| `crates/mobile-protocol` | Shared, schema-versioned Unix-socket contract and client for state snapshots, subscriptions, and portable product commands. It deliberately excludes host mock mutations. |
| `crates/mobile-core` | Persistent daemon. Serves the shared protocol on `/run/ferrix/core.sock`, routes product commands through portable capability traits, applies mock-control commands only on the test channel, and prints machine-readable JSON records to serial. |
| `crates/mobile-shell` | Default product compositor, built on Smithay 0.7: portrait-first, rotation-aware, fullscreen-only xdg-shell, one seat (keyboard, pointer, touch via libinput), and pixman software rendering into DRM dumb buffers on `virtio-gpu`. GL is optional and used only by nested host development. The Catacomb evaluation is in `crates/mobile-shell/EVALUATION.md`. |
| `crates/app-api` | Stable public application contract: validated `AppId`s, `RuntimeKind` (`native`; `web` and `android` reserved), public JSON `AppDescriptor`s, lifecycle state, and typed errors. No target or implementation conditionals (enforced). |
| `crates/app-runtime-api` | Internal, replaceable daemon-to-runtime traits and opaque launch-configuration boundary; never linked by applications or shells. |
| `crates/app-protocol` | Schema-versioned line-delimited JSON protocol and client for `appd`: role-gated requests, no-reply compositor reports, and pushed notifications. |
| `crates/appd` | Application service. Loads the catalog, enforces one instance per app, launches through `Runtime` adapters, records crashes, restarts the home surface with backoff, associates compositor surfaces with instances by kernel-reported pid, and publishes lifecycle events. |
| `crates/runtime-native` | The native runtime adapter and the `ferrix-sandbox` binary: safe Rust (rustix, signal-hook) that starts an application inside its own PID, mount, IPC, and UTS namespaces (user namespace when unprivileged) with a private `/proc` and `/tmp`, `no_new_privs`, and a PID 1 that forwards signals and reports the exact exit reason. |
| `crates/ferrum-ui` | Shared software-rendering toolkit (tiny-skia, fontdue, Tokyo Night theme, on-screen keyboard layout, keymap) and the fullscreen Wayland client runner used by first-party surfaces. A library, not an ABI. |
| `crates/mobile-basic-screen` | The home surface: catalog-driven launcher grid, status bar, connected quick settings, notifications, crash notices, portrait/landscape layouts. Launchable tiles go through `appd`; reference entries keep illustrative in-process pages. |
| `crates/ferrum-terminal` | The Terminal application as an independent Wayland client (alacritty VT layer on a PTY, touch-first on-screen keyboard), launched and isolated by `appd`. |

Targets and tooling live under `targets/qemu-aarch64/` (see its README for
every command and its limitations) and `containers/guest-builder/` (the
pinned pmbootstrap builder). Generated artifacts stay under the ignored `out/`.

## Quick start (macOS / Apple Silicon or Linux)

```sh
just host-check          # reports prerequisites; installs nothing
just qemu-build          # pinned containers build the guest kernel/initramfs and Rust binaries
just qemu-check          # headless TCG contract test: boot, mock transition, clean poweroff
just qemu-agent-start    # headless graphical session driven over QMP
just qemu-frame          # capture the real framebuffer to out/qemu-aarch64/agent/frame.png
just qemu-mock set-power charging=true   # host -> guest mock-control command
just qemu-agent-stop
```

Interactive sessions: `just qemu-run` (serial), `FERRIX_QEMU_GRAPHICS=cocoa
just qemu-run` (window, `mobile-shell` compositor by default,
`FERRIX_QEMU_SHELL=weston` for the reference compositor).

postmarketOS route:

```sh
just pmos-build        # build pmOS and inject the product stack
just pmos-check        # fresh TCG boot; requires product-core-ready
just pmos-run          # interactive serial route
FERRIX_QEMU_GRAPHICS=cocoa just pmos-run  # graphical route (not yet validated here)
```

Host-side development without QEMU: `just shell-nested` builds and launches
`mobile-core`, `appd` (which starts the home surface and launches apps), and
the compositor together against the same mock profile in a Linux host
session. `just shell-compositor-nested` is retained as an alias.
`cargo run -p mobile-basic-screen -- targets/qemu-aarch64/profile.toml
--preview out/preview.ppm` renders the home surface to a file on any host;
`cargo run -p ferrum-terminal -- --preview out/terminal.ppm` does the same for
the Terminal. `just linux-check [cargo args]` runs cargo inside the pinned
arm64 Linux builder so Linux-only code (Smithay backends, the sandbox) can be
compiled and unit-tested from macOS.

## The contract test

`just qemu-check` is the CI contract and runs unchanged in
`.github/workflows/ci.yml` (arm64 hosted runner, TCG, no display). It passes
only when the serial log shows, in order:

1. `mobile-core`'s own `boot-health` record (evidence `mock`) after it bound
   its socket;
2. init's `boot-health` (evidence `QEMU`), which init may only print after the
   core reported;
3. `mock-control` ready on the virtio-serial port `ferrix.control`;
4. no `charging:true` before the host sends `set-power charging=true` over
   that real channel;
5. the `power-changed` event the core emitted in response;
6. a `shutdown` command accepted and a clean QEMU exit.

A missing core, a core that exits, or a core that never binds produces a
`boot-failed` record and a failing check. There is no fallback that prints
success.

Graphical readiness is equally observable: `mobile-shell` prints
`compositor-ready` only once the Wayland socket listens and the DRM output is
configured, and init prints `wayland-ready` only after the client committed a
frame. A graphical claim also requires inspecting the captured framebuffer,
never a process-start log.

The postmarketOS contract is separate: `just pmos-check` re-hashes its exported
artifacts, boots a fresh overlay under TCG, requires `product-core-ready` before
`pmos-boot-health`, and requires an orderly power-off. This proves that the
intended full guest substrate contains and starts the core; it does not yet
constitute graphical pmOS validation.

## Validation before handoff

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
just arch-check
just qemu-check
```

For graphical changes, also run the agent session, capture frames, and inspect
them. Record evidence and limitations honestly, labelled `QEMU` or `mock`.

## Boundaries

- No phones are inspected or operated. No board targets, Android boot tooling,
  or Solana Seeker support exist or are claimed without a separately approved
  board-support proposal.
- Product code depends on `hardware-api`, never on QEMU, kernel, vendor, or
  board details. Target selection lives in `mobile-session`.
- Linux services (NetworkManager, UPower, PipeWire, ModemManager, sensors) go
  behind typed project-owned adapters when they are integrated; their
  protocols are not reimplemented.
- Weston is a diagnostic fallback and `mobile-basic-screen` is a reference
  home surface. The product compositor is `mobile-shell`: minimal,
  portrait-first, touch-first, rotation-aware, and software-rendered by default.
- Applications, shells, and the core depend on `app-api`/`app-protocol`, never
  on `appd` itself or on a runtime implementation; only `appd`'s binary
  composes `runtime-native`. `just arch-check` enforces this.

## Known gaps

- The postmarketOS graphical route is installed but still needs captured and
  inspected framebuffer evidence.
- App pages beyond the Terminal and system controls are still illustrative
  reference pages inside the home surface.
- Isolation is process and namespace based (PID, mount, IPC, UTS, private
  `/proc` and `/tmp`, `no_new_privs`). Applications still run as the session
  user with access to the shared filesystem and sockets; portals, permission
  enforcement, seccomp, and cgroup limits are not implemented yet.
- Production adapters for NetworkManager, UPower, PipeWire, ModemManager, and
  sensors are not implemented yet; QEMU uses deterministic mock adapters.
- Crash recovery covers applications (a crashed app returns focus to the home
  surface, which `appd` restarts if it crashes itself); supervision of the
  core and compositor, and native postmarketOS packaging, remain future work.

License: MIT OR Apache-2.0.
