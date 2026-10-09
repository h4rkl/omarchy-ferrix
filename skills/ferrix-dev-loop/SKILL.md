---
name: ferrix-dev-loop
description: Use when starting development work on Omarchy Ferrix - changing product crates, apps, the shell, OpenRC services, the Pixel 9a kernel or image - and you need to build, deploy, see the screen, drive input, and gather evidence on QEMU or the Pixel 9a (google-pixel-9a-tegu-gxq96-mp1-0) without a human. Covers the remote builder flow, hot-deploying binaries, reflashing slot b, framebuffer and host-webcam capture, uinput tap/type injection, parallel subagents, and the GitHub issue backlog. Subordinate to AGENTS.md and skills/mobile-device-provisioning.
---

# Ferrix dev loop

The working loop that took the Pixel 9a from "boots" to "every app usable"
in October 2026, written down so the next agent does not rediscover it.
Read `AGENTS.md` first; this skill grants nothing it withholds. Device
gates and recovery live in `skills/mobile-device-provisioning/SKILL.md`.

Scripts are in `scripts/` (POSIX sh, macOS host). They read machine-local
settings from the ignored `local/dev/dev-loop.env`, with the existing
`out/dev-loop.env` path retained as a fallback. Run `just local-init` for a
template, and see `scripts/env.sh` for the keys. Never commit the builder host,
its paths, serials or MACs.

## 1. Pick work

- Backlog: GitHub issues on `h4rkl/omarchy-ferrix` (`gh issue list`). Each
  one has context with file paths, ordered tasks, acceptance criteria and
  the rules footer. Labels: `pixel-9a`, `qemu`, `security`, `tech-debt`,
  `product-shell`. Read cross-link comments before starting.
- Current truth: `docs/status.md` (newest entry first) and
  `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/capabilities.toml`.
  Older status entries can contradict newer bullets; the newest wins.
- Before changing architecture, the shell or hardware APIs, read
  `plans/baseline.md` and `plans/plan.md`.

## 2. Build where it builds

| What | Where | How |
| --- | --- | --- |
| Rust workspace, tests, clippy | Mac | `cargo …` as in AGENTS.md |
| QEMU guest and checks | Mac | `just qemu-build`, `just qemu-check`, `just qemu-scenario-check` |
| Pixel kernel (Kleaf, ~11 min) | Linux builder | `KERNEL=1 scripts/build-image.sh` |
| Pixel product (aarch64 musl) | Linux builder | `scripts/deploy-product.sh` or `build-image.sh` |
| Pixel image (dev-compose) | Linux builder | `scripts/build-image.sh` |
| Android base rootfs (Waydroid) | Linux builder | `FERRIX_PIXEL9A_ANDROID=1` rootfs build; slow, rarely needed |

- The builder only sees commits: commit, then `scripts/builder-sync.sh`
  (pushes `HEAD` to `$FERRIX_REMOTE_BRANCH` and fast-forwards there). Access
  notes for a builder belong in ignored `local/dev/`.
- Every builder run goes through `tools/pixel-9a/attempt-env.sh` (UTC stamp +
  named capacity TSV) and `tools/pixel-9a/native-builder.sh run kernel|product|dev-compose build`
  with its typed acknowledgement variable. The scripts do this for you.
- `dev-compose` on the Mac fails on pin checks (Fulguris APK and Waydroid
  image paths, no e2fsprogs); run it in the builder.
- `kernel.sh` reverts and reapplies `targets/boards/…/kernel-patches/`
  around source verification; "kernel patch reverted/applied" lines are
  normal. Only rebuild the kernel when patches or the defconfig fragment
  changed; otherwise the last kernel attempt (`/tmp/ferrix-attempt` on the
  builder) is reused.
- Mali: `mali_kbase`/`mali_pixel` load on every boot (eudev modalias
  coldplug; `ferrix.modules_skip` only filters the initramfs lists). The CSF
  firmware is staged from the factory image (`gpu-mali` in the firmware
  allowlist). Product rendering is still software. GPU work is tracked in
  issues #4 and #19; read
  `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/gpu-research.md` first.

## 3. Get code onto the phone

Pick the cheapest path that covers the change:

1. **Binaries only** (`crates/*` changes): `scripts/deploy-product.sh`.
   A few minutes. Builds on the builder, sends a tarball over the
   CDC-ACM shell, checks sha256, swaps `/usr/bin/ferrix-*`, restarts
   networkd, core and product. The session that wrote this killed the
   processes and used `rc-service … zap` + `--nodeps start`; the
   `rc-service` restart sequence in the script is the cleaner equivalent.
   If sockets are left root-owned, `chown root:user` and `chmod 0660`
   `/run/ferrix/*.sock`.
2. **OpenRC scripts, rootfs files, packages, kernel**: full image.
   `d=$(scripts/build-image.sh)` then `scripts/reflash.sh "$d"`. Expect tens of
   minutes, and the Kleaf build adds about 11.

`reflash.sh` reaches Fastboot through `/usr/sbin/reboot-bootloader` when
Ferrix is running (or `just pixel-9a-enter-fastboot` from Android). It
then flashes slot b plus userdata through `just pixel-9a-dev-flash`,
supplying the plan's `confirm_token`, selects b, reboots, and waits for
OpenRC to settle. The possible outcomes are:

- `Ferrix up`
- back in Fastboot: the boot failed
- stock Android: the bootloader fell back to slot a, which is still stock
- a timeout

If the phone cannot get back to Fastboot, **stop** and ask the human to
hold Volume Down + Power. Never call raw `fastboot flash|erase|oem` or
`adb` mutations.

