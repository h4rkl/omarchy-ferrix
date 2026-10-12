#!/usr/bin/env python3
"""Write the issue #80 gated reference tone for the Pixel 9a microphone proof.

  mic-acoustic-tone.py --out FILE --seconds N

Writes a mono 16-bit PCM WAV at 24 kHz to FILE (which must not exist): a
1180 Hz sine at -20 dBFS peak, gated on for 200 ms and off for 200 ms, with
5 ms raised-cosine ramps so the gate edges carry no broadband click. One
400 ms period holds exactly 472 sine cycles, so the period repeats with
continuous phase. The output is deterministic for a given N; the host
workflow records its SHA-256, plays it on the host's default output device
at the operator's existing volume, and deletes it. Nothing here selects a
device or changes a volume. Python standard library only.

The gate (200/200 ms) and the frequencies must match tone_hz,
control_hz, tone_window_frames, and tone_period_windows in
mic-capture-proof-remote.sh: 8 windows of 2400 frames at 48 kHz.
"""
import argparse
import math
import os
import struct
import sys
import wave

RATE = 24000
TONE_HZ = 1180
LEVEL_DBFS = -20.0
ON_MS = 200
OFF_MS = 200
RAMP_MS = 5
MIN_SECONDS = 10
MAX_SECONDS = 140


def period_samples():
    on = RATE * ON_MS // 1000
    off = RATE * OFF_MS // 1000
    ramp = RATE * RAMP_MS // 1000
    peak = 32767 * 10 ** (LEVEL_DBFS / 20)
    out = []
    for i in range(on + off):
        if i >= on:
            out.append(0)
            continue
        gain = 1.0
        if i < ramp:
            gain = 0.5 - 0.5 * math.cos(math.pi * i / ramp)
        elif i >= on - ramp:
            gain = 0.5 - 0.5 * math.cos(math.pi * (on - 1 - i) / ramp)
        out.append(int(round(peak * gain * math.sin(2 * math.pi * TONE_HZ * i / RATE))))
    return struct.pack("<%dh" % len(out), *out)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True)
    parser.add_argument("--seconds", type=int, required=True)
    args = parser.parse_args()
    if not MIN_SECONDS <= args.seconds <= MAX_SECONDS:
        print("mic-acoustic-tone: --seconds must be %d-%d" % (MIN_SECONDS, MAX_SECONDS), file=sys.stderr)
        return 2
    period_ms = ON_MS + OFF_MS
    if (TONE_HZ * period_ms) % 1000:
        print("mic-acoustic-tone: the period must hold whole tone cycles", file=sys.stderr)
        return 2
    one = period_samples()
    periods = args.seconds * 1000 // period_ms
    fd = os.open(args.out, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "wb") as raw, wave.open(raw, "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(RATE)
        for _ in range(periods):
            wav.writeframes(one)
    return 0


if __name__ == "__main__":
    sys.exit(main())
