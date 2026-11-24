# Pixel 9a audio bring-up (AoC)

Target: `google-pixel-9a-tegu-gxq96-mp1-0`, Google Kleaf dev route.
Status: **offline only, not yet booted.** All the analysis below comes from
the pinned sources and the hash-verified `CP3A.260905.009` factory
`vendor.img` (sha256 `41c5d717…dc29bf7`). Nothing here has run on the phone.
The "today" observations are the lead's, taken on the exact target.

## Why there is no sound card today

Observed on the device (2026-10-11): `/proc/asound/cards` lists no cards.
`aoc_core`, `aoc_alsa_dev`, `aoc_channel_dev`, `aoc_control_dev`,
`snd_soc_tas25xx` and `snd_soc_cs40l26` are loaded. dmesg shows
`found aoc with interrupt:0 sram:[…] dram:[…]`, then
`platform sound-aoc: deferred probe pending` and repeated
`alsa: aoc_snd_card_probe: wait for aoc output ctrl`. There is no firmware
request for AoC. PipeWire offers only "Dummy Output".

The cause, from the pinned AoC source (`private/google-modules/aoc`
`ac69293d`):

1. `aoc_core.ko` is a first-stage module (`vendor_kernel_boot.modules.load`).
   Its probe maps SRAM/DRAM and the mailboxes, then loads firmware only if
   the module parameter `aoc_autoload_firmware` is set. It defaults to false.
2. On Android, `/vendor/bin/aocd` (`aocd.rc`, `class late_start`) waits for
   `/dev/aoc` and writes `aoc.bin` to
   `/sys/devices/platform/17000000.aoc/firmware`. The sepolicy grants
   `aocd` write on `sysfs_aoc_firmware`. Strings in the binary show the
   attribute path, `/vendor/firmware/`, `aoc.bin` and `aoc-beta.bin`. The
   `persist.vendor.aoc.*` properties and their ioctls (monitor-mode disable,
   force VNOM, UART, AP-restart disable) are debug options that default
   off.
3. `firmware_store()` calls `request_firmware_nowait(…, "aoc.bin", …)`.
   `aoc_fw_callback()` validates the superbin header, checks the interface
   UUID table, has GSA authenticate the signed image
   (`gsa_load_aoc_fw_image`), passes board and heap data, and starts the
   DSP (`GSA_AOC_START`). AoC then announces its services. That creates the
   AoC service devices, including `audio_output_control`.
4. `google-aoc-snd-card`'s probe returns `-EPROBE_DEFER` until
   `audio_output_control` exists: "wait for aoc output ctrl". Nothing on the
   Ferrix image performs step 2, so the card never binds.

`aoc_autoload_firmware=1` (`aoc_core.aoc_autoload_firmware=1` on the kernel
command line) is not a fix. The probe runs from the vendor ramdisk before
userdata is mounted, so `/lib/firmware/aoc.bin` does not exist yet and the
direct load fails at once. `CONFIG_FW_LOADER_USER_HELPER_FALLBACK` is off.

### A second blocker: the haptics back end

The AoC card has a haptics back end (`dai_be_haptic_rx`,
`zumapro-tegu-haptics.dtsi`). Its codec is `cs40l26_codec`. ASoC binds a
card only when every link's components exist. The Tegu DT sets neither
`cirrus,fw-defer` nor `cirrus,calib-fw`, so `cs40l26_probe()` calls
`request_firmware("cs40l26.wmfw")` synchronously. It runs from the Kleaf
initramfs with `firmware_class.path=/newroot/lib/firmware`. Without the file
the probe fails and `cs40l26-codec` is never registered, so the card cannot
bind even with AoC online. The lead should check whether today's dmesg
contains `Direct firmware load for cs40l26.wmfw failed` or a cs40l26 probe
error.

### The speaker amplifier

TAS2572 ×2 on I2C `0x48`/`0x49` (`zumapro-tegu-audio-common-tas2572.dtsi`;
every Tegu DT variant uses TAS2572). `snd-soc-tas25xx` requests
`tismartpa_driver_tuning.bin` from its ASoC component probe, so only once
the card binds. The file holds the register init sequences, the
interrupt map and the codec profiles `default`, `music_17dB_stereo`,
`music_17dB_top`, `music_17dB_bottom` and `voice`. Without it `hw_params`
keeps retrying the load and the amplifier is never initialised.

