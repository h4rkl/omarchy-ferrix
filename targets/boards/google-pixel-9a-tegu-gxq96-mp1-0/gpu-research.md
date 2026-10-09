# Pixel 9a GPU: research, measurements and recommendation

Target: `google-pixel-9a-tegu-gxq96-mp1-0` only. Tracking: issues #4 (Mali
bring-up) and #19 (GPU-accelerated browser). Evidence labels follow
AGENTS.md. Nothing here applies to QEMU or to other Tensor devices.

## Hardware and kernel facts (physical, 2026-10-10)

- GPU: Mali-G715 MC7, `GPU identified as 0x2 arch 11.8.10 r1p3`, Valhall
  CSF. Kernel DDK `r52p0` (`private/google-modules/gpu`, Kleaf
  android14-6.1 GKI route, `6.1.124-android14-11`).
- `mali_kbase`/`mali_pixel` probe at boot. eudev modalias coldplug loads them
  even when `ferrix.modules_skip` names them, because that option only filters
  the initramfs lists. Boots have been stable with them loaded.
- CSF firmware: the kernel asks for the DT `firmware-name`, then
  `mali_csffw-r52p0.bin`, then `mali_csffw.bin`. The pinned CP3A.260905.009
  vendor image ships `mali_csffw-r51p0.bin`, `-r54p1/2/3.bin` and
  `-legacy-r56p0.bin`, but no r52p0. `r51p0` boots on this kernel
  (`Firmware boot completed`, `Firmware load successful`). It is staged by the
  `gpu-mali` rows in `firmware-stage-allowlist.tsv`. Firmware bytes stay out
  of Git.
- CPU DVFS works (`schedutil`, all 8 cores online, frequencies scale), so CPU
  frequency is not the cause of the browser slowness.

## Baseline: browser on the software path (physical)

Fulguris in Waydroid (LineageOS 20, `ro.hardware.egl=swiftshader`,
`gralloc=default`). SurfaceFlinger reports
`ANGLE (Vulkan 1.2.0 (SwiftShader Device (LLVM 10.0.0)))`. Six up-swipes and
three down-swipes on a search-results page give:

| metric | value |
| --- | --- |
| frames | 195 |
| janky | 95.4 % |
| p50 / p90 / p95 / p99 frame time | 81 / 97 / 101 / 113 ms |
| CPU busy (all 8 cores) | 56 % |

That is about 12 fps. Almost every frame is a "slow issue draw commands"
frame (GL work on SwiftShader).

## Offscreen GL benchmark (physical)

