# Application platform

This document records the application contract implemented in the workspace:
how applications are identified, described, launched, isolated, and tracked,
and which process owns which state. It applies the bundled
`ferrum-mobile-architecture` skill (phases 1 and 2 of its implementation path)
to the current crates. `plans/baseline.md` remains the implementation
authority; this document does not change the target, the guest substrate, or
the hardware boundary. Evidence is QEMU/mock only; nothing here is a
physical-device claim.

## Decision

Applications are supervised runtime instances; native applications are
independent Linux processes, while Android applications are processes inside
the separately managed Waydroid container. One application service
(`ferrix-appd`) owns the catalog, activation, foreground/background state,
crash records, and surface association. The compositor (`ferrix-shell`) owns
surfaces, focus, and the trusted global home gesture. The home surface
(`ferrix-basic-screen`) is one application among others: it queries `ferrix-appd`
for the catalog and asks `ferrix-appd` to launch; it never forks commands or scans
packages.

Every native application runs inside its own Linux namespaces through a small
safe-Rust sandbox (`ferrix-sandbox`). A crash therefore ends exactly one
process tree: the compositor keeps running, focus returns to the home surface,
`ferrix-appd` records the crash and publishes it, and the home surface shows a
low-noise notice. The home surface itself is restarted by `ferrix-appd` with backoff.

## Layers and ownership

```text
  home surface           native apps             Android app
  (ferrix-basic-screen)  (Terminal, Phone, …)     (Fulguris via Waydroid)
        |  Launch/Subscribe        |  YieldForeground
        +-------------+------------+-----------------------------+
                      |  ferrix-app-protocol (line-delimited JSON, schema v1)
                    ferrix-appd  <--- SurfaceMapped / FocusChanged / gestures ----- ferrix-shell
                      |   --- Activate{pid} ------------------------------->  (compositor)
            ferrix_app_runtime_api::Runtime (internal trait)
                      |                         |
              ferrix-runtime-native     ferrix-runtime-android
                      |                         |
              ferrix-sandbox ---> app          Waydroid ---> Android app
```

| Crate | Owns | Trust ring |
| --- | --- | --- |
| `ferrix-app-api` | public `AppId`, `RuntimeKind`, `AppDescriptor`, `AppCapability`, `AppIdentity`, `GrantsPolicy`, `LifecycleState`, `ExitReason`, `AppInstance`, `AppEvent`, `AppError`, and launch environment constants | contract |
| `ferrix-app-protocol` | `Request`/`Report`/`Reply`/`Notification` wire types (including `Identify`/`Identity`), blocking client, subscription reader | contract |
| `ferrix-app-runtime-api` | internal `Runtime`/`RunningApp` traits, launch context (with the effective grants), and opaque runtime configuration boundary | internal contract |
| `ferrix-appd` | catalog, grants policy, single-instance policy, lifecycle state machine, crash records, home restart policy, surface association, pid → identity resolution, role gating | system |
| `ferrix-runtime-native` | native launch configuration and isolation types (namespaces, private `/tmp`, socket-directory view); the native `Runtime` that spawns `ferrix-sandbox`, classifies exits, and terminates | internal adapter |
| `ferrix-shell` | toplevels keyed by client pid, focus, `Activate` handling, trusted navigation/dismiss gestures, crash refocus | system |
| `ferrix-basic-screen` | catalog-driven launcher, in-process quick settings/notifications, crash notice | privileged first-party |
| `ferrix-terminal`, `ferrix-phone`, `ferrix-messages`, `ferrix-contacts`, `ferrix-settings`, `ferrix-files` | independent native Wayland applications | untrusted applications |
| `ferrix-runtime-android` | private Waydroid adapter for Android descriptors; composed only by `ferrix-appd` | internal adapter |
| `ferrix-ui` | software-rendering toolkit and the shared fullscreen Wayland client runner; a convenience library, not part of any ABI | library |

Product crates depend on `ferrix-app-api`/`ferrix-app-protocol` and `ferrix-hardware-api`. The
architecture test in `crates/ferrix-session/tests/architecture.rs` fails when
an application, shell, or the core names `ferrix-runtime-native`, `ferrix-app-runtime-api`,
`ferrix-hardware-emulator`, or the `ferrix-appd` crate, and when `ferrix-app-api` gains target or
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
and `capabilities`. `capabilities` is a **declaration** (discovery: what the
application asks for); it grants nothing. Authorization comes from the
separate grants policy described under *Grants and identity*. A daemon-private `launch` object in the same manifest is
opaque to `ferrix-appd` and interpreted only by the selected runtime adapter. For the
native adapter it contains `exec` (program resolved through `PATH`) and an
optional isolation policy. The `launch` object is never returned by
`ferrix-app-protocol`, so applications and shells cannot couple to another runtime's
implementation. Non-launchable entries exist for catalog placeholders such as
Camera; the catalog, not an enum, decides what is shown. `web`
remains a reserved kind and returns `AppError::RuntimeUnsupported`. Android
descriptors are handled by the private `ferrix-runtime-android` adapter; a
launch returns `RuntimeUnsupported` when Waydroid is absent and `LaunchFailed`
when the runtime or package is unavailable. Android package/activity details
remain in the daemon-private `launch` object.

