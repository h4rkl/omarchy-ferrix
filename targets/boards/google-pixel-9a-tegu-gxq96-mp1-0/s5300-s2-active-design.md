# S5300 S2 active run: OpenRC teardown and disconnect-safe observation design (issue #109)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.

> **This is a design for owner review. Nothing in it is implemented, and it
> authorises nothing.** It adds no command, script, service or package
> change. It does not authorise a source mount, scratch provisioning, an
> `arm`, a CP boot, a teardown, or any device-side characterisation run.
> No phone, USB link, builder or CPIF node was touched while writing it.
> `[telephony]` stays `unsupported`. Every later slice listed at the end
> needs its own review, and any physical step needs its own owner decision.

Scope: blockers 2, 5 and 6 of
[`s5300-s2-active-blockers.md`](s5300-s2-active-blockers.md) (parent #92).
Blocker 3 (opener scan) is #107, blocker 4 (identity and decision binding)
is #108, and blocker 1 (mounts rollback) is #110. This design consumes the
#108 record contract but does not change it.

Inputs read for this design: [`s5300-route-decision.md`](s5300-route-decision.md)
(S2 GO/STOP), [`s5300-s2-preflight.md`](s5300-s2-preflight.md),
[`s5300-cpif-integration.md`](s5300-cpif-integration.md),
[`s5300-cpif-adapter.md`](s5300-cpif-adapter.md),
[`s5300-cpif-e0.md`](s5300-cpif-e0.md), the four `pmos/device-google-tegu/`
files named below, [`acm-probe.py`](../../../tools/pixel-9a/acm-probe.py),
and the #108 binding design on branch `issue108-s2-binding` (not on `main`
when this was written; cited by branch path).

Citation forms: `exynos-modem.initd:175` means
`pmos/device-google-tegu/exynos-modem.initd` line 175 in this directory;
`cpif.sh` is `pmos/device-google-tegu/ferrix-s5300-cpif.sh`; `mounts.initd`
is `pmos/device-google-tegu/ferrix-s5300-mounts.initd`; doc names without a
path are in this directory; `#108:` cites the #108 branch. Statements about
**upstream OpenRC** are the reviewer's reading of upstream behaviour. OpenRC
source is **not in this repository**, and the OpenRC version on the image is
**not recorded** anywhere in the repo (see 1.1). Every such statement is a
hypothesis until slice C1/C2 below observes it.

## Summary of recommendations

1. **Characterise before designing code.** OpenRC semantics are hypotheses
   today. Observe them first in a container with the image's exact OpenRC
   package (labelled `mock`), then once on the device with a throwaway
   `sleep` service that has no relation to any S5300 service. Each result
   either confirms an assumption below or blocks the active slice.
2. **Use a guest-side, one-shot supervisor**, launched detached
   (`setsid`, stdin `/dev/null`, output on tmpfs) by a short host session.
   It runs `arm` and `start`, observes read-only with its own monotonic
   deadline, and tears down in a fixed, never-forced order. The host only
   launches it and later fetches evidence over separate read-only sessions.
   A host-held 10-minute session is rejected, because a timeout or USB
   disconnect can kill `start_pre` or `start` part-way (section 2).
3. **Never automatic:** no retry, no re-arm, no respawn, no signal from the
   supervisor to the daemon, no forced or lazy unmount, no manual loop
   detach, and no reboot by the supervisor. A teardown step that refuses
   ends the run, and the lead reboots after fetching the evidence.
4. **STOP on the first CP crash state**, not on the second. The daemon
   re-boots the CP by itself in an internal loop (finding F2), so the
   route's "repeated crash" would otherwise happen without any supervisor
   decision.
5. **Capture daemon output by a pinned package change**, not by an
   override: `output_log`/`error_log` set in `exynos-modem.initd` itself,
   pointing at a fixed tmpfs path, with a new `device-google-tegu`
   release and pins. The preflight then also requires that log to be absent
   before `arm`.
6. **The #108 10-minute expiry covers decision → `start` only.** The
   observation window and teardown run after the record has expired, under
   the supervisor's own deadline, inside the same boot, and the guest
   re-checks the boot before `arm`.

## 1. OpenRC semantics the active run depends on

### 1.1 What is pinned, and what is not

- The rootfs is built from pmaports `v26.06` at
  `368093c7…4cf4d4` (`pmos/pins.env:5-7`) on Alpine `v3.24` /
  `3.24-stable` (`pmos/channels.cfg:12-13`). Binary packages, including
  `openrc` and `busybox`, come from that moving stable branch. No OpenRC
  version or package hash is pinned or recorded in the repository.
  `s5300-scratch-dir.md:182` and `s5300-cpif-integration.md:340` already
  ask for it to be recorded. **Unknown:** the exact `openrc` version on the
  1-r3 image.
- The rootfs `inittab` is not in the repository. The Kleaf initramfs only
  appends the `ttyGS1` getty and the `ttyGS0` kernel-log respawn lines
  (`initramfs-init-kleaf:169-173`). **Unknown:** whether the image runs
  `openrc shutdown` from a `::shutdown:` line. This matters for the reboot
  fallback (2.6).
- `/etc/rc.conf` (for example `rc_depend_strict`) is checked by the
  preflight only by a name-mention heuristic
  (`tools/pixel-9a/s5300-s2-preflight-remote.sh:310-319`).

### 1.2 Semantics, one by one

For each item: what the checked-in source says, what upstream OpenRC is
believed to do, what is unknown, and the characterisation case (section
1.3) that settles it.

**O1. `arm` is only allowed while the service is stopped.**
- Source: `exynos-modem.initd:36` declares `extra_stopped_commands="arm"`.
  `arm()` runs the full preflight and then writes the token
  (`exynos-modem.initd:156-161`, `cpif.sh:407-416`). The integration doc
  relies on this ("only while the service is stopped",
  `s5300-cpif-integration.md:152`).
- Upstream belief: `openrc-run` refuses an `extra_stopped_commands` entry
  unless the service state is stopped. A *failed* start leaves the stopped
  state plus a separate `failed` flag. A *crashed* daemon (state `started`,
  process gone) and a stale `starting` state are not stopped.
- Unknown: whether `failed`, `crashed`, `starting`, `stopping` and
  `inactive` each refuse `arm` in the pinned version, and whether `arm`
  takes the service's exclusive lock (if it does not, it could run
  concurrently with a `start`).
