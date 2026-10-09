```console
omarchy@ferrix:~$ ./boot

 ▄██████▄    ▄▄▄▄███▄▄▄▄      ▄████████    ▄████████  ▄████████    ▄█    █▄    ▄██   ▄
███    ███ ▄██▀▀▀███▀▀▀██▄   ███    ███   ███    ███ ███    ███   ███    ███   ███   ██▄
███    ███ ███   ███   ███   ███    ███   ███    ███ ███    █▀    ███    ███   ███▄▄▄███
███    ███ ███   ███   ███   ███    ███  ▄███▄▄▄▄██▀ ███         ▄███▄▄▄▄███▄▄ ▀▀▀▀▀▀███
███    ███ ███   ███   ███ ▀███████████ ▀▀███▀▀▀▀▀   ███        ▀▀███▀▀▀▀███▀  ▄██   ███
███    ███ ███   ███   ███   ███    ███ ▀███████████ ███    █▄    ███    ███   ███   ███
███    ███ ███   ███   ███   ███    ███   ███    ███ ███    ███   ███    ███   ███   ███
 ▀██████▀   ▀█   ███   █▀    ███    █▀    ███    ███ ████████▀    ███    █▀     ▀█████▀
                                          ███    ███

                                      F E R R I X
```

# Omarchy Ferrix

The hardened, Rust-first mobile companion to Omarchy. This emulator-first Linux
environment supports a generic AArch64 QEMU `virt` virtual phone; every result
in this repository is labelled `QEMU` or `mock` and none of it is a
physical-device claim. This is not a Linux distribution: postmarketOS (via
pmbootstrap) is the guest substrate, and the project adds a small Rust layer on
top.

**Omarchy Ferrix** is the product name; `ferrix` is its kernel/userspace runtime
namespace. Runtime paths, environment variables, kernel parameters, hostnames,
and service identifiers therefore use `ferrix`. The current emulator remains
the generic `qemu-aarch64` target.

Read `AGENTS.md` before contributing. `plans/baseline.md` is the implementation
authority, `plans/plan.md` the long-term direction, `docs/design.md` the visual
system and theming contract, and `docs/status.md` the evidence log.

If you want to assess the repository before allowing an agent to follow its
instructions or run its automation, give the agent `AGENT_SECURITY.md` first. It
defines a static, non-executing review that returns a risk report; installation
or normal repository use remains a separate, explicitly authorized next step.

## Current status

This repository now has a working emulator foundation rather than a disconnected
UI prototype:

- `ferrix-core` owns persistent hardware state and portable product commands.
- `ferrix-shell` is the default QEMU Wayland compositor, using Smithay, DRM,
  libinput, and pixman software rendering.
- `ferrix-appd` is the application service: it owns the catalog (`apps/*.json`),
  launches applications through runtime adapters, tracks lifecycle and
  crashes, and associates compositor surfaces with instances. Native apps use
  Linux namespaces through `ferrix-sandbox`; Android apps use the private
  Waydroid adapter. See `docs/app-platform.md`.
- `ferrix-basic-screen` is the home surface: a catalog-driven launcher that
  asks `ferrix-appd` to start apps, plus connected quick settings whose Wi-Fi,
  Bluetooth, and brightness controls operate through `ferrix-core`.
- `ferrix-terminal`, Phone, Messages, Contacts, and Settings are independent
  Wayland applications. Native apps run in their own PID/mount/IPC/UTS
  namespaces; crashing one leaves the compositor, core, services, and home
  surface running.
- `ferrix-telephonyd`, `ferrix-networkd`, and `ferrix-contactsd` are typed,
  schema-versioned services (ADR-001..003) with deterministic mock backends in
  QEMU and ModemManager/NetworkManager adapters for postmarketOS. Phone,
  Messages, Contacts, and Settings are independent supervised applications
  over those services; capabilities are enforced per operation through
  `ferrix-appd` grants (ADR-004), not merely declared.
