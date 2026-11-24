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
  private, starts a short lived child to mount a fresh `/proc`, and mounts
  (with `private_tmp`) a tmpfs on `/tmp`,
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

## Per-application cgroup (`resources`, issue #10)

`IsolationPolicy.resources` (`#[serde(default)]`):

```json
"isolation": { "resources": { "cgroup": true, "memory_max": 1073741824, "pids_max": 1024, "cpu_weight": 100 } }
```

The defaults are those values (1 GiB, 1024 tasks, the kernel's default
weight); a `null` limit leaves that controller alone and `cgroup: false`
creates no group. Stage 1 creates `ferrix.slice/app-<app-id>-<stage-1 pid>`
under `/sys/fs/cgroup` (`FERRIX_CGROUP_SLICE` overrides the slice path),
enables `memory`/`pids`/`cpu` down the path (slice parent, then the slice),
writes `memory.max` (with `memory.oom.group = 1`, so an OOM kill takes the
whole application down as one signal death), `pids.max` and `cpu.weight`,
and moves itself into the group **before** `unshare`, so stage 2 and every
application process inherit it. The runtime knows the path from the
readiness report (or from the stage 1 pid when no report arrived), exposes
it as `RunningApp::cgroup_path()` (the handle for a later `cpu.uclamp.min`
boost, spec 10), and removes the group after reaping stage 1
(`cgroup.kill`, then `rmdir` with retries; a failure is a
`sandbox-cgroup-cleanup-failed` record).

Failure semantics follow the IPC-namespace precedent: a system without the
feature (no cgroup v2 hierarchy at the slice parent, a controller not
offered there, or a hierarchy the launcher may not write) is a degraded
launch with a `sandbox-degraded{feature:"cgroup"}` record and the reason in
`sandbox-ready`; any other failure (for example a limit the kernel rejects,
or a busy group of the same name) fails closed under `Required`
(`sandbox-failed`, status 126) and is degraded under `BestEffort`.
`sandbox-ready` and `--probe` report `cgroup: {path, memory_max, pids_max,
cpu_weight}` (`{}` when none). The QEMU initramfs mounts cgroup2 on
`/sys/fs/cgroup` and enables the three controllers from the root group
(`cgroup-ready` record); the pmOS image sets `rc_cgroup_mode="unified"` in
`/etc/rc.conf` explicitly and records it in `image-facts.json`
(`cgroup.rc_cgroup_mode`).

## Network namespace (`net_namespace`, issue #10)

`IsolationPolicy.net_namespace` (default `true`) gives the application a
private network namespace holding only `lo`, brought up by stage 1 with one
rtnetlink `RTM_NEWLINK` request (a fixed 32-byte message; hand-rolled
because the `SIOCSIFFLAGS` ioctl needs `unsafe` and spawning `ip` depends on
a host tool). Under `ferrix-appd` the value is **derived from the effective
grants** by `with_grants`: on unless the application holds the `network`
capability (`network-state`/`network-control` reach `ferrix-networkd` and do
not count); a manifest value is replaced, so a descriptor cannot keep the
host network without the grant. Only `IsolationPolicy::none()` (no
isolation at all) stays unsandboxed. Applications with `network` share the
launcher's network namespace (per-application veth and nftables are later
work). Pathname sockets (Wayland, `/run/ferrix/*.sock`) are not scoped to a
network namespace and keep working.

`sandbox-ready` adds `namespaces.net` (proven from `/proc/self/ns/net`) and
`network: {interfaces, loopback_up}` read from `/proc/self/net/dev` inside
the namespace. A kernel without `CONFIG_NET_NS` is a degraded launch with a
note (as for IPC); a loopback that cannot be brought up is a
`sandbox-degraded{feature:"loopback"}` record and `degraded: true` (the
application still has no route out). Abstract socket names are per network
namespace, so stage 1 connects its readiness report to the runtime's
listener **before** `unshare` (`channel::ReportSender`) and writes it
afterwards; the runtime's listener reads a connection that arrives early and
writes late, and a sender that closes without writing still ends as the
child's exit status (`IsolationUnavailable` for 126).

## Filesystem view (`fs_view`, issue #10)

```json
"isolation": { "fs_view": { "mode": "minimal" | "inherit" } }
```

`minimal` (the default) makes stage 2 assemble a new root on a 16 MiB
tmpfs at `/tmp/.ferrix-root-<pid>` and enter it, after the `/proc`, `/tmp`
and socket-directory views exist and before capabilities are dropped:

