# Generic AArch64 QEMU virtual phone

This is a **QEMU/mock** target, never a physical-device claim. It uses QEMU `virt`, a serial console, and the versioned mock profile in `profile.toml`.
It runs Omarchy Ferrix's `ferrix` kernel/userspace runtime surface. The emulator and its
artifacts retain the technical `qemu-aarch64` identity; Gossan is reserved for
a future device image.

## Install (macOS / Apple Silicon)

Install the host tools once:

```sh
xcode-select --install
brew install qemu just cpio
brew install --cask orbstack
rustup target add aarch64-unknown-linux-gnu
```

Open OrbStack, or run `orb`, and wait until its container engine is running.
Then, from the repository root, verify the environment:

```sh
just host-check
```

It should report QEMU, the Docker-compatible container CLI, the Rust AArch64
target, and `cpio` as available. When OrbStack is selected on macOS, the
active Docker context should be `orbstack`; another Docker-compatible engine
may expose a different context. The host-side AArch64 linker is optional for
this initial container-built guest.

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
`ferrix-core` daemon's `boot-health` record and init's `QEMU`-labelled
`boot-health` that may only follow it, then for the `telephonyd-ready` and
`networkd-ready` records of the two mock-backed service daemons (ADR-006),
asserts the mock is still unplugged,
sends `set-power charging=true` over the real host-to-guest mock-control
channel, waits for the `power-changed` event the core emits, then sends
`shutdown` and requires a clean QEMU exit. Every failure is a machine-readable
`check-failed` record; there is no fallback that prints success. Inspect the
captured serial output at `out/qemu-aarch64/serial.log`.
`FERRIX_QEMU_CHECK_TIMEOUT` (seconds, default 90) bounds the whole check.
`FERRIX_QEMU_REQUIRE_SERVICES=0` drops the two daemon assertions (the
`check-passed` record then says `"services":"optional"`); it exists for
integration work while a daemon is still a skeleton and is not the CI setting.

### Guest services and the mock-control channel

The guest init starts `ferrix-core` (crate `crates/ferrix-core`) as a
persistent daemon before anything else. It bootstraps through
`ferrix-session`, which parses `profile.toml` with the shared strict
`ferrix-target-profile` parser and selects the `ferrix-hardware-emulator` adapter set for
target id `qemu-aarch64`; product code (`ferrix-core`, `ferrix-basic-screen`,
`ferrix-shell`) depends only on `ferrix-hardware-api`, and `just arch-check` fails
if that boundary is crossed. The daemon serves typed state and an event
stream on `/run/ferrix/core.sock` (line-delimited JSON: `{"request":"state"}`,
`{"request":"subscribe"}`, and typed product `{"request":"command",...}`
requests) and prints every event as a JSON record on the
serial console. If the binary is missing, exits, or never binds its socket,
init prints `{"evidence":"QEMU","event":"boot-failed","reason":...}` and, in
check mode, powers off.

Every QEMU session also attaches a `virtio-serial` port named
`ferrix.control`, exposed on the host as a Unix socket
(`out/qemu-aarch64/control.sock`, or `agent/control.sock` for agent sessions;
`FERRIX_QEMU_CONTROL_SOCKET` overrides). `ferrix-core` reads line-delimited
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

Bluetooth discovery and pairing on the mock controller (devices come from
`[[bluetooth.discoverable]]` in `profile.toml`, each with a fixed pairing
method and outcome; ids are `mock-bt-found-N` in profile order). The
`bluetooth-*` verbs are the same product requests Settings sends;
`bluetooth-tick`, `bluetooth-incoming`, and `set-bluetooth-pairing-outcome`
are mock-only. The
discovery bound (30 s), remote pairing steps (1 s, 3 s typing, 30 s silent
remote), and the 30 s prompt timeout advance only on `bluetooth-tick`:

```sh
just qemu-mock bluetooth-discovery active=true      # scan; ends after bluetooth-tick ms=30000
just qemu-mock bluetooth-pair device=mock-bt-found-0  # headphones: passkey prompt in Settings
just qemu-mock bluetooth-reply accept=true          # or tap Pair in Settings
just qemu-mock bluetooth-tick ms=1000               # the remote finishes the bond
just qemu-mock bluetooth-disconnect device=mock-bt-found-0
just qemu-mock bluetooth-forget device=mock-bt-found-0
just qemu-mock set-bluetooth-pairing-outcome outcome=rejected   # success|rejected|timeout; omit to reset
just qemu-mock bluetooth-cancel                     # cancel, or dismiss a finished outcome
just qemu-mock bluetooth-pair device=mock-bt-found-4  # legacy headset: PIN card in Settings
just qemu-mock bluetooth-reply-pin pin=0000          # or type it on the card (profile pin; others are rejected)
just qemu-mock bluetooth-incoming device=mock-bt-found-2  # a remote pairs to us: Settings is raised over any app
```

`bluetooth-incoming` models a remote-initiated request: the session marks it
`incoming`, `ferrix-core` opens the system `show-bluetooth-pairing` intent
through `ferrix-appd` (record `bluetooth-attention`), and Settings is launched
or raised over the foreground app with the request's card. The PIN is never
echoed: `mock-command` records name the command only.

Invalid or unknown commands are rejected with `"status":"error"`; they never
succeed silently. The client is `scripts/mock_control.py` (python3 standard
library only). This drives *mock* state in the virtual phone and is never a
product interface or a physical-device control.

### Telephony and network services in the guest

After the core's `boot-health`, init starts three service daemons (ADR-001,
002, 003), each under a small shell supervisor (`supervise NAME CMD...` in
`initramfs/init`: restart on non-zero exit with 1/2/4/8 s backoff and a
`daemon-restarted {name, exit, backoff_s}` record; a missing binary is a
`service-missing {name}` record, never a boot failure):

| Daemon | Command line in the smoke guest | Socket | Ready record |
| --- | --- | --- | --- |
| `ferrix-telephonyd` | `--backend mock --profile /etc/ferrix/profile.toml --mock-control auto --appd /run/ferrix/ferrix-appd.sock --data-dir /var/lib/ferrix/telephony` | `/run/ferrix/telephonyd.sock` | `telephonyd-ready` |
| `ferrix-networkd` | `--backend mock --profile /etc/ferrix/profile.toml --mock-control auto --appd /run/ferrix/ferrix-appd.sock` | `/run/ferrix/networkd.sock` | `networkd-ready` |
| `ferrix-contactsd` | `--data-dir /var/lib/ferrix/contacts --appd /run/ferrix/ferrix-appd.sock [--fixtures /usr/share/ferrix/contacts/mock-contacts.json]` | `/run/ferrix/contactsd.sock` | `contactsd-ready` |

Start order and identity: the daemons start **before** `ferrix-appd` and
resolve client identity (ADR-004) lazily at a client's first request, so
`ferrix-appd` may appear later. Headless sessions (`qemu-check`, `-nographic`)
then start `ferrix-appd --no-autostart` as the identity authority and grants
holder (`/usr/share/ferrix/apps/grants.json`, staged from `apps/grants.json`)
without a home surface; graphical sessions start it once, after the
compositor, with autostart on, exactly as before. The mock backends take their
boot state from the profile's `[telephony]` and `[network]` sections (below).
Init records `services-ready {telephony, network, contacts, persistent:false}`
once the three sockets exist; `boot-health` keeps its core-only meaning.
The daemons write their JSON records straight to the serial console.

