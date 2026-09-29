#!/bin/sh
set -eu

# Device-free contract test for native-builder.sh.  The fake Docker CLI never
# creates a container; it records arguments so this test can prove the wrapper
# cannot pass USB/ADB transport and rejects physical workflows.

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script="$root/tools/pixel-9a/native-builder.sh"
dockerfile="$root/containers/pixel-9a-builder/Dockerfile"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/bin" "$work/log"

# Zig 0.14.1 supplies the pinned compiler/linker/archive tools, but its
# objcopy rejects Kbuild's --prefix-symbols and it has no objdump/strip
# subcommands. Keep those binutils on the Debian LLVM implementation, and
# assert that clang/LLD remain Zig-backed.
for tool in llvm-objcopy llvm-objdump llvm-strip; do
  grep -F -- "exec /usr/bin/$tool \"\$@\"" "$dockerfile" >/dev/null || {
    echo "builder image does not route $tool through Debian LLVM" >&2
    exit 1
  }
done
grep -F -- 'exec python3 /usr/local/bin/ferrix-zig-clang.py "$@"' "$dockerfile" >/dev/null || {
  echo 'builder image no longer routes clang through the pinned Zig wrapper' >&2
  exit 1
}
grep -F -- 'exec /opt/zig/zig ld.lld "$@"' "$dockerfile" >/dev/null || {
  echo 'builder image no longer routes ld.lld through pinned Zig LLVM' >&2
  exit 1
}
if grep -F -- 'exec /opt/zig/zig objcopy' "$dockerfile" >/dev/null; then
  echo 'builder image still routes objcopy through unsupported Zig subcommand' >&2
  exit 1
fi
if grep -F -- 'exec /opt/zig/zig objdump' "$dockerfile" >/dev/null; then
  echo 'builder image still routes objdump through unsupported Zig subcommand' >&2
  exit 1
fi
if grep -F -- 'exec /opt/zig/zig strip' "$dockerfile" >/dev/null; then
  echo 'builder image still routes strip through unsupported Zig subcommand' >&2
  exit 1
fi

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
