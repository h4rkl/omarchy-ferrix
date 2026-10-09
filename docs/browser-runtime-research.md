# Browser runtime research (2026-10-09)

Status: research note, not a decision. Subordinate to `plans/baseline.md`,
`plans/plan.md`, ADR-005 and ADR-007. Nothing here was run on the phone. The
evidence comes from repo docs, the pinned GKI source, Alpine package indexes
and upstream Waydroid sources, all fetched on 2026-10-09. Any statement marked
**unverified** has no primary-source confirmation yet.

## 0. Why the Browser tile fails today

`apps/org.ferrix.browser.json` is `runtime: "android"`, and its private
`launch` object names `net.slions.fulguris.full.fdroid` with the pin file
`tools/android/fulguris.pins`. `ferrix-appd` composes
`AndroidRuntime::new(WaydroidCli::new(WaydroidOptions::default()))`.
`WaydroidOptions::default()` looks for the binary in `FERRIX_WAYDROID_BIN`,
then for `waydroid` on `PATH`. When it is missing, `HostError::Absent` maps to
`AppError::RuntimeUnsupported { runtime: Android }` and an
`android-runtime-absent` record. Home then shows "no android runtime is
available" (`crates/ferrix-runtime-android/src/runtime.rs`).

- **QEMU:** this is expected on `just qemu-run` (Alpine smoke) and on
  `just pmos-run`. Waydroid is installed only by the separate opt-in image:
  `just waydroid-images-fetch && APKSIGNER=… just pmos-android-build &&
  FERRIX_PMOS_MEMORY=4096 FERRIX_PMOS_SMP=4 FERRIX_PMOS_FRESH=1
  FERRIX_QEMU_GRAPHICS=cocoa just pmos-android-run`
  (`targets/qemu-aarch64/README.md` §routes). On that route, Fulguris
  onboarding has an inspected framebuffer (HVF, `docs/status.md` 2026-09-28).
  Interaction past onboarding is only partially verified.
- **Pixel 9a:** Waydroid is not in the rootfs on purpose.
  `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/README.md` says the
  package "does not install Android or Waydroid", and `docs/status.md` lists
  "Browser: the Android runtime needs Waydroid, which is not in the rootfs".

## 1. What the authority says the Browser is

- ADR-005 chose Waydroid: "Waydroid is the selected runtime". It is
  implemented by "a **private** adapter crate `ferrix-runtime-android` behind
  `ferrix-app-runtime-api::Runtime`", which "shells out to the runtime's
  maintained CLI/API … It never speaks Binder."
- ADR-007 replaced the APK: "Keep Waydroid as the Android compatibility
  runtime, but replace the Brave APK with the F-Droid build of Fulguris Web
  Browser … The stable Ferrix descriptor id remains `org.ferrix.browser`."
- `docs/android-runtime.md` §4 scopes the decision to the QEMU pmOS route:
  "Use **Waydroid** as the Android compatibility runtime for the opt-in QEMU
  route". It was never accepted for a physical target.
- `plans/ferrix-mobile-architecture/SKILL.md` keeps engines replaceable:
  "Waydroid, LXC, WebKit/Wry, Tauri-like packaging, Halium, and libhybris are
  evaluation candidates, not architectural dependencies."
  `references/architecture.md` adds: "Keep rendering toolkit, web engine,
  Android container … replaceable", and for the web runtime: "WebKit/Wry or
  another engine is an implementation choice. Verify Wayland, software
  rendering, ARM64, accessibility, and security-update behavior before
  selection."
