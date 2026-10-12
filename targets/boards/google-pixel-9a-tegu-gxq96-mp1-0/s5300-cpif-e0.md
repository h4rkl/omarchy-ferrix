# S5300 E0: device-free CPIF ABI comparison (issue #1)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.

This is the E0 gate from [`s5300-route-decision.md`](s5300-route-decision.md).
It is source and offline-binary evidence only. No phone, USB, ADB, Fastboot,
builder, or CP was touched, and no capability changes. QEMU evidence does not
apply to this board.

## Decision

**E0 result: conditional GO for route A at the ABI level. It does not by
itself justify a CP boot trial.**

- Every ioctl and `read`/`write` that `exynos-modem` uses on the cold-boot
  path has a GPL-defined argument layout in Google CPIF `a8976934`. The
  encodings match exactly. The factory CP3A `cpif.ko` uses the same numeric
  encodings (checked by disassembly).
- What the GPL source does not define is the std_dl download protocol, the
  meaning of the handover fields, and the modem.bin TOC format. That is
  firmware behaviour, reverse-engineered from the proprietary `cbd`. It is
  identical on route A and route B, because both kernels only carry those
  bytes. So it is not grounds to switch to B.
- The unmodified `exynos-modem` must **not** be run against Google CPIF.
  Three bounded userspace adapters are needed first (rows R1, R2, O1 below).
  Without them, recovery and crash-dump paths fail with `-EINVAL`. The GEMS
  channel would also close and reopen every 100 ms, and Google CPIF drops CP
  frames for a node while it is closed (`rx_demux`). The external notes say
  lost GEMS requests crash the CP.
