#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
runtime=${OMARCHY_OUT_DIR:-"$root/out"}/qemu-aarch64/agent
pid_file="$runtime/qemu.pid"
if [ -f "$pid_file" ] && kill -0 "$(cat "$pid_file")" 2>/dev/null; then
  printf 'QEMU agent session: running (pid %s)\n' "$(cat "$pid_file")"
  printf 'QMP socket: %s\nserial log: %s\nframebuffer: %s\n' "$runtime/qmp.sock" "$runtime/serial.log" "$runtime/frame.png"
else
  echo "QEMU agent session: stopped"
  exit 1
fi