## Ferrix route (this change)

Following the Mali CSF / MFC pattern:

| Controller | `vendor.img` source | Runtime path | Bytes | sha256 |
| --- | --- | --- | --- | --- |
| `audio-aoc` (`aoc_core`) | `/firmware/aoc.bin` | `lib/firmware/aoc.bin` | 29076160 | `09d71c45…d4ea97` |
| `audio-tas25xx` (`snd-soc-tas25xx`) | `/firmware/tismartpa_driver_tuning.bin` | same name | 23160 | `da10e6bc…59e6fe` |
| `haptics-cs40l26` (`cs40l26-core`) | `/firmware/cs40l26.wmfw` | same name | 57716 | `73eb7b0f…615d5b` |
| `haptics-cs40l26` | `/firmware/cs40l26.bin` (wavetable) | same name | 1704 | `9b5c2251…78b4d1` |
| `haptics-cs40l26` | `/firmware/cs40l26-svc.bin` | same name | 1356 | `f755904b…fb29c2` |
| `haptics-cs40l26` | `/firmware/cs40l26-dvl.bin` | same name | 300 | `18a39612…86f96f` |
| `haptics-cs40l26` | `/firmware/cs40l26-dbc.bin` | same name | 156 | `19e1fae4…00fbbe` |

- Rows are in `firmware-stage-allowlist.tsv`, and the controllers are in
  `firmware-policy.toml`. The rootfs allowlist
  `pmos/device-google-tegu/firmware-allowlist.tsv` lists them as optional
  (`0`), like Mali and MFC. They are not radio gate inputs, but
  `ferrix-radio-firmware` hashes every manifest row that is present.
- **Compatibility, checked offline:** the factory `aoc.bin` is a signed
  release image (4096-byte auth header, version `16047897-polygon`) with
  553 UUIDs. Every one of the 293 UUIDs that `_aoc_fw_is_compatible()`
  requires from `aoc-interface-zuma.h` is present (`CONFIG_SOC_ZUMA=y` in the
  dev kernel). The image therefore passes the driver's interface check.
  GSA signature and rollback acceptance can only be shown on the device.
  The bootloader is from the same pinned factory package.
- **Trigger:** `ferrix-aoc-firmware` is an OpenRC one-shot in the default
  runlevel, ordered before `ferrix-pipewire`/`ferrix-audio`. Nothing
  `need`s it. It refuses a missing or symlinked `/lib/firmware/aoc.bin` and
  waits up to 10 s for the `firmware` attribute. If `revision` reads
  anything other than `Offline` it does nothing. Otherwise it writes
  `aoc.bin` and waits up to 10 s for `revision` to leave `Offline`. AoC
  watchdog restarts reload the firmware inside the driver
  (`aoc_watchdog_restart` → `start_firmware_load`), so no daemon stays
  resident. It is shell rather than Rust for the same reason as
  `ferrix-bluetooth-uart`: one sysfs write and a status poll, with no
  project logic to own. The device-free test is
  `tools/pixel-9a/tests/aoc-firmware-runtime.sh` (`just pixel-9a-firmware-test`).
- **Not staged, on purpose:**
  - `ti_sp_tuning.bin` is the audio HAL's speaker-protection tuning, which
    the HAL pushes to the AoC DSP over `/dev/acd-audio_output_tuning`. It
    has no kernel consumer.
  - `/mnt/vendor/persist/audio/speaker.cal` and `mic_compensation.bin` are
    per-unit calibration on the persist partition. That is device data, so
    it is never copied or committed.
  - `cs40l26-calib.{wmfw,bin}` are only for the calibration firmware.
  - `/vendor/etc/aoc/*.pb` and `*.dat` are HAL-pushed DSP tuning.
  - `aoc-beta.bin` is not in this vendor image.

## Device verification (for the lead)

Build: re-stage firmware with the new allowlist
(`FERRIX_PIXEL9A_FIRMWARE=I-understand-this-extracts-verified-firmware just pixel-9a-firmware-stage`
on the builder), then `dev-compose` with `--firmware-dir`, then
`just pixel-9a-dev-flash`. The dev composer installs `ferrix-*.initd` and the
APKBUILD runlevel list itself, so no base rootfs rebuild is needed.

