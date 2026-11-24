# Pixel 9a GNSS source evidence (spec 08, deliverable 1)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu`, `GXQ96`, `MP1.0`).
Spec: [`plans/todos/08-location-gnss.md`](../../../plans/todos/08-location-gnss.md)
deliverable 1. Issue #28.

This is a read-only feasibility record for the **Google Kleaf route that
Ferrix boots today**. It extends, and does not replace, the radio-first-route
audit in [`associated-radio-audit.md`](associated-radio-audit.md). No GNSS
traffic was sent, and no device node was opened. `capabilities.toml` is
unchanged, and no location API is added.

## Verdict

**NO-GO** for GNSS location on this target until four blockers are lifted.

The receiver is Samsung's "KEPLER" GNSS, coupled to the Exynos modem. Its
firmware boot and dump channels are CPIF channels over the S5300 PCIe link,
so a fix depends on the cellular CP being up (spec 01). The run-time transport
is a dedicated AP SPI link (`spi_21`) carrying Samsung's BETP frames through a
raw misc device, `/dev/gnss_ipc`. Everything that turns those frames into
positions is closed Android userspace: `gnssd`, the SLSI GNSS HAL,
`libsitril-gps`, and the firmware `kepler.bin`.

No maintained Linux stack (gpsd, GeoClue) speaks BETP.

### Blockers for a GO

1. **CP boot.** The S5300 CP must boot under Ferrix and keep the
   `gnss_boot` and `gnss_dump` CPIF channels serviceable. This is spec 01's
   work, which is not proven on this route ([`s5300-route-decision.md`](s5300-route-decision.md)).
2. **GNSS firmware boot.** We would need a licence-clear and documented
   sequence for booting the KEPLER core with `kepler.bin` (1,026,920 bytes in
   `vendor.img:/firmware/`). On stock, the closed `gnssd` does this.
3. **BETP decoding.** We would need either a documented BETP specification,
   or a receiver mode that emits NMEA or another standard protocol over
   `/dev/gnss_ipc`, consumable by gpsd behind a typed Ferrix location
   adapter. `gps.cfg` has an `NmeaBitmask` key, but that configures the
   closed daemon's own NMEA output. It is not evidence that the receiver
   speaks NMEA on the wire. Reimplementing an undocumented vendor protocol
   breaks the AGENTS.md rule against protocol reimplementation.
4. **Exact-target outdoor evidence** (spec 08 deliverable 4), with
   coordinates kept out of Git.

Two interim routes remain open:

- **Network location** (Wi-Fi, through GeoClue) is not GNSS. If pursued, it
  needs its own evidence row, and spec 08 requires it to stay separate.
- **Running the vendor GNSS stack inside the Android container** would
  depend on the same CP boot and on proprietary daemons with SIT RIL
  dependencies. It is not evaluated here and is not recommended.

## Physical evidence

Label: `physical:google-pixel-9a-tegu-gxq96-mp1-0`, read-only inventory taken
2026-11-24 on the booted Ferrix Kleaf dev route. The kernel was
`6.1.124-android14-11-ga4662a84a15f`. The raw transcripts are kept out of Git
(ignored `out/pixel-9a/inventory-20261124.txt` and `inventory3-20261124.txt`).

| Observation | Result |
| --- | --- |
| SPI | `spi21.0`, modalias `spi:gnss-spi`, bound to driver `gnss_spi` |
| Platform driver | `gnss_interface`, from the `gnssif` module |
| Device nodes | `/dev/gnss_ipc` (misc), `/dev/gnss_boot`, `/dev/gnss_dump`, `/dev/sscd_gnss`. There is no `/dev/gnss0`. |
| dmesg | `gnss: GNSS driver registered with major 503` (the GKI GNSS core; no receiver registers with it). Then `gif: gnss_probe: Exynos GNSS interface driver probe begins`, `parse_dt_common_pdata: device name: KEPLER node name: gnss_ipc`, and `gnss_spi_probe: +++` / `---`. `SPI driver gnss_spi has no spi_device_id for samsung,gnss-spi` is a harmless modalias warning. |
| Modules | `gnssif`, `gnss_spi`, `bcm47765` (no device bound), `cp_thermal_zone` |
| Modem coupling | The `cp_on_chip_0..3`, `rf1_therm` and `rf2_therm` thermal zones read empty, which is consistent with a CP that is not running |

