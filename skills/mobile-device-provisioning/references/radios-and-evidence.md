# Radios and evidence

## Radio subsystems, Linux services, Ferrix adapters

Ferrix never speaks a radio protocol itself. Each subsystem is owned by a
standard Linux service and wrapped by a typed, project-owned adapter. Product
code sees only `ferrix-hardware-api` (coarse) or the richer network and
telephony APIs.

| Subsystem | Kernel side (Pixel 9a `tegu`) | Linux service | Ferrix adapter crate | Product-facing contract | Selected by |
| --- | --- | --- | --- | --- | --- |
| Wi-Fi | Broadcom BCM4383, `bcmdhd` driver, firmware `fw_bcmdhd.bin`, `bcmdhd.cal`, `bcmdhd_clm.blob`, `fw_bcmdhd.map`, `bcmdhd_txcap.blob` (`firmware-policy.toml`) | NetworkManager | `ferrix-network-nm` inside `ferrix-networkd --backend networkmanager` | `ferrix-network-api` / `-protocol` (ADR-002) | `FERRIX_NETWORK_BACKEND=networkmanager` |
| Bluetooth | Broadcom Bluetooth module (pinned `bluetooth-broadcom` source) | BlueZ (`org.bluez.Adapter1`) | `ferrix-bluetooth-bluez` (`--features production`) | `ferrix-hardware-api::Bluetooth` (present/enabled only) | `FERRIX_BLUETOOTH_BACKEND=bluez` |
| Cellular | Samsung S5300 CPIF (pinned `modem-s5300` source); SIPC5 link frames | ModemManager, if and only if it can expose the modem | `ferrix-telephony-mm` inside `ferrix-telephonyd --backend modemmanager`; `ferrix-s5300-cpif` is a Rust SIPC5 frame codec only | `ferrix-telephony-api` / `-protocol` (ADR-001) | `FERRIX_TELEPHONY_BACKEND=modemmanager` |
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
is not `google,tegu` (exit 4) and refuses a QEMU `linux,dummy-virt` guest
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

## What counts as radio evidence

| Radio | Evidence rows (all redacted) | Not evidence |
| --- | --- | --- |
| Wi-Fi | driver bound, firmware files present with hashes matching `firmware-manifest.tsv`, `rfkill` unblocked, NetworkManager device state, scan count (not SSIDs), association state, DHCP lease obtained (address family only), DNS resolution, traffic bytes; `ferrix-networkd` `networkd-ready` record | a QEMU `set-radio` scenario; `dmesg` alone; stock Android connectivity |
| Bluetooth | adapter object present, `Powered=true` after `ferrix-bluetooth-bluez` enable, re-read state; optionally discovery count | pairing or audio unless separately evidenced |
| Modem | module loaded, CPIF device nodes present; ModemManager `mmcli -L` result; SIM state; registration; bearer | any claim of calls, SMS, VoLTE, or emergency calling; modem enumeration alone is not phone support |
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
- Cellular: the S5300 CPIF is not a QMI/MBIM/AT modem that ModemManager
  supports out of the box. `ferrix-telephony-mm` cannot provide phone service
  until a target-specific CPIF backend or ModemManager plugin exists.
  `ferrix-s5300-cpif` stops at the SIPC5 wire-frame boundary and claims no
  modem, SMS, data, voice, IMS, or call audio. Emergency calling is rejected
  as `unsupported{feature: emergency}` everywhere and must stay that way.
- Wi-Fi/Bluetooth: the firmware policy names the BCM4383 files but no
  Linux load or association has been observed; the stock `modem.img` is a raw
  radio payload the firmware stager refuses to treat as a filesystem.
- Audio: `wpctl set-default` cannot select ports on a single sink; mobile UCM
  layouts that expose earpiece and speaker as ports will report fewer routes.
- Power: the adapter keeps the last authoritative reading if UPower
  disappears; there is no "unavailable" state in the portable contract.
- Bluetooth: the adapter models presence and power only; no discovery,
  pairing, profiles, or audio routing.
- Kernel/module coherence: stock Android modules must never be mixed with
  the Ferrix-built kernel; module and DTBO evidence is valid only for the
  pinned Kleaf build recorded in `build-manifest.tsv`.