A faster first check without a reflash, on the running image (root shell):
copy the seven staged files into `/lib/firmware/`, reboot so `cs40l26`
re-probes, and run `echo -n aoc.bin > /sys/devices/platform/17000000.aoc/firmware`.

1. AoC:
   ```sh
   rc-service ferrix-aoc-firmware status
   cat /sys/devices/platform/17000000.aoc/revision      # FW Revision : 0x…, not "Offline"
   dmesg | grep -iE 'aoc|gsa' | grep -iE 'firmware|online|signed|GSA|incompatible|failed'
   ```
   Expect `attempting to load firmware "aoc.bin"`,
   `successfully loaded firmware version 16047897-polygon type release`,
   `Loading signed aoc image`, and then
   `firmware version … did become online with N services`. Failure strings
   to look for: `GSA authentication failed`, `firmware and drivers are
   incompatible`, `Failed to load AoC firmware image`, and `AOC detected not
   online` (SSR).
2. Haptics codec and amplifier:
   ```sh
   dmesg | grep -iE 'cs40l26|tas25xx|TI-SmartPA|wmfw'
   ls /sys/bus/i2c/drivers/ | grep -iE 'cs40l26|tas25xx'
   ```
   Expect no `cs40l26.wmfw` load failure and a `Register codec component`
   line or a bound device. For tas25xx, expect `fw load requested` and no
   `firmware is not ready`.
3. Card and PCMs:
   ```sh
   cat /proc/asound/cards          # a card from "google,aoc-snd-card" (driver aoc-snd-card)
   cat /proc/asound/pcm            # "EP1 playback" … stream names
   aplay -l; amixer -c <N> controls | wc -l
   ```
   `aplay`/`amixer`/`speaker-test` come from `alsa-utils`, and `pw-play`
   from `pipewire-tools`. Neither is in the image (`apk add alsa-utils
   pipewire-tools`).
4. Touch after AoC is online: AoC also hosts the touch bus negotiation
   (TBN) service (`gti: … AOC TBN service is not ready` today). Confirm
   that touch still works, including after a screen-off/on cycle.
5. Path with no sound (no speaker risk). Route EP1 to the speaker TDM and
   push silence:
   ```sh
   C=<card index>
   amixer -c $C cset name='TDM_0_RX Mixer EP1' 1
   aplay -D hw:$C,0 -f S16_LE -r 48000 -c 2 -d 2 /dev/zero
   ```
   With no route, `prepare` fails with `ASoC: no backend DAIs enabled for
   EP1 …`. That is the expected missing-mixer state, not a driver bug.
6. Speaker, at low level only (see risks):
   ```sh
   amixer -c $C cset name='TAS25XX CODEC PROFILE' music_17dB_stereo
   amixer -c $C cset name='TAS25XX ASI Switch' 1
   # leave TAS25XX PLAYBACK_VOLUME_{LEFT,RIGHT} at the stock 110
   rc-service ferrix-audio restart          # WirePlumber re-probes the card
   wpctl status                             # an AoC sink besides Dummy Output
   wpctl set-volume @DEFAULT_AUDIO_SINK@ 0.1
   pw-play --volume=0.1 /usr/share/sounds/alsa/Front_Center.wav   # or any short speech WAV
   ```
   Stop at once on distortion, buzzing or a warm speaker. Afterwards set
   `TAS25XX ASI Switch` 0 and `TDM_0_RX Mixer EP1` 0.
7. Headset: the Pixel 9a has no 3.5 mm jack. Wired headsets are USB-C:
   USB Audio Class through the built-in `snd-usb-audio`, which needs the
   port in host mode while the dev route uses it as a CDC-ACM gadget, or
   AoC USB offload (`USB_RX Mixer EP*`, `aoc_usb_driver`). Bluetooth A2DP
   through PipeWire/BlueZ needs no AoC and is the safe loud-audio path.

## Risks

