# AoC speaker protection on Linux — research and GO/NO-GO (issue #90)

Target: `google-pixel-9a-tegu-gxq96-mp1-0` only. Review date 2026-10-12.
Offline and read-only: no phone, builder, GitHub, playback, mixer, TAS, or
AoC write. No calibration contents were read, and nothing extracted is in
Git. This record extends the issue #83 decision in
[`audio-route-decision.md`](audio-route-decision.md#speaker-protection-source-and-effective-build-decision-issue-83).
It does not change `capabilities.toml`. `[audio]` stays `unsupported`.

**Decision: NO-GO** for designing or running an internal speaker or earpiece
trial. The pinned public sources define no Linux contract that shows AoC
speaker protection is active. They also define no documented way to set it
up without Android's closed audio HAL. The earpiece uses the same TAS/AoC
chain and stays blocked too. Bluetooth A2DP is still the recommended output
proof for issue #2.

## Evidence labels and sources

- **source**: public pinned trees, fetched by exact revision into a scratch
  directory outside Git and checked with `git rev-parse HEAD`. Each citation
  is `path:line` with the file's git blob ID, so another reviewer can check
  the same bytes.
  - `google-modules/aoc` `ac69293d11343b47f0b2b758b0df9f936360dbd4`
    (`kernel-sources.toml`, `audio-aoc`)
  - `google-modules/aoc-ipc` `1fe9b9cf3a2954a4a0c0f9868aa04c9d88c902b9`
  - `google-modules/amplifiers` `33976d763bc0ace11603e7ecb207fc2b6e3f2608`
- **#83**: the findings P1–P6 from built and stock artifacts, which this
  review did not repeat. These are the built `snd-soc-tas25xx.ko`
  `6a2c8935…`, the stock module `e5df7fc1…`, and the strings of the stock HAL
  in factory archive `0c2f9053…`.
- **external**: public third-party documentation. It is a precedent only,
  never evidence about this device.
- **inference**: reasoning from the above. It is not proof.

| Cited file (aoc unless noted) | Blob |
| --- | --- |
| `aoc-interface.h` | `bdc65b272b7618af78e16a304f18c041c12554d5` |
| `aoc-interface-zuma.h` | `f0d941e4db05b970f4752d98bb16556fde3d214e` |
| `Kbuild` | `91f35ac62cf8153fe1a2471a58a198fe09dde400` |
| `aoc.c` | `ac86d4c82bb5527140ca66d14e2656a69d119a8d` |
| `aoc_char_dev.c` | `4649885ca58c55c32851eefca56b0b9bd4ec6956` |
| `alsa/aoc_alsa.h` | `ef68823507ce3673b0000a5334aff2a08b3d93d7` |
| `alsa/aoc_alsa_drv.c` | `23d4154f90b68040c5bec5985c22565545a49f7d` |
| `alsa/aoc_alsa_ctl.c` | `afeb5ebe1e5941e6bace3fafa9358a75793c1120` |
| `alsa/aoc_alsa_hw.c` | `e85a57e7d719ac421c2c3441d7542a467195aae6` |
| `alsa/aoc_alsa_path.c` | `fcb4368fe678748720bb077c2ba6f3198cdbea8d` |
| `alsa/aoc_alsa_card.c` | `4cc722f414484dacfc6612078485158049e880c7` |
| `alsa/aoc_alsa_compr.c` | `f06ba949baa249fb8878b07652780e0425173a72` |
| `Documentation/ABI/sysfs-devices-platform-aoc` | `81a5b6ccb6cb372f6450217948d6922a8996a17c` |
| amplifiers `tas25xx/src/tas25xx-regbin-parser.c` | `70786813abb797d54609bae6b6a2e9f073d4157f` |

## What is known

| # | Finding | Label |
| --- | --- | --- |
| K1 | The AoC firmware IPC names a processing module whose name suggests speaker protection (the reading of the name is **inference**). `enum ASP_MODULE_ID` lists `ASP_ID_PROT` (`aoc-interface-zuma.h:1177`, value 29) and `ASP_ID_SPKRCOM` (`:1187`, value 39). The values were computed by compiling the enum. `enum AUDIO_BLOCK_ID` has `ABLOCK_SPEAKER` = 16 (`:1195-1199`). The header names these IDs. It does not say what they do or whether they are on by default. | source |
| K2 | The header defines a module query, `CMD_AUDIO_OUTPUT_GET_MODULES_ID` = 215 (`:138`). Its struct (`:1214-1220`) carries `blk` (`AUDIO_BLOCK_ID`), `module` (`ASP_MODULE_ID`) and a `uint32_t val`. No Linux code in the pinned tree sends this command (repo-wide grep). What `val` means (present, enabled, configured, or engaged) is not documented anywhere in the tree. | source |
| K3 | The only Linux speaker-chain processing control is `AoC Speaker Mixer ASP Mode` (`alsa/aoc_alsa_ctl.c:2570`). It is `SOC_ENUM_SINGLE_DECL(block_16_state_enum, 2, 16, …)` (`:2436`): block 16, component 2, key 0. Its states are `ASP_OFF`/`ASP_ON`/`ASP_BYPASS`/`ASP_GROUND` (`:2434-2435`). Both get and put send `CMD_AUDIO_OUTPUT_GET/SET_PARAMETER` over `audio_output_control` (`alsa/aoc_alsa_hw.c:1012-1063`). Nothing maps component 2 or this mode to `ASP_ID_PROT`. `ASP_ON` (#83 inventory) therefore shows only that a block-level processing mode is on. | source + inference |
| K4 | User space has no raw path to the audio control IPC. The ALSA driver claims `audio_output_control` and `audio_input_control` by name (`alsa/aoc_alsa_drv.c:34-36`). `aoc_bus_match` keeps generic drivers such as `aoc_char` away from a service that has a named driver (`aoc.c:1066-1093`). `/dev/acd-<service>` nodes (`aoc_char_dev.c:118-120`) exist only for unclaimed services. Android's HAL tuning channels (#83 P3, from strings) are among those unclaimed services. Their names and message formats come from the closed HAL and AoC firmware, and are not in any public source. | source + #83 |
| K5 | `TDM_0_TX`, the TAS2572 return path in the tegu DT (`audio-route-decision.md`, speaker back end), is a normal AoC back end. It is configured S16_LE, 2 ch, 48 kHz, 4 × S32_LE slots (`alsa/aoc_alsa_card.c:247-248`). DAPM can route it to the EP1 capture front end (`alsa/aoc_alsa_path.c:1678`, `:2197`). That it carries the amplifier's V/I sense samples, and in which slots, is **inference**. The slot layout and IV width come from the opaque factory tuning file (`HEADR` `iv_width`, `tas25xx-regbin-parser.c:1995`). The TAS `IVSENSE_STATUS_*` controls are cached `KCNTR` values (#83 P6). | source + inference |
| K6 | The kernel TAS `ALGO` protection path is not compiled in the built or stock module (#83 P1). Linux has only fault-IRQ reaction from the factory tuning (#83 P5). | #83 |
| K7 | The only protection-related comment in the driver is `/* TODO: temporary compr offload volume set to protect speaker*/` before a volume set on compress-offload open (`alsa/aoc_alsa_compr.c:382-383`). It shows the authors wanted a driver-side volume guard on one path. It says nothing about what AoC firmware does without HAL setup. | source (weak) |
| K8 | The AoC sysfs ABI document lists only AoC status and notification attributes. It has no audio or protection entry. `aoc_audio_state` (#83 P4) reports service online/offline only. | source |
| K9 | A Linux precedent exists: Asahi's `speakersafetyd` is a user-space Rust daemon that models voice-coil temperature from TAS2764 V/I sense and caps gain. Its README says only Apple Silicon is supported. It depends on measured per-speaker Thiele/Small and thermal parameters. Code after v2.0.2 is GPL-2.0, while earlier versions are MIT. | external |

## What is not known (the GO blockers)

1. **AoC behaviour without HAL setup.** No public source says whether the AoC
   speaker chain bypasses protection, runs an uncalibrated default, or mutes
   when `InitSpeakerProtection`/`LoadCalibrationDataToAoC` (#83 P3, strings
   only) have not run. K1 shows only that the IPC names the module, not its
   default state.
2. **A protection-active signal.** No control, sysfs, procfs, or debugfs
   entry reports `ASP_ID_PROT` state (K2–K4, K8). K2 is the only candidate
   query, and its reply semantics are undocumented.
3. **Speaker-chain gain.** The AoC sink gain, EQ and DRC after the PCM front
   end are unknown (#83 P4). A digital stimulus level therefore cannot bound
   the amplifier output.
4. **Speaker model parameters.** Linux-side protection needs the speaker's
   thermal and excursion limits. They are not public. The per-unit
   `speaker.cal` is out of scope by data policy.

Reasoning from strings or from the shape of the IPC structs cannot fill these
gaps. This review stops at that boundary as the issue requires.

## Integration routes evaluated

| Route | What it would take | Verdict |
| --- | --- | --- |
| R1. Trust AoC defaults | Play through `TDM_0_RX Mixer EP1` and assume the firmware protects the speaker. | **Rejected.** Blocker 1 is unresolved. |
| R2. Read-only kernel query (target patch) | A target-local patch under `kernel-patches/private/google-modules/aoc/` (precedent: the gpu and wlan patches) adds a read-only debugfs or sysfs file. It sends `CMD_AUDIO_OUTPUT_GET_MODULES` for `blk=ABLOCK_SPEAKER, module=ASP_ID_PROT` and prints the raw `val`. The struct comes from the GPL header (K2), not from guesswork. | **Conditional, research only.** It cannot give GO alone because `val`'s meaning is undocumented. It becomes useful only with a vendor statement of the meaning, or a differential observation that shows `val` tracks protection state. It still needs its own issue, a patch review, and a decision that a GET IPC is safe on a running chain. |
| R3. Reimplement the HAL's AoC configuration | Re-create `InitSpeakerProtection`/`LoadCalibrationDataToAoC` over `/dev/acd-*`, using `ti_sp_tuning.bin` and per-unit `speaker.cal`. | **Rejected.** The only sources are closed-binary strings (K4, #83 P3). This would reimplement a vendor protocol by guesswork. It also needs per-unit calibration data, which `AGENTS.md` and the stage policy exclude. |
| R4. Linux-side protection (speakersafetyd-style) | Capture `TDM_0_TX` through an EP TX route (K5), run a thermal model, and enforce a gain cap through a project-owned interlock. | **Not viable now; long-term candidate.** It needs: proof that `TDM_0_TX` carries V/I and in which slots; speaker parameters that are not public (blocker 4); bounded AoC gain (blocker 3); confidence that AoC is not also processing the chain; and an interlock that fails muted. It is a new protection subsystem, so it needs a `plans/` decision and licence review (K9). |
| R5. Vendor or upstream statement | Ask Google for the documented default of the AoC speaker chain without the HAL, and the semantics of `GET_MODULES`. | **Owner action.** This is outward-facing, so the agent did not attempt it. A positive answer could resolve blockers 1–2, and R2 with it. |
| R6. Bluetooth A2DP output | Existing `pipewire-spa-bluez`; does not drive TAS. | **Recommended** for issue #2 output proof, as in #83. |

## Read-only protection-active indication: missing contract

None exists on this kernel. An indication needs at least all of these:

- a documented AoC query, or one with vendor-stated semantics, that reports
  whether `ASP_ID_PROT` is instantiated in `ABLOCK_SPEAKER`, has parameters
  loaded, and is engaged (not just present);
- an exposure path that cannot change state. It must read without a `put`,
  must not depend on a cached value, and must have defined behaviour while
  AoC is offline;
- ideally excursion or temperature telemetry, so that limiting is observable
  during a trial.

The existing observables are the ASP Mode enum (K3), `aoc_audio_state` (K8),
the cached TAS kcontrols (#83 P6), and the TAS IRQ counters (#83 P5). None of
them meets this contract. The IRQ counters stay useful only as
before-and-after fault evidence around a trial that is already justified.

## Design inputs for a future bounded internal speaker proof

No such proof may be designed for execution until blockers 1–3 are resolved
by route R2+R5 or by R4. When they are, the proof should at least:

- run only on the exact device after a fresh read-only preflight that also
  records the protection indication above and refuses unless it reads
  engaged;
- record the TAS IRQ counters, `TDM_0_RX Mixer EP1`, `TAS25XX ASI Switch`,
  `CODEC PROFILE`, and the AoC ASP mode before and after, and restore them
  with an EXIT trap (the same shape as the mic workflow);
- use a short, deterministic, low-level stimulus whose acoustic output is
  bounded by the resolved gain (blocker 3). Choosing a level by digital
  amplitude alone is not enough;
- capture the result with the host microphone or webcam, keep raw output in
  ignored `out/`, and never edit `capabilities.toml` from the workflow;
- need its own typed confirmation and a separate owner decision.

## Limitations

- Sources were read at the pinned revisions only. Google's later AoC releases
  were not reviewed.
- Kbuild builds the AoC module only for GS101, GS201, or ZUMA (`Kbuild:12`).
  `aoc-interface.h:12-20` picks the zuma header under `CONFIG_SOC_ZUMA`. That
  tegu builds with the zuma header is **inference**. It is consistent with
  the controls observed on the phone, but the effective config was not read.
- The `/dev/acd-*` service set on this device was not inventoried. That would
  be a future read-only `ls` and must not open any node.
- AoC firmware (`aoc.bin`) was not disassembled. That would be reverse
  engineering of proprietary bytes and is out of scope.
- Web searches on 2026-10-12 found no public Pixel/AoC speaker-protection
  documentation and no mainline or postmarketOS Tensor audio port.

## Next issue steps

1. Keep #90 open as **NO-GO / blocked on evidence**. Move #2 output work to a
   Bluetooth A2DP proof issue.
2. Owner decision on R5: whether to ask Google, publicly or through a
   partner channel, about the AoC speaker-chain default without the HAL and
   the `GET_MODULES` reply semantics.
3. Optional research issue for R2. Write the target-local read-only AoC
   query patch, with offline build and review only. Before any phone run,
   decide how a raw `val` would be interpreted. Without R5 it stays
   diagnostic and cannot give GO.
4. Optional `plans/` proposal for R4 (Linux-side protection). Scope V/I
   provenance, the speaker-parameter source, the fail-muted interlock, and
   `speakersafetyd` licence fit before any code.
