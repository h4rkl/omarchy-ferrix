# Pixel 9a camera source evidence (spec 07, deliverable 3)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu`, `GXQ96`, `MP1.0`).
Spec: [`plans/todos/07-camera.md`](../../../plans/todos/07-camera.md)
deliverable 3. Issue #3. Contract: [ADR-009](../../../docs/adr/009-camera-contract-and-linux-boundary.md).

This is a read-only feasibility record. The Pixel `[camera]` entry stays
`unsupported` in `capabilities.toml`, and the session keeps composing
`UnsupportedCamera` on this target.

## Verdict

**NO-GO** for a Pixel 9a camera adapter (spec 07 deliverables 4 and 5).

None of the three image sensors can stream raw Bayer through V4L2 on this
kernel. No V4L2 capture device, media controller graph or sensor subdev exists
for the camera path. The CSI-2 receivers (CSIS) and the frame DMA (PDMA) are
plain register windows inside the `isp-fe` LWIS device. Google's closed camera
HAL drives them from userspace through LWIS ioctls, and the sensor
register-programming sequences live in that HAL, not in the kernel.

libcamera's simple pipeline plus the software ISP needs a V4L2 video node
behind a CSI-2 receiver driver and a sensor subdev. There is nothing on this
kernel for it to open.

Spec 07's risk note holds: the realistic outcome is "camera unsupported". The
owner decides the release-scope consequence (spec 07, owner-gated).

### What a GO would need

All of the following, each for the exact sensor fitted:

1. Sensor identification. The DT names the sensors only by codename:
   `sensor-barghest`, `sensor-leshen` and `sensor-leshen-uw`. For each, we
   would need the vendor part number and either a mainline V4L2 sensor driver
   or a licence-clear register map: modes, MIPI lane rate, power sequencing
   through the `slg51002` and `rt6160` camera PMICs, and clocks.
2. A V4L2 / media-controller driver for the Tensor G4 CSIS link, the MIPI
   CSI PHY (`phy_exynos_mipi`) and the PDMA writer. It must expose a raw
   Bayer capture video node. There is no public register documentation, so
   this means reverse engineering from LWIS command streams. That is a
   multi-month kernel project with no upstream counterpart.
3. A matching kernel built from source. Ferrix already builds GKI and vendor
   modules from source on this route, so this would be a new out-of-tree
   module.
4. libcamera pipeline support (simple pipeline plus soft ISP) for that
   graph, plus per-sensor tuning. Image quality would be well below stock,
   with no PDAF, OIS (`ois-nessie`), HDR or the TNR/GTNR blocks.
5. Exact-target proof, as spec 07 acceptance asks: a preview frame rate
   measurement and a saved still taken in the Camera app.

An optional, separate route needs none of the above: a USB UVC camera on the
USB-C port. The GKI sets `CONFIG_USB_VIDEO_CLASS=y`. That route would need its
own capability evidence and must never be presented as the phone's camera.

## Physical evidence

Label: `physical:google-pixel-9a-tegu-gxq96-mp1-0`, read-only inventory taken
2026-11-24 on the booted Ferrix Kleaf dev route. The kernel was
`6.1.124-android14-11-ga4662a84a15f`. The raw transcripts are kept out of Git
(ignored `out/pixel-9a/inventory-20261124.txt` and `inventory3-20261124.txt`).

| Observation | Result |
| --- | --- |
| `/sys/class/video4linux` | `video6` `s5p-mfc-dec`, `video7` `s5p-mfc-enc`, `video8`/`video9` secure MFC, `video10`/`video11` MFC OTF encoders, `video12` `exynos-jpeg`, and `v4l-touch0` `heatmap` (the touch controller). These are codec, JPEG and touch nodes only. There is **no capture node**. |
| `/dev/media*` | **None** (`/dev/media*` did not expand). `media-ctl` and `v4l2-ctl` are not installed. |
| LWIS | The `lwis` module is loaded, with platform drivers `lwis-top`, `lwis-i2c`, `lwis-ioreg`, `lwis-dpm`, `lwis-slc` and `lwis-test`. `/dev/lwis-sensor-barghest`, `/dev/lwis-sensor-leshen` and `/dev/lwis-sensor-leshen-uw` exist. The other LWIS nodes were not listed by this inventory's filter. |
| Sensor I2C clients | `2-001a`, `3-001a`, `5-0056`, plus EEPROM/actuator `2-0050`, `3-0050`, `5-0024` and flash `4-0063`. All have an empty `name` and no driver. LWIS owns them by bus address, not through an I2C client driver. |
| Camera modules | `phy_exynos_mipi` and `lwis` are loaded. No V4L2 sensor or CSIS driver is loaded. |
| Cooling devices | `CAM`, `TNR` and `INTCAM` exist, all at 0. |

## Offline source evidence

Label: `offline`. Sources are pinned by
[`kernel-sources.toml`](kernel-sources.toml) (manifest `893fbfd2`). `lwis` is
not pinned in that file. Its commit below is the one checked out by the same
manifest. The factory `vendor.img` (SHA-256 `41c5d717…9bf7`) and
`vendor_dlkm.img` (SHA-256 `32337be5…cb579e`) were listed read-only outside
the repository.

