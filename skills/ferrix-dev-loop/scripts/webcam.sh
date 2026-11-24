#!/bin/sh
# usage: webcam.sh NAME [DEVICE] [CROP] -- photograph the physical phone with a host camera; prints the path.
# Use it when no framebuffer grab is possible or trustworthy: bootloader and
# kernel-panic screens, boots that never reach the bring-up shell, scan-out
# colour/byte-order bugs, panel power. List devices with:
#   ffmpeg -f avfoundation -list_devices true -i ""
# CROP is ffmpeg crop=W:H:X:Y for the region the phone sits in.
# macOS asks for camera permission the first time.
. "$(dirname -- "$0")/env.sh"
dev=${2:-${FERRIX_WEBCAM:-0}}
crop=${3:-${FERRIX_WEBCAM_CROP:-}}
out="$FERRIX_SHOTS/$1"
# Some UVC cameras (e.g. EMEET S800) reject ffmpeg's default yuv420p and
# then block forever, so ask for a packed format and bound the capture.
pix=${FERRIX_WEBCAM_PIXFMT:-uyvy422}
ffmpeg -hide_banner -loglevel error -y -f avfoundation -pixel_format "$pix" -framerate 30 \
	-video_size 1920x1080 -i "$dev" -frames:v 1 "$out.jpg" </dev/null &
pid=$!
n=0
while kill -0 "$pid" 2>/dev/null; do
	n=$((n + 1))
	[ $n -gt 20 ] && { kill -9 "$pid" 2>/dev/null; echo 'STOP: webcam capture timed out (camera permission or device index?)' >&2; exit 1; }
	sleep 1
done
wait "$pid" || exit 1
if [ -n "$crop" ]; then
	ffmpeg -hide_banner -loglevel error -y -i "$out.jpg" -vf "crop=$crop" "$out.crop.jpg" && echo "$out.crop.jpg"
else
	echo "$out.jpg"
fi