- A CP boot trial still needs things outside E0: the adapter patch and its
  review, E1, the S0 owner review (#34), and the runtime unknowns below
  (U4 skew, NV file sizes).

**Smallest next read-only step:** run E1 as written. In the same read-only
pass, add the cpif probe `dmesg` lines and record whether `nv_normal.bin` and
`nv_protected.bin` are at least the TOC size (0x80000). Record presence and
size only, never content, and only through the reviewed read-only loop mount
once S0 allows it. After that, the adapter patch (R1, R2, O1 plus CLI paths)
is the only code change route A needs before S2.

## Pinned inputs (all hash- or revision-verified, under ignored `out/`)

| Input | Pin | Verification |
| --- | --- | --- |
| Google CPIF `radio/samsung/s5300` | `a8976934fe7f396627f9900958eb1457a74ebcca` (head of `android-gs-tegu-6.1-android16`, committed 2025-06-07) | git object ID |
| Board DTS `devices/google/zuma` | `42395e8adcb880aa336f0da6bb2a19ed53929c40`; tegu include chain `zumapro-tegu-common.dtsi` → `zumapro-board-cp-s5300-sit.dtsi` → `zuma-cp-s5300-sit.dtsi` (tegu `6276884d`, zumapro `8831630e`) | git object IDs |
| Channel constants `soc/gs` | `204078758737776959ecc85c527a00fd2cabb62f` | git object ID |
| `exynos-modem` | `634e8eb404c3fcf959dce6a0b0d97b33ba0fc346` archive | SHA-256 `324efde8…a9445f` matches `s5300-userspace-sources.toml` |
| Radio-first `linux-google-tegu` | `88335b765d920cc345cdffb9c1a6d77e3a531ff9` archive | SHA-256 `efb166b1…3705af` and 266688197 bytes match `kernel-radio-route.toml` |
| Factory `CP3A.260905.009` | `tegu-cp3a.260905.009-factory-0c2f9053.zip` | SHA-256 `0c2f9053…68d1c0d7` and 4227813715 bytes match `recovery.pins` |

Short references used below:
G = [Google CPIF `a8976934`](https://android.googlesource.com/kernel/google-modules/radio/samsung/s5300/+/a8976934fe7f396627f9900958eb1457a74ebcca/),
D = [`zuma-cp-s5300-sit.dtsi` @ `42395e8a`](https://android.googlesource.com/kernel/devices/google/zuma/+/42395e8adcb880aa336f0da6bb2a19ed53929c40/dts/zuma-cp-s5300-sit.dtsi),
X = [`exynos-modem` `634e8eb4`](https://git.deusch.me/pixel-mainline/exynos-modem/src/commit/634e8eb404c3fcf959dce6a0b0d97b33ba0fc346/crates/) (paths below `crates/`),
R = [`s5300_modem.c` @ `88335b76`](https://github.com/SteffenDE/linux-google-tegu/blob/88335b765d920cc345cdffb9c1a6d77e3a531ff9/drivers/net/wwan/s5300_modem.c),
F = factory `vendor_kernel_boot.img` → `lib/modules/cpif.ko`
(SHA-256 `cac472fcf8ee57ac67da1b44f2ccaf55d078dea6a99cb757372615c64a4c0aa5`).

Evidence levels: **source** = handler read in pinned source; **binary** =
disassembly or strings of the hash-verified factory module; **fw-meta** =
parsed from the factory `modem.bin` TOC; **external** = claim in external
code comments, not verified; **runtime** = cannot be settled offline.

## Operation map

`exynos-modem` passes every boot-node ioctl on `/dev/umts_boot0`. Google
dispatches them in `bootdump_ioctl` (G `bootdump_io_device.c:138`). Anything
unmatched falls through to `shmem_ioctl` (G `link_device.c:2735`), which
returns `-EINVAL` for unknown commands.

| # | Operation | `exynos-modem` | Google CPIF (route A) | Radio-first (route B) | Evidence | Decision |
| --- | --- | --- | --- | --- | --- | --- |
| B1 | `IOCTL_HANDOVER_BLOCK_INFO` `_IO('o',0x57)`, pointer to 161 B | X `exynos-modem/src/boot/common.rs:41,227`; layout `boot/s5300.rs:141-162` | `update_handover_block_info` copies `sizeof(t_handover_block_info)` = 161 (packed, G `modem_prj.h:180`) to cmsg `0x82c` (G `link_device.c:1878-1898`; D L102 `DRAM_V1 2092`) | `s5300_handover_block` → ipc+`0x82c` (R L180, L2591) | source; binary (`0x6f57` in the F jump-table range) | **GO as-is.** Offsets 0/4/8/12/16/20/24/28-36/40/44/48-60/64/80/96 match the Google struct exactly. Field *values* (chosen/plat mapping, `cpsha`) are firmware-defined and RE-derived on both routes: **opaque, route-neutral** |
| B2 | `IOCTL_POWER_ON` `_IO('o',0x19)` | X `boot/common.rs:32,236` | `power_on_cp`: STATE_RESET→OFFLINE, clears ap2cp/cp2ap msg words, **full GPIO power off/on** (G `modem_ctrl_s5100.c:992-1037`, L1030) | publishes control words only; reset is a separate ioctl (R L2378) | source; binary | **GO as-is.** Stronger than R (it cold-cycles the CP), which is what `cbd` expects. It does not clear the handover area, so B1-before-B2 ordering is safe |
| B3 | `IOCTL_LOAD_CP_IMAGE` `_IOW('o',0x40, 28 B)` | `CpImage{u64 binary,u32 size,m_offset,b_offset,mode,len}` packed, `m_offset = TOC BOOT.load`, `mode = 0` (X `boot/common.rs:39,47-55,242-257`) | `struct cp_image` same 28 B packed (G `modem_prj.h:82-90`). PCIe path: `boot_img_offset = round_up(0x3000, 64K) = 0x10000`, copies `len` to `base+0x10000+m_offset` (G `link_device.c:2042-2116`) | same layout; dst `0x10000+m_offset`, warns if `m_offset≠0` (R L511-518, L2408-2452) | source; binary (`0x401c6f40` compared in F `bootdump_ioctl`); fw-meta | **GO as-is.** CP3A TOC: `BOOT off=0x410 load=0x0 size=0x16800`, so `m_offset=0` lands where `set_cp_rom_boot_img` points the ROM (G `modem_ctrl_s5100.c:1274-1305`). Size is within `0x20000` (X `boot/s5300.rs:294`) |
| B4 | `IOCTL_START_CP_BOOTLOADER` `_IOW('o',0x22, 4 B)` | `&i32 mode` (0 normal, 1 dump) (X `boot/common.rs:34,262-272`) | `struct boot_mode{enum cp_boot_mode}` 4 B (G `modem_prj.h:61-75`). NORMAL → `start_normal_boot`: ctrl msgs, `link_start_normal_boot` (BOOT_ALIGNED, DPRAM magic), doorbell, `boot_stage` DONE poll (200×20 ms), PCIe bounce, CP2AP_WAKEUP poll (G `bootdump_io_device.c:250-285`, `modem_ctrl_s5100.c:1335-1412`) | does the same steps in-driver plus LINK_ACK doorbell (R L2454-2589) | source; binary (`0x40046f22`) | **GO as-is.** Same number, same arg, and the handshake is driven by the kernel |
| B5 | std_dl stream: `write()` one frame, `poll`+`read()` 4 B ack | X `boot/common.rs:279-375`; stages 1..7 + `0xa400` (`boot/s5300.rs:333-356`) | `bootdump_write`: one skb, no `ul_buffer_size` split, SIT header `{0xABCD, seq, 0xC000, 12+n, ch 241, ch_seq}`, 8-byte pad (G `bootdump_io_device.c:454-607`, `modem_io_device.c:554-598`), sent to the legacy RAW queue (G `link_device.c:1425-1426`). RX: pad trimmed, 12 B header pulled (G `modem_io_device.c:511-522,360-370`). `poll` is POLLIN in BOOTING (G `bootdump_io_device.c:87-109`) | same header bytes (R L343-350, L2649-2718); blocking read (R L2720) | source | **GO as-is** for the transport (frame bytes match R; bytes 10-11 and pad are uninitialised in G but zero in R; the CP is assumed to ignore them, runtime). Std_dl opcodes and CRC frame are **opaque firmware protocol**, not defined in G or R |
| B6 | `IOCTL_COMPLETE_NORMAL_BOOTUP` `_IO('o',0x23)` | X `boot/common.rs:35,274` | waits `init_cmpl` 15 s (G `modem_ctrl.h:10`, `modem_ctrl_s5100.c:1414-1481`). PHONE_START handler sends INIT_END without userspace for SIT (`rild_ready` → true, G `link_device.c:276-285,526-635`) | 15 s wait (R L491, L2603) | source; binary | **GO as-is** |
| B7 | `IOCTL_GET_CP_STATUS` `_IO('o',0x27)`, return value | expects 0 OFFLINE, 4 ONLINE (X `boot/common.rs:124-131`) | returns `enum modem_state` (OFFLINE=0, BOOTING=3, ONLINE=4; RESET maps to OFFLINE) (G `modem_prj.h:222`, `bootdump_io_device.c:296-318`) | same values (R L494-496) | source; binary | **GO as-is** |
| R1 | CP reset (`ResetMode` Warm 0 / Dump 1 / Cold 2) | `_IO('o',0x21)` = `0x6f21`, **integer** arg (X `boot/common.rs:33,141-155`). Used on startup when state ≠ OFFLINE, on every failed-boot retry (Cold), and after a crash (Warm) (`boot/s5300.rs:387-418`) | `IOCTL_POWER_RESET` `_IOW('o',0x21, boot_mode)` = `0x40046f21`, **pointer** arg. NORMAL → full off/on, DUMP → wreset + DUMP_NOTI high, SILENT → wreset + DUMP_NOTI low (G `modem_prj.h:74`, `bootdump_io_device.c:167-217`, `modem_ctrl_s5100.c:1099-1181`) | `0x6f21` integer (R L525, L2781-2816) | source; binary (F compares `0x40046f21`) | **Bounded adapter.** `0x6f21` hits `shmem_ioctl` → `-EINVAL`. The first cold boot is not affected (no reset when OFFLINE), but any retry loops forever with no CP action. Map Cold→NORMAL(0), Dump→DUMP(1), Warm→SILENT(3) via a pointer, or drop the reset on CPIF, since B2 already cold-cycles |
| R2 | `IOCTL_GET_SRINFO` crash record | `_IOWR('o',0x45,16)`, `{u64 buf,u32 len,u32 rsvd}` (X `boot/common.rs:40,158-178`) | `_IO('o',0x45)`, `struct shmem_srinfo{u32 size; char buf[]}`: data copied **inline** after `size` (G `modem_prj.h:100`, `link_device.c:2729-2759`) | `_IOWR`, `{u64,u32,u32}` (R L543-551, L1157) | source; binary (F `shmem_ioctl` accepts only `cmd-0x6f45 ≤ 3`) | **Bounded adapter or disable.** Diagnostic only. Disable the dump-boot path (`boot/s5300.rs:268-275,311-315`) for the first trial |
| P1 | Device paths | defaults `/dev/umts_boot0`, `/dev/umts_rfs0`, `/dev/umts_oem1` (X `exynos-modem/src/main.rs:37-40`); sit-proxy `/dev/wwan0sit0/1` (X `sit-proxy/src/main.rs:42`) | DT creates `umts_boot0` (ch 241), `umts_rfs0` (41), `umts_ipc0/1` (245/246), `oem_ipc0..7` (129..136), names via `"%s%d"` (D L334-474; G `modem_main.c:541-543`) | `umts_oem1` = ch `0x82` (R L253, L3297) | source | **GO with CLI args only:** `--oem-gems-dev /dev/oem_ipc1` (ch 130 = R's `umts_oem1`) and `sit-proxy --dev /dev/umts_ipc0 --dev /dev/umts_ipc1`. The init script currently refuses the Kleaf backend (`exynos-modem.initd`), which is route A integration work |
| O1 | OEM/GEMS read loop | blocking `read()` with no `poll`; `Ok(0)` is treated as EOF → close, sleep 1 s, reopen (X `exynos-modem/src/oem.rs:31-62`) | `ipc_read` waits 100 ms on an empty queue and then **returns 0** (G `ipc_io_device.c:428-446`); release purges the rxq (L57-70); `rx_demux` drops frames for an unopened iod (G `modem_io_device.c:482-486`) | blocks until data (R L2720-2743) | source | **Bounded adapter required.** Poll for POLLIN before reading, or treat 0 as "no data" on the CPIF backend. As-is, GEMS requests arriving during the 1 s gap are dropped. The external notes say that leads to PAL_QUEUE_FULL crashes |
| O2 | OEM framing | one `write()` per message ≤ 256×2036 B; the kernel reassembles on RX (X `oem.rs:23-26,67-77`) | IPC_FMT: TX split into ≤2 KiB SIT multi-frames (G `ipc_io_device.c:270-399`); RX reassembles via `gather_multi_frame_sit` for `is_oem_ch` (G `modem_io_device.c:249-315,497-499`) | same idea (R L3297-3304) | source | **GO as-is** (G is the stock reference framing) |
| F1 | FMT/SIT control on `umts_ipc0/1` | sit-proxy reads whole or fragmented frames, accumulates and splits on the 8-byte SIT header length (X `sit-proxy/src/port.rs:47,566-577`; `samsung-sit/src/lib.rs:74,355-363`); TX ≤ 2036 B, retries EBUSY/EAGAIN (`port.rs:51,500-521`) | strips the 12 B SIT link header and reassembles multi-frame before queueing one skb (G `modem_io_device.c:317-358`). TX: single frame when `n+12 ≤ 2048` (G `ipc_io_device.c:327-328`); `-EPERM` before ONLINE, `-EAGAIN` before INIT_END (L264-307). POLLHUP on crash/reset states (L83-124) | strips the header; delivers fragments separately (`port.rs:20-25`) | source | **GO as-is for the transport** with `--dev` paths. sit-proxy's length-based reassembly also accepts G's reassembled messages, and POLLHUP leads to a clean reopen. Byte equivalence with `wwan0sit*` on device is **runtime** (U3). The ModemManager plugin's `wwan`-only subsystem watch is unchanged (S3, not E0) |
| F2 | FMT header the kernel imposes | — | `struct sipc_fmt_hdr` (7 B, G `modem_prj.h:265`) is used only on the **SIPC** multi-frame path (G `modem_io_device.c:220`). With `PROTOCOL_SIT` (D L37) the kernel never parses FMT payloads | — | source | **Finding:** the 7-byte envelope in `crates/ferrix-s5300-cpif` (`fmt.rs`) is not the SIT payload format. SIT userspace uses the 8-byte header defined only by the external `samsung-sit`. This is not changed here; it should be noted before any Ferrix code relies on it |
| S1 | RFS | `CHUNK=2012`, frames ≤ `0x14+2012` B, poll then read, read buffer 4096 (X `exynos-modem/src/rfs.rs:30-31,131-184,411-429`) | `umts_rfs0` is IPC_RAW: TX single frame because `2032+12 ≤ 2048`; RX header stripped with **no** multi-frame reassembly (G `modem_io_device.c:360-370,502-503`). Writes refused before ONLINE (G `ipc_io_device.c:264`) | 4 KiB single frame, also refused before ONLINE (R L3279-3288, L3412-3415) | source | **GO as-is** while the CP's RFS frames stay ≤ 2036 B (the daemon requests ≤ 2012 B chunks). CP-originated RFS frames > 2 KiB would arrive as fragments on G: **runtime** |
| S2 | RFS/daemon writes | NV writes go to `/tmp/rfs-nv_*.bin`; crash JSON to `/tmp`; reads use `File::open` only on `/mnt/vendor/{modem_img,efs,persist,modem_userdata}` (X `rfs.rs:45-61,293-308`; `boot/s5300.rs:27-56,213,257`) | — | — | source | **GO for the recovery invariant.** No write path to calibration partitions. Writes go to `/tmp`, not `/var/lib/exynos-modem` as the route doc assumes. CP NV changes are not persisted (external claim: the CP rebuilds NV each boot) |
| T1 | Firmware TOC and inputs | TOC 32×32 B, sections TOC/MAIN/VSS/APM from modem.bin, NV_NORM/NV_PROT from efs, REPLAY padded from `modem_userdata` (X `boot/s5300.rs:42-56,284-350`) | the kernel never parses modem.bin | same (transport only) | fw-meta | **GO offline for CP3A image metadata:** INFO `g5300t-260604-260807-B-16036236` passes the s5300 gate; MAIN/VSS/APM present; NV/REPLAY TOC size 0x80000. Whether the efs files are ≥ 0x80000 is **runtime** (device data). TOC/CRC semantics are **opaque firmware** |
| K1 | Opening nodes | — | for SIT, `shmem_init_comm` returns before doing any work (G `link_device.c:1964-1978`); open/close only adjust the refcount and purge rx queues | `open` is a no-op (R L2634) | source | **Note:** in this revision, opening is not link bring-up. The never-open rule for read-only probes stays in place (skew, U4) |

## Platform-kernel and firmware skew (U4)

| Item | Finding | Evidence |
| --- | --- | --- |
| Factory module identity | `vermagic=6.1.162-android14-11-g622c1358585b-ab15912884`, `scmversion=gfd5221767c57`, `depends=logbuffer,s5910,google_modemctl,shm_ipc,ect_parser,cmupmucal,exynos-pcie-iommu,pcie-exynos-gs,exynos_pm_qos,bts` | binary (`.modinfo`) |
| Factory source revision | `fd5221767c57` is in none of the 557 published commits of the S5300 repo. Pinned `a8976934` is the published tegu head, about 14 months older. The CP3A driver source is unpublished | binary + public repo history |
| Ioctl ABI | F `bootdump_ioctl` compares `0x40046f21`, `0x40046f22`, `0x401c6f40`, `0x40106f53`, `0xc0286f52`, `0x82046f55`, `0x80146f56`, `0x80046f59`, and jump-tables `_IO` `0x6f19`–`0x6f60`. `shmem_ioctl` handles `0x6f45`–`0x6f48`. Same version string `CPIF-20220408R1` and handler log strings as G | binary |
| Function set | all 376 function symbols in F map to identifiers defined in pinned G source (including macro-generated ones). Function bodies can still differ | binary vs source |
| Board DT | factory MP1.0 overlay (`dtbo.img` entry `id=0x60b06`, "MP on ZUMA PRO") `cpif` node matches the pinned include chain (D plus `zumapro-board-cp-s5300-sit.dtsi`) on every compared value: link_attrs `0x37c8`, legacy queue offsets/sizes, srinfo, capabilities 0, ctrl-msg offsets including handover `0x82c`, and all 17 iodevs (name, channel, format, attrs, count). The only extra property is `offset_boot_indicator = 0x90` | binary DT vs source |
| Boot indicator | not read by G or present in any pinned DTS. F reads it in two `bootdump_ioctl` paths only as a `readl()` of the word at that DT offset (from a driver-held base) plus a log line ("cp boot indicator"). No AP-side write was found | binary (disassembly) |
| Residual | whether G `a8976934`, built for the 6.1.124 dev GKI, interoperates at runtime with CP3A firmware (shared-memory behaviour outside these ioctls, PCIe PM, pktproc) | **runtime** (S2) |

## What E0 cannot conclude offline

1. CP3A firmware acceptance of the std_dl stream and handover produced by
   `exynos-modem` on the G driver. This is runtime, and only physical S2
   can show it.
2. NV file sizes and `cpsha`/chosen-plat presence on this device. This is
   device data, gated by S0/E1.
3. U4 runtime skew beyond the ioctl/DT surface.
4. Whether `umts_ipc0/1` payloads are byte-identical to `wwan0sit0/1` (U3).
   Both kernels strip the same 12-byte header, but equivalence needs S3.
5. Whether the dev cpif build actually loads and binds (E1).

## Reproduction (device-free)

All downloads go to ignored `out/e0-src/` and `out/e0-factory/`. Parsers lived
outside those directories and read the archives with `python3 -I`. The steps:
shallow-fetch each pinned git revision; fetch and `shasum -a 256` both external
archives and the factory zip against their pins; extract `vendor_kernel_boot`
(vendor boot v4, LZ4 cpio), `dtbo.img`, and `modem.img` (a tar whose raw bytes
are also a valid ext4; `images/default` is a symlink to the versioned
directory); read `.modinfo`; disassemble with `objdump -d` (Apple LLVM). No
firmware bytes, identifiers, or extracted images are committed.