- Cases: C-arm-1 to C-arm-4.

**O2. `start_pre` consumes the token, and the order of checks.**
- Source: `start_pre` runs `command -v exynos-modem` first
  (`exynos-modem.initd:164-167`), then `load_cpif_checks`, then
  `cpif_consume_arm`, then the full preflight (`exynos-modem.initd:175`).
  `cpif_consume_arm` refuses a token that is a symlink, not regular, or not
  `root 0600 1 0`, without removing it, and otherwise removes it
  (`cpif.sh:418-431`).
- Finding F1: the token is **not** consumed "first" or "before anything
  else", as `exynos-modem.initd:173-174`, `s5300-cpif-integration.md:157-159`
  and `s5300-s2-active-blockers.md:87-88` say. A missing binary, a wrong
  backend, or a missing library returns before `cpif_consume_arm`, and the
  token survives. Safety still holds, because a later `start` re-runs the
  full preflight after consuming the token, and the S2 preflight treats a
  present token as NO-GO (`s5300-s2-preflight.md:130`). The docs should
  still be corrected in a later slice. The active supervisor must check
  that the token is absent after any `start` attempt, whatever its result.
- Upstream belief: `openrc-run.sh` runs `start_pre`, then `start`, then
  `start_post` in one shell, and exits non-zero at the first failure. The
  default `start()` then calls `start-stop-daemon --start --background
  --make-pidfile` for `command_background=yes` with a `pidfile`
  (`exynos-modem.initd:28-29`).
- Unknown: nothing about the token logic itself. The token test is shell
  and is already covered offline (`s5300-cpif-integration.md:268-269`).
  What is unknown is the OpenRC state afterwards (O3).
- Cases: C-tok-1, C-tok-2.

**O3. Service state after a failed or killed `start_pre`.**
- Source: the S2 preflight reads the state directories
  `started starting stopping inactive wasinactive failed` under
  `/run/openrc/` and calls a service in none of them `stopped`
  (`s5300-s2-preflight-remote.sh:266-279`). Its own `service_started` is
  strict: `started` with no other flag. A `failed`, `starting`, `stopping`
  or `inactive` mounts state is NO-GO or `incomplete`
  (`s5300-s2-preflight.md:210-211`). For a failed mounts start, the only
  safe rule today is C6: no retry, no `stop()`, no manual unmount, reboot
  (`s5300-s2-preflight.md:297-300`).
- Upstream belief: a `start_pre` that returns non-zero leaves the service
  `stopped`. The `failed` flag may be set only while a runlevel is changing,
  so a manual start may leave no `failed` flag at all. If the `openrc-run`
  process is killed by SIGINT or SIGTERM, its signal handler marks the
  service stopped. If it is killed by SIGHUP or SIGKILL (for example a tty
  hangup of the session that ran it), the service may stay in `starting`.
  Any daemon that `start-stop-daemon` already launched keeps running, and
  OpenRC does not record it as started.
- Unknown: all of the above, in the pinned version. The dangerous case is
  "killed after `start-stop-daemon` launched the daemon, before OpenRC
  marked it started". The daemon would then hold `umts_boot0` while
  `rc-service exynos-modem stop` refuses with "not started". This case is
  the main reason the run is driven from a detached supervisor (section 2).
- Cases: C-fail-1, C-kill-1 to C-kill-3.

**O4. `stop` on a crashed pidfile daemon.**
- Source: on CPIF the daemon runs under `start-stop-daemon` with
  `/run/exynos-modem.pid` and no supervisor (`exynos-modem.initd:22-29`,
  `s5300-cpif-integration.md:26`). Nothing in the service defines `stop()`,
  so the OpenRC default applies.
- Upstream belief: `rc-service … status` reports `crashed` when the
  recorded daemon is gone. `start-stop-daemon --stop` that finds no
  matching process still removes the pidfile and the daemon record, and
  exits 0, so the service becomes stopped.
- Unknown: the exit status and the residue (pidfile,
  `/run/openrc/daemons/<svc>`) in the pinned version.
- Cases: C-crash-1, C-crash-2.

**O5. Default stop retry and escalation.**
- Source: neither service sets `retry`, `stopsig` or a `stop()`
  (`exynos-modem.initd:1-182`, `sit-proxy.initd:1-102`). The preflight's
  rule for teardown is "if any step refuses, stop and reboot rather than
  escalate" (`s5300-s2-preflight.md:332-333`).
- Upstream belief: without `retry`, `start-stop-daemon --stop` sends
  SIGTERM, waits a short default period, and fails if the process remains.
  It does not send SIGKILL. A *numeric* `retry=N` expands to
  `SIGTERM/N/SIGKILL/N`, which escalates.
- Unknown: the default schedule and wait in the pinned version, the exit
  status when the process survives, and whether OpenRC then leaves the
  service `started` or `stopping`. Also unknown, and not settled by any
  OpenRC test: how `exynos-modem` handles SIGTERM. Its upstream source is
  not in the repository. The checked-in patches show only `eprintln!`
  logging (for example `0001-exynos-modem-google-kleaf-cpif-backend.patch:595-606`).
- Design consequence: if C-stop-1 shows that the default escalates to
  SIGKILL, the later package slice sets an explicit signal-only
  `retry="SIGTERM/<n>"` in `exynos-modem.initd`. That is a pinned change,
  never a `conf.d` override.
- Cases: C-stop-1, C-stop-2.

**O6. Side effects of `use`.**
- Source: on CPIF, `exynos-modem` has `need ferrix-radio-firmware`,
  `use ferrix-s5300-mounts`, `after udev ferrix-s5300-mounts`, and
  `before sit-proxy modemmanager` (`exynos-modem.initd:50-68`).
  `sit-proxy` has `use ferrix-s5300-mounts exynos-modem`
  (`sit-proxy.initd:35-51`). ModemManager has `use` on the chain and is in
  the default runlevel (`s5300-cpif-integration.md:48-56`). The
  integration doc states as fact that "with `use`, OpenRC no longer stops
  dependents first … stopping `ferrix-s5300-mounts` leaves the daemon
  running" (`s5300-cpif-integration.md:73-80`), but its own verification
  section says OpenRC `use` semantics were never run
  (`s5300-cpif-integration.md:298-300`). Finding F3: that statement is
  unverified.
