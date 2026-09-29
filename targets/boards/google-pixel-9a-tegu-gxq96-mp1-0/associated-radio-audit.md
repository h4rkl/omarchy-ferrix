# Tegu associated-radio audit

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu`, `GXQ96`, `MP1.0`).

This is a source and route audit, not physical-device evidence. It covers the
radios and radio-adjacent controllers that are easy to lose when the cellular
route is changed from Google's Kleaf kernel to the independent
`linux-google-tegu-wwan` kernel. No Android userspace, proprietary module, or
firmware byte is copied by this record.

## Decision matrix

| Function | Exact source finding | Selected Ferrix route | Current evidence boundary |
| --- | --- | --- | --- |
| GNSS / GPS | The pinned radio-first DTS has a `samsung,kepler` device on `spi_21`, with `gnss2ap` and `ap2gnss` handshakes. Its `tegu.config` enables `CONFIG_GNSS` and `CONFIG_GNSS_KEPLER`. | Require the GNSS core, KEPLER driver, and SPI symbols in `kernel-radio-route.toml`; keep them built in so the radio-first initfs has no unverified GNSS `.ko` dependency. | The candidate driver exposes the Linux GNSS raw-device contract but forwards Samsung BETP frames; it does not provide Ferrix location/NMEA userspace. No physical Linux GNSS evidence exists. |
| NFC | The pinned Google Tegu source has `st21nfc.ko` and `st54spi.ko` in the vendor ramdisk list. Its NFC DTS node is an ST21 I2C client at `hsi2c_9`, with reset, IRQ, and power-idle GPIOs. | Not part of the selected radio-first route. The candidate Tegu DTS enables `hsi2c_9` but deliberately has no NFC child node or ST21/ST54 driver contract. | NFC and the secure-element path remain unsupported. Enabling the bus alone is not NFC support; a future route needs a source-built driver, exact DT wiring, Linux NFC userspace, and a reviewed ESE boundary. |
| UWB | The candidate tree contains dormant AoC UWB helper sources, but its Kbuild references an undefined `CONFIG_AOC_UWB_DRIVER`; the candidate Kconfig does not define that symbol and its Tegu DTS has no `google,aoc_uwb_rst` node. The official Pixel 9a specification lists Wi-Fi, Bluetooth, NFC, and dual-band GNSS, but not UWB. | No UWB route is selected or claimed. | Unsupported; no exact-target hardware or Linux runtime evidence justifies adding a UWB service or firmware row. |

## GNSS / KEPLER route details

The independent kernel revision pinned by
[`kernel-radio-route.toml`](kernel-radio-route.toml) contains:

- [`zumapro-tegu.dts`](https://github.com/SteffenDE/linux-google-tegu/blob/88335b765d920cc345cdffb9c1a6d77e3a531ff9/arch/arm64/boot/dts/exynos/google/zumapro-tegu.dts), which enables `spi_21` and declares `compatible = "samsung,kepler"`;
- [`drivers/gnss/kepler.c`](https://github.com/SteffenDE/linux-google-tegu/blob/88335b765d920cc345cdffb9c1a6d77e3a531ff9/drivers/gnss/kepler.c), which registers the receiver with the Linux GNSS core and transfers raw data over SPI;
- [`tegu.config`](https://github.com/SteffenDE/linux-google-tegu/blob/88335b765d920cc345cdffb9c1a6d77e3a531ff9/arch/arm64/configs/tegu.config), which sets `CONFIG_SPI=y`, `CONFIG_GNSS=y`, and `CONFIG_GNSS_KEPLER=y`.

The driver comments say the baseband starts the receiver and that the driver
does not load firmware or decode BETP. The existing S5300 userspace package is
therefore not enough to claim a location fix: after a native Linux boot we need
to observe `/dev/gnss*`, establish the CP/receiver startup sequence, and add a
reviewed Rust-owned location adapter or parser before promoting GNSS.

The route-local `modules-initfs-radio` list intentionally stays empty. GNSS is
required as a built-in kernel path by the route metadata; it must not be added
to that list as an assumed module.

The machine-checked route metadata records this boundary explicitly:
`gnss_binding = "samsung,kepler"`, `gnss_driver_source =
"drivers/gnss/kepler.c"`, and `gnss_userspace_status = "raw-device-only"`.
The kernel preflight rejects a route that changes those values or carries
`kepler`, `st21nfc`, `st54spi`, or `bcm47765` in the radio-first initfs. This
protects the native Kbuild handoff from silently inheriting Android/Kleaf
modules while keeping the candidate GNSS path available for later, evidenced
userspace work.

## NFC / ST54 route boundary

The Google Tegu source's pinned vendor module list names
[`bcm47765.ko`, `st21nfc.ko`, and `st54spi.ko`](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8ac412ee8/vendor_ramdisk.modules.tegu),
and its [NFC device-tree include](https://android.googlesource.com/kernel/devices/google/tegu/+/6276884d9940694b25a6934671918ec8/dts/zuma-tegu-nfc.dtsi)
describes the ST21 controller at I2C address `0x08`. Those are useful
comparison facts for the Google Kleaf route only. They do not make the
radio-first kernel compatible with Google's Android-only module ABI.

The radio-first DTS deliberately leaves the `hsi2c_9` address unclaimed. Do
not instantiate an ST21 node, copy `st21nfc.ko`/`st54spi.ko`, or stage an ESE
blob until a native Linux source route and its exact DT binding are reviewed.
ST54 is a secure-element companion to NFC rather than an independent user
radio; any future payment/ESE interface must also remain outside Android
Binder/HAL APIs.

## UWB boundary

The candidate's AoC UWB files only provide an internal reset-GPIO/service
helper. They are not a complete UWB controller driver or userspace API, are not
enabled by the candidate Kconfig, and are not instantiated by the Tegu DTS.
The [official Pixel 9a specification](https://store.google.com/product/pixel_9a_specs)
does not list UWB. Ferrix therefore records UWB as unsupported rather than
inventing a device node, firmware dependency, or capability.

## Promotion gate

No row in `capabilities.toml` is changed by this audit. Promotion of any
associated radio requires all of the following on the exact physical target:

1. a native Linux boot using the hash-verified radio-first kernel and rootfs;
2. kernel evidence for the exact driver/device-tree binding and firmware or CP
   provenance, with no Android framework dependency;
3. a working project-owned Rust service/adapter and an observable typed API;
4. a redacted runtime transcript proving the feature, not just a loaded module.

Until those gates pass, GNSS, NFC, ST54, and UWB remain unsupported even when
the kernel source contains a promising symbol or dormant helper.

No new `ferrix-hardware-api` capability or Rust location/NFC service is added
by this audit. The existing portable API has no location or NFC contract, and
the KEPLER driver forwards raw BETP frames without a reviewed native-Linux
parser/startup sequence. Adding a product-facing API now would overclaim an
unobserved subsystem; the next justified implementation is a target-local
read-only probe after a native boot, followed by a reviewed Rust adapter only
if the kernel/runtime contract is established.

The target-local `radios.sh associated` workflow now provides that bounded
probe. It reads only `/dev` and `/sys` GNSS/NFC/ST54/UWB surfaces, writes an
ignored `associated-evidence-latest.tsv`, and keeps all four rows
`unsupported`, even when a kernel surface is visible. It does not open device
nodes, send radio protocol traffic, toggle power, or add capability sections.
No physical Pixel 9a session has been run; the probe output is therefore
synthetic-test coverage and source/runtime evidence only until the exact target
boots native Linux and the promotion gates above are independently recorded.
