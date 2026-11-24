# Pixel 9a sensors source evidence (spec 04, deliverable 5)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu`, `GXQ96`, `MP1.0`).
Spec: [`plans/todos/04-lifecycle-suspend-sensors.md`](../../../plans/todos/04-lifecycle-suspend-sensors.md)
deliverable 5. Issue #9.

This is a read-only feasibility record. Nothing was written to the phone, and
no sensor code, adapter or capability change follows from it.
`capabilities.toml` is unchanged.

## Verdict

| Sensor | Verdict | One-line reason |
| --- | --- | --- |
| Accelerometer (auto-rotate) | **NO-GO** | Not on any AP bus. The IMU sits behind the AoC sensor hub and reaches Android only through the proprietary USF HAL. No IIO device exists for it. |
| Proximity (call blanking) | **NO-GO** | Same route as the accelerometer: there is no AP-side proximity node in the DT or in sysfs. The only AP ranging part is the rear-camera laser-AF ToF, which is not a proximity sensor. |
| Ambient light (auto-brightness) | **NO-GO** | Same route as the accelerometer: there is no AP-side ALS node in the DT or in sysfs. |

`iio-sensor-proxy` therefore has nothing to bind to on this target. Manual
rotation, rotation lock and the manual brightness rail stay the only routes.
Proximity blanking during calls stays unsupported, and the shell must not
claim it.

### What a GO would need

A GO for any of the three sensors needs one of the following, proven on this
exact target:

1. **A documented host protocol for the AoC sensor hub.** The stock route is
   the USF channel (`/dev/acd-com.google.usf`). Its client libraries
   (`libusf.so`, `sensors.usf.so`) are closed, and no public protocol
   description is pinned. Reimplementing an undocumented vendor protocol
   breaks the AGENTS.md rule against protocol reimplementation, so this
   route needs a vendor-published or licence-clear specification first.
2. **Or a CHRE route.** The CHRE host-message protocol is AOSP open source.
   The stock vendor image ships the generic AOSP context-hub HAL
   (`android.hardware.contexthub-service.generic`), and the AoC exposes CHRE
   channels. This route would need all of:
   - a nanoapp that forwards accelerometer, proximity or light samples to the
     host;
   - evidence that this AoC firmware accepts a project-built (unsigned or
     project-signed) nanoapp;
   - a project-owned Rust adapter that publishes the samples, either through
     an IIO-like typed API or as an `iio-sensor-proxy`-compatible D-Bus
     surface.

   None of this is evidenced. The stock nanoapps under `/vendor/etc/chre` are
   prebuilt binaries.
3. **An exact-target runtime transcript** showing samples that change with
   orientation, cover/uncover and light level. A loaded module or an open
   channel is not enough.

Until then, spec 04's rotation, proximity and auto-brightness acceptance items
stay open, and `[lifecycle]` must not cite sensors.

## Physical evidence

Label: `physical:google-pixel-9a-tegu-gxq96-mp1-0`, read-only inventory taken
2026-11-24 on the booted Ferrix Kleaf dev route. The kernel was
`6.1.124-android14-11-ga4662a84a15f`. The raw transcripts are kept out of Git
as ignored `out/pixel-9a/inventory-20261124.txt` and
`out/pixel-9a/inventory3-20261124.txt`. They contain no serials, MACs or IMEIs;
`serial_number` appears only as an attribute name.