- Upstream belief: starting a service starts its `need`, and its `use`
  only when the used service is in the current runlevel (with
  `rc_depend_strict=YES`). Stopping a service stops its `needsme`
  dependents, not its `usesme` dependents, but it may wait for a `usesme`
  service that is currently stopping. `restart` restarts dependents that
  were running.
- Unknown: all of it, in the pinned version and with the image's
  `rc.conf`.
- Design consequence: the active run never calls `restart`, never stops
  ModemManager, and never relies on OpenRC to order the teardown. The
  supervisor calls each stop itself, in order, and checks each outcome.
- Cases: C-use-1 to C-use-4.

**O7. The sourced, unpinned `backends.env`.**
- Source: both services source `/etc/ferrix/backends.env` as shell
  (`exynos-modem.initd:3`, `sit-proxy.initd:3`). It is not in the pin list
  (`cpif.sh:56`). The preflight parses only its `FERRIX_TARGET` and
  `FERRIX_S5300_KERNEL_BACKEND` keys and never sources it
  (`s5300-s2-preflight-remote.sh:99-125`).
- Finding F4: any other line in that file runs inside both services. The
  CPIF branch of `exynos-modem.initd` does not assign `supervisor`,
  `retry`, `output_log` or `start_stop_daemon_args`
  (`exynos-modem.initd:22-30`), so a line there could add respawn
  (`supervisor=supervise-daemon`), escalation or other daemon arguments
  without any pinned file changing. The preflight's override checks
  (`s5300-s2-preflight.md:129`) do not look at it. This is not an OpenRC
  question, but the active run depends on it. A later preflight slice
  should require that file to be byte-identical to the composed,
  checked-in content (or pin it). Until then the active design treats its
  extra lines as an open owner question (Q8).

### 1.3 Characterisation plan

Two tiers. Neither starts, arms, stops or names an S5300 service, opens a
CPIF node, mounts anything, or boots the CP.

**Tier A, offline (labelled `mock`).** A pinned Alpine 3.24 aarch64
container with the exact `openrc` and `busybox` package versions read from
the built 1-r3 rootfs's `/lib/apk/db/installed` on the builder. It boots
OpenRC as PID 1 in the container, with no network. This settles O1–O6 at
the userspace level. It is not device evidence.

**Tier B, exact device (lead only, after its own review and decision).**
This repeats only what tier A cannot show: the pinned binaries on the real
image, and the CDC-ACM hangup behaviour (C-hup-1, C-hup-2). It needs a
checked-in, target-scoped workflow with the usual exact-device gate.
Writes are limited to:
- two throwaway scripts, `/etc/init.d/ferrix-rcprobe-a` and
  `/etc/init.d/ferrix-rcprobe-b`, root 0755 (a rootfs write on `userdata`;
  owner question Q7);
- their state under `/run` (tmpfs);
- one evidence directory `/run/ferrix-rcprobe/` (root 0700).

The scripts are removed at the end. The phone is then rebooted, and the
S2 preflight and E1 are re-run to show no residue.

Throwaway service contract (the review checks every line):
- `ferrix-rcprobe-a`: `command=/bin/sleep`, `command_args=3600`,
  `command_background=yes`, `pidfile=/run/ferrix-rcprobe-a.pid`,
  `extra_stopped_commands="arm"`. `arm` writes
  `/run/ferrix-rcprobe.armed` exactly as `cpif_write_arm` does
  (`cpif.sh:407-416`). `start_pre` reads a mode word from
  `/run/ferrix-rcprobe/mode` (`ok`, `fail`, or `hold` = `sleep 60`), then
  consumes the token exactly as `cpif_consume_arm` does (`cpif.sh:418-431`).
  A variant `ignore-term` runs `/bin/sh -c 'trap "" TERM; exec /bin/sleep 3600'`.
  The ignored signal survives `exec`, which models a daemon that does not
  exit on SIGTERM.
- `ferrix-rcprobe-b`: same shape, with `use ferrix-rcprobe-a` and
  `after ferrix-rcprobe-a`.
- Neither has `need`, `provide`, `before` or `after` on any real service,
  and neither is linked into any runlevel. They touch nothing outside the
  paths above. A static test refuses any S5300 name, `/dev`, `/mnt`, `/sys`
  write, `mount`, `losetup` or `blockdev` in them.

Cases. "Expect" is the upstream belief. Each case records the `rc-service`
exit status, the `/run/openrc/<state>/` entries, `/run/openrc/daemons/`,
the pidfile, the token, process liveness and elapsed time.

| Case | Action (on the throwaway service only) | Expect | Settles |
| --- | --- | --- | --- |
| C-arm-1 | `arm` while stopped | token written | O1 |
| C-arm-2 | `arm` while started | refused, no token | O1 |
| C-arm-3 | `arm` after C-fail-1 (failed start) | allowed (stopped, maybe `failed` flag) | O1, O3 |
| C-arm-4 | `arm` while a `hold` start is in `start_pre` | refused (`starting`); also shows whether `arm` waits for the lock | O1 |
| C-tok-1 | `arm`, `start` with `ok` | token gone, `started`, pid alive | O2 |
| C-tok-2 | `start` without `arm` | refused by `start_pre`, stays stopped | O2 |
| C-fail-1 | `arm`, `start` with `fail` | token gone, `stopped`; record whether `failed` is set | O3 |
| C-kill-1 | `arm`, `start` with `hold` from a detached shell; SIGTERM the `openrc-run` process | state `stopped`; no daemon | O3 |
| C-kill-2 | as C-kill-1 with SIGHUP | record: stuck `starting`? | O3 |
| C-kill-3 | as C-kill-1 with SIGKILL after `start-stop-daemon` has launched (`hold` moved to `start_post`) | record: orphan daemon and state | O3 |
| C-crash-1 | `start` `ok`, then `kill -KILL` the `sleep` | `status` reports `crashed` | O4 |
| C-crash-2 | `stop` after C-crash-1 | exit 0, pidfile and daemon record gone, `stopped` | O4 |
| C-stop-1 | `ignore-term`; `stop` with no `retry` | record liveness, elapsed time, exit status and final state; a process that is still alive afterwards shows that no SIGKILL was sent | O5 |
| C-stop-2 | as C-stop-1 with `retry="SIGTERM/5"` set in the script | TERM only, fails after 5 s, service stays `started` (or `stopping`) | O5 |
| C-use-1 | `b` started, `a` started; `stop a` | `b` keeps running | O6 |
| C-use-2 | `a` stopped; `start b` | `a` is **not** started | O6 |
| C-use-3 | `restart a` with `b` started | record whether `b` restarts | O6 |
| C-use-4 | `stop b` while `a` is stopping (`ignore-term`) | record any wait | O6 |
| C-hup-1 | tier B only: from the ACM session, start a foreground `sleep 600` and a `setsid` `sleep 600` that each write an uptime heartbeat to `/run/ferrix-rcprobe/` every second; the lead unplugs USB for 30 s, then reconnects | foreground dies, `setsid` survives | 2 |
| C-hup-2 | tier B only: as C-hup-1, but let `acm-probe.py` time out (it sends Ctrl-C, `acm-probe.py:275-286`) | same | 2 |
| C-ver | tier B: `openrc --version`, apk versions of `openrc` and `busybox`, the `inittab` `::shutdown:` line, `rc_depend_strict`, `/run` tmpfs size, `/sys/fs/pstore` names only | recorded | 1.1, 2.6, 3 |

