# Pixel 9a Broadcom Bluetooth firmware source evidence

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu`, `GXQ96`, `MP1.0`)

This is a source/provenance record, not hardware evidence. It does not claim
that Ferrix Linux has booted on the phone or that Bluetooth currently works.
The binary firmware remains outside Git.

## Finding

The exact selected Broadcom HCD blob is available from Google's official Pixel
factory package, but it is not published as source in AOSP, the Google Nitrous
kernel module tree, or official postmarketOS pmaports:

| Item | Evidence |
| --- | --- |
| Selected filename | `BCM.hcd` |
| Factory image member | `vendor.img` |
| Path inside `vendor.img` | `/firmware/brcm/BCM.hcd` |
| Android mounted path | `/vendor/firmware/brcm/BCM.hcd` |
| Linux firmware-class destination | `/lib/firmware/brcm/BCM.hcd` |
| Uncompressed bytes | `628097` |
| SHA-256 | `14f1aae62d5f961aa9cbaf3873d7504b6b387c0f53aac784815e30b9e57c5ea6` |
| Selection evidence | `vendor.img:/etc/bluetooth/firmware_config.json` sets `firmware_folder_name` to `vendor/firmware/brcm/` and `firmware_file_name` to `BCM.hcd` for `chip_id` `123` |

The hash above was computed from the exact file extracted offline from the
verified factory archive below. It is metadata only; the HCD bytes are not
committed.

## Exact Google factory provenance

The checked-in recovery pin selects Google's official Pixel 9a factory package:

- Build: `CP3A.260905.009` / Android `17.0.0`
- Download: [`tegu-cp3a.260905.009-factory-0c2f9053.zip`](https://dl.google.com/dl/android/aosp/tegu-cp3a.260905.009-factory-0c2f9053.zip)
- Official source page: [Google Pixel factory images](https://developers.google.com/android/images)
- Archive SHA-256: `0c2f9053bf064b09483c937cf16f4824371afe92fa057029d6f9237668d1c0d7`
- Archive size: `4227813715` bytes
- Nested image archive: `tegu-cp3a.260905.009/image-tegu-cp3a.260905.009.zip`
- Nested image archive size: `4173927063` bytes
- `vendor.img` uncompressed size: `888745984` bytes
- `vendor.img` SHA-256: `41c5d71776d28d36a903b4b6cb696d374aaed9f5fd00a5cb2a2f03181dc29bf7`

The archive and its compatibility metadata were verified by the offline
`just pixel-9a-factory-inspect` workflow. No device-facing command was used.

The same vendor image contains two additional HCD files. They are recorded to
prevent a later filename guess, but neither is selected by the exact factory
configuration:

| Status | Path inside `vendor.img` | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| present, not selected | `/firmware/brcm/BTFW_B.hcd` | `619033` | `505985e32635e24972340a1b1d0f0c9040744944860ea70c4fe1d3e53e5d3a3b` |
| present, not selected | `/firmware/brcm/BTFW_D.hcd` | `451225` | `c56593c4be449ff70f4e2578eafbed55e014d91c44f78e4080cec3be0c771c36` |

## Google source correlation

The exact pinned Google zuma board source includes the following evidence:

- [`zuma-board-bluetooth.dtsi` at `42395e8a`](https://android.googlesource.com/kernel/devices/google/zuma/+/42395e8adcb880aa336f0da6bb2a19ed53929c40/dts/zuma-board-bluetooth.dtsi) enables `serial_18` and defines a `btbcm` node with `compatible = "goog,nitrous"`.
- The same node wires the Broadcom controller's shutdown, host-wakeup and device-wakeup GPIOs and the Bluetooth low-power DBO GPIO. This is the power/UART side of the path; it does not contain firmware bytes or a firmware filename.
- [`nitrous.c` at `d7bd4dd2`](https://android.googlesource.com/kernel/google-modules/bluetooth/broadcom/+/d7bd4dd23cc52fedd2e36c6e45c12f0c2a595ae/nitrous.c) and its [`nitrous.txt` binding](https://android.googlesource.com/kernel/google-modules/bluetooth/broadcom/+/d7bd4dd23cc52fedd2e36c6e45c12f0c2a595ae/nitrous.txt) describe GPIO/rfkill/UART power management. The pinned module tree contains no HCD blob and no firmware-loading call; Nitrous is not the provenance of `BCM.hcd`.
- The same pinned tree's [`BUILD.bazel`](https://android.googlesource.com/kernel/google-modules/bluetooth/broadcom/+/d7bd4dd23cc52fedd2e36c6e45c12f0c2a595ae/BUILD.bazel) emits `nitrous.ko`. The target firmware policy records the exact module basename (`nitrous`) separately from the broader Broadcom Bluetooth source family.
- Google's generic Linux Broadcom HCI implementation documents the firmware-class naming/fallback behavior in [`btbcm.c`](https://android.googlesource.com/kernel/common/+/fb4ff0f96de37/drivers/bluetooth/btbcm.c), including `brcm/BCM.hcd`. That is the Linux-side name compatible with the factory-selected blob; it is not evidence that the target has booted.

The AOSP target tree corroborates the board integration but does not publish the
proprietary HCD:

- [`device-tegu.mk` at Android 15.0.0_r25](https://android.googlesource.com/device/google/tegu/+/refs/tags/android-15.0.0_r25/device-tegu.mk) includes the common Broadcom Bluetooth product integration and copies the tegu overlay/configuration files.
- The tegu [`bt_vendor_overlay.conf`](https://android.googlesource.com/device/google/tegu/+/refs/tags/android-15.0.0_r25/bluetooth/bt_vendor_overlay.conf) selects `/dev/ttySAC18`, matching the zuma DT. It does not contain the HCD.
- The common [`bcmbt/bluetooth.mk`](https://android.googlesource.com/device/google/gs-common/+/49e609ba234d9b2a723e455e5ba64b83c376cb99/bcmbt/bluetooth.mk) selects the Broadcom Bluetooth service, while the proprietary vendor payload is delivered separately in Google's factory/vendor image.

## postmarketOS result

Official postmarketOS pmaports `main` at
[`fb0bfa97aa63e44b9813a35244fca41207ed70cf`](https://gitlab.postmarketos.org/postmarketOS/pmaports/-/tree/fb0bfa97aa63e44b9813a35244fca41207ed70cf)
has no `device-google-tegu`/Pixel 9a port and no `BCM.hcd` or `BTFW_*` source
or firmware package for this target. Existing Broadcom firmware packages for
other devices are not interchangeable evidence and are not reused here.

## What remains open

The exact binary provenance is no longer a filename/hash gap. The remaining
gaps are integration and validation:

1. Keep the proprietary `BCM.hcd` outside Git and stage only the exact,
   reviewed file through the target's firmware workflow.
2. Keep the two kernel ownership models separate. The canonical Google route
   uses `nitrous.ko` for GPIO/rfkill/UART power and a generic HCI transport; the
   selected radio-first candidate instead wires `serial_18` and the BCM4383
   power/wake/ BLE-DBO GPIOs into its Tegu `hci_bcm` path and must not load
   `nitrous.ko` alongside it. In either route, the HCI transport is not a
   substitute for the factory patchram.
3. Verify on the exact physical `tegu`/`GXQ96`/`MP1.0` device that the selected
   kernel path loads `brcm/BCM.hcd`, registers an HCI controller, and passes
   BlueZ/Ferrix adapter evidence. Until then Bluetooth remains `unsupported`.

No speculative filename, path, variant, or firmware hash was added. The
target-level and target-scoped staging policies now record only the
factory-selected `BCM.hcd`; the runtime bytes remain external and are still
rechecked by the rootfs manifest workflow.
