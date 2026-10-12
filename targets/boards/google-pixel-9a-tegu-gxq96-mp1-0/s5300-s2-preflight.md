# S5300 S2 read-only preflight (issue #92)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.

This is a read-only slice of issue #92 that must land before any manual CP
boot. It adds a checked-in, bounded preflight that collects the facts a
reviewer needs for a later one-shot S2 manual CP boot, and returns a
machine-readable GO/NO-GO for those facts. **It does not boot the CP, start
or arm a service, mount anything, or implement any active S2 step.** The owner
has not authorised a CP boot. The implementation pass was offline; the PM
subsequently ran the read-only preflight once on the exact phone. No mount,
service start, CPIF node open, or CP boot was performed.

Route context: [`s5300-route-decision.md`](s5300-route-decision.md) (stage 2),
[`s5300-cpif-integration.md`](s5300-cpif-integration.md) (services, library,
and opener scan).

## Running it

```sh
just pixel-9a-s5300-s2-preflight        # tools/pixel-9a/s5300-s2-preflight.sh
just pixel-9a-s5300-s2-preflight-test   # offline tests only
```

The workflow has one mode, and that mode is read-only. It takes no
arguments, so a mount or arm argument cannot be passed. `FERRIX_SHELL`
selects the host CDC-ACM node, which is never printed or retained.
`FERRIX_PIXEL9A_S5300_S2_EVIDENCE=mock` labels stubbed runs, and a mock run
can never report a physical gate. `FERRIX_PIXEL9A_ACM_TIMEOUT` defaults to
180 s. The first read-only device run completed within this bound; later
image and service states may differ.

Outputs go, mode 0600, to the ignored `out/pixel-9a/s5300-s2-preflight/`:

- `preflight-<stamp>.txt` and `preflight-latest.txt`: the `key=value`
  classification, the host-artifact section, and the raw probe output.
- `summary-<stamp>.json` and `summary-latest.json`: `preflight_result`,
  `preflight_gate`, `preflight_phase`, `device_blockers`, `image_skew`,
  `host_blockers`, `unknown_facts`, `deferred_until_mounts`,
  `deferred_to_active_workflow`, `cp_nodes_opened: false`,
  `state_changes: "none"`, `cp_boot_authorized: false`, and
  `active_s2_workflow: "not-implemented"`.

These files contain no serial, port name, partition label, block device
name, file name under `/mnt/vendor`, PID, kernel log, IMEI/SIM data, or
per-unit bytes. CPIF holder task names are reduced to the known service
names or `other`; any unexpected CPIF class entry is also `other`.
Package-file hashes and versions are included.

Exit status: 0 `go`, 8 `no-go`, 9 `incomplete`; 2 usage; 3 transport or guest
failure; 4 not the exact tegu device tree; 5 malformed or incomplete probe
output; 6 a host input was refused before the port was opened.

## What runs where

**Host, before the port is opened** (exit 6 on failure):

- The checked-in `ferrix-s5300-cpif.sh` must match its own package pin in
  `s5300-cpif-services.sha256`. This guarantees that the guest receives the
  reviewed issue #92 alias-aware library.
- The checked-in `ferrix-s5300-mounts.initd` must still be the #34-accepted
  `b90d3b41…f74b`, and the library must pin that same hash.
- The pin list must name each of the seven pinned files exactly once. The
  expected releases are read from the recipes: `device-google-tegu`
  `1-r3`, `modemmanager-samsung-sit` `1.25.95_git20260731-r3`, and
  `exynos-modem` `0.1.0_git20260731-r3`.

**Host recovery artifact evidence.** This is a fact about this host, never
about the device, and the transcript keeps it in its own section. The
manifest `identity` gate must be `passed` and `recovery_procedure` must be
`tested`. The pinned factory archive named in `recovery.pins` must be present
under the ignored `recovery/`, be a regular file, and match the pinned size
and SHA-256 (`host_recovery_factory_archive=verified`). Anything else is a
`host_blockers` entry. The workflow does not run or test a restore, and it
cannot show that the restore would work on the device today.

