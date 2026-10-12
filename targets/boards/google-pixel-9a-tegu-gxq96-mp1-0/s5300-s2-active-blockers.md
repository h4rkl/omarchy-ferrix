# S5300 S2 active workflow: blocker analysis (issue #92)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.

This is an offline review. It adds no command, script, or service change.
It does not authorise or perform a source mount, scratch provisioning, an
arm, a CP boot, or a teardown. No phone, USB link, builder, or CPIF node was
touched. Telephony stays unsupported.

Question reviewed: do the current OpenRC services
(`exynos-modem.initd`, `sit-proxy.initd`, the #34-accepted
`ferrix-s5300-mounts.initd` `b90d3b41…f74b`) and the CPIF library
(`ferrix-s5300-cpif.sh`) already make a fail-closed, one-shot S2 workflow
possible, with no change to the accepted service or its pin?

**Conclusion: not yet.** No active S2 command is checked in. The gaps below
must be closed, or explicitly accepted by the owner, first. The read-only
preflight ([`s5300-s2-preflight.md`](s5300-s2-preflight.md)) stays the only
checked-in S2 workflow.

## Blockers

1. **Partial mount start has no checked-in recovery.** The accepted
   `ferrix-s5300-mounts` `start()` does not roll back. After a failed start,
   OpenRC records the service as stopped, so `rc-service … stop` does not run
   `stop()`. The only safe rule today is C6 in the preflight document: do not
   retry, do not unmount by hand, and reboot. A workflow can stop and report
   that state. It cannot release it. Fixing this means changing the accepted
   #34 service (contract C1–C5). That needs its own #34 review, new pins, and
   new per-role proofs. It is out of scope here.
2. **OpenRC behaviour has never been exercised on the device.** The offline
   suites stub `service_started`, `checkpath`, `start-stop-daemon`, and the
   `use` semantics. None of these have been observed on the device: the
   `arm` command for stopped services, token consumption in `start_pre`, the
   service state after a failed or killed `start_pre`, how `stop` acts on a
   crashed pidfile daemon, the default stop retry or escalation, and whether
   stopping `ferrix-s5300-mounts` touches services that only `use` it. A
   teardown that must be verified by its outcome needs these semantics first.
   A read-only, device-side characterisation can supply them. It must not
   start any S5300 service.
3. **The opener scan lacks a positive control on the device.** *(Offline fix in #107, `device-google-tegu` 1-r4; open until a rebuilt image passes on the exact phone.)* The scan
   matches holders by `stat -L` device number on every thread's fd table, and
   it fails closed on unreadable entries. On the one exact-device run it
   returned `clean`, but it has never been shown to *detect* a holder with
   the guest's BusyBox `stat` output format. A format difference would turn
   every scan into a false `clean`. Open hardening items, all read-only:
   - a positive control against a harmless character device the scanner
     itself holds;
   - coverage of `/proc/<pid>` entries whose `task` directory cannot be
     listed (today such a pid drops out silently instead of failing);
   - proof that every character device under the `cpif` majors is in the
     `cpif` class, so a device with another name under the same major is not
     missed.

   The library is pinned in `s5300-cpif-services.sha256`, so any change to
   it needs a new device-package release and image (after 1-r3). Changes
   carried only in a workflow body would leave `arm`/`start_pre` relying on
   the installed, unhardened scan.
4. **Decision binding has no device identity source over CDC-ACM.** The
   preflight never reads a serial. It identifies the board only by device
   tree, kernel, and bootconfig revision. A per-run owner decision must be
   bound to the exact observed unit, the 1-r3 image, the pinned factory
   artifact, the package pins, and the current CPIF state, and it must
   expire with the boot. That needs a redacted per-unit identity (a hash
   prefix only) and a per-boot identifier from the guest. Neither has been
   reviewed or observed. One more rule is needed: a decision recorded against
   a non-`go` preflight (for example the 1-r2 run) must never satisfy a later
   image.
5. **Session lifetime vs. bounded observation.** On timeout the CDC-ACM
   transport stops its own probe from the host side, and a USB disconnect
   ends the session. An observation of 10 minutes or more, with teardown,
   therefore cannot depend on the host session staying alive. Any design
   where the guest proceeds on its own is new, safety-relevant behaviour. It
   needs its own review before code exists.
6. **Evidence gaps.** These do not block safety, but they block a GO:
   - The CPIF `exynos-modem` service sets no `output_log`. Daemon output
     cannot be kept without changing a pinned file or adding an OpenRC
     override, and the preflight treats an override as NO-GO.
   - The S2 GO criterion "RFS requests are served" is therefore not
     observable.
   - Kernel-log crash signatures for Google CPIF have not been reviewed, so
     any crash classifier would be a heuristic.
   - Whether the CP returns to `OFFLINE` after the daemon stops is unknown.

## What is already sufficient

- One-shot start token on tmpfs: it is consumed before `start_pre` checks
  and cleared by reboot.
- No runlevel links and no respawn: an AP panic costs at most one reboot.
- `use` instead of `need`: no service starts another S5300 service.
- The preflight's NO-GO classification of stale, partial, and override
  state.
- Every source mount is a verified read-only loop with BLKROSET set: no
  failure path above opens a write path to per-unit partitions.

## Proposed next slices (each separately reviewed)

1. [#107](https://github.com/h4rkl/omarchy-ferrix/issues/107): read-only scan hardening (blocker 3) in the library, with offline
   positive and negative cases. Then rebuild the image and repeat the
   read-only preflight on the exact device.
2. [#108](https://github.com/h4rkl/omarchy-ferrix/issues/108): read-only identity and boot-ID facts (blocker 4), redacted to hash
   prefixes, added to the preflight.
3. [#109](https://github.com/h4rkl/omarchy-ferrix/issues/109): a written design for blockers 2, 5 and 6, for owner review before any
   active code exists.
   Design: [`s5300-s2-active-design.md`](s5300-s2-active-design.md) (for owner review; not implemented; authorises nothing).
4. [#110](https://github.com/h4rkl/omarchy-ferrix/issues/110): #34 follow-up for blocker 1 (contract C1–C5), if the owner wants
   recovery without a reboot.

Until then, physical S2 remains blocked. A fresh owner decision is still
needed for each of: the mounts start, and any CP boot.