Failing closed:
- Each assumption in sections 2 and 4 names the case that confirms it. If
  that case is missing, `unknown`, or differs from the belief in the unsafe
  direction (for example C-stop-1 escalates, or C-use-2 starts `a`), the
  active slice is blocked until this design is revised and re-reviewed.
- Tier B stops at the first unexpected state. It leaves the throwaway
  services as they are, removes nothing by force, writes its evidence, and
  asks the lead to reboot. A reboot clears every `/run` artefact. The
  scripts on `/etc/init.d` remain until removed, and they are inert,
  because no runlevel links them.
- Tier B refuses to begin if any S5300 service is not `stopped`, if the arm
  token exists, if `/run/ferrix-rcprobe*` already exists, or if the device
  tree is not the exact tegu board (the same checks as the S2 preflight).

## 2. Disconnect-safe bounded observation

### 2.1 Why the session cannot hold the run

- `acm-probe.py` runs one probe per session, piped to the guest `sh` from
  the interactive shell (`acm-probe.py:12-23`, `56-65`). On a timeout or
  output overrun it sends Ctrl-C to the guest (`acm-probe.py:275-286`). Its
  timeout is at most 3600 s (`acm-probe.py:315`).
- On the rootfs the shell is a `getty` login shell on `ttyGS1`, respawned by
  init (`initramfs-init-kleaf:169-171`). A USB disconnect ends the session
  on the host (`port-closed`, `acm-probe.py:172-173`). In the guest it is
  expected to hang up `ttyGS1` and SIGHUP the session. That is upstream
  `u_serial` behaviour, not in the repo, and C-hup-1 settles it.
- So with a host-held run, a host timeout or USB loss in the wrong second
  delivers SIGINT or SIGHUP to `rc-service exynos-modem start`. That is
  exactly the O3 case C-kill-2/C-kill-3: possibly a daemon holding
  `umts_boot0` that OpenRC does not know is started. A 10-minute
  observation plus teardown also has no owner if the host goes away.

### 2.2 Options

| | A. Host-held session | B. Guest one-shot supervisor (recommended) |
| --- | --- | --- |
| Who runs `arm`/`start` | the ACM session's shell | a detached process with no controlling tty |
| Deadline | host monotonic clock | guest `/proc/uptime` (monotonic; not affected by the wall clock) |
| Host loss or sleep | probe killed or left without an observer; no teardown | supervisor finishes and tears down on its own |
| USB loss | SIGHUP mid-command (O3) | not affected (C-hup-1) |
| Evidence | streamed; lost if the session dies | written to tmpfs, fetched later, bounded |
| New behaviour to review | little | a guest process acting without a host, which blocker 5 calls safety-relevant (`s5300-s2-active-blockers.md:69-74`) |

Rejected variants: an OpenRC service for the supervisor (another pinned
service in the chain, and OpenRC would stop it at shutdown in an unknown
order); `nohup` without `setsid` (keeps the controlling tty); a
supervisor that restarts itself or the daemon.

### 2.3 Recommended shape (option B)

1. **Host, before any session:** run a fresh post-mount preflight; consume
   the #108 record with it (section 4); require the owner's typed phrase.