**Guest, one CDC-ACM session.** `acm-probe.py` sends one file made of the
checked-in library followed by `tools/pixel-9a/s5300-s2-preflight-remote.sh`.
The guest checks its SHA-256 before running it. The body:

- refuses to run without the library (exit 5) or on another device tree
  (exit 4);
- replaces the library's `cpif_write_arm`, `cpif_consume_arm`,
  `cpif_provision_scratch`, and `checkpath` with refusals, and never calls
  `cpif_modem_preflight`;
- reaches the system only through the library roots (`cpif_fs`, `cpif_proc`,
  `cpif_sys`, `cpif_dev`, `cpif_run`). It reaches `/dev` only through the
  library's file tests and `stat(2)`, and open files only through `stat(2)` of
  `/proc/<pid>/task/<tid>/fd` links;
- parses `backends.env` without sourcing it, and reads only the
  `androidboot.slot_suffix` and `androidboot.revision` keys from
  bootconfig/cmdline;
- runs the installed `exynos-modem --print-config` only if the apk database
  names the pinned release;
- runs the accepted service's `verify_own_mount`, through the library's
  `cpif_check_sources`, only if `ferrix-s5300-mounts` is **already** started.
  This reads sysfs, mountinfo, and util-linux `blkid -p` types, and checks
  per-unit inputs by `stat(2)` only.

## Check matrix

"Now" means the check can pass on the current installed image. "New image"
means it cannot pass until an image rebuilt with `device-google-tegu` 1-r3
is flashed. "After mounts" means it is evaluated only after an
owner-approved manual `rc-service ferrix-s5300-mounts start`. "Active" means
only the later S2 workflow can observe it.

| Check | Fact | GO condition | When it can pass |
| --- | --- | --- | --- |
| Device tree | model, compatible | exact tegu MP | now (exit 4 otherwise) |
| Kernel | `uname -r` | `6.1.124-android14-11-ga4662a84a15f` | now |
| Root | `id -u` | 0 | now |
| Slot | bootconfig, then cmdline | exactly one of `_a`/`_b` | now |
| Hardware revision | bootconfig `androidboot.revision` | `MP1.0`; absent → `incomplete` | **unknown**: the guest has never been read for this key (see limits) |
| PID namespace | `/proc/self/ns/pid` | initial namespace | now |
| Backend | `backends.env` (parsed) | target exact, `google-kleaf-cpif` | now (composer sets it) |
| Library `cpif_check_target` | — | pass | now |
| Installed files vs checked-in | SHA-256 of the 7 pinned paths | all equal to the checked-in pins | **new image**: `s5300-cpif.sh` differs on 1-r2 |
| On-device pins | `s5300-cpif-services.sha256` | `cpif_check_package` pass | now (1-r2 is self-consistent) |
| Releases | apk db | device 1-r3, modem r3, MM r3 | **new image**: device package is 1-r2 |
| Mounts service | installed SHA-256 | #34-accepted `b90d3b41…` | now |
| CLI | `exynos-modem --print-config` | exact reviewed text | now (binary is r3) |
| Surface | modules, platform, driver, `modem_state`, WWAN | three modules `Live`, one `cp_interface`, `OFFLINE`, no WWAN | now |
| Nodes | twelve E1 nodes | each passes `cpif_check_node` | now |
| Openers | every `cpif` class entry, alias-aware, every thread | `clean`; `held`, `incomplete`, or `not-run` is NO-GO | now (the checked-in library is sent to the guest) |
| S5300 daemons | task names | none running | now |
| Services | `/run/openrc/<state>/` | `exynos-modem` and `sit-proxy` stopped; no stale daemon record or pidfile; mounts `stopped` (pre-mount) or `started` (post-mount) | now |
| Runlevels | `/etc/runlevels/*/<svc>[.*]` | none for the three S5300 services | now |
| OpenRC overrides | conf.d, instances, rc.conf, local.d, udev rules | none; only `modemmanager_opts` in `conf.d/modemmanager` | now |
| Arm token | `/run/ferrix-s5300-cpif.armed` | absent | now |
| `/run` | `cpif_check_run` | tmpfs, root-owned, not group/other-writable | now |
| Scratch | `/run/exynos-modem` | absent (not yet provisioned), or root:root 0700 and confirmed empty; an unreadable listing blocks | now (provisioning is an active step) |
| Firmware dirs | uecapconfig, carrierconfig | present | now |
| Partitions | uevent `PARTNAME`, sysfs | each role unique; no direct mount | now |
| Stale/partial mounts (pre-mount) | mountinfo, loop `backing_file`, holders | no `/mnt/vendor` mount, no role-backed loop, no holder | now |
| Partial mounts (post-mount) | same | each destination exactly one read-only loop mount; one loop per role; BLKROSET set | **after mounts** |
| Sources | `cpif_check_sources` | pass | **after mounts** |
| Scratch provision, arm, token use, `start_pre` re-check, CP boot, logs, ONLINE ≥10 min, teardown | — | — | **active** (not implemented) |

