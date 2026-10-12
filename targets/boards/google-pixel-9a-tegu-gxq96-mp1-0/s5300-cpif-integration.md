# S5300 Google Kleaf CPIF service integration (issue #88)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.

This is the offline code slice of issue #88. It wires the reviewed
`--kernel-backend google-kleaf-cpif` adapter
([`s5300-cpif-adapter.md`](s5300-cpif-adapter.md)) and the private scratch
directory ([`s5300-scratch-dir.md`](s5300-scratch-dir.md)) into the target's
OpenRC services and packages. No phone, USB, ADB, Fastboot, builder, modem
node, or CP was touched, and no image was built. Capabilities do not change.
QEMU evidence does not apply to this board.

**Nothing starts a modem service by itself.** No S5300 service is linked into
any runlevel on either backend, and on `google-kleaf-cpif` the daemon starts
only after a manual preflight and a one-shot `arm`. Nothing here boots the CP;
the S2 manual CP boot needs its own reviewed workflow and decision (route
decision, stage 2).

## Backend selection

| | `linux-google-tegu-wwan` | `google-kleaf-cpif` |
| --- | --- | --- |
| Selected by | the package's `ferrix-backends.env` (unchanged) | the Kleaf dev composer, which rewrites only that one line in the image |
| `exynos-modem` CLI | `--model 5300 --uecap-dir /vendor/firmware/uecapconfig --boot-dev /dev/umts_boot0 --rfs-dev /dev/umts_rfs0 --oem-gems-dev /dev/umts_oem1` (unchanged) | `--model 5300 --kernel-backend google-kleaf-cpif --uecap-dir /vendor/firmware/uecapconfig --boot-dev /dev/umts_boot0 --rfs-dev /dev/umts_rfs0 --oem-gems-dev /dev/oem_ipc1` |
| `sit-proxy` CLI | `--dev /dev/wwan0sit0 --dev /dev/wwan0sit1 --socket-dir /run/sit-proxy` (unchanged) | `--kernel-backend google-kleaf-cpif --dev /dev/umts_ipc0 --dev /dev/umts_ipc1 --socket-dir /run/sit-proxy` |
| Supervision | `supervise-daemon`, `respawn_delay=2` (unchanged) | `start-stop-daemon` with a pidfile; **no respawn** |
| `start_pre` | the previous checks, unchanged | consume the arm token, then the full preflight below |

Any other `FERRIX_S5300_KERNEL_BACKEND` value, including an empty one, leaves
`command_args` empty and makes `start_pre` refuse. The CPIF node mapping is the
one in the adapter's table: boot DT ch 241 `umts_boot0`, RFS ch 41
`umts_rfs0`, OEM/GEMS ch 130 `oem_ipc1`, FMT ch 245/246 `umts_ipc0/1`. Both
init scripts also carry the exact `--print-config` text those CLIs must
produce, and the preflight compares it.

Why no respawn on CPIF: the daemon already handles CP recovery itself
(adapter, row R1). A daemon that exits should stay stopped until someone has
looked at the kernel log and run the preflight again, rather than reopening
`umts_boot0` every two seconds.

## Runlevel and dependency policy

- `device-google-tegu` (pkgrel 2) no longer links `ferrix-s5300-mounts`,
  `exynos-modem`, or `sit-proxy` into the default runlevel. It still installs
  all three. Before this change the package listed them, and only the Kleaf
  composer removed the links. The composer now refuses a package that lists
  them, and it still removes any such link from an older base rootfs.
- `modemmanager.initd` (`modemmanager-samsung-sit` pkgrel 3) changes
  `need ferrix-s5300-mounts exynos-modem sit-proxy` to `use`. OpenRC starts a
  `need` dependency even when it is in no runlevel, so the old line would have
  started the whole S5300 chain, and with it the CP, whenever ModemManager
  started from the default runlevel. `use` keeps the ordering but never pulls
  the chain in. ModemManager itself stays in the default runlevel, because
  `ferrix-telephonyd` needs it. The composer's older catch-up edit, which
  deleted that `need` line on the Kleaf image, is now a no-op for the new
  package.