2. **Launch session** (one `acm-probe.py` run, a few seconds long). Its
   hash-checked body:
   - checks the exact device tree and that the boot identity hash equals
     the record's. The host prepends the salt as the #108 preflight does
     (`#108: s5300-s2-preflight.md` "Salt and redaction design");
   - creates `/run/ferrix-s5300-s2/` with `mkdir` (atomic; the run lock,
     root 0700). It refuses if the directory exists, so at most one
     supervisor runs per boot. It also refuses if the record id is already
     listed in `/run/ferrix-s5300-s2.used` (a second guard behind the
     host's ledger);
   - writes the supervisor script to `/run/ferrix-s5300-s2/sup.sh`,
     re-checks its SHA-256 against a hash in the body, and starts it with
     `setsid sh sup.sh </dev/null >/run/ferrix-s5300-s2/sup.out 2>&1 &`.
     It waits up to 5 s for the supervisor's `launched pid=… uptime=…`
     line, prints that line, and exits. A missing line returns `incomplete`,
     and the host then fetches state instead of launching again;
   - passes the run parameters as fixed lines in the script: `record_id`,
     `start_budget_s` (section 4), and the bounds in 2.4.
3. **Supervisor** (sh, guest-side, no network, no CPIF open). It traps
   HUP/INT to ignore them. It never sends a signal to another process.
   - Phase P (prepare), each step logged: `rc-service exynos-modem
     provision`; `rc-service exynos-modem preflight`; check the start
     budget; `rc-service exynos-modem arm`; check the budget again (if it
     has run out, remove only its own token and stop); `rc-service
     exynos-modem start`. Any failure ends phase P. After `start`, whatever
     its result, it checks that the token is absent (finding F1) and
     records the pidfile.
   - Phase O (observe): every 2 s, read `modem_state` by the library's
     method (`cpif.sh:164`, a sysfs attribute, not a CPIF node), check that
     the daemon pid is alive through `/proc/<pid>/comm` (equal to
     `exynos-modem`), and check `/sys/class/wwan`. Every 10 s, take a
     bounded kernel-log delta and the NV sink metadata (section 3). Every
     30 s, run the opener scan (`cpif_holders`, `cpif.sh:234-290`) and
     require that the only holder is the daemon pid. It writes a heartbeat
     line every poll.
   - Phase T (teardown, 2.5), then a final `summary.kv` and a `done`
     marker.
4. **Fetch sessions** (read-only, any number, any time before reboot).
   Each prints the redacted, allow-listed evidence and the heartbeat age.
   An optional abort request is the only write allowed to a fetch session:
   it creates `/run/ferrix-s5300-s2/abort`, which the supervisor treats as
   a STOP. That write needs its own review. Owner question Q9.

### 2.4 Bounds and STOP criteria

The route's S2 rule is: GO = `modem_state` ONLINE for at least 10 minutes,
no CP crash dump, no AP panic, RFS requests served. STOP = repeated crash,
handshake timeout, or AP reset; capture dmesg and the boot log
(`s5300-route-decision.md:254-261`, table row `:322`).

Proposed bounds (owner question Q5): `start_budget_s` ≤ the record's
remaining life minus 30 s; `T_online` 180 s from `start` to first
`ONLINE`; `T_window` 600 s of continuous `ONLINE`; `T_stop` 30 s per stop
step; `T_offline` 60 s of post-stop `modem_state` sampling; hard cap
`T_total` = 20 min from launch. Evidence is capped at 2 MiB in total.

STOP (ends phase O at once and enters phase T; GO becomes impossible except
for S-done):

| Id | Trigger | Source of the rule |
| --- | --- | --- |
| S-crash | any `modem_state` in `CRASH_RESET`, `CRASH_EXIT`, `WDT_RESET` | stricter than "repeated crash" because of F2; state names from `crates/ferrix-s5300-cpif/src/status.rs:19-50` and `tools/pixel-9a/s5300-cpif-inventory.sh:211-217` |
| S-handshake | no `ONLINE` within `T_online`, or `start` failed | route "handshake timeout" |
| S-left-online | after `ONLINE`, any other state (including `RESET`, `BOOTING`, unrecognised, unreadable) | route GO "stays there" |
| S-daemon | daemon pid gone, or a second `exynos-modem` task | no respawn (`s5300-cpif-integration.md:36-39`) |
| S-daemon-log | a daemon line `boot: CP down, re-booting` or `-resetting the CP before re-boot` (section 3) | F2 |
| S-kernel | a reviewed CPIF or AP crash signature in the kernel-log delta (3.2) | route "no CP crash dump" |
| S-barrier | any read-only-barrier anomaly from `tools/pixel-9a/s5300-mount-proof.sh:570-575` | S0 calibration gate |
| S-holder | a CPIF holder other than the daemon, or an incomplete scan | `cpif.sh:303-312` |
| S-wwan | a `/sys/class/wwan` entry appears | `cpif.sh:169-173` |
| S-budget | evidence cap, or `T_total` reached | bound |
| S-abort | the abort file exists | Q9 |
| S-done | `T_window` of continuous `ONLINE` reached with no other STOP | the only route to a GO |

AP reset cannot be seen by the supervisor, because the supervisor dies with
the AP. The host classifies it on fetch: a changed boot identity, or a
missing `/run/ferrix-s5300-s2/`, means `ap-reset`.

### 2.5 Teardown order (never forced)

This is the order from `s5300-s2-preflight.md:327-334` and
`s5300-cpif-integration.md:75-80`, with each step's outcome verified. The
first step that refuses ends the teardown. The supervisor records
`teardown_refused_at=<step>` and leaves every later step undone.

1. **T1 `sit-proxy`:** must be `stopped` (S2 never starts it). If it is
   not, run `rc-service sit-proxy stop`, then require `stopped`, no
   `sit-proxy` task, and no `/run/sit-proxy.pid`.
2. **T2 `exynos-modem`:** `rc-service exynos-modem stop` with the service's
   own stop schedule (O5). Then poll for up to `T_stop` for: no
   `exynos-modem` task, no pidfile, no daemon record, state `stopped`. The
   supervisor sends no signal of its own.
3. **T3 CP state:** sample `modem_state` every 2 s for `T_offline` and
   record the sequence and the first time it reads `OFFLINE`. Nothing is
   forced, and no reset ioctl is issued (section 3.3).
4. **T4 holders:** `cpif_check_unheld` over every `cpif` class node
   (`cpif.sh:294-312`) must be clean and complete.
5. **T5 mounts:** `rc-service ferrix-s5300-mounts stop`. Its own `stop()`
   verifies, never forces, and leaves BLKROSET set (`mounts.initd:419-451`).
   Then require no `/mnt/vendor` mount and no role-backed loop, using the
   same tests as the pre-mount preflight (`s5300-s2-preflight.md:135`).
6. **T6 residue:** the arm token is absent. `/run/exynos-modem` is left as
   it is, because it holds the NV sinks (evidence, section 3). This makes the
   next preflight NO-GO until a reboot (`s5300-s2-preflight.md:132`).
7. **T7:** write `summary.kv` and `done`.

`ferrix-s5300-mounts` must never be stopped while T2 or T4 has not passed.
A busy unmount inside `stop()` is a refusal, not a cue to retry.

### 2.6 Reboot fallback

- The supervisor never reboots. Rebooting would destroy the tmpfs evidence,
  and an orderly state is no safer for calibration data: every source stays
  a read-only loop over a BLKROSET partition (`mounts.initd:25-37`).
- The lead reboots after fetching the evidence. This is mandatory after any
  teardown refusal, after S-crash, and whenever T3 did not reach `OFFLINE`,
  because the next `arm` needs `OFFLINE` (`cpif.sh:441`,
  `s5300-cpif-integration.md:311-314`). It is recommended after every run
  (Q6).
- Use the ordinary `reboot` through init. If C-ver shows a `::shutdown:`
  `openrc shutdown` line, shutdown will itself try to stop the started
  services. That is acceptable. BusyBox init then signals the remaining
  processes, which the read-only loops make harmless to `efs`/`persist`.
  Never `reboot -f`, sysrq, or a Fastboot write as a teardown shortcut.
- If the device does not come back to the ACM shell within 5 minutes, stop
  and ask the human to hold Volume Down + Power (`AGENTS.md`;
  `s5300-route-decision.md:308-310`). Recovery then follows the existing
  recovery-gated routes.
- After any reboot: re-run E1 and the S2 preflight
  (`s5300-s2-preflight.md:333-334`). A new run needs a new record and a new
  owner decision.

### 2.7 Threat analysis

| Event | What happens | Residual risk | Detection |
| --- | --- | --- | --- |
| Host loss or sleep | supervisor continues and tears down at its deadline | none new | fetch shows `done` |
| USB loss | as host loss; the login shell gets SIGHUP and respawns (`initramfs-init-kleaf:171`) | depends on C-hup-1 | heartbeat continues across the gap |
| AP panic or watchdog reset | all of `/run` is lost, including the token; no S5300 service is in a runlevel, so nothing restarts the CP at boot (`s5300-s2-active-blockers.md:87-90`) | boot loop only if the panic repeats without the CP, which is not expected; evidence is lost except pstore (3.2) | boot identity changed |
| CP crash loop | the daemon re-boots the CP by itself (F2); S-crash/S-daemon-log stop it at the first crash | one CP re-boot attempt may already be under way when the stop arrives | crash state, daemon log |
| Daemon ignores SIGTERM | T2 refuses after `T_stop`; nothing escalates | CP stays up until the lead reboots | `teardown_refused_at=T2` |
| Supervisor crash or OOM kill | no teardown; the daemon keeps running with no observer | as for a refused teardown | stale heartbeat, no `done`; the lead reboots |
| Second launch | refused by the `mkdir` lock and the `.used` list | — | launch output |
| Clock change | the supervisor uses `/proc/uptime` only; the record's host clock is checked on the host before launch | — | — |
| Tmpfs fills | capped writes; S-budget | — | summary |

## 3. Evidence capture

### 3.1 Daemon output without an override

Today the daemon's output is lost. `command_background=yes` without
`output_log`/`error_log` sends it to `/dev/null`
(`s5300-cpif-integration.md:309-310`; `s5300-s2-active-blockers.md:75-80`).
The daemon logs with `eprintln!`, that is to **stderr**
(`0001-exynos-modem-google-kleaf-cpif-backend.patch:535`, `564-570`,
`576`, `596-606`; `0003-exynos-modem-private-scratch-directory.patch:71-75`,
`193`, `212`, `217`). A file in `/etc/conf.d/exynos-modem` would be an
override, which the preflight treats as NO-GO (`s5300-s2-preflight.md:129`).
A line in `backends.env` would be worse (F4).

Proposal, for a later package slice:
- In `exynos-modem.initd`, CPIF branch only, add
  `output_log=/run/exynos-modem.log` and
  `error_log=/run/exynos-modem.log`. Tmpfs, root-only `/run` (checked by
  `cpif_check_run`, `cpif.sh:322-334`). It is outside the scratch directory,
  so the daemon's empty-scratch requirement is unaffected. Upstream
  `start-stop-daemon` opens these files only for `--background`, which this
  service uses. Whether the pinned version supports both variables is
  confirmed by C-ver (tier A).
- Optionally, in the same release, an explicit signal-only `retry` (O5).
- Pin changes: the new `exynos-modem.initd` hash in
  `s5300-cpif-services.sha256` (`:3`); its sha512 and the pin file's sha512
  in `device-google-tegu/APKBUILD` (`:178`, `:196`); and a `pkgrel` bump to
  the release after #107's 1-r4. The library's path list is unchanged
  (`cpif.sh:56`). `cpif_modem_config` is unchanged, because the CLI is the
  same (`exynos-modem.initd:42-48`). The host preflight reads the expected
  release from the recipe (`s5300-s2-preflight.md:63-66`), so a stale image
  shows as image skew. Every earlier #108 record fails with `pins-changed`.
- Preflight changes, in the same slice: `/run/exynos-modem.log` must be
  absent for `go` (a stale log means an earlier run in this boot), in the
  same way as the pidfile and token checks (`s5300-s2-preflight.md:127`,
  `130`).
- Persistence: tmpfs is lost on an AP panic. A persistent path such as
  `/var/log` on `userdata` would survive, but it would retain unredacted
  daemon output on the device (Q4). The handover is built from per-unit
  inputs including the IMEI (`0001-…-backend.patch:552`), and the daemon's
  upstream log lines are not all visible in the repo. So the default is
  tmpfs, and the fetch redacts.

"RFS requests are served" becomes observable as follows:
- from the log: `rfs: serving on …` (`0003-…-scratch-directory.patch:217`)
  and `rfs:   write done fid=…` (`:212`). `rfs: IO write …` appears only in
  verbose mode (`:201-204`), which the service CLI does not enable
  (`exynos-modem.initd:27`). RFS *read* serving has no log line visible in
  the repo; whether one exists upstream is unknown;
- without the log: the NV sinks that patch 0003 writes under
  `/run/exynos-modem` (`s5300-scratch-dir.md`). The supervisor records only
  their names, sizes and mtimes, never their content.

GO needs both: at least one `write done` (or a sink created) and no
`NV sink refused` line (`0003-…:193`). This is a proposal for the owner
(Q5). The route says "served" without defining it.

### 3.2 Kernel-log crash signatures

**Sources not in the repository.** The CPIF driver source (Google
`radio/samsung/s5300` `a8976934`, `kernel-sources.toml:54-57`) and the
factory `cpif.ko` were fetched only into ignored `out/e0-src/` and
`out/e0-factory/` (`s5300-cpif-e0.md:41-58`, `117-126`). The kernel's
CPIF log strings (`mif_err`/`pr_err` call sites in `modem_ctrl_s5100.c`,
`link_device.c`, `bootdump_io_device.c`, `ipc_io_device.c`) **have not
been reviewed** and are not quoted anywhere in the repo. The only CPIF
log facts in the repo are: the factory module carries the version string
`CPIF-20220408R1` and the same handler log strings as the pinned source
(`s5300-cpif-e0.md:99`), and the factory module logs a
"cp boot indicator" line (`s5300-cpif-e0.md:102`). So **no CPIF-specific
kernel signature can be listed as reviewed here.** Slice C4 must produce
that list from the pinned source, with file:line for each string.

What can be reviewed now, from repo sources:

| Class | Signature | Source | Action |
| --- | --- | --- | --- |
| CP state (sysfs, not kernel log) | `CRASH_RESET`, `CRASH_EXIT`, `WDT_RESET` | `status.rs:19-50`; `s5300-cpif-inventory.sh:215` | S-crash |
| CP state | `RESET`, `BOOTING`, `LOADER_DONE`, `NV_REBUILDING`, `SIM_ATTACH`, `SIM_DETACH` after `ONLINE` | `s5300-cpif-inventory.sh:214` | S-left-online |
| Daemon log | `boot: CP down, re-booting` | `0001-…-backend.patch:606` | S-daemon-log |
| Daemon log | `boot: … -resetting the CP before re-boot` | `0001-…-backend.patch:576` | S-daemon-log |
| Daemon log | `boot: crash-reason dump-boot disabled on google-kleaf-cpif; see the kernel log` | `0001-…-backend.patch:564-567`; `s5300-cpif-adapter.md:72` | S-daemon-log (it only appears after a crash) |
| Daemon log | `rfs: NV sink refused, discarding write` | `0003-…-scratch-directory.patch:193` | not a STOP; blocks GO |
| Read-only barrier | `read-only block-device`, `I/O error`, `blk_update_request`, `loop: Write error`, `EXT4-fs error`, `F2FS-fs … error/inconsistent/need fsck`, orphan or recovery messages | `tools/pixel-9a/s5300-mount-proof.sh:570-575` | S-barrier |

Generic AP-side signatures (`Kernel panic`, `Internal error:`, `Unable to
handle kernel`, `BUG:`, `WARNING:` with a CPIF or PCIe frame, SError,
watchdog, and S2MPU or IOMMU faults) are standard Linux and Exynos text.
They are **heuristics** until C4 checks them against the pinned kernel.
Until then a match is recorded and treated as S-kernel, never as a GO
signal.

Capture and redaction:
- The supervisor writes a kernel-log marker at launch (as
  `s5300-mount-proof-remote.sh:324` does) and keeps only lines after it.
  It keeps lines that match the reviewed patterns or contain `cpif`,
  `s5300`, `modem`, `pcie`, `mif` or a panic word, capped at 2,000 lines.
- Redaction happens in the guest, before anything is written to the
  evidence directory: the bootconfig serial value, when present, is
  replaced; runs of 8 or more digits are replaced (IMEI, IMSI, ICCID,
  MSISDN); MAC-like and long hex values are replaced. Fetch output follows
  the preflight's no-identifier rules (`s5300-s2-preflight.md:44-48`) and
  the route's (`s5300-route-decision.md:209-211`). The redaction rules
  need their own offline tests (slice C5).
- AP panic: tmpfs is lost. Two existing paths can still give evidence, and
  neither is new: the Kleaf initramfs mounts pstore
  (`initramfs-init-kleaf:315`) and, on slot b, copies the previous boot's
  pstore to the tail of `vendor_boot_b` once per boot
  (`initramfs-init-kleaf:237-277`). The lead can fetch it through the
  existing `pixel-9a-dev-flash fetch` route (`:238`). Its dmesg loop runs
  for 300 × 2 s from the initramfs (`:278-288`), and initramfs processes
  are not expected to keep working after `switch_root` (`:160-161`), so it
  is **not** expected to cover an S2 window. Whether pstore records survive
  on this board is unknown (C-ver lists names only). A host-side passive
  capture of the `ttyGS0` kernel-log stream (`initramfs-init-kleaf:172`) is
  possible. It would carry unredacted boot logs to the host, so it is an
  owner question (Q4), not part of the default design.

### 3.3 Does the CP return to OFFLINE after the stop?

- Unknown, and nothing in the repo settles it. The daemon's SIGTERM
  behaviour is upstream code that is not in the repo. Closing CPIF nodes
  is not shown anywhere to power the CP off. The ioctl
  `IOCTL_GET_CP_STATUS` maps `RESET` to `OFFLINE`, but the sysfs
  `modem_state` exposes `RESET` as a separate string
  (`s5300-cpif-e0.md:80`; `status.rs:50`). `cpif_check_surface OFFLINE`
  compares the sysfs string exactly (`cpif.sh:164-168`), so a CP left in
  `RESET` fails the next preflight. That is the expected fail-closed
  result.
- Observation: T3 samples sysfs `modem_state` only, read-only, every 2 s
  for `T_offline`. It records the sequence, the time to `OFFLINE` (or
  none), and whether `/sys/class/wwan` or a holder appears. No ioctl, no
  CPIF node open, no reset, nothing forced. The next preflight after the
  reboot independently confirms `OFFLINE` (`cpif.sh:441`).
- Interpretation: `OFFLINE` within `T_offline` is a fact for later S2 runs
  (fewer reboots). Anything else means "reboot required before re-arm",
  which is the current rule (`s5300-cpif-integration.md:311-314`). Neither
  outcome is GO or NO-GO for S2 itself.

## 4. Decision binding (consuming the #108 record)

The #108 contract (branch `issue108-s2-binding`):
- A record exists only for a physical `go` with both identities `ok`, and
  carries `cp_boot_authorized=no` (`#108: s5300-s2-preflight.md:312-335`).
- `consume` renames the record first, then validates, so it is single-use
  (`#108: s5300-s2-preflight.md:376-380`;
  `#108: tools/pixel-9a/s5300-s2-binding.sh:65-73`).
- It is boot-bound: the fresh facts' `boot_identity` must equal the
  record's (`#108: s5300-s2-binding.sh:162-163`).
- It expires exactly 600 s after `created_epoch` on the host clock
  (`#108: tools/pixel-9a/s5300-s2-binding-lib.sh:17`;
  `#108: s5300-s2-binding.sh:118-124`).
- The active workflow must run a fresh preflight in the same invocation,
  consume against it, require a typed phrase naming the target and
  `record_id`, and repeat its own checks before `arm`
  (`#108: s5300-s2-preflight.md:385-392`).

**Resolution: the 600 s expiry bounds the decision → `start` interval, not
the observation.** The S2 GO needs at least 600 s of `ONLINE` after the CP
boots (`s5300-route-decision.md:257-259`). That cannot fit inside a 600 s
record that was created before the owner even typed the decision. The
record answers "are the facts this decision was made on still current?".
That question matters up to the moment the token is consumed and the CP
boot starts. After that, the state is no longer the reviewed pre-boot
state, and the run is governed by the supervisor's STOP rules and
`T_total`, not by the record. Concretely:

1. A post-mount record is needed for the CP boot. A pre-mount record is
   enough only for the separate mounts-start decision
   (`s5300-s2-preflight.md:200-208`; `s5300-s2-active-blockers.md:108-109`).
   Whether that is one decision or two is owner question Q3.
2. The host consumes the record with a fresh post-mount preflight
   (`phase-mismatch` otherwise), then computes
   `start_budget_s = expires_epoch − now − 30`. It refuses to launch if the
   budget is under 60 s.
3. The launch body re-checks in the guest that the boot identity equals the
   record's (closing the gap between the host's preflight session and the
   launch session), and records the `record_id` in
   `/run/ferrix-s5300-s2.used`.