### Sensors are LWIS I2C devices

[`zuma-tegu-camera.dtsi` at `6276884d`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/dts/zuma-tegu-camera.dtsi)
declares nine `compatible = "google,lwis-i2c-device"` nodes:

| `node-name` | Bus | Address |
| --- | --- | --- |
| `sensor-barghest` | `hsi2c_5` | `0x56` |
| `sensor-leshen` | `hsi2c_3` | `0x1A` |
| `sensor-leshen-uw` | `hsi2c_2` | `0x1A` |
| `eeprom-nessie`, `act-nessie`, `ois-nessie` | `hsi2c_5` | `0x24` |
| `eeprom-smaug-leshen-uw` | `hsi2c_2` | `0x50` |
| `eeprom-smaug-leshen` | `hsi2c_3` | `0x50` |
| `flash-lm3644` | `hsi2c_4` | `0x63` |

None of these has a V4L2 sensor binding (`port`/`endpoint`,
`data-lanes`, `link-frequencies`). The camera PMICs are
[`zuma-tegu-camera-pmic.dtsi`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/dts/zuma-tegu-camera-pmic.dtsi)
(`dlg,slg51002` at `4-0075`, `richtek,rt6160` at `4-0077`).

### CSI receiver and ISP are LWIS register windows

[`zuma-isp.dtsi` at `42395e8a`](https://android.googlesource.com/kernel/devices/google/zuma/+/42395e8adcb880aa336f0da6bb2a19ed53929c40/dts/zuma-isp.dtsi)
is included by `zumapro.dtsi`. It declares:

- `google,lwis-ioreg-device` `isp-fe` with
  `reg-names = "isp-fe", "csis-link-phy", "isp-fe-sysreg", "pdma", "pdma-wrap", "cmu-top"`.
  The CSI-2 link, its PHY and the frame DMA are therefore raw register
  ranges of one LWIS ioreg device.
- More LWIS ioreg devices for the rest of the ISP: `gdc0`, `gdc1`, `mcsc`,
  `gse`, `be-core`, `gtnr-merge`, `gtnr-align`, `lme` and `votf`.
- `lwis-top`, `lwis-dpm`, `lwis-test` and `lwis-slc`.

[`lwis` at `13e2d358`](https://android.googlesource.com/kernel/google-modules/lwis/+/13e2d358e1cf3c6fe6cdc7bc9f42bc4b339e79f0)
(`lwis_ioctl.c`, `lwis_commands.h`, `lwis_device_ioreg.c`, `lwis_device_i2c.c`)
is a generic "lightweight imaging subsystem". It gives userspace register
read and write, I2C transfers, interrupts and events, buffers and fences. It
registers no V4L2 device.

### The only V4L2 drivers are codec and JPEG

A search of the pinned `soc/gs` tree (`20407875`) and `lwis` for
`video_register_device`, `v4l2_async_register_subdev` and `v4l2_subdev_init`
finds only `drivers/media/platform/exynos/mfc/mfc.c` (MFC) and
`drivers/media/platform/exynos/smfc/smfc-dev.c` (JPEG). That matches the
physical `/sys/class/video4linux` listing exactly. `phy-exynos-mipi.c` provides
the MIPI PHY for DSI and CSI, but not a capture path.

### Stock userspace that owns the camera

| Factory image path | Role |
| --- | --- |
| `vendor.img:/apex/com.google.pixel.camera.hal.apex` | The closed Google camera HAL, which programs LWIS and owns the sensor modes and ISP |
| `vendor.img:/etc/init/android.hardware.camera.provider@2.7-service-google-apex.rc` | Starts the provider from that APEX |
| `vendor.img:/etc/init/libg3a_gabc.rc`, `libg3a_gaf.rc`, `libg3a_ghawb.rc`, `init.camera.set-interrupts-ownership.rc` | Closed 3A (AE, AF, AWB) and interrupt-ownership setup |
| `vendor_dlkm.img:/lib/modules/lwis.ko`, `stmvl53l1.ko` | The LWIS kernel module and the laser-AF ToF driver |

Waydroid camera access (spec 07 deliverable 5) would need either this HAL or
a Linux camera underneath. Both are absent, so APK camera access is NO-GO
too.

## Next read-only checks

For the owner, on the booted Ferrix dev route. These only list nodes and read
the kernel log. Do not open the LWIS nodes or issue ioctls.

```sh
ls -l /dev | grep -E 'lwis|video|media|v4l'
ls /sys/class/lwis 2>/dev/null
for d in 2-001a 3-001a 5-0056; do echo "$d: $(ls /sys/bus/i2c/devices/$d 2>/dev/null | tr '\n' ' ')"; done
lsmod | grep -i -E 'lwis|mipi|slg51002|rt6160'
dmesg | grep -i -E 'lwis|csis|isp-fe|mipi' | head -80
```

These checks complete the LWIS node list and confirm that no capture node
appears later in boot. They cannot change the verdict unless a V4L2 capture
node or a `/dev/media*` graph appears.