| Observation | Result |
| --- | --- |
| `/sys/bus/iio/devices` | Two devices only: `iio:device0` (`s2mpg15-odpm`) and `iio:device1` (`s2mpg14-odpm`). Both are PMIC on-device power meters with `in_energyN_mean_raw` and `enabled_rails`. There is no `in_accel_*`, `in_proximity_*` or `in_illuminance_*` channel. |
| Input devices | `gpio_keys`, `goodix_fingerprint`, `cs40l26_input`, `synaptics_tcm_touch`, plus the two Ferrix test devices. There is no sensor-class input device. |
| AP I2C clients | No accelerometer, ALS or proximity client on any bus. `19-0029` is `stmvl53l1`, the rear-camera laser-AF ToF, and has no driver bound. All the other clients are power, audio, haptics, NFC, USB or camera parts. |
| AoC | `/sys/devices/platform/17000000.aoc` exists, with the attribute `sensor_power_enable` and service entries `com.google.usf`, `com.google.usf.non_wake_up`, `com.google.chre`, `com.google.chre.non_wake_up`, `usf_sh_mem_doorbell` and `chre_ctl`. Matching `/dev/acd-*` character devices exist. `revision` read `FW Revision : 0x0` / `HW Revision : 0x41a53` at capture time. |
| Loaded modules | `aoc_core`, `aoc_channel_dev`, `aoc_char_dev`, `aoc_control_dev` and related modules. No IIO sensor driver is loaded. |

The ODPM IIO devices are useful for spec 05's power measurement (see
[`power-thermal-source-evidence.md`](power-thermal-source-evidence.md)). They
are not motion or light sensors.

## Offline source evidence

Label: `offline`. All sources are the pinned Kleaf tree from
[`kernel-sources.toml`](kernel-sources.toml), manifest `893fbfd2`, read
read-only on the builder checkout.

### No AP-side motion, light or proximity node

- [`zumapro-tegu-mp.dts` at `6276884d`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/dts/zumapro-tegu-mp.dts)
  includes `zumapro-tegu-common.dtsi`. Every `compatible` reachable from the
  tegu board includes was listed. None of them is an accelerometer, gyroscope,
  ALS or proximity part. The full set is: LWIS camera I2C devices, `st,st21nfc`,
  `st,st54spi`, `st,stmvl53l1`, `goodix,fingerprint`, `synaptics,tcm-spi`,
  `ti,tas25xx`, `cirrus,cs40l26a`, `cirrus,cs35l41`, `richtek,swchg`,
  `richtek,rt6160`, `idt,p9222`, `dlg,slg51002`, `google,s2mpg15-spmic-thermal`,
  `google,ccd`, `at,24c08` and the panel and display nodes.