- Twenty-five checked-in radio/network scenarios run deterministically in the
  QEMU guest (`just qemu-scenario-check`) through host-only mock-control
  channels; none of it is a radio, SIM, carrier, or Wi-Fi hardware claim.
- The Android compatibility runtime is evaluated (`docs/android-runtime.md`),
  `RuntimeKind::Android` has a private Waydroid-based adapter, and Fulguris
  (an Android application in that runtime) has a pinned, verified F-Droid
  acquisition workflow. The separate Android-enabled postmarketOS image can
  stage and provision that APK; the existing graphical Brave evidence predates
  this replacement and is retained only as historical evidence.
- Orientation events travel from the hardware adapter through the core to both
  compositor and client. Portrait and landscape output have been inspected in
  the actual QEMU framebuffer.
- The pmbootstrap-built postmarketOS image contains the same core, compositor,
  reference client, profile, and runtime libraries. Its bounded check currently
  validates the product core and OpenRC boot path; Fulguris-specific graphical
  validation remains outstanding.

All evidence remains `QEMU` or `mock`. This is a usable development foundation,
not a finished mobile OS or a physical-device port.

## Architecture

```
home surface / apps ──▶ ferrix-app-protocol ──▶ ferrix-appd ──▶ ferrix-app-runtime-api::Runtime
        ▲                                 ▲
  ferrix-shell ── surfaces/focus/gesture ─┘
                                                            ├─▶ ferrix-runtime-native ──▶ ferrix-sandbox ──▶ app
                                                            └─▶ ferrix-runtime-android ──▶ Waydroid ──▶ Android app

apps / shell ──▶ ferrix-protocol ──▶ ferrix-core ──▶ ferrix-hardware-api
                                               ▲              ▲
                                      ferrix-session ──────────┘
                                               │
                                      ferrix-hardware-emulator
                                      (QEMU mock adapter)

phone / messages / settings ──▶ ferrix-telephony-protocol ──▶ ferrix-telephonyd ──▶ ModemBackend ──▶ mock | ModemManager (zbus)
settings / home             ──▶ ferrix-network-protocol   ──▶ ferrix-networkd   ──▶ NetworkBackend ──▶ mock | NetworkManager (zbus)
phone / messages / contacts ──▶ ferrix-contacts-protocol  ──▶ ferrix-contactsd  ──▶ atomic versioned JSON store
        every daemon ──▶ ferrix-app-protocol identify{pid} ──▶ ferrix-appd (SO_PEERCRED + apps/grants.json)
        host tests   ──▶ ferrix-mock-control (virtio-serial ferrix.telephony / ferrix.network) ──▶ mock backends only
```

