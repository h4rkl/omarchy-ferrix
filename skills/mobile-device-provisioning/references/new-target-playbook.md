# New target playbook

How an unknown phone becomes an approved board target. Every step before
"owner approval" is read-only on the device. Nothing here authorizes
mutation. Model the result on the admitted Pixel 9a package under
`targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/` and its proposal
`docs/board-support/pixel-9a-attached.md`; the pending OnePlus proposal
`docs/board-support/oneplus-attached.md` shows the empty-table starting
state.

## 1. Discover (read-only)

- `just device-discover` (or `tools/device/discover.sh`). Record from
  `out/device/identity-latest.env`: manufacturer, model, codename/product,
  board, hardware revision, SKU, bootloader version and state, verified boot
  state, slot suffix and count, dynamic partitions, Android release and
  fingerprint. Over Fastboot the manufacturer comes from the human
  (`FERRIX_DEVICE_MANUFACTURER`) and is recorded as operator-supplied.
- `just device-match` must say `match=none`. If it says the codename matches
  a checked-in target with a different hardware revision, this is still a new
  target: create a new id and do not reuse the other revision's evidence.
- Do not create `targets/boards/<id>/` from a marketing name. The id is
  `<manufacturer>-<marketing-or-model>-<codename>-<model-number>-<revision>`
  in lowercase, e.g. `google-pixel-9a-tegu-gxq96-mp1-0`, built only from
  observed values.

## 2. Scaffold the proposal

`just device-propose` (`tools/device/propose-board.sh [identity.env]`,
Stub-tested only) refuses if a checked-in target already matches the identity
or the paths exist, then creates:

- `targets/boards/<id>/manifest.toml` - `schema_version`, `target_id`,
  `kind = "physical"`, `status`, identity fields, `discovered_utc`,
  `discovery_transport`, `discovery_workflow`, stock build/bootloader/slot
  fields, `[linux]` (substrate `postmarketos`, statuses `not-built`,
  `not-attempted`), `[gates]` (`identity`, `image_compatibility`, `backup`,
  `bootloader_unlock`, `recovery_procedure`, `per_run_confirmation =
  "required"`). Fill only what discovery observed; leave the rest `pending`.
- `targets/boards/<id>/capabilities.toml` - every `ferrix-hardware-api`
  capability (`power`, `display`, `input`, `network`, `bluetooth`, `audio`,
  `telephony`, `lifecycle`, `mock-control`) with `status = "unsupported"` and
  an `evidence` sentence saying no Linux evidence exists.
- `targets/boards/<id>/recovery.pins` - header row only until Section 3.
- `targets/boards/<id>/README.md` - identity table, "not a claim that Ferrix
  boots" sentence, source links, support boundary.
- `docs/board-support/<codename>-attached.md` - owner authorization
  paragraph, exact-target checklist table (Field / Value / Source /
  Status), recovery and firmware policy, evidence plan, discovery log.
- Printed `.gitignore` lines. Add them: `/out/<target-short>/`,
  `/targets/boards/<id>/{recovery,backups,evidence,identifiers,keys,partition-dumps,firmware}/`
  and the `**/*.img|zip|bin|raw|dump|log|serial|imei|imsi|iccid|mac|adbkey|key|pem|p12|jks`
  patterns exactly as the Pixel 9a block in `.gitignore`.

Then STOP. Report the scaffold and the `.gitignore` diff. The owner must
approve the proposal in writing (recorded in the proposal's authorization
paragraph with a date) before any download token, unlock, or further tooling.

## 3. Official recovery images and pins

Pins format (TSV, one row per kind):

```text
# kind<TAB>build<TAB>version<TAB>filename<TAB>url<TAB>sha256<TAB>bytes<TAB>source-page
```

Rules:

- Official vendor sources only. Google: factory images at
  <https://developers.google.com/android/images>, full OTA at
  <https://developers.google.com/android/ota>; download URLs are under
  `https://dl.google.com/dl/android/aosp/` and the Pixel workflow refuses
  anything else. The page lists the SHA-256; record it from the page.
- Other vendors (OnePlus/OPPO, Samsung, Fairphone, Sony, Motorola, ...): the
  owner supplies the official URL and hash from the vendor's own support or
  developer site. Never use third-party mirrors, forum reuploads, or
  "unbrick tool" bundles. If the vendor publishes no full image, record that
  fact and the vendor's documented recovery route (e.g. an official flash
  tool) in the proposal; that is a gap, not a license to improvise.
- Match the exact codename and, where the vendor splits by carrier or region,
  the exact variant the device reports. When in doubt, ask the owner.
- Record `bytes` from the vendor page or a verified download; record the
  `source-page` so a reviewer can re-check.
- Download with the generic workflow
  (`FERRIX_RECOVERY_DOWNLOAD=I-understand-this-downloads-vendor-firmware
  tools/device/recovery-download.sh --target <id>`, Stub-tested only;
  `https://` URLs only; resumable via `.part`) only after the owner agreed to
  the download in this session. A mismatch deletes the file and is a stop.
- Inspect offline where a tool exists. The Pixel inspector reads
  `android-info.txt` requirements (bootloader and baseband minimums) and the
  image member list; those requirements become the proposal's image
  compatibility criteria.

