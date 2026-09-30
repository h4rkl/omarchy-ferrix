# Generic device front door (`tools/device/`)

Vendor-agnostic, read-only tooling for any Android phone attached over ADB or
Fastboot. It identifies the device, matches it against the checked-in board
targets under `targets/boards/*/`, and, for an unknown device, scaffolds a
board-support proposal plus target skeleton for owner review. Nothing here
mutates a device.

## What is here (generic)

| Command | What it does | Device contact |
| --- | --- | --- |
| `just device-discover` | Enumerates ADB/Fastboot, refuses zero (exit 2), multiple (exit 3), or unauthorized (exit 4) devices, reads a broad property set, prints `key=value` to the operator, and writes a serial-redacted transcript plus `identity-latest.env` under `out/device/`. | read-only: `adb devices -l`, `adb -s S shell getprop P`, `fastboot devices -l`, `fastboot -s S getvar V` |
| `just device-match` | Compares `identity-latest.env` against every `kind = "physical"` manifest. Exact match needs manufacturer (case-insensitive), codename, and hardware revision, plus SKU when both sides have one. The Pixel 9a Fastbootd variant placeholder needs matching-serial Android proof of its SKU. Exit 0 exact, 10 none (with partial candidates and an explicit do-not-reuse note for other revisions), 1 refusal. | none |
| `just device-propose` | For an unmatched identity, creates `targets/boards/<id>/{manifest.toml,capabilities.toml,recovery.pins,README.md}` and `docs/board-support/<codename>-attached.md` with every AGENTS.md-required section marked `pending`, and prints the `.gitignore` block to add. Refuses if the target or proposal exists or an exact target already matches. Carries only `serial_sha256_prefix`, never the serial. | none |
| `just device-recovery-download <target_id> [kind]` | Downloads the files pinned in `targets/boards/<id>/recovery.pins` (TSV: kind, build, version, filename, url, sha256, bytes, source-page) into the ignored `recovery/` folder, resumably, verifying byte size and SHA-256 and deleting mismatches. Requires `FERRIX_RECOVERY_DOWNLOAD=I-understand-this-downloads-vendor-firmware`. Never extracts. | none |
| `just device-tools-test` | Offline tests with strict `adb`/`fastboot` stubs (any verb outside the allow-list exits 99), a synthetic non-Google fixture, same-codename/different-revision proof, serial-leak grep, and a `file://` download fixture. | none |

Direct invocation, when `just` is not in use:

```sh
tools/device/discover.sh
tools/device/match-target.sh [out/device/identity-latest.env]
tools/device/propose-board.sh [out/device/identity-latest.env]
FERRIX_RECOVERY_DOWNLOAD=I-understand-this-downloads-vendor-firmware \
  tools/device/recovery-download.sh --target google-pixel-9a-tegu-gxq96-mp1-0 --kind all
sh tools/device/tests/discover-read-only.sh
sh tools/device/tests/match-target.sh
sh tools/device/tests/propose-board.sh
sh tools/device/tests/recovery-download.sh
```

Environment overrides: `FERRIX_OUT_DIR` (default `out/`), `FERRIX_BOARDS_DIR`
(default `targets/boards`), `FERRIX_DOCS_DIR` (default `docs/board-support`),
`FERRIX_DEVICE_MANUFACTURER` (operator hint for Fastboot-only discovery, which
cannot observe the manufacturer; recorded as `manufacturer_source='operator'`).

## Identity file

`out/device/identity-latest.env` is sh-sourceable (`key='value'`, quotes
escaped, newline-bearing values rejected) and redacted: it has no `serial`
key, only `serial_sha256_prefix`. Normalized keys: `transport, manufacturer,
model, codename, board, hardware_revision, sku, bootloader_version,
bootloader_state (locked|unlocked|unknown), verified_boot, current_slot,
slot_count, dynamic_partitions, android_release, fingerprint`, plus
`discovered_utc`, `discovery_workflow`, and `serial_sha256_prefix`. Everything
under `out/device/` is generated and ignored; never commit it. Fastboot
discovery also records `fastboot_userspace`; its `variant` is only a transport
value until the target's identity rules establish that it is a SKU.

## What is deliberately NOT here

- Any device mutation: reboot, bootloader unlock, erase, format, slot change,
  flash, `fastboot boot`, `adb sideload`, or OEM commands. Those exist only in
  checked-in, target-scoped workflows (for example `tools/pixel-9a/`) that
  print the exact device identity, partition target, image hash, and recovery
  artifact and require per-run confirmation, after the target's proposal has
  owner approval and its gates pass.
- Archive extraction, firmware handling, partition dumps, backups, or any
  claim of boot or hardware support. A scaffolded target is a proposal with
  `status = "proposed-identity-only"` and every capability `unsupported`.
- Generalization: a same-codename device with a different hardware revision
  is `match=none`; target evidence is never reused across revisions, models,
  QEMU, or the mock profile.