4. The supervisor converts the budget to a `/proc/uptime` deadline. It
   refuses `arm` if the deadline has passed. If the deadline passes between
   `arm` and `start`, it removes only its own token and stops. So the
   token is consumed, and the CP boot begins, strictly before the record's
   expiry.
5. The observation (`T_online` + `T_window`) and teardown run after expiry.
   They are bounded by `T_total`, tied to the boot by the run lock on
   tmpfs, and never extended or restarted.
6. The evidence carries `record_id`, the boot identity hash, the launch
   uptime and every phase's uptime. The host's classification requires the
   same boot identity on every fetch. A changed one is `ap-reset`.
7. A consumed record never authorises a second `start` in the same boot,
   even after a refused phase P. That needs a reboot, a new preflight, a new
   record and a new decision.

## 5. Open questions for the owner

1. **Q1 Autonomous guest teardown.** May the guest supervisor run the
   teardown (2.5) by itself at its deadline or on a STOP, while no host is
   connected? (Recommended: yes. The alternative leaves a booted CP with no
   observer.)
2. **Q2 No automatic reboot.** Accept that the supervisor never reboots,
   and that a refused teardown leaves the CP up until the lead reboots
   after fetching? Or should it run an orderly `reboot` after a refusal?
3. **Q3 Decisions per run.** Is it one decision and record for the mounts
   start and one for the CP boot (recommended), or a single decision
   covering both? Also, is a start budget inside the 600 s record life,
   which in practice means typing the decision within a few minutes of
   the post-mount preflight, acceptable?