- `exynos-modem` and `sit-proxy` choose their dependencies from
  `FERRIX_S5300_KERNEL_BACKEND`, and declare
  `config /etc/ferrix/backends.env` so OpenRC rebuilds its deptree when that
  file changes. On `linux-google-tegu-wwan` the old dependencies are kept:
  `exynos-modem` needs `ferrix-s5300-mounts`, and `sit-proxy` needs
  `ferrix-s5300-mounts` and `exynos-modem`. On `google-kleaf-cpif`, and on any
  backend that `start_pre` refuses, both only `use` and order `after` them.
  OpenRC starts a `need` before the dependent's `start_pre` runs. With `need`,
  a manual `rc-service exynos-modem start` would therefore have run the
  per-unit source mounts before any check or the arm token was looked at, and
  `rc-service sit-proxy start` would have tried to start the CP daemon too.
  The preflights still refuse unless those services are already started:
  `exynos-modem` requires `ferrix-s5300-mounts`, and `sit-proxy` requires
  `exynos-modem`. Both still `need` `ferrix-radio-firmware`. That service is a
  read-only firmware check, so letting OpenRC start it adds a check and changes
  nothing. `ferrix-telephonyd` only orders after the chain.
- With `use`, OpenRC no longer stops dependents first on CPIF. Stopping
  `exynos-modem` leaves `sit-proxy` running, and stopping `ferrix-s5300-mounts`
  leaves the daemon running. The manual stop order is `sit-proxy`, then
  `exynos-modem`, then `ferrix-s5300-mounts`. A wrong order fails closed.
  `ferrix-s5300-mounts`' `stop` never forces an unmount, so a mount that is
  still held stays mounted, read-only, over its loop. A later `arm` or `start`
  refuses while any CPIF node is held or the mounts are not started and
  verified.
- `99-google-tegu-sit.rules` is unchanged and tags only `wwan` SIT ports.
  No CPIF node is handed to ModemManager. Binding the SIT plugin to CPIF
  (or through the `sit-proxy` sockets) is S3 work and is **not** done here.
  ModemManager's own probing of the `cpif` class was not reviewed in the
  pinned fork's source. The preflight's open-file check (below) can detect a
  direct `/dev` opener, subject to the issue #92 scan limitation.

## The google-kleaf-cpif preflight

The checks live in `pmos/device-google-tegu/ferrix-s5300-cpif.sh`, which is
installed as `/usr/lib/ferrix/s5300-cpif.sh` and sourced by both init scripts.
They never open a CPIF node. `/dev` is touched only by `-L`/`-c` file tests,
`stat(2)`, and `readlink(2)` of `/proc/<pid>/fd` links. Per-unit files are
checked by `stat(2)` only. No firmware or NV byte is read or copied. Package,
identity, node and mount mismatches are refusals. The `/proc/<pid>/fd` opener
scan is a snapshot and can miss an inaccessible link or an alternate name
for the same character device; issue #92 must harden and review that scan
before an active CP boot.

`rc-service exynos-modem preflight` checks, in order:

1. **Target**: `FERRIX_TARGET` is exactly this board and the backend is
   `google-kleaf-cpif`; the device tree model is
   `ZUMA PRO TEGU MP on ZUMA PRO` with compatible `google,ZUMA PRO TEGU`;
   `uname -r` is the pinned Kleaf `6.1.124-android14-11-ga4662a84a15f`; and
   it is running as root.
2. **Package**: `/usr/share/ferrix/pixel-9a/s5300-cpif-services.sha256` names
   each of these exactly once, and each installed file (not a symlink) has the
   pinned SHA-256: `/etc/init.d/exynos-modem`, `sit-proxy`,
   `ferrix-s5300-mounts`, `modemmanager`, `/usr/lib/ferrix/s5300-cpif.sh`,
   `s5300-userspace-sources.toml`, and `99-google-tegu-sit.rules`. The apk
   database must record exactly one `exynos-modem` at `0.1.0_git20260731-r3`,
   the release that carries patches 0001–0003. A stale base rootfs, an
   overlaid script, or a hand edit is refused.
3. **CLI**: `/usr/sbin/exynos-modem <the CPIF command_args> --print-config`
   must print exactly the reviewed configuration (`oem_gems_dev=/dev/oem_ipc1`,
   `scratch_dir=/run/exynos-modem`, `crash_dump_boot=disabled`, …). An
   unpatched or older binary is refused. `--print-config` resolves the CLI and
   exits before any node is opened (adapter verification).
