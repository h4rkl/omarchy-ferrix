# S5300 S2 read-only preflight (issue #92)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.

This is a read-only slice of issue #92 that must land before any manual CP
boot. It adds a checked-in, bounded preflight that collects the facts a
reviewer needs for a later one-shot S2 manual CP boot, and returns a
machine-readable GO/NO-GO for those facts. **It does not boot the CP, start
or arm a service, mount anything, or implement any active S2 step.** The
implementation pass was offline. The preflight itself has never mounted,
started, armed or booted anything, or opened a CPIF node. Later, the owner
approved one manual S2 run, which the lead performed by hand around this
preflight. Its result is in
[S2 active CP boot](#exact-device-s2-active-cp-boot-on-the-1-r3-image-2026-10-12).

Route context: [`s5300-route-decision.md`](s5300-route-decision.md) (stage 2),
[`s5300-cpif-integration.md`](s5300-cpif-integration.md) (services, library,
and opener scan).

## Running it

```sh
just pixel-9a-s5300-s2-preflight        # tools/pixel-9a/s5300-s2-preflight.sh
just pixel-9a-s5300-s2-preflight-test   # offline tests only
just pixel-9a-s5300-s2-binding check|consume RECORD FACTS   # offline (#108)
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
  `opener_scan`, `opener_positive_control`,
  `deferred_to_active_workflow`, `cp_nodes_opened: false`,
  `state_changes: "none"`, `cp_boot_authorized: false`, and
  `active_s2_workflow: "not-implemented"`. Since issue #108 it also has
  `unit_identity`, `unit_identity_source`, `boot_identity`,
  `binding_salt_id`, `decision_binding`, `decision_binding_id`, and
  `decision_binding_record`.
- `binding-<stamp>.rec`, only for a `go` with both identities available: the
  single-use decision-binding record (see
  [Decision binding](#redacted-identity-and-decision-binding-issue-108)).
  It is also appended to the end of its transcript.

New `key=value` lines (issue #108; every earlier key and exit code is
unchanged): `session_header=binding-salt` and `binding_salt_id` in the
transcript header; `unit_identity`, `unit_identity_source`,
`unit_identity_state`, `boot_identity`, `boot_identity_state`,
`decision_binding` (`written`, `refused-no-go`, `refused-incomplete`,
`unit-identity-unavailable`, or `boot-identity-unavailable`) and
`decision_binding_id` (32 hex or `none`) on stdout and in the transcript;
and `decision_binding_record=<path>` on stdout when a record is written.
The raw probe section gains `unit_id_source`, `unit_id_state`,
`unit_id_hash`, `boot_id_state`, and `boot_id_hash`.

These files contain no serial, port name, partition label, block device
name, file name under `/mnt/vendor`, PID, kernel log, IMEI/SIM data, or
per-unit bytes. The unit and the boot appear only as salted 16-hex
SHA-256 prefixes, and the salt never appears. CPIF holder task names are reduced to the known service
names or `other`; any unexpected CPIF class entry is also `other`.
Package-file hashes and versions are included.

Exit status: 0 `go`, 8 `no-go`, 9 `incomplete`; 2 usage; 3 transport or guest
failure; 4 not the exact tegu device tree; 5 malformed or incomplete probe
output (including inconsistent identity fields); 6 a host input, or the
binding salt, was refused before the port was opened.

## What runs where

**Host, before the port is opened** (exit 6 on failure):

- The checked-in `ferrix-s5300-cpif.sh` must match its own package pin in
  `s5300-cpif-services.sha256`. This guarantees that the guest receives the
  reviewed issue #92 alias-aware library.
- The checked-in `ferrix-s5300-mounts.initd` must still be the #34-accepted
  `b90d3b41…f74b`, and the library must pin that same hash.
- The pin list must name each of the seven pinned files exactly once. The
  expected releases are read from the recipes: `device-google-tegu`
  `1-r4` (issue #107 opener-scan hardening), `modemmanager-samsung-sit`
  `1.25.95_git20260731-r3`, and
  `exynos-modem` `0.1.0_git20260731-r3`.

**Host recovery artifact evidence.** This is a fact about this host, never
about the device, and the transcript keeps it in its own section. The
manifest `identity` gate must be `passed` and `recovery_procedure` must be
`tested`. The pinned factory archive named in `recovery.pins` must be present
under the ignored `recovery/`, be a regular file, and match the pinned size
and SHA-256 (`host_recovery_factory_archive=verified`). Anything else is a
`host_blockers` entry. The workflow does not run or test a restore, and it
cannot show that the restore would work on the device today.

**Guest, one CDC-ACM session.** `acm-probe.py` sends one file made of one
`s2_salt=<64 hex>` line, then the checked-in library, then
`tools/pixel-9a/s5300-s2-preflight-remote.sh`. The transcript's
`composed_probe_sha256` is still the hash of the library plus body only, so
it stays reproducible and never depends on the salt. The guest checks the
SHA-256 of the whole file before running it. The body:

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
  bootconfig/cmdline, plus the identity candidates and boot ID below, which
  it only hashes;
- runs the installed `exynos-modem --print-config` only if the apk database
  names the pinned release;
- runs the accepted service's `verify_own_mount`, through the library's
  `cpif_check_sources`, only if `ferrix-s5300-mounts` is **already** started.
  This reads sysfs, mountinfo, and util-linux `blkid -p` types, and checks
  per-unit inputs by `stat(2)` only.

## Check matrix

"Now" means the check can pass on the current installed image. "New image"
means it cannot pass until an image rebuilt with `device-google-tegu` 1-r4
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
| Unit identity (#108) | first of the four candidates below | reported only; never a blocker or unknown. `go` + `ok` is needed for a binding record | **unconfirmed** on tegu |
| Boot identity (#108) | `/proc/sys/kernel/random/boot_id` | reported only; `go` + `ok` is needed for a binding record | **unconfirmed** on tegu |
| PID namespace | `/proc/self/ns/pid` | initial namespace | now |
| Backend | `backends.env` (parsed) | target exact, `google-kleaf-cpif` | now (composer sets it) |
| Library `cpif_check_target` | — | pass | now |
| Installed files vs checked-in | SHA-256 of the 7 pinned paths | all equal to the checked-in pins | **new image**: `s5300-cpif.sh` differs on 1-r3 and older |
| On-device pins | `s5300-cpif-services.sha256` | `cpif_check_package` pass | now (older images are self-consistent) |
| Releases | apk db | device 1-r4, modem r3, MM r3 | **new image**: any 1-r3 or older device package is skew |
| Mounts service | installed SHA-256 | #34-accepted `b90d3b41…` | now |
| CLI | `exynos-modem --print-config` | exact reviewed text | now (binary is r3) |
| Surface | modules, platform, driver, `modem_state`, WWAN | three modules `Live`, one `cp_interface`, `OFFLINE`, no WWAN | now |
| Nodes | twelve E1 nodes | each passes `cpif_check_node` | now |
| Openers | every `cpif` class entry, alias-aware, every thread | `clean`; `held`, `incomplete`, or `not-run` is NO-GO | now (the checked-in library is sent to the guest) |
| Opener positive control (#107) | `opener_positive_control` | `detected`; `missing` is NO-GO and makes the scan `incomplete` | **offline only**: never observed on the device |
| Opener format control (#107) | `opener_format_control` | `matched`; `mismatch` or `missing` is NO-GO | **offline only** |
| Foreign devices under CPIF majors (#107) | `opener_major_foreign` | `0`; a count is NO-GO, `unknown` is NO-GO | **offline only** |
| Vanished pids/tasks (#107) | `opener_vanished` | informational count | **offline only** |
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
| Scratch provision, arm, token use, `start_pre` re-check, CP boot, logs, ONLINE ≥10 min, teardown | — | — | **active** (no checked-in workflow; one manual, owner-approved run, see below) |

`partition_ro_flag_since_boot=yes` is informational. BLKROSET stays set
until reboot after any earlier mount in this boot. It only tightens the
barrier, but it shows that this boot is not fresh.

### Exact-device S2 active CP boot on the 1-r3 image (2026-10-12)

Evidence label: `physical:google-pixel-9a-tegu-gxq96-mp1-0`. The owner
approved three steps for one run: the S5300 source mounts, the steps that
follow them, and one CP boot. The lead ran each step by hand over the
CDC-ACM dev shell, around this checked-in preflight. **No active S2 workflow
is checked in.** This run does not close any blocker in
[`s5300-s2-active-blockers.md`](s5300-s2-active-blockers.md). It answers
some of their open questions on the device (see "Effect on the open
blockers" below).

Image: dev bundle `tegu-kleaf-dev-20261012T062930Z` on slot B. Its rootfs
was built from `0032a6b` with `device-google-tegu` 1-r3, `exynos-modem` r3
and `modemmanager-samsung-sit` r3. The kernel is
`6.1.124-android14-11-ga4662a84a15f`. All times below are UTC.

| Step | Time | Result |
| --- | --- | --- |
| Pre-mount preflight | 08:29 | `go` (AP uptime 54 min, not a fresh boot) |
| `rc-service ferrix-s5300-mounts start` | 08:29 | **failed before any mount**: `checkpath` could not create `/mnt/vendor/modem_img` because `/mnt/vendor` did not exist. No role loop was attached and nothing was mounted. |
| Recovery (rule C6) | 08:30–08:32 | Reboot. Then a one-off operator step on the device: `install -d -m 0755 -o root -g root /mnt/vendor`. This is not a checked-in workflow step. |
| Pre-mount preflight | 08:32 | `go` |
| `ferrix-s5300-mounts start` | 08:33 | rc 0, service `started` |
| Post-mount preflight | 08:33 | `go`, `check.sources=pass`, `vendor_mounts=4`, `partition_ro_flag_since_boot=yes` |
| `rc-service exynos-modem provision` | 08:33:53 | rc 0; `/run/exynos-modem` root:root 0700 |
| `rc-service exynos-modem preflight` | 08:33 | rc 0 (no CPIF node opened) |
| `arm`, then `start` | 08:34:08–08:34:26 | both rc 0; `modem_state` read `OFFLINE` as `start` returned |
| Observation | 08:34:40–08:48:31 | `ONLINE` in every sample (below) |
| Teardown | 08:49:01 | both stops rc 0; two findings (below) |
| Reboot, pre-mount preflight | 08:53 | `go`: CP `OFFLINE`, no vendor mount, no role-backed loop, `partition_ro_flag_since_boot=no` |

**Sources.** `modem_img` (ext4) and `efs`, `persist` and `modem_userdata`
(f2fs) were each mounted read-only with `norecovery`, plus
`nosuid,nodev,noexec`. Each was on its own loop. Every loop and every
partition read `ro=1`, with BLKROSET set. Every role had one loop, no
direct mount and no holder.

**CP boot.** In the kernel log, the daemon opened `umts_boot0` and issued
`IOCTL_POWER_ON`, then `IOCTL_START_CP_BOOTLOADER` (normal boot mode), then
`IOCTL_COMPLETE_NORMAL_BOOTUP`, about 2.5 s after power-on. The kernel then
logged `cmd_phone_start_handler`. `modem_state` was `ONLINE` at the first
poll, 08:34:40 (uptime 206 s). It was still `ONLINE` at the last sample,
08:48:31 (uptime 1038 s). That is about 13.9 minutes of samples from the
same AP boot, with an unchanged boot identity. Every sample showed the
daemon alive and the service `started`.

Sampling was not continuous. A read-only host poll ran about every 45 s
from 08:34:40 to 08:42:13 (11 samples). One more sample was taken at
08:48:31. Nothing was sampled between those two times. With the same boot
identity at both ends, an AP reset in that gap is ruled out. A brief CP
state change in that gap is not ruled out.

**S2 GO assessment** (route decision stage 2):

- *ONLINE ≥10 min, no CP crash dump or AP panic*: **met**, within the
  sampling limits above.
- *RFS requests are served*: **not observed**. The `exynos-modem` service
  sets no `output_log`, so no daemon output was kept. The CP reaching and
  holding `ONLINE` is consistent with RFS working, but it is not evidence
  of it. This stays an evidence gap (blocker 6, #109).

**Known-benign kernel-log signatures.** The poll counted lines in the new
kernel log that matched
`crash|panic|silent reset|cp_fail|watchdog|cp crash|dump`. It found 9 hits,
then 10 from 08:41. Every hit is one of these signatures. Future runs need
not re-investigate them:

| Signature | Why it matches | Classification |
| --- | --- | --- |
| `cpif: bootdump_open/_release/_ioctl: umts_boot0 …` (`IOCTL_HANDOVER_BLOCK_INFO`, `IOCTL_POWER_ON`, `IOCTL_START_CP_BOOTLOADER`, `normal boot mode`, `IOCTL_COMPLETE_NORMAL_BOOTUP`) | `dump` in the CPIF boot-device function names | normal CP boot sequence |
| `cpif: cmd_phone_start_handler: Set crash_reason type:65535` | `crash` | `65535` is the "none" sentinel that the kernel sets at phone start; it is not a crash |
| `[dhd][wlan]dhd_dump_mod_pkt_timer: Arm the pktcnt timer. reason=3` | `dump` | Wi-Fi driver, unrelated to the CP |

No line reported a CP crash, a silent reset, a watchdog or an AP panic.
This list is not a reviewed CPIF crash-signature list. That review remains
#109 work. A new signature must still be investigated.

**Teardown findings.** Each one needs its own follow-up issue.

1. **Stopping the daemon does not stop the CP.** `sit-proxy` was already
   stopped. `rc-service exynos-modem stop` returned 0, and 3 s later the
   daemon was gone and the service `stopped`. `modem_state` then read
   `ONLINE` in six consecutive reads. The checked-in service has no
   `stop()` override, so OpenRC's default stop only signals the daemon.
   The checked-in patches add no stop-time power-off or reset of the CP. A
   reboot was required to return the CP to `OFFLINE`. Two consequences:
   - every daemon stop leaves the CP running with no userspace;
   - `cpif_modem_preflight` requires `OFFLINE`, so every re-arm after a
     stop also needs a reboot.

   This relates to findings F2 and F5 of the #109 design (PR #114), which
   cover the daemon's own CP re-boot loop and the `RESET` and `OFFLINE`
   states.
2. **Role loops are not detached on stop.** `rc-service ferrix-s5300-mounts
   stop` returned 0, unmounted all four destinations, and left no
   `/mnt/vendor/*` mount. The arm token was absent. All four role loop
   devices stayed attached, each with `autoclear=1`, `ro=1`, no holders and
   no mount. A reboot detached them. `stop()` runs `losetup -d` on each loop
   after its unmount and treats its exit status 0 as success. Possible
   cause, not verified: the kernel's deferred detach. That path sets
   autoclear instead of detaching while the loop is still open, and returns
   0. Whatever the cause, the outcome does not match the documented "detach
   that loop", and a start/stop cycle without a reboot uses up role loops
   faster than expected.

An ad-hoc holder grep before the mounts stop counted 2 processes. Both were
`python3` holding `/dev/binderfs`: the pattern matched the substring `rfs`.
Neither held a CPIF node or a `/mnt/vendor` file. The checked-in opener scan
does not use that grep.

**Effect on the open blockers.**

- Blocker 2: on the device, `arm` worked on a stopped service. The token was
  used once (absent after the run), and `stop` on a live pidfile daemon
  returned 0. A failed or killed `start_pre`, and stop on a crashed daemon,
  were not exercised.
- Blocker 6: the CP does **not** return to `OFFLINE` after the daemon stops;
  this is no longer unknown. Daemon output and RFS service are still not
  observable.
- Blocker 1 was not exercised: the failed start stopped before any mount.
  Blockers 3–5 are unchanged.
- The opener scans in this run used the 1-r3 library. That library predates
  the #107 positive control. Each scan reported `clean`, but it was not
  shown to be able to detect a holder.

**Next.** First, a rebuilt 1-r4 image. It carries `/mnt/vendor` (`02eebc7`)
and the #107 opener-scan control. Then a pre-mount preflight `go` with
`opener_positive_control=detected`. S3 (`sit-proxy`, ModemManager, SIM)
has not started. S3 planning waits on the owner review of the #109 design.
Each mounts start and CP boot still needs a fresh owner decision.

Not done in this run: anything for SIM or carrier, `sit-proxy`,
ModemManager binding, and any write to a per-unit partition. `[telephony]`
and `[audio]` remain `unsupported`. The raw shell log, poll output and
preflight transcripts are ignored under `out/pixel-9a/`. This record
contains no serial, IMEI, block device name, loop backing name or boot ID.

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
a CP boot. The owner had not yet decided on either; the later, separately
approved run is recorded above.
[`s5300-s2-active-blockers.md`](s5300-s2-active-blockers.md) still blocks a
checked-in active S2 workflow. The transcript and summary are ignored under
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
`out/pixel-9a/s5300-s2-preflight/`. That run predates the issue #107
scan hardening, so it reported none of the `opener_positive_control`,
`opener_format_control`, `opener_major_foreign`, or `opener_vanished`
facts, and its `clean` was never shown to be able to detect a holder. The
checked-in expectation is now `device-google-tegu` 1-r4: a 1-r3 image also
reports `file-s5300-cpif.sh,pkg-device-google-tegu` skew. A rebuilt and
reflashed 1-r4 image is
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

## Opener scan hardening (issue #107, offline only)

The guest body now runs the library's `cpif_opener_scan` and re-validates
its facts before they leave the guest. The library is pinned, so these
checks reach `arm`/`start_pre` only from a rebuilt `device-google-tegu`
1-r4 image. **None of them has run on the exact phone yet.** Until a
rebuilt image runs this preflight on `google-pixel-9a-tegu-gxq96-mp1-0`,
they are offline (fixture) evidence only.

- `opener_positive_control=detected|missing`. The scanning shell holds the
  sysfs mem-class null device (`/dev/null`) open read-only on fd 7 for the
  whole scan. It is opened only after `stat(2)` shows a character device,
  not a symlink, with the sysfs `class/mem/null/dev` number, under no CPIF
  major. The same `/proc/<pid>/task/<tid>/fd` walk, the same
  `stat -L -c '%n|%t:%T|%F'` format, and the same awk matcher must report
  that device as held by the shell's own pid (from `/proc/self/stat`, read
  by the same process). `missing` makes the scan `incomplete`. Blocker:
  `opener-positive-control-missing`.
- `opener_format_control=matched|mismatch|missing`. The null device reads
  `1:3` in decimal and in hex, so the control alone cannot catch a numeric
  format difference. Every `/dev` character device named like a `cpif`
  class entry must print, through the same format and matcher, exactly
  that entry's sysfs number. Blocker: `opener-format-control-<value>`.
- `opener_major_foreign=<count>|unknown`. Every character device under a
  CPIF major, in `/sys/dev/char` and anywhere under `/dev` (a `find` walk,
  `stat(2)` only), must be the `cpif` class entry of that name and number,
  and every class entry must be listed in `/sys/dev/char`. Others are
  counted, never named. An fd on such a device is also a holder (reported
  as `other`). Blockers: `cpif-major-foreign-device`,
  `opener-major-enumeration-incomplete`.
- `opener_vanished=<count>`: pids or tasks that were listed but had gone
  when re-checked. They are skipped. A pid that still exists but whose
  `task` directory is missing, empty, or unlistable makes the scan
  `incomplete`; before #107 such a pid dropped out silently.

The physical run must show, on a rebuilt 1-r4 image:
`opener_positive_control=detected`, `opener_format_control=matched`,
`opener_major_foreign=0`, and `opener_scan=clean`. If the CPIF devices
share a major with another driver, `opener_major_foreign` will block, and
that needs its own review rather than a relaxed check.

## Limits

- The positive control proves the scan can detect the scanner's own fd on
  the null device. It does not prove that every other task is visible, and
  in the offline fixture the control's fd entry is a static symlink, so the
  fixture cannot show that the open happened; only a device run can.
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

## Redacted identity and decision binding (issue #108)

Blocker 4 of the #92 active-S2 analysis: an S2 owner decision must be bound
to the exact observed unit and boot, and a decision recorded against a
non-`go` preflight (for example the 1-r2 run) must never satisfy a later
image. This section is offline work. No physical run has exercised it yet.

### Identity sources

The guest tries these in a fixed order. The first one present wins. A
candidate that is present but repeated (`ambiguous`) or outside
`[A-Za-z0-9._:-]{4,64}` (`malformed`) stops the search. There is no
fallback past it, so a later candidate can never replace a damaged earlier
one.

| Order | `unit_id_source` | Read from | Evidence on tegu |
| --- | --- | --- | --- |
| 1 | `bootconfig-serialno` | `androidboot.serialno` in `/proc/bootconfig` (exact key, like the slot and revision) | Expected, not observed. `audio-route-decision.md` records that bootconfig and cmdline carry `androidboot.serialno`. The #92 exact-device run read ABL's `androidboot.revision=MP1.0` from bootconfig, but never this key. |
| 2 | `cmdline-serialno` | `androidboot.serialno=` token in `/proc/cmdline` | Same note as order 1. Older boot-image routes put it here. |
| 3 | `dt-serial-number` | `/proc/device-tree/serial-number` | Unverified. No checked-in inventory reads it. |
| 4 | `chipid-unique-id` | `/sys/devices/system/chip-id/unique_id` (Google downstream `gs-chipid`) | Unverified. The path is assumed and has not been observed on this kernel. |

The unit hash covers the source name and the value
(`<source>:<value>`), so two sources never compare equal. If a later boot
reports another source, the unit no longer matches and the binding is
refused.

Boot identity is the kernel's per-boot random UUID,
`/proc/sys/kernel/random/boot_id`. It must be 36 characters of `[0-9a-f-]`.
It is standard Linux procfs, but it has not been observed on tegu.

States: `unit_id_state` and `boot_id_state` are `ok`, `absent`,
`malformed`, `no-salt`, or `no-hasher`, and the unit can also be
`ambiguous`. A hash (16 hex) is printed exactly when the state is `ok`.
Otherwise the hash is `unavailable`. The host refuses inconsistent
combinations with exit 5.

### Salt and redaction design

- **Salt.** 32 random bytes as hex, in the ignored
  `out/pixel-9a/s5300-s2-binding/salt` (directory 0700, file 0600, owned by
  the user). It is created once with `O_EXCL` on first use. A salt or
  directory with another owner or mode, or another size, is refused (exit
  6), never repaired. It is never printed, logged, committed, or passed as
  an argument. `binding_salt_id` is a public 16-hex fingerprint
  (`SHA-256("ferrix-s5300-s2-salt-id-v1\n" salt "\n")`).
- **Hash in the guest.** The host prepends the salt to the session as the
  only header line. The guest reads each raw value into a shell variable and
  feeds `ferrix-s5300-s2-unit-v1\n<salt>\n<source>:<value>\n` (or the
  `boot` form) to `sha256sum` on stdin with the `printf` builtin, so the
  value never appears on an argument vector. It prints only the first 16 hex
  digits, then unsets the salt and both raw values before running any other
  program. The salt is never exported, so `exynos-modem --print-config` and
  `blkid` never inherit it.
- **Why not hash on the host?** That would put the raw serial and boot ID
  across the transport and into the host's work files. Hashing in the guest
  means that neither raw value ever leaves the guest.
- **Why salt?** A serial is short, structured, and visible on the box, in
  Fastboot, and in ADB. An unsalted hash in a transcript that gets shared
  would let anyone confirm a guessed serial. A 256-bit host secret makes the
  prefix unlinkable without the salt. One persistent salt per host, not one
  per run, keeps the unit hash stable across boots, which is what binding
  needs. 64 bits is ample to tell one unit from another.
- **Transport.** The salt travels inside the base64 probe that
  `acm-probe.py` sends after `stty -echo`. It runs piped to `sh`, so it is
  not in shell history (the bootstrap line holds only the nonce, hash, and
  line count). Only framed probe output is kept. The session file sits in
  the private `mktemp` work directory, which is removed on exit.
- **Threat model.** This protects against accidental disclosure in
  transcripts and against stale or reused decisions. It does not protect
  against someone who already runs as the owner and can read the salt.

### Unavailable identity: reported, binding fails closed

An unavailable unit or boot identity is **not** a blocker or an unknown.
It never changes `preflight_result` or the exit status. It only sets
`decision_binding=unit-identity-unavailable` (or `boot-...`) and writes no
record. This is deliberate. `preflight_result` answers one question: does a
read-only device fact block the next step? Identity says nothing about CP,
node, mount, or service state. Making it `incomplete` would also stop the
first physical run from reaching a pre-mount `go` before anyone has
confirmed which identity source tegu actually exposes. Safety holds anyway,
because every later action needs a valid, consumed record, and no record
can exist without both identities.

### Decision-binding record

A record is written only when `preflight_result=go` and both identities are
`ok`. It goes, mode 0600, to `binding-<stamp>-<pid>.rec` beside the
transcript, and is appended to the end of the transcript under
`## decision binding (single use; s5300-s2-preflight.md)`. It has exactly
these `key=value` lines, in this order (`s2b_record_keys` in
`tools/pixel-9a/s5300-s2-binding-lib.sh`):

| Key | Meaning |
| --- | --- |
| `record`, `record_version` | `ferrix-s5300-s2-decision-binding`, `1` |
| `record_id` | 32 random hex; the owner's typed decision must name it |
| `target_id`, `evidence` | exact target; `physical:<target>` or `mock` |
| `preflight_result`, `preflight_gate`, `preflight_phase` | `go`; `go` or `not-evidence-mock`; `pre-mount` or `post-mount` |
| `unit_identity`, `unit_identity_source`, `boot_identity` | salted prefixes and source |
| `binding_salt_id` | fingerprint of the salt that made the hashes and the seal |
| `kernel_release`, `slot_suffix` | observed |
| `library_sha256`, `probe_body_sha256`, `composed_probe_sha256` | checked-in library, body, and library+body |
| `package_pins_sha256`, `mounts_service_sha256` | `s5300-cpif-services.sha256` and the #34 mounts service |
| `pkg.device-google-tegu`, `pkg.exynos-modem`, `pkg.modemmanager-samsung-sit` | checked-in releases (equal to installed, since `go`) |
| `factory_archive_sha256`, `host_recovery_factory_archive` | pinned factory archive hash; `verified` |
| `preflight_transcript`, `preflight_transcript_sha256` | transcript name and the SHA-256 of every line before the binding section |
| `created_epoch`, `expires_epoch` | host clock; expiry is exactly 600 s later |
| `single_use`, `cp_boot_authorized` | `consume-by-rename`; `no` |
| `record_mac` | HMAC-SHA-256 of all lines above, keyed by the salt |

### Checker contract (`tools/pixel-9a/s5300-s2-binding.sh`)

```sh
s5300-s2-binding.sh check   RECORD FACTS   # validate only
s5300-s2-binding.sh consume RECORD FACTS   # rename first, then validate
```

`FACTS` is the transcript of a fresh preflight that the caller ran itself.
It may not be the record's own transcript or a copy of it. Output is
`binding_check=pass|refused`, with `binding_refusal=<token>` on refusal and
`binding_consumed=<path>` after a rename. Exit status is 0 pass, 2 usage, 7
refused. The checker never creates a salt and never contacts a device.

It refuses, with these tokens:

- **record:** a name other than `binding-*.rec` (`record-name`); not 0600
  (`record-not-private`); schema (`record-malformed`, which also covers
  unknown or reordered keys and any lifetime other than 600 s); a bad seal
  (`record-mac`); `record-target`; `mock` evidence or a non-`go` gate
  (`record-mock-origin`); `no-go` or `incomplete` (`record-not-go`);
  another salt (`record-salt-changed`); `record-expired` or
  `record-not-yet-valid`; a missing or edited transcript
  (`transcript-missing`, `transcript-mismatch`);
- **single use:** a renamed `.consumed-*` file, or a `record_id` already in
  the 0600 ledger `out/pixel-9a/s5300-s2-binding/consumed-ids`, which
  catches copies (`record-consumed`); a record that was already renamed away
  (`record-already-consumed`);
- **current facts:** not a private preflight transcript
  (`facts-unreadable`, `facts-not-private`); `facts-not-fresh`;
  `facts-not-physical`; not `go` (`facts-not-go`); `phase-mismatch`;
  `unit-identity-unavailable`, `unit-mismatch` (another unit or source);
  `boot-identity-unavailable`, `boot-changed`; `kernel-changed`,
  `slot-changed`;
- **checked-in inputs now:** library, body, composition, pin list, mounts
  service, or any release changed (`pins-changed`); the pinned factory hash
  changed (`factory-changed`); `host-recovery-not-verified`;
- **salt:** `salt-missing`, `salt-not-private`, `salt-dir-not-private`,
  `salt-malformed`.

**Single use.** `consume` renames the record to
`<record>.consumed-<epoch>-<pid>` with `rename(2)` before any validation, so
of any number of concurrent consumers exactly one wins. It then appends the
`record_id` to the ledger and validates. A record that fails after the
rename stays consumed, and a retry is refused. **Expiry** is whichever comes
first: 10 minutes after `created_epoch`, or a boot change (the current
`boot_identity` must equal the record's). A clock that runs backwards is
refused.

**The later active workflow must:** run a fresh preflight in the same
invocation; call `consume` with the record and that transcript; require the
owner's per-run typed phrase to name the exact target and the `record_id`;
and act only on exit 0, then repeat its own checks before `arm`. A passing
record is an input to the owner decision. It never authorises a CP boot by
itself. Deleting the salt directory resets the ledger, but it also
invalidates every earlier record, because the salt id and the seal no longer
match.

### What the physical read-only run must confirm

1. Which `unit_identity_source` tegu reports (expected:
   `bootconfig-serialno`), with `unit_identity_state=ok`.
2. That `boot_identity_state=ok`.
3. That `unit_identity` is the same across two runs and across a reboot
   with the same host salt.
4. That `boot_identity` is the same across two runs in one boot, and
   changes after a reboot.
5. That BusyBox `sha256sum` hashes stdin as expected.
6. That no transcript, summary, or record contains the raw serial or boot
   ID. The operator checks this locally and never writes the raw value
   anywhere.

Until these are confirmed, the identity sources stay
**unconfirmed physical facts**.

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
- After #107 (`device-google-tegu` 1-r4, opener-scan positive control): a
  rebuilt 1-r4 image flashed the same way, and a pre-mount `go` that reports
  `opener_positive_control=detected`, `opener_format_control=matched` and
  `opener_major_foreign=0` on the exact device.
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
  forcing it. On 2026-10-12 the CP stayed `ONLINE` after the daemon
  stopped, so a stop does not return the CP to `OFFLINE`. Only a reboot
  does, and a later arm needs one. Then require no CPIF holder, and run
  `ferrix-s5300-mounts` stop with its own verified, non-forced unmount.
  Require no `/mnt/vendor` mount and the arm token to be absent. Record
  the role-backed loops rather than require none: on 2026-10-12 they
  stayed attached after a successful stop. If any step refuses, stop and
  reboot rather than escalate. The teardown is complete only after the
  reboot that clears the CP state and the loops. After any teardown or
  reboot, re-run E1 and this preflight.
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

- **identity** (#108, real composed probe): the salted hash of each of the
  four candidates in order; absent, ambiguous (no fallback), and malformed
  serials; an absent or malformed boot ID; and no session salt. Every case
  checks that no raw serial, boot ID, or salt is printed.
- **decision binding** (#108, host workflow plus checker): a physical `go`
  writes one 0600 record with the documented keys, appended to its
  transcript and named in the summary. A fresh `go` on the same unit and
  boot passes. Each refusal token above is exercised: mock, `no-go`, and
  `incomplete` runs and records; another unit, source, or boot; an
  unavailable identity (the preflight stays `go`, but no record is
  written); the record's own transcript as facts; other phase, salt, pins,
  library, guest body, pin list, release, or factory pin; expired,
  future-dated, or longer-lived records; unknown keys; an unsealed edit; a
  missing or edited transcript; wrong modes for the record, salt, or salt
  directory (the preflight refuses with exit 6 before any session); no salt
  on the checking host; consume, second consume, a consumed file, a copy
  made before consumption, and a failed consume that still burns the
  record. Greps show that no raw serial, boot ID, or salt appears in any
  transcript, summary, record, ledger, or stdout.

`tools/pixel-9a/tests/s5300-mounts-service.sh` adds the partial-start
characterisation above.

Issue #107 (offline only): the real composed probe adds a detected
positive control on the clean case, and cases for a control the scan does
not see, an unlistable task directory, a vanished pid, and a foreign device
under a CPIF major (counted, never named). The host workflow adds NO-GO
cases for a missing control, a format mismatch, a foreign device, and an
unknown enumeration, and refuses an out-of-set or missing control fact.
Run on 2026-10-12 (macOS arm64 host, offline), `s5300-s2-preflight.sh`
passed all 275 checks and `s5300-cpif-services.sh` all 141 checks. Not
run: anything on the phone, BusyBox, mawk, or gawk (none is installed on
this host; the services suite runs its awk cases with each one it finds).

Earlier run on 2026-10-12 (macOS arm64 host, offline): `s5300-s2-preflight.sh`
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

Issue #108 run (2026-10-12, macOS arm64 host, offline): `s5300-s2-preflight.sh`
passed all 376 checks, `s5300-cpif-services.sh` passed, and `cargo fmt
--check` and `git diff --check` were clean. Three deliberate regressions,
each in a throwaway copy, were caught: a guest unit hash that skipped the
salt (the static check and the identity checks failed); a checker without
the boot comparison (all three boot-change cases failed); and a checker
without the ledger (the case of a copy made before consumption failed).
mawk, gawk, BusyBox, and shellcheck were not available on the host, so the
awk was written to the POSIX subset (no interval expressions and no gawk
extensions) and was not run under those tools.