## Lifecycle

```text
not-running -> starting -> foreground <-> background -> frozen
                   \                                 /
                    -> crashed / terminated <-------
```

`LifecycleState::transition` enforces the edges. `ExitReason` distinguishes a
clean exit (`Terminated`) from a signal death or non-zero exit (`Crashed`).
`ferrix-appd` publishes every change as `AppEvent::Lifecycle` to subscribers and as a
serial record. Exactly one instance per application exists at a time: a second
`Launch` activates the running instance instead.

## Grants and identity

Authorization is decided by `ferrix-appd` from two inputs that are kept apart
on purpose (ADR-004):

- the descriptor **declares** capabilities (`capabilities`);
- the checked-in **grants policy** `apps/grants.json` (installed beside the
  descriptors; `--grants PATH` / `FERRIX_APP_GRANTS` override it) says what
  each application *may* hold: `{"schema_version": 1, "grants": {"<app id>":
  ["telephony-call", ...]}}`, parsed strictly (`deny_unknown_fields`, ids and
  capability names validated, schema 1 only).

The effective set is `declared ∩ granted`. It is computed once at launch,
handed to the runtime in `LaunchContext.granted`, fixed for the life of the
instance, and reported in the `app-started` record as `granted` together with
`denied_capabilities` (declared but withheld). Trust rings never bypass the
file: `org.ferrix.home` is `privileged` and still only holds what its row
grants. **An application without a row, or a system without a grants file,
has zero effective capabilities**; a missing file is recorded
(`grants-missing`), a file that exists but is invalid is a startup failure
(`grants-invalid`).

Capability names (`AppCapability`, kebab-case on the wire): `core-state`,
`core-control`, `shell`, `background`, `telephony-state`, `telephony-call`,
`telephony-sms`, `telephony-data`, `telephony-radio`, `contacts-read`,
`contacts-write`, `network-state`, `network-control`. Read-only `*-state`
grants and privileged `*-control`/
`*-call`/`*-sms`/`*-write` grants are distinct so a state grant can never
reach a control verb.

Services (telephony, network, contacts) never trust an `AppId` in a payload.
They read the peer pid from their own socket (`SO_PEERCRED`), connect to
`ferrix-appd` with role `service`, and send `{"request":"identify","pid":P}`.
`ferrix-appd` resolves the pid through the same `/proc` ancestry walk used for
surface association to a **running** instance and replies
`{"reply":"identity","schema_version":1,"app":"org.ferrix.phone","trust":"untrusted","granted":["telephony-state","telephony-call"]}`;
a pid it did not launch, or whose instance has exited, yields
`{"reply":"identity","schema_version":1,"app":null,"trust":null,"granted":[]}`.
Every daemon checks the required capability on every request against that
identity (ADR-004 §3). `Identify` is answered for the `service` and `tool`
roles only.

## Launch and isolation

`ferrix-appd` launches through the `ferrix-app-runtime-api::Runtime` trait. The native runtime spawns
`ferrix-sandbox --policy <IsolationPolicy JSON> --app-id <id> -- <exec>` in a
new process group with inherited stdio, so application records keep reaching
the serial console through `ferrix-appd`'s log. The sandbox is written entirely in
safe Rust (the workspace forbids `unsafe`) on top of `rustix` and
`signal-hook`:

1. Stage 1 arms `PR_SET_PDEATHSIG` so it dies with `ferrix-appd`, unshares the
   requested namespaces in one `unshare(2)` call (`user` when not root and
   the policy says `auto`, then `pid`, `mount`, `ipc`, `uts`), writes the
   identity uid/gid mapping when a user namespace was created, sets the UTS
   hostname to the application id, and spawns stage 2.
2. Stage 2 is PID 1 of the application's PID namespace. It makes mount
   propagation private, mounts a fresh `/proc` and a tmpfs `/tmp`, builds the
   socket-directory view when `run_dir.mode` is not `inherit` (below), sets
   `no_new_privs`, spawns the application, forwards `TERM`/`INT`/`HUP`, reaps
   zombies, and exits with the application's status (`128 + signal` on a
   signal death). When PID 1 exits the kernel kills everything left in the
   namespace, so no orphaned helper survives a crash.
3. Stage 1 exits with the same status; `ferrix-runtime-native` maps it back to an
   `ExitReason`. Exit status 126 from the sandbox means the required isolation
   could not be established and surfaces as `AppError::IsolationUnavailable`.