`partition_ro_flag_since_boot=yes` is informational. BLKROSET stays set
until reboot after any earlier mount in this boot. It only tightens the
barrier, but it shows that this boot is not fresh.

### Exact-device result on the rebuilt 1-r3 image (2026-10-12)

On 2026-10-12 the lead rebuilt the native rootfs from reviewed source `0032a6b`
(attempt `20261012T062930Z`, `device-google-tegu` 1-r3, `exynos-modem` and
`modemmanager-samsung-sit` r3). The rootfs image and tar hashes matched the
manifest and `e2fsck -fn` was clean. The root-only files that the
unprivileged first tar/mke2fs attempt logged as `Permission denied` are
present with root ownership in the final image, from the script's `sudo -n`
retry. The installed `s5300-cpif.sh` equals the checked-in
`06dc8032…8347`. The development bundle was composed with the unchanged
pinned Kleaf kernel (6.1.124, kernel manifest `281f5e35…0671`). Every boot
partition image is byte-identical to the 1-r2 bundle; only `userdata`
changed (`7a248f5a…1152`). All seven artifact hashes were re-verified after
transfer. The bundle was flashed to slot B plus `userdata` through
`just pixel-9a-dev-flash`, which checked the recovery invariant (pinned
factory archive verified; stock bootloader and baseband on the device).
Ferrix reached OpenRC default 77 s after reboot.

The read-only preflight from `463d8ec` then returned
`preflight_result=go` in phase `pre-mount`, with `image_skew=none`,
`device_blockers=none`, `host_blockers=none` and `unknown_facts=none`.
Bootconfig reported `MP1.0` and slot B. All seven pinned files matched both
the checked-in and on-device pins. CP was `OFFLINE` with no WWAN device.
All twelve E1 nodes passed, and the opener scan was complete and clean
across 23 CPIF class nodes. The three S5300 services were stopped with no
runlevel link, override or arm token. The scratch directory was absent,
because no S5300 service has run since boot. All four source roles were
unique, unmounted, unheld and not loop-backed, and
`partition_ro_flag_since_boot=no`. Source and NV checks stay deferred until
a mounts start. The run reported `cp_boot_authorized=no`.

This GO is a pre-mount fact only. It does not authorise the mounts start or
a CP boot. The owner has not given a decision for either, and
[`s5300-s2-active-blockers.md`](s5300-s2-active-blockers.md) still blocks
an active S2 workflow. The transcript and summary are ignored under
`out/pixel-9a/s5300-s2-preflight/` (`20261012T073650Z`).

### Earlier exact-device result on the 1-r2 image