`/var/lib/ferrix/{telephony,contacts}` are on the initramfs tmpfs: stores
survive a daemon restart, not a guest reboot. Persistence across boots is a
property of the postmarketOS route, where the same directories live on the
real root filesystem (`product-services-ready` reports `"persistent":true`).

The guest binaries added by `qemu-build` are `ferrix-telephonyd`,
`ferrix-networkd`, `ferrix-contactsd`, `ferrix-phone`, `ferrix-messages`,
`ferrix-contacts`, `ferrix-settings`, and `ferrix-files` (hashes `ferrix_<name>_sha256` in
`manifest.json`, plus `grants_sha256`, `contacts_fixtures_sha256` and the
`mock_control_ports` list).

#### Profile sections `[telephony]` and `[network]`

`profile.toml` (schema 1, strict) carries two optional sections parsed by
`ferrix-target-profile` (`Profile::telephony_section()` /
`network_section()` return them or derive them from `[connectivity]` for an
older profile). Every value is synthetic emulator configuration:

```toml
[telephony]
modem = "ready"            # absent | disabled | initializing | ready | failed
sim = "ready"              # absent | locked | blocked | ready
sim_pin = "0000"           # synthetic; only meaningful when sim = "locked"
operator = "Ferrix Mock"
access = "lte"             # none | gsm | umts | lte | nr
signal_bars = 3            # 0..=4
roaming = false
features = ["voice", "sms", "multipart-sms", "delivery-reports", "dtmf", "hold", "audio-route", "sim-pin", "radio-power"]

[network]
adapter = "present"        # absent | present
radio = "enabled"          # enabled | disabled
connected = "ferrix-lab"   # ssid of the AP joined at boot, or ""; must be listed below
[[network.access_points]]
ssid = "ferrix-lab"
security = "wpa2-psk"      # open | wep | wpa-psk | wpa2-psk | wpa3-sae | enterprise
signal_percent = 78
passphrase = "mock-passphrase"
```

Unknown keys, unknown enum values, `signal_bars > 4`, `signal_percent > 100`,
duplicate or blank SSIDs, and `connected` naming an unlisted SSID are hard
errors.

### Mock-control channels

Each mocked daemon owns one host-only channel (ADR-006); there is no
cross-daemon forwarding and no application API for mutations. `qemu-run.sh`
attaches all three virtio-serial ports to every session:

| Daemon | QEMU virtio-serial port | Host socket (`out/qemu-aarch64/[agent/]`) | Nested host path |
| --- | --- | --- | --- |
| `ferrix-core` | `ferrix.control` | `control.sock` | `--control none` (or `--control PATH`) |
| `ferrix-telephonyd` | `ferrix.telephony` | `telephony-control.sock` | `--mock-control out/qemu-aarch64/telephony-control.sock` |
| `ferrix-networkd` | `ferrix.network` | `network-control.sock` | `--mock-control out/qemu-aarch64/network-control.sock` |

`FERRIX_QEMU_TELEPHONY_SOCKET` / `FERRIX_QEMU_NETWORK_SOCKET` override the
host paths (Unix socket paths must stay short: macOS limits them to about
100 bytes). The wire form is the same everywhere: one JSON command per line
in, one `{"status":"ok"|"error",...}` ack per line out; unknown commands and
unknown fields are rejected. Drive them from the host:

```sh
just qemu-mock --service telephony incoming-call number=+15550100042
just qemu-mock --service telephony remote-hangup
just qemu-mock --service telephony set-sim state=locked attempts_left=3
just qemu-mock --service network set-radio enabled=false
just qemu-mock --service network set-connectivity connectivity=portal url=http://portal.ferrix.invalid/login
just qemu-mock --service telephony '{"cmd":"tick","ms":5000}'
just qemu-mock set-power charging=true          # --service core is the default
```

Telephony commands: `state`, `set-modem {state}`, `set-sim {state,
attempts_left?}`, `set-registration {registration, roaming?, operator?,
access?}`, `set-signal {bars}`, `incoming-call {number}`, `remote-answer`,
`remote-hangup`, `remote-busy`, `remote-reject`, `outgoing-timeout`,
`incoming-sms {number, body}`, `incoming-sms-multipart {number, ref_id,
parts}`, `incoming-sms-part {number, ref_id, index (1-based), count, text}`,
`deliver-sms`, `fail-sms {reason}`, `set-outgoing-result {connected | busy |
rejected | timeout | no-service}`, `set-sms-result {sent | failed |
delayed-delivery}`, `set-radio {power}`, `set-audio-available {routes}`,
`tick {ms}`, `crash-backend`. Network commands: `state`, `set-adapter
{present}`, `set-radio {enabled}`, `set-access-points {access_points}`,
`scan-result {outcome, reason?}`, `set-connect-result {connected |
auth-failed | dhcp-failed | dns-failed | timeout}`, `set-connectivity
{connectivity, url?}`, `disconnect {reason}`, `set-ip {...}`, `tick {ms}`,
`crash-backend`. Time inside the mocks advances only on `tick`, never on the
wall clock. Numbers in commands are synthetic (`+1555010xxxx`); the daemons'
records carry ids and states only.

### Scenarios

`targets/qemu-aarch64/scenarios/<name>.toml` are deterministic scripts over
the mock-control channels, run by the host binary `ferrix-scenario`
(crate `crates/ferrix-scenario`). Schema (strict; unknown keys, unknown
services, `schema_version != 1`, a command without `cmd`, a zero timeout, or
a datetime are errors; the file stem must equal `name`):

```toml
schema_version = 1
name = "incoming-call-answer-route-remote-hangup"
description = "..."

[[step]]
service = "telephony"                    # telephony | network | core
command = { cmd = "incoming-call", number = "+15550100042" }
expect = [ { event = "telephony-event", event_name = "call-changed", state = "ringing" } ]
timeout_ms = 5000                        # bound only; default 5000; never asserted
ack = { status = "ok" }                  # optional JSON subset of the ack
expect_ack_error = false                 # true when the command must be rejected
note = "free text carried into result.json"
```

Each step sends the command, requires a `"status":"ok"` ack (or an error when
`expect_ack_error = true`, or the `ack` subset), then waits at most
`timeout_ms` for every `expect` entry, **in order**, among records appended
*after* the command was sent. Matching is a JSON subset: expected keys must be
present and equal (nested objects recurse, arrays match element-wise and may
be longer in the record, a number never equals a string), so extra fields in
a record are fine and field names live only in the scenario files. Mock state
persists across scenarios within one session, so every checked-in scenario
starts with setup steps that restore the profile boot state.

```sh
just qemu-scenario-list                                     # names
just qemu-scenario-validate                                 # schema only, no QEMU
just qemu-agent-start                                       # or qemu-run in another terminal
just qemu-scenario incoming-call-answer-route-remote-hangup # one scenario, guest mode
just qemu-scenario wifi-captive --frame                     # plus a framebuffer capture
just qemu-scenario-check                                    # all of them, fresh headless TCG session
```

