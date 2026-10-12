# S5300 Google Kleaf CPIF userspace adapter (issue #1)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.

This adds the route A userspace adapter that
[`s5300-cpif-e0.md`](s5300-cpif-e0.md) asked for (rows R1, R2, O1 and the P1
CLI paths). It is code and offline tests only. No phone, USB, ADB, Fastboot,
builder, modem node, or CP was touched. Capabilities do not change. QEMU
evidence does not apply to this board.

**Nothing selects the new backend.** `ferrix-backends.env` still pairs the
userspace with `linux-google-tegu-wwan`. `exynos-modem.initd` and
`sit-proxy.initd` still refuse every other backend, and no modem service is
in a default runlevel. Running the CPIF backend on the device needs a separate,
reviewed integration change (see "Still required").

**Update (issue #88):** the integration is now in code, offline-tested only;
see [`s5300-cpif-integration.md`](s5300-cpif-integration.md). The init scripts
map each backend to its own explicit CLI. The Kleaf composer pairs the image
with `google-kleaf-cpif`, and the CPIF daemon starts only manually after a
read-only preflight and a one-shot `arm`. Still no S5300 service is in a
runlevel, and no CP boot has run.

## What changed

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

## Backend selection

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

## Operation mapping on `google-kleaf-cpif`

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

## Transport review: std_dl, RFS, `/tmp`

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
  the crash record; see [`s5300-scratch-dir.md`](s5300-scratch-dir.md). The
  device's runtime backing and permissions for `/run/exynos-modem` are still
  unverified.

## Still required before a CP boot trial

- An integration change that adds `--kernel-backend google-kleaf-cpif` to
  `exynos-modem` and `sit-proxy` explicitly, under a `google-kleaf-cpif`
  backend value, with manual start for S2 (route decision, stage 2). The
  current init scripts must keep refusing that backend until then.
  **Update (issue #88):** done in code
  ([`s5300-cpif-integration.md`](s5300-cpif-integration.md)); the
  on-device package checks listed there are still required.
- ModemManager plugin binding for the CPIF ports, or binding through the
  `sit-proxy` sockets (S3).
- The prerequisites already listed in the route decision: E1 inventory, the
  NV file size check (≥ `0x80000`), the S0 gate, and U4 skew.
- The `/tmp` sink finding above: fixed in code by patch 0003
  ([`s5300-scratch-dir.md`](s5300-scratch-dir.md)); the read-only device
  proof listed there is still required.

## Verification

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

## Regenerating the patches

Extract the pinned archive into a scratch git repository under `out/`,
commit it, apply the two patches as commits (`git am`), and edit. Then
export with `git format-patch --no-signature --zero-commit` and update both
the recipe's `sha512sums` and the manifest's `sha256`. The test fails if any
of these disagree.
