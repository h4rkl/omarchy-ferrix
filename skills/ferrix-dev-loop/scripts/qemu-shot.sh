#!/bin/sh
# usage: qemu-shot.sh NAME [WAIT] -- capture the QEMU agent session framebuffer; prints a 900px PNG path.
# Needs a running just qemu-agent-start session (honours FERRIX_OUT_DIR).
. "$(dirname -- "$0")/env.sh"
cd "$FERRIX_ROOT" || exit 1
sleep "${2:-3}"
just qemu-frame >/dev/null 2>&1 || { echo 'STOP: qemu-frame failed' >&2; exit 1; }
src="${FERRIX_OUT_DIR:-out}/qemu-aarch64/agent/frame.png"
sips -Z 900 "$src" --out "$FERRIX_SHOTS/$1.png" >/dev/null && echo "$FERRIX_SHOTS/$1.png"
