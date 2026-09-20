# Application platform

This document records the application contract implemented in the workspace:
how applications are identified, described, launched, isolated, and tracked,
and which process owns which state. It applies the bundled
`ferrum-mobile-architecture` skill (phases 1 and 2 of its implementation path)
to the current crates. `docs/baseline.md` remains the implementation
authority; this document does not change the target, the guest substrate, or
the hardware boundary. Evidence is QEMU/mock only; nothing here is a
physical-device claim.

## Decision

Applications are independent, supervised Linux processes. One application
service (`appd`) owns the catalog, activation, foreground/background state,
crash records, and surface association. The compositor (`mobile-shell`) owns
surfaces, focus, and the trusted global home gesture. The home surface
(`mobile-basic-screen`) is one application among others: it queries `appd`
for the catalog and asks `appd` to launch; it never forks commands or scans
packages.

Every native application runs inside its own Linux namespaces through a small
safe-Rust sandbox (`ferrix-sandbox`). A crash therefore ends exactly one
process tree: the compositor keeps running, focus returns to the home surface,
`appd` records the crash and publishes it, and the home surface shows a
low-noise notice. The home surface itself is restarted by `appd` with backoff.

## Layers and ownership

```text
  home surface           Terminal app            future apps
  (mobile-basic-screen)  (ferrum-terminal)       (native | web | android)
        |  Launch/Subscribe        |  YieldForeground
        +-------------+------------+-----------------------------+
                      |  app-protocol (line-delimited JSON, schema v1)
                    appd  <--- SurfaceMapped / FocusChanged / HomeGesture --- mobile-shell
                      |   --- Activate{pid} ------------------------------->  (compositor)
            app_runtime_api::Runtime (internal trait)
                      |
              runtime-native ---> ferrix-sandbox ---> init (PID 1 of the app's PID ns) ---> app
```

| Crate | Owns | Trust ring |
| --- | --- | --- |
| `app-api` | public `AppId`, `RuntimeKind`, `AppDescriptor`, `LifecycleState`, `ExitReason`, `AppInstance`, `AppEvent`, `AppError`, and launch environment constants | contract |
| `app-protocol` | `Request`/`Report`/`Reply`/`Notification` wire types, blocking client, subscription reader | contract |
| `app-runtime-api` | internal `Runtime`/`RunningApp` traits, launch context, and opaque runtime configuration boundary | internal contract |
| `appd` | catalog, single-instance policy, lifecycle state machine, crash records, home restart policy, surface association, role gating | system |
| `runtime-native` | native launch configuration and isolation types; the native `Runtime` that spawns `ferrix-sandbox`, classifies exits, and terminates | internal adapter |
| `mobile-shell` | toplevels keyed by client pid, focus, `Activate` handling, bottom-edge home gesture, crash refocus | system |
| `mobile-basic-screen` | catalog-driven launcher, in-process quick settings/notifications, illustrative reference pages, crash notice | privileged first-party |
| `ferrum-terminal` | the Terminal as an independent Wayland client | untrusted application |
| `ferrum-ui` | software-rendering toolkit and the shared fullscreen Wayland client runner; a convenience library, not part of any ABI | library |

Product crates depend on `app-api`/`app-protocol` and `hardware-api`. The
architecture test in `crates/mobile-session/tests/architecture.rs` fails when
an application, shell, or the core names `runtime-native`, `app-runtime-api`,
`hardware-emulator`, or the `appd` crate, and when `app-api` gains target or
implementation conditionals.

## Identity and descriptors

An `AppId` is a lowercase reverse-DNS-style identifier (`org.ferrix.terminal`).
It is validated on construction and on deserialization and is the single key
for launch, instances, crash records, and surface association. Runtime-specific
identifiers are descriptor attributes, never separate identities.

Descriptors are JSON files, one per application, in `apps/` (installed to
`/usr/share/ferrix/apps/` in the guest). Fields: `schema_version`, `id`,
`runtime` (`native` | `web` | `android`), `name`, `icon` (`glyph`, `tone`),
`launchable`, `autostart`, `role` (`application` | `home`), `trust`,
and `capabilities`. A daemon-private `launch` object in the same manifest is
opaque to `appd` and interpreted only by the selected runtime adapter. For the
native adapter it contains `exec` (program resolved through `PATH`) and an
optional isolation policy. The `launch` object is never returned by
`app-protocol`, so applications and shells cannot couple to another runtime's
implementation. Non-launchable entries exist so the reference launcher keeps its
illustrative pages while the catalog, not an enum, decides what is shown.
`web` and `android` are reserved kinds: a descriptor of those kinds is valid,
but launching it returns `AppError::RuntimeUnsupported` until a runtime exists.

## Lifecycle

```text
not-running -> starting -> foreground <-> background -> frozen
                   \                                 /
                    -> crashed / terminated <-------
```

`LifecycleState::transition` enforces the edges. `ExitReason` distinguishes a
clean exit (`Terminated`) from a signal death or non-zero exit (`Crashed`).
`appd` publishes every change as `AppEvent::Lifecycle` to subscribers and as a
serial record. Exactly one instance per application exists at a time: a second
`Launch` activates the running instance instead.

## Launch and isolation