The `IsolationPolicy` default requests all four namespaces, a private `/tmp`,
and `no_new_privs`, in `required` mode, with `run_dir: {mode: inherit}`.
`best-effort` mode still starts the
application as a separate supervised process and reports the degraded
isolation in the private runtime diagnostics attached to the `app-started`
evidence record; it never hides it or exposes Linux-specific fields as public
application state.

### Socket visibility

Platform daemons listen under one directory, `/run/ferrix` (`FERRIX_RUN_DIR`
in nested runs): `ferrix-appd.sock`, `core.sock`, `telephonyd.sock`,
`networkd.sock`, `contactsd.sock`. The Wayland socket is elsewhere
(`XDG_RUNTIME_DIR`, `/run/user/<uid>`). A manifest opts into the filtered
view with `"isolation": {"run_dir": {"mode": "private-run"}}`; the native runtime
then **derives the visible socket list from the effective grants** (a list
written into the manifest is replaced, never merged): always
`ferrix-appd.sock`; `core-*` → `core.sock`; `telephony-*` → `telephonyd.sock`;
`network-*` → `networkd.sock`; `contacts-*` → `contactsd.sock`. Stage 2 mounts
a tmpfs on a staging directory, creates one file per listed socket and
bind-mounts the original over it (the originals stay visible until the very
end, so no descriptor has to be held open), then `mount --move`s the staging
directory over `/run/ferrix`. Inside the application `connect()` on any other
daemon socket fails with `ENOENT`; the host's directory is untouched. A
listed socket whose daemon is not running yet is reported as `missing`, not
bound, and not a failure. `mode: private-run` goes further: a tmpfs replaces
all of `/run`, with the filtered `/run/ferrix` and `/run/user` bind-mounted
back, so the D-Bus system bus socket (`/run/dbus/system_bus_socket`) and
everything else under `/run` disappear from the application's view.

Under `required`, a socket-directory view that cannot be built (no
`/run/ferrix`, no mount permission) stops the launch with exit 126 →
`AppError::IsolationUnavailable`; under `best-effort` the application starts
with the inherited directory and the `sandbox-ready`/`app-started`
diagnostics say `run_dir: {mode: inherit}` with `degraded: true`. The
`sandbox-ready` record always carries `run_dir: {mode, sockets: [names]}`.
On macOS the policy parses and is recorded as `inherit` (not enforced); the
Linux behaviour is exercised by `tests/sandbox.rs` in the pinned container
(`mock` evidence).