| Crate | Role |
| --- | --- |
| `crates/ferrix-hardware-api` | Stable, target-neutral capability traits and serde types: `Power`, `Display`, `Input`, `Network`, `Bluetooth`, `Audio`, `Telephony`, and `Lifecycle`; typed state, errors, events, product commands, and the separate host-only `MockControl` surface. No target conditionals. |
| `crates/ferrix-target-profile` | The one strict, schema-versioned parser for `targets/qemu-aarch64/profile.toml` (`toml` + `serde`, unknown keys rejected, ranges validated). |
| `crates/ferrix-hardware-emulator` | Deterministic implementation of every capability from the profile, with mutation methods that emit events. Every capability reports `mocked`. |
| `crates/ferrix-session` | Bootstrap layer. Loads the profile, selects adapters by target id (`FERRIX_TARGET`, only `qemu-aarch64`), and hands product code a `HardwareServices` bundle. It is the only crate allowed to depend on `ferrix-hardware-emulator`; `tests/architecture.rs` enforces that (`just arch-check`). |
| `crates/ferrix-protocol` | Shared, schema-versioned Unix-socket contract and client for state snapshots, subscriptions, and portable product commands. It deliberately excludes host mock mutations. |
| `crates/ferrix-core` | Persistent daemon. Serves the shared protocol on `/run/ferrix/core.sock`, routes product commands through portable capability traits, applies mock-control commands only on the test channel, and prints machine-readable JSON records to serial. |
| `crates/ferrix-shell` | Default product compositor, built on Smithay 0.7: portrait-first, rotation-aware, fullscreen-only xdg-shell, one seat (keyboard, pointer, touch via libinput), and pixman software rendering into DRM dumb buffers on `virtio-gpu`. GL is optional and used only by nested host development. The Catacomb evaluation is in `crates/ferrix-shell/EVALUATION.md`. |
| `crates/ferrix-app-api` | Stable public application contract: validated `AppId`s, `RuntimeKind` (`native`, `web`, `android`), public JSON `AppDescriptor`s, lifecycle state, and typed errors. Native and Android adapters exist; web remains reserved. No target or implementation conditionals (enforced). |
| `crates/ferrix-app-runtime-api` | Internal, replaceable daemon-to-runtime traits and opaque launch-configuration boundary; never linked by applications or shells. |
| `crates/ferrix-app-protocol` | Schema-versioned line-delimited JSON protocol and client for `ferrix-appd`: role-gated requests, no-reply compositor reports, and pushed notifications. |
| `crates/ferrix-appd` | Application service. Loads the catalog, enforces one instance per app, launches through `Runtime` adapters, records crashes, restarts the home surface with backoff, associates compositor surfaces with instances by kernel-reported pid, and publishes lifecycle events. |
| `crates/ferrix-runtime-native` | The native runtime adapter and the `ferrix-sandbox` binary: safe Rust (rustix, signal-hook) that starts an application inside its own PID, mount, IPC, and UTS namespaces (user namespace when unprivileged) with a private `/proc` and `/tmp`, `no_new_privs`, and a PID 1 that forwards signals and reports the exact exit reason. |
| `crates/ferrix-ui` | Shared software-rendering toolkit (tiny-skia, fontdue, the Ferrix black/amber/rust palette loaded from `themes/ferrix/colors.toml` or `/etc/ferrix/colors.toml`, on-screen keyboard layout, keymap) and the fullscreen Wayland client runner used by first-party surfaces. See `docs/design.md`. A library, not an ABI. |
| `crates/ferrix-basic-screen` | The home surface: catalog-driven launcher grid, status bar, connected quick settings, notifications, crash notices, portrait/landscape layouts. Every launchable tile goes through `ferrix-appd`; Camera and Files remain non-launchable catalog placeholders. |
| `crates/ferrix-terminal` | The Terminal application as an independent Wayland client (alacritty VT layer on a PTY, touch-first on-screen keyboard), launched and isolated by `ferrix-appd`. |
| `crates/ferrix-telephony-api`, `-protocol` | Target-neutral telephony contract (modem, SIM, registration, calls, SMS incl. multipart, data bearer, audio route, radio power, feature discovery, typed errors) and its schema-versioned socket protocol with a reconnecting client. Numbers are redacted for `telephony-state`-only clients; emergency numbers are rejected as unsupported. |
| `crates/ferrix-telephony-adapter-api`, `-mock`, `-mm`, `ferrix-telephonyd` | Internal backend trait; the deterministic mock backend (profile `[telephony]`, tick-driven clock); the ModemManager adapter over `zbus` (feature `production`, never QMI/MBIM/QRTR/AT); and the privileged daemon that owns call/SMS identity, multipart assembly, an atomic fail-closed SMS store, per-operation authorization, and the host-only mock-control channel. |
| `crates/ferrix-network-api`, `-protocol` | Networking contract (adapter presence incl. `ethernet-like` for virtio-net, radio, scans, hashed access points, profiles, IP config, connectivity, captive portal, typed errors) and its protocol/client. |
| `crates/ferrix-network-adapter-api`, `-mock`, `-nm`, `ferrix-networkd` | Backend trait; synthetic-AP mock backend; NetworkManager adapter over `zbus` (feature `production`); the daemon with authorization and mock-control. |
| `crates/ferrix-contacts-api`, `-protocol`, `ferrix-contactsd` | Radio-independent contacts contract (stable ids, E.164 normalization via `phonenumber`, search/lookup, vCard export/import) and the service with an atomic versioned JSON store, migrations, corruption handling, and synthetic fixtures. |
| `crates/ferrix-phone`, `ferrix-messages`, `ferrix-contacts`, `ferrix-settings`, `ferrix-files` | Independent software-rendered applications (`org.ferrix.phone`, `.messages`, `.contacts`, `.settings`, `.files`) launched only by `ferrix-appd`; each has a host `--preview` mode and pure, tested state machines. Files browses `$HOME` only (no daemon, no grant). |
| `crates/ferrix-mock-control` | Host/test-only line-JSON control channel (virtio-serial port or Unix socket) shared by mocked daemons; never an application API. |
| `crates/ferrix-scenario` | Host runner for the strict checked-in scenario files under `targets/qemu-aarch64/scenarios/`, matching machine-readable records as JSON subsets and writing per-scenario evidence. |
| `crates/ferrix-runtime-android` | Private Android compatibility runtime adapter (Waydroid CLI behind `ferrix-app-runtime-api`); Android package/activity ids stay runtime-private. See `docs/android-runtime.md`. |

