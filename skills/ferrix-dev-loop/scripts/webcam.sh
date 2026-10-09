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
ffmpeg -hide_banner -loglevel error -y -f avfoundation -framerate 30 -video_size 1920x1080 \
	-i "$dev" -frames:v 1 "$out.jpg" </dev/null || exit 1
if [ -n "$crop" ]; then
	ffmpeg -hide_banner -loglevel error -y -i "$out.jpg" -vf "crop=$crop" "$out.crop.jpg" && echo "$out.crop.jpg"
else
	echo "$out.jpg"
fi
