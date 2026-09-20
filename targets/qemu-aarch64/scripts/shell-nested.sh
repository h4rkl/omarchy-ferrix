#!/bin/sh
# Run the complete nested product session: persistent core plus compositor.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64
profile=${FERRIX_QEMU_PROFILE:-"$root/targets/qemu-aarch64/profile.toml"}
socket=${FERRIX_CORE_SOCKET:-"$out/core.sock"}
mkdir -p "$out"

cargo build --manifest-path "$root/Cargo.toml" -p mobile-core -p mobile-shell
FERRIX_CORE_SOCKET="$socket" "$root/target/debug/mobile-core" "$profile" --control none &
core_pid=$!
cleanup() {
  kill "$core_pid" 2>/dev/null || true
  wait "$core_pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

remaining=100
while [ ! -S "$socket" ] && [ "$remaining" -gt 0 ]; do
  sleep 0.05
  remaining=$((remaining - 1))
done
[ -S "$socket" ] || { echo "mobile-core did not create $socket" >&2; exit 1; }

FERRIX_CORE_SOCKET="$socket" "$root/target/debug/mobile-shell" --nested --core "$socket" "$profile"
