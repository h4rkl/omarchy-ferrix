# Pixel 9a NFC and fingerprint source evidence (spec 09, deliverables 1 and 3)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu`, `GXQ96`, `MP1.0`).
Spec: [`plans/todos/09-nfc-fingerprint.md`](../../../plans/todos/09-nfc-fingerprint.md)
deliverables 1 and 3. Issue #29.

Spec 09 asks for two separate decisions, so this document records two
independent verdicts. It is a read-only record. No NFC frame was sent, the
fingerprint sensor was not powered or read, and no biometric data exists in
this work. `capabilities.toml` is unchanged.

It extends the NFC/ST54 boundary in
[`associated-radio-audit.md`](associated-radio-audit.md), which was written
for the radio-first route, to the Google Kleaf route that Ferrix boots today.

## Verdicts

| Feature | Verdict | One-line reason |
| --- | --- | --- |
| NFC tag read | **GO-with-conditions**, for a bounded kernel feasibility spike only. Product NO-GO until a tag read is proven. | The ST54L NFC controller is reachable from the AP over I2C, and its firmware lives in the controller, but this kernel has no Linux NCI driver for it. The downstream driver is a raw character device for the closed ST HAL. |
| Fingerprint unlock | **NO-GO** | The Goodix under-display sensor has no AP data path. The kernel driver only handles GPIO, IRQ and power. Image capture and matching run in a Trusty TA (`g7a.app`) that the closed Goodix HAL drives. libfprint and fprintd cannot reach the sensor. |

### What an NFC GO needs

The spike stays inside owner-approved kernel and DT changes on the dev slot.
Each step needs evidence:

1. **An NCI transport driver that speaks this controller's framing.** The
   downstream `st21nfc` reads a 3-byte NCI header and then the payload, which
   is plain NCI over I2C. Mainline `st-nci` (pinned GKI `drivers/nfc/st-nci/`)
   wraps NCI in its NDLC link layer (`ndlc.c`, `PCB_TYPE_DATAFRAME`). It
   matches only `st,st21nfcb-i2c`, `st,st21nfcb_i2c` and `st,st21nfcc-i2c`.
   It is therefore not expected to work with ST54L unchanged. The spike must
   do one of the following:
   - show that an existing maintained Linux NCI driver works with ST54L;
   - or scope a small transport module on the Linux NCI core (`nci_register_device`,
     as mainline NCI I2C drivers do). That module would need review as new
     kernel code under AGENTS.md.
2. **Kernel configuration.** `CONFIG_NFC=m` is already built and loaded
   (`nfc.ko`). `CONFIG_NFC_NCI` is not in the GKI defconfig. Ferrix already
   builds GKI from source with `kernel-config.ferrix-gki`, so it can be added
   there.
3. **DT rebinding.** A reviewed DT overlay must bind the `hsi2c_9` `0x08`
   client to the chosen driver, keeping `reset-gpios`, `irq-gpios` and
   `pidle-gpios`, and must blocklist `st21nfc`. The downstream node also
   programs a PMU clock pad (`pmu_clk_pad = <0x3e84>`, `st,extpad_control_en`).
   The spike must show that the RF clock is enabled without it, or must
   reproduce it.
4. **RF configuration.** The stock HAL loads `st54l_fw.bin` and
   `st54l_conf.bin` from `/vendor/firmware`. Assume no Ferrix firmware or
   conf update; the controller keeps its resident firmware. The spike must
   show that a tag is read with the controller's current, stock-applied
   configuration. If it is not, record that as a blocker, not as a reason to
   copy the HAL's update logic.
5. **Userspace and end-to-end flow.** neard (GPL-2.0, a separate process)
   behind a typed Ferrix NFC adapter, with a tag read proven through an
   app-facing flow on this exact target. Secure-element, eSE (`st54spi`), HCE
   and payment claims stay out of scope, as spec 09 requires.

If step 1 finds no working transport and the review rejects new driver code,
the verdict becomes NO-GO.

### What a fingerprint GO would need

None of the following is available, and none can be built by the project
alone:

- a Trusty TA interface, or a licence and specification, for the Goodix
  sensor TA (`g7a.app`);
- or a non-TEE sensor data path that Linux can reach;
- plus a libfprint driver for that sensor and an enrolment and unlock design
  tied to spec 12.

The project cannot load its own TA into the production Trusty, and the AP
cannot read the sensor without the TEE. Fingerprint therefore stays visibly
absent from Settings (spec 09 deliverable 5).

## Physical evidence

