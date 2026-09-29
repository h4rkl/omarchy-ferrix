#!/bin/sh
set -eu

# Device-free contract test for native-builder.sh.  The fake Docker CLI never
# creates a container; it records arguments so this test can prove the wrapper
# cannot pass USB/ADB transport and rejects physical workflows.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script="$root/tools/pixel-9a/native-builder.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/bin" "$work/log"

cat >"$work/bin/docker" <<'EOF'
#!/bin/sh
set -eu
case "${1:-}" in
  info) printf '%s\n' linux/amd64 ;;
  build) printf '%s\n' "$*" >"${FAKE_DOCKER_LOG:?}" ;;
  image) printf '%s\n' 'pixel-9a-builder=sha256:test linux/amd64' ;;
  run) printf '%s\n' "$*" >>"${FAKE_DOCKER_LOG:?}" ;;
  *) printf 'unexpected docker verb: %s\n' "${1:-}" >&2; exit 1 ;;
esac
EOF
chmod 0755 "$work/bin/docker"
log="$work/log/docker"

PATH="$work/bin:$PATH" FAKE_DOCKER_LOG="$log" \
  FERRIX_PIXEL9A_BUILDER_IMAGE=ferrix-pixel9a-builder:test \
  "$script" image >/dev/null
grep -F -- '--platform linux/amd64' "$log" >/dev/null
grep -F -- 'containers/pixel-9a-builder' "$log" >/dev/null
if grep -Eq -- '(/dev/bus/usb|ADB_SERVER_SOCKET|--device)' "$log"; then
  echo 'builder image workflow exposed a physical transport' >&2
  exit 1
fi

: >"$log"
PATH="$work/bin:$PATH" FAKE_DOCKER_LOG="$log" \
  FERRIX_PIXEL9A_BUILDER_IMAGE=ferrix-pixel9a-builder:test \
  "$script" run kernel-radio preflight >/dev/null
grep -F -- '--mount type=bind,src=' "$log" >/dev/null
grep -F -- '/workspace' "$log" >/dev/null
if grep -Eq -- '(/dev/bus/usb|ADB_SERVER_SOCKET|--device)' "$log"; then
  echo 'builder run workflow exposed a physical transport' >&2
  exit 1
fi

if PATH="$work/bin:$PATH" FAKE_DOCKER_LOG="$log" "$script" run flash preflight >/dev/null 2>&1; then
  echo 'builder accepted a device-facing workflow' >&2
  exit 1
fi

printf '%s\n' '{"event":"pixel-9a-native-builder-test","status":"passed"}'