## 4. See the screen

You cannot trust a process log for graphical claims (AGENTS.md). Capture,
then open the image with your file-reading tool and look at it.

- **QEMU**: `just qemu-agent-start`, then `scripts/qemu-shot.sh NAME`.
  Input: `just qemu-tap X Y`, `qemu-swipe`, `qemu-type`, `qemu-key`. Mock
  hardware state: `just qemu-mock …`. Serial log:
  `out/qemu-aarch64/agent/serial.log` (apps print `ui-state` JSON lines,
  which are good for asserting state without pixels).
- **Pixel framebuffer**: once per boot run `scripts/stage-devtest.sh`, then
  `scripts/phone-shot.sh NAME` → 360x808 PNG. It reads the scanned-out DRM
  buffer (`--reversed` because the panel profile has
  `scanout_byte_order = "reversed"`).
- **Host webcam aimed at the phone** (`scripts/webcam.sh NAME [DEV] [CROP]`):
  - Use it when the framebuffer can't show the truth: bootloader and
    Fastboot screens, kernel panics, boots that never reach the shell, the
    panel being off, or colour and byte-order bugs where the buffer looks
    right but the glass doesn't.
  - Colour and byte-order bugs: show a test pattern (SMPTE bars via
    `modetest`) in each candidate format and compare webcam shots. That is
    how the byte-reversed scanout was found.
  - The camera device index comes from
    `ffmpeg -f avfoundation -list_devices true -i ""`. Set the crop once
    for the phone's position.
  - Webcam shots may show the room: keep them in `out/`.

Everything lands in `out/dev-loop/shots/` (ignored). Captures can show
personal data; never commit raw ones.

## 5. Drive the UI

- Pixel: `scripts/phone-input.sh 'tap 540 1665'`, `'swipe 540 2380 540 1200 250'`,
  `'type hello'`, `'key ENTER'`. Coordinates are panel pixels, portrait
  1080x2424. The injector is a uinput touchscreen and keyboard that
  ferrix-shell picks up through libinput. Restart it after a reboot
  (`stage-devtest.sh`).
- Find coordinates by capturing at scale 3 and multiplying by 3.
- Shell access: `tools/pixel-9a/devtest/shell-run.py 'CMD' [WAIT]`, and
  `shell-put.py` / `shell-get.py` for files (sha256-checked). It is a
  root shell on `/dev/cu.usbmodemferrix_tegu3`; there is no ADB in Ferrix.
  Give long commands a larger WAIT. Only one process may use the port at a
  time.
- `scripts/boot-health.sh` gives a quick summary of services, sockets,
  radios and kernel errors.

## 6. Evidence and handoff

- Verify on the device by looking at screenshots and checking `ui-state`
  and service state, not by trusting that "it started".
- Record it in `docs/status.md` with the source labelled: `QEMU`, `mock`,
  or `physical:google-pixel-9a-tegu-gxq96-mp1-0`. Note the limitations.
  Only update `capabilities.toml` when you have evidence.
- Redact serials, MACs, IMEIs and SSID passwords in anything committed.
  Test Wi-Fi credentials come from the owner in chat and never go into Git.
- Run the AGENTS.md checks before handoff (`cargo fmt --check`, clippy with
  `-D warnings`, `cargo test --workspace`, `just qemu-check`). Mirror shared
  service changes to `targets/qemu-aarch64` and keep the Pixel and QEMU
  OpenRC helpers in sync (issue #16).

## 7. Parallel work with subagents

- Give code tasks to subagents in git worktrees (`isolation: worktree`).
  Each worktree has its own `out/`, and the QEMU sockets live in a short
  directory derived from the out dir (`targets/qemu-aarch64/scripts/paths.sh`),
  so deep `.claude/worktrees/…` paths work without `FERRIX_OUT_DIR`. Two
  QEMU sessions must not share an out dir.
- **Only the lead agent touches the phone and the builder's shared paths.**
  The serial port and Fastboot are single-user; collisions corrupt
  transfers or leave flashes half done.
- Merge worktree branches into `main` one at a time, re-run the checks,
  then deploy once.

## 8. Lessons that cost time

- The bring-up shell echoes input; `shell-run.py` brackets output with a
  marker. Multi-line commands work but are noisier. Prefer `;`-joined lines.
- Large `shell-put` transfers go through base64 over ACM and are slow. Tar and gzip first, and always compare sha256 on both ends.
- `rsync` from the builder sometimes drops; `build-image.sh` retries with
  `--partial` and verifies every manifest hash before anything is flashed.
- A service showing "crashed" in `rc-status` may be a one-shot that
  start-stop-daemon tracked as a daemon. Start one-shots detached, with a
  pidfile only for stop() (see `ferrix-android.initd`, commit 8404f57).
- zbus: never block on a call on the same connection that feeds a signal
  stream. That deadlock froze networkd at "associating".
- The sandbox's parent-death signal is tied to the spawning *thread*;
  spawn apps from a long-lived thread.
- `/run/user/1000` must exist before the session runtime dir is created
  (`ferrix_user_runtime_dir` in `/usr/lib/ferrix/openrc.sh`).
- bcm4383 Wi-Fi needed a kernel patch for WPA3-SAE (`0002-…wpa-version-3…`);
  check `kernel-patches/` before blaming userspace for driver-level
  failures, and read `docs/pixel-9a-bt-wpa3-research.md`.
- Bluetooth is a UART controller (`btattach -P bcm`, `/dev/ttySAC18`, 115200)
  and needs about 90 s after boot before scans return anything.
- Fulguris's first-run terms are for the owner to accept; don't click
  through them.