## Offline source evidence

Label: `offline`. Sources are pinned by
[`kernel-sources.toml`](kernel-sources.toml) (manifest `893fbfd2`). `gps/broadcom/bcm47765`
(`f0c16ad0`) is not pinned in that file. Its commit is the one the same
manifest checks out. The factory `vendor.img` (SHA-256 `41c5d717…9bf7`) was
listed read-only outside the repository.

### Chip and binding

- [`zumapro-tegu-mp.dts` at `6276884d`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/dts/zumapro-tegu-mp.dts)
  includes `zuma/zuma-cp-s5400-phone-gnss.dtsi` and
  `zuma-tegu-gnss-samsung.dtsi`. So do every proto, EVT, DVT and PVT tegu
  board file. Only `zumapro-tegu-dev1.dts` uses `zumapro-phone-ignss.dtsi`.
- [`zuma-cp-s5400-phone-gnss.dtsi` at `42395e8a`](https://android.googlesource.com/kernel/devices/google/zuma/+/42395e8adcb880aa336f0da6bb2a19ed53929c40/dts/zuma-cp-s5400-phone-gnss.dtsi)
  declares:
  - a `gnssif` node with `compatible = "samsung,exynos-gnss"`,
    `device,name = "KEPLER"`, `device_node_name = "gnss_ipc"`, and the
    handshake GPIOs `gpio_gnss2ap_spi` (`gpa5-4`) and `gpio_ap2gnss_spi`
    (`gpp2-3`);
  - on `&spi_21`, a child `gnss@0` with `compatible = "samsung,gnss-spi"` at
    `spi-max-frequency = <3125000>`.
- [`zuma-tegu-gnss-samsung.dtsi`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/dts/zuma-tegu-gnss-samsung.dtsi)
  sets `&aoc { gnss-type = <2>; }` with the comment "Overwrite gnss-type to 2
  for samsung S5400". `aoc.c` (`ac69293d`) passes this value to the AoC
  firmware as `kAOCGnssType`. The AoC side of GNSS (for example the CHRE
  `location` nanoapp) is closed firmware.
- The Broadcom alternative,
  [`zumapro-board-gps.dtsi` at `8831630e`](https://android.googlesource.com/kernel/devices/google/zumapro/+/8831630eee2ad095b85ed452944e6353940966a5/dts/zumapro-board-gps.dtsi)
  (`compatible = "ssp-spi,bcm4775"`), is included by no tegu board file.
  `bcm47765.ko` is in `vendor_ramdisk.modules.tegu` and loads, but it binds
  nothing on this phone.

### Driver: raw BETP over SPI, no firmware load

[`gnssif_spi` in `soc/gs` at `20407875`](https://android.googlesource.com/kernel/google-modules/soc/gs/+/204078758737776959ecc85c527a00fd2cabb62f/drivers/soc/google/gnssif_spi):

- `gnss_main.c` is the `gnss_interface` platform driver. It parses
  `device_node_name`.
- `gnss_io_device.c` calls `misc_register()` with the DT node name, giving
  `/dev/gnss_ipc`. It provides `read`, `write` and `poll` over a queue of
  received BETP frames (`io_dev_recv_betp()`, `MAX_RXQ_DEPTH 1024`). Its
  `misc_ioctl()` rejects every command (`undefined cmd`).
- `gnss_keplerctl_device.c` handles only the SPI handshake GPIOs and the
  interrupt (`kepler_spi_irq_handler`, `kepler_send_betp_int`).
- `gnss_link_device.c` and `gnss_spi.c` are the SPI transport.
- There is no `request_firmware()` anywhere in the driver.
- `Kconfig` defines `EXYNOS_GNSS_IF_SPI`, described as "Exynos GNSS SPI
  interface driver".

It does not register with the Linux GNSS core
(`drivers/gnss/core.c`, GKI `a4662a84`). That is why the core's major number
exists while there is no `/dev/gnssN`.

### Firmware boot goes through the modem

[`zuma-cp-s5300-sit.dtsi` at `42395e8a`](https://android.googlesource.com/kernel/devices/google/zuma/+/42395e8adcb880aa336f0da6bb2a19ed53929c40/dts/zuma-cp-s5300-sit.dtsi)
(included by `zumapro-board-cp-s5300-sit.dtsi`) declares CPIF `io_device_15`
`gnss_boot` (`EXYNOS_CH_ID_GNSS_BOOT`, 240) and `io_device_16` `gnss_dump`
(`EXYNOS_CH_ID_GNSS_DUMP`, 239). Both are `IPC_RAW` over `LINKDEV_PCIE`. The
channel numbers come from `soc/gs` `include/dt-bindings/soc/google/exynos-cpif.h`,
and they are already recorded in
[`s5300-source-evidence.md`](s5300-source-evidence.md) and
`pmos/device-google-tegu/s5300-channels.toml`. These channels exist only while
CPIF services the S5300 link.

### Stock Android userspace (factory `vendor.img`)

| Path | Role |
| --- | --- |
| `/etc/init/init.gnss.rc` | Sets permissions on `/dev/gnss_ipc`, `/dev/gnss_dump`, `/dev/gnss_boot` and `/sys/devices/platform/gnssif/coredump`. Starts `service gnssd /vendor/bin/hw/gnssd` (user `gps`, `BLOCK_SUSPEND`) and `slsi_gnss_service` (`android.hardware.gnss-service`). |
| `/etc/init/pixel-gnss-default.rc`, `/bin/hw/android.hardware.gnss-service.pixel` | The Pixel GNSS HAL. It is enabled by `persist.vendor.gps.hal.service.name=vendor`, which is set in vendor `build.prop`. |
| `/firmware/kepler.bin` (1,026,920 bytes) | GNSS core firmware |
| `/etc/gnss/gps.cfg`, `ca.pem`, `bypass.bin`, `hash.bin` | Daemon configuration (SUPL and LPP aiding, `NmeaBitmask`, multiband duty cycles, `PPSTimeSyncEnable`) plus assistance material |
| `/lib64/libsitril-gps.so`, `libcustomgnss.so`, `vendor.google.gnss_ext-V1-ndk.so` | SIT RIL coupling for control-plane assistance, plus vendor extensions |

From the device-node permissions and channel names, `gnssd` is the only stock
consumer of `gnss_boot`, and so the component that boots `kepler.bin` through
the CP. This is an inference: the daemon is closed, and its behaviour was not
traced.

## Next read-only checks

For the owner, on the booted Ferrix dev route. These are listing and log reads
only. Do not open `/dev/gnss_*`.

```sh
ls -l /dev/gnss_ipc /dev/gnss_boot /dev/gnss_dump /dev/sscd_gnss; ls /sys/class/gnss 2>/dev/null
ls /sys/devices/platform/gnssif/
dmesg | grep -i -E 'gif:|gnss|kepler' | head -60
just pixel-9a-radios-associated
```

The last line is the checked-in, read-only associated-radio probe (`radios.sh
associated`). It writes the ignored `associated-evidence-latest.tsv` and keeps
GNSS `unsupported`. Re-run these checks after any spec 01 run that brings the
CP up, to see whether CPIF reports the `gnss_boot` channel as serviceable. The
verdict changes only when blockers 1–3 are lifted.