Guest mode (default when `out/qemu-aarch64/agent/serial.log` exists, else
the plain session directory) sends to `control.sock`, `telephony-control.sock`
and `network-control.sock` and matches the serial log; evidence is `QEMU`.
Nested mode (`ferrix-scenario --nested --log PATH --sockets DIR FILE...`)
sends to the daemons' Unix sockets and matches their combined stdout log;
evidence is `mock`. The runner prints one record per scenario,
`{"evidence":"QEMU"|"mock","event":"scenario-passed"|"scenario-failed","name":...,"step":N,"reason":...}`,
and writes `out/qemu-aarch64/scenarios/<name>/result.json` (per-step acks,
matched records, status), `log-excerpt.log` (every log line from the first
command to the end), and `frame.png` with `--frame` (through `scripts/qmp.py`
on the agent session's QMP socket). Exit status 0/1/2 = all passed / some
failed / setup error.

`just qemu-scenario-check` requires `just qemu-build` artifacts, compiles
only the host runner, validates every file, starts a headless TCG agent
session (`FERRIX_QEMU_GRAPHICS=none`), waits for `telephonyd-ready` and
`networkd-ready`, runs every checked-in scenario, stops the session, and
emits `{"evidence":"QEMU","event":"scenario-check-passed"|"scenario-check-failed","passed":N,"failed":M,"failed_names":[...],"seconds":S}`.
`FERRIX_QEMU_SCENARIO_TIMEOUT` (default 600 s) bounds the whole run;
`FERRIX_QEMU_SCENARIO_FRAME=1` captures a framebuffer per scenario. It needs
no network. CI runs it as the third job (`qemu-scenario-check`) on pushes
and manual dispatch, and on pull requests only when the PR carries the
`scenarios` label; `qemu-check` itself is unchanged in cost.

The checked-in set is ADR-006's required list (25 files) plus five Bluetooth
files: `no-modem`,
`modem-init-failure-recovery`, `sim-absent`, `sim-pin-locked-unlock`,
`sim-ready`, `registration-search-deny-register-roam-loss-recover`,
`incoming-call-answer-route-remote-hangup`, `outgoing-call-connected`,
`outgoing-call-busy`, `outgoing-call-rejected`, `outgoing-call-timeout`,
`outgoing-call-no-service`, `call-interrupted-by-modem-loss`,
`sms-incoming-single`, `sms-incoming-multipart`, `sms-outgoing-success`,
`sms-outgoing-failed`, `sms-delayed-delivery-report`, `radios-disabled`,
`wifi-disabled-scan-connect`, `wifi-auth-failed`, `wifi-dhcp-failed`,
`wifi-captive`, `wifi-offline-recovered`, `service-crash-restart-reconcile`,
plus the Bluetooth set on the core channel: `bluetooth-discovery-bounded`,
`bluetooth-pair-confirm-success`, `bluetooth-pair-failures` (declined,
rejected, prompt and remote timeouts, a declined PIN request, one attempt at
a time), `bluetooth-pair-pin` (legacy PIN: plain accept and malformed PIN
refused, wrong PIN rejected, profile PIN bonds), and
`bluetooth-incoming-request` (remote-initiated request raises Settings
through `ferrix-appd`; an unanswered one is declined after 30 s).
`ferrix-core`'s unit tests replay the same files in-process (`mock`; the
`app-intent` and `bluetooth-attention` records exist only in the guest).
Scenarios that need a *local* action (dial, answer, hangup, send an SMS,
enter a PIN, change the audio route, scan, connect) express it as a
mock-control command of that name (`note = "client-side ... emulated over
mock-control"`), because the runner only speaks the host channels and no
protocol client can reach `/run/ferrix/*.sock` from the host; until a mock
accepts that command the scenario stops with an honest `ack-error: unknown
variant` at that step.

**Limitation (daemon process restart):** `service-crash-restart-reconcile`
exercises backend loss and reconciliation *inside* each daemon
(`crash-backend` → `telephony-backend-lost` / `network-backend-lost`, tick-driven
recovery, `service-restarted` snapshot). It does not kill a daemon process.
Process restart is handled by init's `supervise` loop in the guest
(`daemon-restarted` record), but there is no host-to-guest path to kill a
guest process from CI (typing a shell command over the virtual keyboard into
the Terminal app is not acceptable evidence), so process restart is
demonstrated only nested on a Linux host and is not asserted by any QEMU
scenario.

### Alpine smoke guest stays offline (decision)

The smoke guest has no NIC on purpose (ADR-006). Its job is the bounded,
offline CI contract: boot, core, mock daemons, one deterministic transition,
clean poweroff. Real IP connectivity (DHCP, DNS, TLS, browser evidence)
belongs to the postmarketOS route, which already has user-mode `virtio-net`
as `ethernet-like` transport; no QEMU route may claim 802.11 scanning,
association, or radio behavior. Adding a NIC here would widen the CI surface
without a fit reason, so `qemu-check` and `qemu-scenario-check` need no
network and never assert wall-clock timing or DNS.

`ferrix-basic-screen` subscribes to the core instead of reading power,
network, or modem state from the profile itself, redraws on each event, and
prints a `ui-state` record, so a transition can be proven end to end:
adapter → core → UI → captured framebuffer (the charging indicator switches
to the `ok` status color with a bolt glyph in both the status bar and the
battery card).
Its quick-settings controls send portable product commands through the state
socket; they do not mutate UI-only state or expose the host mock protocol.

To open the experimental QEMU Cocoa window and start the guest Wayland compositor, run:

```sh
FERRIX_QEMU_GRAPHICS=cocoa just qemu-run
```

The guest compositor is `ferrix-shell` (crate `crates/ferrix-shell`), the Rust
product compositor built on Smithay 0.7: portrait, fullscreen-only xdg-shell
toplevels, one `wl_seat` (keyboard + pointer + touch from libinput), wlr-layer-shell
reserved for status/launcher chrome, and software rendering by default (pixman
into DRM dumb buffers on `virtio-gpu`; no GL is required for correctness). The
window is QEMU's virtual display. Weston remains selectable as the reference
compositor:

```sh
FERRIX_QEMU_SHELL=weston FERRIX_QEMU_GRAPHICS=cocoa just qemu-run
FERRIX_QEMU_SHELL=ferrix-shell just qemu-agent-start   # the default
```

The selection travels to the guest as `ferrix.shell=` on the kernel command
line; the guest init defaults to `ferrix-shell`. Readiness stays observable:
`ferrix-shell` prints `compositor-ready` only once the Wayland socket is
listening and the DRM output is configured, `client-mapped` when a toplevel
commits its first buffer, and `compositor-failed` (non-zero exit) on errors;
init then starts the application service `ferrix-appd` and prints `wayland-ready`
only after `ferrix-appd-ready` (socket bound, catalog loaded) and the home surface's
committed `basic-screen-frame` both appeared in `/run/ferrix-appd.log`, otherwise
`wayland-failed` with a reason (`ferrix-appd-missing`, `ferrix-appd-exited`,
`ferrix-appd-not-ready`, `home-not-ready`, ...) and the logs. The Catacomb evaluation
that led to the Smithay decision is `crates/ferrix-shell/EVALUATION.md`.

