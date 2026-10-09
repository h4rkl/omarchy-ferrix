#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
. "$root/targets/qemu-aarch64/scripts/paths.sh"
pid_file="$runtime/qemu.pid"
if [ -S "$sock_agent/qmp.sock" ]; then
  python3 "$root/targets/qemu-aarch64/scripts/qmp.py" --socket "$sock_agent/qmp.sock" quit || true
fi
if [ -f "$pid_file" ]; then
  pid=$(cat "$pid_file")
  for _ in $(seq 1 50); do
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.1
  done
  kill -0 "$pid" 2>/dev/null && kill "$pid" 2>/dev/null || true
fi
rm -f "$pid_file" "$sock_agent/qmp.sock"
echo "QEMU agent session stopped"