What isolation provides today: a crash is confined to one process tree; the
application cannot see or signal platform processes (PID namespace); it has a
private `/proc` and `/tmp`, its own IPC and hostname namespaces, and cannot
gain privileges; with `run_dir.mode = private-run` it can reach exactly the
daemon sockets its grants imply, and every daemon additionally enforces
capabilities per request after resolving the caller through `Identify`.
What it does not provide yet: a different uid (every application runs as the
launcher's uid), seccomp filters, cgroup resource limits, a network
namespace, or a restricted view of the filesystem beyond `/proc`, `/tmp`, and
the socket directory. With `mode: private` the rest of `/run` is unchanged,
so the D-Bus system bus socket remains visible (though only the daemons
speak D-Bus; applications have no grant that reaches ModemManager or
NetworkManager directly); `mode: private-run` hides it. Every checked-in
launchable native application uses `private-run`; `private` remains available
only as a documented compatibility mode for manifests that deliberately need
other `/run` facilities.

Files (`org.ferrix.files`) depends on exactly that boundary. It talks to no
platform daemon, so its descriptor declares no capabilities and
`apps/grants.json` has no row for it (zero effective capabilities; its
`private-run` view holds only `ferrix-appd.sock` for Back). The mount
namespace replaces only `/proc`, `/tmp`, and `/run`, so the inherited `$HOME`
stays the shared, durable home directory Files reads and writes (and where
Contacts writes its vCard export). Keeping Files inside `$HOME` is the
app's job, not the sandbox's: every folder path is canonicalized and must stay
under the canonical home, entry names are single components, and symlinks that
leave home are listed but never followed. A sandbox-enforced filesystem view
(bind-mounting only `$HOME`) is future isolation work, not something this
release provides.

## Surface association and focus

The compositor reads each client's pid from the kernel (`SO_PEERCRED`) and
reports `SurfaceMapped{pid, app_id, title}`, `SurfaceUnmapped{pid}`, and
`FocusChanged{pid}`. `ferrix-appd` resolves a pid to an instance by matching the
sandbox pid or by walking `/proc/<pid>/status` parents (the application is a
grandchild of `ferrix-appd`). The xdg `app_id` string an application sets is untrusted
metadata: it is recorded and compared, never used to identify the caller.

`ferrix-appd` pushes `Notification::Activate{pid}` to the compositor to raise a
window: on a first launch once its surface maps, on a second launch of a running
app, on `Request::Home`, on the compositor's `HomeGesture` report, and when an
application yields the foreground. The pid is always the mapped Wayland client
pid reported by the compositor, never the sandbox supervisor pid. An activation
that arrives before `SurfaceMapped` is retained on that app instance and
delivered when its client pid becomes known. This is a platform invariant for
every runtime-backed app, not Terminal-specific launcher behavior. When the
surface first maps it joins the top of the fullscreen stack; later buffer
commits update that surface without raising it, so a background app's periodic
redraw cannot cover the foreground app. When the
focused client disconnects, the compositor refocuses the next mapped window
(the home surface when nothing else is running) and reports the change;
`ferrix-appd` marks the instance crashed or terminated from the exit status it
observes directly.

## Trusted gestures

The bottom 8% of the output is compositor-owned and never delivered to
clients. A tap or ordinary upward swipe reports `HomeGesture` to
`ferrix-appd`, backgrounds the foreground app, and raises Home. A deliberate
long upward swipe (more than 55% of the panel's shorter side) reports the
compositor-only `CloseGesture`; `ferrix-appd` terminates the foreground app
but refuses to dismiss Home. This is the product's one-stage software-rendered
adaptation of swiping a recent-app card away; a thumbnail overview remains
future shell work. Without the appd link, Home raises the oldest mapped
surface and Close asks a non-Home focused xdg toplevel to close itself.

Back remains application-owned for now. An inward swipe from either side
edge follows the familiar Android/iOS Back convention: it navigates out of a
sub-page and sends `YieldForeground` at the app root. Moving the on-screen
keyboard and navigation into compositor-owned chrome is phase 3 work.

## Protocol roles

Connections identify themselves with `Hello{role}`; roles are `compositor`,
`home`, `application`, `tool`, and `service`. On Linux/QEMU, `ferrix-appd`
reads the peer PID from the socket and resolves launched descendants back to
their daemon-owned application identity; a launched app claiming a different
role (`tool`, `service`, `compositor`) is rejected at `Hello`. `ferrix-appd`
then gates by the verified role:

| message | roles |
|---|---|
| `Catalog`, `Instances`, `Subscribe` | any |
| `Home` | `compositor`, `home`, `application`, `tool` |
| `Identify` | `service`, `tool` |
| `Launch`, `Activate`, `Terminate` | `home`, `tool` |
| `SurfaceMapped`, `SurfaceUnmapped`, `FocusChanged`, `HomeGesture`, `CloseGesture` | `compositor` |
| `YieldForeground` | `application`, `home` |

A `service` resolves identities and observes lifecycle events; it never
launches, activates, terminates, returns home, or sends reports. Malformed
lines get `Reply::ProtocolError`; failures are typed `AppError`s. A schema
mismatch is rejected at `Hello`. Compositor, tool, and service authentication
is still a bootstrap trust boundary because those processes are not launched
by `ferrix-appd` (they start from the session before any application exists);
splitting their privileged endpoint or passing an inherited capability is
future hardening.

## Evidence

Records are single JSON lines with `"evidence"` first (`QEMU` in the guest,
`mock` nested on a host). New names: `ferrix-appd-ready`, `app-launching`,
`app-started` (with `granted`, `denied_capabilities`), `sandbox-ready` (with
`run_dir`), `grants-loaded`, `grants-missing`, `grants-invalid`,
`appd-identify` (`pid`, `app|null`, `granted_count`; once per connection per
pid), `app-surface`, `app-foreground`,
`app-background`, `app-exited`, `app-crashed`, `home-restarting`,
`home-restart-abandoned`, `ferrix-appd-failed` (ferrix-appd and sandbox); `client-gone`,
`client-activated`, `home-gesture`, `ferrix-appd-link` (compositor); `home-launch`,
`home-crash-notice`, `ferrix-appd-unavailable` (home surface); `app-context`,
`terminal-frame` (Terminal). `just qemu-check` is unchanged. Graphical
evidence for this slice is recorded in `docs/status.md`.

## Remaining platform work

Portals and user-facing permission prompts (grants are a checked-in policy,
not a runtime consent flow), a different uid per application, seccomp, cgroup
limits, a network namespace, a filesystem view restricted beyond `/proc`,
`/tmp`, and the socket directory, authenticated (rather than bootstrap-trusted)
compositor/tool/service endpoints, the Rust SDK beyond the launch environment
constants, the TypeScript/web runtime, per-application logs, and any device
target. The Android compatibility runtime is documented separately (ADR-005
and `docs/android-runtime.md`); it never receives a `/run/ferrix` socket. Web
runtime support is still reserved and unimplemented.
