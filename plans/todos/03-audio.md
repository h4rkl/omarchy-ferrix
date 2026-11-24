# 03 — Audio: speaker, earpiece, microphone, call audio

Status: open · Issues: #2 (parent), #94 (speaker-protection contract) ·
Depends on: 00 (the AoC firmware route on `pixel-9a-gpu` lands first) ·
Feeds: 02, 10 (Android audio), 14

## Goal

Speaker playback, earpiece, headset/USB-C, Bluetooth and microphone capture
work through PipeWire. Ferrix exposes volume, mute and route through
`ferrix-hardware-api` via `ferrix-audio-wireplumber`, and real uplink mute
works in calls.

## Where it stands

- Physical: the PipeWire and WirePlumber services run supervised. The AoC
  DSP starts and the ALSA card binds (card 0 `google,aoc-snd-card`, 35 PCMs,
  440 mixer controls); playback is silent. One controlled-tone mic proof
  picked up the tone on channel 0 only. Bluetooth A2DP tone playback was
  heard by the owner (#91 closed).
- Blocked (NO-GO): the internal speaker and earpiece. The TAS25xx
  protection algorithm is not compiled in. AoC protection without Android's
  audio HAL setup, and the output gain range, are unknown (#83, #90, #94).
  A low PCM level cannot bound the analogue gain.
- Done (mock): typed output volume/mute and Settings > Sound.

## Deliverables

1. **The #94 contract.** With owner authorisation to contact upstream or
   Google, get:
   - the AoC speaker-chain default without HAL initialisation;
   - the `CMD_AUDIO_OUTPUT_GET_MODULES` semantics for
     `ASP_ID_PROT` / `ABLOCK_SPEAKER`;
   - a protection-active signal;
   - the gain range.
   If no external contract is available, assess a Linux-side protection
   route from licensed source and speaker parameters you are allowed to
   use: V/I sense provenance, a fail-muted gain interlock, and
   thermal/excursion limits. End with a source-linked GO/NO-GO for
   designing a bounded speaker trial.
2. **Bounded internal-output workflow (only after GO).** A checked-in recipe
   with a preflight, a hard gain cap, short duration, protection-active
   verification, and automatic mute on any anomaly. One owner-approved run
   per decision. Do the earpiece separately, after the speaker.
3. **ALSA UCM profile** for tegu under `pmos/device-google-tegu/`, packaged
   through the APKBUILD:
   - HiFi: speaker, headset, USB-C;
   - VoiceCall: earpiece, speaker, headset;
   - mic selection: main, secondary, headset.
   No board names in product code.
4. **Microphone.** Fix the channel-1 zero result: pick the right PCM and
   mixer path for each mic. Prove capture is non-silent and of usable
   quality with a recorded file (kept out of Git).
5. **Adapter wiring.** Add `ferrix-audio-wireplumber` to the Pixel adapter
   composition for volume, mute, the route list, and route switching
   (speaker / earpiece / wired / Bluetooth).
6. **Bluetooth profiles.** HFP/HSP through PipeWire's BlueZ plugin for call
   audio, plus the A2DP that already works. Route change when a device
   connects or disconnects.
7. **Call audio and uplink mute.** Route modem voice PCM through the AoC
   voice path once spec 01 reaches call state. `set-mic-muted` drives real
   capture mute, and the typed result replaces ModemManager's `unsupported`.
8. **Android audio.** Waydroid's pipewire-pulse stream reaches a real sink.
   Test video playback in Jelly with sound.
9. **Jack and route events.** Plug-in and unplug of USB-C audio and
   Bluetooth produce typed route-change events, and the UI follows them.

## Acceptance

- The Settings volume control audibly changes speaker output in a bounded,
  approved run.
- Mic capture is non-silent on every mic path that is claimed.
- The adapter reports the true route and state. `[audio]` is promoted only
  for the routes that have proof.
- QEMU/mock audio is unchanged and `just qemu-check` passes.

## Owner-gated

Any internal speaker or earpiece output, active mic capture runs, any
external contact for #94, and the capability promotion.

## Risks

- If #94 stays NO-GO, the phone has no speakerphone or earpiece. Calls
  would need wired or Bluetooth audio only, and spec 14 must say so.