`egltest` is a small NDK program (kept out of the tree for now; see issue
#19). It renders 4 blended full-screen layers per frame into a 1080x2292
RGBA8 FBO with `glFinish` after each frame.

| renderer | ms/frame | fps |
| --- | --- | --- |
| SwiftShader via ANGLE (Waydroid LOS20 system EGL) | 22.50 | 44 |
| Mali-G715, stock `libGLES_mali.so` r54p3 (GLES 3.2) | 2.87 | 349 |

The Mali run loaded the CP3A vendor `lib64` from an Android 13 bionic process
inside the Waydroid rootfs. It needed:

1. the vendor AIDL allocator (`android.hardware.graphics.allocator-V2-service`)
   running and declared in VINTF;
2. a one-symbol `LD_PRELOAD` shim, because the A17 service calls
   `AServiceManager_addServiceWithFlags` (Android 14+);
3. the host PID namespace. In Waydroid's PID namespace,
   `gpu_dvfs_kctx_init()` oopses the kernel; see the next section.

## Kernel bug found: per-UID DVFS stats under PID namespaces

`pixel_gpu_dvfs_metrics.c:gpu_dvfs_kctx_init()` resolves the context owner
with `find_get_pid(kctx->kprcs->tgid)`. That looks an init-namespace TGID up
in the caller's PID namespace. Inside an LXC container the lookup returns
NULL, and `task->cred` dereferences it. The result was a panic (`KP: Oops:
Fatal exception: comm:egltest PC:gpu_dvfs_kctx_init+0x50`), reproduced twice.
Stock Android has no PID namespaces, so Google never hits it. The fix,
`kernel-patches/private/google-modules/gpu/0001-…`, uses `current_uid()`:
contexts are created from the owning process's ioctl. Without it, any
Waydroid GPU use panics the phone.

## Routes considered

| route | verdict | why |
| --- | --- | --- |
| Mesa panfrost/panvk + `drm/panthor` | not on this kernel | Panthor supports G715 (arch 11.8, firmware in linux-firmware since 2025-08) and Mesa panfrost GL builds for v11 (panvk v11 is gated behind `PAN_I_WANT_A_BROKEN_VULKAN_DRIVER`). But 6.1 lacks `drm_gpuvm`/`drm_exec`. Panthor would also lose Pixel's ACPM DVFS, power domains, SLC and thermal integration, and nobody has reported running Panthor on Tensor. Revisit with a ≥6.18 mainline zumapro kernel. |
| Mesa over `mali_kbase` | no | Upstream Mesa only speaks panfrost/panthor; the kbase forks are Vulkan-only release candidates. |
| libhybris in the host compositor | no | postmarketOS dropped musl libhybris in 2020; upstream targets glibc. The compositor is not the bottleneck anyway. |
| Stock Mali blob in Waydroid, LOS20 (Android 13) | blocked | A13 libui has no Gralloc5. The vendor ships only the stable-C mapper (`mapper.pixel.so`, `USE_MAPPER5`), so SurfaceFlinger and apps cannot allocate or map gralloc buffers. Declaring the AIDL allocator alone crash-loops the Android display stack (`GraphicBufferMapper: gralloc-mapper is missing`), and the panel flickers. |
| **Stock Mali blob in Waydroid, Android 14+ image** | **recommended** | Gralloc5 (A14+) loads `mapper.<suffix>.so` from the allocator's suffix. Waydroid's `dev/lineage-23.2` (Android 16) branches are active, and their commits already handle the Pixel 9 mapper. No arm64 images are published, so this means a LineageOS build. The hwcomposer needs a linear dma-buf path (`zwp_linux_dmabuf_v1`, which the pixman compositor imports) or the CPU-lock `wl_shm` fallback. |
| LOS20 + gchips gralloc4 rebuilt as mapper4 | fallback | Needs an AOSP tree too, plus the A13/A17 LL-NDK gap. |

## Recommendation and next steps

1. Land the kernel patch and CSF firmware staging (done in this change),
   rebuild the kernel, and confirm a GPU context created inside the Waydroid
   container no longer oopses.
2. Build a Waydroid `lineage-23.2` arm64_only system/vendor image on the
   builder. The owner must decide this: it needs `repo` and `git-lfs` on the
   builder, roughly 300 GB of disk and several hours. Then add the CP3A vendor
   graphics stack (`libGLES_mali`, allocator V2 service, `mapper.pixel.so`,
   VINTF fragments, `vendor.mali.*` properties) as an opt-in overlay
   extracted locally from the pinned factory image. Keep SwiftShader as the
   default and fallback. Re-measure Fulguris against the baseline above.
3. Keep `ferrix-shell` on pixman. Composition is not the bottleneck. A GLES
   renderer would need libhybris or Mesa, and neither exists here.

## Sources

- Kernel/GPU modules: https://android.googlesource.com/kernel/google-modules/gpu/+/refs/heads/android-gs-tegu-6.1-android16
- gchips gralloc4: https://android.googlesource.com/platform/hardware/google/gchips/+/refs/heads/main/gralloc4
- Gralloc5 (A14): https://android.googlesource.com/platform/frameworks/native/+/refs/tags/android-14.0.0_r1/libs/ui/Gralloc5.cpp
- Stable-C mapper: https://android.googlesource.com/platform/hardware/interfaces/+/refs/heads/main/graphics/mapper/stable-c/README.md
- Panthor fw/G715: https://github.com/torvalds/linux/blob/master/drivers/gpu/drm/panthor/panthor_fw.c, https://gitlab.com/kernel-firmware/linux-firmware/-/tree/main/arm/mali
- Mesa panfrost: https://docs.mesa3d.org/drivers/panfrost.html, https://gitlab.freedesktop.org/mesa/mesa/-/blob/main/src/panfrost/model/pan_model.c
- Waydroid hwcomposer: https://github.com/waydroid/android_hardware_waydroid/blob/HEAD/hwcomposer/wayland-hwc.cpp
- Waydroid lineage-23.2: https://github.com/waydroid/android_device_waydroid_waydroid/tree/dev/lineage-23.2
- Waydroid image builds: https://docs.waydro.id/development/compile-waydroid-lineage-os-based-images
- Smithay pixman dmabuf import: https://github.com/Smithay/smithay/blob/master/src/backend/renderer/pixman/mod.rs
- minigbm Arm metadata: https://chromium-review.googlesource.com/c/chromiumos/platform/minigbm/+/6449959