4. **Kernel surface (E1 rules)**: `cpif`, `shm_ipc`, and `cpif_page` each
   appear on exactly one `/proc/modules` line and are `Live`; exactly one
   `samsung,exynos-cp` platform device exists and is bound to `cp_interface`;
   its `modem_state` is exactly `OFFLINE`; and no `/sys/class/wwan` device
   exists.
5. **Nodes**: each of the twelve E1 nodes (`umts_boot0`, `umts_ipc0/1`,
   `umts_rfs0`, `oem_ipc0`–`oem_ipc7`) is a character device and not a
   symlink. It must have exactly one `/sys/class/*/<node>` entry, in class
   `cpif`, with subsystem `cpif`, whose `dev` equals the node's device number.
   A missing, ambiguous, mismatched, or foreign node is refused.
6. **Openers**: no `/proc/<pid>/fd` link names any of the twelve nodes.
7. **Sources**: `ferrix-s5300-mounts` must already be started. The installed
   service must hash to the #34-accepted
   `b90d3b41…f74b`. Its own `verify_own_mount` is then run, unchanged, in a
   subshell for `/mnt/vendor/{modem_img,efs,persist,modem_userdata}`. That
   proves each destination is still the read-only loop over its exact source
   with recovery suppressed. It mounts and unmounts nothing. `ferrix-s5300-mounts.initd`
   itself is byte-for-byte unchanged. Then, by metadata only: current-slot
   `modem.bin`, `efs/nv_normal.bin`, `efs/nv_protected.bin`, and
   `persist/modem/cpsha` must be regular files and not symlinks; both NV files
   must be at least `0x80000` bytes; and the UECAP and carrier-config
   directories must exist.
8. **Scratch**: `/run` must be tmpfs (`/proc/self/mountinfo`); `/` and `/run`
   must not be symlinks, must be root-owned, and must not be group- or
   other-writable; and `/run/exynos-modem` must be a root:root 0700 directory
   and not a symlink.

`rc-service exynos-modem provision` checks the target and the `/run` rules,
refuses a symlink or non-directory at `/run/exynos-modem` without repairing
it, then runs only `checkpath -d -m 0700 -o root:root /run/exynos-modem` and
re-checks the result. It opens no node and starts nothing.

`rc-service exynos-modem arm` (only while the service is stopped) runs the
full preflight and, only if it passes, creates the empty, root-only token
`/run/ferrix-s5300-cpif.armed` (noclobber, umask 077). It refuses if a token
already exists.

`rc-service exynos-modem start` on CPIF first consumes the token. The token
must be a regular file, not a symlink, root-owned 0600, with one link and size
0. It is removed before anything else, then the full preflight runs again. A
failed check therefore still needs a new `arm`. Because `/run` is tmpfs, a
reboot (including an AP panic caused by a CP crash) never leaves a token, so
the CP cannot be restarted at boot even if a link were added to a runlevel.

`rc-service sit-proxy preflight` and `sit-proxy`'s `start_pre` check the
target, the package pins, and the `sit-proxy --print-config` output. They
then require `exynos-modem` to be started, `modem_state` to be exactly
`ONLINE` (route decision: stage 3 only follows a stage 2 GO), `umts_ipc0/1` to
pass the node rules, and nobody else to hold them. They need no token: on
CPIF, `sit-proxy` only `use`s `exynos-modem`, so it never starts the daemon
itself, and it refuses unless the daemon, which needed a token to start, is
already running.

The intended manual order for a future S2/S3 run is therefore:
`rc-service ferrix-s5300-mounts start` → `rc-service exynos-modem provision`
→ `preflight` → `arm` → `start` → (only after an S2 GO) `rc-service sit-proxy
start`. That run needs its own checked-in, bounded workflow; this change does
not perform or authorise it. Issue #92 owns that workflow and its fresh
owner decision, including opener-scan hardening and enforced stop order.

## Files

