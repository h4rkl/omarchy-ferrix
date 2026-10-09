#!/bin/sh
# Run one checked-in scenario against the running QEMU session.
#   just qemu-scenario incoming-call-answer-route-remote-hangup
#   just qemu-scenario wifi-captive --frame        # also capture the framebuffer
#   just qemu-scenario path/to/custom.toml
# Uses the agent session (out/qemu-aarch64/agent/) when its serial log exists,
# otherwise the plain qemu-run session directory. The runner is the host
# binary `ferrix-scenario` (crates/ferrix-scenario); evidence lands in
# out/qemu-aarch64/scenarios/<name>/ (result.json, log-excerpt.log,
# frame.png with --frame). Evidence label: QEMU. Never a physical device.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
. "$root/targets/qemu-aarch64/scripts/paths.sh"
scenarios="$root/targets/qemu-aarch64/scenarios"
[ "$#" -ge 1 ] || { echo "usage: qemu-scenario.sh NAME|FILE [--frame] [-- extra ferrix-scenario args]" >&2; echo "scenarios:" >&2; ls "$scenarios" | sed 's/\.toml$//; s/^/  /' >&2; exit 2; }
name=$1
shift
case "$name" in
  *.toml) file=$name ;;
  *) file="$scenarios/$name.toml" ;;
esac
[ -f "$file" ] || { echo "no scenario $name ($file)" >&2; exit 2; }
target_dir=${CARGO_TARGET_DIR:-"$root/target"}
cargo build -q --manifest-path "$root/Cargo.toml" -p ferrix-scenario
runner="$target_dir/debug/ferrix-scenario"
[ -x "$runner" ] || { echo "ferrix-scenario binary missing at $runner" >&2; exit 2; }
if [ -f "$runtime/serial.log" ]; then
  sockets=$sock_agent
  serial="$runtime/serial.log"
else
  sockets=$sock_run
  serial="$out/serial.log"
fi
[ -f "$serial" ] || { echo "no serial log at $serial; start a session first (just qemu-agent-start)" >&2; exit 2; }
exec "$runner" --guest --out "$out" --sockets "$sockets" --serial "$serial" \
  --qmp-helper "$root/targets/qemu-aarch64/scripts/qmp.py" "$@" "$file"
