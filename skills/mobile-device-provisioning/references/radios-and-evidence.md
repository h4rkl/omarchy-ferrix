# Radios and evidence

## Radio subsystems, Linux services, Ferrix adapters

Ferrix never speaks a radio protocol itself. Each subsystem is owned by a
standard Linux service and wrapped by a typed, project-owned adapter. Product
code sees only `ferrix-hardware-api` (coarse) or the richer network and
telephony APIs.

| Subsystem | Kernel side (Pixel 9a `tegu`) | Linux service | Ferrix adapter crate | Product-facing contract | Selected by |
| --- | --- | --- | --- | --- | --- |
| Wi-Fi | Radio-first candidate: Broadcom BCM4383 PCIe with Linux `brcmfmac`; the checked-in `firmware-radio-route.tsv` copies the verified factory `bcmdhd` members to `brcm/brcmfmac4383-pcie.bin`, `.txt`, `.clm_blob`, and `.txcap_blob`. The canonical Google `bcmdhd` contract remains a separate route and must not be mixed. | NetworkManager | `ferrix-network-nm` inside `ferrix-networkd --backend networkmanager` | `ferrix-network-api` / `-protocol` (ADR-002) | `FERRIX_NETWORK_BACKEND=networkmanager` |
| Bluetooth | Broadcom Bluetooth controller with Linux `btbcm`/HCI-UART support and the factory-selected `brcm/BCM.hcd` firmware path; candidate and canonical routes share this reviewed firmware filename but still require route-specific kernel evidence. | BlueZ (`org.bluez.Adapter1`) | `ferrix-bluetooth-bluez` (`--features production`) | `ferrix-hardware-api::Bluetooth` (present/enabled only) | `FERRIX_BLUETOOTH_BACKEND=bluez` |
| Cellular | Radio-first candidate: Samsung S5300 with the pinned `s5300_modem` WWAN/SIT driver, `/dev/umts_boot0` loader path, runtime `wwan0sit0`/`wwan0sit1` and AT ports, and `rmnet*` data devices after CP ONLINE. The Google Kleaf `cpif` route is incompatible. | ModemManager, if and only if the target-local SIT plugin exposes the modem | `ferrix-telephony-mm` inside `ferrix-telephonyd --backend modemmanager`; target-local `exynos-modem`/SIT package recipes provide the external loader/plugin boundary; `ferrix-s5300-cpif` remains a Rust SIPC5 frame codec only | `ferrix-telephony-api` / `-protocol` (ADR-001) | `FERRIX_TELEPHONY_BACKEND=modemmanager` |
| Power | Google BMS modules | UPower (`/org/freedesktop/UPower/devices/DisplayDevice`) | `ferrix-power-upower` | `ferrix-hardware-api::Power` | `FERRIX_POWER_BACKEND=upower` |
| Audio | AoC / amplifiers | PipeWire + WirePlumber (`wpctl`) | `ferrix-audio-wireplumber` | `ferrix-hardware-api::Audio` | `FERRIX_AUDIO_BACKEND=wireplumber` |

Backend selection lives in
`targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/ferrix-backends.env`
and the host profile `dev/profile.env`; composition is in the board's
`adapter/` crate. Apps and `ferrix-core` never see these names.

Evidence label: the guest exports
`FERRIX_EVIDENCE=physical:google-pixel-9a-tegu-gxq96-mp1-0`. QEMU guests
export `QEMU`; the nested host route exports `mock`. The radios workflow
(`tools/pixel-9a/radios.sh`, Stub-tested only) refuses a guest whose device tree
is not the pinned `google,ZUMA PRO TEGU` / `ZUMA PRO TEGU MP on ZUMA PRO`
identity (exit 4) and refuses a QEMU `linux,dummy-virt` guest
outright (exit 5).

## Evidence status vocabulary

Capability status, in `capabilities.toml` and in the radios
`evidence-latest.tsv` `status` column, has exactly two values for a physical
target:

| Status | Meaning | Minimum to claim it |
| --- | --- | --- |
| `unsupported` | No exact-target Linux evidence, or the stack cannot provide the capability, or only part of it works | default for every new target; a bounded result stays here with an `evidence` sentence naming what was observed |
| `supported` | The `ferrix-hardware-api` (or richer API) contract for that capability is met on this exact device through the Ferrix adapter path, reproducibly | Linux service state and the Ferrix adapter record/socket both observed; repeated run; redacted evidence; documented limitations |

Constraints:

- `ferrix-hardware-api::CapabilityStatus` (`crates/ferrix-hardware-api/src/lib.rs`)
  has the variants `Supported`, `Unsupported`, and `Mocked`. There is no
  `Partial`. `mocked` is for QEMU/mock profiles only and never for a physical
  target. The radios workflow's static test enforces that it never prints
  `partial` or `mocked`.
- Nuance lives in the TSV `progress` column and in the `evidence` text, for
  example `device-present-driver-or-firmware-missing`, `radio-disabled`,
  `linux-wifi-up-ferrix-adapter-not-observed`,
  `linux-and-ferrix-adapter-observed`, `controller-unpowered`,
  `no-modemmanager-modem`, `modem-enumerated-only`. The printed
  `capabilities.toml` fragment folds `progress=` into `evidence`.
- The word `partial` belongs only to `manifest.toml` gate fields (for example
  `image_compatibility = "partial"`) and to proposal checklist tables. It is a
  documentation status for a gate, never a capability status.

Do not use "works", "should work", "likely", or "tested" without one of the
words above. A capability status change goes through the owner: the radios
`evidence` command prints a `capabilities.toml` fragment labelled
`physical: google-pixel-9a-tegu-gxq96-mp1-0`; the owner pastes it after
review. Telephony never becomes `supported` from this workflow.

The board's
`targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/s5300-source-evidence.md`
records the immutable external sources used by the target-local S5300 loader
and SIT ModemManager package recipes, while
`targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/grapheneos-radio-correlation.md`
records Android-source corroboration for hardware-facing module names and load
order. The package recipes are a checked-in native-Linux build boundary, but
neither source record is physical evidence. Runtime adoption requires verified
source/archive hashes, licensing/source offer, exact factory carrierconfig and
UECAP directory inputs, and a readiness boundary through public ModemManager
D-Bus; Android RIL/HAL/Binder/SELinux/vendor services and prebuilts remain
excluded.

The host firmware manifest is not runtime evidence. Before any radio service
starts, `ferrix-radio-firmware` validates the exact target/schema, checks every
manifest destination against the reviewed runtime allowlist (including every
descendant under the explicitly reviewed carrierconfig/UECAP directories),
rechecks regular-file size and SHA-256, and verifies the four radio-first
aliases. A passing offline stage or runtime gate only establishes a safe input
boundary; it does not show that a physical controller loaded.

The radio-first split-image composer is likewise device-free implementation
evidence. Even with a complete source-built partition handoff, its manifest
remains `composed-not-booted` / `bootable=false`; only a later exact-target
guest run through the Linux service and Ferrix adapter can provide capability
evidence.

## What counts as radio evidence