| File | Change |
| --- | --- |
| `pmos/device-google-tegu/exynos-modem.initd` | per-backend CLI/supervision; per-backend `depend()` (CPIF: `use` the mounts); `preflight`, `provision`, `arm`; CPIF `start_pre`; WWAN `start_pre` body unchanged |
| `pmos/device-google-tegu/sit-proxy.initd` | per-backend CLI/supervision; per-backend `depend()` (CPIF: `use` the mounts and `exynos-modem`); CPIF `preflight` and `start_pre` |
| `pmos/device-google-tegu/ferrix-s5300-cpif.sh` | new check library (`/usr/lib/ferrix/s5300-cpif.sh`) |
| `pmos/device-google-tegu/s5300-cpif-services.sha256` | new installed pin list |
| `pmos/device-google-tegu/APKBUILD` | pkgrel 2; installs the two new files; S5300 services out of the runlevel list; sha512 re-pins |
| `pmos/device-google-tegu/s5300-userspace-sources.toml` | records `start_policy = "manual-only"` and both backends |
| `pmos/device-google-tegu/ferrix-backends.env` | comment only; the package value stays `linux-google-tegu-wwan` |
| `pmos/modemmanager-samsung-sit/{modemmanager.initd,APKBUILD}` | `need` → `use`; pkgrel 3 |
| `tools/pixel-9a/dev-compose.py` | sets the Kleaf image's backend line to `google-kleaf-cpif`; always removes S5300 runlevel links; refuses a package that links them |
| `tools/pixel-9a/rootfs.sh` | requires the two new package files |
| `ferrix-s5300-mounts.initd`, the exynos-modem recipe and patches, udev rule | **unchanged** |

To regenerate the pin list after an intended change to a pinned file, write
`<sha256>  <installed path>` for each path in the library's
`cpif_pinned_paths`, mapping `/etc/init.d/modemmanager` to
`pmos/modemmanager-samsung-sit/modemmanager.initd` and each other path to its
`pmos/device-google-tegu/` source. Then update the device `APKBUILD` sha512.
The test fails if either is stale. Changing `ferrix-s5300-mounts.initd` also
needs a new #34 review and new pins in the library and the NV workflow.

## Verification

`just pixel-9a-s5300-cpif-services-test`
(`tools/pixel-9a/tests/s5300-cpif-services.sh`, also run by
`tools/pixel-9a/tests/rootfs-workflow.sh`) checks:

- **static**: `ferrix-s5300-mounts.initd` still hashes to the #34-accepted
  value, which the library and the NV workflow also pin; the pin list names
  exactly the library's paths, once each, with the checked-in hashes; every
  local sha512 in both recipes matches; the new files are installed where the
  library looks; no S5300 service is in the runlevel list (ModemManager is);
  ModemManager has no `need` on the chain; the udev rule tags no CPIF node;
  the library's version and kernel pins match the recipe and the mount-proof
  workflow; both init scripts carry the exact WWAN and CPIF CLIs and the
  reviewed `--print-config` text; and CPIF node names and `/dev` paths appear
  only in the CLIs and in the library's file-test and `stat` lines;
- **stub harness**: both init scripts and the library are sourced with
  OpenRC helpers stubbed and every root redirected into a fixture. The twelve
  CPIF nodes are FIFOs: the single `-c` test is rewritten to `-p` in the
  fixture copy, so any `open()` of a node blocks and a 15 s bound fails the
  case. A fake `stat` reports device numbers. The binaries are stubs that
  log their arguments, and every case fails if a binary ran without
  `--print-config`. Covered: both backends' CLI and supervisor selection;
  each service's `depend()`, run with OpenRC's `need`/`use`/`after`/`before`/
  `config` keywords stubbed to record their arguments. On WWAN it must keep
  `need` on the mounts (and on `exynos-modem` for `sit-proxy`). On
  `google-kleaf-cpif`, an empty backend, and an unknown backend, the only
  `need` allowed is `ferrix-radio-firmware`, and the mounts and daemon must
  appear under `use` and `after`. A self-check confirms that a reintroduced
  `need` is caught;
  refusal of unknown and empty backends; preflight/arm/start, one-shot token
  consumption, and refusal of a re-arm or a forged or symlinked token; each
  target, package, binary, module, platform, `modem_state`, WWAN, node, class,
  opener, source, NV-size, `/run`, and scratch refusal listed above;
  provisioning and its refusals; and the `sit-proxy` ONLINE, ordering, and
  FMT-holder gates;
- **composer**: the Kleaf composer rewrites only the backend line, is
  idempotent, refuses a missing, duplicate, or unknown backend line, leaves
  the new ModemManager service alone, and always strips the three S5300
  runlevel links.

`tools/pixel-9a/tests/exynos-modem-cpif-adapter.sh` now checks the exact
per-backend CLIs and the unknown-backend refusal instead of "still refuses
CPIF". With the pinned archive it also drives the real patched binaries
through the exact service CLIs.