- **Speaker damage: yes, possible.** On Android the voice-coil
  thermal/excursion protection for the two micro-speakers is not in the
  kernel. The audio HAL enables it on the AoC DSP
  (`audio_platform_configuration.xml`: speaker `codec="tas25xx"`,
  `EnableProtection`, `AcousticShockProtection`,
  `DefaultReValues 5.699,6.91` Ω, `DefaultTemperature 25`, plus
  `ti_sp_tuning.bin` and the per-unit `speaker.cal`). The HAL also
  throttles on `VIRTUAL-SKIN-SPEAKER` and on battery current
  (`BCL_AUDIO_BAACL`). Ferrix runs none of this. The TAS2572 protects
  itself (over-temperature, over-current, brownout), not the speaker. Until
  a Ferrix adapter configures protection, keep speaker tests to:
  - ≤ −20 dBFS digital (`--volume=0.1`) at the stock amp volume 110 and the
    `music_17dB_*` profiles;
  - a few seconds, speech or music only: no sustained sine, no
    `speaker-test` pink noise, nothing below about 300 Hz;
  - no raising `TAS25XX PLAYBACK_VOLUME_*` or `TAS25XX BOOST CURRENT *`
    above the stock 110 and 25.
  Use Bluetooth for anything loud.
- **AoC version skew:** the kernel modules come from the Android 16 branch
  and the AoC image from the Android 17 factory build. The UUID check
  passes offline, but IPC behaviour beyond the method signatures is
  unverified. An SSR loop shows as repeated `aoc subsystem restart`.
- **AoC also owns sensors, CHRE, UWB and touch TBN.** Bringing it online
  can change touch-bus arbitration and power draw, and it holds the AP
  awake for 2 s per boot.
- **Haptics:** staging `cs40l26.wmfw` makes the vibrator driver probe
  fully, so its input force-feedback device appears. Nothing in Ferrix
  drives it yet.

## Follow-up: mixer/UCM paths the AoC card needs

Taken from the stock `vendor.img:/etc/mixer_paths_aidl.xml` (with
`audio_platform_configuration.xml`). A Ferrix UCM profile or audio adapter
must provide these. None are set by this change.

- **Initial defaults:** `TDM_0_RX/TX Chan Four`, `Format S32_LE`, `Sample
  Rate SR_48K`; `PCM Stream Wait Time in MSec 100`; `TAS25XX
  PLAYBACK_VOLUME_LEFT/RIGHT 110`; `TAS25XX ASI Switch 0`; `TAS25XX CODEC
  PROFILE default`; `TAS25XX EM LEFT/RIGHT NG_ON`; `TAS25XX BOOST CURRENT
  LEFT/RIGHT 25`; `HAC AMP EN 0`; and every `*_RX Mixer EP*` set to 0.
- **Front ends (AoC endpoints → stock use case):** EP1 = mmap playback,
  EP2 = primary playback, EP4 = spatializer offload, EP6 = deep buffer,
  EP7 = compress offload, plus RAW, VOIP, IMSV and HIFI (USB only).
- **Speaker:** `TDM_0_RX Mixer EP<n> 1` for the stream, plus device path
  `speaker`: `TAS25XX CODEC PROFILE music_17dB_stereo` and `TAS25XX ASI
  Switch 1`. `speaker-safe` uses `music_17dB_bottom`. Earpiece (top
  speaker as receiver) uses `speaker-earpiece`: profile `voice`, with HAC
  through `HAC AMP EN 1`.
- **USB headset/device:** `USB_RX Mixer EP<n> 1`.
- **Bluetooth (AoC offload):** `BT_RX Mixer EP<n> 1`. Ferrix currently
  routes BT through BlueZ/PipeWire instead.
- **Capture:** `EP<n> TX Mixer TDM_0_TX` (IV-sense feedback for protection)
  and the `audio_capture*` front ends. The microphones are AoC-managed, and
  `aoc: unsupported dmic power list, err = -22` means the DT has no
  `dmic_power_list`. The Ferrix mic adapter is a separate task.
- **Protection and tuning:** speaker protection enable, Re/temperature,
  `ti_sp_tuning.bin` and the `/vendor/etc/aoc/*` tuning blobs go through
  the `/dev/acd-audio_output_tuning` / `acd-audio_bulk_*` AoC services. That
  needs a project-owned adapter. Do not reimplement the HAL protocol until
  it is documented or fully understood.
