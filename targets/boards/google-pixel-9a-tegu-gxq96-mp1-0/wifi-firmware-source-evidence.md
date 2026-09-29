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

  - `fw_bcmdhd.bin`
  - `bcmdhd.cal`
  - `bcmdhd_clm.blob`
  - `fw_bcmdhd.map`
  - `bcmdhd_txcap.blob`

The standard firmware API requests those basenames through Linux's firmware
class. The checked-in target kernel command line sets
`firmware_class.path=/lib/firmware`; the staging allowlist therefore maps the
factory paths under `vendor.img:/firmware/` to
`/lib/firmware/<same-basename>` without renaming or mixing the unrelated
`brcmfmac4383-pcie.*` contract from the unadopted native-Linux candidate.

## Staging and hash boundary

The checked-in [`firmware-stage-allowlist.tsv`](firmware-stage-allowlist.tsv)
contains exactly these five `vendor.img` members. `firmware.sh` verifies the
owner-selected factory archive against [`recovery.pins`](recovery.pins),
requires an explicit extraction confirmation, and records byte count and
SHA-256 for every staged file in the ignored `firmware-manifest.tsv`.
Firmware hashes are intentionally not checked in because the binary payloads
are proprietary and must remain outside Git. The rootfs workflow rechecks the
external manifest and every file before accepting it.

No Linux Wi-Fi controller, scan, association, DHCP, or Ferrix adapter result
is claimed here; those require exact-device runtime evidence.