Targets and tooling live under `targets/qemu-aarch64/` (see its README for
every command and its limitations) and `containers/guest-builder/` (the
pinned pmbootstrap builder). Generated artifacts stay under the ignored `out/`.

## Quick start (macOS / Apple Silicon or Linux)

```sh
just host-check          # reports prerequisites; installs nothing
just qemu-build          # pinned containers build the guest kernel/initramfs and Rust binaries
just qemu-check          # headless TCG contract test: boot, mock transition, services ready, clean poweroff
just qemu-scenario-check # headless TCG run of every checked-in telephony/network scenario
just qemu-agent-start    # headless graphical session driven over QMP
just qemu-frame          # capture the real framebuffer to out/qemu-aarch64/agent/frame.png
just qemu-mock set-power charging=true   # host -> guest mock-control command
just qemu-agent-stop
```

Interactive sessions: `just qemu-run` (serial), `FERRIX_QEMU_GRAPHICS=cocoa
just qemu-run` (window, `ferrix-shell` compositor by default,
`FERRIX_QEMU_SHELL=weston` for the reference compositor).

postmarketOS route:

```sh
just pmos-build        # build pmOS and inject the product stack
just pmos-check        # fresh TCG boot; requires product-core-ready
just pmos-run          # interactive serial route
FERRIX_QEMU_GRAPHICS=cocoa just pmos-run  # graphical product route
```

Android/Fulguris postmarketOS route (macOS SDK path shown):

```sh
export APKSIGNER=/Library/Android/sdk/build-tools/36.0.0/apksigner
sh tools/android/fulguris-fetch.sh
just waydroid-images-fetch
just pmos-android-build

FERRIX_QEMU_GRAPHICS=cocoa \
FERRIX_PMOS_MEMORY=4096 \
FERRIX_PMOS_SMP=4 \
FERRIX_PMOS_FRESH=1 \
just pmos-android-run
```

