# Android compatibility runtime: evaluation and integration record

Status: **Waydroid selected and implemented for the opt-in Android-enabled
postmarketOS QEMU route**, 2026-09-21. This document combines the upstream
evaluation with the resulting pinned implementation and evidence. The normal
postmarketOS image and CI route remain Android-free. Subordinate to
`plans/baseline.md` and `plans/plan.md`.

The current reference APK is Fulguris Web Browser, pinned and verified from
F-Droid. A fresh QEMU run now reaches and has inspected its first-run
onboarding framebuffer; interactive advancement beyond that screen remains
partially unverified. The Brave provisioning and framebuffer run described in
the historical evidence below predates this replacement and must not be
treated as Fulguris evidence. This document therefore records the current
Fulguris input boundary separately from the historical Brave run; neither is a
claim of general APK compatibility, complete interaction, or physical-device
support.

Terminology: the reference application is "Fulguris Web Browser (Android application
running in the Android compatibility runtime)". It is never described as a
native application.

## 1. Scope and boundary

- `RuntimeKind::Android` is implemented by the private adapter crate
  `ferrix-runtime-android` behind `ferrix_app_runtime_api::Runtime`
  (§7). Applications, shells, `ferrix-core`, and `ferrix-app-api` never name
  the runtime; the architecture guard (`just arch-check`) enforces this.
- The runtime is an *application facility*. It does not decide the host
  service architecture, the compositor, or the hardware API
  (`plans/ferrum-mobile-architecture/references/architecture.md`).
- Android Studio/AVD is excluded by the baseline as the Linux-phone emulator
  and is listed here only for completeness.

## 2. Candidates and upstream state (verified 2026-09-21)

| Candidate | Upstream state | Verdict |
| --- | --- | --- |
| **Waydroid** (github.com/waydroid/waydroid) | Latest release `1.6.3`, 2026-05-28; last commit on `main` 2026-09-06; not archived; GPL-3.0 (`LICENSE` is GPL v3; source headers say `GPL-3.0-or-later`, Alpine packages it as `GPL-3.0-only`). Runs a LineageOS 20 (Android 13) userspace in an LXC container using Linux namespaces; Wayland output via its own hwcomposer HAL; CLI for init/session/app/prop/shell. | **Only viable candidate.** Evaluate in QEMU. |
| **Anbox** (github.com/anbox/anbox) | Archived; last commit 2023-02-03; README: "no longer actively developed"; points to Waydroid. GPL-3.0. | Excluded: unmaintained. |
| **redroid** (github.com/remote-android/redroid-doc) | Active (last commit 2026-05-17); arm64 and amd64; Android 8.1–16 images; needs `binderfs` (+ashmem/memfd, ION/DMA-BUF heaps); designed for Docker/podman/k8s `--privileged` containers with `adb connect` + `scrcpy` display; software GPU mode `androidboot.redroid_gpu_mode=guest`; "redroid itself is under Apache License" but no LICENSE file in the doc repo, kernel modules GPL-2.0, images include third-party modules. | Excluded for the product: no Wayland integration (display only through ADB/scrcpy), binderfs mandatory while the pinned pmOS kernel ships static binder nodes, container tooling (Docker) foreign to the postmarketOS guest. Possible future headless test harness only. |
| **Android-x86** (android-x86.org) | Newest release 9.0-r2 (2020-03-25); x86/x86_64 bootable ISOs only. | Excluded: x86-only full OS image, not a runtime. |
| **BlissOS** (blissos.org) | Status page (August 2026): "Public images are paused. Development continues."; x86_64 PCs only; bootable ISO; dual GPLv3/commercial licensing for Bliss-specific parts. | Excluded: x86-only full OS image, not a runtime. |
| **Android emulator/AVD** | Google's emulator (Apache-2.0 sources, SDK binaries under the SDK licence). | Excluded by `plans/baseline.md` §"excluded" as the Linux-phone emulator; nested virtualisation inside the QEMU guest is also not a product path. |

## 3. Criteria matrix (ADR-005) — Waydroid

Each row states what upstream documents or its source does, with the source.
"Unverified" means it could not be confirmed from a primary source today.

