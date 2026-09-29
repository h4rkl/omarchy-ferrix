#!/bin/sh
set -eu

# Runtime guard for the target-scoped builder.  This image is never a device
# transport environment: no ADB/Fastboot packages, USB nodes, or host sockets
# are installed or mounted by the checked-in wrapper.
fail() {
  printf '{"evidence":"physical:google-pixel-9a-tegu-gxq96-mp1-0","event":"pixel-9a-builder-failed","reason":"%s","detail":"%s"}\n' "$1" "$2" >&2
  exit 2
}

[ "$(uname -s)" = Linux ] || fail wrong-host-os "the Pixel builder requires Linux"
[ "$(uname -m)" = x86_64 ] || fail wrong-host-arch "the Pixel builder requires native x86_64 Linux"

if [ "${FERRIX_PIXEL9A_PRIVILEGED:-0}" = 1 ]; then
  [ -e /dev/loop-control ] || fail no-loop-control "rootfs builds need Docker privileged loop access"
fi

mkdir -p /workspace /cache /out
chown ferrix-builder:ferrix-builder /workspace /cache /out 2>/dev/null || true

if [ "$(id -u)" = 0 ]; then
  exec runuser --preserve-environment --user ferrix-builder -- "$@"
fi
exec "$@"
