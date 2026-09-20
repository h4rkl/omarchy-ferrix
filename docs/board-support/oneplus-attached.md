# Attached OnePlus board-support proposal

Status: discovery blocked — host tools installed, no USB transport enumerated

Owner authorization: read-only device discovery and proposal work approved on
2026-09-20. Destructive provisioning is gated by this proposal.

## Identity gate

- Manufacturer: OnePlus (owner supplied)
- Marketing model: pending read-only discovery
- Manufacturer model number: pending
- Device codename/product: pending
- Hardware revision/SKU: pending
- SoC: pending
- A/B and dynamic-partition layout: pending
- Bootloader state and current slot: pending

No `targets/boards/<target-id>/` directory may be created until these values
identify one stable hardware target. Unique serial numbers stay out of Git.

## Host transport

- Host: macOS arm64
- Android Platform Tools: 37.0.1 installed with Homebrew
- `adb`: installed at `/opt/homebrew/bin/adb`; server starts successfully
- `fastboot`: installed at `/opt/homebrew/bin/fastboot`
- Discovery result on 2026-09-20: no ADB device, Fastboot device, or matching
  macOS USB registry entry was visible

The next discovery attempt is read-only. In Android, enable Developer options
and USB debugging, unlock the screen, select a data-capable USB mode, and accept
the host RSA prompt. Alternatively, place the phone in its bootloader/fastboot
mode. Then run `just oneplus-discover`.

## Required implementation evidence

- Upstream or maintained postmarketOS/device-package basis for the exact target
- Kernel, device tree, boot image format, firmware, and driver sources
- Complete partition inventory and slot semantics obtained without mutation
- Stock firmware source plus verified hashes and a tested restore procedure
- Backup inventory stored outside the repository and verified readable
- Non-destructive boot route where supported; otherwise a documented rationale
- Capability matrix for every `ferrix-hardware-api` capability
- Boot, display, touch, storage, charging, networking, audio, modem, sensors,
  suspend, and recovery evidence, each labelled with exact target and revision

## Provisioning gate

No unlock, erase, format, slot change, boot, or flash command may run until all
identity and implementation fields above are complete and reviewed. The
provisioning workflow must bind to one explicit serial, verify product and
revision again immediately before mutation, print partition names and image
hashes, verify recovery artifacts, and require a typed per-run confirmation.