### Application service, namespaces, and crash isolation

`ferrix-appd` (crate `crates/ferrix-appd`) is the only process that starts applications. It
loads one JSON descriptor per app from `/usr/share/ferrix/apps/` (checked in
under `apps/`, schema in `crates/ferrix-app-api`), auto-starts the home surface
(`org.ferrix.home`, `ferrix-basic-screen`), and serves a schema-versioned
line-delimited JSON protocol on `/run/ferrix/ferrix-appd.sock` (`crates/ferrix-app-protocol`).
The home surface asks it to launch (`{"request":"launch","id":"org.ferrix.terminal"}`);
the compositor reports surfaces and focus to it by kernel-reported client pid
and obeys its `activate` notifications; applications may only yield the
foreground. Roles are declared at `hello` and gate what each connection may do.
For every app, `ferrix-appd` keeps the runtime supervisor pid separate from the
mapped Wayland client pid. A launch or activation that arrives before the first
surface is retained on that app instance, then sent to the compositor when
`SurfaceMapped` supplies the client pid. Reopening an app, Home, and foreground
yield therefore raise the app's real toplevel; they never try to raise the
sandbox supervisor. This is the common lifecycle path for future native and
runtime-backed apps, not a Terminal-specific exception. A toplevel is raised
when it maps for the first time or when an explicit activation names it;
subsequent buffer commits from background apps (for example Home's clock tick)
update their content without changing fullscreen focus or stack order.

Every native application runs as `ferrix-appd -> ferrix-sandbox -> init -> app`. The
sandbox (crate `crates/ferrix-runtime-native`, safe Rust on `rustix`/`signal-hook`)
unshares PID, mount, IPC, and UTS namespaces (plus a user namespace when it is
not root), gives the app a private `/proc` and tmpfs `/tmp`, sets the UTS
hostname to the app id, sets `no_new_privs`, and runs a tiny PID 1 that
forwards signals, reaps zombies, and exits with the app's exact status. A crash
therefore kills one process tree only: `ferrix-shell` refocuses the home
surface and records `client-gone`, `ferrix-appd` records `app-crashed` and publishes
the lifecycle event, and the home surface shows a crash notice. If the home
surface itself crashes, `ferrix-appd` restarts it with backoff (`home-restarting`).

Records to look for on serial: `ferrix-appd-ready`, `app-launching`, `app-started`
(with the isolation report), `sandbox-ready` (per-namespace proof from
`/proc/self/ns`), `app-surface`, `app-foreground`/`app-background`,
`app-exited`/`app-crashed`, `home-gesture`, `client-activated`, `client-gone`,
`home-launch`, `home-crash-notice`. `docs/app-platform.md` is the contract.

Deterministic crash demonstration from the host (agent session):

```sh
just qemu-tap 180 1236                 # Terminal tile: ferrix-appd launches ferrix-terminal
just qemu-frame                        # Terminal surface, prompt root@org:~#
just qemu-type 'kill -SEGV $PPID'      # the shell's parent is the Terminal app
just qemu-key ret                      # app dies with SIGSEGV inside its namespaces
just qemu-frame                        # home surface again, with the crash notice
just qemu-tap 180 1236                 # relaunch: a fresh instance, new pid
just qemu-swipe 540 2290 540 1900      # compositor home gesture; Terminal keeps running
```

Signals sent to PID 1 of a namespace from inside it are ignored unless a
handler exists, which is why the demo signals the Terminal (PID 2 in its
namespace, child of the sandbox init) rather than the init.

`ferrix-shell` runs nested on a Linux host session from the same mock profile
(`just shell-nested`, with `just shell-compositor-nested` retained as an alias;
both start `ferrix-core` plus the compositor, `mock` evidence, GLES through winit); the
crate's policy modules (geometry, rotation, focus stack, evidence records) have
unit tests that run on every host, and `--print-config` resolves the geometry
without a display. Live orientation comes from `ferrix-core` and reconfigures
the output and connected clients; `F9` remains a compositor-only debug hook.

The host window is sized to fit the desktop. `qemu-run.sh` reads the host's
visible screen height and backing scale (macOS, via `osascript`), scales the
profile's portrait panel to that height in physical pixels (aspect ratio
preserved, never upscaled), and passes the result to the guest as
`ferrix.display=WxH`. `ferrix-shell` reads that value and pins its DRM mode to
it; for the Weston fallback the guest init generates `/etc/ferrix/weston.ini`
(connector `Virtual-1`, idle blanking off) with the same mode. Pinning matters
because QEMU's Cocoa UI otherwise pushes its own window size into virtio-gpu and
the guest surface ends up at an arbitrary size. On a 1920x1080-point Retina
desktop this opens a fixed 415x900-point window showing an 830x1800 guest panel
at 1:1 host pixels. Set `FERRIX_QEMU_WINDOW_HEIGHT` (logical points) to
override the automatic height; QEMU's View menu "Zoom To Fit" makes the window
resizable. Headless sessions (`-display none`) keep the full profile panel so
framebuffer evidence stays at the mock geometry, and `just qemu-tap` coordinates
remain profile pixels in both cases. The guest reports the chosen geometry as a
`display-geometry` record on serial. The GTK backend path is unverified on this
macOS host.

The first visible surface is `ferrix-basic-screen`, the home surface: a fullscreen Rust Wayland client (crate `crates/ferrix-basic-screen`, built on the shared `crates/ferrix-ui` toolkit: `smithay-client-toolkit`, `tiny-skia`, `fontdue`) that software-renders the Omarchy Ferrix launcher — the black/amber/rust palette and type rules in `docs/design.md`, a status bar following GrapheneOS conventions, a grid of tiles from the `ferrix-appd` catalog, and the gesture pill — with battery, network, and display state from `ferrix-core`. It is a reference home surface, not the final compositor-owned shell chrome. Preview it on any host without QEMU (tiles come from `apps/*.json` when no `ferrix-appd` is reachable):

```sh
cargo run -p ferrix-basic-screen -- targets/qemu-aarch64/profile.toml --preview out/preview.ppm
```

### Unified theming (`colors.toml`)

The compositor and every first-party surface load one palette file at process
start through `ferrix_ui::theme` (format and tokens: `docs/design.md`). Both
guest routes carry the checked-in `themes/ferrix/colors.toml` as
`/etc/ferrix/colors.toml`: `qemu-build` stages it next to `profile.toml` and
the initramfs init copies it into place; the pmOS finalize step installs the
same product copy into the rootfs. `FERRIX_QEMU_COLORS=<path>` substitutes
another file at build time. At runtime the lookup order is `FERRIX_COLORS=<path>`,
then `/etc/ferrix/colors.toml`, then the built-in palette (identical to the
checked-in file). Each process reports what it loaded; `ferrix-shell`'s record
reaches serial through its log replay, the surfaces' through the `ferrix-appd`
log:

```json
{"evidence":"QEMU","event":"theme-loaded","source":"/etc/ferrix/colors.toml"}
```