`appd` launches through the `app-runtime-api::Runtime` trait. The native runtime spawns
`ferrix-sandbox --policy <IsolationPolicy JSON> --app-id <id> -- <exec>` in a
new process group with inherited stdio, so application records keep reaching
the serial console through `appd`'s log. The sandbox is written entirely in
safe Rust (the workspace forbids `unsafe`) on top of `rustix` and
`signal-hook`:

1. Stage 1 arms `PR_SET_PDEATHSIG` so it dies with `appd`, unshares the
   requested namespaces in one `unshare(2)` call (`user` when not root and
   the policy says `auto`, then `pid`, `mount`, `ipc`, `uts`), writes the
   identity uid/gid mapping when a user namespace was created, sets the UTS
   hostname to the application id, and spawns stage 2.
2. Stage 2 is PID 1 of the application's PID namespace. It makes mount
   propagation private, mounts a fresh `/proc` and a tmpfs `/tmp`, sets
   `no_new_privs`, spawns the application, forwards `TERM`/`INT`/`HUP`, reaps
   zombies, and exits with the application's status (`128 + signal` on a
   signal death). When PID 1 exits the kernel kills everything left in the
   namespace, so no orphaned helper survives a crash.
3. Stage 1 exits with the same status; `runtime-native` maps it back to an
   `ExitReason`. Exit status 126 from the sandbox means the required isolation
   could not be established and surfaces as `AppError::IsolationUnavailable`.

The `IsolationPolicy` default requests all four namespaces, a private `/tmp`,
and `no_new_privs`, in `required` mode. `best-effort` mode still starts the
application as a separate supervised process and reports the degraded
isolation in the private runtime diagnostics attached to the `app-started`
evidence record; it never hides it or exposes Linux-specific fields as public
application state.

What isolation provides today: a crash is confined to one process tree; the
application cannot see or signal platform processes (PID namespace); it has a
private `/proc` and `/tmp`, its own IPC and hostname namespaces, and cannot
gain privileges. What it does not provide yet: a different uid, seccomp
filters, cgroup resource limits, a network namespace, or a restricted
filesystem view. The Wayland socket, the core socket, and the `appd` socket
remain reachable because they live on the shared filesystem; permission
enforcement at those services is phase 3 work.

## Surface association and focus

The compositor reads each client's pid from the kernel (`SO_PEERCRED`) and
reports `SurfaceMapped{pid, app_id, title}`, `SurfaceUnmapped{pid}`, and
`FocusChanged{pid}`. `appd` resolves a pid to an instance by matching the
sandbox pid or by walking `/proc/<pid>/status` parents (the application is a
grandchild of `appd`). The xdg `app_id` string an application sets is untrusted
metadata: it is recorded and compared, never used to identify the caller.

`appd` pushes `Notification::Activate{pid}` to the compositor to raise a
window: on a second launch of a running app, on `Request::Home`, on the
compositor's `HomeGesture` report, and when an application yields the
foreground. When the focused client disconnects, the compositor refocuses the
next mapped window (the home surface when nothing else is running) and reports
the change; `appd` marks the instance crashed or terminated from the exit
status it observes directly.

## Trusted gestures

The bottom 8% of the output is the compositor's home zone. A contact that
starts there is never delivered to clients; a tap or an upward swipe reports
`HomeGesture` to `appd` (or raises the oldest mapped window when `appd` is not
connected). Back remains application-owned for now: the Terminal's header
arrow and left-edge swipe send `YieldForeground`. Moving the on-screen
keyboard and navigation into compositor-owned chrome is phase 3 work.

## Protocol roles

Connections identify themselves with `Hello{role}`. On Linux/QEMU, `appd`
reads the peer PID from the socket and resolves launched descendants back to
their daemon-owned application identity; a launched app claiming a different
role is rejected. `appd` then gates by the verified role:
surface and focus reports are accepted only from the compositor; `Launch`,
`Activate`, and `Terminate` only from the home surface and tools;
`YieldForeground` from applications and the home surface. Malformed lines get
`Reply::ProtocolError`; failures are typed `AppError`s. A schema mismatch is
rejected at `Hello`. Compositor/tool authentication is still a bootstrap trust
boundary because those processes are not launched by `appd`; splitting their
privileged endpoint or passing an inherited capability is future hardening.

## Evidence

Records are single JSON lines with `"evidence"` first (`QEMU` in the guest,
`mock` nested on a host). New names: `appd-ready`, `app-launching`,
`app-started`, `sandbox-ready`, `app-surface`, `app-foreground`,
`app-background`, `app-exited`, `app-crashed`, `home-restarting`,
`home-restart-abandoned`, `appd-failed` (appd and sandbox); `client-gone`,
`client-activated`, `home-gesture`, `appd-link` (compositor); `home-launch`,
`home-crash-notice`, `appd-unavailable` (home surface); `app-context`,
`terminal-frame` (Terminal). `just qemu-check` is unchanged. Graphical
evidence for this slice is recorded in `docs/status.md`.

## Not in this slice

Portals and permission enforcement (phase 3), the Rust SDK beyond the launch
environment constants, the TypeScript/web runtime, the Android compatibility
runtime, per-application logs, cgroup limits, and any device target.
