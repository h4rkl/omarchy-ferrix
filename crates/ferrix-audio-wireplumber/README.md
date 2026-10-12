# Ferrix WirePlumber audio adapter

This target-neutral adapter implements `ferrix-hardware-api::Audio` through
WirePlumber's maintained `wpctl` control surface. It does not open ALSA,
Bluetooth, or vendor audio devices and does not implement PipeWire protocols.
Build the production transport with `--features production`; host tests use a
deterministic in-memory transport.

The adapter treats WirePlumber's available sink nodes as routes and the
default sink as active. `set_audio_route` selects an available representative
sink with `wpctl set-default`, then re-reads WirePlumber and returns only the
authoritative result. Sink properties and names are classified into Ferrix's
coarse speaker, earpiece, headphones, and Bluetooth routes.

The default sink's mixer level is read with `wpctl get-volume <id>` and
reported as `AudioState::output` (`volume_percent`, `muted`). That is
WirePlumber's reported volume, clamped to 0–100. A node with no
mixer volume reports `output: None`, and the volume and mute setters then
return `unsupported`. `set_audio_volume` runs `wpctl set-volume <id> <0.00–1.00>
--limit 1.0` and `set_audio_muted` runs `wpctl set-mute <id> 1|0`, always on the
current default sink's numeric ID. Each write is re-read; a level WirePlumber
did not apply is a typed `failed` error, and the reported state is kept.

## Interface limitations

- `wpctl set-default` controls where new streams are routed. It does not move
  streams that are already running; call/session policy must do that through a
  separate audio-session integration.
- Some mobile ALSA UCM configurations expose speaker and earpiece as ports on
  one sink instead of distinct sink nodes. `wpctl` cannot select those ports,
  so this adapter reports only routes exposed as distinct sinks. A future
  maintained WirePlumber API for port selection should replace this boundary
  rather than adding ALSA or PipeWire protocol code here.
- Route classification uses standard PipeWire properties first and stable
  node/description tokens second. Unclassifiable sinks are ignored. Targets
  must validate their WirePlumber graph before claiming audio support.
- Change events are derived by polling the authoritative WirePlumber snapshot;
  duplicate states are suppressed. A temporary command failure retains the
  last known state because the hardware API has no unavailable audio state.
- Volume and mute apply to the default *sink* only. Microphone (source)
  capture mute and in-call uplink mute are not implemented here.
- A level is a WirePlumber mixer value, not proof of audible output. A target
  must still prove speaker playback before claiming audio support.