A file that is present but rejected produces `theme-invalid` (with `source`
and `error`) first and the process continues on the built-in palette
(`"source":"builtin"`). `ferrix-shell` paints the theme `bg` (`#000000`) as
the clear color under every client, so a framebuffer capture taken before a
client maps shows that color exactly when the product compositor is the one
scanning out. `just shell-nested` exports `FERRIX_COLORS` pointing at the
repository file, so the nested `mock` session reports `theme-loaded` with that
path; set it to another `colors.toml` to preview a different palette on the
host.

`qemu-build` compiles the guest binaries for aarch64 musl inside a pinned `rust` container image; its digest, the exact Rust channel from `rust-toolchain.toml`, the binary hashes (`ferrix_basic_screen_sha256`, `ferrix_core_sha256`, `ferrix_shell_sha256`, `ferrix_appd_sha256`, `ferrix_sandbox_sha256`, `ferrix_terminal_sha256`, `ferrix_telephonyd_sha256`, `ferrix_networkd_sha256`, `ferrix_contactsd_sha256`, `ferrix_phone_sha256`, `ferrix_messages_sha256`, `ferrix_contacts_sha256`, `ferrix_settings_sha256`), the catalog hash (`apps_sha256`) and grants hash (`grants_sha256`), the contacts fixtures hash (`contacts_fixtures_sha256`, empty when the fixture file is absent), and the theme file hash (`colors_sha256`, with its source path as `colors`) are recorded in `out/qemu-aarch64/manifest.json`. `ferrix-shell` is linked dynamically against the guest's Alpine musl libraries (libinput, libudev, libseat, libdrm, pixman, libxkbcommon, libgbm, libgcc_s), which `qemu-build` installs into the initramfs. The initramfs is first written to a temporary sibling, checked with `gzip -t`, and atomically moved into place, so an interrupted or corrupt archive cannot replace the last usable guest artifact or receive a manifest hash.

On Apple Silicon, `qemu-run` selects HVF automatically; `qemu-check` always uses portable TCG. This smoke-test image uses the pinned Alpine arm64 builder image and its generic `linux-virt` kernel; the postmarketOS/pmbootstrap guest (below) is the full mobile-image route.
The smoke runner defaults to `FERRIX_QEMU_MEMORY=2048` MiB so the current
software-rendered initramfs can unpack under TCG; set that variable explicitly
when testing a constrained host.

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

`qemu-agent-start` returns only after four serial records: `wayland-ready`
(compositor and home surface committed), `input-ready` for the pointer
(seat announced), `cursor-ready` (a neutral QMP pointer motion reached the
compositor), and `home-ready {tiles, catalog, width, height}` (the launcher's
catalog is loaded and hit-testable). This prevents the first `just qemu-tap`
from being consumed while the pointer is initialized. The pmOS graphical start
waits for `home-ready` as well and prints `product-home-not-ready` when it never
arrives.
`qemu-frame` produces `out/qemu-aarch64/agent/frame.png`: the rendered QEMU
framebuffer. Inspect that exact PNG after the guest has reported
`wayland-ready` in `out/qemu-aarch64/agent/serial.log`. A running QEMU
process by itself is not graphical readiness evidence. The command emits a
machine-readable QEMU record and fails if the framebuffer is entirely blank,
while retaining the PNG for diagnosis.

The agent session is headless by default (`-display none`) and uses portable TCG
unless `FERRIX_QEMU_ACCEL` is explicitly set, so it remains useful on hosts
without a display server. Headless sessions use QEMU's own daemon mode, so the
guest remains alive after `qemu-agent-start` returns and later `qemu-frame`,
`qemu-tap`, and `qemu-swipe` invocations reach the same VM. To simultaneously
open a local QEMU window, choose a host backend:

```sh
FERRIX_QEMU_GRAPHICS=cocoa just qemu-agent-start
```

For deterministic input, coordinates are portrait framebuffer pixels from
`profile.toml`; they are converted to QEMU's absolute pointer range:

```sh
just qemu-tap 540 1170
just qemu-swipe 540 2250 540 1800
just qemu-key esc
```

Run the complete navigation regression against a fresh TCG guest with:

```sh
just qemu-gesture-check
```

It verifies that a normal bottom swipe backgrounds Messages without terminating
it, inward swipes from both side edges yield to Home, and a long bottom swipe
terminates Messages before capturing the final Home framebuffer under
`out/qemu-aarch64/gesture-check/`.

The interactive reference surface accepts pointer and Wayland touch input. Tap
any launcher tile to open its QEMU/mock app view, tap the status bar for quick
settings, drag the brightness rail, swipe down from the top for notifications,
swipe inward from either side edge for Back/yield, swipe up from the bottom
for Home/minimise, or make a long bottom-to-top swipe to close the foreground app.
Virtual-tablet input displays a translucent accent-colored touch ring with a
center dot marking the exact hotspot; native Wayland touch contacts do not show a cursor.
These interactions demonstrate the guest input path; this client remains a
reference surface rather than the final product compositor or mobile shell.

### Terminal app