Label: `physical:google-pixel-9a-tegu-gxq96-mp1-0`, read-only inventory taken
2026-11-24 on the booted Ferrix Kleaf dev route. The kernel was
`6.1.124-android14-11-ga4662a84a15f`. The raw transcripts are kept out of Git
(ignored `out/pixel-9a/inventory-20261124.txt` and `inventory3-20261124.txt`).

### NFC

| Observation | Result |
| --- | --- |
| I2C | `9-0008`, name `st21nfc`, driver `st21nfc` |
| SPI | `spi15.0`, `spi:st54spi`, driver `st54spi` (the eSE link) |
| Device nodes | `/dev/st21nfc` (misc), `/dev/st54spi` |
| Modules | `nfc` (the Linux NFC core), `st21nfc`, `st54spi`. There is no NCI or `st_nci` module. |

### Fingerprint

| Observation | Result |
| --- | --- |
| Platform drivers | `goodix_fp`, `fps_touch_handler`, `trusty`, `trusty-irq`, `trusty-log`, `trusty-virtio`, `trusty-test` |
| Device nodes | `/dev/goodix_fp` |
| Input | `event1` `goodix_fingerprint`, which is a key-event input device |
| SPI | Only `spi15.0` (ST54), `spi20.0` (touch, `synaptics_tcm_spi`) and `spi21.0` (GNSS). **No fingerprint SPI device is visible to Linux.** |
| Modules | `goodixfp`, `fps_touch_handler` |

## Offline source evidence

Label: `offline`. Sources are pinned by
[`kernel-sources.toml`](kernel-sources.toml) (manifest `893fbfd2`). `nfc`
(`bc086de2`) and `fingerprint/goodix` (`1ef2931f`) are not pinned in that
file. Their commits are the ones the same manifest checks out. The factory
`vendor.img` (SHA-256 `41c5d717…9bf7`) and `vendor_dlkm.img` were listed
read-only outside the repository, and no bytes are committed.

### NFC controller, binding and driver

- [`zuma-tegu-nfc.dtsi` at `6276884d`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/dts/zuma-tegu-nfc.dtsi)
  declares `&hsi2c_9` at `clock-frequency = <950000>` with child
  `st21nfc@8`. Its properties are `compatible = "st,st21nfc"`,
  `reset-gpios` (`gph1-6`), `irq-gpios` (`gpa7-5`), `pidle-gpios` (`gph1-3`),
  `pmu_clk_pad = <0x3e84>` and `st,extpad_control_en`.
  [`zuma-tegu-ese.dtsi`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/dts/zuma-tegu-ese.dtsi)
  adds the `st,st54spi` eSE on SPI.
