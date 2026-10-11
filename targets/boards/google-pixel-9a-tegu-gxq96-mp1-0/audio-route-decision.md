# Audio route decision — google-pixel-9a-tegu-gxq96-mp1-0

Status: **offline decision record, 2026-10-12, with read-only physical
inventory findings (2026-10-11, [AoC offline](#physical-finding-2026-10-11-aoc-offline);
after #58, [TAS25xx tuning missing](#physical-finding-after-58-tas25xx-tuning-missing)).**
Nothing here is physical audio playback or capture evidence.
`capabilities.toml` keeps `[audio] status = "unsupported"`
(`capabilities.toml:27-29`). It is written for issue #2; the AoC firmware
slice is issue #55 and the TAS25xx tuning slice is issue #59.

Repository claims below cite checked-in paths and lines. The pinned AoC
source was inspected for the firmware-loading decision below; other upstream
audio sources and generated module lists still need review where noted.

## What the repository proves today

| Layer | Checked-in fact | What it does **not** prove |
| --- | --- | --- |
| Kernel sources | The target pins `google-modules/amplifiers`, `aoc`, and `aoc-ipc` and labels them audio sources (`kernel-sources.toml:69-83`). `tools/pixel-9a/kernel.sh:94-96` maps them into the Kleaf tree. | That any of them builds an ALSA/ASoC card driver for tegu, or which amplifier tegu uses. |
| Kernel config | Neither Ferrix fragment sets a sound, ASoC, or AoC audio symbol. `kernel-config.tegu:1-23` covers panel, S5300, and touch only (its single AoC symbol, line 21, is the touch channel). `kernel-config.ferrix-gki:1-19` covers namespaces, devtmpfs, netfilter, and IPC. | That audio is absent. Audio comes from Google's unmodified tegu/GKI defconfigs, which are not checked in. |
| Required modules | `pmos/device-google-tegu/modules-initfs:25-26` requires `cs40l26-i2c` and `snd-soc-cs40l26`, copied from the stock vendor-ramdisk list (lines 7-11). `kernel.sh:214-224` fails the build if Kleaf does not emit them. | That `cs40l26` is a speaker, earpiece, or microphone path. Its role is unverified (see Unknowns). |
| Loaded modules (dev route) | `initramfs-init-kleaf:371-378` loads all of Kleaf's generated `system_dlkm.modules.load` and `vendor_dlkm.modules.load`. Firmware lookup points at the rootfs while they load. `tools/pixel-9a/dev-compose.py:203-218` copies those lists unfiltered from the Kleaf archives. | Which audio modules are in those generated lists, or whether they probe. The lists are build outputs and are not checked in. |
| Radio-first route | `modules-vendor-dlkm:1-4` and `modules-system-dlkm:1-3` select no loadable modules. | Applies only to the native radio route. The Kleaf dev route above is a different route. |
| Firmware | Issue #55 adds one `audio-aoc` controller to `firmware-policy.toml` and one stage row, `vendor.img:/firmware/aoc.bin` → `lib/firmware/aoc.bin` (`firmware-stage-allowlist.tsv`), with an optional runtime row (`pmos/device-google-tegu/firmware-allowlist.tsv`). The physical #58 image brought AoC online. Issue #59 adds the same shape for `audio-tas25xx`: `vendor.img:/firmware/tismartpa_driver_tuning.bin` → `lib/firmware/tismartpa_driver_tuning.bin`, optional at runtime. The `display-audio-touch` controller still has `required_names = []`. `allow_unlisted_paths = false`, and the stage workflow refuses persist or `*calib*` rows. | That the factory TAS tuning parses and initialises the amplifier on this kernel. Haptics (CS40L26) firmware is not staged. No bytes are in Git. |
| Userspace | The device package depends on `pipewire`, `pipewire-spa-bluez`, and `wireplumber` (`pmos/device-google-tegu/APKBUILD:23-25`). It enables `ferrix-pipewire` and `ferrix-audio` (`APKBUILD:131-138`). It does not depend on `alsa-utils` or `alsa-ucm-conf`. No UCM file exists under the target or `tools/pixel-9a`. | That `aplay`, `arecord`, `amixer`, or `alsaucm` are present in the image, or that UCM is loaded. |
| AoC loader | `pmos/device-google-tegu/ferrix-aoc-firmware.initd` is a fail-open OpenRC one-shot, ordered before `ferrix-pipewire`, `ferrix-audio`, and `ferrix-core`. Nothing `need`s it. It writes `aoc.bin` to the AoC `firmware` control once, and only when `revision` reads `Offline` and the staged image matches its manifest row. It has bounded waits. The physical #58 inventory found the service started and AoC online. | That the sound card binds or audio works. Offline tests also cover the service failure paths (`tools/pixel-9a/tests/aoc-firmware-runtime.sh`). |
| Services | `ferrix-pipewire.initd:6-18` runs PipeWire as `user` after `ferrix-radio-firmware`. `ferrix-audio.initd:13-49` runs WirePlumber and waits up to 10 s for `wpctl status`. `docs/status.md:517-521` records only that both are supervised and respawn. | Any ALSA node in the PipeWire graph. A running daemon is not an audio device. |
| Android audio | Waydroid audio goes through `pipewire-pulse` (`pmos/android/packages:11-23`). | Any hardware output. With no ALSA sink, it reaches only whatever PipeWire offers. |
| Ferrix adapter | `ferrix-backends.env:15` selects `wireplumber`. The target adapter composes `WirePlumberAudio::connect()` and falls back to `UnavailableHardware` on error (`adapter/src/lib.rs:837-841`, `292-302`). `connect` fails when no classified default sink exists (`crates/ferrix-audio-wireplumber/src/lib.rs:71`, `157-174`). | That a route works. Without an ALSA sink, the product sees `unsupported`. |
| Product API | `ferrix-hardware-api::Audio` models output routes only: speaker, earpiece, headphones, Bluetooth (`crates/ferrix-hardware-api/src/lib.rs:833-898`). Call route and uplink mute belong to telephony (`crates/ferrix-telephony-api/src/lib.rs:825-833`). | General microphone capture, volume, or playback mute as typed hardware controls. None exists, and this record does not add them. |

Conclusion: this target has **built and supervised audio services, no proven
ALSA card, no proven speaker output, no proven mic capture, and no call audio.**
The board plan orders audio after Bluetooth and before cellular. Call audio
comes only after voice calls (`linux-plan.md:827-829`, `837-840`).

## Decision

1. **Target first, adapter unchanged.** First determine whether a kernel card,
   firmware, or mixer policy is missing. Those are target layers. No
   `ferrix-audio-wireplumber` or `ferrix-hardware-api` change is justified
   before an ALSA card exists on this exact device.
2. **No UCM until observed.** Do not translate Android `mixer_paths*.xml`,
   audio HAL XML, or AoC HAL configuration into UCM. A target UCM profile may
   be written only from controls captured on this device under Linux
   (`amixer contents`, `alsactl store`). Upstream `alsa-ucm-conf` is used
   instead if it already matches the reported card name.
3. **Layer ownership** once a card exists:
   - kernel modules and DT: Kleaf config in this target package;
   - firmware names: add a narrowly scoped controller to the existing
     policy only after verifying the driver request path, pinned factory
     source, and exact staging paths;
   - mixer and enable sequences: target UCM under `pmos/device-google-tegu/`;
   - node naming and priority: a target WirePlumber rule, if needed;
   - `ferrix-audio-wireplumber`: changes only if UCM exposes speaker and
     earpiece as ports of one sink. Its README (`crates/ferrix-audio-wireplumber/README.md:20-24`)
     already names that limitation;
   - mic capture: requires its own audio input proof and typed product control
     later; call audio and real uplink mute also depend on #1's modem route
     and #27's end-to-end voice path;
   - volume and playback mute: need typed hardware controls after the actual
     sink and WirePlumber behavior are known; this offline record does not
     change the product API.
4. **`capabilities.toml` stays `unsupported`** until every pass criterion
   below holds and audible speaker output is captured on camera or by a host
   microphone.

## Physical finding 2026-10-11: AoC offline

`physical: google-pixel-9a-tegu-gxq96-mp1-0`, Kleaf/pmOS dev image at source
commit `f49a460`, read-only. These are the redacted findings. Raw
transcripts stay in ignored `out/`.

- The merged inventory (#54) verified the guest identity. ALSA has no cards
  and no PCMs. `dmesg` shows `aoc_snd_card_probe` repeatedly waiting for the
  AoC output control. TAS25xx probes on I2C, but no audio firmware file is
  present. The WirePlumber/PipeWire graph is reachable but has no ALSA sink.
  `ferrix-core` reports audio `unsupported`.
- A targeted read-only sysfs probe found
  `/sys/devices/platform/17000000.aoc/revision`. It reads `Offline`. The
  device's `firmware` control exists. Nothing was written.
- `cs40l26.wmfw` fails a direct firmware load with `-2`. That is a separate
  haptics follow-up and is not handled here.

### Why explicit AoC firmware loading is now justified

Decision 3 above allowed a firmware policy update only after verifying the
kernel request, the pinned factory source, and the exact staging path. The
kernel logged **no** direct-firmware-load request for an AoC image. The
pinned `private/google-modules/aoc/aoc.c` at revision
`ac69293d11343b47f0b2b758b0df9f936360dbd4` sets
`aoc_autoload_firmware = false` by default and names `aoc.bin` as the
default image. Its `firmware_store` accepts a filename in the existing
sysfs control and calls `request_firmware_nowait`; its `revision_show`
returns `Offline` until the DSP is ready. Ferrix currently never writes the
control. That explains the absent firmware request and waiting sound card
without assuming that the firmware itself will boot successfully.

The slice stays narrow:

- one factory file, `vendor.img:/firmware/aoc.bin`, staged under its own
  name through the existing hash-verified factory workflow (archive
  size/SHA-256 pin, per-file manifest bytes/SHA-256, rootfs re-hash, runtime
  radio-gate re-hash);
- one target OpenRC one-shot that names it once while AoC reads `Offline`;
- no TAS25xx or CS40L26 firmware, no per-unit calibration, no UCM, no mixer
  writes, and no adapter or `ferrix-hardware-api` change.

The audio inventory probe (version 2) now reports the AoC device, a
classified revision (`offline`/`online`/`unreadable`/`absent`, never the raw
text), firmware-control presence, the staged image, its manifest row count,
and the loader service state. The host records them as the `aoc-dsp` row,
which stays `unsupported`.

`[audio]` stays `unsupported`. AoC reporting online would be progress
toward A1, not sound. The pass criteria below are unchanged.

## Physical finding after #58: TAS25xx tuning missing

`physical: google-pixel-9a-tegu-gxq96-mp1-0`, dev image carrying #58,
read-only, reported by the PM. The raw, redacted inventory stays in ignored
`out/pixel-9a/audio/`.

- AoC is online and the `ferrix-aoc-firmware` service started.
- ALSA still has zero cards and zero PCMs. PipeWire has no ALSA sink or
  source.
- The TAS25xx driver on I2C `19-0048` repeatedly requests
  `tismartpa_driver_tuning.bin` and gets `-2` each time. `aoc_snd_card_probe`
  registration then fails with `-2`.

### Source review (issue #59)

Pinned `google-modules/amplifiers` revision
`33976d763bc0ace11603e7ecb207fc2b6e3f2608` (`kernel-sources.toml:69-72`),
`tas25xx/` subtree, read-only:

- **Exact name.** `src/tas25xx-regbin-parser.c` defines
  `TAS25XX_BINFILE_NAME "tismartpa_driver_tuning.bin"`.
  `tas25xx_load_firmware` passes it to `request_firmware_nowait` and retries
  while the file is absent. Nothing reads a DT property for the name, so the
  Linux path must be `lib/firmware/tismartpa_driver_tuning.bin`.
- **When it is requested.** The only callers are in `src/tas25xx-codec.c`.
  The ASoC component probe calls `tas25xx_start_fw_load(p_tas25xx, 20)`, and
  `hw_params` plus three other paths retry with 3. The I2C probe does not
  request it. The request therefore happens when the AoC card binds its
  components. That is after AoC is online, which on this image is after
  `switch_root`, so the default `/lib/firmware` search path applies.
- **Failure path.** A missing file ends as `-ENOENT` (`-2`), which matches
  the observed `-2` per request. However, the component probe maps
  `-ENOENT` to `0` (`if (ret == -ENOENT) ret = 0;`), so a missing tuning
  file alone does not fail that probe. The firmware stays unloaded and
  `hw_params` retries later. The AoC card's own `-2` is therefore **not**
  shown to be caused by the missing file. The file is still needed before
  the amplifier can be initialised for playback.
- **Content and selection.** `tas25xx_fw_ready` parses an optional `MDATA`
  table that picks one sub-image by the chip's silicon revision ID. The ID is
  read from `TAS25XX_REVID_REG` in `src/tas25xx-regmap.c`. A `HEADR` block
  follows: version ≤ `SUPPORTED_BIN_VERSION` 7, name, timestamp, channel
  count, I2C addresses, IV width and Vbat monitor. Then come per-channel
  `INITP` register sequences, `HWPRM` format options, `PROFP` profiles,
  `INTRP` interrupt handling and `BLKOP` operation blocks. Last are the
  `KCNTR` mixer controls and an optional `ALGOP` block. The `BLKOP` `CLINI`
  and `CLDIN` entries are register sequences that put the amplifier into and
  out of calibration mode on request. They are not measured values. Nothing
  in the format is keyed to a unit serial or a per-unit measurement.
- **Calibration is a separate path.** TI's per-unit speaker calibration is
  `SMARTAMP_SPEAKER_CALIBDATA_FILE "/persist/audio/smartamp_calib.bin"`
  (`algo/inc/tas_smart_amp_v2.h:44`, opened with `filp_open` in
  `algo/src/tas25xx-algo-bin-intf.c`). A second copy is requested as
  `smartamp_calib.bin` in `algo/src/tas25xx-algo-kbin-intf.c`. Both are under
  `algo/` and need `TAS25XX_ALGO`, `TISA_BIN_INTF` or `TISA_KBIN_INTF`. The
  module `Makefile` enables only `SND_SOC_TAS25XX`, `TAS25XX_MISC`,
  `TAS25XX_CODEC` and `TAS25XX_IRQ_BD`; the `ALGO` and `KBIN` lines are
  commented out. The driver source contains no `speaker.cal` or
  `ti_sp_tuning.bin` reference. Those are consumed outside the kernel.
- **Provenance.** The PM independently verified a prior hash-pinned
  extraction: `vendor.img:/firmware/tismartpa_driver_tuning.bin` is a
  23,160-byte regular file, SHA-256
  `da10e6bc27db8b339a28636534404b1d4068ac91de2c2118d7f02ca28059e6fe`. It
  came from the factory archive with SHA-256
  `0c2f9053bf064b09483c937cf16f4824371afe92fa057029d6f9237668d1c0d7`.
  `vendor.img` is one build-wide image flashed to every unit, so it cannot
  carry this unit's measurements. This is existence and provenance, not
  proof that the file loads.

Conclusion: the request name and source path are justified, and the file is
generic, revision-selected factory tuning rather than per-unit data. The
slice mirrors #55:

- one `audio-tas25xx` controller in `firmware-policy.toml`;
- one stage row, `vendor.img:/firmware/tismartpa_driver_tuning.bin` →
  `lib/firmware/tismartpa_driver_tuning.bin`, through the same hash-verified
  factory workflow and manifest;
- one optional runtime row (`required = 0`), so a missing file cannot block
  the radio gate or boot;
- no loader or service: the driver requests the file itself;
- no `speaker.cal`, `smartamp_calib.bin`, `ti_sp_tuning.bin`, CS40L26
  firmware, UCM, mixer writes, adapter change, or `ferrix-hardware-api`
  change.

Offline tests cover the single stage row, staging it through the synthetic
factory archive, the rejection of TAS-controller calibration rows, and the
rootfs and manifest handoff. **No image carrying this change has booted.**
TAS initialisation, the ALSA card and PipeWire nodes all remain unproved.
`[audio]` stays `unsupported`.

### Open questions for the next physical inventory

- Whether the factory file's `MDATA` table has an entry for this unit's TAS
  silicon revision. A miss logs `metadata parse failure` and yields `-EINVAL`
  rather than `-2`.
- Whether the file's `HEADR` version is ≤ 7. A newer version is rejected
  after the header is read.
- What actually returns `-2` from `aoc_snd_card_probe`. The TAS component
  probe swallows `-ENOENT`, so another component, link or resource may be
  missing. Record the first error before the card failure. Staging the
  tuning file may not by itself let the card register.
- Whether the requests happen at card bind after `switch_root`, as the
  source implies. If dmesg shows them before `mounting`, the firmware search
  path matters, and staging alone would not be enough.
- Whether the Google Kleaf build passes extra `CONFIG_TAS25XX_*` options
  beyond the module `Makefile`. The checked copy is only the `tas25xx/`
  subtree. If `TAS25XX_ALGO` were enabled, a later `smartamp_calib.bin`
  request would appear in dmesg. It must still not be staged.
- The exact amplifier part and the second channel's address (`0x49` in the
  driver's sample DT). Record them from the probe log.
- Whether the AoC card then registers, or next waits on the CS40L26 haptics
  back end (`cs40l26.wmfw -2`). CS40L26 firmware stays out of this slice
  until a physical inventory shows that it blocks the card.
- The parser does no bounds checking on the file. Only the hash-pinned
  factory bytes that Android ships may reach it.

## Exact-device read-only inventory plan

**Preconditions (all required):**

- issue #25 restore and recovery proof is complete;
- the device is booted on the development flash route (`README.md#development-flash-route`);
- the guest is reachable over the USB network SSH used by
  `tools/pixel-9a/radios.sh`;
- the device-tree identity check from `radios.sh` passes.

No `adb` or `fastboot` is used. Nothing writes to partitions, mixer controls,
or module state.

**Read-only commands** (guest shell, as root unless noted). Save each one's
output under ignored `out/pixel-9a/audio/<UTC>/`.

| # | Command | Records |
| --- | --- | --- |
| 1 | `uname -r; cat /proc/device-tree/compatible \| tr '\0' '\n'` | kernel release and the exact-target guard |
| 2 | `cat /proc/asound/version /proc/asound/cards /proc/asound/pcm; ls /proc/asound` | ALSA core presence, card IDs, PCM devices |
| 3 | `for c in /proc/asound/card*; do cat $c/id; ls $c; done` | per-card IDs and streams |
| 4 | `ls -l /dev/snd` | device nodes and their group |
| 5 | `grep -iE 'snd\|aoc\|cs3\|cs4\|tas\|codec' /proc/modules` and `lsmod` | loaded audio-related modules |
| 6 | `d=/lib/modules/$(uname -r); grep -iE 'snd\|aoc\|cs40l26\|cs35\|tas' $d/system_dlkm.modules.load $d/vendor_dlkm.modules.load` | which audio modules the dev route attempted |
| 7 | `dmesg \| grep -iE 'firmware\|aoc\|snd\|asoc\|cs40l26\|cs35\|tas\|codec\|deferred'` | firmware names requested or failed, probe deferrals, ASoC bind errors |
| 8 | `ls /sys/bus/platform/drivers \| grep -iE 'aoc\|snd\|asoc\|cs\|tas'` and `cat /sys/kernel/debug/devices_deferred` (if debugfs is mounted read-only) | bound drivers and deferred devices |
| 9 | `find /proc/device-tree -name compatible -exec grep -l -iE 'aoc\|audio\|sound\|cs40l26\|cs35\|tas' {} +` | DT nodes the kernel sees, by path only |
| 10 | `command -v aplay arecord amixer alsaucm; apk info -e alsa-utils alsa-ucm-conf` | tool and UCM package presence |
| 11 | if tools exist: `aplay -l; aplay -L; arecord -l; arecord -L; amixer -c N contents` | PCMs and mixer control names, read only |
| 12 | if tools exist: `alsaucm -c <card-id> list _verbs` | whether any UCM profile matches the card |
| 13 | as `user`, with `XDG_RUNTIME_DIR=/run/user/1000`: `wpctl status; wpctl inspect @DEFAULT_AUDIO_SINK@; wpctl inspect @DEFAULT_AUDIO_SOURCE@` | the PipeWire graph: ALSA sinks/sources versus dummy/null only |
| 14 | `rc-service ferrix-pipewire status; rc-service ferrix-audio status` | supervision state |
| 15 | the Ferrix core state request already used by `radios.sh` for other capabilities, audio field only | whether ferrix-core sees `WirePlumberAudio` or the unavailable fallback |

**Excluded on purpose:**

- no `aplay`, `speaker-test`, or `pw-play`;
- no `amixer set`/`cset`, `alsactl restore`, or `alsaucm set`;
- no `modprobe`/`rmmod`, no firmware copies, no `echo` into sysfs.

The amplifier and any speaker-protection requirement are unknown, so
inventory plays no sound.

**Redaction:**

- Do not capture `/proc/cmdline` or the bootconfig raw. They carry
  `androidboot.serialno`.
- Redact Bluetooth MAC addresses in `wpctl` node names (`bluez_*`).
- Redact `device.serial`, `api.alsa.card.longname`, and `*.bus-path` values
  if they contain unique IDs.
- Redact any IMEI, IMSI, or ICCID that appears in dmesg.
- Label every committed line `physical: google-pixel-9a-tegu-gxq96-mp1-0`.
  Raw transcripts stay in ignored `out/`.

**Pass/fail observations:**

| ID | Pass | Fail meaning |
| --- | --- | --- |
| A1 | `/proc/asound/cards` lists at least one card that is not a dummy/loopback card | no ALSA card: a kernel-module or DT/probe gap. Read the step 5-9 output. |
| A2 | `aplay -l` lists at least one playback PCM on that card | the card exists but has no playback stream |
| A3 | `arecord -l` lists at least one capture PCM | no capture path is exposed |
| A4 | step 7 shows no failed firmware load for an audio-related name | record each name and verify its source and purpose before considering a pinned `firmware-policy.toml` entry. |
| A5 | `wpctl status` lists an `alsa_output.*` sink and an `alsa_input.*` source | inspect ALSA discovery and ACP/UCM/WirePlumber policy; do not assume the profile alone is at fault |
| A6 | ferrix-core reports an audio route and not the unavailable fallback | the adapter cannot classify any sink: check sink naming against `classify_sink` |
| A7 | `alsaucm` finds verbs for the card | no UCM: expected. Write UCM later from the A2/A3/step-11 controls. |

A1-A7 passing still does **not** prove sound. Speaker output, earpiece
output, and mic capture each need a separate, owner-approved active test at
low volume. That test needs captured audio as evidence and comes after the
amplifier is identified. Call audio stays blocked on telephony.

## Unknowns

- Whether the pinned `aoc` tree builds an ALSA/ASoC machine driver for tegu,
  and under which module names. Settle this with the pinned upstream
  `aoc/BUILD.bazel` or Kbuild and the generated `vendor_dlkm.modules.load`.
- Whether naming the factory `aoc.bin` in the AoC `firmware` control brings
  AoC online on this kernel. That includes whether the driver accepts the
  write, authenticates the image, and leaves `Offline`. Whether the AoC sound
  card then binds without the CS40L26 haptics back end is also unknown.
  The 2026-10-11 finding settles the earlier question of whether Linux must
  start AoC (it stays `Offline` otherwise). It does not settle these. Settle
  them with the next physical inventory after an image that carries #55. The
  `aoc_core` is loaded in the physical inventory; its inclusion in the
  generated module list can still be checked independently.
- The role of `cs40l26` / `snd-soc-cs40l26` (`modules-initfs:25-26`). The
  name suggests a Cirrus Logic haptics part with an ASoC component. Treat
  that as unverified until the pinned `amplifiers` source and tegu DT are
  read. Do not count it as a speaker path.
- The exact speaker and earpiece amplifier part, and whether it needs
  calibration or speaker protection before safe playback. After #58 a
  `snd-soc-tas25xx` device probes at I2C `19-0048` (see
  [above](#physical-finding-after-58-tas25xx-tuning-missing)). The exact TAS
  part number is not yet recorded.
- Whether Pixel audio on this SoC is reachable from Linux without Android's
  AoC audio HAL. If it is not, a project-owned adapter would be needed. That
  is a plan-level decision, not a target tweak, and it needs `plans/` review.
- How `ferrix-audio-wireplumber` behaves when a card appears after ferrix-core
  starts. `connect` runs once, at composition (`adapter/src/lib.rs:837`).

## Next coding slice (justified, not done here)

Add a read-only `tools/pixel-9a/audio-inventory.sh` with a `just` recipe. It
would run steps 1-15 over the existing guest SSH route, apply the redactions
above, and emit an A1-A7 table. It would be stub-tested with fixtures like
`tools/pixel-9a/tests/fixtures/radios/`, and it would never edit
`capabilities.toml`. Add `alsa-utils` to the opt-in dev image only if A1
passes and step 10 shows the tools are missing. No UCM, firmware, or product
API change belongs in that slice.
