# Synthetic audio inventory fixtures (mock)

Canned guest-probe output for `tools/pixel-9a/tests/audio-inventory-workflow.sh`.
Every value is synthetic and labelled mock: the card names, module names,
Bluetooth MAC, serial, cookie, and host name exist only to exercise parsing
and redaction and were never read from a device. Nothing here is
physical-target evidence for `google-pixel-9a-tegu-gxq96-mp1-0`.

- `guest-tegu.txt` - verified identity with ALSA, an online AoC DSP,
  PipeWire, and Ferrix state.
- `guest-minimal.txt` - verified identity where every interface is absent.
The test derives probe-failure, unreachable-graph, degraded-adapter, and AoC
(offline with or without a firmware control, unreadable revision, missing
fields) variants from `guest-tegu.txt` in a temporary directory.
- `guest-qemu.txt`, `guest-mismatch.txt` - probe refusals (exit 5 / 4).
- `guest-forged.txt` - exit 0 but a near-match model; the host must refuse.
- `guest-truncated.txt` - verified identity without `probe_complete=yes`.
