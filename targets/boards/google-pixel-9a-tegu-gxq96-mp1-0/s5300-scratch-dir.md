# S5300 daemon scratch directory (issue #81)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu` / `GXQ96` / `MP1.0`) only.

This closes the `/tmp` finding from
[`s5300-cpif-adapter.md`](s5300-cpif-adapter.md#transport-review-std_dl-rfs-tmp)
in code. The patch is offline-tested only; a separate lead-run read-only
inspection of the still-booted `674157b` Ferrix image observed `/tmp` and
`/run` as tmpfs, `/tmp` mode 1777, and `/` and `/run` as root-owned 0755
directories. No modem node or CP was touched. Capabilities do not change.
QEMU evidence does not apply to this board. **The new `/run/exynos-modem`
directory has not yet been packaged or observed on the device** and needs
the post-package proof below before S2.

## The problem

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

## The change

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

## Packaging and service

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
  ([`s5300-cpif-integration.md`](s5300-cpif-integration.md)) adds a separate
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

## Verification

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

## Still required before S2

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
