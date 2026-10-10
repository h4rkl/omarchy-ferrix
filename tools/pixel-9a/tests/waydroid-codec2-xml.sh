#!/bin/sh
# Offline test for tools/pixel-9a/waydroid-codec2-xml.py (synthetic XMLs, no
# stock bytes) and the opt-in gate of tools/pixel-9a/waydroid-codec2-overlay.sh.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
tool="$root/tools/pixel-9a/waydroid-codec2-xml.py"
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-codec2-xml.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
fail() { printf 'FAIL waydroid-codec2-xml-test: %s\n' "$*" >&2; exit 1; }
xml() { python3 -I "$tool" "$@"; }

# BOM, comments, settings and an include as in the stock files.
printf '\357\273\277<?xml version="1.0" encoding="utf-8" ?>\n<!-- c -->\n<MediaCodecs>
  <Settings><Setting name="supports-multiple-secure-codecs" value="true" /></Settings>
  <Decoders>
    <MediaCodec name="c2.vendor.avc.decoder" type="video/avc">
      <Limit name="size" min="32x32" max="7680x4320" />
      <Feature name="adaptive-playback" />
    </MediaCodec>
    <MediaCodec name="c2.vendor.avc.decoder.secure" type="video/avc">
      <Feature name="secure-playback" required="true" />
    </MediaCodec>
  </Decoders>
  <Encoders>
    <MediaCodec name="c2.vendor.avc.encoder" type="video/avc"><Limit name="size" min="32x32" max="4096x4096" /></MediaCodec>
  </Encoders>
</MediaCodecs>\n' >"$work/a.xml"
printf '<MediaCodecs>\n  <Include href="media_codecs_aosp_c2.xml" />\n  <Decoders>
    <MediaCodec name="c2.vendor.av1.decoder" type="video/av01"><Feature name="secure-playback" /></MediaCodec>
  </Decoders>\n</MediaCodecs>\n' >"$work/b.xml"
printf '<MediaCodecs><Decoders>
  <MediaCodec name="c2.vendor.avc.decoder" type="video/avc" update="true"><Limit name="measured-frame-rate-320x240" range="528-531" /></MediaCodec>
  <MediaCodec name="c2.vendor.avc.decoder.secure" type="video/avc" update="true"><Limit name="measured-frame-rate-320x240" range="1-2" /></MediaCodec>
</Decoders></MediaCodecs>\n' >"$work/perf.xml"

xml --rank 256 "$work/out.xml" "$work/a.xml" "$work/b.xml"
python3 -I - "$work/out.xml" <<'EOF' || fail "merged XML is wrong"
import sys
import xml.etree.ElementTree as ET
root = ET.parse(sys.argv[1]).getroot()
assert root.tag == "MediaCodecs"
assert [c.tag for c in root if c.tag is not ET.Comment] == ["Decoders", "Encoders"], [c.tag for c in root]
dec = root.find("Decoders").findall("MediaCodec")
assert [c.get("name") for c in dec] == ["c2.vendor.avc.decoder", "c2.vendor.av1.decoder"], dec
assert all(c.get("rank") == "256" for c in root.iter("MediaCodec"))
assert root.find("Settings") is None and root.find("Include") is None
assert not [f for f in root.iter("Feature") if f.get("name") == "secure-playback"]
assert dec[0].find("Limit").get("max") == "7680x4320"
assert root.find("Encoders/MediaCodec").get("name") == "c2.vendor.avc.encoder"
EOF
! grep -q 'secure"' "$work/out.xml" || fail "secure codec left"

# Performance updates: secure entries dropped, no rank on update entries.
xml "$work/perf-out.xml" "$work/perf.xml"
[ "$(grep -c '<MediaCodec ' "$work/perf-out.xml")" = 1 ] || fail "secure performance entry kept"
! grep -q 'rank=' "$work/perf-out.xml" || fail "rank on an update entry"
xml --rank 256 "$work/perf-out2.xml" "$work/perf.xml"
! grep -q 'rank=' "$work/perf-out2.xml" || fail "rank on an update entry"

# Refusals: a codec defined twice, a non-MediaCodecs document, malformed XML,
# nothing left after filtering.
! xml "$work/dup.xml" "$work/a.xml" "$work/a.xml" 2>/dev/null || fail "duplicate codec accepted"
printf '<Other/>\n' >"$work/other.xml"
! xml "$work/x.xml" "$work/other.xml" 2>/dev/null || fail "foreign document accepted"
printf '<MediaCodecs>\n' >"$work/broken.xml"
! xml "$work/x.xml" "$work/broken.xml" 2>/dev/null || fail "malformed XML accepted"
printf '<MediaCodecs><Decoders><MediaCodec name="c2.x.secure" type="video/avc"/></Decoders></MediaCodecs>\n' >"$work/only-secure.xml"
! xml "$work/x.xml" "$work/only-secure.xml" 2>/dev/null || fail "empty result accepted"

# The extraction script is opt-in and refuses without its token.
if FERRIX_OUT_DIR="$work/out" sh "$root/tools/pixel-9a/waydroid-codec2-overlay.sh" 2>"$work/gate.err"; then
  fail "overlay extraction ran without its opt-in token"
fi
grep -q 'opt-in\|missing prerequisite' "$work/gate.err" || fail "unexpected gate error: $(cat "$work/gate.err")"
[ ! -e "$work/out" ] || fail "overlay extraction wrote output without its opt-in token"
printf 'PASS waydroid-codec2-xml-test\n'