- [`zuma-tegu-ldaf.dtsi`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/dts/zuma-tegu-ldaf.dtsi)
  describes the `stmvl53l1@29` node on `hsi2c_19`, titled "Laser Detection
  Auto Focus". Its driver is
  [`drivers/input/misc/vl53l1/`](https://android.googlesource.com/kernel/google-modules/soc/gs/+/204078758737776959ecc85c527a00fd2cabb62f/drivers/input/misc/vl53l1)
  in `soc/gs` `20407875`. `zumapro/BUILD.bazel` builds it as `stmvl53l1.ko`,
  and the factory `vendor_dlkm.img` ships it. It is a camera ranging part
  behind the rear lens, not a front proximity sensor.

### Sensor power and transport belong to the AoC

- [`zumapro-common.dtsi` at `8831630e`](https://android.googlesource.com/kernel/devices/google/zumapro/+/8831630eee2ad095b85ed452944e6353940966a5/dts/zumapro-common.dtsi)
  gives the `&aoc` node `sensor_1v8-supply`, `sensor_3v3-supply` and
  `sensor_power_list = "sensor_1v8", "sensor_3v3"`. The sensor rails are AoC
  supplies, not supplies of any AP I2C/SPI device.
- [`aoc.c` at `ac69293d`](https://android.googlesource.com/kernel/google-modules/aoc/+/ac69293d11343b47f0b2b758b0df9f936360dbd4/aoc.c)
  has `sensor_power_enable_store()` and `reset_sensor_power()`, which toggle
  those rails around AoC firmware state.
- [`aoc_channel_dev.c`](https://android.googlesource.com/kernel/google-modules/aoc/+/ac69293d11343b47f0b2b758b0df9f936360dbd4/aoc_channel_dev.c)
  names the `com.google.usf`, `com.google.usf.non_wake_up`, `com.google.chre`,
  `com.google.chre.non_wake_up` and `usf_sh_mem_doorbell` services. The kernel
  only moves opaque messages and shared memory for these services. It has no
  IIO, input or sensor-class registration.

### Stock Android userspace (factory `vendor.img`)

Offline, the hash-verified factory archive
`tegu-cp3a.260905.009-factory-0c2f9053.zip` (SHA-256 `0c2f9053…c0d7`, see
[`bluetooth-firmware-source-evidence.md`](bluetooth-firmware-source-evidence.md))
was opened read-only. Its `vendor.img` (SHA-256 `41c5d717…9bf7`) was extracted
to a scratch directory outside the repository and listed with `debugfs -R ls`.
Only names and small text configs were read, and no bytes are committed.

| Path | What it shows |
| --- | --- |
| `/bin/hw/android.hardware.sensors-service.multihal`, `/etc/init/android.hardware.sensors-service-multihal.rc` | The Android sensors HAL runs as `system` with groups `wakelock context_hub input uhid audio` and `BLOCK_SUSPEND`. |
| `/etc/sensors/hals.conf` | Loads `sensors.dynamic_sensor_hal.so` (generic HID dynamic sensors) and `sensors.usf.so` (the AoC USF sub-HAL). |
| `/lib64/libusf.so`, `/lib64/sensors.usf.so`, `/etc/init/init.usf.rc` | The closed USF client and its persist registry (`/mnt/vendor/persist/sensors/registry`). |
| `/bin/hw/android.hardware.contexthub-service.generic`, `/etc/chre/*.so` | The generic CHRE HAL plus prebuilt nanoapps, for example `imu_cal`, `motiondetector`, `gesture` and `sensorcollector`. |
| `/firmware/aoc.bin` (29,076,160 bytes) | The AoC firmware that hosts the sensor hub. Ferrix already stages and loads this file for audio ([`audio-route-decision.md`](audio-route-decision.md)). |

Taken together: on stock, every accelerometer, proximity and light sample
reaches Android through AoC firmware and the closed USF sub-HAL, never through
a Linux sensor driver.

## Relationship to existing work

- AoC is already brought online by Ferrix's fail-open `ferrix-aoc-firmware`
  loader for audio. Sensors add no new firmware requirement, but they would add
  a new AoC protocol dependency. The audio notes already record that AoC also
  owns sensors, CHRE, UWB and touch TBN ([`audio-bringup.md`](audio-bringup.md)).
- The Ferrix shell's rotation path (`crates/ferrix-shell/src/orientation/`)
  can consume `iio-sensor-proxy`. It stays QEMU/mock-only on this target.

## Next read-only checks

These are for the owner to run on the booted Ferrix dev route. Each is
read-only: no writes, no ioctls, and no opening of AoC channels.

```sh
cat /sys/devices/platform/17000000.aoc/revision
cat /sys/devices/platform/17000000.aoc/restart_count
ls /sys/bus/iio/devices; for d in /sys/bus/iio/devices/*; do echo "$d $(cat $d/name)"; done
ls -l /dev/acd-com.google.usf /dev/acd-com.google.chre /dev/acd-chre_ctl
cat /proc/bus/input/devices | grep -E '^N:|^H:'
ls /sys/bus/i2c/devices/19-0029/; cat /sys/bus/i2c/devices/19-0029/modalias
find /lib/modules -name 'stmvl53l1*'
dmesg | grep -i -E 'aoc|usf|chre' | head -80
```

The first three lines check whether the `FW Revision : 0x0` reading came from
a capture taken before `ferrix-aoc-firmware` ran, and whether an AoC-online
boot adds any IIO device. The verdict does not change unless a motion, light or
proximity IIO channel appears.
