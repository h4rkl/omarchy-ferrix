#!/bin/sh
# usage: phone-shot.sh NAME [SCALE] -- grab the scanned-out DRM framebuffer to a PNG; prints the path.
# Scale 3 gives 360x808. This shows what the compositor scanned out; if you
# suspect the panel shows something else (byte order, blank, wrong mode) use webcam.sh.
. "$(dirname -- "$0")/env.sh"
out="$FERRIX_SHOTS/$1"
phone "python3 /tmp/fbgrab.py /tmp/fb.ppm ${2:-3} --reversed >/dev/null 2>&1" 60 >/dev/null
python3 "$DEVTEST/shell-get.py" /tmp/fb.ppm "$out.ppm" >/dev/null || exit 1
sips -s format png "$out.ppm" --out "$out.png" >/dev/null && echo "$out.png"