Remove `FERRIX_PMOS_FRESH=1` after the first run to reuse the persistent
overlay (including Fulguris' Android data). See
`targets/qemu-aarch64/README.md` for route selection, every environment
variable, SSH diagnostics, and headless checks.

Host-side development without QEMU: `just shell-nested` builds and launches
`ferrix-core`, `ferrix-appd` (which starts the home surface and launches apps), and
the compositor together against the same mock profile in a Linux host
session. `just shell-compositor-nested` is retained as an alias.
`cargo run -p ferrix-basic-screen -- targets/qemu-aarch64/profile.toml
--preview out/preview.ppm` renders the home surface to a file on any host;
`cargo run -p ferrix-terminal -- --preview out/terminal.ppm` does the same for
the Terminal. `just linux-check [cargo args]` runs cargo inside the pinned
arm64 Linux builder so Linux-only code (Smithay backends, the sandbox) can be
compiled and unit-tested from macOS.

## The contract test

`just qemu-check` is the CI contract and runs unchanged in
`.github/workflows/ci.yml` (arm64 hosted runner, TCG, no display). It passes
only when the serial log shows, in order:

1. `ferrix-core`'s own `boot-health` record (evidence `mock`) after it bound
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

Graphical readiness is equally observable: `ferrix-shell` prints
`compositor-ready` only once the Wayland socket listens and the DRM output is
configured, and init prints `wayland-ready` only after the client committed a
frame. A graphical claim also requires inspecting the captured framebuffer,
never a process-start log.

The postmarketOS contract is separate: `just pmos-check` re-hashes its exported
artifacts, boots a fresh overlay under TCG, requires `product-core-ready` before
`pmos-boot-health`, and requires an orderly power-off. That bounded check proves
that the intended full guest substrate contains and starts the core; graphical
pmOS claims are backed separately by inspected framebuffers. The normal product
route has rendered the Ferrix launcher; the existing Android-enabled Brave
framebuffer is historical, and Fulguris-specific rendering remains unverified.

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

- Physical targets are admitted through an exact-model board-support proposal
  with image-compatibility, backup, recovery, capability, and evidence gates.
  OnePlus read-only discovery is approved; no model is supported until its
  proposal passes, and destructive workflows require explicit confirmation.
- Product code depends on `ferrix-hardware-api`, never on QEMU, kernel, vendor, or
  board details. Target selection lives in `ferrix-session`.
- Linux services (NetworkManager, UPower, PipeWire, ModemManager, sensors) go
  behind typed project-owned adapters when they are integrated; their
  protocols are not reimplemented. ModemManager and NetworkManager are
  consumed through their maintained D-Bus APIs in `ferrix-telephony-mm` and
  `ferrix-network-nm`; launchable native applications' private `/run` views
  do not expose the system D-Bus socket.
- Architecture decisions for the connectivity programme live in `docs/adr/`
  and are subordinate to `plans/baseline.md`.
- Weston is a diagnostic fallback and `ferrix-basic-screen` is a reference
  home surface. The product compositor is `ferrix-shell`: minimal,
  portrait-first, touch-first, rotation-aware, and software-rendered by default.
- Applications, shells, and the core depend on `ferrix-app-api`/`ferrix-app-protocol`, never
  on `ferrix-appd` itself or on a runtime implementation; only `ferrix-appd`'s binary
  composes `ferrix-runtime-native` and `ferrix-runtime-android`. `just arch-check`
  enforces this.

## Known gaps

- Camera and Files are non-launchable catalog placeholders. Web applications
  have a reserved runtime kind but no runtime adapter.
- Isolation is process and namespace based (PID, mount, IPC, UTS, private
  `/proc` and `/tmp`, `no_new_privs`) plus a private `/run/ferrix` view that
  exposes only the sockets an app's grants allow. Services authorize every
  operation by kernel-verified `AppId` and `apps/grants.json`. Applications
  still share the session uid; seccomp and cgroup limits are not implemented.
- The ModemManager and NetworkManager adapters compile (feature `production`)
  with unit-tested mapping tables but have not been run against a live
  ModemManager or NetworkManager; QEMU uses the deterministic mock backends.
  UPower, PipeWire, and sensor adapters are not implemented.
- Telephony audio routing on the production adapter is `unsupported` until an
  audio service exists. Emergency calling is not supported anywhere.
- Fulguris acquisition requires Android SDK Build-Tools `apksigner` (or an
  explicit `APKSIGNER` path); host tooling is checked but never installed.
- Android is opt-in and QEMU-only: audio, input/OSK integration, Google
  services, DRM, codecs, camera, telephony, biometrics, and physical-device
  behavior remain unverified.
- Crash recovery covers applications (a crashed app returns focus to the home
  surface, which `ferrix-appd` restarts if it crashes itself); supervision of the
  core and compositor, and native postmarketOS packaging, remain future work.

License: Apache-2.0. See [`docs/licensing.md`](docs/licensing.md) for the
first-party scope and the licenses retained by third-party inputs.