4. **Q4 Logs and retention.** Daemon log and kernel-log extract on tmpfs
   only (recommended), or a persistent copy on `userdata`? Should the host
   ever capture the `ttyGS0` kernel-log stream? Which redaction rules,
   and what retention on the host?
5. **Q5 GO/STOP definitions.** STOP on the first CP crash state rather
   than the second (recommended, because of F2)? The bounds `T_online`
   180 s, `T_stop` 30 s, `T_offline` 60 s and `T_total` 20 min? Does
   "RFS requests are served" mean at least one `write done` (or one NV sink)
   and no refused sink?
6. **Q6 Reboot after every run?** Recommended even when T3 reaches
   `OFFLINE`, so that each run starts from a fresh boot.
7. **Q7 Characterisation writes.** Tier B writes two inert scripts to
   `/etc/init.d` on `userdata`. Is that acceptable, or should OpenRC
   semantics come from tier A only, with tier B limited to C-hup and
   C-ver?
8. **Q8 `backends.env`.** Make the active run (and the preflight) require
   the exact composed content of `/etc/ferrix/backends.env`, because both
   services source it as shell (F4)?
9. **Q9 Abort file.** Allow one host-initiated write (the abort file) during
   observation?
10. **Q10 SIGKILL.** Confirm that the daemon is never SIGKILLed as part of
    teardown, and that a numeric `retry` is never used, so a reboot is the
    only escalation.