The installed bundle was built from `1b70e3a` (`device-google-tegu` 1-r2).
Between that commit and this branch, the only pinned file that changed is
`ferrix-s5300-cpif.sh`. The read-only run returned `preflight_result=no-go`
with exactly `image_skew=file-s5300-cpif.sh,pkg-device-google-tegu` and no
other blocker or unknown. Bootconfig reported `MP1.0` and slot B. CP was
`OFFLINE`, the host factory archive was hash/size verified, all twelve E1
nodes passed, and the opener scan was complete and clean across 23 CPIF
class nodes (eleven additional nodes were counted, never named in output).
The S5300 services were stopped with no runlevel link or arm token; scratch
was root-owned, mode 0700 and empty on tmpfs; all four source roles were
unique, unmounted, unheld and not loop-backed. Source verification remains
deferred. The aggregate transcript is ignored under
`out/pixel-9a/s5300-s2-preflight/`. A rebuilt and reflashed 1-r3 image is
required before the preflight can return `go`.

### Phases

1. **pre-mount** (mounts service stopped): `go` means every live fact,
   image check, and host recovery check passes, and the source checks are
   listed in `deferred_until_mounts`. The only step this unlocks is a
   separately approved manual `rc-service ferrix-s5300-mounts start`. That
   step is outside this workflow, and the lead agent runs it.
2. **post-mount** (mounts service started): run the preflight again. `go`
   now also needs all four single read-only loop mounts and a passing
   `cpif_check_sources`. That is the input to the S2 owner decision. It is
   still not a CP boot authorisation.

A `failed`/`starting`/`stopping`/`inactive` mounts state, or an unreadable
OpenRC state directory, is NO-GO or `incomplete`.

## Limits

- Snapshot only. Every check, including the opener scan, describes one
  moment, so the active workflow must repeat its own checks immediately
  before `arm` and again in `start_pre` (the library already does this). The
  scan does not see SCM_RIGHTS fds in flight, io_uring fixed files, or kernel
  references (integration doc).
- The OpenRC state comes from `/run/openrc/<state>/` directories rather than
  from `rc-status`. The read-only exact-device run found the expected service
  states, but a missing state directory still reports `unknown` and blocks.
- The override checks are heuristics. They cover conf.d content, multiplexed
  instances, and `rc.conf`/`local.d`/`/etc/udev/rules.d` lines that name the
  S5300 services or CPIF nodes. They do not prove that no other mechanism
  starts or reconfigures a service.
- Bootconfig carried `androidboot.revision=MP1.0` in one exact-device run.
  The preflight still returns `incomplete` if that key is absent on a later
  run; it does not substitute prior Fastboot identity evidence.
- The offline tests run the real composed probe with the host `/bin/sh`,
  Python stand-ins for `stat`/`uname`, and a FIFO fixture. The exact-device
  run exercised the guest BusyBox tools, real OpenRC state directories and
  CDC-ACM link for this read-only path. It did not exercise active S2.

## Review: partial-mount failure path of `ferrix-s5300-mounts`

The accepted service (`b90d3b41…`) is unchanged in this slice. Changing it
needs a fresh #34 safety review, a new library and NV-workflow pin, and a
re-run of the per-role proofs.

Findings, from the source and offline stubs:

1. **No rollback on a partial start.** `start()` mounts `modem_img`, `efs`,
   `persist`, and `modem_userdata` in that order, and returns 1 at the first
   failure. Destinations it already mounted stay mounted. They are still
   verified read-only loops with recovery suppressed, over partitions with
   BLKROSET set. `tools/pixel-9a/tests/s5300-mounts-service.sh` now pins this
   behaviour: a `persist` of unknown type leaves `modem_img` and `efs`
   mounted, with no unmount and no detach.
2. **No checked-in teardown for that state.** After a failed start, OpenRC
   records the service as stopped, and by OpenRC semantics
   `rc-service ferrix-s5300-mounts stop` does not run `stop()` for a stopped
   service. This behaviour has not been observed on the device. The
   characterisation test shows that `stop()` itself would remove exactly the
   partial state without clearing BLKROSET, but nothing checked in invokes it
   in that state.
