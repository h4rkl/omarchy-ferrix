# Google Pixel 9a — tegu / GXQ96 / MP1.0

This is an exact physical-target identity and stock-recovery package for the
attached device. It is not a claim that Ferrix Linux boots on this phone.
Ferrix product code remains target-neutral; the capability matrix is
conservatively `unsupported` until target-specific Linux evidence exists.

Identity was established by the checked-in read-only workflow
`just pixel-9a-discover`:

| Field | Observed value |
| --- | --- |
| Manufacturer / product | Google / Pixel 9a |
| Model number / SKU | GXQ96 |
| Codename / board | `tegu` |
| Hardware revision | `MP1.0` |
| Bootloader | `tegu-16.0-12754673`, locked |
| Verified boot | `green` |
| Current slot | `_a` |
| Dynamic partitions | `true` |
| Stock build | `BD4A.240925.111`, Android 15, 2025-01-05 SPL |

The serial and all other unique identifiers are intentionally absent. The
redacted discovery transcript is generated under ignored `out/pixel-9a/`.

## Recovery files

The checked-in pins select Google's current public Pixel 9a build recorded on
2026-09-28: `CP3A.260905.009` / Android 17.0.0.

Run:

```sh
just pixel-9a-recovery-download
```

The workflow downloads and SHA-256 verifies both archives under:

```text
targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/recovery/
```

That exact folder is ignored by Git. The factory archive is the Fastboot
recovery input. The full OTA archive is the preferred ADB recovery input.
Firmware bytes remain local; the checked-in `recovery.pins` file records the
official URLs and hashes.

## Bootloader unlock gate

The recovery gate is satisfied for the expected ADB/Fastboot failure modes,
but unlocking is destructive: Google will erase all user data. Make a
verified backup outside this repository before proceeding. The checked-in
workflow re-verifies both recovery archives and the exact device identity,
refuses ambiguous transports, prints the serial/product/revision/slot and
artifact hashes, and stops on any mismatch.

Run the read-only gate first:

```sh
just pixel-9a-unlock-bootloader --preflight
```

After reviewing that output and accepting the data wipe, the execution mode
requires two exact typed confirmations and the phone's own bootloader prompt:

```sh
just pixel-9a-unlock-bootloader --confirm-unlock
```

The workflow may issue only `adb reboot bootloader` and then
`fastboot flashing unlock`. It does not flash a ROM or change slots. If the
unlock succeeds, the phone remains in Fastboot so the verified factory archive
can be restored. Do not continue if the preflight or either identity check
fails.

## Recovery choices

Back up personal data first. Google documents full OTA sideload as the safer
route: it normally does not require an unlocked bootloader or a data wipe. The
current device is locked, so this is the preferred route while stock recovery
is intact.

For an OTA sideload, use the verified `tegu-ota-*.zip`, enter the phone's
stock recovery and choose **Apply update from ADB**, confirm that `adb devices`
shows the device in `sideload` state, then run the equivalent of:

```sh
adb sideload targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/recovery/tegu-ota-cp3a.260905.009-6750e3df.zip
```

That is a state-changing operation and is documented here for the operator;
this change does not invoke it. Follow Google's recovery instructions and
verify the device/build again afterward.

For Fastboot restoration, use the verified
`tegu-cp3a.260905.009-*-factory-*.zip` archive. It contains Google's factory
image and `flash-all.sh`. Factory flashing commonly requires an unlocked
bootloader and erases user data; do not run the vendor script blindly. Review
its exact partitions and hashes, make a verified backup outside the repository,
and require explicit per-run confirmation before any unlock, wipe, flash, slot
change, or relock. This repository has not run a Fastboot mutation and does
not claim a tested restore path yet.

For expected failure states, use the route matching the transport: stock
Android or stock recovery can use the verified full OTA with `adb sideload`;
an unlocked Fastboot device can use the verified factory archive and its
reviewed Fastboot commands. A hardware, cable, or bootloader failure that
leaves neither ADB nor Fastboot visible cannot be repaired by a host-side
workflow alone.

## Source and support boundary

- Google factory images: <https://developers.google.com/android/images>
- Google full OTA images: <https://developers.google.com/android/ota>
- Google Pixel 9a specifications/model list: <https://support.google.com/pixelphone/answer/7158570>
- AOSP device repository: <https://android.googlesource.com/device/google/tegu/>
- AOSP Android GS kernel basis: <https://android.googlesource.com/device/google/tegu-kernels/>
- AOSP device kernel tree: <https://android.googlesource.com/kernel/devices/google/tegu/>

No postmarketOS `device-google-tegu` package or complete Ferrix adapter has
been admitted by this change. Do not infer display, input, audio, modem,
Wi-Fi/Bluetooth, sensors, suspend, or Linux boot support from stock Android or
from QEMU.