- [`nfc/st21nfc.c` at `bc086de2`](https://android.googlesource.com/kernel/google-modules/nfc/+/bc086de230162e9fc77bd59f9791d30d7f2c9e47/st21nfc.c)
  registers a misc device named `"st21nfc"`. Its `read` path reads
  `HEADER_LENGTH 3` bytes and then `buffer[2]` bytes of payload, which is a
  plain NCI packet. `st21nfc.h` defines only GPIO, clock and power ioctls:
  `ST21NFC_GET_WAKEUP`, `ST21NFC_PULSE_RESET`, `ST21NFC_RECOVERY` and
  `ST21NFC_CLK_ENABLE`/`DISABLE`/`STATE`. The driver does not register with
  the Linux NFC or NCI core. `nfc/ese/st54spi.c` is the eSE pass-through.
- Pinned GKI `a4662a84`: `arch/arm64/configs/gki_defconfig` has `CONFIG_NFC=m`
  and no `CONFIG_NFC_NCI`. `drivers/nfc/st-nci/i2c.c` matches
  `st,st21nfcb-i2c`, `st,st21nfcb_i2c` and `st,st21nfcc-i2c`, and uses the NDLC
  link layer (`ndlc.c`).
- `vendor_ramdisk.modules.tegu` lists `st21nfc.ko` and `st54spi.ko`, as the
  existing audit already records.

### NFC stock userspace (factory `vendor.img`)

| Path | Role |
| --- | --- |
| `/bin/hw/android.hardware.nfc-service.st`, `/etc/init/nfc-service-default.rc` | ST NFC AIDL HAL (user `nfc`) |
| `/lib64/nfc_nci.st21nfc.default.so` | ST HAL core, named in `NCI_HAL_MODULE` |
| `/etc/libnfc-hal-st.conf` | `ST_NFC_DEV_NODE="/dev/st21nfc"`, `STNFC_FW_PATH_STORAGE="/vendor/firmware"`, `STNFC_FW_BIN_NAME="/st54l_fw.bin"`, `STNFC_FW_CONF_NAME="/st54l_conf.bin"`, eSE and UICC off-host routes |
| `/firmware/st54l_fw.bin` (252,333 bytes), `/firmware/st54l_conf.bin` | Controller firmware and RF configuration that the HAL applies. The controller part is **ST54L**, as named by these files. |
| `/bin/hw/android.hardware.secure_element-service.thales`, `…secure_element-service.uicc`, `/etc/libse-gto-hal.conf` | Secure-element HALs. Out of scope. |

### Fingerprint sensor, transport and TEE

- [`zumapro-tegu-udfps.dtsi` at `6276884d`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/dts/zumapro-tegu-udfps.dtsi)
  puts `goodixfp` under `&odm`, not under an SPI controller. It has
  `compatible = "goodix,fingerprint"`, `fp-gpio-irq` (`gpa7-6`),
  `fp-gpio-reset` (`gpp1-0`) and `spi-max-frequency = <1000000>`, plus
  `fps_touch_handler`. `s_ldo15_reg` is fixed always-on at 3.3 V. The
  under-display optical sensor is the "UDFPS" of the file name.
- [`drivers/input/fingerprint/` in `soc/gs` at `20407875`](https://android.googlesource.com/kernel/google-modules/soc/gs/+/204078758737776959ecc85c527a00fd2cabb62f/drivers/input/fingerprint):
  `gf_spi.h` defines `USE_PLATFORM_BUS 1` and leaves `USE_SPI_BUS` commented
  out. As a result, `gf_spi.c` registers a `platform_driver`, not an
  `spi_driver`. The character device's ioctls (`GF_IOC_RESET`,
  `GF_IOC_ENABLE_IRQ`, `GF_IOC_ENABLE_POWER`, `GF_IOC_ENABLE_SPI_CLK`,
  `GF_IOC_INPUT_KEY_EVENT`, `GF_IOC_CHIP_INFO` and so on) handle only power,
  reset, IRQ, clock and key events. `netlink.c` forwards IRQ notifications
  on `NETLINK_TEST` (25). The AP kernel has no path that reads sensor data.
- [`fingerprint/goodix` at `1ef2931f`](https://android.googlesource.com/kernel/google-modules/fingerprint/goodix/+/1ef2931fdde39005e2849f55b3d21a3019a7d2a0)
  contains only `fps_touch_handler`, which forwards touch-panel finger events
  for the UDFPS illumination flow.
- `trusty` (`2494a3e1`) provides the Trusty IPC transport. It does not
  describe the fingerprint TA's protocol.

### Fingerprint stock userspace (factory `vendor.img`)

| Path | Role |
| --- | --- |
| `/etc/init/fingerprint-goodix.rc` | When `ro.vendor.trusty.storage.fs_ready=1`, runs `trusty_apploader /vendor/firmware/${ro.vendor.fingerprint.ta.name}` and then starts `vendor.fingerprint-goodix` |
| vendor `build.prop` | `ro.vendor.fingerprint.ta.name=g7a.app` |
| `/firmware/g7a.app` (14,086,533 bytes) | The Goodix Trusty TA, which handles capture, anti-spoof and matching |
| `/bin/hw/android.hardware.biometrics.fingerprint-service.goodix`, `/lib64/libvendor.goodix.hardware.biometrics.fingerprint@2.1.so`, `fp_utils.so`, `com.google.hardware.biometrics.fingerprint.fingerprint-ext-V1-ndk.so` | Closed Goodix HAL and extensions |
| `/bin/hw/android.hardware.gatekeeper-service.trusty`, `…keymint-service.rust.trusty` | The Android authentication-token chain that the fingerprint HAL relies on. Ferrix has none of it. |

## Next read-only checks

For the owner, on the booted Ferrix dev route. These list nodes and read logs
only. Do not open `/dev/st21nfc`, `/dev/st54spi`, `/dev/goodix_fp` or any
Trusty node.

```sh
ls -l /dev/st21nfc /dev/st54spi /dev/goodix_fp; ls /dev | grep -i -E 'trusty|tipc'
ls /sys/bus/i2c/devices/9-0008/; cat /sys/bus/i2c/devices/9-0008/modalias
grep -E '^(nfc|st21nfc|st54spi|goodixfp|trusty)' /proc/modules
ls /sys/bus/platform/devices | grep -i -E 'goodix|trusty|fps'
grep -A4 -i goodix /proc/bus/input/devices
dmesg | grep -i -E 'st21nfc|st54|nfc|goodix|gf_|trusty' | head -80
```

These checks confirm that no NCI device appears, and they record the Trusty
node names. They do not change the fingerprint verdict. The NFC spike needs
an owner-approved build and flash, not more read-only checks.