## 6. Proposed slice plan (each separately reviewed; characterisation first)

| Slice | Content | Touches the phone? |
| --- | --- | --- |
| C0 | This design, owner review | no |
| C1 | Tier A OpenRC characterisation in a pinned container with the image's exact `openrc`/`busybox` versions; throwaway services and all C-arm/C-tok/C-fail/C-kill/C-crash/C-stop/C-use cases; results labelled `mock` | no |
| C2 | Tier B checked-in workflow (throwaway services, C-hup, C-ver), its offline tests, then one lead-run exact-device pass after its own decision; reboot, then E1 and the preflight | yes (lead only; no S5300 service) |
| C3 | Package release after #107's 1-r4: `output_log`/`error_log` (and `retry` if C1/C2 require it) in `exynos-modem.initd`; pins, sha512, `pkgrel`; the preflight's log-absent check; the F1 doc corrections; optionally the F4 `backends.env` check; image rebuild, then a pre-mount `go` on the device | read-only preflight only |
| C4 | Crash-signature review against pinned CPIF `a8976934` source (fetched to ignored `out/`), with file:line for each string, checked in as data; offline classifier tests | no |
| C5 | Supervisor, launch and fetch bodies, host workflow and redaction, offline only against the fake guest and fixtures: every STOP, every teardown refusal, a lost heartbeat, a boot change, a stale lock, and the budget expiring between `arm` and `start` | no |
| C6 | Owner decisions (mounts, CP boot), then the first physical S2 run by the lead | yes |

#110 (mounts rollback, contract C1–C5 in `s5300-s2-preflight.md:277-296`)
is independent of these slices. Until it lands, rule C6 (reboot after a
failed mounts start) applies to phase P.

## Findings that differ from existing documents

- **F1** The token is not consumed first in `start_pre`. Two checks run
  before it (`exynos-modem.initd:164-167`, `175`), contrary to
  `exynos-modem.initd:173-174`, `s5300-cpif-integration.md:157-159` and
  `s5300-s2-active-blockers.md:87-88`. It is fail-closed, but the
  documents are inaccurate.
- **F2** "No respawn" and "never retry automatically"
  (`s5300-cpif-integration.md:36-39`; `s5300-s2-preflight.md:325-326`)
  apply to OpenRC only. The daemon itself loops CP re-boots after a crash
  (`0001-…-backend.patch:601-606`; `s5300-cpif-adapter.md:70`). The
  repo does not show whether that loop is bounded.
- **F3** The `use` stop behaviour in `s5300-cpif-integration.md:73-80` is
  stated as fact but has never been observed
  (`s5300-cpif-integration.md:298-300`).
- **F4** `backends.env` is sourced by both services but is neither pinned
  nor fully checked (`exynos-modem.initd:3`; `cpif.sh:56`;
  `s5300-s2-preflight-remote.sh:99-125`). This contradicts the override
  coverage implied by `s5300-s2-preflight.md:129` and
  `s5300-cpif-integration.md:112-113`.
- **F5** The sysfs `RESET` state is distinct from `OFFLINE`, although the
  ioctl folds it into `OFFLINE` (`s5300-cpif-e0.md:80`; `status.rs:50`).
  Any reasoning that "RESET counts as OFFLINE" applies only to the ioctl,
  not to the preflight.