| Criterion | Finding | Source |
| --- | --- | --- |
| Upstream maintenance | Release 1.6.3 (2026-05-28); commits through 2026-09-06; 12k stars; images rebuilt 2026-04-03 (lineage-20). A `dev/lineage-23.2` branch exists in `android_vendor_waydroid`, no images published for it. | GitHub API; `ota.waydro.id` |
| AArch64 support | `tools/helpers/arch.py` maps `aarch64 → arm64`, and `arm64_only` when the CPU lacks AArch32 (probe via `personality(PER_LINUX32)`). OTA channels `waydroid_arm64` and `waydroid_arm64_only` exist. | waydroid source; OTA JSON |
| postmarketOS compatibility | No `waydroid` package in pmaports v26.06; it ships from **Alpine aports `community/waydroid`**: `3.24-stable` has `1.6.2` (`depends="dnsmasq gbinder-python iptables kmod lxc py3-dbus py3-gobject3 py3-pyclip python3"`, subpackages `waydroid-openrc`, `waydroid-nftrules`, `waydroid-systemd`, built with `USE_NFTABLES=1`). pmOS wiki: "Waydroid is available in postmarketOS v21.12 and newer"; OpenRC steps `rc-update add cgroups default`, `rc-service cgroups start`, `rc-service waydroid-container start`. | Alpine APKBUILD; pmOS wiki (Wayback snapshot 2026-08-23, live page behind an anti-bot challenge) |
| Binder requirements | MAINLINE vendor type needs `/dev/binder`, `/dev/hwbinder`, `/dev/vndbinder` (static nodes or binderfs; `drivers.py` mounts binderfs and allocates nodes only when the static nodes are missing). Ashmem optional: `sys.use_memfd=true` is set when `/dev/ashmem` is absent. | `tools/helpers/drivers.py`, `lxc.py` |
| Kernel configuration (pinned guest kernel) | `device/main/linux-postmarketos-stable` v26.06 = **7.0.12**, `config-stable.aarch64`: `CONFIG_ANDROID_BINDER_IPC=y`, `CONFIG_ANDROID_BINDER_DEVICES="binder,hwbinder,vndbinder"`, `# CONFIG_ANDROID_BINDERFS is not set`, `CONFIG_PSI=y`, `CONFIG_MEMFD_CREATE=y`, `CONFIG_VETH=m`, `CONFIG_TUN=m`, `CONFIG_BRIDGE=m`, `CONFIG_BLK_DEV_LOOP=y`, `CONFIG_OVERLAY_FS=m`, `CONFIG_SQUASHFS=m`, `CONFIG_EROFS_FS=m`, nftables + iptables NAT/masquerade as modules, all namespaces, cgroups. The kernel carries `pmb:kconfigcheck-community`, whose alias includes `category:waydroid` in pmaports `kconfigcheck.toml` (`ANDROID_BINDERFS = "n"`, `ANDROID_BINDER_DEVICES = ["binder","hwbinder","vndbinder"]`, `ANDROID_BINDER_IPC = "y"`, `BLK_DEV_LOOP`, `BRIDGE`, `VETH`, `TUN`, `SQUASHFS*`, `FUSE_FS`, `IP_NF_MANGLE`, `NETFILTER_XT_MATCH_COMMENT`, …). **No kernel change is required**; `pmos-check` should still prove the three nodes exist. `CONFIG_RT_GROUP_SCHED` must stay disabled (pmOS wiki). `psi=1` on the command line is a documented workaround for "Service manager /dev/binder has died". | pmaports v26.06 raw config and `kconfigcheck.toml`; docs.waydro.id troubleshooting |
| Container / process isolation | One LXC container (user, pid, uts, net, mount, ipc namespaces) for the whole Android userspace; all apps are processes inside it (visible from the host `/proc`). No per-app host process. LXC mounts the binder nodes, `/dev/fuse`, `/dev/net/tun`, DRI render node when present, `/dev/dma_heap/*`, the Wayland socket, and the PulseAudio socket into the container. Every passed device must be recorded before any claim (§8). | README; `lxc.py` |
| Wayland integration | Host `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY` is bind-mounted to `/run/xdg/wayland-0`; the container-side hwcomposer is the Wayland client. `xdg_toplevel` `app_id` is `"waydroid." + <package>` for apps and `"Waydroid"` for the full UI; title is the app name. `persist.waydroid.multi_windows` (default false) selects one surface vs. per-app windows. `waydroid session start` needs `WAYLAND_DISPLAY` and `XDG_RUNTIME_DIR` of the compositor session and refuses `sudo`. | `wayland-hwc.cpp` (lineage-20), `session_manager.py`, docs |
| Software rendering without virgl/host GPU | FAQ "Get Waydroid to work through a VM": add `ro.hardware.gralloc=default` and `ro.hardware.egl=swiftshader` to `[properties]` in `/var/lib/waydroid/waydroid.cfg`, then `waydroid upgrade -o`. The QEMU Android image also sets `ro.sf.lcd_density=400`, a slightly relaxed Pixel-class logical density that makes the 1080-pixel panel approximately 432 dp wide; this setting is target-local and is not part of the shared Ferrix profile. `lxc.py` auto-selects the same renderer pair when no DRI render node exists; with a render node it selects `gralloc=gbm`, `egl=mesa`. Upstream states only QEMU (and possibly crosvm) have working 3D acceleration in a VM and only with virtio-gpu 3D enabled. **Performance caveat:** SwiftShader is a CPU rasteriser; under TCG the guest CPU is itself emulated, so Android boot and Fulguris rendering are expected to be very slow. No upstream frame-rate numbers exist; a future QEMU check (§8) must measure boot-to-ready and time-to-first-frame and record them. Correctness (non-blank framebuffer) is the acceptance bar, not speed. | docs.waydro.id FAQ; `lxc.py` |
| Input and on-screen keyboard | Touch/mouse/keyboard arrive through the Wayland surface (`persist.waydroid.fake_touch` maps mouse to touch per package). Waydroid shows the **Android** virtual keyboard inside its surface by default; no host OSK/text-input integration is documented. Consequence: the product OSK (phase 3) and the Android keyboard would compete; the initial evaluation keeps Android's keyboard inside the Android surface and records this as a departure. | docs FAQ "Disable on-screen keyboard" |
| Audio | Binds the host PulseAudio socket `$PULSE_RUNTIME_PATH/native` (default `$XDG_RUNTIME_DIR/pulse/native`) into the container. PipeWire is not mentioned by upstream; PipeWire's pulse compatibility server is the expected path but is **unverified**. pmOS wiki: the container fails to start when the socket is absent. | `lxc.py`, `config/__init__.py`; pmOS wiki |
| Networking | `waydroid-net.sh` creates bridge `waydroid0` (192.168.240.1/24), runs `dnsmasq` (DHCP/DNS), enables `ip_forward`, and adds NAT masquerade via iptables-legacy/iptables or nftables (`waydroid-nftrules` ships `51_waydroid.nft` on Alpine). This sits **on top of** the guest's virtio-net user-mode networking: Android → `waydroid0` NAT → guest IP stack → QEMU slirp. Double NAT is acceptable for a browser; the container never sees `/run/ferrix` sockets (ADR-004 §5). `waydroid status` prints `IP address: UNKNOWN` when DHCP is blocked (pmOS wiki). | `data/scripts/waydroid-net.sh`; pmOS wiki |
| Lifecycle and crash reporting | `waydroid status` prints `Session:\tRUNNING|STOPPED`, `Container:\t<state>` (RUNNING/FROZEN), `Vendor type:`, `IP address:`, `Session user:`, `Wayland display:` (tab-separated; exact `print` statements in `tools/actions/status.py`). `waydroid app list` prints `Name:`, `packageName:`, `categories:` blocks. `waydroid app launch <pkg>` sets `waydroid.active_apps`, calls `launchApp`, and starts a session if none runs. `waydroid log`, `waydroid logcat`, `waydroid shell -- am force-stop <pkg>` exist. There is no per-app exit event; the adapter polls the host `/proc` for the package's process (§7). | `status.py`, `app_manager.py`, `arguments.py` |
| Data persistence | System: `/var/lib/waydroid/{images,rootfs,overlay,overlay_rw,data,lxc,host-permissions}`; per user: `~/.local/share/waydroid/data` (Android `/data`); desktop entries `~/.local/share/applications/waydroid.<pkg>.desktop` (`Exec=waydroid app launch <pkg>`). Images: `/etc/waydroid-extra/images` or `/usr/share/waydroid-extra/images` for pre-seeded images (`waydroid init -f`). | `config/__init__.py`; docs |
| Permission mediation | Android permissions are mediated by the Android framework inside the container. Waydroid has no host-side permission portal; the container gets exactly the devices LXC passes (above). Cross-runtime integration (notifications, share, clipboard) is out of scope (ADR-004 §5) and would need its own reviewed bridge. | `lxc.py`; ADR-004 |
| APK install/update | `waydroid app install <apk>` copies the file to `~/.local/share/waydroid/data/waydroid_tmp/base.apk` and calls `IPlatform.installApp`; `waydroid app remove <pkg>`. No auto-update; Ferrix re-runs the pinned fetch/verify and installs explicitly. | `app_manager.py` |
| Reproducible builds | System/vendor images are downloaded by `waydroid init` from `https://ota.waydro.id/...json`, whose entries carry `url` (SourceForge), `size`, `datetime`, and `id` = SHA-256 (verified by `images.py`: "Downloaded system image hash doesn't match"). Image sources: `github.com/waydroid/android_vendor_waydroid` (`lineage-20`), built from LineageOS 20 with Waydroid patches (`docs` "Compile Waydroid LineageOS based images"). Ferrix must pin the exact `filename`, `url`, `size`, and `id` (§6) and pre-seed them so `waydroid init` runs offline; the image build itself is not reproduced by this project. | `images.py`; OTA JSON; docs |
| Licensing | Waydroid: GPL-3.0 — used as a separate process via its CLI, never linked; Ferrix code is Apache-2.0. LineageOS images: AOSP/LineageOS code is Apache-2.0 with GPL-2.0 kernel-side pieces; the MAINLINE vendor image is built by Waydroid with Mesa (`TARGET_USE_MESA`), i.e. **no proprietary vendor blobs are documented** for the MAINLINE/VANILLA pair. **Unverified**: a full licence inventory of the shipped image (SwiftShader is Apache-2.0; other components not audited). GAPPS images bundle Google apps and are **not** proposed. Fulguris: MPL-2.0, built and signed by F-Droid, never committed. | `LICENSE`; Alpine APKBUILD; F-Droid package page |
| Security-update path | Waydroid tool: Alpine `community` package updates within the pinned channel (`3.24-stable` currently 1.6.2; `master` 1.6.3). Android userspace: Waydroid rebuilds lineage-20 images periodically (last 2026-04-03; Android 13 is past AOSP's active security-bulletin window, LineageOS 20 receives backports on a best-effort basis — **unverified** for this image). Fulguris: a new F-Droid version requires an explicit edit to `fulguris.pins`. Every update is a deliberate pin bump with manifest hashes, never automatic. | Alpine aports; OTA JSON; F-Droid index |

## 4. Decision

Use **Waydroid** as the Android compatibility runtime for the opt-in QEMU
route, under these conditions:

1. Runtime tool from Alpine `community/waydroid` at the version the pinned
   pmOS channel resolves (1.6.2 on 3.24-stable today); no pmaports fork.
2. Images pinned to the `waydroid_arm64_only` channel (§6.3). Rationale: the
   reference app ships an arm64-only APK; the smaller image (728 MiB + 77 MiB
   vs 905 MiB + 148 MiB) shortens `pmos-build`; and `arch.py` would choose
   `arm64_only` automatically on an Apple Silicon host under HVF (no AArch32)
   but `arm64` under TCG, so pinning removes a host-dependent difference.
   The implemented route intentionally does not provide 32-bit app
   compatibility.
3. Software rendering forced explicitly (`ro.hardware.gralloc=default`,
   `ro.hardware.egl=swiftshader`) rather than relying on auto-detection, so
   the QEMU route never silently depends on a render node. The QEMU Android
   image separately sets `ro.sf.lcd_density=400` for a slightly relaxed
   Pixel-class mobile
   logical viewport; this property is not shared with other targets.
4. Claim levels are reported strictly per `architecture.md` §"APK
   compatibility claims". Current QEMU evidence reaches the Fulguris
   onboarding framebuffer, while interactive advancement remains only
   partially verified. This is not a general Android compatibility claim.

Rejected alternatives: writing a Ferrix-owned container runtime (baseline:
prefer maintained projects; Binder/ART/HAL surface is enormous); redroid
(display path is ADB/scrcpy, not Wayland).

## 5. Remaining unverified areas

- The built manifest, rather than a channel assumption, remains authoritative
  for the resolved Alpine and Waydroid package versions.
- pmOS wiki content comes from a Wayback snapshot (2026-08-23 capture of a
  page last modified 2026-06-21); the live page was behind an anti-bot
  challenge.
- PipeWire pulse-compat as the audio path; upstream documents PulseAudio only.
- Host OSK / `zwp_text_input` integration: nothing documented; hwcomposer
  source not audited for it.
- Licence inventory of the shipped LineageOS/Waydroid image beyond the
  headline Apache-2.0/GPL-2.0 split.
- Interactive touch/keyboard behavior beyond the onboarding surface and
  time-to-first-frame under TCG. The inspected Fulguris graphical run used
  HVF; the bounded Android check is headless and does not establish full app
  interaction.

## 6. Implemented guest inputs

These inputs are checked in for the separate Android-enabled image. The
default `pmos-build`, `pmos-check`, and CI routes do not consume them.

### 6.1 `targets/qemu-aarch64/pmos/pmbootstrap-android.cfg`

```ini
extra_packages = seatd,eudev,libinput,libseat,libdrm,pixman,libxkbcommon,mesa-gbm,libgcc,waydroid,waydroid-openrc,waydroid-nftrules,nftables,lxc,dnsmasq,pulseaudio,dbus
```

Notes: `waydroid` pulls `lxc dnsmasq iptables kmod gbinder-python py3-dbus
py3-gobject3 py3-pyclip python3` itself; `waydroid-openrc` provides
`/etc/init.d/waydroid-container`; `waydroid-nftrules` provides
`/usr/share/nftables.avail/51_waydroid.nft`, which the finalize step links
into `/etc/nftables.d/`. This route installs `pulseaudio` and `dbus`; product
bootstrap creates the session bus and PulseAudio socket before starting the
Waydroid session. PipeWire pulse compatibility remains unverified.

### 6.2 Kernel

No config change. `just pmos-android-check` requires the guest's
`android-binder-devices` record for `/dev/binder`, `/dev/hwbinder`, and
`/dev/vndbinder`; the first-boot hook also records PSI availability. The
checked-in kernel already provides the static Binder devices and PSI, so no
`psi=1` override was needed in the successful graphical run.

### 6.3 Image pins (OTA JSON fetched 2026-09-21, first entry = newest)

Pinned channel `waydroid_arm64_only`, LineageOS 20.0 (Android 13), built
2026-04-03:

| Image | Filename | Size | SHA-256 (`id`) |
| --- | --- | --- | --- |
| system VANILLA | `lineage-20.0-20260403-VANILLA-waydroid_arm64_only-system.zip` | 728,517,282 | `c3cf6ab5ded7496761bae4a49fe1a6f691923c5b84818755790f3ae4e239127b` |
| vendor MAINLINE | `lineage-20.0-20260403-MAINLINE-waydroid_arm64_only-vendor.zip` | 77,349,347 | `bad5e84a2a47b877d8a85f6a212720f5562e3eeb26696bfd0e2de50dab1ec3ba` |

URLs: `https://sourceforge.net/projects/waydroid/files/images/system/lineage/waydroid_arm64_only/<filename>/download`
and `https://sourceforge.net/projects/waydroid/files/images/vendor/waydroid_arm64_only/<filename>/download`
(index files `https://ota.waydro.id/system/lineage/waydroid_arm64_only/VANILLA.json`,
`https://ota.waydro.id/vendor/waydroid_arm64_only/MAINLINE.json`).

Alternative `waydroid_arm64` (with AArch32): system
`lineage-20.0-20260403-VANILLA-waydroid_arm64-system.zip` 904,855,273 bytes
`c4b45fad36bee7c0db8a1d9315a5be0035520c53d3d005a807735ae9b7ee79cf`; vendor
`lineage-20.0-20260403-MAINLINE-waydroid_arm64-vendor.zip` 147,648,350 bytes
`1e6d33d464277ea3964e4658001c8882f21325616d6bcc66d473bc9ee1e246c7`.

Handling: `tools/android/waydroid-images-fetch.sh` uses allow-listed hosts
`sourceforge.net` and `*.dl.sourceforge.net`, enforces size windows and
SHA-256 pins, and stores the zips under ignored
`out/android/waydroid/<build>/`; `pmos-android-build` copies the extracted
`system.img`/`vendor.img` into the guest at `/etc/waydroid-extra/images/`
and the finalize step runs `waydroid init -f -i /etc/waydroid-extra/images`
offline (no `ota.waydro.id` access from the guest), then writes
`/var/lib/waydroid/waydroid.cfg` `[properties]`:

```ini
ro.hardware.gralloc=default
ro.hardware.egl=swiftshader
persist.waydroid.multi_windows=false
ro.sf.lcd_density=400
```

followed by `waydroid upgrade -o`. The manifest records image filenames,
sizes, hashes, the `waydroid` package version, and the QEMU-only
`density_dpi` value.

### 6.4 Service start order (OpenRC, as pinned)

```text
cgroups + dbus (default runlevel)
  → 00-ferrix-android.start: offline init, renderer materialization, container
  → 00-ferrix-product.start: ferrix-shell, session bus, PulseAudio, Waydroid session
  → Fulguris install/default-browser verification
  → ferrix-appd (composes AndroidRuntime) and Home
```

The early Android hook runs before the compositor exists, so it initializes
the images and starts `waydroid-container` but cannot start the Wayland-backed
session. Product bootstrap creates the compositor socket and session services,
starts the Waydroid session, waits for Android framework boot, provisions
Fulguris, and only then starts `ferrix-appd`. Later launches go through the
private runtime adapter.

### 6.5 Generated data

`/out/` is ignored, so downloaded APKs, Waydroid images, overlays, logs, and
captures under `out/android/` and `out/qemu-aarch64/pmos-android/` never enter
Git.

### 6.6 `ferrix-appd` composition (`crates/ferrix-appd/src/main.rs`)

`ferrix-appd/Cargo.toml` includes `ferrix-runtime-android = { path =
"../ferrix-runtime-android" }`. The composition point is:

```rust
use std::sync::Arc;

use ferrix_app_runtime_api::Runtime;
use ferrix_runtime_android::{AndroidRuntime, AndroidRuntimeOptions, WaydroidCli, WaydroidOptions};
use ferrix_runtime_native::{NativeRuntime, NativeRuntimeOptions};

// Composition point: the only place runtime implementations are named.
let native: Box<dyn Runtime> = Box::new(NativeRuntime::new(NativeRuntimeOptions {
    sandbox_bin: args.sandbox.clone(),
    evidence: recorder.evidence().to_owned(),
}));
let android: Box<dyn Runtime> = Box::new(AndroidRuntime::new(
    Arc::new(WaydroidCli::new(WaydroidOptions::default())),
    AndroidRuntimeOptions {
        evidence: recorder.evidence().to_owned(),
        ..AndroidRuntimeOptions::default()
    },
));
let service = Arc::new(Service::new(
    catalog,
    vec![native, android],
    Box::new(ProcTree::new()),
    recorder.clone(),
    options,
));
```

`WaydroidOptions::default()` resolves the binary from `FERRIX_WAYDROID_BIN`
then `waydroid` on `PATH`; on a host without Waydroid every Android launch
returns `AppError::RuntimeUnsupported { runtime: Android }` and an
`android-runtime-absent` record with a reason. The
`daemons_name_their_backends_only_in_main`
architecture test already lists `ferrix_runtime_android` for `ferrix-appd`.

## 7. Adapter design: `ferrix-runtime-android`

```text
ferrix_appd ──Runtime::launch──> AndroidRuntime ──AndroidHost──> WaydroidCli ──subprocess──> waydroid
                                        │                          └──/proc scan──> host pids
                                        └──tests──> testing::FakeAndroidHost
```

- **Public surface** (crate-private to the platform; only `ferrix-appd`'s
  binary links it): `AndroidRuntime::new(Arc<dyn AndroidHost>,
  AndroidRuntimeOptions)`, `AndroidRuntime::status()`,
  `AndroidRuntime::install(app, config)` (explicit, never from `launch`),
  `AndroidLaunch`/`ApkSource` (the descriptor's `launch` object,
  `deny_unknown_fields`), `validate_package` (grammar
  `[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)+`), `validate_activity`
  (same, or `.Relative`), `AndroidHost`, `RuntimeStatus
  {absent|stopped|starting|running{session_pid}|failed{reason}}`,
  `LaunchedApp {pid, surface_client_pid}`, `HostError`, `WaydroidCli`,
  `WaydroidOptions {binary, proc_root, surface_client_needle}`,
  `parse_status`, `parse_app_list`, `pid_from_cmdlines`,
  `testing::FakeAndroidHost`.
- **`AndroidHost`**: `name`, `status`, `ensure_session(env, timeout)`,
  `install(apk)`, `is_installed(pkg)`, `launch(pkg, activity)`,
  `is_running(pkg)`, `stop(pkg)`, `app_pid(pkg)`, `surface_client_pid()`.
  `ensure_session` takes the launch environment because
  `waydroid session start` needs the compositor's `WAYLAND_DISPLAY` and
  `XDG_RUNTIME_DIR`.
- **`WaydroidCli`** maps: `status` → `waydroid status` (parser handles the
  tab-separated `Session:`/`Container:` lines and the "not initialized"
  notice); `ensure_session` → detached `waydroid session start` in its own
  process group, then polls `status` until `RUNNING` (default 180 s, TCG);
  `is_installed` → `waydroid app list` (`packageName:` lines); `launch` →
  `waydroid app launch <pkg>` or `waydroid shell -- am start -n
  <pkg>/<activity>`; `stop` → `waydroid shell -- am force-stop <pkg>`;
  `app_pid` → scan `/proc/*/cmdline` for `argv[0] == <pkg>` (zygote rewrites
  the command line to the process name), falling back to the lowest
  `<pkg>:<subprocess>` pid; `is_running` → the same scan, or `waydroid shell
  -- pidof -s <pkg>` when `/proc` is unavailable; `surface_client_pid` → the
  lowest host pid whose command line contains `surface_client_needle`
  (default `graphics.composer`, **unverified**, §5). Every CLI call is
  bounded (60 s) and a missing binary is `HostError::Absent`.
- **Error mapping** (`launch`): runtime binary absent →
  `AppError::RuntimeUnsupported { runtime: Android }` plus an
  `android-runtime-absent` record with the reason; `status` `failed` or a
  session that does not reach `RUNNING` → `LaunchFailed { reason:
  "session-failed: …" }`; package not listed → `LaunchFailed { reason:
  "not-installed" }` (launch never installs); bad `launch` object →
  `InvalidDescriptor`; wrong `RuntimeKind` → `RuntimeUnsupported`.
- **`RunningApp`**: `pid()` returns `surface_client_pid` when known, else
  the app pid (see limitation below). `try_wait`/`wait` poll
  `is_running`; a process that never appeared within `appear_grace` (30 s)
  or disappears for two consecutive polls is classified: after our own
  `terminate` → `ExitReason::Signaled { signal: 15 }`; otherwise →
  `ExitReason::Exited { code: 1 }` (a crash). The host cannot distinguish an
  Android-side finish from a crash or a low-memory kill; this is recorded as
  `exit_cause: "process-gone"` in diagnostics and is a documented
  limitation, not hidden. `terminate` force-stops and reports an error if
  the process outlives the grace period.
- **Diagnostics** (runtime-private, attached to `app-started`):
  `{"kind":"android","host":"waydroid","package":"net.slions.fulguris.full.fdroid",
  "activity":null,"pid":…,"surface_client_pid":…,"session":{"state":
  "running","session_pid":…},"exit_cause":null}`. The package name appears
  only here and in the runtime's records, never in `ferrix-app-protocol`.
- **Surface association limitation.** The compositor sees one Wayland client
  for the whole container: Waydroid's hwcomposer, whose host pid is not a
  descendant of `ferrix-appd`. `ferrix-appd`'s ancestry walk therefore
  cannot match it. The adapter reports that pid as `RunningApp::pid()` so
  appd's direct-pid match associates the surface; with `multi_windows=false`
  there is exactly one Android surface, so this is sufficient for the
  single-app phase. The `xdg_toplevel` `app_id` `waydroid.<package>` is
  useful for confirmation but is untrusted metadata and must not identify
  the caller. Appd refuses a launch whose reported runtime pid is already
  owned by another app, so this limitation fails closed instead of silently
  reassigning the shared surface. Multiple simultaneous Android apps would need Waydroid's
  multi-window mode and a per-window mapping — not in scope.
- **Records**: `android-runtime-absent`, `android-session-failed`,
  `android-app-not-installed`, `android-app-launched` (fields: `evidence`,
  `event`, `runtime`, `host`, `app`, `reason`/`package`/`pid`/
  `surface_client_pid`/`instance`).
- **Tests** (`cargo test -p ferrix-runtime-android`, 21 tests, `mock`):
  package/activity grammar; `launch` object parsing and unknown-field
  rejection; absent runtime → `RuntimeUnsupported`; failed session (both
  pre-existing `failed` status and a failing start) → `LaunchFailed`;
  not installed → `not-installed` and no install side effect; happy path
  with diagnostics, session reuse, and explicit activity; crash detection
  with two-poll confirmation and cached result; terminate → force-stop,
  `Signaled{15}`, idempotent; launch command failure; explicit install;
  pid fallback; `waydroid status` parser (running/stopped/frozen/
  not-initialized/garbage); `waydroid app list` parser; `/proc` cmdline
  ranking; missing binary → `Absent`; fake `/proc` tree scan.

## 8. Evidence and remaining acceptance work

`just pmos-android-check` is a separate TCG-compatible, headless, serial
check. It verifies the Android-enabled image's hashes and requires records for
Binder devices, offline Waydroid initialization, and the staged Fulguris APK;
it also prints the renderer/container outcome records for diagnosis. Because
it has no compositor, it deliberately does not claim package installation or
rendering.

Historical Brave graphical evidence on Apple Silicon/HVF (2026-09-21; not
evidence for the current Fulguris input):

1. A fresh overlay recorded Binder devices, offline Waydroid initialization,
   `renderer:"swiftshader"`, and Brave provisioning status `installed` with
   `browser_role:"default"`.
2. Launching `org.ferrix.browser` through `ferrix-appd` recorded
   `android-app-launched`, `app-started`, and the mapped
   `waydroid.com.brave.browser` surface.
3. An inspected Cocoa framebuffer showed Brave's first-run UI.
4. After orderly shutdown, reusing the overlay recorded provisioning status
   `present` with the browser role retained.

Current Fulguris acceptance work:

1. Run a fresh Fulguris graphical QEMU boot and inspect its framebuffer before
   claiming **graphical** compatibility.
2. Use synthetic touch and keyboard input to change Fulguris' rendered state
   before claiming **interactive** compatibility.
3. Force-stop Fulguris inside the container and verify `app-crashed` plus home
   recovery within the polling interval.
4. Measure boot-to-ready and time-to-first-frame under TCG if graphical TCG
   becomes an acceptance requirement; today only the headless check uses TCG.

Every passed device, socket, mount, and namespace of the LXC config must
be listed in `docs/status.md` with the evidence. Google services,
attestation, DRM, codecs, camera, telephony, biometrics, and background
execution remain **unverified** regardless of the outcome.

## 9. Fulguris acquisition (`tools/android/`)

- `fulguris.pins` (checked in) pins Fulguris Web Browser package
  `net.slions.fulguris.full.fdroid`, version `2.0.10` / version code `261`,
  asset `net.slions.fulguris.full.fdroid_261.apk`, and the direct F-Droid
  source URL:
  <https://f-droid.org/repo/net.slions.fulguris.full.fdroid_261.apk>. The
  pinned size is 9,310,262 bytes; the SHA-256 is
  `cc151e4608d68fa32aba0b5b80c9c89a286050d7b0710a081e77bc1dddbcc8b6`; and
  the F-Droid signing-certificate SHA-256 is
  `66958f3366388c23ca1a3cbfd91545cca4abf3e79edaab4fe988213e392b07a1`.
  The package page is
  <https://f-droid.org/en/packages/net.slions.fulguris.full.fdroid/> and the
  values correlate with F-Droid's `index-v2.json`.
- `fulguris-fetch.sh` (POSIX sh, `set -eu`) refuses to run if `curl`, Android
  SDK Build-Tools `apksigner`, or `shasum` is missing (reports, never
  installs); refuses placeholder or malformed pins; performs an HTTPS HEAD
  preflight against the exact pinned URL; allows only `f-droid.org`, requires
  an APK/octet-stream content type and the exact pinned size; downloads to
  `${FERRIX_OUT_DIR:-out}/android/fulguris/<VERSION>/<ASSET>.part`; re-checks
  the effective host and size; runs `fulguris-verify.sh`; and then renames
  atomically. `--local PATH` verifies a developer-supplied APK. Events are
  host-labelled and contain no credentials.
- `fulguris-verify.sh` checks the exact pinned size and SHA-256
  (`shasum -a 256`), then Android SDK Build-Tools
  `apksigner verify --print-certs` validates the APK signing schemes and
  reports the verified signer certificate; its SHA-256 must equal the pin
  (`wrong-signer` otherwise). A certificate merely embedded in the zip is
  never accepted as proof of a valid APK signature. `APKSIGNER` may select a
  pinned SDK installation; tooling is reported missing, never installed
  automatically.
- Tests: `sh tools/android/tests/run.sh` (19 assertions, host tooling) build a
  deterministic stored zip and fake `apksigner` to cover correct local
  verification, exact-size enforcement, wrong hash, wrong signer, invalid
  signature, placeholder pins, missing `apksigner`/`curl`, missing file,
  checked-in pins parsing, foreign redirect refusal, allowed-host download,
  tampered-download cleanup, and shell-safe alternate pins.
- Nothing from Fulguris is committed; the APK lives under ignored
  `out/android/fulguris/2.0.10/`.

## 10. Remaining risks and limitations

1. **Performance under TCG.** A full Android 13 userspace with SwiftShader
   on an emulated CPU may exceed the bounded check's time budget; the
   graphical TCG is not part of the bounded check. The separate Android check
   stays headless and serial-observable.
2. **Image size and provenance.** ~800 MiB of third-party images pinned by
   SHA-256 from SourceForge, built by the Waydroid project from LineageOS
   sources; the project verifies but does not reproduce those image builds.
3. **Android 13 security posture** of lineage-20 images (§3 "Security
   update path"). Revisit before any physical target.
4. **Architecture:** `arm64_only` intentionally excludes 32-bit Android apps.
5. **Audio stack**: PulseAudio in the guest vs PipeWire + pulse compat
   (unverified).
6. **Keyboard**: Android's own OSK inside the surface for now; a departure
   from the product OSK direction, to be reconciled in phase 3.
7. **Networking**: double NAT (waydroid0 → slirp) accepted for the browser
   case; the container is isolated from `/run/ferrix` by construction.
8. **Fulguris source**: the official F-Droid APK is pinned by exact URL,
   size, asset hash, and signer; `--local` accepts only a file matching the
   same pins.