| Radio | Evidence rows (all redacted) | Not evidence |
| --- | --- | --- |
| Wi-Fi | driver bound, firmware files present with hashes matching `firmware-manifest.tsv`, `rfkill` unblocked, NetworkManager device state, scan count (not SSIDs), association state, DHCP lease obtained (address family only), DNS resolution, traffic bytes; `ferrix-networkd` `networkd-ready` record | a QEMU `set-radio` scenario; `dmesg` alone; stock Android connectivity |
| Bluetooth | adapter object present, `Powered=true` after `ferrix-bluetooth-bluez` enable, re-read state; optionally discovery count | pairing or audio unless separately evidenced |
| Modem | radio-first `s5300_modem` bound, CP ONLINE transition, `/dev/umts_boot0` loader, `wwan0sit0`/`wwan0sit1` and AT WWAN ports, `rmnet*` presence, verified carrierconfig/UECAP rows, ModemManager `mmcli -L`/D-Bus readiness, SIM state, registration, and bearer; the legacy Google CPIF surface is recorded separately as an ABI mismatch | any claim of calls, SMS, VoLTE, or emergency calling; modem enumeration alone is not phone support |
| Power | UPower percentage and charging state; a Ferrix `Power` change on plug/unplug | QEMU unplugged-to-charging |
| Audio | WirePlumber sinks; default sink change via `wpctl set-default` reflected in Ferrix state | ALSA control dumps |

## Redaction rules

Never commit, paste into a repo file, or leave in a checked-in log:

- serial numbers (transcripts keep a 12-character SHA-256 prefix only),
  IMEI, IMSI, ICCID, MSISDN, EID;
- MAC and BSSID addresses, SSIDs, PSKs, Wi-Fi profiles, IP addresses beyond
  the address family, DHCP leases, hostnames from the network;
- Bluetooth device addresses and names of nearby devices;
- keys, certificates, `adbkey`, AVB keys, unlock tokens;
- vendor firmware bytes, calibration data, partition dumps, backups, images;
- raw `dmesg`, `logcat`, `journalctl`, or console logs (they contain the
  above). Extract the needed line, redact, and cite the ignored source path.

Generated files stay under ignored `out/pixel-9a/` and the ignored target
subfolders in `.gitignore`. Before staging, grep the diff for `serial`,
`imei`, `imsi`, `iccid`, colon-separated MAC patterns, and any SSID you saw.

## Known Pixel 9a limitations (as of 2026-09-28)

- Nothing has booted Linux on the device; all nine capabilities are
  `unsupported` in `capabilities.toml`.
- Cellular: the Google Kleaf S5300 CPIF surface (`cpif` and
  `/dev/umts_ipc*`) is not the WWAN SIT ABI consumed by the reviewed
  `exynos-modem`/SIT ModemManager packages. `ferrix-telephony-mm` cannot
  provide phone service until the radio-first `s5300_modem` kernel backend,
  target-local package, exact factory `/firmware/carrierconfig` and
  `/firmware/uecapconfig` inputs, and plugin are built together and expose a
  modem through the WWAN/SIT ports; those package inputs are not physical
  evidence. The checked-in `radios.sh` workflow must observe that route's
  WWAN/SIT readiness before any telephony capability can change.
  `ferrix-s5300-cpif` stops at the SIPC5 wire-frame boundary and claims no
  modem, SMS, data, voice, IMS, or call audio. Emergency calling is rejected
  as `unsupported{feature: emergency}` everywhere and must stay that way.
- Wi-Fi/Bluetooth: the firmware policy names the BCM4383 files and the
  radio-first route records explicit `bcmdhd`→`brcmfmac4383-pcie.*` aliases,
  but no Linux load, association, controller power, or BlueZ evidence has been
  observed; the stock `modem.img` is a raw radio payload the firmware stager
  refuses to treat as a filesystem.
- Audio: `wpctl set-default` cannot select ports on a single sink; mobile UCM
  layouts that expose earpiece and speaker as ports will report fewer routes.
- Power: the adapter keeps the last authoritative reading if UPower
  disappears; there is no "unavailable" state in the portable contract.
- Bluetooth: the adapter models presence and power only; no discovery,
  pairing, profiles, or audio routing.
- Kernel/module coherence: stock Android modules must never be mixed with
  the Ferrix-built kernel. The Google Kleaf and radio-first kernel backends
  have different CPIF and BCM4383 firmware contracts; module, DTB/DTBO, and
  firmware evidence is valid only for the one pinned backend recorded in
  `build-manifest.tsv`.
