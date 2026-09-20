# runtime-native

The native application runtime: `NativeRuntime` implements `app_runtime_api::Runtime`
for `RuntimeKind::Native`, and the `ferrix-sandbox` binary is the supervisor
that puts each application into its own Linux namespaces. All code is safe
Rust (`rustix`, `signal-hook`, `std::process`).

## What the sandbox does

Every application is a separate supervised process, so a crash never reaches
`appd`, the compositor, or the core. The sandbox adds containment on top, per
the manifest's runtime-private `IsolationPolicy`:

```text
appd -> ferrix-sandbox (stage 1) -> ferrix-sandbox --init (stage 2, PID 1) -> application
```

- Stage 1 dies with `appd` (parent-death signal), `unshare`s the requested
  namespaces in one call, writes the identity `uid_map`/`gid_map` when a user
  namespace was created (ids captured before `unshare`; inside the new
  namespace they read back as the overflow id), sets the hostname to the app
  id, and starts stage 2. It forwards TERM/INT/HUP to stage 2 and exits with
  stage 2's status. Stage 1 is deliberately single-threaded: after
  `unshare(CLONE_NEWPID)` the kernel refuses new threads, so signals are
  drained from the main thread (SIGCHLD wakes the final wait).
- Stage 2 is PID 1 of the new PID namespace: it makes mount propagation
  private, mounts a fresh `/proc` and (with `private_tmp`) a tmpfs on `/tmp`,
  sets `no_new_privs`, runs the application, forwards TERM/INT/HUP to it, reaps
  zombies, and exits with the application's shell status (`128 + signal` on a
  signal death). PID 1 exiting kills everything left in the namespace.
- The runtime maps the forwarded status back to `ExitReason`, so `appd`
  classifies crashes exactly.

What each namespace buys: **PID** – the application cannot see or signal
platform processes, and all its descendants die with it. **Mount** – a private
`/proc` (only its own processes) and a private `/tmp`. **IPC** – no shared
System V / POSIX message queue access. **UTS** – its own hostname (the app id).
**User** – lets an unprivileged launcher create the others; `Auto` uses it only
when the launcher is not root (with one retry if a root launcher is denied).

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
`no_new_privs`, `degraded`, `reason`), `sandbox-failed{app,reason}`,
`sandbox-mount-failed{app,target,error}`, `sandbox-degraded{app,feature,error}`,
`sandbox-exec-failed{app,program,error}`, `sandbox-app-exited{app,exit}`,
`sandbox-probe{applied,degraded,reason}`.

Exit statuses of the sandbox itself: 126 = required isolation unavailable
(`AppError::IsolationUnavailable`; once readiness was reported a 126 is the
application's own exit code), 127 = the program could not be executed,
125 = internal failure (could not start stage 2), 2 = usage error.

## Degraded and Required semantics

`IsolationMode::Required` (default): if the `unshare` call fails, stage 1
prints `sandbox-failed` and exits 126 without starting anything; the runtime
returns `AppError::IsolationUnavailable` from `launch()`. `BestEffort`: the
application still starts, without namespaces, and the report says so.
Mount failures and `no_new_privs` failures inside stage 2 never abort a
launch in either mode; they are recorded and set `degraded`. A policy that
requests no namespaces, no private tmp, and no `no_new_privs` skips the
sandbox and spawns the program directly (still supervised). On non-Linux hosts
the sandbox is unavailable: `Required` fails, `BestEffort` spawns directly with
`IsolationReport::unisolated(.., "not linux")`.

`probe_sandbox()` runs `ferrix-sandbox --probe` (default policy) and returns
what could be established, or the reason it could not; tests skip with a
`sandbox-test-skipped` record when the environment forbids namespaces.

## Not provided yet

No uid change (the application keeps the launcher's identity, mapped 1:1 in a
user namespace), no seccomp filter, no cgroups (no memory/CPU limits, no
freezing), no network namespace, no filesystem view restriction (the
application sees the same root filesystem apart from `/proc` and `/tmp`), and
no capability dropping beyond `no_new_privs`. Sandbox `isolation()` is only as
truthful as the readiness report; the `sandbox-ready` record on the log is the
evidence of record.

## Validation

macOS host: `cargo test -p runtime-native` (library and pure planning tests).
Linux: `just linux-check test -p runtime-native` runs `tests/sandbox.rs` inside
the privileged pinned arm64 Alpine Rust container (mock evidence, not QEMU).
