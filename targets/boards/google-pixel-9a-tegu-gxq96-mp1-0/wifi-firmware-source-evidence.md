# Pixel 9a BCM4383 Wi-Fi firmware source evidence

Target: `google-pixel-9a-tegu-gxq96-mp1-0` (`tegu`, `GXQ96`, `MP1.0`)

This is a source/provenance record, not physical-device evidence. The
firmware bytes remain outside Git and are accepted only through the exact
factory archive and target-scoped staging workflow.

## Pinned kernel contract

- Source: [`kernel/google-modules/wlan/bcmdhd/bcm4383`](https://android.googlesource.com/kernel/google-modules/wlan/bcmdhd/bcm4383/)
- Revision: `3526a8cb8e13b7a72fca642c7b6411cd69e70d87`
- Kleaf output: [`BUILD.bazel`](https://android.googlesource.com/kernel/google-modules/wlan/bcmdhd/bcm4383/+/3526a8cb8e13b7a72fca642c7b6411cd69e70d87/BUILD.bazel) emits `bcmdhd4383.ko`.
- The pinned [`Kbuild`](https://android.googlesource.com/kernel/google-modules/wlan/bcmdhd/bcm4383/+/3526a8cb8e13b7a72fca642c7b6411cd69e70d87/Kbuild) selects the PCIe standard firmware API and its exact request names are:
- The pinned [`dhd_custom_google.c`](https://android.googlesource.com/kernel/google-modules/wlan/bcmdhd/bcm4383/+/3526a8cb8e13b7a72fca642c7b6411cd69e70d87/dhd_custom_google.c) implements platform-extension candidate names for the board revision/SKU and chip version.

  - `fw_bcmdhd.bin`
  - `bcmdhd.cal`
  - `bcmdhd_clm.blob`
  - `fw_bcmdhd.map`
  - `bcmdhd_txcap.blob`

The standard firmware API requests those basenames through Linux's firmware
class. The Google driver also has platform-extension lookup for calibration,
CLM, and TXCAP data: it constructs candidate names from the chip version and
board revision/SKU before requesting them through the firmware API. The
factory image contains `_4383_a3` variants for all five payloads, while generic
firmware and map files also exist. The staging allowlist preserves both sets
of firmware/map names and preserves each `_4383_a3` runtime name. For the
calibration, CLM, and TXCAP payloads, which have no generic factory member, it
maps the selected A3 source to both the base request name and the
`_4383_a3`-suffixed candidate; both copies retain the same source provenance
and hash in the external manifest. This provides a base-name fallback without
discarding the platform variant.

The separate native-Linux candidate uses `brcmfmac4383-pcie.*` names. Its
explicit radio-first aliases use the A3-suffixed factory variants as sources;
they are byte-preserving copies, not a claim that the Google and mainline
drivers share a firmware ABI. The alias route remains experimental and has
not been exercised on hardware.

## Staging and hash boundary

The checked-in [`firmware-stage-allowlist.tsv`](firmware-stage-allowlist.tsv)
contains only explicit `vendor.img` source/runtime mappings for these files,
the exact Bluetooth patchram blob, and the modem configuration bundles.
`firmware.sh` verifies the owner-selected factory archive against
[`recovery.pins`](recovery.pins), requires an explicit extraction
confirmation, and records byte count and SHA-256 for every staged file in the
ignored `firmware-manifest.tsv`. Firmware hashes are intentionally not checked
in because the binary payloads are proprietary and must remain outside Git.
The rootfs workflow rechecks the external manifest and every file before
accepting it.

The suffix match is a source-level selection rationale, not runtime proof:
`_4383_a3` is present in the pinned factory package and agrees with the
BCM4383 driver family, but Ferrix has not yet booted this module on the exact
MP1.0 device to observe which candidate the driver requests.

No Linux Wi-Fi controller, scan, association, DHCP, or Ferrix adapter result
is claimed here; those require exact-device runtime evidence.