## 4. Sources to pin

- **Kernel**: vendor downstream tree or mainline/pmOS if it exists for the
  codename. Record branch and exact commits in a `kernel-sources.toml`
  (`[[source]] name/url/revision`) like the Pixel 9a file. A moving branch
  name is not a pin.
- **Device tree**: the tree and commit that carries the board DT/DTBO for the
  observed hardware revision.
- **Drivers**: display panel, touch, storage, Wi-Fi, Bluetooth, modem
  interface, audio, sensors, charging; one row each with source and commit.
- **Firmware**: an official package for the exact build, a `firmware-policy.toml`
  listing controllers, allowed source images, runtime paths, and
  `commit_firmware_bytes = false`. Extraction is local and opt-in.
- **postmarketOS basis**: check pmaports for `device-<vendor>-<codename>` and
  `linux-<vendor>-<codename>`. If present, cite the package and commit. If
  absent (as for `tegu`), plan a target-local package under
  `targets/boards/<id>/pmos/device-<vendor>-<codename>/` with `deviceinfo`,
  `deviceinfo_flash_method="none"`, a `ferrix-backends.env` selecting
  production adapters (`networkmanager`, `bluez`, `modemmanager`,
  `wireplumber`, `upower`) and `FERRIX_EVIDENCE=physical:<id>`.
- **Boot container**: header version, split boot layout (`boot`,
  `init_boot`, `vendor_boot`, `vendor_kernel_boot`, `dtbo`, `*_dlkm`,
  `vbmeta*`), page size, cmdline, derived from the stock boot image or the
  factory metadata. Record it in an `image-layout.toml` with
  `temporary_boot_before_persistent_write = true` and
  `stock_modules_may_be_reused = false` unless the owner documents why not.

## 5. Evidence each capability needs

Order from `linux-plan.md`; each starts `unsupported` and moves only with
exact-target, redacted evidence:

| Capability | Minimum evidence |
| --- | --- |
| lifecycle | serial/USB console reaches a login or Ferrix readiness record; orderly shutdown |
| storage (prerequisite) | UFS/eMMC enumerated; rootfs mounted rw; fsck clean after reboot |
| display | DRM mode set; inspected framebuffer capture, not a process log |
| input | touch events with verified coordinates |
| power | UPower `DisplayDevice` reports battery and charging; `ferrix-power-upower` publishes changes; unplugged-to-charging observed |
| network | Wi-Fi firmware loads; scan, WPA association, DHCP, DNS, traffic via NetworkManager and `ferrix-networkd` |
| bluetooth | BlueZ adapter present and powered via `ferrix-bluetooth-bluez`; discovery/pairing are extra, documented separately |
| audio | WirePlumber sinks enumerated; playback and capture; route classification |
| telephony | ModemManager exposes the modem; SIM state; registration; data; SMS; only then voice. If the modem has no ModemManager backend, status stays `unsupported` |
| suspend/resume | repeated cycles with charger, Wi-Fi, Bluetooth, modem wake cases |

## 6. Mutation workflows for the new target

No mutation until `tools/<target>/` has checked-in, stub-tested workflows.
Model them on `tools/pixel-9a/unlock-bootloader.sh`:

- Two modes: `--preflight` (read-only) and `--confirm-<op>`.
- Refuse zero or multiple devices; require exactly one transport.
- Re-verify recovery archives by size and SHA-256 from `recovery.pins`.
- Read and print identity (serial, product/codename, revision, SKU,
  bootloader, slot, slot count, verified boot, unlocked); compare with the
  manifest; `die` on mismatch.
- Print `operation=`, `partition_target=`, `input_image_sha256=`,
  `factory_recovery_sha256=`, `ota_recovery_sha256=`, `preflight=passed`.
- `confirm_phrase` reads one line from stdin and compares exactly; on
  mismatch, `die` with "no mutation performed".
- Re-check identity between transport changes (after any reboot).
- Issue only the named commands; print `mutation=<command>` before each.
- Print a `result=` line and where the device is left.
- Tests under `tools/<target>/tests/`: a static allow-list grep for every
  `adb`/`fastboot` invocation (see `tools/pixel-9a/tests/discover-read-only.sh`)
  and dynamic stub runs for zero devices, multiple devices, wrong identity,
  wrong phrase, and exact phrase; the stubs log every call and the test
  asserts no mutation verb ran in the read-only paths.
- Add `just <target>-<op>` recipes with `[positional-arguments]` and a
  `<target>-tools-test` recipe.

Until those exist, the only permitted state-changing artifact is a gate
skeleton that refuses, like `tools/oneplus/provision-gate.sh` (exit 3,
`no-approved-proposal`).

## 7. Approval record

Before the first mutation, the proposal must state, with dates: owner
approval of the proposal; identity `passed`; official recovery inputs
pinned and verified; backup verified readable outside Git; recovery
procedure documented (and, before flashing Ferrix, tested); image
compatibility criteria recorded; per-run confirmation `required`. Copy the
Pixel 9a `manifest.toml` `[gates]` block and keep it truthful.
