#!/bin/sh
# Bounded, headless, TCG-only run of every checked-in scenario (ADR-006).
#
#   1. require the guest artifacts from `just qemu-build` (nothing is built
#      for the guest here; only the host runner binary is compiled);
#   2. start a headless agent session (FERRIX_QEMU_GRAPHICS=none, TCG);
#   3. wait for `telephonyd-ready` and `networkd-ready` on serial;
#   4. run every targets/qemu-aarch64/scenarios/*.toml with ferrix-scenario;
#   5. stop the session and emit one summary record.
#
# FERRIX_QEMU_SCENARIO_TIMEOUT (seconds, default 600) bounds the whole run;
# FERRIX_QEMU_SCENARIO_FRAME=1 also captures a framebuffer per scenario.
# Offline: the smoke guest has no NIC; nothing here needs a network. Evidence
# label: QEMU. Never a physical device.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64
runtime="$out/agent"
scenarios="$root/targets/qemu-aarch64/scenarios"
timeout=${FERRIX_QEMU_SCENARIO_TIMEOUT:-600}
case "$timeout" in
  ""|*[!0-9]*|0) echo "invalid FERRIX_QEMU_SCENARIO_TIMEOUT: $timeout (positive integer seconds)" >&2; exit 2 ;;
esac
start=$(date +%s)
elapsed() { echo $(( $(date +%s) - start )); }
started_session=0
fail() {
  printf '{"evidence":"QEMU","event":"scenario-check-failed","reason":"%s","seconds":%s,"serial_log":"%s"}\n' "$1" "$(elapsed)" "$runtime/serial.log" >&2
  if [ "$started_session" = 1 ]; then
    "$root/targets/qemu-aarch64/scripts/qemu-agent-stop.sh" >/dev/null 2>&1 || true
  fi
  [ ! -f "$runtime/serial.log" ] || { echo "--- serial log ($runtime/serial.log) ---" >&2; cat "$runtime/serial.log" >&2; }
  exit 1
}
[ -f "$out/Image" ] && [ -f "$out/initramfs.cpio.gz" ] || fail "missing-artifacts-run-qemu-build"
command -v python3 >/dev/null || fail "missing-python3"
files=$(ls "$scenarios"/*.toml 2>/dev/null || true)
[ -n "$files" ] || fail "no-scenario-files"
if [ -f "$runtime/qemu.pid" ] && kill -0 "$(cat "$runtime/qemu.pid")" 2>/dev/null; then
  fail "agent-session-already-running"
fi
target_dir=${CARGO_TARGET_DIR:-"$root/target"}
cargo build -q --manifest-path "$root/Cargo.toml" -p ferrix-scenario || fail "ferrix-scenario-build-failed"
runner="$target_dir/debug/ferrix-scenario"
[ -x "$runner" ] || fail "ferrix-scenario-binary-missing"
# Validate every file before touching QEMU: a schema error is a fast failure.
"$runner" --validate $files >/dev/null || fail "scenario-schema-invalid"

started_session=1
FERRIX_QEMU_ACCEL=tcg FERRIX_QEMU_GRAPHICS=none "$root/targets/qemu-aarch64/scripts/qemu-agent-start.sh" || fail "agent-session-start-failed"
wait_for() {
  while ! grep -q -- "$1" "$runtime/serial.log" 2>/dev/null; do
    kill -0 "$(cat "$runtime/qemu.pid")" 2>/dev/null || fail "qemu-exited-before-$2"
    [ "$(elapsed)" -lt "$timeout" ] || fail "timeout-waiting-for-$2"
    sleep 0.2
  done
}
wait_for '"event":"telephonyd-ready"' telephonyd-ready
wait_for '"event":"networkd-ready"' networkd-ready
frame=
[ "${FERRIX_QEMU_SCENARIO_FRAME:-0}" != 1 ] || frame=--frame
passed=0
failed=0
failed_names=
for file in $files; do
  name=$(basename "$file" .toml)
  [ "$(elapsed)" -lt "$timeout" ] || fail "timeout-before-$name"
  if "$runner" --guest --out "$out" --sockets "$runtime" --serial "$runtime/serial.log" \
       --qmp-helper "$root/targets/qemu-aarch64/scripts/qmp.py" $frame "$file"; then
    passed=$((passed + 1))
  else
    failed=$((failed + 1))
    failed_names="$failed_names${failed_names:+,}\"$name\""
  fi
done
"$root/targets/qemu-aarch64/scripts/qemu-agent-stop.sh" >/dev/null 2>&1 || true
started_session=0
if [ "$failed" -eq 0 ]; then
  printf '{"evidence":"QEMU","event":"scenario-check-passed","passed":%s,"failed":0,"seconds":%s,"evidence_dir":"%s"}\n' "$passed" "$(elapsed)" "$out/scenarios"
else
  printf '{"evidence":"QEMU","event":"scenario-check-failed","reason":"scenarios-failed","passed":%s,"failed":%s,"failed_names":[%s],"seconds":%s,"evidence_dir":"%s"}\n' "$passed" "$failed" "$failed_names" "$(elapsed)" "$out/scenarios" >&2
  exit 1
fi
