# ferrix-runtime-native

The native application runtime: `NativeRuntime` implements `ferrix_app_runtime_api::Runtime`
for `RuntimeKind::Native`, and the `ferrix-sandbox` binary is the supervisor
that puts each application into its own Linux namespaces. All code is safe
Rust (`rustix`, `signal-hook`, `std::process`).

## What the sandbox does

Every application is a separate supervised process, so a crash never reaches
`ferrix-appd`, the compositor, or the core. The sandbox adds containment on top, per
the manifest's runtime-private `IsolationPolicy`:

```text
ferrix-appd -> ferrix-sandbox (stage 1) -> ferrix-sandbox --init (stage 2, PID 1) -> application
```

- Stage 1 dies with `ferrix-appd` (parent-death signal), `unshare`s the requested
  namespaces in one call, writes the identity `uid_map`/`gid_map` when a user
  namespace was created (ids captured before `unshare`; inside the new
  namespace they read back as the overflow id), sets the hostname to the app
  id, and starts stage 2. It forwards TERM/INT/HUP to stage 2 and exits with
  stage 2's status. Stage 1 is deliberately single-threaded: after
  `unshare(CLONE_NEWPID)` the kernel refuses new threads, so signals are
  drained from the main thread (SIGCHLD wakes the final wait).
- Stage 2 is PID 1 of the new PID namespace: it makes mount propagation
  private, mounts a fresh `/proc` and (with `private_tmp`) a tmpfs on `/tmp`,
  builds the socket-directory view (below) when `run_dir.mode` is not
  `inherit`, sets `no_new_privs`, runs the application, forwards TERM/INT/HUP
  to it, reaps zombies, and exits with the application's shell status
  (`128 + signal` on a signal death). PID 1 exiting kills everything left in
  the namespace.
- The runtime maps the forwarded status back to `ExitReason`, so `ferrix-appd`
  classifies crashes exactly.

What each namespace buys: **PID** – the application cannot see or signal
platform processes, and all its descendants die with it. **Mount** – a private
`/proc` (only its own processes) and a private `/tmp`. **IPC** – no shared
System V / POSIX message queue access. **UTS** – its own hostname (the app id).
**User** – lets an unprivileged launcher create the others; `Auto` uses it only
when the launcher is not root (with one retry if a root launcher is denied).

## Socket-directory view (`run_dir`, ADR-004 §4)

`IsolationPolicy.run_dir` (`#[serde(default)]`, so old manifests are
unchanged):

```json
"isolation": { "run_dir": { "mode": "inherit" | "private" | "private-run", "sockets": ["ferrix-appd.sock"] } }
```

- `inherit` (default): the platform socket directory stays shared; nothing
  is filtered.
- `private`: stage 2 replaces `/run/ferrix` (or `FERRIX_RUN_DIR`) with a
  private tmpfs that holds only the listed socket files.
- `private-run`: a private tmpfs replaces all of `/run`; the filtered socket
  directory is put back at its path and `/run/user` (Wayland,
  `XDG_RUNTIME_DIR`) is bind-mounted back recursively. Everything else under
  `/run` — including `/run/dbus/system_bus_socket` — is gone from the
  application's view. Requires the socket directory to be under `/run`.

Under `ferrix-appd` the `sockets` list is **derived from the effective grants**
in `LaunchContext.granted` by `with_granted_sockets` (`sockets_for_grants`):
always `ferrix-appd.sock`; `core-state|core-control` → `core.sock`;
`telephony-*` → `telephonyd.sock`; `network-*` → `networkd.sock`;
`contacts-*` → `contactsd.sock`. A list written into a manifest is replaced,
so a descriptor cannot widen its own view. The names are constants in
`src/sockets.rs` and nowhere else.

Mechanism (safe Rust over `rustix::mount`): bind-mounting a socket by path
needs the source to stay visible, so the view is assembled on a tmpfs
mounted at a staging directory (`/tmp/.ferrix-run-<pid>`, which is the
application's own private tmpfs when `private_tmp` is on; a sibling of the
socket directory is the fallback for `private`). For each listed name the
original `<run_dir>/<name>` is bind-mounted over an empty file created in the
staging tree (`/run/user` recursively, under `private-run`); then one
`mount(MS_MOVE)` places the staging tree over the target and the empty
staging directory is removed. Nothing is opened or held open, and
propagation is already `rprivate` (stage 2 begins with that), which `MS_MOVE`
requires. Socket names must be bare file names (no `/`, not `.`/`..`). A
listed socket that does not exist yet (daemon not started) is reported in
`missing`, not bound, and is not a failure.

