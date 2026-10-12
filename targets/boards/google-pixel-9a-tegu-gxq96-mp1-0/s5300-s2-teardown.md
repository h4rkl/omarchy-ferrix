# S5300 S2 teardown: loop detach and post-stop CP state (issues #117, #116)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.
Parent: #92, under the #109 design
([`s5300-s2-active-design.md`](s5300-s2-active-design.md)).

> **This is an offline, source-only analysis plus a DRAFT design for owner
> review. Nothing in it is implemented, and it authorises nothing.** It
> changes no init script, pin, APKBUILD, `capabilities.toml` row or evidence
> value. No phone, USB link, ADB, Fastboot, builder or CPIF node was touched
> while writing it. `[telephony]` stays `unsupported`. It does not answer the
> #109 owner questions Q1–Q10 ([`s5300-s2-active-design.md`](s5300-s2-active-design.md#5-open-questions-for-the-owner)).
> Any code change it proposes needs its own review, and any device step
> needs its own owner decision.

The device observations being explained are in
[`s5300-s2-preflight.md`](s5300-s2-preflight.md#exact-device-s2-active-cp-boot-on-the-1-r3-image-2026-10-12)
(`physical:google-pixel-9a-tegu-gxq96-mp1-0`, 2026-10-12, 1-r3 image):

1. `rc-service ferrix-s5300-mounts stop` returned 0 and left no
   `/mnt/vendor/*` mount. All four role loops stayed attached with
   `autoclear=1`, `ro=1`, no holders and no mount until a reboot (#117).
2. `rc-service exynos-modem stop` returned 0 and the daemon exited. The CP
   stayed `ONLINE` until a reboot (#116).

## Sources read

Every source below was fetched into the ignored `out/` of a worktree. None
of it is committed.

| Source | Pin | Where the pin comes from |
| --- | --- | --- |
| Android common kernel (GKI) | `a4662a84a15f38b99cf597354edccf75988852de`; its `Makefile` reads `6.1.124` | `kernel-sources.toml` `gki-common`; the running kernel string `6.1.124-android14-11-ga4662a84a15f` in the S2 record |
| Google S5300 CPIF | `a8976934fe7f396627f9900958eb1457a74ebcca` | `kernel-sources.toml` `modem-s5300` |
| `exynos-modem` | archive `634e8eb4…`, SHA-512 matched the APKBUILD; then patches 0001–0003 applied with `patch -p1` | `pmos/exynos-modem/APKBUILD` |
| util-linux | 2.40.2 (`lib/loopdev.c`, `sys-utils/losetup.c`) | **not pinned**: the version the board README already cites. The image's util-linux is from a moving pmOS branch |
| BusyBox | 1.36.1 (`util-linux/umount.c`, `libbb/loop.c`) | **not pinned**: the image's BusyBox version is not recorded (#109 C-ver) |
| Alpine `util-linux` APKBUILD | `aports` `master` (2.42.4) | packaging layout only |

**Verification status.** Every `file:line` below was read in the fetched pinned source, unless it is marked **unverified**. The holder identity (H1, H2) and the LXC/Waydroid context are hypotheses, not source facts.

Citation forms: `loop.c:N` is `drivers/block/loop.c` in the GKI pin, and the
other kernel paths are also in that pin. `cpif/<file>:N` is the CPIF pin.
`s5300.rs:N` and `main.rs:N` are
`crates/exynos-modem/src/{boot/s5300.rs,main.rs}` **after** patches
0001–0003. `mounts.initd`, `exynos-modem.initd` and `cpif.sh` are the files
in `pmos/device-google-tegu/`.

## #117: why `losetup -d` returned 0 and left the loop attached

### Verdict

The **mechanism** part of the hypothesis is **confirmed from source**. The
**holder** is **not identified**: source analysis rules some candidates out,
but only a device observation can name the one that was there.

1. `LOOP_CLR_FD` goes to `loop_clr_fd()` (`loop.c:1557-1558`). If the disk
   has more than one opener, it sets `LO_FLAGS_AUTOCLEAR` and **returns 0
   without unbinding** (`loop.c:1244-1247`). The comment above it says this
   is deliberate, so that a udev `blkid` racing with `losetup -d` does not
   cause `EBUSY` (`loop.c:1234-1243`). The opener count is
   `disk->part0->bd_openers` (`include/linux/blkdev.h:228-231`), and it
   includes the fd that `losetup` itself opened. So "more than one" means at
   least one opener other than `losetup`.
2. In 6.1.124 no other path returns 0 from `LOOP_CLR_FD` and leaves the loop
   bound. The other outcomes are a lock error (`:1227-1229`), `-ENXIO` for a
   loop that is not bound (`:1230-1233`), or a synchronous
   `__loop_clr_fd()` (`:1249-1253`). That last one removes the `loop/`
   sysfs group, `backing_file` included, before the ioctl returns
   (`loop_sysfs_exit`, `:1160`). So re-reading `backing_file` straight after
   `losetup -d` is an exact test. No delay is needed.
3. A deferred detach happens only on the last close. `lo_release()` returns
   early while `disk_openers > 0` (`loop.c:1725-1731`). Otherwise it clears a
   bound loop that has `AUTOCLEAR` (`:1733-1741`). It runs from
   `blkdev_put_whole()` after the decrement (`block/bdev.c:697-703`). A loop
   that stays bound for minutes therefore had an opener that stayed open for
   minutes.
4. `autoclear=1` is the deferred path's fingerprint
   (`loop_attr_autoclear_show`, `loop.c:696-700`). Nothing in the service
   asks for autoclear. util-linux `losetup` sets only `READ_ONLY`,
   `PARTSCAN` and `DIRECT_IO` from its options (`losetup.c:762,812,822`).
   libmount's autoclear is for `mount -o loop` (`loopdev.c:1288`), which the
   service does not use: it mounts the loop node directly
   (`mounts.initd:371`). BusyBox `umount` issues `LOOP_CLR_FD` only with
   `-d` (`umount.c:88,215-216`, `loop.c:68-79`), and the service passes no
   `-d` (`mounts.initd:447`). **Limit:** nobody read `autoclear` right after
   the attach on the device, so "it was 0 before `stop`" is an inference.
   The characterisation below closes that gap.
5. `stop()` treats exit 0 as success (`mounts.initd:302-313`). It returns 0
   even though the loop is still bound.

### Holder candidates

| # | Candidate | Assessment | Source |
| --- | --- | --- | --- |
| R1 | The filesystem's own block-device reference in the service's mount namespace ("mount has its own fd") | **Ruled out.** A non-forced `umount(2)` succeeds only if `propagate_mount_busy()` finds no extra reference (`fs/namespace.c:1710-1716`). The last `mntput` queues `cleanup_mnt` as `TWA_RESUME` task work (`fs/namespace.c:1260-1266`). **Unverified in the fetched files:** that this task work runs before the syscall returns to user space is general kernel behaviour, not read here from `kernel/task_work.c` or the return-to-user path. That calls `deactivate_super()` → `kill_sb` → `kill_block_super()` → `blkdev_put()` (`fs/namespace.c:1166-1188`; `fs/super.c:328-334,1466-1476`). So when `umount` exits 0, this namespace's mount no longer holds the loop open | kernel |
| R2 | `losetup`'s own fd | **Ruled out.** It is counted, and the threshold is `> 1`. util-linux opens one fd for the ioctl (`loopdev.c:344-347,1623-1640`) | kernel, util-linux |
| R3 | A file open on the filesystem by a process in the same namespace (daemon thread, shell `cd`, mmap) | **Ruled out** by the same busy check as R1: `umount` would have failed with `EBUSY`, and `stop()` would have reported it (`mounts.initd:447`) | kernel |
| R4 | A short udev or `blkid` probe that raced with `losetup -d` | **Unlikely as the whole story.** It fits `autoclear=1`, but the loop would have detached as soon as the probe closed (point 3). The loops were still bound when checked after the teardown and stayed bound until the reboot (teardown 08:49, next preflight 08:53 in the S2 record). The service's own `blkid -p` runs and exits inside `verify_own_mount` before the `umount`, not after it | kernel |
| H1 | **A copy of the mount in another mount namespace.** The superblock stays active, so `kill_block_super()` never runs and the loop stays open | **Leading candidate. It fits every observation.** `umount` in the service's namespace succeeds, because the busy check looks only at that mount and its propagation peers (`fs/namespace.c:1710-1716`). It is invisible to `stop()` and to the preflight, which both read `/proc/self/mountinfo` only (`mounts.initd:12,69-71`). It does not appear under `holders/`. That directory is filled by `bd_link_disk_holder` (`block/holder.c:73`). **Unverified:** that only stacking drivers (dm, md) call it was not checked by a tree-wide search. It lasts as long as that namespace keeps the copy. A namespace created **after** the mounts start, by `unshare -m`, `clone(CLONE_NEWNS)` or a container start, copies the `/mnt/vendor/*` mounts (`copy_mnt_ns`, `fs/namespace.c:3468-3496`). Context, not proof: the opt-in Waydroid/LXC runtime (`ferrix-android.initd`) starts its container in the background after boot. The S2 holder grep saw two `python3` processes holding `/dev/binderfs`, consistent with Waydroid's tooling. The mounts started about 2 minutes after the reboot. Whether that image linked `ferrix-android`, and whether LXC keeps such copies after its `pivot_root`, are not established here (no LXC or Waydroid source was reviewed) | kernel; image context |
| H2 | A long-lived process holding `/dev/loopN` open | **Possible, no named candidate.** No checked-in service opens a loop node. Only an all-process fd scan can rule it in or out | — |

Why the per-role proofs detached cleanly: on the same kernel, each #34
per-role proof ran `umount` and then `losetup -d`, and **checked**
`backing_file` afterwards (`tools/pixel-9a/s5300-mount-proof-remote.sh:357-368`).
Each one reported "unmounted and detached"
([README](README.md#s5300-source-mount-proof-issue-34)). Each mount there
lasted seconds inside one session, so no new namespace could have copied it.
The S2 mounts lasted about 16 minutes, with the rest of the dev image
running. That difference fits H1 and H2, and does not fit a kernel or
`losetup` defect.

The deferred state is **safe**. The loop stays read-only (`LO_FLAGS_READ_ONLY`
and the disk flag are unchanged), BLKROSET stays set on the partition, and
the loop detaches by itself if the holder ever closes. The costs are loop
exhaustion and a NO-GO `stale-loop-<role>` in the preflight
(`tools/pixel-9a/s5300-s2-preflight.sh:563`) until a reboot.

### Proposed fail-closed `stop()` check (not implemented)

Change only `detach_loop` (`mounts.initd:302-313`). `stop()` and #110's
proposed rollback (contract C2 in
[`s5300-s2-preflight.md`](s5300-s2-preflight.md#review-partial-mount-failure-path-of-ferrix-s5300-mounts))
both call it, so both get the check.

```sh
detach_loop() {
	[ -e "$sys_block_path/$1/loop/backing_file" ] || return 0
	if device_mounted "$(sys_value "$1/dev")"; then
		eerror "leaving $1 attached: it is still mounted"
		return 1
	fi
	losetup -d "$dev_root/$1" || { eerror "could not detach $1"; return 1; }
	# loop_clr_fd() unbinds synchronously, removing loop/, unless another
	# opener exists; then it only sets autoclear and returns 0.
	[ ! -e "$sys_block_path/$1/loop/backing_file" ] || {
		eerror "$1 is still bound after losetup -d (autoclear=$(sys_value "$1/loop/autoclear")): another opener holds it; reboot before the next preflight"
		return 1
	}
}
```

Properties:

- It never forces. No `umount -l`, no `LOOP_SET_STATUS` to clear autoclear,
  no second `losetup -d`, no signal to a holder, and no `nsenter`. Clearing
  autoclear would remove the only path by which the loop could still
  detach by itself.
- BLKROSET is never cleared, and no write path is added.
- `stop()` then returns 1 with the mounts gone and the loop bound. That is
  a truthful failure. The preflight already reports the residue as
  `stale-loop-<role>`, and the operating rule stays: reboot. What OpenRC
  records for a failed `stop()` has not been observed (#109 C-stop). The
  check must not depend on it.
- Optional, diagnostic only: before each `umount`, scan
  `/proc/[0-9]*/mountinfo` for the loop's `major:minor` in a namespace other
  than the service's own (compare `/proc/<pid>/ns/mnt`), and `eerror` how
  many there are. Refusing the `umount` on a hit is a separate design
  choice. It would leave the mount in place, which needs a reboot just the
  same.
- An optional bounded re-check (for example 10 × 0.2 s) would absorb an R4
  probe. It is not needed for correctness, because point 2 makes the first
  read exact.
- Cost: this changes the accepted #34 service (`b90d3b41…`). It needs a
  fresh #34 review, new pins (`s5300-cpif-services.sha256`, the APKBUILD
  `sha512`, a `pkgrel`), a new device-package release, and new per-role
  proofs. It should go in the same release as #110, which touches the same
  function. Offline: a `losetup` stub in
  `tools/pixel-9a/tests/s5300-mounts-service.sh` that exits 0 and leaves
  `backing_file` in place must make `stop()` return 1, and the existing
  cases must still pass.

### Read-only characterisation plan (lead only, each tier separately approved)

| Tier | Where | What it does | What it may not do |
| --- | --- | --- | --- |
| M | Any Linux VM or the QEMU guest, labelled `mock`/`QEMU` (not evidence for the device) | Reproduce both shapes on a scratch file: `losetup -r`, mount, then (H1) `unshare -m sleep 300 &`, or (H2) `exec 3</dev/loopN`; then `umount` and `losetup -d`. Expect exit 0, `autoclear=1` and `backing_file` present. Then end the holder and expect `backing_file` to disappear. Use this to validate the proposed check and the classifier below | — |
| D0 | Exact device, any boot, no S5300 service | Record the implementations of `umount`, `mount` and `losetup` (`readlink -f "$(command -v …)"`, version strings) and the apk versions of `busybox`, `util-linux`, `mount`, `umount`, `losetup`, `lxc` and `waydroid`. Alpine ships `mount` and `umount` as their own subpackages, and the device package depends on neither, so `umount` is probably BusyBox's. Also record whether `ferrix-android` is linked and running; the number of distinct mount namespaces with their `comm` names (`readlink /proc/[0-9]*/ns/mnt`); the propagation fields of `/` and `/mnt` in `/proc/self/mountinfo`; and the number of loop devices | no mount, no loop attach, no CPIF node, no service start |
| D1 | Only inside a future owner-approved mounts run, as read-only samples around #109 step T5 | Before `stop`, for each role's loop: `autoclear` (expected 0; closes the gap in point 4), the count and `comm` of processes in **other** mount namespaces whose `mountinfo` lists the loop's `major:minor`, and the count and `comm` of `/proc/*/fd` links to the loop node. After `stop`: `backing_file` present or not, and `autoclear`, at +0, +2, +10 and +60 s, then the scans again | no extra mount, unmount or detach; no holder killed; no `nsenter`; no write |

Classifier for D1: **H1** if an other-namespace `mountinfo` hit exists;
**H2** if an fd hit exists; **R4** if the loop detached within 60 s with
neither; **unknown** otherwise. Redaction: record role names, counts,
`comm` names and namespace counts only. Never record a block device name, a
backing name, a serial or a PID-to-identity mapping.

## #116: post-stop CP state

### Source facts

- **The daemon dies on SIGTERM with no shutdown code.** `exynos-modem.initd`
  has no `stop()`, so OpenRC's default stop signals the pidfile process
  (`exynos-modem.initd:28-29`). The patched daemon installs no signal
  handler. Nothing in its crates registers one, and its only dependencies
  are `prost`, `lexopt` and `libc`. `main()` just joins the channel threads
  (`main.rs:231-285`). SIGTERM therefore ends the process at once, and the
  kernel closes its fds.
- **Closing the CPIF nodes does not touch CP power.** `bootdump_release`
  and `ipc_release` purge the receive queues and call
  `ld->terminate_comm` (`cpif/bootdump_io_device.c:60-85`,
  `cpif/ipc_io_device.c:57-80`). The shared-memory link sets
  `terminate_comm = NULL` (`cpif/link_device.c:3714`). That matches the
  device: `ONLINE` after the stop.
- **A power-off primitive exists and is unused.** `IOCTL_POWER_OFF`
  (`_IO('o',0x20)`, `cpif/modem_prj.h:59`) on the boot node calls
  `mc->ops.power_off` (`cpif/bootdump_io_device.c:159-165`), which is
  `power_off_cp`. That sets `STATE_OFFLINE` through `change_modem_state`,
  cleans up the PCIe link and drops the CP GPIOs
  (`cpif/modem_ctrl_s5100.c:1039-1056,906-918`). Neither the daemon nor the
  adapter uses it: the R1 row maps only resets
  ([adapter](s5300-route-decision.md#operation-mapping-on-google-kleaf-cpif)).
- **Reboot uses a different, more orderly path.** The platform `.shutdown`
  hook `cpif_shutdown` calls `power_shutdown_cp` and sets `STATE_OFFLINE`
  (`cpif/modem_main.c:929-939,978`). `power_shutdown_cp` disables the CP
  IRQs, drains the wakeup workqueue, waits up to 3 s for `CP_ACTIVE`, then
  powers off (`cpif/modem_ctrl_s5100.c:1059-1094`). No ioctl reaches
  `power_shutdown`. This is the only CP-off path that this device has been
  through, and only implicitly: after the reboot, the preflight read
  `OFFLINE`.
- **`OFFLINE` in sysfs does not mean the CP is unpowered.** `power_reset_cp`,
  which handles `IOCTL_POWER_RESET` Cold, writes `STATE_OFFLINE` and then
  powers the CP rails back on (`cpif/modem_ctrl_s5100.c:1152-1176`,
  `gpio_power_offon_cp` `:920-934`). A reset must never be used as a
  power-off, because `cpif_check_surface OFFLINE` (`cpif.sh:141-169,647`)
  would then pass with the CP powered.
- **F5.** Sysfs `modem_state` prints the raw state (`cpif/modem_main.c:699-705`),
  but `IOCTL_GET_CP_STATUS` folds `RESET` into `OFFLINE`
  (`cpif/bootdump_io_device.c:296-318`). Any CP-off check must read sysfs,
  as the preflight does.
- **F2.** While the daemon runs, its boot thread polls the CP every second.
  It treats any departure from `ONLINE` as a crash, then Warm-resets and
  re-boots the CP in an unbounded loop (`s5300.rs:427-445,448-472`). On
  startup it adopts a CP that is already `ONLINE` (`s5300.rs:405-420`). Its
  reset policy assumes that `OFFLINE` means "fresh after the RC probe's own
  reset" and boots with no reset in that case (`s5300.rs:389-397`).

### Options

| | (a) Accept "reboot ends every S2/S3 run" | (b) Fail-closed CP power-down in the daemon's shutdown or in `stop_post` | (c) Separate, explicitly armed CP-off step |
| --- | --- | --- | --- |
| Mechanism | None new. The lead reboots after fetching the evidence. The kernel's `cpif_shutdown` → `power_shutdown_cp` powers the CP off | (b1) a new SIGTERM handler in the daemon that stops the supervisor, issues `IOCTL_POWER_OFF` on the held boot fd, checks the result and exits. (b2) a `stop_post` in `exynos-modem.initd` that runs a new helper (for example an `exynos-modem --power-off` subcommand) after the daemon is gone | An extra command that is valid only while the service is stopped (`extra_stopped_commands`, like `arm`), or a checked-in `tools/pixel-9a` step, with its own one-shot token. Preconditions: `sit-proxy` and `exynos-modem` stopped (no task, no pidfile), a clean and complete `cpif_check_unheld`, and sysfs `modem_state` in a reviewed allowlist. Action: open `umts_boot0`, issue `IOCTL_POWER_OFF` exactly once, close it. Postcondition: sysfs reads `OFFLINE` and no node is held. Otherwise it refuses, and the lead reboots |
| Code touched | Docs only | (b1) a new upstream patch 0004 (signal handling across the threads). (b2) the pinned initd, plus patch 0004 or new Rust. Both need a package release, pins, `sha512` and `pkgrel` | The pinned initd or a new workflow, plus a new Rust ioctl caller. `ferrix-s5300-cpif` is `#![forbid(unsafe_code)]`, so the ioctl belongs in the patched `exynos-modem` or a new reviewed crate. Release and pins as in (b) |
| F2 interaction | None. The daemon is gone before the CP goes down | **(b1) races F2.** Unless the supervisor loop is stopped first, it reads the departure from `ONLINE` as a crash and re-boots the CP (`s5300.rs:427-436`). (b2) avoids that | None. It runs only after the daemon is confirmed gone |
| F5 interaction | The preflight reads sysfs after the reboot (existing rule) | Must check sysfs, not the ioctl. A `RESET` reading is a failure | Same. It must also never use `IOCTL_POWER_RESET` (it leaves the CP powered while sysfs shows `OFFLINE`) |
| Ends the need for a reboot? | — | **No.** The next preflight is still NO-GO without a reboot because of the #117 loops (until the holder is fixed), the `/run/exynos-modem` residue (#109 T6), and the reset-policy assumption above | **No**, for the same reasons. It shortens the time the CP runs with no userspace (no RFS server, no boot-device owner) |
| New risk | The CP runs unattended between T2 and the reboot. That window is bounded by how quickly the lead reboots. No calibration exposure: the CP reaches storage only through the stopped RFS server, and the partitions stay read-only loops with BLKROSET | Every `rc-service stop`, including the one OpenRC runs at shutdown, becomes a CP power operation with no separate decision. `power_off_cp` has never run on this device and skips the steps that `power_shutdown_cp` takes. A SIGKILL skips (b1). The semantics of a failed `stop_post` have not been observed (#109 C1) | `power_off_cp`, as for (b), but only when a person asks for it and with checks before and after. Never exercised on this device |
| Evidence needed first | None | A review of `power_off_cp` against `power_shutdown_cp` for CP3A, offline tests, an OpenRC characterisation, then one exact-device observation | As for (b) without the OpenRC part, then one exact-device observation that `modem_state` reads `OFFLINE` and that the next boot after the reboot is clean |

### Recommendation: DRAFT, pending an owner decision

**(a) now.** Make "reboot ends every S2/S3 run" part of the teardown
contract: T3 records `modem_state` read-only, and a reboot follows every
run. Keep **(c)** as the preferred later option, if reboot-per-run becomes
a real cost (for example in S3). **Reject (b1)**, and do not choose (b2)
over (c).

Reasons:

1. A reboot is already required for three independent reasons: the #117
   loops (until the holder is found and removed), the `/run/exynos-modem`
   residue that makes the next preflight NO-GO
   ([#109 §2.5 T6](s5300-s2-active-design.md#25-teardown-order-never-forced)),
   and the daemon's assumption about `OFFLINE`. A CP-off step removes none
   of them.
2. The reboot path powers the CP off with the kernel's orderly
   `power_shutdown_cp`. The device has already shown it reaching `OFFLINE`.
   `IOCTL_POWER_OFF` uses `power_off_cp`, which takes fewer steps and has
   never run here.
3. (a) adds no code to an owner-gated CPIF path and leaves the accepted #34
   service, the package pins and the F2 behaviour as they are.
4. (b1) has to handle F2 inside upstream code that has no signal handling.
   (b1) and (b2) both turn every stop into an unannounced CP power action.
   (c) keeps that action separate, armed, checked before and after, and
   fail-closed to a reboot.

How these choices relate to Q2, Q6 and Q10 is for the owner to decide. This
document does not answer them.

## Follow-ups (each separately reviewed)

1. #117: the D0 characterisation, then D1 in the next approved mounts run.
2. #117: the `detach_loop` check, in the same #34 re-review and release as
   #110.
3. #116: on approval of (a), record the teardown contract in
   `s5300-s2-preflight.md` and the #109 design (docs only).
4. #116: if (c) is ever wanted, first review `power_off_cp` against
   `power_shutdown_cp` and the CP3A firmware, then design the step and its
   offline tests.