3. **Retry adopts.** A later `start` accepts any existing destination that
   passes `verify_mount`, and attaches fresh loops for the rest. Loops that a
   failed attempt detached keep `ro=1` and are refused, so a retry can run
   out of clean loops (existing test).
4. Inside `mount_one`, failures after the attach detach the loop. A failure
   after the mount unmounts and detaches it. If that unmount fails, the
   unconfirmed mount stays, and only `eerror` reports it. BLKROSET is never
   cleared, by design. The exit status of `checkpath -d` on the destination
   is not checked. This is benign, because the following mount fails without
   the directory.

**Safety assessment.** The leftover state does not open a new write path,
because every leftover mount is still a verified read-only loop. The risk is
that a later S2 step runs on an unknown, partly mounted state. The CPIF path
already fails closed, because the library requires `service_started
ferrix-s5300-mounts`. This preflight also reports the state explicitly:
`stale-mount-<dest>`, `stale-loop-<role>`, and `stale-vendor-mount` in
pre-mount, and `partial-mount-<dest>`/`partial-mount-loop-<role>` in
post-mount.

**Proposed narrow fail-safe contract** (for a later, separately reviewed
change; **not implemented**):

- C1. `start()` records only the destinations that this invocation newly
  mounted. Adopted, pre-existing mounts are never recorded.
- C2. On any later failure, it rolls back only those destinations, in
  reverse order. Each one goes through the same `verify_own_mount` before
  `umount --` and the same `detach_loop` after the unmount as `stop()`. It
  never uses `-f`/`-l`, never touches an adopted mount, and never clears
  BLKROSET.
- C3. The first rollback step that fails ends the rollback. `start` still
  returns 1, and the remaining state is left for this preflight to report.
  Nothing is forced.
- C4. No `mark_service_*` call makes a partial start look started.
- C5. Required tests: a failure injected at each of the four destinations and
  at each step (type probe, BLKROSET, attach, `verify_loop`, mount,
  `verify_mount`) reverts exactly the mounts that invocation created, in
  reverse order. An adopted mount survives. A busy rollback forces nothing
  and returns 1. BLKROSET stays set on every partition touched. Every
  existing test still passes.
- C6. Until such a change is accepted, the operating rule is: after any
  failed `ferrix-s5300-mounts start`, do not retry, do not call `stop()`, and
  do not unmount or detach by hand. Reboot instead. A reboot clears `/run`,
  the loops, and BLKROSET, and writes nothing. Then re-run this preflight.

## Remaining requirements for the active S2 workflow (later PR, not implemented)

An offline review found that the current contracts are not yet sufficient
for a fail-closed active command; see
[`s5300-s2-active-blockers.md`](s5300-s2-active-blockers.md).

- Done 2026-10-12: a rebuilt image from a reviewed commit with
  `device-google-tegu` 1-r3, flashed through the recovery-gated
  `just pixel-9a-dev-flash` by the lead, then a pre-mount `go` from this
  preflight on the exact device (see the result above). Re-run the
  preflight after any reflash or reboot.
- A fresh, recorded owner decision for one S2 run, a per-run typed phrase
  naming the exact target, and refusal of an ambiguous or second device.
- The sequence, with each step bounded and logged: preflight (pre-mount) →
  manual mounts start → preflight (post-mount) →
  `rc-service exynos-modem provision` → `preflight` → `arm` → `start`. The
  checks must repeat with as little delay as possible before `arm`.
- Observation: periodic read-only polls of `modem_state`, daemon liveness,
  and a boot-ID change (AP reset), for at least the 10-minute ONLINE window
  (route decision S2 GO). Also collect the daemon output (the service sets no
  `output_log`) and a bounded, redacted kernel log covering CP crash dumps.
  No IMEI, ICCID, IMSI, MSISDN, or cell IDs.
- STOP criteria from the route decision: a repeated crash, a handshake
  timeout, or an AP reset. Capture evidence, then tear down. Never retry
  automatically, because the service has no respawn by design.