Outcome and failure: `sandbox-ready` gains `run_dir: {mode, sockets:
[names]}` (`missing` when non-empty); `mode` is what was actually applied
(`inherit` when nothing was mounted). Under `IsolationMode::Required` a view
that cannot be built (`sandbox-mount-failed{target,error}`) makes stage 2
exit 126 before starting the application and stage 1 forward that as
`sandbox-failed` → `AppError::IsolationUnavailable`; under `BestEffort` the
application starts with the inherited directory and the report is
`degraded` with a `run_dir:` reason. On macOS the policy parses and the
report says `inherit` (not enforced).

## Readiness and records

Readiness is observable: stage 2 reports to stage 1 and stage 1 reports to the
runtime over abstract Unix sockets (no filesystem, no stdio changes), and stage
1 prints `sandbox-ready` only after that. `RunningApp::isolation()` therefore
carries the confirmed outcome; if no report arrives it falls back to the
expected outcome marked `degraded` with an `unverified:` reason. Namespaces are
proven by comparing `/proc/self/ns/*` before and after `unshare`, never
assumed.

Records (one JSON line on stdout, `evidence` first): `sandbox-ready`
(`app`, `init_pid`, `namespaces{pid,mount,ipc,uts,user}`, `private_tmp`,
`no_new_privs`, `run_dir{mode,sockets,missing?}`, `degraded`, `reason`),
`sandbox-failed{app,reason}`,
`sandbox-mount-failed{app,target,error}`, `sandbox-degraded{app,feature,error}`,
`sandbox-exec-failed{app,program,error}`, `sandbox-app-exited{app,exit}`,
`sandbox-probe{applied,degraded,reason}`.

Exit statuses of the sandbox itself: 126 = required isolation unavailable
(`AppError::IsolationUnavailable`; once readiness was reported a 126 is the
application's own exit code), 127 = the program could not be executed,
125 = internal failure (could not start stage 2), 2 = usage error.

## Degraded and Required semantics

`IsolationMode::Required` (default): stage 1 fails immediately when `unshare`
cannot establish the requested namespaces, and stage 2 rechecks every requested
namespace plus private `/tmp`, `no_new_privs`, and the private runtime-directory
view before exec. Any unmet control prints `sandbox-failed` and exits 126
without starting the application; the runtime returns
`AppError::IsolationUnavailable` from `launch()`. Under `BestEffort`, the
application still starts and the report names every degraded control. A policy
that requests no namespaces, no private tmp, and no `no_new_privs` skips the
sandbox and spawns the program directly (still supervised). On non-Linux hosts
the sandbox is unavailable: `Required` fails, `BestEffort` spawns directly with
`IsolationReport::unisolated(.., "not linux")`.

`probe_sandbox()` runs `ferrix-sandbox --probe` (default policy) and returns
what could be established, or the reason it could not; a probe that exits 0
with a degraded report (for example `uid_map` or the init-stage mounts denied
after `unshare` succeeded) counts as "could not". Tests skip with a
`sandbox-test-skipped` record when the environment cannot meet the policy,
unless `FERRIX_SANDBOX_TESTS=required`, which turns every skip into a failure.

## Not provided yet

No uid change (the application keeps the launcher's identity, mapped 1:1 in a
user namespace), no seccomp filter, no cgroups (no memory/CPU limits, no
freezing), no network namespace, no filesystem view restriction beyond
`/proc`, `/tmp`, and the socket directory (with `run_dir: private` the rest of
`/run`, including the D-Bus system bus socket, stays visible; `private-run`
hides it), and no capability dropping beyond `no_new_privs`. Socket
visibility is enforced only when the manifest sets `run_dir.mode`; the
services enforce capabilities per request regardless (ADR-004 §3). Sandbox
`isolation()` is only as truthful as the readiness report; the `sandbox-ready`
record on the log is the evidence of record.

## Validation

macOS host: `cargo test -p ferrix-runtime-native` (library and pure planning tests).
A plain `cargo test` on an unprivileged Linux host (CI's `rust` job on
`ubuntu-latest`) runs the same and skips the namespace tests with records.
Linux: `FERRIX_SANDBOX_TESTS=required just linux-check test -p ferrix-runtime-native`
runs `tests/sandbox.rs` inside the privileged pinned arm64 Alpine Rust
container (mock evidence, not QEMU; CI's `sandbox-check` job, where a skip is
a failure). Socket directories are created outside `/tmp` under a root short
enough for `sun_path` and removed afterwards. The socket-visibility tests
cover: a policy listing only `a.sock` shows
`a.sock` and not `b.sock` (via `FERRIX_RUN_DIR` on a temp directory); through
`NativeRuntime`, a launch without a telephony grant cannot see
`telephonyd.sock` while one with `telephony-state` can; a missing socket
directory is `IsolationUnavailable` under `required` and degraded under
`best-effort`; `private-run` hides a marker file placed in `/run`.
