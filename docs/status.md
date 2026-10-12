# Status

This file is the current-state summary. The dated evidence log, newest first,
lives in [`docs/status-log/`](status-log/), one file per month. Add a new
dated entry at the top of the current month's `docs/status-log/YYYY-MM.md`
(create the file if the month is new). Update the "Current state" tables
below only when a target's capability matrix changes, together with the
matrix itself.

## Current state

One table per target. **Status** is the target's capability matrix value,
copied verbatim; **Evidence** (Pixel 9a) is the matrix `evidence` string,
copied verbatim. `tools/dev/tests/status-current-state.py` fails if either
drifts from the matrix. **Evidence label** and **Latest evidence** describe
the newest dated log entry that bears on that capability. They are not a
promotion: only the matrix decides the status. **Owning issues** are open
GitHub issues for the remaining work.

### `qemu-aarch64`

Generic AArch64 QEMU `virt` with the versioned mock profile
(`targets/qemu-aarch64/profile.toml`). There is no `capabilities.toml`; the
matrix is `CapabilityMatrix::uniform(CapabilityStatus::Mocked)` in
`ferrix-hardware-emulator` (`Emulator::capability_matrix`). Nothing
here is physical evidence.

| Capability | Status | Evidence label | Latest evidence | Owning issues |
| --- | --- | --- | --- | --- |
| `power` | `mocked` | QEMU/mock | [2026-10-10](status-log/2026-10.md#2026-10-10-bluetooth-legacy-pin-incoming-pairing-raise-live-scan-countdown-qemumock-only) (`unplugged-to-charging` in `just qemu-check`) | — |
| `display` | `mocked` | QEMU | [2026-10-10](status-log/2026-10.md#2026-10-10-icon-set-filled-remix-icon-v470-glyphs-issue-20-qemuhost-preview-only) | #12, #14, #16, #20 |
| `input` | `mocked` | QEMU | [2026-10-10](status-log/2026-10.md#2026-10-10-terminal-and-shared-keyboard-key-repeat-selection-and-clipboard-scrollbar-bell-and-title-non-us-layouts-qemu-only) | #12, #21 |
| `network` | `mocked` | QEMU/mock | [2026-10-09](status-log/2026-10.md#2026-10-09-settings-pages-mic-mute-app-intents-incoming-call-raise-qemumock-only) | #7 |
| `bluetooth` | `mocked` | QEMU/mock | [2026-10-10](status-log/2026-10.md#2026-10-10-bluetooth-legacy-pin-incoming-pairing-raise-live-scan-countdown-qemumock-only) | — |
| `audio` | `mocked` | mock | [2026-10-12](status-log/2026-10.md#2026-10-12-typed-output-volumemute-and-settings-sound-host-tests-and-mock-previews-only) | — |
| `telephony` | `mocked` | mock | [2026-10-12](status-log/2026-10.md#2026-10-12-telephony-subscribe-is-a-gap-free-snapshot-plus-stream-issue-68-host-tests-mock-only) | — |
| `lifecycle` | `mocked` | — | — (no dedicated entry) | — |
| `mock-control` | `mocked` | QEMU | [2026-10-10](status-log/2026-10.md#2026-10-10-qemu-harness-flakes-cursor-ready-start-serial-interleaving-socket-path-limit-record-identity-daemon-kill-qemu-only) | — |

### `google-pixel-9a-tegu-gxq96-mp1-0`

Matrix: [`targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/capabilities.toml`](../targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/capabilities.toml).
Evidence labelled `physical:` applies to this exact target only.

| Capability | Status | Evidence | Evidence label | Latest evidence | Owning issues |
| --- | --- | --- | --- | --- | --- |
| `power` | `supported` | UPower battery and charger state through ferrix-power-upower reached ferrix-core and the home surface. 2026-10-09 Kleaf dev route (slot b, images composed by tools/pixel-9a/dev-compose.py); see docs/status.md. | physical:google-pixel-9a-tegu-gxq96-mp1-0 | [2026-10-09](status-log/2026-10.md#2026-10-09-pixel-9a-product-reaches-home-unattended-under-required-isolation-physical-google-pixel-9a-tegu-gxq96-mp1-0) | #26 |
| `display` | `supported` | ferrix-shell scans out 1080x2424 portrait on the DSI panel with profile byte order and safe area; scanout read back via DRM GETFB2 and checked on camera. 2026-10-09 Kleaf dev route (slot b, images composed by tools/pixel-9a/dev-compose.py); see docs/status.md. | physical:google-pixel-9a-tegu-gxq96-mp1-0 | [2026-10-12](status-log/2026-10.md#2026-10-12-fresh-pixel-9a-development-boot-and-read-only-inventories-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0) | #4, #19 |
| `input` | `supported` | synaptics_tcm_touch and uinput test touch/keyboard devices reach ferrix-shell through libinput; taps launch apps and drive the on-screen keyboard. 2026-10-09 Kleaf dev route (slot b, images composed by tools/pixel-9a/dev-compose.py); see docs/status.md. | physical:google-pixel-9a-tegu-gxq96-mp1-0 | [2026-10-09](status-log/2026-10.md#2026-10-09-pixel-9a-product-reaches-home-unattended-under-required-isolation-physical-google-pixel-9a-tegu-gxq96-mp1-0) | — |
| `network` | `supported` | bcm4383 Wi-Fi scans and joins a WPA2/WPA3 transition AP with WPA3-SAE (driver patched for NL80211 WPA version 3) and WPA2-PSK through NetworkManager and ferrix-networkd (DHCP, ping). 2026-10-09 Kleaf dev route (slot b, images composed by tools/pixel-9a/dev-compose.py); see docs/status.md. | physical:google-pixel-9a-tegu-gxq96-mp1-0 | [2026-10-09](status-log/2026-10.md#2026-10-09-pixel-9a-product-reaches-home-unattended-under-required-isolation-physical-google-pixel-9a-tegu-gxq96-mp1-0) | #7, #8, #24 |
| `bluetooth` | `supported` | ferrix-bluetooth-uart attaches the BCM4383 controller (btattach -P bcm on ttySAC18, brcm/BCM.hcd); ferrix-core reports present/enabled through ferrix-bluetooth-bluez and BlueZ scans find devices. Pairing UI discovery verified; pairing not exercised on hardware; link runs at 115200 baud. 2026-10-09 Kleaf dev route; see docs/status.md. | physical:google-pixel-9a-tegu-gxq96-mp1-0 | [2026-10-12](status-log/2026-10.md#2026-10-12-a2dp-active-proof-play-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0) (A2DP) | #5 |
| `audio` | `unsupported` | No Ferrix adapter or physical Linux evidence. | physical:google-pixel-9a-tegu-gxq96-mp1-0 (partial, no promotion) | [2026-10-12](status-log/2026-10.md#2026-10-12-a2dp-active-proof-play-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0) (A2DP) | #2, #94 |
| `telephony` | `unsupported` | No Ferrix adapter or physical Linux evidence. | physical:google-pixel-9a-tegu-gxq96-mp1-0 (S5300 staging, no promotion) | [2026-10-12](status-log/2026-10.md#2026-10-12-1-r5-image-s5300-s2-pre-mount-go-with-opener-positive-control-and-unitboot-identity-issues-107-and-108-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0) | #1, #27, #76, #92, #109, #110, #116, #117 |
| `lifecycle` | `unsupported` | No Ferrix adapter or physical Linux evidence. | — | — | #9 |
| `mock-control` | `unsupported` | Physical target; no mock control is claimed. | — | — | — |

## Recent

The ten newest log entries, newest first:

1. [2026-10-12: 1-r5 image, S5300 S2 pre-mount GO with opener positive control and unit/boot identity](status-log/2026-10.md#2026-10-12-1-r5-image-s5300-s2-pre-mount-go-with-opener-positive-control-and-unitboot-identity-issues-107-and-108-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0)
2. [2026-10-12: device-google-tegu 1-r5: /mnt/vendor via install script](status-log/2026-10.md#2026-10-12-device-google-tegu-1-r5-mntvendor-via-install-script-builder-evidence-no-physical-claim)
3. [2026-10-12: A2DP active proof play](status-log/2026-10.md#2026-10-12-a2dp-active-proof-play-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0)
4. [2026-10-12: S5300 S2 active CP boot](status-log/2026-10.md#2026-10-12-s5300-s2-active-cp-boot-issue-92-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0)
5. [2026-10-12: S5300 S2 decision binding](status-log/2026-10.md#2026-10-12-s5300-s2-decision-binding-issue-108-offline-only-no-physical-claim)
6. [2026-10-12: S5300 CPIF opener-scan positive control](status-log/2026-10.md#2026-10-12-s5300-cpif-opener-scan-positive-control-issue-107-offline-only)
7. [2026-10-12: 1-r3 image and S5300 S2 pre-mount GO](status-log/2026-10.md#2026-10-12-1-r3-image-and-s5300-s2-pre-mount-go-issue-92-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0)
8. [2026-10-12: S5300 S2 read-only preflight](status-log/2026-10.md#2026-10-12-s5300-s2-read-only-preflight-issue-92-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0)
9. [2026-10-12: A2DP active proof dry runs](status-log/2026-10.md#2026-10-12-a2dp-active-proof-dry-runs-physicalgoogle-pixel-9a-tegu-gxq96-mp1-0-dry-only)
10. [2026-10-12: A2DP active proof workflow](status-log/2026-10.md#2026-10-12-a2dp-active-proof-workflow-mock-google-pixel-9a-tegu-gxq96-mp1-0)

## Log

- [`docs/status-log/2026-10.md`](status-log/2026-10.md)
- [`docs/status-log/2026-09.md`](status-log/2026-09.md)

Entries are newest first within each month. They were moved byte-for-byte
from this file in #119, so earlier links to `docs/status.md#<entry>` resolve
to the same anchor in the month file.