- Teardown, in this order and never forced: `sit-proxy` (not started in S2),
  then `exynos-modem` stop. Record `modem_state` after the stop without
  forcing it; whether the CP returns to `OFFLINE` is unknown. Then require no
  CPIF holder, run `ferrix-s5300-mounts` stop with its own verified,
  non-forced unmount, require no `/mnt/vendor` mount and no role-backed
  loop, and require the arm token to be absent. If any step refuses, stop and
  reboot rather than escalate. After any teardown or reboot, re-run E1 and
  this preflight.
- Panic or boot-loop handling: the start stays manual-only and the token
  lives on tmpfs, so a panic costs at most one reboot. A device that does not
  return needs the human to hold Volume Down + Power (AGENTS.md).
- `sit-proxy`, ModemManager binding (S3), and anything after S2 stay out of
  scope.

## Verification

`just pixel-9a-s5300-s2-preflight-test`
(`tools/pixel-9a/tests/s5300-s2-preflight.sh`) runs three layers:

- **static**: the body names no device, service, mount, or write verb, and
  never redirects into a file. It replaces every state-changing helper. It
  uses CPIF `/dev` paths only in the CLI and config text, which match
  `exynos-modem.initd` exactly. It reaches the system only through the
  library roots. The binary runs only behind the release check, and the
  source checks only behind `service_started`. The host has exactly one
  transport session and sends exactly library plus body. Its pins match the
  library, it names the library's pinned files, and the just recipes exist.
- **real composed probe** against a FIFO fixture: each case runs with a 15 s
  bound, takes a full before/after snapshot of every fixture file (no file
  created, changed, or removed), and checks that the binary ran only with
  `--print-config` and that no serial or command-line value was printed.
  Cases: clean pre-mount; a holder through an alias, through a non-leader
  thread, and on an extra class node; an unlistable fd directory; another
  PID namespace; an ONLINE CP; a stale earlier run (token, pidfile, scratch
  entry, started service, live daemon); every OpenRC override kind; a
  partial mount after a failed start; a direct partition mount; post-mount
  with the verify path run for all four destinations; post-mount with short
  NV files; an installed 1-r2 image (self-consistent on the device but
  skewed); a stale `exynos-modem` release (binary not run); and the WWAN
  backend with no slot.
- **host workflow** through the real CDC-ACM transport and the fake guest:
  pre-mount and post-mount `go`; physical labelling; 20 device NO-GO cases,
  including image skew, a held node, an incomplete scan, the token, a
  runlevel link, conf.d and rc.conf overrides, stale and partial mounts, a
  started service, a pidfile, scratch state, the PID namespace, slot,
  kernel, backend, hardware revision, and a failed mounts state; an absent
  hardware revision → `incomplete`; a missing or tampered recovery archive
  (host blocker only); malformed, duplicate, missing, out-of-set, and
  incomplete output refused without a result, and an unexpected value
  neither echoed nor retained; device-tree, missing-library, guest-error,
  and unreachable refusals; usage refusals; and host-gate refusals for an
  edited library, an edited mounts service, or a missing body, all before
  any file or session.

`tools/pixel-9a/tests/s5300-mounts-service.sh` adds the partial-start
characterisation above.

Run on 2026-10-12 (macOS arm64 host, offline): `s5300-s2-preflight.sh`
passed all 238 checks. `s5300-mounts-service.sh` (with the new partial-start
characterisation), `s5300-cpif-services.sh` (108 checks), and
`s5300-cpif-inventory.sh` (144 checks) also passed, and `git diff --check`
was clean. Three deliberate regressions, each in a throwaway copy, were
caught:

- a file write appended to the guest body failed the static redirect check
  and every snapshot check;
- a one-byte read of a CPIF node failed the static `/dev` guard, and every
  probe case hung on the FIFO;
- dropping the host's package-release skew check failed the skew case.

**Not run:** anything on the phone, busybox, real OpenRC, or the real
CDC-ACM link.