- `plans/baseline.md` says nothing about a browser. Its Android-related lines
  cover only claims ("must never claim to emulate or validate … Android
  compatibility layers") and the exclusion of AVD.
- `plans/ferrix-mobile-interaction-plan.md` puts `Browser` on the right radial
  command and says unavailable commands "remain visible with an explanation".
  The current notice already satisfies that.
- `plans/plan.md` and AGENTS.md: product code stays target-agnostic, and
  target selection belongs in bootstrap/staging. AGENTS.md also says "Prefer
  popular, maintained libraries … from existing ecosystems."

**Conclusion:** the authorized Browser is Fulguris, an Android app running in
Waydroid. A native browser would be a new decision (a new ADR that supersedes
the browser choice for some or all targets). The plans allow it, because the
engine and runtime are explicitly replaceable.

## 2. Feasibility of the planned Android runtime

### 2a. QEMU aarch64

The route is already implemented and has graphical evidence under HVF.

- Kernel: `linux-postmarketos-stable` 7.0.12 provides static binder nodes. No
  change is needed (`docs/android-runtime.md` §3, §6.2).
- Packages: Alpine `community/waydroid` 1.6.2 on 3.24-stable, with
  `waydroid-openrc`, `waydroid-nftrules`, `lxc`, `dnsmasq`, `pulseaudio` and
  `dbus` (`targets/qemu-aarch64/pmos/pmbootstrap-android.cfg`). The waydroid
  package itself is noarch, 225 KiB (371 KiB installed).
- Images: pinned `waydroid_arm64_only` LineageOS 20 images. The system zip is
  728,517,282 bytes and the vendor zip is 77,349,347 bytes. The route needs
  about 5 GiB of host space and 4 GiB of guest RAM.
- Rendering: forced to `ro.hardware.gralloc=default` and
  `ro.hardware.egl=swiftshader`. Waydroid's hwcomposer works against
  `ferrix-shell`'s current protocol set (wl_shm path).
- Gaps:
  - Under TCG, the graphical route is too slow to be part of a bounded check.
    Only the headless `pmos-android-check` runs under TCG.
  - Interaction past onboarding is partially verified.
  - The route is opt-in, so the default `qemu-run` and `pmos-run` will always
    show the "no android runtime" notice.

### 2b. Pixel 9a GKI (android14-6.1, Kleaf)

I checked GKI `arch/arm64/configs/gki_defconfig` at the pinned `gki-common`
revision `a4662a84…` from `kernel-sources.toml`.

| Waydroid/LXC need | GKI defconfig | Effect |
| --- | --- | --- |
| Binder | `CONFIG_ANDROID_BINDER_IPC=y`, `CONFIG_ANDROID_BINDERFS=y`; `BINDER_DEVICES` keeps its Kconfig default | No static nodes, because binderfs is enabled. Waydroid's `tools/helpers/drivers.py` mounts binderfs at `/dev/binderfs` and allocates the devices itself. OK. |
| ashmem/memfd | `CONFIG_ASHMEM=y` | OK; memfd is the fallback anyway. |
| PID namespace | `# CONFIG_PID_NS is not set` | **Blocker.** LXC unshares the PID namespace, and Android init expects to be PID 1 in its own namespace. The repo already adds `CONFIG_PID_NS=y` in `kernel-config.ferrix-gki` for the source-built GKI, which is **not yet flashed**. |
| User namespace | not set (Kconfig default n) | Waydroid's container is privileged (no idmap in `config_base`), so this is not needed for Waydroid. `ferrix-sandbox` needs it, and the fragment adds it. |
| UTS/IPC/NET ns | `CONFIG_NAMESPACES=y` with defaults | OK |
| cgroups | v2 + `MEMCG`, `CGROUP_FREEZER`, `CGROUP_BPF`, `PSI=y` | OK. `CGROUP_FREEZER` is needed for Waydroid's freeze. |
| veth / bridge / tun | `VETH=y`, `BRIDGE=y`, `TUN=y` | OK |
| NAT | `IP_NF_IPTABLES/NAT/TARGET_MASQUERADE/MANGLE=y`; **no `NF_TABLES`**; **no `NETFILTER_XT_TARGET_CHECKSUM`** | nftables cannot work, so the Pixel must not install `nftables`/`waydroid-nftrules`. `waydroid-net.sh` falls back to `iptables-legacy`, which the Alpine `iptables` package ships. The final mangle `CHECKSUM` rule fails without aborting, because the script has no `set -e`. Optionally add `CONFIG_NETFILTER_XT_TARGET_CHECKSUM=y` to the fragment. |
| loop, ext4, overlay, FUSE | `BLK_DEV_LOOP=y`, `EXT4_FS=y`, `OVERLAY_FS=y`, `FUSE_FS=y` | OK. No squashfs is needed because the images are ext4. |
| DMA-BUF heaps | `DMABUF_HEAPS=y` | OK |
| SELinux | `CONFIG_SECURITY_SELINUX=y`, and the pmOS host loads no policy | **Unverified risk.** Waydroid works on Ubuntu kernels in the same state, and on Halium/Droidian Android vendor kernels. Not tested on this kernel. |

Other Pixel-specific facts:

- GPU: the Mali modules are skipped, and Mesa has no route on the vendor kbase
  kernel, so there is no `/dev/dri` render node. `lxc.py` then auto-selects
  SwiftShader, the same pair the QEMU route forces. CPU rendering on the
  X4/A720 cores should be usable. Frame rate is **unverified**.
- Audio: the Pixel rootfs uses PipeWire, but Waydroid binds a PulseAudio
  socket and the container fails to start without one (pmOS wiki). It needs
  `pipewire-pulse`, which upstream has not verified, or the QEMU route's
  system PulseAudio.
- Input: touch arrives over `wl_touch`. Android shows its own keyboard inside
  the surface, which matters because `ferrix-shell` has no system OSK for
  foreign clients (§3).
- Storage: rootfs is a 2 GiB ext4 image on `userdata` (117 GB physical). The
  extracted system and vendor images (more than 805 MiB zipped, about
  1.5–2 GiB raw, **unverified**) plus Android `/data` require a larger rootfs
  image, or a separate image path on `userdata`.
- Board boundary: `pmos/README.md` ("does not install Android or Waydroid")
  and `linux-plan.md` (no Waydroid "as the operating environment") must be
  amended. Running Waydroid as an *app runtime* is different from using it as
  the OS, but the README's exclusion is explicit and needs owner sign-off.

**Verdict:**

- QEMU: works today on the opt-in route.
- Pixel: feasible, but blocked first on flashing the source-built GKI with
  `CONFIG_PID_NS=y`. After that, it needs packaging, image staging, an
  iptables-only network, a pulse socket, and on-device evidence.

### Adapter integration (no change to `ferrix-runtime-android` needed)

The crate only needs:

- a `waydroid` binary on `PATH`, or `FERRIX_WAYDROID_BIN`;
- an initialized container (`waydroid init` with pre-seeded images);
- the compositor's `WAYLAND_DISPLAY`/`XDG_RUNTIME_DIR`, passed to
  `ensure_session`;
- the package already installed (launch never installs);
- a `/proc` view of container processes, used for pid and surface
  association (the `graphics.composer` needle is still **unverified**).

The QEMU first-boot logic covers init, renderer properties, session start,
Fulguris install and the browser role. It lives in
`targets/qemu-aarch64/pmos/local.d/00-ferrix-android.start` and
`00-ferrix-product.start`. The Pixel needs the equivalent in its own
`ferrix-product.initd`.

## 3. Native-browser fallback

`ferrix-shell` advertises only these globals (`crates/ferrix-shell/src/compositor/mod.rs`, Smithay 0.7):

- `wl_compositor` and `wl_subcompositor`
- `wl_shm`
- `xdg_wm_base`
- `zwlr_layer_shell_v1`
- `wl_output` and `zxdg_output_manager_v1`
- `wl_data_device_manager`
- `wl_seat`

It does **not** advertise:

- `zwp_linux_dmabuf_v1`
- `wp_viewporter`
- `zxdg_decoration_manager_v1`
- `zwp_text_input_v3` and `zwp_input_method_v2`
- `zwp_virtual_keyboard_v1`
- `wp_fractional_scale_v1`
- `wp_presentation`
- `xdg_activation_v1`
- primary selection
- pointer constraints
- cursor-shape

Smithay 0.7 ships handlers for all of these (`smithay-0.7.0/src/wayland/`).

| Candidate | Alpine 3.24 aarch64 | Rendering without GPU | Fit with ferrix-shell | Notes |
| --- | --- | --- | --- | --- |
| **Firefox ESR** + `mobile-config-firefox` | `firefox-esr` 140.12.0 (edge 140.14.0); 83 MiB download, 216 MiB installed; `mobile-config-firefox` 5.4.1 | Software WebRender (SWGL) presenting through `wl_shm`; no Mesa needed | Its required globals (compositor, subcompositor, shm, xdg_wm_base, seat, output, data device) are all present. dmabuf/viewporter/fractional-scale are optional. GTK3 draws CSD; fullscreen hides it. | This is postmarketOS's default mobile browser, so it is maintained and current on ESR. Text entry: GTK uses text-input-v3, which is missing, so **the Pixel has no OSK** (QEMU has virtio-keyboard). Content sandbox uses user namespaces; it degrades without `USER_NS`. MPL-2.0. |
| Chromium | 152.0.7977.82 | SwiftShader/Skia CPU via Ozone Wayland | Works with the base protocols (**unverified** here) | Heavier (289 MiB installed). Sandbox needs a user namespace or suid. Also has no OSK without text-input-v3. |
| Epiphany (WebKitGTK 6.0) | epiphany 50.4; `webkit2gtk-6.0` **2.48.1** in both 3.24 and edge | GTK4 GL through llvmpipe, or `GSK_RENDERER=cairo` | Base protocols are enough; OSK has the same gap | The WebKitGTK version lags upstream, which is a security-update concern. The bubblewrap sandbox needs `USER_NS`. |
| WPE/Cog | no `cog`/`wpewebkit` package (only `libwpe` and `libwpebackend-fdo` in edge) | EGL | — | Would need packaging. Not soon. |
| Servo (Rust) | not packaged. `servo` crate 0.7.0 is on crates.io (updated 2026-10-05); the first crates.io release 0.1.0 was 2026-04-13 with an LTS track | GL through surfman; would need llvmpipe/OSMesa (software context **unverified**) | Would need a Ferrix-owned shell (winit or `ferrix-ui`) | Best long-term Rust fit for the Phase 4 web runtime. But it needs mozjs built on musl aarch64, web compatibility is incomplete, and the monthly API breaks. Not a near-term browser. |

The biggest gap for any native browser is the on-screen keyboard. Ferrix's
OSK lives inside `ferrix_ui` clients (`ferrix_ui::osk`). A third-party client
on a touch-only Pixel cannot type a URL until `ferrix-shell` provides one of:

- (a) `zwp_text_input_v3` + `zwp_input_method_v2` with a Ferrix OSK
  input-method client, which is the plan-aligned Phase-3 "on-screen keyboard
  integration";
- (b) at minimum `zwp_virtual_keyboard_v1` plus a layer-shell OSK.

## 4. Ranked recommendation

### 1. QEMU, now, at no cost

Use the existing `pmos-android-run` route for the Browser. Optionally, make
the absence clearer on routes without Android:

- Add a short note to `targets/qemu-aarch64/README.md` and `docs/status.md`
  that the Browser needs `pmos-android-run`.
- Optional product change: have `ferrix-appd` probe runtime availability and
  expose `available: false` with a reason in the catalog, so Home can show
  the tile disabled before a tap. This touches `ferrix-app-api` (descriptor
  view), `ferrix-app-protocol` (a schema bump) and `ferrix-basic-screen`.

### 2. Pixel, the plan-aligned path: Waydroid + Fulguris (ADR-007 extended to the board)

**Blocked on** flashing the source-built GKI from `kernel-config.ferrix-gki`
(`PID_NS`). That kernel also unblocks required app isolation. Steps:

1. Kernel: optionally add `CONFIG_NETFILTER_XT_TARGET_CHECKSUM=y` to
   `targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/kernel-config.ferrix-gki`.
   Rebuild with `just pixel-9a-kernel-build`, flash with
   `just pixel-9a-dev-flash`, and check that `/proc/self/ns/pid` exists and
   that `binder` is listed in `/proc/filesystems`.
2. Packages: add a target-scoped opt-in pmbootstrap config. Either create
   `pmos/pmbootstrap-android.cfg` or have `tools/pixel-9a/rootfs.sh` append
   the packages when `FERRIX_PIXEL9A_ANDROID=1`. Packages: `waydroid`,
   `waydroid-openrc`, `lxc`, `dnsmasq`, `iptables`, `pipewire-pulse` (or
   `pulseaudio`). Do **not** add `nftables` or `waydroid-nftrules`.
3. Staging, in `tools/pixel-9a/rootfs.sh`:
   - copy the pinned images (`tools/android/waydroid.pins`, via
     `waydroid-images-fetch.sh`) to `/etc/waydroid-extra/images`;
   - copy the verified Fulguris APK to `/usr/share/ferrix/android/fulguris.apk`;
   - write `waydroid.cfg` `[properties]` with `gralloc=default` and
     `egl=swiftshader`;
   - grow the rootfs image to about 6 GiB (the size is set in
     `tools/pixel-9a/dev-compose.py` and the rootfs builder);
   - record image hashes in the manifest.
4. Boot glue: port the Waydroid parts of `00-ferrix-android.start` and
   `00-ferrix-product.start` into
   `pmos/device-google-tegu/ferrix-product.initd`. The sequence is: offline
   `waydroid init -f`, `waydroid upgrade -o`, `waydroid-container`, then after
   `ferrix-shell` a pulse socket, `waydroid session start`, an idempotent
   Fulguris install, and the browser role. All of it must happen before
   `ferrix-appd` starts.
5. Docs and gates:
   - amend `pmos/README.md`, `linux-plan.md` and `capabilities.toml`
     (Android app runtime: mocked/unsupported → candidate);
   - extend ADR-007 or add ADR-008 for the physical target;
   - record the evidence in `docs/status.md`: an on-device framebuffer
     capture of Fulguris, plus the LXC device and mount list.
6. No changes to `ferrix-runtime-android` or `apps/org.ferrix.browser.json`.

### 3. Pixel and QEMU fallback: native Firefox ESR (new ADR-008)

Use this if the GKI flash or Waydroid-on-GKI stalls, or as the Android-free
default browser. Steps:

1. Write ADR-008 "Native browser fallback". It keeps `org.ferrix.browser` as
   the id and chooses the descriptor per target at staging time. No target
   conditionals go in apps or core.
2. Add a native descriptor variant such as
   `targets/<target>/apps/org.ferrix.browser.json`:
   - `runtime: "native"`;
   - `launch.exec: ["firefox-esr", "--kiosk"]` (or a `ferrix-browser` wrapper
     that sets `MOZ_ENABLE_WAYLAND=1` and a profile under `$HOME`);
   - `isolation.run_dir.mode: "private-run"`;
   - `mode: "best-effort"` on the Pixel until `USER_NS` lands.

   Select this variant in `tools/pixel-9a/rootfs.sh`, and in
   `targets/qemu-aarch64/pmos/finalize-in-container.sh` for the non-Android
   pmOS image.
3. Packages: `firefox-esr`, `mobile-config-firefox` and `font-dejavu` in
   `targets/boards/…/pmos/pmbootstrap.cfg` and
   `targets/qemu-aarch64/pmos/pmbootstrap.cfg`. The Alpine smoke initramfs
   stays browser-free.
4. Compositor, required for touch-only use: implement text input in
   `crates/ferrix-shell/src/compositor/mod.rs` (`TextInputManagerState`,
   `InputMethodManagerState`, ideally `VirtualKeyboardManagerState`). Add a
   Ferrix OSK input-method client that reuses `ferrix_ui::osk`. This is
   Phase-3 work, and Firefox is unusable on the Pixel without it.
5. Evidence: an inspected framebuffer on QEMU and on the Pixel, a URL typed
   through the OSK, a page load, and crash/terminate recovery through appd.

### 4. Not recommended near-term

- **Servo:** track it for the Phase-4 TypeScript/web runtime evaluation, not
  as the browser.
- **WebKitGTK/Epiphany:** Alpine's version lags upstream.
- **Chromium:** heavier than Firefox with no extra benefit.
- **WPE/Cog:** not packaged.

## 5. Honest blockers summary

- **Pixel Android runtime:** blocked on the unflashed source-built GKI
  (`PID_NS`) and on amending the board boundary docs. SELinux-in-container,
  PipeWire-pulse and SwiftShader performance on this kernel are **unverified**.
- **Pixel native browser:** blocked on compositor text-input/OSK support for
  foreign clients. Content-process sandboxing is reduced without `USER_NS`.
- **QEMU:** nothing is blocked. The Browser works only on `pmos-android-run`,
  so default routes will keep showing the notice unless the catalog gains an
  availability flag.

## Sources

- GKI defconfig at the pinned revision:
  <https://android.googlesource.com/kernel/common/+/a4662a84a15f38b99cf597354edccf75988852de/arch/arm64/configs/gki_defconfig>
- Waydroid sources, `main` branch (`data/configs/config_*`,
  `data/scripts/waydroid-net.sh`, `tools/helpers/drivers.py`):
  <https://github.com/waydroid/waydroid>
- Alpine `waydroid` APKBUILD, 3.24-stable:
  <https://gitlab.alpinelinux.org/alpine/aports/-/blob/3.24-stable/community/waydroid/APKBUILD>
- Alpine package index (`firefox-esr`, `mobile-config-firefox`, `chromium`,
  `epiphany`, `webkit2gtk-6.0`, `libwpe`, `waydroid`):
  <https://pkgs.alpinelinux.org/packages>
- Servo on crates.io: <https://crates.io/crates/servo>; Servo 0.1.0 release
  post: <https://servo.org/blog/2026/04/13/servo-0.1.0-release/>
- Repo sources: `docs/android-runtime.md`, `docs/adr/005-*.md`,
  `docs/adr/007-*.md`, `docs/app-platform.md`, `docs/status.md`,
  `plans/ferrix-mobile-architecture/`.
