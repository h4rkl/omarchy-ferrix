# S5300 cellular route decision (issue #1)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.

This is a source-grounded engineering decision, not physical evidence. No
modem, SIM, registration, data, SMS, voice, or emergency function is claimed
on any route. `capabilities.toml` keeps `telephony = "unsupported"` and
`docs/status.md` is not changed until exact-target evidence exists. QEMU mock
telephony is not evidence for this board.

## Document map

Since #119 (2026-10-13) this file is the S5300 **route and design** record. It
holds, in order, verbatim and one heading level deeper:

1. the route decision itself (issue #1, below);
2. [the Google Kleaf CPIF userspace adapter](#s5300-google-kleaf-cpif-userspace-adapter-issue-1)
   (issue #1, formerly `s5300-cpif-adapter.md`);
3. [the daemon scratch directory](#s5300-daemon-scratch-directory-issue-81)
   (issue #81, formerly `s5300-scratch-dir.md`);
4. [the Google Kleaf CPIF service integration](#s5300-google-kleaf-cpif-service-integration-issue-88)
   (issue #88, formerly `s5300-cpif-integration.md`).

Later parts update earlier ones; where a part carries an **Update** note, the
later part is current. Related S5300 records:
[`s5300-source-evidence.md`](s5300-source-evidence.md) (source and ABI
evidence, including E0), [`s5300-s2-preflight.md`](s5300-s2-preflight.md) (S2
preflight, physical S2 results and the active-workflow blockers) and
[`s5300-s2-active-design.md`](s5300-s2-active-design.md) (#109 teardown and
observation design, a standalone review document).

## Decision

**Try route A first: keep the booting Google Kleaf kernel and bridge its CPIF
character-device surface to the existing external S5300 userspace
(`exynos-modem`, `sit-proxy`, SIT ModemManager plugin) through target-local
patches.** Route B (the radio-first `linux-google-tegu` WWAN/SIT kernel) stays
the fallback.

The choice is conditional on two cheap gates that run before any modem
character device is opened: E0, a device-free ABI diff, and E1, a read-only
CPIF inventory on the current dev boot (see "Decision experiment"). If either
gate fails in the ways listed there, switch to B.

Why A first:

1. **It is the only kernel that boots this exact board.** The Kleaf route
   reaches Home on slot b with Wi-Fi, Bluetooth discovery, and Waydroid
   (docs/status.md, 2026-10-09). It uses Google's exact MP1.0 device tree,
   which carries the CP PCIe, GPIO, PMIC, and S2MPU wiring the modem needs.
   Route B has never booted. Its base DT has only 49 of the 113 labels the
   stock MP overlay needs (`linux-plan.md`), and the one related probe ended
   in `watchdog,apc,early`. Under B, the modem cannot be tested until a whole
   board port boots, and that port would also put display, UFS, USB, Wi-Fi
   (brcmfmac rather than bcmdhd), Bluetooth, and Waydroid kernel support back
   to unproven.
2. **The known ABI gap is in userspace; its full extent is unknown.** The pinned
   `exynos-modem` CLI already uses CPIF-style node names for boot, RFS, and
   OEM/GEMS (`/dev/umts_boot0`, `/dev/umts_rfs0`, `/dev/umts_oem1`;
   `exynos-modem.initd`). The 2026-09-28 ABI audit (`s5300-source-evidence.md`)
   found three concrete differences:
   - the CP reset/status (and boot) ioctl argument encodings differ;
   - FMT control arrives on Google `umts_ipc0`/`umts_ipc1` (SIT channels
     245/246) rather than `wwan0sit0`/`wwan0sit1`, and the ModemManager
     plugin only watches the `wwan` subsystem;
   - the OEM node is named `oem_ipc0`–`oem_ipc7` in Google's DTS, not
     `umts_oem1`.

   Each has a plausible patch in the external GPL packages, subject to E0 and
   the CP-boot result. Udev rules or `sit-proxy` arguments alone cannot fix
   them, as the audit says.
3. **A smaller change means a smaller blast radius.** A changes rootfs
   packages and init scripts on the dev slot only. The kernel, DT, and
   bootloader inputs that already boot stay the same.

What would make B the better first choice: E0 shows that Google CPIF's CP
boot needs userspace behaviour not visible in the GPL kernel source (for
example opaque handshakes that Android's proprietary CP boot daemon
performs). Or E1/S2 shows a kernel-side CPIF failure that userspace cannot
work around.

## Known evidence vs unknowns

| Item | Status | Source |
| --- | --- | --- |
| Kleaf route boots exact device to Home | Physical, exact target | docs/status.md 2026-10-09 |
| Kleaf build includes `shm_ipc`, `cpif_page`, `cpif` | Source/config (`modules-initfs`, `kernel-config.tegu`) | not observed loaded |
| DT selects `PROTOCOL_SIT`; FMT 245/246, RFS 41, OEM 129–136, rmnet 181–210, boot 241 | Pinned source | `s5300-channels.toml` |
| tegu SIT payload framing; the seven-byte `sipc_fmt_hdr` belongs to the SIPC path | E0 pinned-source comparison; Rust codec correction tracked in #76 | not exercised on device |
| Opening `umts_boot0`/`umts_ipc*` calls `init_comm` and changes open counts; the pinned SIT handler returns without link setup | Pinned source, E0 | runtime module skew remains, so E1 does not open nodes |
| CP boot inputs: TOC `BOOT`/MAIN/VSS/APM from current-slot `modem.bin`, EFS NV, persist `cpsha`, runtime 161-byte handover | External `exynos-modem` source | not exercised |
| Physical S5300 + stock firmware can attach on this board | External Halium port report | not Ferrix evidence; Android RIL path |
| **U1** Does `cpif` load and probe on the dev boot; `modem_state`; which `/dev` nodes and class name exist | Physical E1 `go`: three modules Live, one bound `cp_interface`, `OFFLINE`, twelve `cpif` class character nodes; 30 `rmnet*` before CP boot | E1; issue #1 |
| **U2** Google CPIF boot/load ioctl contract (image struct, offsets, sequence) fully defined by GPL kernel source | E0 found the normal cold-boot layouts and encodings defined and factory-module commands matching; runtime firmware acceptance remains unknown | E0, then S2 |
| **U3** FMT payload on `umts_ipc0/1` byte-identical to what `wwan0sit0/1` gives `sit-proxy` | Unknown (same CP firmware; both kernels strip the SIT link header, still unproven) | E0, then S3 |
| **U4** Driver/firmware skew: pinned CPIF `a8976934` (android16, 6.1.124 dev GKI) vs CP3A `modem.bin` shipped with stock 6.1.162 | Unknown | E0 (offline `modinfo` of factory `cpif.ko`), S2 |
| **U5** SIT/GEMS command vocabulary sufficient for SIM/attach/data/SMS/voice | External plugin's claim only | S3–S7 |
| **U6** CP crash handling on Kleaf panics the AP (boot-loop risk) | Unknown | S2 under manual start |
| **U7** `efs`/`persist`/`modem_userdata` filesystem types and whether a plain `ro` mount replays a journal | Physical inventory: all three are f2fs; plain `ro` is unsafe by pinned-kernel source review. One owner-approved read-only-loop mount proof per role completed without observed writes; owner accepted the final mount gate on 2026-10-12 | pre-S2 gate; issue #34 |

## Required interfaces

Route A, kernel (unchanged Kleaf build):
`cpif` platform driver bound to `samsung,exynos-cp`; sysfs `modem_state`;
`/dev/umts_boot0` (boot/status ioctls, CP image load), `/dev/umts_ipc0`,
`/dev/umts_ipc1` (FMT), `/dev/umts_rfs0` (RFS), `oem_ipc*` (GEMS); raw-IP
`rmnet0`–`rmnet29` netdevs.

Route A, userspace (target-local patches to external packages):
- `exynos-modem`: a CPIF ioctl backend selected by an explicit flag, with
  encodings taken from the pinned `modem_prj.h` and the kernel's ioctl
  handlers, never guessed; the OEM device path set to the observed
  `oem_ipc*` node.
- `sit-proxy`: given `/dev/umts_ipc0 /dev/umts_ipc1` as `--dev` arguments
  (it already takes paths).
- SIT ModemManager plugin: accept the CPIF class/subsystem from E1 alongside
  `wwan`, or bind through the `sit-proxy` sockets. Group the FMT ports and
  `rmnet*` netdevs into one modem with standard udev
  `ID_MM_PHYSDEV_UID` tagging, because CPIF nodes do not share a WWAN parent
  device.
- ModemManager D-Bus (`Modem`, `Modem3gpp`, `Sim`, `Bearer`, `Messaging`,
  `Voice`/`Call`) consumed unchanged by `ferrix-telephony-mm` →
  `ferrix-telephonyd`. NetworkManager owns the rmnet data link.

Route B: built-in `s5300_modem` WWAN driver (`CONFIG_S5300_MODEM=y`) that
creates `umts_boot0`, `wwan0sit0/1`, an AT port, and `rmnet0`–`rmnet29` after
CP ONLINE. It uses the pinned userspace unmodified, but first needs a booting
MP1.0 kernel/DT port.

## Where the work belongs

| Work | Location | Not here |
| --- | --- | --- |
| Patches to `exynos-modem` / ModemManager plugin (GPL, process boundary) | `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/{exynos-modem,modemmanager-samsung-sit}/` as hash-pinned APKBUILD patch sources | not vendored; not linked into Ferrix Rust crates |
| Backend selection (`google-kleaf-cpif` vs `linux-google-tegu-wwan`), per-backend init args, CPIF udev rules | `pmos/device-google-tegu/` (`ferrix-backends.env`, `*.initd`, `99-google-tegu-sit.rules`) | not in apps, `ferrix-core`, `ferrix-hardware-api` |
| Opt-in modem services on the Kleaf dev image (today they are stripped) | `tools/pixel-9a/dev-compose.py` (`RADIO_ROUTE_SERVICES`) | — |
| Read-only CPIF observation | existing `ferrix_s5300_cpif::discover_at` and the `radios.sh` CPIF probe | must never open IPC/boot nodes |
| Telephony mapping | existing `ferrix-telephony-mm` (observer of ModemManager objects) | must not become a SIT client; no Rust reimplementation of the modem protocol (baseline: do not reimplement device protocols) |
| Capability promotion | `capabilities.toml` + `docs/status.md`, per stage, with physical evidence | not from this document |

No Android RIL, HAL, Binder/AIDL, `shared_modem_platform`, CP boot daemon,
or SELinux policy enters either route.

## Decision experiment (run before writing the bridge)

**E0, device-free source diff.** Compare three things:
- Google S5300 `a8976934`: `modem_prj.h` plus the handlers for
  `IOCTL_START_CP_BOOTLOADER`, `IOCTL_LOAD_CP_IMAGE`,
  `IOCTL_COMPLETE_NORMAL_BOOTUP`, the power/reset/status ioctls,
  `ipc_io_device.c`, and `modem_ctrl_s5100.c`;
- the `s5300_modem` driver at `linux-google-tegu` `88335b7`;
- `exynos-modem` `634e8eb` (its ioctl definitions, boot sequence, and RFS
  write handling).

Also run offline `modinfo` on `cpif.ko` from the pinned factory
`vendor_kernel_boot`/`vendor_dlkm` to compare with the dev build.

Expected for GO on A:
- Every boot step `exynos-modem` performs maps to a Google CPIF ioctl or
  write whose argument layout the GPL source fully defines.
- The source-defined FMT payload layout and header-stripping behaviour are
  compatible enough to justify a bounded runtime test; E0 cannot prove byte
  equivalence on this device.
- With issue #81's target-local patch 0003, RFS NV writes and crash records
  go only under private `/run/exynos-modem`; source mounts are opened
  read-only. Confirm the patched directory and `/run` backing on the
  deployed image before S2 (see
  [`s5300-scratch-dir.md`](#s5300-daemon-scratch-directory-issue-81)).

Switch to B if CPIF boot needs undefined userspace data or sequencing.

**E0 result (2026-10-12, [`s5300-cpif-e0.md`](s5300-source-evidence.md#s5300-e0-device-free-cpif-abi-comparison-issue-1)): conditional
GO for A at the ABI level; not by itself a CP boot authorisation.** Every
`exynos-modem` cold-boot ioctl and `read`/`write` has a GPL-defined layout in
`a8976934`. The factory CP3A `cpif.ko` uses the same encodings (checked by
disassembly). The undefined parts (std_dl opcodes, handover field meaning,
TOC) are firmware behaviour that both routes only carry, so they do not
favour B. Before any S2 run, three bounded `exynos-modem` adapters are
required: the CP reset encoding, `GET_SRINFO` (or disabling the dump path),
and the OEM read loop, which treats CPIF's 100 ms empty read as EOF. The
OEM and FMT paths also need CLI arguments (`/dev/oem_ipc1`,
`/dev/umts_ipc0/1`). U4 skew remains open: the factory module comes from
unpublished source `fd5221767c57`. NV file sizes are also unknown. The next
step is E1, plus read-only NV presence and size checks once S0 allows it.

**Adapter (2026-10-12, [`s5300-cpif-adapter.md`](#s5300-google-kleaf-cpif-userspace-adapter-issue-1)):
code only.** Two hash-pinned patches in `pmos/exynos-modem/` add an explicit
`--kernel-backend google-kleaf-cpif` to `exynos-modem` and `sit-proxy`. On
that backend, reset uses `IOCTL_POWER_RESET` with a `boot_mode` pointer, and
srinfo uses the inline layout. The crash-reason dump-boot is disabled (a
diagnostic limit), OEM reads are poll-gated with 0 meaning no data, and the
nodes are fixed to `umts_boot0`, `umts_rfs0`, `oem_ipc1`, and `umts_ipc0/1`.
These are offline-tested only. The init scripts still refuse the CPIF
backend, so wiring it in remains separate integration work before S2.

**Integration (issue #88, [`s5300-cpif-integration.md`](#s5300-google-kleaf-cpif-service-integration-issue-88)):
code only.** `exynos-modem.initd` and `sit-proxy.initd` select the explicit
CPIF CLI under `FERRIX_S5300_KERNEL_BACKEND=google-kleaf-cpif`, which the Kleaf
composer sets. No S5300 service is in a runlevel; ModemManager only `use`s
them. On CPIF, `exynos-modem` starts only after a read-only `preflight` and a
one-shot `arm` on tmpfs `/run`. These check the target, kernel, package pins,
binary configuration, E1 surface with `OFFLINE`, nodes, openers, the
#34-accepted mounts, NV sizes, and the scratch directory, and open no CPIF
node. ModemManager CPIF binding (S3) is not implemented.

**E1, read-only dev-boot inventory** (`just pixel-9a-s5300-cpif-inventory`
over CDC-ACM; nothing opened; the exact `go`/`no-go`/`incomplete` rules and
remaining unknowns are in the
[README](README.md#s5300-cpif-e1-inventory-issue-1)):
- `/proc/modules` contains `cpif`, `shm_ipc`, and `cpif_page`;
- the `samsung,exynos-cp` platform device is bound;
- `modem_state` is exactly `OFFLINE` for an E1 `go`;
- `/dev/umts_boot0`, `umts_ipc0`, `umts_ipc1`, `umts_rfs0`, and `oem_ipc*`
  are character devices, with their class/subsystem recorded;
- Record whether `rmnet*` netdevs exist before CP boot; their absence at this
  stage alone does not reject A.

`go` needs all of these, exactly one bound platform device, exactly
`OFFLINE`, all twelve nodes in one sysfs class with matching device numbers,
and no WWAN surface. An unreadable fact gives `incomplete`, never `go`.
The exact-device run on the `674157b` image met those criteria without
opening any CPIF node. The observed `rmnet*` count was 30 before CP boot;
this does not establish a data connection.
If the modules are not loaded, fix module loading first (cheap). If the
platform node is missing or probe fails in dmesg, that is a kernel/DT problem:
investigate it before any userspace work.

## Staged proof sequence (route A)

Each stage records its evidence under ignored `out/` with redacted IMEI,
ICCID, IMSI, MSISDN, and cell IDs. A stage is complete only when its
observation is captured; later stages never imply earlier ones.

0. **Pre-S2 safety gate.** Confirm the U7 filesystem types and change
   `ferrix-s5300-mounts.initd` before enabling it. Mount `efs`, `persist`,
   and `modem_userdata` with recovery writes disabled (`ro,noload` for
   [ext4](https://docs.kernel.org/admin-guide/ext4.html), `ro,norecovery`
   for [f2fs](https://docs.kernel.org/filesystems/f2fs.html)); a plain `ro`
   ext4 mount can replay a dirty journal and write to the device. Refuse an
   unknown filesystem type. Confirm `exynos-modem` opens source mounts
   read-only and writes NV sinks and crash records only under
   `/run/exynos-modem` (patch 0003, [`s5300-scratch-dir.md`](#s5300-daemon-scratch-directory-issue-81);
   code and offline tests only); check that directory's runtime backing,
   owner, and mode read-only before S2. A pinned 6.1.124 source review found
   that these options leave orphan-cleanup and error-superblock writes
   possible on both filesystems. Only a read-only device skips them;
   `BLKROSET` alone warns about writes rather than failing them. A loop attached with
   `losetup --read-only` fails every write before it reaches the partition
   ([README](README.md#s5300-source-mount-proof-issue-34)). The gate
   therefore also needs `ferrix-s5300-mounts` itself to set `BLKROSET` and
   mount only from a verified read-only loop. The checked-in service now
   does this (offline stub-tested). The exact-device rootfs contained the
   byte-identical service, which stayed out of runlevels. The physical
   preflight and one gated mount proof per role completed on slot B: `modem_b`
   ext4, plus owner-approved per-unit f2fs `efs`, `persist`, and
   `modem_userdata`. Every proof reported a verified read-only loop, contract
   met, no observed write anomaly, and clean unmount/detach. These observations
   do not cover another opener, whole-disk I/O, or UFS passthrough. The owner
   explicitly accepted the final #34 mount gate on 2026-10-12 after reviewing
   these results. The workflow correctly continues to report
   `owner-review-required`, because only the human decision passes this gate.
   No CP boot has run. E0 and E1 are separate prerequisites before a manual
   CP boot trial, along with adapter review and read-only NV checks.
   The NV check is the separate issue #79 workflow
   ([README](README.md#s5300-nv-metadata-preflight-issue-79)). It mounts
   only `efs`, through the unchanged #34 read-only loop and behind its own
   per-run gate, and reports only the presence, type, and size of
   `nv_normal.bin` and `nv_protected.bin` against `0x80000`. Its offline
   tests pass. On 2026-10-12 its one owner-approved exact-device run found
   both files regular and exactly `0x80000` bytes, with the loop barrier
   verified, clean teardown, and no reported write anomaly. This meets the
   metadata-size gate only; firmware acceptance is unproved. `cpsha` and
   the device-tree identity values remain unchecked.
1. **E1 inventory** (above).
2. **CP boot.** Start manually, not from the default runlevel, so a
   CP-crash panic cannot boot-loop:
   `ferrix-s5300-mounts`, then `exynos-modem --<cpif backend>`.
   - GO: `modem_state` reaches ONLINE and stays there for at least 10
     minutes, with no CP crash dump or AP panic, and RFS requests are
     served.
   - STOP: repeated crash, handshake timeout, or AP reset. Capture dmesg and
     the boot log, then re-check U4.
   - Observed 2026-10-12 (`physical:google-pixel-9a-tegu-gxq96-mp1-0`, one
     manual owner-approved run on 1-r3): `ONLINE` for about 13.9 minutes of
     samples, with no panic. RFS service was not observable (no daemon
     output). Stopping the daemon left the CP `ONLINE`. See
     [`s5300-s2-preflight.md`](s5300-s2-preflight.md#exact-device-s2-active-cp-boot-on-the-1-r3-image-2026-10-12).
3. **Control path.** Start `sit-proxy` on `umts_ipc0/1`, then the patched
   ModemManager.
   - GO: `mmcli -L` lists one modem using the `samsung-sit` plugin; IMEI is
     readable (redacted); the SIM state is reported, either present or
     missing.
4. **SIM and registration.**
   - GO: the SIM is unlocked (PIN flow if needed); `Modem3gpp` reports home
     or roaming registration, the operator, and signal quality;
     `ferrix-telephonyd` reflects them.
   - Only after this passes is `telephony` eligible for promotion, scoped to
     "registration observed".
5. **Packet data.**
   - GO: a bearer connects and `rmnet*` gets addresses; a NetworkManager GSM
     connection reaches DNS and ping with Wi-Fi off; disconnect and
     reconnect both work.
6. **SMS.**
   - GO: receive and send against the owner's own number; PDU and storage
     visible through `Messaging`.
7. **Call state.**
   - GO: MO and MT call objects transition dialing → active → terminated
     through `Voice`/`Call`.
   - Call audio is out of scope (audio is `unsupported`). VoLTE/IMS is a
     separate later gate.
   - **Never dial emergency numbers for testing.** Limited-service or
     emergency registration states are not emergency-calling evidence.
8. **Cold-boot repeat.** Move the services to the default runlevel. Pass
   three consecutive cold boots to stage 4 with the product shell still
   reaching Home if the modem fails. Repeat after a suspend/resume.

## Rollback and recovery implications

- Route A uses the existing `just pixel-9a-dev-flash` image path. That path
  may write shared `userdata`; a slot switch first writes the pinned stock
  bootloader and radio images to the target slot. Rollback options are to
  disable the services, reflash a reviewed dev image, or use the pinned stock
  factory restore. No rollback is assumed to preserve user data.
- **The factory image does not restore `efs` or `persist`; treat
  `modem_userdata` as equally sensitive until its recovery semantics are
  established.** These partitions may contain device-specific NV or
  calibration data. No write to them is admitted by the development route.
  They must stay read-only with journal replay disabled (stage 0). Neither
  route may add a write path to them.
- The modem image is read from the current-slot `modem_{a,b}` partition.
  `pixel-9a-dev-flash` writes stock radio to the target slot before a slot
  switch, so Ferrix reads stock CP firmware; it never writes modem
  partitions.
- A CP crash that panics the AP can leave the phone rebooting. The human
  holds Volume Down + Power to return to Fastboot (AGENTS.md). Manual service
  start in S2–S7 keeps this to one reboot.
- Route B adds whole-board risk on top: an unbooted kernel and DT, watchdog
  history, and no console until early boot works. Recovery is the same
  factory restore, but expect more round trips.

## Stop/go summary

| Gate | Go | Stop → action |
| --- | --- | --- |
| E0 | Boot ABI fully defined in GPL source; FMT payload equivalent | Undefined boot data → pursue B (MP1.0 overlay port first) |
| E1 | CPIF bound, nodes present, not ONLINE | No platform/probe → kernel/DT investigation on Kleaf |
| S0 | Calibration partitions confirmed non-writing | Cannot guarantee → do not open `umts_boot0` |
| S2 | ONLINE ≥10 min, no panic | Repeated crash → U4 skew analysis; then B |
| S3 | `mmcli -L` lists modem | Plugin cannot bind CPIF ports → plugin patch; still failing → B |
| S4–S7 | Per stage above | Missing command vocabulary → record the gap upstream; do not reverse-engineer it into Ferrix Rust |

## Open risks

- U4 driver/firmware skew may defeat both routes equally. A fix might need
  CPIF built from the revision matching the CP3A factory build.
- The external SIT plugin's attach/data/SMS/voice claims are unverified for
  this board and firmware build.
- Patch maintenance: two external GPL forks gain target-local patches, and
  upstreaming them should be offered.
- Carrier, VoLTE/IMS, emergency calling, and certification are out of scope
  and remain unsupported regardless of outcome.


## S5300 Google Kleaf CPIF userspace adapter (issue #1)

> **Moved here on 2026-10-13 (#119)** verbatim from `s5300-cpif-adapter.md`, with every
> heading one level deeper. The old path is a stub that maps each old
> section anchor to its new one.

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.

This adds the route A userspace adapter that
[`s5300-cpif-e0.md`](s5300-source-evidence.md#s5300-e0-device-free-cpif-abi-comparison-issue-1) asked for (rows R1, R2, O1 and the P1
CLI paths). It is code and offline tests only. No phone, USB, ADB, Fastboot,
builder, modem node, or CP was touched. Capabilities do not change. QEMU
evidence does not apply to this board.

**Nothing selects the new backend.** `ferrix-backends.env` still pairs the
userspace with `linux-google-tegu-wwan`. `exynos-modem.initd` and
`sit-proxy.initd` still refuse every other backend, and no modem service is
in a default runlevel. Running the CPIF backend on the device needs a separate,
reviewed integration change (see "Still required").

**Update (issue #88):** the integration is now in code, offline-tested only;
see [`s5300-cpif-integration.md`](#s5300-google-kleaf-cpif-service-integration-issue-88). The init scripts
map each backend to its own explicit CLI. The Kleaf composer pairs the image
with `google-kleaf-cpif`, and the CPIF daemon starts only manually after a
read-only preflight and a one-shot `arm`. Still no S5300 service is in a
runlevel, and no CP boot has run.

### What changed

The pinned `exynos-modem` archive (`634e8eb4`, SHA-256 `324efde8…a9445f`) is
unchanged. `pmos/exynos-modem/APKBUILD` (pkgrel 2) applies two target-local
patches through abuild's `default_prepare` (`patch -p1`). Both are SHA-512
pinned in the recipe and SHA-256 pinned in
`device-google-tegu/s5300-userspace-sources.toml`.

| Patch | Scope |
| --- | --- |
| `0001-exynos-modem-google-kleaf-cpif-backend.patch` | `exynos-modem`: new `backend.rs`; `--kernel-backend` and `--print-config`; per-backend reset and srinfo encodings; dump-boot gate; poll-gated OEM loop on CPIF; unit tests |
| `0002-sit-proxy-google-kleaf-cpif-nodes.patch` | `sit-proxy`: `--kernel-backend` (CPIF defaults to `umts_ipc0/1` and refuses other paths) and `--print-config`; unit tests |

The patches touch only `crates/{exynos-modem,sit-proxy}/src/*.rs`. They add
no crate, change no `Cargo.lock`, build script, or cargo config, and add no
process or network code (the test checks this). Upstream formatting is kept;
only new code is rustfmt-formatted.

### Backend selection

`--kernel-backend linux-google-tegu-wwan` is the default, and its behaviour
is the upstream behaviour: the same ioctls, the same free-form `--*-dev`
paths, and the same blocking OEM read that treats 0 as EOF.
`--kernel-backend google-kleaf-cpif` is only ever chosen explicitly. The
value must match exactly, and giving the flag twice is refused.

| Channel | WWAN default | CPIF node (the only path accepted) | CPIF source |
| --- | --- | --- | --- |
| boot | `/dev/umts_boot0` | `/dev/umts_boot0` | DT ch 241 |
| RFS | `/dev/umts_rfs0` | `/dev/umts_rfs0` | DT ch 41 |
| OEM/GEMS | `/dev/umts_oem1` | `/dev/oem_ipc1` | DT ch 130 (the radio-first driver names this channel `umts_oem1`) |
| SIT (`sit-proxy`) | `/dev/wwan0sit0`, `/dev/wwan0sit1` | `/dev/umts_ipc0`, `/dev/umts_ipc1`, each at most once | DT ch 245/246 |

On CPIF each channel also accepts `none`. Any other path, including a
trailing `/`, exits with status 2 before a thread starts or a node is opened.
`--print-config` prints the resolved configuration and exits without opening
anything; the offline test uses it.

### Operation mapping on `google-kleaf-cpif`

Encodings come from Google CPIF `a8976934` `modem_prj.h` and its handlers.
The factory CP3A `cpif.ko` compares the same numbers (E0). Unit tests assert
each value and the argument layout.

| Row | WWAN (unchanged) | CPIF | Notes |
| --- | --- | --- | --- |
| R1 reset | `_IO('o',0x21)` = `0x6f21`, integer arg Warm 0 / Dump 1 / Cold 2 | `IOCTL_POWER_RESET` `_IOW('o',0x21,4)` = `0x40046f21`, pointer to `struct boot_mode { enum cp_boot_mode }` | Mapped by kernel action: **Cold → NORMAL (0)** (`power_reset`, full power off/on), **Dump → DUMP (1)** (`power_reset_dump(mc,0)`, wreset with DUMP_NOTI high), **Warm → SILENT (3)** (`power_reset_dump(mc,1)`, wreset with DUMP_NOTI low). The radio-first reset policy is kept: none for a cold boot from OFFLINE, Cold when the CP is found mid-boot at startup and on every failed-boot retry, Warm after a supervised crash. Before this, every retry got `-EINVAL` from `shmem_ioctl` and the CP never reset |
| R2 srinfo | `_IOWR('o',0x45,16)`, `{u64 buf, u32 len, u32 rsvd}` | `_IO('o',0x45)` = `0x6f45`, `struct shmem_srinfo { u32 size; char buf[]; }`: the caller writes its capacity, and the kernel writes back `min(region, capacity)` and copies the record inline | Implemented and tested against a model of `shmem_ioctl`. A returned length larger than the buffer is refused. **The crash-reason dump-boot is disabled on CPIF**, so nothing in the boot flow reaches it (next row) |
| R2 dump-boot | reset(Dump) + LOAD PBL + START(DUMP) + GET_SRINFO, once per supervised crash | not run; logs `crash-reason dump-boot disabled on google-kleaf-cpif; see the kernel log` | `START_CP_BOOTLOADER(DUMP)` goes into CPIF's `start_dump_boot`, whose link and PCIe effects have not been reviewed against CP3A. **Diagnostic limit:** on CPIF no `/tmp/modem_crash_*.json` record is written; the kernel log is the only source of the crash cause. Recovery (Warm → SILENT, then a normal boot) still runs |
| O1 OEM read | blocking `read()`; `Ok(0)` = EOF → close, sleep 1 s, reopen | `poll(POLLIN, 1000 ms)` before each `read()`; `Ok(0)` and `EINTR` go back to poll; POLLHUP/POLLERR/POLLNVAL end the session (reopen after 1 s); queued data is drained before a hangup | `ipc_read` waits 100 ms and returns 0 on an empty queue. `ipc_release` purges the rx queue, and `rx_demux` drops frames for a closed node, so the handle now stays open while the link is up. There is no busy loop: each iteration blocks in poll unless the kernel reports queued data, and an empty read also costs the kernel's 100 ms wait. CPIF returns POLLHUP on this IPC_FMT node in its crash states, and in RESET only for nodes with `IO_ATTR_STATE_RESET_NOTI`. CPIF nodes persist across a CP reboot (unlike WWAN nodes), so a hangup just reopens the same node. A rejected write (`-EAGAIN` before INIT_END, `-EPERM` before ONLINE) still ends the session, as upstream does |
| B1–B7 | — | unchanged | Same numbers on both backends (`0x6f19`, `0x40046f22`, `0x6f23`, `0x6f27`, `0x401c6f40`, `0x6f57`; 28-byte `cp_image`), also asserted |

`sit-proxy` needs no read adapter. Device reads are already gated on POLLIN,
POLLHUP ends the session, and its length-based SIT framing accepts CPIF's
reassembled FMT messages. Its 2036-byte TX limit fits CPIF's single-frame
path (`n + 12 ≤ 2048`). Byte equivalence with `wwan0sit*` is still runtime
evidence (U3, S3).

### Transport review: std_dl, RFS, `/tmp`

These were checked before choosing the patch shape. None of them needed a
code change.

- **std_dl download (`umts_boot0`)**: unchanged. Each 4-byte ack read is gated
  by `poll`. `bootdump_poll` returns POLLIN only in BOOTING or ONLINE with
  queued data, and POLLHUP on the boot channel in crash states. Like
  `ipc_read`, `bootdump_read` returns 0 after an empty 100 ms wait, but the
  daemon only reaches it after a POLLHUP. `read_exact` then fails with
  `UnexpectedEof`, the boot attempt fails, and the retry resets Cold. That
  reset is now a valid NORMAL power reset on CPIF. `bootdump_read` puts the
  unread part of an skb back on the queue, so an ack longer than 4 bytes
  would desync, and the next comparison fails (fail-closed). `bootdump_write`
  sends one skb per `write()` (up to `12 + 0xC000` bytes) unless the DT sets
  `iod,max_tx_size` for `umts_boot0`. E0 row B5 records a single skb with no
  `ul_buffer_size` split. A split would show up at runtime as an ack timeout, not as silent
  corruption. The std_dl opcodes stay opaque firmware protocol (E0 B5).
- **RFS (`umts_rfs0`)**: unchanged on both backends. Reads are gated by
  `poll(5 s)`, so the 100 ms empty read happens only after a wakeup without
  POLLIN. That is POLLHUP in `STATE_RESET` for nodes with
  `IO_ATTR_STATE_RESET_NOTI`, where upstream's close-and-reopen (fresh fid
  table) is the intended response. `umts_rfs0` is IPC_RAW, so CP frames over
  2036 bytes would arrive fragmented. That is runtime (E0 S1).
- **`/tmp` and state**: the daemon writes the NV sinks
  `/tmp/rfs-nv_{normal,protected,user}.bin`. Only on the WWAN backend does it
  also write `/tmp/modem_crash_<ts>.json`. It opens source partitions
  read-only only. Upstream comments say `/tmp` is tmpfs, but no checked-in
  Pixel input mounts tmpfs on `/tmp`. On the dev image it is whatever the
  rootfs provides, which is unverified and must be checked before S2 (route
  decision, stage 0). `/tmp` is not a calibration partition either way.
  `exynos-modem.initd` creates `/var/lib/exynos-modem`, and the source
  manifest names it `state_directory`, but the daemon never uses it.
- **Finding (not fixed here):** the root daemon opens those fixed `/tmp` names
  with create and truncate, following symlinks. Any local user who can write
  to `/tmp` could point the truncating write at another file. This applies to
  both backends and is outside R1/R2/O1, so this change leaves it alone.
  Before S2, a follow-up patch should move the sinks to a root-owned private
  directory (for example `/run/exynos-modem`, mode 0700) opened with
  `O_NOFOLLOW`.
  **Update (issue #81):** patch 0003 does this in code for the NV sinks and
  the crash record; see [`s5300-scratch-dir.md`](#s5300-daemon-scratch-directory-issue-81). The
  device's runtime backing and permissions for `/run/exynos-modem` are still
  unverified.

### Still required before a CP boot trial

- An integration change that adds `--kernel-backend google-kleaf-cpif` to
  `exynos-modem` and `sit-proxy` explicitly, under a `google-kleaf-cpif`
  backend value, with manual start for S2 (route decision, stage 2). The
  current init scripts must keep refusing that backend until then.
  **Update (issue #88):** done in code
  ([`s5300-cpif-integration.md`](#s5300-google-kleaf-cpif-service-integration-issue-88)); the
  on-device package checks listed there are still required.
- ModemManager plugin binding for the CPIF ports, or binding through the
  `sit-proxy` sockets (S3).
- The prerequisites already listed in the route decision: E1 inventory, the
  NV file size check (≥ `0x80000`), the S0 gate, and U4 skew.
- The `/tmp` sink finding above: fixed in code by patch 0003
  ([`s5300-scratch-dir.md`](#s5300-daemon-scratch-directory-issue-81)); the read-only device
  proof listed there is still required.

### Verification

`just pixel-9a-s5300-cpif-adapter-test`
(`tools/pixel-9a/tests/exynos-modem-cpif-adapter.sh`) always runs the static
checks: the ordered patch list, hash pins in both files, patch scope, and
that the init scripts and backend environment still refuse CPIF and install
no service. With the pinned archive cached under
`out/pixel-9a/s5300-cpif-adapter/dl`, or with
`FERRIX_S5300_ADAPTER_FETCH=1`, it goes further:

1. Verifies the archive's SHA-256 and SHA-512 before extracting it.
2. Applies the patches with `patch -p1 -N` and checks for no rejects and an
   unchanged `Cargo.lock`.
3. Runs `cargo fetch --locked`, then the recipe's test set (`exynos-modem`,
   `sit-proxy`, `samsung-sit-ffi`) offline.
4. Drives both binaries through `--print-config` and the refusal cases.

Linux runs this natively. Other hosts use the pinned `rust@sha256:7cc1c22d…`
(Alpine) image with `--network none`, because `sit-proxy` is Linux-only. CI
runs the full mode in the `rust` job.

Run on 2026-10-12 (macOS arm64 host, Alpine aarch64 musl container): all
static checks passed; the patches applied cleanly; 31 `exynos-modem`, 25
`sit-proxy`, and 9 `samsung-sit-ffi` unit tests passed; all 14 CLI checks
passed. Clippy (`-D warnings`, all targets, checked for
`aarch64-unknown-linux-musl`) was clean on both patched crates. A release
build for `aarch64-unknown-linux-musl` in the container produced static
aarch64 ELF binaries. **Not run:** the recipe's exact
`aarch64-alpine-linux-musl` target, abuild/pmbootstrap packaging, and
anything on the device.

### Regenerating the patches

Extract the pinned archive into a scratch git repository under `out/`,
commit it, apply the two patches as commits (`git am`), and edit. Then
export with `git format-patch --no-signature --zero-commit` and update both
the recipe's `sha512sums` and the manifest's `sha256`. The test fails if any
of these disagree.


## S5300 daemon scratch directory (issue #81)

> **Moved here on 2026-10-13 (#119)** verbatim from `s5300-scratch-dir.md`, with every
> heading one level deeper. The old path is a stub that maps each old
> section anchor to its new one.

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.

This closes the `/tmp` finding from
[`s5300-cpif-adapter.md`](#transport-review-std_dl-rfs-tmp)
in code. The patch is offline-tested only; a separate lead-run read-only
inspection of the still-booted `674157b` Ferrix image observed `/tmp` and
`/run` as tmpfs, `/tmp` mode 1777, and `/` and `/run` as root-owned 0755
directories. No modem node or CP was touched. Capabilities do not change.
QEMU evidence does not apply to this board. **The new `/run/exynos-modem`
directory has not yet been packaged or observed on the device** and needs
the post-package proof below before S2.

### The problem

The `exynos-modem` daemon runs as root. Upstream (`634e8eb4`) it creates and
truncates the fixed names `/tmp/rfs-nv_{normal,protected,user}.bin` (and
`rfs-nv-x.bin` for any other fid) when the CP writes NV over RFS. On the WWAN
backend it also writes `/tmp/modem_crash_<unix seconds>.json` after a
supervised crash. Both opens follow symlinks. Anyone who can write to `/tmp`
could plant a symlink under one of these predictable names, and the next CP
write would truncate and overwrite its target. That could be any file the
root daemon can write. `/var/lib/exynos-modem`, which `exynos-modem.initd`
created and the source manifest named `state_directory`, was never used by
the daemon.

### The change

`pmos/exynos-modem/0003-exynos-modem-private-scratch-directory.patch`
(pkgrel 3) is the third target-local patch on the pinned archive. It applies
after 0001/0002 and is SHA-512 pinned in the recipe and SHA-256 pinned in
`device-google-tegu/s5300-userspace-sources.toml`. It touches only
`crates/exynos-modem/src/{scratch,rfs,main,boot/s5300}.rs`. It adds no crate
and changes no `Cargo.lock`, and it uses only `libc`, which the crate already
depends on.

Both scratch outputs now go to one directory, `/run/exynos-modem`:

| Output | Before | After |
| --- | --- | --- |
| RFS NV write sinks | `/tmp/rfs-nv_*.bin`, create + truncate, follows symlinks | `/run/exynos-modem/rfs-nv_*.bin`, reached and opened as below |
| Crash record (WWAN backend only; CPIF never runs the dump-boot) | `/tmp/modem_crash_<ts>.json`, `fs::write`, follows symlinks | `/run/exynos-modem/modem_crash_<ts>.json`, `O_CREAT\|O_EXCL\|O_NOFOLLOW`, mode 0600 |

The new `scratch.rs` resolves the directory and opens files like this:

1. Every path component is opened with `openat(O_DIRECTORY|O_NOFOLLOW)`,
   starting from the previous directory's fd. A symlink anywhere on the path
   fails the open, and no component can be swapped between check and use.
2. Each ancestor (`/`, `/run`) must be owned by root or the daemon's user and
   must not be group- or other-writable, so nobody else can rename entries in
   it.
3. `/run/exynos-modem` must be owned by the daemon's user (root) with no
   group or other permission bits. If it is missing, it is created 0700 with
   `mkdirat`. If it exists with another owner or mode, it is refused and left
   as it is. The daemon never runs `chmod` or `chown`.
4. NV sinks are opened with
   `openat(O_WRONLY|O_CREAT|O_NOFOLLOW|O_NONBLOCK|O_NOCTTY, 0600)` relative to
   the directory fd. They are truncated only after `fstat` shows a regular
   file owned by the daemon's user, with one link and no group or other bits.
   A symlink (including a dangling one), a hard link to another file, a FIFO
   (no blocking), a directory, a device node, a group-readable file, or
   another user's file is refused without being truncated or written. A
   private file left by the daemon is reused and truncated, which keeps
   upstream's semantics.
5. File names must be plain names. Anything with `/`, `.` or `..` is refused.

A refusal is logged (`rfs: NV sink refused, discarding write: …`). Nothing
falls back to another path. The RFS exchange still completes, so the CP gets
its WRITE requests and the final `OP_STATUS` and does not stall. The data is
dropped, as upstream did when its `/tmp` open failed. A refused crash record
is an error from `save_crash_record`, which the boot loop already logs.
Recovery still runs.

The rest is unchanged: NV reads still use `File::open` (read-only) on the
`/mnt/vendor/efs` factory files, carrier-config reads are still whitelisted
and read-only, and the boot, OEM and SIT behaviour and the CLI paths of both
backends are the same. `--print-config` now also prints
`scratch_dir=/run/exynos-modem`.

### Packaging and service

- `exynos-modem.initd` `start_pre` now runs
  `checkpath -d -m 0700 -o root:root /run/exynos-modem` in place of
  `checkpath -d -m 0750 /var/lib/exynos-modem`. This is provisioning only.
  The daemon re-checks the directory itself and does not rely on how
  `checkpath` handles symlinks. `/run` is root-owned and not world-writable
  on OpenRC, so no unprivileged user can create `/run/exynos-modem` before
  the service does. The parent `/run` was observed root-owned 0755 on the
  current device image; the new service provisioning has not run there.
- `s5300-userspace-sources.toml` replaces the unused `state_directory` with
  `scratch_directory = "/run/exynos-modem"` and pins the 0003 SHA-256.
- `device-google-tegu` pkgrel 1 re-pins `exynos-modem.initd` and
  `s5300-userspace-sources.toml`. The second pin was already stale on `main`
  after the adapter change.
- The init scripts still refuse every backend but `linux-google-tegu-wwan`
  and do not select `google-kleaf-cpif`. No modem service is in a runlevel.
  No integration or CP boot change is part of this stage.
  **Update (issue #88):** the CPIF integration
  ([`s5300-cpif-integration.md`](#s5300-google-kleaf-cpif-service-integration-issue-88)) adds a separate
  `rc-service exynos-modem provision` that runs the same `checkpath` line,
  only after checking that `/run` is a root-owned tmpfs, and the CPIF
  preflight requires the directory to be root:root 0700.

**Cleanup:** nothing deletes files. The NV sinks are a fixed set of four
names, each truncated on reuse. Crash records are one file per supervised
WWAN crash (at most 64 KiB each) and are never overwritten. The directory is
expected to be volatile on `/run`; the current physical image reports `/run`
as tmpfs. Verify that the image containing this patch retains that mount.
A removal (`rm` inside `/run/exynos-modem` as root) cannot follow a
symlink out of a root-owned 0700 directory. No automatic cleanup was added,
because a recursive delete would be another root path to review.

### Verification

`just pixel-9a-s5300-cpif-adapter-test`
(`tools/pixel-9a/tests/exynos-modem-cpif-adapter.sh`) now covers the three
patches. The static checks cover the ordered patch list, pins in both files,
patch scope, the manifest `scratch_directory`, the exact `checkpath` line,
no `/var/lib/exynos-modem` or `/tmp` in the service or manifest, and no
`/tmp` literal added by 0003. On the patched tree it also checks that no
non-test daemon code outside `scratch.rs` can create, truncate, or append to
a file, and that no `/tmp` path remains. It then runs the cargo tests and CLI
contract (with `scratch_dir`) as before.

The unit tests build a scratch directory below a temporary anchor
(`<tmp>/run/exynos-modem`) and assert:

- the 0700 directory and 0600 file are created, and nothing else appears
  below the anchor;
- a private file is reused and truncated;
- a symlinked sink (to a stand-in calibration file, or dangling) is refused,
  and the target is unchanged or not created;
- a hard link to an outside file is refused and the file is not truncated;
- a FIFO, a directory, and a 0644 file are refused, and the FIFO does not
  block;
- a symlinked private directory or ancestor is refused, and nothing is
  created at the target;
- a group- or other-writable ancestor, and an existing 0755 private directory,
  are refused, and the directory is not repaired;
- a file, the private directory, or an ancestor owned by another user is
  refused (root only, so it runs in the pinned container and is skipped
  elsewhere);
- the crash-record open refuses any existing name;
- names with `/`, `.` or `..` are refused, and so is a scratch path that is
  empty or not relative;
- end to end through `Rfs::handle` on a socketpair, an NV write lands only in
  the private directory. With a planted symlink to a stand-in calibration
  file, that file is unchanged, and the CP side still receives WRITE then
  `OP_STATUS`.

Run on 2026-10-12 (macOS arm64 host, pinned Alpine aarch64 musl container,
`--network none`): all static checks passed; the three patches applied
cleanly; 46 `exynos-modem`, 25 `sit-proxy`, and 9 `samsung-sit-ffi` unit
tests passed; all 14 CLI checks passed. The `exynos-modem` tests also passed
as uid 65534 in the same container (the root-only test was skipped). Each
check in `scratch.rs` was removed in turn (`O_NOFOLLOW` on files or
directories, the link-count, ownership/mode, and ancestor checks, and
deferring the truncation), and every removal made at least one test fail.
Clippy (`-D warnings`, all targets, checked for
`aarch64-unknown-linux-musl`) was clean on both patched crates.
`tools/pixel-9a/tests/rootfs-workflow.sh`,
`s5300-mounts-service.sh`, and `s5300-source-inventory.sh` passed.
**Not run:** the recipe's exact `aarch64-alpine-linux-musl` target,
abuild/pmbootstrap packaging, OpenRC's `checkpath` itself, and anything on
the device.

### Still required before S2

The lead's read-only observation on the existing exact-device image found
`/run` and `/tmp` on tmpfs, `/tmp` mode 1777, and `/` and `/run` root-owned
0755 directories. The redacted transcript is ignored under
`out/pixel-9a/s5300-scratch/` and the finding is recorded on
[issue #81](https://github.com/h4rkl/omarchy-ferrix/issues/81#issuecomment-6116798033).
After the package is deployed, read-only proof on the exact device is still
required, with no modem service started and no CPIF node opened:

- reconfirm the filesystem type of `/run` (`/proc/self/mountinfo`) and the
  owner and mode of `/` and `/run` on the new image;
- after OpenRC `start_pre` provisioning (or its dry equivalent),
  `/run/exynos-modem` is a root-owned 0700 directory and not a symlink;
- the OpenRC version on the image, and whether its `checkpath` refuses
  symlinks. This is not a dependency of the daemon's own check, but it should
  be recorded.

The other S2 prerequisites in [`s5300-route-decision.md`](s5300-route-decision.md)
are unchanged.


## S5300 Google Kleaf CPIF service integration (issue #88)

> **Moved here on 2026-10-13 (#119)** verbatim from `s5300-cpif-integration.md`, with every
> heading one level deeper. The old path is a stub that maps each old
> section anchor to its new one.

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.

This is the offline code slice of issue #88. It wires the reviewed
`--kernel-backend google-kleaf-cpif` adapter
([`s5300-cpif-adapter.md`](#s5300-google-kleaf-cpif-userspace-adapter-issue-1)) and the private scratch
directory ([`s5300-scratch-dir.md`](#s5300-daemon-scratch-directory-issue-81)) into the target's
OpenRC services and packages. No phone, USB, ADB, Fastboot, builder, modem
node, or CP was touched, and no image was built. Capabilities do not change.
QEMU evidence does not apply to this board.

**Nothing starts a modem service by itself.** No S5300 service is linked into
any runlevel on either backend, and on `google-kleaf-cpif` the daemon starts
only after a manual preflight and a one-shot `arm`. Nothing here boots the CP;
the S2 manual CP boot needs its own reviewed workflow and decision (route
decision, stage 2).

### Backend selection

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
`umts_boot0` every two seconds. A stopped daemon does not stop the CP: on
the exact device a clean stop left it `ONLINE` until a reboot (see
Limitations).

### Runlevel and dependency policy

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

### The google-kleaf-cpif preflight

The checks live in `pmos/device-google-tegu/ferrix-s5300-cpif.sh`, which is
installed as `/usr/lib/ferrix/s5300-cpif.sh` and sourced by both init scripts.
They never open a CPIF node. `/dev` is touched only by `-L`/`-c` file tests
and `stat(2)`, and open files only by `stat(2)` through
`/proc/<pid>/task/<tid>/fd` links. Per-unit files are
checked by `stat(2)` only. No firmware or NV byte is read or copied. Package,
identity, node and mount mismatches are refusals. The opener scan was hardened for issue #92
(see "Opener scan" below).

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
6. **Openers**: no task holds any device in the `cpif` sysfs class (the
   twelve nodes and any other class entry), by the opener scan below.
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
Its read-only first part, the S2 preflight
([`s5300-s2-preflight.md`](s5300-s2-preflight.md),
`just pixel-9a-s5300-s2-preflight`), reports GO/NO-GO for the facts before
and after the manual mounts start. That document also reviews the partial-mount
failure path of `ferrix-s5300-mounts` and lists the active-workflow and
teardown requirements that are still open.

### Opener scan (issue #92 hardening)

`cpif_holders` / `cpif_check_unheld` in the library replace the earlier
`readlink` string comparison:

- **Aliases.** A holder is matched by device number, not by path. Each
  `/proc/<pid>/task/<tid>/fd/<n>` magic link is `stat -L`'d (stat(2), never
  open(2)), and its `%F` and `%t:%T` are compared with the `dev` attribute
  of each named class entry. Another node with the same numbers, a deleted
  node, or a path in another mount namespace (for example the Waydroid
  container) therefore counts as a holder.
- **Threads.** Every thread's fd directory is read, not only the leader's.
  This covers a thread with an unshared fd table and a zombie leader.
- **Fail closed.** The scan refuses (status 2) when the shell is not in the
  initial PID namespace (`/proc/self/ns/pid` must be `pid:[4026531836]`), when
  a live task's fd directory cannot be listed, when a live fd cannot be
  `stat`'ed, or when the scan subshell does not reach its completion line.
- **Churn.** A task or fd that has gone when it is re-checked is treated as
  exited or closed and skipped. That is a bounded race and is not a
  refusal.

Issue #107 adds, in `cpif_opener_scan` (which `cpif_holders` now wraps, so
`arm` and `start_pre` get the same checks). These are **offline only** until
a rebuilt `device-google-tegu` 1-r5 image runs on the exact phone:

- **Positive control.** The scanning shell holds the mem-class null device
  open read-only on fd 7 for the whole scan. It is the only device the
  library opens, and only after `stat(2)` shows a character device (not a
  symlink) with the number in `/sys/class/mem/null/dev`, under no CPIF major.
  The same proc walk, `stat -L` format, and awk matcher must report it as
  held by the shell's own pid (`/proc/self/stat`, read by the same process).
  Otherwise the scan is `incomplete`, never `clean`. The descriptor is scoped
  to the scan subshell, so it is closed on every path.
- **Format control.** `1:3` reads the same in decimal and hex, so every
  `/dev` character device named like a `cpif` class entry must also print
  that entry's sysfs number through the same format and matcher.
- **Task directories.** A pid that still exists but whose `task` directory
  is missing, empty, or unlistable makes the scan `incomplete` (it used to
  drop out). A pid or task that has gone is skipped and counted
  (`opener_vanished`).
- **CPIF majors.** Every character device under a CPIF major in
  `/sys/dev/char` and under `/dev` (a `find` walk; `stat(2)` only) must be
  the `cpif` class entry of that name and number, and every class entry must
  be in `/sys/dev/char`. Any other device is counted (never named) and
  refused. An open fd on such a device is a holder (`+foreign`).

Facts, one line each after the `holder PID NAME` lines:
`fact opener_positive_control=detected|missing`,
`fact opener_format_control=matched|mismatch|missing`,
`fact opener_major_foreign=<count>|unknown`, `fact opener_vanished=<count>`,
and `fact opener_scan=clean|held|incomplete`. The S2 preflight forwards them
([`s5300-s2-preflight.md`](s5300-s2-preflight.md)). Offline coverage in
`s5300-cpif-services.sh`: control detected, missing, under another pid,
own pid unknown, unopenable, symlinked, a CPIF node, under a CPIF major, and
absent from sysfs (none of these opens a node); descriptor closed after the
scan; unlistable, missing, and empty task directories; vanished pids and
tasks; foreign sysfs and `/dev` names, an alias under another name, a class
entry missing from `/sys/dev/char`, and a held foreign device; GNU, BusyBox,
and upper-case `stat` output (clean, holder detected) and decimal and
zero-padded output (`incomplete`, never `clean`); and the same cases under
mawk, gawk, and BusyBox awk when installed.

Remaining limits: the scan is a snapshot, so an open after a task was read
is not seen. A later scan (for example `start_pre` after `arm`) narrows this
window but does not close it. SCM_RIGHTS fds in flight, io_uring fixed files,
mappings whose fd was closed, and kernel-internal references are not visible.
The cost is one `stat` per open descriptor plus a slow path for failures, so
it grows with task and descriptor counts. It has not been timed on the device.

### Files

| File | Change |
| --- | --- |
| `pmos/device-google-tegu/exynos-modem.initd` | per-backend CLI/supervision; per-backend `depend()` (CPIF: `use` the mounts); `preflight`, `provision`, `arm`; CPIF `start_pre`; WWAN `start_pre` body unchanged |
| `pmos/device-google-tegu/sit-proxy.initd` | per-backend CLI/supervision; per-backend `depend()` (CPIF: `use` the mounts and `exynos-modem`); CPIF `preflight` and `start_pre` |
| `pmos/device-google-tegu/ferrix-s5300-cpif.sh` | new check library (`/usr/lib/ferrix/s5300-cpif.sh`) |
| `pmos/device-google-tegu/s5300-cpif-services.sha256` | new installed pin list |
| `pmos/device-google-tegu/APKBUILD` | pkgrel 4 (issue #107 positive control and CPIF-major checks; 3 was the issue #92 opener scan, 2 before that); installs the two new files; S5300 services out of the runlevel list; sha512 re-pins |
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

### Verification

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

### Limitations and blockers

- **ModemManager CPIF binding (S3) is not implemented.** Nothing here can make
  `mmcli -L` list a modem on CPIF. This needs the SIT plugin to accept the
  CPIF class or bind through the `sit-proxy` sockets, plus
  `ID_MM_PHYSDEV_UID` grouping. That has to be reviewed against the pinned
  fork's source; it was not guessed.
- **No log capture.** As before, the services set no `output_log`. An S2
  workflow must collect the daemon's output and `dmesg` itself.
- **Re-arming after a crash or a stop.** If the daemon exits after the CP
  left `OFFLINE`, the next preflight refuses until `modem_state` is
  `OFFLINE` again. That is deliberate. On the exact device (2026-10-12, S2
  run), a clean `rc-service exynos-modem stop` also left the CP `ONLINE`.
  The service does not power down or reset the CP, so any re-arm after
  a CP boot currently needs a reboot.
- The `cp_interface` driver name, `cpif` class, and kernel release are pinned
  to the values E1 observed on the `674157b` image. A rebuilt Kleaf kernel
  with any difference is refused, not accepted.
- U4 driver/firmware skew, U6 AP panic behaviour, FMT byte equivalence (U3),
  firmware acceptance, and `cpsha`/DT identity values remain unknown (route
  decision).

### Next physical checks (lead only, after review and CI)

On the exact booted phone, from an image built through the checked-in
builder and flashed through the recovery-gated workflow, with no modem
service started and no CPIF node opened:

1. The installed `device-google-tegu` is pkgrel 4 (issue #107 library), `modemmanager-samsung-sit`
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