- read-only recursive binds of `/usr`, `/etc`, `/bin`, `/sbin`, `/lib`,
  `/lib64`, `/opt` (a symlink such as a merged-`/usr` `/bin` is recreated as
  that symlink), and of `/sys`; every submount below them is remounted
  read-only with its own flags kept (so `/sys/fs/cgroup` cannot be used to
  leave the application's group);
- a minimal `/dev`: `null`, `zero`, `full`, `random`, `urandom`, `tty`, a new
  devpts instance (`newinstance`, so no application sees another's ptys;
  bind of the host's `/dev/pts` as fallback), a private `/dev/shm`, `dri`
  when present, and the `ptmx`/`fd`/`std*` symlinks; no block, input or
  memory devices;
- the sandbox's own `/proc`, its `/run` view, its `/tmp`, and the socket
  directory when it is outside `/run` (nested runs, tests);
- the application's data directory, created owner-only before the view and
  bound read-write: `$XDG_DATA_HOME/ferrix/<app-id>`, else
  `$HOME/.local/share/ferrix/<app-id>`, exported as `FERRIX_APP_DATA_DIR`
  (also under `inherit`);
- the Ferrix configuration directory (`$XDG_CONFIG_HOME/ferrix`, else
  `~/.config/ferrix`, for example `keyboard.toml`) read-only;
- `$HOME`: bound read-write **only with the `files-home` grant**
  (`fs_view.home`, derived from the effective grants by `with_grants`; a
  manifest value is replaced). Without it `$HOME` is an empty, writable
  directory on the root tmpfs (lost at exit). A home that is `/`, not
  absolute, not normalized, or inside a system directory (`/usr`, `/etc`,
  `/run`, `/tmp`, `/var`, ...) is never exposed: an unmet `files-home` is a
  reason, so `Required` fails the launch instead of widening anything.

The root is entered with `pivot_root(".", ".")` and the old root detached;
on an initramfs root, which the kernel cannot pivot away from (EINVAL), the
new root is moved over `/` and entered with `chroot` (`root:
"move-chroot"`; the shadowed initramfs stays reachable to a process that can
`chroot` again, which seccomp removes). Everything else on the host (`/var`,
`/srv`, `/mnt`, other homes, the checkout) is absent. `inherit` keeps the
launcher's filesystem as before (plus the data directory). `sandbox-ready`
gains `fs_view: {mode, root, data_dir, home, read_only}`; a view that cannot
be built is `sandbox-mount-failed{target:"/"}` and fails closed under
`Required` (`unmet: fs_view`). `minimal` requires `mount_namespace`
(`InvalidDescriptor` otherwise). Grants: Files, Terminal and Contacts hold
`files-home` (Contacts exports to `~/Documents` until a document portal
exists, #11); every other application sees only its data directory.

## System-call filter (`seccomp`, issue #10)

```json
"isolation": { "seccomp": { "profile": "default" | "unconfined", "allow": ["ptrace"] } }
```

`default` installs a default-deny-dangerous filter compiled with
[`seccompiler`](https://github.com/rust-vmm/seccompiler) (rust-vmm,
`Apache-2.0 OR BSD-3-Clause`, taken under Apache-2.0; its only dependency is
`libc`; Linux-only dependency). Everything is allowed except the calls in
`seccomp::DENIED_SYSCALLS` (plus `ioperm`/`iopl`/`uselib` on x86_64), which
return `EPERM`: loading kernel code or BPF (`kexec_*`, `*_module`, `bpf`),
reaching into other processes (`ptrace`, `process_vm_*`, `pidfd_getfd`,
`kcmp`), changing the filesystem view or namespaces the sandbox built
(`mount`, `umount2`, `pivot_root`, `chroot`, the new mount API,
`mount_setattr`, `setns`, `unshare`, and `clone` with any `CLONE_NEW*`
flag), the kernel keyring, system-wide state (clocks, hostname, swap,
`reboot`, `acct`, quotas, `syslog`), and exploit primitives (`userfaultfd`,
`perf_event_open`, `open_by_handle_at`). `clone3` returns `ENOSYS` from a
second, stacked filter (its flags sit behind a pointer), so libc falls back
to `clone`. Stage 2 installs the filters last, after every mount and
`no_new_privs`, before it reports and starts the application; stage 2
itself and every descendant run under them.

The escape hatch is per launch configuration (the runtime-private part of
the descriptor, so each runtime adapter decides what its payload needs):
`allow` removes named entries from the deny list (`clone-namespaces` for the
`clone` rule); a name that is not on the deny list is `InvalidDescriptor`, so
a typo never reads as a relaxation. `profile: unconfined` installs nothing.
The Android runtime (Waydroid) does not use this sandbox and is not covered.
`sandbox-ready` gains `seccomp: {profile, denied, allowed}`; a kernel that
refuses the filter (no `CONFIG_SECCOMP_FILTER`) is a
`sandbox-degraded{feature:"seccomp"}` record and fails closed under
`Required` (`unmet: seccomp`). `kexec_file_load`'s number is pinned in the
source because musl's aarch64 `libc` bindings lack it.

## Per-application uid (`uid`, issue #10)

```json
"isolation": { "uid": "launcher" | "per-app" }
```

`per-app` (opt-in; root launchers only) runs the application as its own
stable subuid and subgid `S = app_subuid(id)` = `0x60000000 + FNV-1a(id) mod
2^24` (clear of `/etc/subuid` user ranges and `nobody`; the checked-in ids
are tested not to collide). It always creates a user namespace whose map
holds two identity lines, `0 0 1` and `S S 1`. The sandbox stages stay in-
namespace root (root's file ownership, capabilities only over the
application's own namespaces) and build the view as before; **only the
application process** switches to `S` (stage 2 starts it with std's
`CommandExt::uid`/`gid`, which also clears the supplementary groups), so it
holds no capability even inside its namespaces and cannot reach a set-uid
root (`no_new_privs` is required). A map with an id other than the writer's
own needs `CAP_SETUID` in the parent namespace, which stage 1 gives up by
unsharing; a helper started before the `unshare` (`ferrix-sandbox --map-ids
PID S`, waiting on stdin) writes `/proc/<stage 1>/{uid,gid}_map` when stage
1 says `go`. Stage 2 confirms the map from its own `/proc/self/uid_map`
before it trusts it, `chown`s the data directory to `S:S` (mode 0700) and
gives `S` the stand-in `$HOME`. `sandbox-ready` gains `identity: {mode, uid,
gid}`; an unprivileged launcher or a map that cannot be written fails closed
under `Required` (`unmet: uid`).

Constraints (`InvalidDescriptor` otherwise): `fs_view: minimal` (the subuid
reaches its data directory only through the view's own directories),
`no_new_privs`, and no `files-home` (the home folder belongs to the session
user; exposing it needs an id-mapped mount). The default stays `launcher`:
under `per-app` the application cannot reach root-owned sockets such as the
Wayland socket in `/run/user/0` (0700) or `/run/ferrix/*.sock`, so no
checked-in graphical application uses it yet.

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

No uid change by default (`uid: launcher`: the application keeps the launcher's
identity; `per-app` exists but no graphical app can use it yet), no cgroup freezing (limits only, see
above), no per-application network routing (granted apps share the launcher
network), no filesystem restriction under `fs_view: inherit` beyond `/proc`,
`/tmp`, and the socket directory (with `run_dir: private` the rest of `/run`,
including the D-Bus system bus socket, stays visible; `private-run` hides
it), and no capability dropping beyond `no_new_privs`: a root launcher's
application is still root inside its namespaces, so the read-only binds hold
only as long as it cannot `mount` (the seccomp filter refuses it). Socket
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
The cgroup tests cover: the limits are written, stage 1, stage 2 and the
application are in the group, and the group is gone after the exit; a fork
loop under `pids_max: 16` is refused (`pids.events max > 0`,
`pids.current <= 16`); writing 48 MiB to the private tmpfs under
`memory_max: 24 MiB` (swap taken away from that group) ends in one OOM
`SIGKILL` of the whole application; `cgroup: false` creates no group.
`linux-check.sh` first moves the container's processes into a leaf group so
the container's cgroup-namespace root can delegate controllers, as the guest
root does. The network tests cover: without the `network` grant (with
`network-state`) the application lists only `lo`, pings `127.0.0.1`, and a
`wget` to a TCP service on the launcher's loopback fails while that service
accepts nothing; with `network` the same `wget` succeeds; a manifest with
`net_namespace: false` still gets the namespace; the readiness report still
arrives (not `unverified`) from inside the namespace. The filesystem tests
cover: under `minimal` a host scratch file and a home document are absent,
`/bin/sh` and `/etc/passwd` are present, `touch` in `/usr` and `/etc` fails,
writing the application's pid to `/sys/fs/cgroup/cgroup.procs` fails, `/dev`
has `null` and a private `pts/ptmx` but no `kmsg`/`mem`, `$HOME` is an empty
stand-in, and a file written to `$FERRIX_APP_DATA_DIR` is on the host after
the exit (`root: pivot_root`); with `files-home` the document is readable
and a new file lands in the real home; `HOME=/etc` with `files-home` is
`IsolationUnavailable`; `inherit` sees the host file and still gets the
data directory. The cgroup tests run under `inherit` because they signal
through host files.
The seccomp tests cover: under the default profile a root application in its
namespaces cannot `mount` a tmpfs, remount `/usr` read-write, `unshare -n`
or `-U`, or `chroot`, and `/proc/self/status` shows `Seccomp: 2`; the
control run with `profile: unconfined` gets as far as the `mount` (exit
20), so the refusal is the filter's; `allow: [mount, umount2]` lets the
mount through while `unshare` stays refused.
The per-app uid test (home under the container's own `/home`, since a
macOS share does not keep foreign owners) covers: the application's uid and
gid are `S`, it has no supplementary groups and `CapEff` 0, the root-only
`/etc/shadow` (which the launcher can read) is refused, it can write its
data directory and stand-in `$HOME`, and on the host the data directory is
`S:S` mode 0700 and the written file is `S`'s.
The QEMU guest runs the same classes of negative check in check mode
(`targets/qemu-aarch64/initramfs/sandbox-selftest.sh`: forbidden path,
forbidden system calls, no network, pids and memory limits, per-app uid),
each a `sandbox-denied{check,status}` record; `just qemu-check` requires
the `sandbox-selftest` summary with `status: ok`.