Run on 2026-10-12 (macOS arm64 host): `s5300-cpif-services.sh` (89 checks)
passed. `exynos-modem-cpif-adapter.sh` passed in full mode
(`FERRIX_S5300_ADAPTER_FETCH=1 FERRIX_S5300_ADAPTER_FULL=1`: hash-verified
archive, patches applied, pinned Alpine aarch64 container with
`--network none`): 46 `exynos-modem`, 25 `sit-proxy`, and 9 `samsung-sit-ffi`
unit tests, and all 16 CLI checks, including the two new ones in which the real
patched binaries, given the exact init-script CPIF CLIs, print exactly the text
the preflight expects. `s5300-mounts-service.sh`, `rootfs-workflow.sh`, and the
existing S5300 workflow suites also passed. Two deliberate regressions were
caught: adding a read of a node to the library failed the static scan and made
the node cases time out, and dropping the token consumption failed the arm
cases. A later run added the nine `depend()` cases (98 checks, passed). A
deliberate regression, putting `need` back in both CPIF branches, failed all
six CPIF and refused-backend `depend()` cases, plus the two stale-pin checks.
**Not run:** OpenRC itself (`checkpath`, `service_started`,
`start-stop-daemon`, `use` semantics), busybox `stat`/`sha256sum` on the guest,
abuild/pmbootstrap packaging, and anything on the device.

## Limitations and blockers

- **ModemManager CPIF binding (S3) is not implemented.** Nothing here can make
  `mmcli -L` list a modem on CPIF. This needs the SIT plugin to accept the
  CPIF class or bind through the `sit-proxy` sockets, plus
  `ID_MM_PHYSDEV_UID` grouping. That has to be reviewed against the pinned
  fork's source; it was not guessed.
- **No log capture.** As before, the services set no `output_log`. An S2
  workflow must collect the daemon's output and `dmesg` itself.
- **Re-arming after a crash.** If the daemon exits after the CP left
  `OFFLINE`, the next preflight refuses until `modem_state` is `OFFLINE`
  again. That is deliberate, but recovering from a mid-boot CP may need a
  reboot.
- The `cp_interface` driver name, `cpif` class, and kernel release are pinned
  to the values E1 observed on the `674157b` image. A rebuilt Kleaf kernel
  with any difference is refused, not accepted.
- U4 driver/firmware skew, U6 AP panic behaviour, FMT byte equivalence (U3),
  firmware acceptance, and `cpsha`/DT identity values remain unknown (route
  decision).

## Next physical checks (lead only, after review and CI)

On the exact booted phone, from an image built through the checked-in
builder and flashed through the recovery-gated workflow, with no modem
service started and no CPIF node opened:

1. The installed `device-google-tegu` is pkgrel 2, `modemmanager-samsung-sit`
   pkgrel 3, and `exynos-modem` `0.1.0_git20260731-r3`. Each path in
   `/usr/share/ferrix/pixel-9a/s5300-cpif-services.sha256` matches
   (`sha256sum -c`), and `/etc/init.d/ferrix-s5300-mounts` still hashes to
   `b90d3b41…f74b`.
2. `/etc/ferrix/backends.env` has
   `FERRIX_S5300_KERNEL_BACKEND=google-kleaf-cpif`, and no runlevel under
   `/etc/runlevels/` links `ferrix-s5300-mounts`, `exynos-modem`, or
   `sit-proxy`. `rc-status` shows them stopped after boot, and no
   `/run/ferrix-s5300-cpif.armed` exists.
3. `/run` is tmpfs and `/` and `/run` are root-owned 0755. After
   `rc-service exynos-modem provision`, `/run/exynos-modem` is a root:root
   0700 directory and not a symlink. Record the OpenRC version.
4. `rc-service exynos-modem preflight` **without** starting
   `ferrix-s5300-mounts` must refuse with "start ferrix-s5300-mounts manually".
   It must have passed the target, package, CLI, surface, node, and opener
   checks, which shows they hold on the device. Starting the mounts and
   re-running the preflight is part of the S2 workflow. Do not run `arm` or
   `start` in this issue.
5. Re-run the E1 inventory, and confirm that no process holds a CPIF node with
   ModemManager running from the default runlevel.
