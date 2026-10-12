# S5300 cellular route decision (issue #1)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.

This is a source-grounded engineering decision, not physical evidence. No
modem, SIM, registration, data, SMS, voice, or emergency function is claimed
on any route. `capabilities.toml` keeps `telephony = "unsupported"` and
`docs/status.md` is not changed until exact-target evidence exists. QEMU mock
telephony is not evidence for this board.

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
| FMT userspace envelope (7-byte packed header) | Pinned source, `ferrix-s5300-cpif` round-trip tests | not exercised on device |
| Opening `umts_boot0`/`umts_ipc*` triggers link `init_comm` (active, not read-only) | Pinned source | `ipc_io_device.c` |
| CP boot inputs: TOC `BOOT`/MAIN/VSS/APM from current-slot `modem.bin`, EFS NV, persist `cpsha`, runtime 161-byte handover | External `exynos-modem` source | not exercised |
| Physical S5300 + stock firmware can attach on this board | External Halium port report | not Ferrix evidence; Android RIL path |
| **U1** Does `cpif` load and probe on the dev boot; `modem_state`; which `/dev` nodes and class name exist | Unknown | E1 |
| **U2** Google CPIF boot/load ioctl contract (image struct, offsets, sequence) fully defined by GPL kernel source | Unknown | E0 |
| **U3** FMT payload on `umts_ipc0/1` byte-identical to what `wwan0sit0/1` gives `sit-proxy` | Unknown (same CP firmware; both kernels strip the SIT link header, still unproven) | E0, then S3 |
| **U4** Driver/firmware skew: pinned CPIF `a8976934` (android16, 6.1.124 dev GKI) vs CP3A `modem.bin` shipped with stock 6.1.162 | Unknown | E0 (offline `modinfo` of factory `cpif.ko`), S2 |
| **U5** SIT/GEMS command vocabulary sufficient for SIM/attach/data/SMS/voice | External plugin's claim only | S3–S7 |
| **U6** CP crash handling on Kleaf panics the AP (boot-loop risk) | Unknown | S2 under manual start |
| **U7** `efs`/`persist`/`modem_userdata` filesystem types and whether a plain `ro` mount replays a journal | Unknown | pre-S2 gate |

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
- RFS writes go only to the daemon's rootfs state directory.

Switch to B if CPIF boot needs undefined userspace data or sequencing.

**E1, read-only dev-boot inventory** (lead, existing `radios.sh` CPIF probe
or `discover_at`; nothing opened):
- `/proc/modules` contains `cpif`, `shm_ipc`, and `cpif_page`;
- the `samsung,exynos-cp` platform device is bound;
- `modem_state` is `OFFLINE` (or similar non-ONLINE);
- `/dev/umts_boot0`, `umts_ipc0`, `umts_ipc1`, `umts_rfs0`, and `oem_ipc*`
  are character devices, with their class/subsystem recorded;
- Record whether `rmnet*` netdevs exist before CP boot; their absence at this
  stage alone does not reject A.

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
   unknown filesystem type. Confirm `exynos-modem` writes only under
   `/var/lib/exynos-modem`. A pinned 6.1.124 source review found that these
   options leave orphan-cleanup and error-superblock writes possible on both
   filesystems. Only a read-only device skips them, and `BLKROSET` alone only
   warns about writes rather than failing them. A loop attached with
   `losetup --read-only` fails every write before it reaches the partition
   ([README](README.md#s5300-source-mount-proof-issue-34)). The gate
   therefore also needs `ferrix-s5300-mounts` itself to set `BLKROSET` and
   mount only from a verified read-only loop. The checked-in service now
   does this (offline stub-tested only). The gate also needs the guest to
   run that exact service, and an observed
   `just pixel-9a-s5300-mount-proof` preflight and per-role mount runs
   reviewed by the owner. Until then the workflow reports the gate `blocked`
   or, at best, `owner-review-required`; no result passes it.
1. **E1 inventory** (above).
2. **CP boot.** Start manually, not from the default runlevel, so a
   CP-crash panic cannot boot-loop:
   `ferrix-s5300-mounts`, then `exynos-modem --<cpif backend>`.
   - GO: `modem_state` reaches ONLINE and stays there for at least 10
     minutes, with no CP crash dump or AP panic, and RFS requests are
     served.
   - STOP: repeated crash, handshake timeout, or AP reset. Capture dmesg and
     the boot log, then re-check U4.
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