The Terminal tile asks `ferrix-appd` to launch `ferrix-terminal` (crate
`crates/ferrix-terminal`, app id `org.ferrix.terminal`), an independent
Wayland client in its own Linux namespaces: it spawns BusyBox `/bin/sh` on a
PTY and renders it through `alacritty_terminal` (Alacritty's VT parser and
grid, Apache-2.0) with the shared `ferrix-ui` software renderer. Because the
sandbox sets the full UTS hostname to `org.ferrix.terminal`; BusyBox's `\h`
prompt displays its short form as `root@org:~#`. `cat /proc/1/comm` shows
`ferrix-sandbox`: the app sees only its own process tree. Input comes from the
touch keyboard drawn below the grid (letters, `?123` symbols, shift within
symbols for the remaining punctuation, plus an `esc tab ctrl ` \ | ← ↓ ↑ →`
accessory row; shift and ctrl are sticky for one key) or from the virtual
keyboard QEMU attaches as `virtio-keyboard-pci`, so typing in the Cocoa
window works as well. Dragging inside the grid scrolls history. The session
keeps running while you go Home (compositor bottom-edge gesture) and come
back (tap the tile again: `ferrix-appd` activates the running instance instead of
starting a second one); the header arrow or an inward side-edge swipe yields the foreground to the home
surface. When the shell exits, tapping Enter restarts it. Serial evidence
records are `app-context`, `terminal-frame`, `terminal-spawned`,
`terminal-ready` (first line the shell wrote), `keyboard-input`, and
`terminal-exited`.

Opening Terminal must leave the Terminal framebuffer visible and the PTY alive;
returning immediately to the launcher without `terminal-exited` is an activation
failure, not an application exit. In a healthy run, serial includes
`app-surface` for the Terminal's Wayland client followed by `client-activated`
with that same client pid and `raised:true`. The `app-started` pid is the sandbox
supervisor and is intentionally different.

Drive it deterministically from the host:

```sh
just qemu-tap 180 1236            # open the Terminal tile (first tile: launchable apps sort first)
just qemu-type 'uname -a'         # virtual keyboard, US layout; \n sends Enter
just qemu-key ret
just qemu-frame                   # inspect out/qemu-aarch64/agent/frame.png
```

Touch-keyboard taps use the same coordinates the client hit-tests; the unit
tests in `crates/ferrix-ui/src/osk.rs` pin the layout. Preview the app on any
host without QEMU (canned transcript, no PTY):

```sh
cargo run -p ferrix-terminal -- --preview out/terminal.ppm
```

The physical-keyboard path is a small hand-rolled US evdev table because the
static musl guest binary does not link libxkbcommon; the touch keyboard is the
primary phone input.

### Files app

The Files tile (third launcher tile in the portrait profile, after Browser
and Contacts) asks `ferrix-appd` to launch `ferrix-files` (crate
`crates/ferrix-files`, app id `org.ferrix.files`). It browses `$HOME` only:
`/root` on this smoke guest, `/home/user` on the pmOS route and the Pixel 9a
rootfs. Folders sort first, rows show size or item count and the
modification date, the breadcrumb chips jump back up, a vertical drag in the
list scrolls by whole rows, a tap on a file opens it read-only, and the
trailing `···` of a row opens its details (Open, Rename, Delete with a
confirmation). New folder and Rename use the shared touch keyboard. A
Contacts **Export vCard** lands in `~/Documents/contacts.vcf`, and Files picks
it up at the next minute tick or on Refresh. It needs no daemon and no
capability grant. See *Socket visibility* in `docs/app-platform.md` for why
`$HOME` stays writable inside the sandbox and how confinement is enforced.
Serial records: `app-context`, `files-frame`, `ui-state {page, state}`,
`files-action {action, status, error}`, `files-back`. They carry depths,
counts, and error codes, never file names.

```sh
just qemu-tap 540 1236            # Contacts tile; then tap Export vCard
just qemu-swipe 540 2250 540 1800 # home
just qemu-tap 876 1236            # Files tile
just qemu-frame
```

Host preview of every page (synthetic home in a scratch folder, `mock`):

```sh
cargo run -p ferrix-files -- --preview out/files.ppm --page view
```

Inspect session state with `just qemu-agent-status`, and end it cleanly with
`just qemu-agent-stop`. Agent artifacts are:

```text
out/qemu-aarch64/agent/qmp.sock      # local QMP control socket
out/qemu-aarch64/agent/control.sock            # ferrix-core mock-control (just qemu-mock)
out/qemu-aarch64/agent/telephony-control.sock  # ferrix-telephonyd mock-control (--service telephony)
out/qemu-aarch64/agent/network-control.sock    # ferrix-networkd mock-control (--service network)
out/qemu-aarch64/scenarios/<name>/             # ferrix-scenario evidence (result.json, log excerpt, frame)
out/qemu-aarch64/agent/serial.log    # mandatory serial evidence
out/qemu-aarch64/agent/frame.png     # latest rendered framebuffer
out/qemu-aarch64/agent/launch.log    # resolved QEMU command and host errors
```

Run only one session per output directory: `qemu-build`, the check, and the
agent session all share `out/qemu-aarch64/`, so a concurrent rebuild
truncates the initramfs under a booting guest and two guests would fight over
the same control socket. Set `FERRIX_OUT_DIR` to isolate parallel work.

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

This is the intended **full guest substrate** from `plans/baseline.md`; the
Alpine initramfs above stays the explicitly labelled smoke-test route. It is a
**QEMU** target only: generic AArch64 `virt`, never a physical device. The
postmarketOS UI package remains `console` (OpenRC), while the image also
contains the Ferrix compositor, home surface, services, and applications. They
start when graphics are enabled; the bounded check remains serial-only.

```sh
just pmos-build    # pinned guest-builder container runs pmbootstrap install
just pmos-check    # bounded, headless, TCG: readiness record + clean poweroff
just pmos-run      # interactive serial console (HVF on Apple Silicon)
```

Prerequisites are those of `just host-check`: OrbStack running on macOS (or a
Docker-compatible engine on Linux), arm64 containers, `qemu-system-aarch64`
and `qemu-img` from Homebrew's `qemu`, and `python3`. Nothing is installed on
the host; pmbootstrap only ever runs inside the container.

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
`--privileged` is the one container-engine flag that provides that set; no host
PID or network namespace and no extra mounts are used. The four mounts are the
pmbootstrap work directory (rw, keeps chroots, the apk cache and the pmaports
clone between runs), the checked-in `targets/qemu-aarch64/pmos/` inputs (ro),
the product binaries/profile produced by `qemu-build` (ro), and the output
directory (rw).

The work directory is a named container volume (`FERRIX_PMOS_WORK_VOLUME`,
default `ferrix-pmbootstrap-work`; remove it with `docker volume rm`), not the
`${FERRIX_CACHE_DIR:-.cache/ferrix}/pmbootstrap` path the rest of the cache
uses. This is required on macOS because host bind mounts may not preserve the
ownership, suid `su`/`sudo`/BusyBox, and device-node semantics that pmbootstrap
chroots need; a bind-mounted work directory would silently corrupt the guest.
The entrypoint probes the three semantics on the mounted work directory and
refuses to continue (`guest-builder-failed`, reason `work-fs-*`) when they are
missing; Linux hosts with a real filesystem can set `FERRIX_PMOS_WORK_BIND=1`
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
`targets/qemu-aarch64/pmos/local.d/ferrix-boot-health.start` as
`/etc/local.d/ferrix-boot-health.start` (OpenRC's `local` service is the last
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

The test password (`FERRIX_PMOS_PASSWORD`, default `147147`, user `user`) is
handled in plain text by pmbootstrap and is only for this emulator image.

### Outputs and manifest

Everything is generated under the ignored `out/qemu-aarch64/pmos/`
(`FERRIX_OUT_DIR` relocates `out/`):

```text
out/qemu-aarch64/pmos/vmlinuz            # kernel from the pmOS boot partition
out/qemu-aarch64/pmos/initramfs          # postmarketos-mkinitfs initramfs
out/qemu-aarch64/pmos/qemu-aarch64.img   # raw disk image (pmOS_boot + pmOS_root)
out/qemu-aarch64/pmos/cmdline.txt        # kernel command line boot-deploy generated
out/qemu-aarch64/pmos/manifest.json      # pins, versions, hashes
out/qemu-aarch64/pmos/build.log          # full pmbootstrap output
out/qemu-aarch64/pmos/builder-image.log  # container image build output
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
paths and hashes; hashes of the product binaries and of the installed
`/etc/ferrix/colors.toml` (`colors_sha256`); the
project git revision (`-dirty` when the tree has changes) and hashes of
`pins.env`, `pmbootstrap.cfg` and the marker; and `sha256` plus size of every
exported artifact. `pmos-check` re-hashes `vmlinuz`, `initramfs` and the disk
image against the manifest before booting.

### Running and checking

`pmos-run.sh` boots with `-kernel`/`-initrd` from the exported files, the
recorded command line plus `panic=-1 ferrix.mode=<run|check>`, the disk as
`virtio-blk`, user-mode networking (`FERRIX_PMOS_SSH_PORT=2222` adds an SSH
host forward) and `-nographic` by default, so the serial console is always
present. Set `FERRIX_QEMU_GRAPHICS=cocoa` (or another QEMU display backend)
to attach virtio GPU/input devices and start `ferrix-shell` plus
`ferrix-basic-screen`. The
pristine image is never written: QEMU boots a qcow2 overlay
(`out/qemu-aarch64/pmos/run/disk.qcow2`, kept between runs unless
`FERRIX_PMOS_FRESH=1`). The overlay is paired with the base-image hash from
`manifest.json`; after `pmos-build` changes the raw image, `pmos-run`
automatically replaces the now-incompatible overlay instead of trying to boot
it with the new image's filesystem UUIDs. Press `Ctrl-a`, then `x` to quit. `FERRIX_PMOS_MEMORY`
(MiB, default 2048) and `FERRIX_PMOS_SMP` (default 2) adjust the machine.

`pmos-check.sh` always uses TCG, a fresh overlay, `-no-reboot`, a bounded
timeout (`FERRIX_PMOS_CHECK_TIMEOUT`, default 600 s) and captures serial to
`check/serial.log`. It passes only when QEMU exits cleanly after the guest
printed

```json
{"evidence":"QEMU","event":"product-core-ready","socket":"/run/ferrix/core.sock"}
{"evidence":"QEMU","event":"pmos-boot-health","status":"ok","substrate":"postmarketos","init":"openrc","runlevel":"default","mode":"check"}
{"evidence":"QEMU","event":"pmos-check-poweroff"}
```

and powered itself off (the marker calls `poweroff` in check mode). Failures
are reported as `{"evidence":"QEMU","event":"pmos-check-failed","reason":…}`
with the serial log retained. Observed on this Apple Silicon host (QEMU 11.1.1,
label QEMU): the TCG check passes in about 40 s from launch to `reboot: Power
down`; under HVF `pmos-run` reaches the `ferrix login:` prompt in about 15 s.

### pmOS QEMU environment reference

Choose the guest route first; they intentionally produce separate artifacts
and overlays:

| Route | Command | Purpose |
| --- | --- | --- |
| Alpine smoke guest | `just qemu-run` | Small emulator/CI contract guest; not the full substrate and no Waydroid. |
| postmarketOS product | `just pmos-run` | Full product substrate without the Android compatibility packages/images. |
| postmarketOS + Android | `just pmos-android-run` | Separate image with Waydroid images and the pinned Fulguris APK; use this for Fulguris. |

Use these variables with `just pmos-run` or `just pmos-android-run`; they are
read by the checked-in runner, not by an application. The smoke route has its
own similarly named controls documented earlier in this file.

| Variable | Default | Purpose |
| --- | --- | --- |
| `FERRIX_QEMU_GRAPHICS` | `off` | QEMU display backend. On macOS use `cocoa`; `off` is serial-only. |
| `FERRIX_QEMU_ACCEL` | `auto` | `auto` selects `hvf` on macOS and `tcg` elsewhere. |
| `FERRIX_PMOS_MEMORY` | `2048` | Guest RAM in MiB; use `4096` for Android/Waydroid. |
| `FERRIX_PMOS_SMP` | `2` | Guest vCPUs; use `4` for Android where host capacity permits. |
| `FERRIX_PMOS_FRESH` | `0` | Set `1` to discard the persistent overlay and test first boot. This loses overlay changes. |
| `FERRIX_PMOS_SSH_PORT` | unset | Loopback SSH forward; `2222` enables `ssh -p 2222 user@127.0.0.1`. |
| `FERRIX_PMOS_CHECK_TIMEOUT` | `600` | Headless-check timeout in seconds. |
| `FERRIX_QEMU_WINDOW_HEIGHT` | host visible height minus 60 points | Window height in logical points for graphical runs; the guest width is derived from the profile aspect ratio. Falls back to `800` when host metrics are unavailable. |

`APKSIGNER` is a host build/acquisition variable, not a QEMU variable. If
Android SDK Build-Tools is not on `PATH`, set it explicitly on macOS:

```sh
export APKSIGNER=/Library/Android/sdk/build-tools/36.0.0/apksigner
```

Normal pmOS graphical run on Apple Silicon:

```sh
FERRIX_QEMU_GRAPHICS=cocoa just pmos-run
```

Fresh Android first-boot run:

```sh
FERRIX_QEMU_GRAPHICS=cocoa \
FERRIX_PMOS_MEMORY=4096 \
FERRIX_PMOS_SMP=4 \
FERRIX_PMOS_FRESH=1 \
FERRIX_PMOS_SSH_PORT=2222 \
just pmos-android-run
```

The terminal is the serial console and the Cocoa window is the Ferrix display.
The graphical runner preserves the profile's portrait aspect ratio and sizes
the Cocoa window to fit the desktop. Set `FERRIX_QEMU_WINDOW_HEIGHT=720` (in
logical points), for example, to request a smaller development window.
Keep it open; quit QEMU with `Ctrl-a`, then `x`. Omit `FERRIX_PMOS_FRESH=1`
on later runs to retain the guest overlay and Android application data. The
emulator account is `user`, with test-only password `147147` by default.

For deterministic Android diagnostics:

```sh
just pmos-android-check
tail -n 200 out/qemu-aarch64/pmos-android/check/serial.log
```

### Service daemons and backend selection on the pmOS route

`finalize-in-container.sh` installs the same binaries as the smoke guest
(`ferrix-telephonyd`, `ferrix-networkd`, `ferrix-contactsd`, `ferrix-phone`,
`ferrix-messages`, `ferrix-contacts`, `ferrix-settings`, `ferrix-files`, the catalog with
`grants.json`, the contacts fixtures when present) and creates
`/var/lib/ferrix/{telephony,contacts}` on the root filesystem, so stores
persist across guest reboots. `00-ferrix-product.start` starts the daemons
after `product-core-ready`, supervised (`daemon-restarted` records), with
`--mock-control none` (this route attaches no virtio-serial mock ports), and
reports `product-service-started {name, backend}`,
`product-service-missing`, and `product-services-ready {telephony, network,
contacts, telephony_backend, network_backend, persistent:true}`. Headless
boots start `ferrix-appd --no-autostart` as the identity authority.

Backend selection is a bootstrap decision, never an app's: the kernel command
line `ferrix.telephony=mock|modemmanager` and `ferrix.network=mock|networkmanager`
win, then `FERRIX_TELEPHONY_BACKEND` / `FERRIX_NETWORK_BACKEND` from
`/etc/ferrix/backends.env` if that file exists, otherwise **`mock`** — the
default for this milestone. The image's `image-facts.json`/`manifest.json`
record whether `networkmanager` and `modemmanager` are installed
(`networkmanager_installed`, `modemmanager_installed`; the pmOS base image
ships NetworkManager, ModemManager is not requested by `pmbootstrap.cfg`).
Flipping to a production backend requires the daemons built with the
`production` feature, the D-Bus service running on the system bus, and a
recorded `pmos-check` with that command line; until then the flip is wired
but not exercised, and no evidence claims it.

### Opt-in Android compatibility runtime (Waydroid) — graphical QEMU validated

`FERRIX_PMOS_ANDROID=1` (recipes `just waydroid-images-fetch`,
`pmos-android-build`, `pmos-android-run`, `pmos-android-check`) builds a
**separate** image under `out/qemu-aarch64/pmos-android/` from
`pmos/pmbootstrap-android.cfg` (identical to `pmbootstrap.cfg` plus
`waydroid, waydroid-openrc, waydroid-nftrules, nftables, lxc, dnsmasq,
pulseaudio, dbus` from the pinned Alpine community mirror and a 4096 MiB
Android-only rootfs reserve for the pre-seeded images and Waydroid writable
state; inputs from
`docs/android-runtime.md` §6). The default `pmos-build`, `pmos-check`, and
CI never read that file. Steps:

1. Acquire all pinned host inputs, then build. The Android build now refuses
   to proceed if Fulguris is absent or fails its hash/signature/signer check:

   ```sh
   export APKSIGNER=/Library/Android/sdk/build-tools/36.0.0/apksigner
   sh tools/android/fulguris-fetch.sh
   just waydroid-images-fetch
   just pmos-android-build
   ```

   `just waydroid-images-fetch` downloads the pinned LineageOS 20 images
   (`tools/android/waydroid.pins`: channel `waydroid_arm64_only`, build
   20260403, system zip 728,517,282 bytes, vendor zip 77,349,347 bytes, SHA-256
   verified; allow-listed hosts `sourceforge.net` and `*.dl.sourceforge.net`)
   into ignored `out/android/waydroid/<build>/`, unpacks `system.img` /
   `vendor.img`, writes `images.json`, and points `out/android/waydroid/current`
   at the directory. `--verify-only` re-checks without network.
2. `just pmos-android-build` mounts those images read-only into the finalize
   step, which installs them to `/etc/waydroid-extra/images/`, writes
   `/etc/ferrix/waydroid-properties.ini` (`ro.hardware.gralloc=default`,
   `ro.hardware.egl=swiftshader`, `persist.waydroid.multi_windows=false`,
   `ro.sf.lcd_density=400`). The 400 dpi value is scoped to this QEMU
   Android image and gives the 1080-pixel panel an approximately 432 dp
   Pixel-class logical width; it is not read by the default pmOS image or any
   physical-target workflow.
   links `51_waydroid.nft` into `/etc/nftables.d/`, enables `cgroups` and
   `dbus` in the default runlevel, installs
   `/etc/local.d/00-ferrix-android.start`, and stages
   `/usr/share/ferrix/android/fulguris.apk` after requiring
   `out/android/fulguris/<version>/net.slions.fulguris.full.fdroid_261.apk` to
   pass `tools/android/fulguris-verify.sh`. Every
   hash lands in `image-facts.json` / `manifest.json` under `guest.android`
   (`waydroid_version`, image hashes, properties and init-script hashes,
   `fulguris_apk_sha256`, `density_dpi`, `nftables_rule_linked`).
3. At first boot `00-ferrix-android.start` (before `00-ferrix-product.start`)
   records `android-binder-devices {binder, hwbinder, vndbinder, psi}`, runs
   `waydroid init -f -i /etc/waydroid-extra/images` offline (`waydroid-init
   {status}`; the kernel `linux-postmarketos-stable` already provides the
   Binder devices, no kernel change), appends the properties, and runs the
   required offline upgrade to materialize `gralloc=default`,
   `egl=swiftshader`, and the 400 dpi density into `waydroid_base.prop` before
   starting the container.
   This prevents Waydroid from auto-selecting Mesa/GBM merely because QEMU has
   a render node, a combination that made SurfaceFlinger abort with `output
   buffer not gpu writeable`.
4. Once `ferrix-shell` has created the Wayland socket, the product bootstrap
   starts the Waydroid session, waits for Android `sys.boot_completed=1`, and
   installs the staged APK only when `pm path net.slions.fulguris.full.fdroid`
   has no package result. It then assigns `android.app.role.BROWSER` to
   Fulguris as Android's shell UID and verifies that an HTTPS intent resolves
   to Fulguris. It records
   `android-app-provision` with `browser_role:"default"`, as `installed` on
   first boot and `present` on subsequent persistent-overlay boots.
   `ferrix-appd` never installs packages; it launches the installed package
   through its private runtime adapter. An idle Waydroid container's `FROZEN`
   state is healthy and launchable; Waydroid's app command unfreezes it.
5. `just pmos-android-check` additionally requires the
   `android-binder-devices` and `waydroid-init` records and, when Fulguris was
   staged, the `android-app-staged` record. The headless check deliberately
   does not claim installation because no compositor/Wayland session exists.
   `just android-bootstrap-test` exercises offline init, renderer
   materialization, container startup, and staging without QEMU.

Historical Brave evidence (2026-09-21) on Apple Silicon/HVF with QEMU 11.1.1
recorded `waydroid-properties {status:"materialized", renderer:"swiftshader"}`
and `android-app-provision {status:"installed", package:"com.brave.browser",
browser_role:"default"}`. That evidence predates the Fulguris replacement and
does not validate Fulguris.
Launching `org.ferrix.browser` through `ferrix-appd` recorded
`android-app-launched`, `app-started`, and a mapped
`waydroid.com.brave.browser` surface; the Cocoa framebuffer was captured and
inspected showing Brave's first-run UI. After orderly power-off, a run without
`FERRIX_PMOS_FRESH=1` recorded provisioning status `present`, proving the
installation lives in the persistent overlay. This remains historical QEMU
evidence only, not a physical-device claim. A fresh Fulguris QEMU run now
reaches and has inspected its onboarding framebuffer; full Android
interaction remains bounded by the limitations recorded in `docs/status.md`.

### Limitations

- The image boots via direct kernel load, not through its own systemd-boot/UEFI
  path; the bootloader on the `pmOS_boot` partition is present but unexercised.
  The command line is the one boot-deploy wrote to the loader entry (including
  `pmos_root_uuid`), so the initramfs resolves the same root it would under
  systemd-boot.
- postmarketOS ships `quiet splash` on that command line, so the serial log
  carries OpenRC service output and the project records but few kernel
  messages; the kernel version is recorded in `manifest.json`, not on serial.
- The pmbootstrap UI selection remains `console`; Omarchy Ferrix supplies its own
  compositor/client rather than a postmarketOS desktop package. The bounded
  pmOS check remains serial-only, but separate graphical QEMU runs have
  framebuffer-validated the normal Ferrix launcher. The Android-enabled Brave
  capture is historical; a fresh Fulguris onboarding capture is recorded under
  `out/qemu-aarch64/pmos-android/`. Those captures do not broaden the
  headless contract or establish physical-device support.
- The rootfs is what pmbootstrap installs from the pinned binary channel. The
  project binaries, profile, theme file, startup script, and readiness marker are injected
  by mounting the image; native postmarketOS packaging is still future work.
- `pmbootstrap install` output (`build.log`) is the record of what was
  downloaded; the binary repository behind a stable channel can still change,
  so the manifest's package versions, not the channel alone, define the build.
- The pmbootstrap work directory lives in a named container volume rather than under
  `.cache/ferrix` (see above); `just qemu-clean` does not remove it.
- First build downloads several hundred MB of packages; subsequent builds reuse
  the volume's apk cache and pmaports clone.
