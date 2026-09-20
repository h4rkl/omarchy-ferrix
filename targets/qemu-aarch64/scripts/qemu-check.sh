#!/bin/sh
# Bounded, headless, TCG-only smoke check: the CI contract.
#
#   1. boot the generic AArch64 virt guest with serial captured to a file;
#   2. wait for the persistent ferrix-core's `boot-health` record and the
#      init-emitted QEMU `boot-health` that follows it (no fallbacks exist);
#   3. assert the mock is still *unplugged*, then send the unplugged->charging
#      command over the REAL host->guest mock-control channel;
#   4. wait for the `power-changed` record ferrix-core emits;
#   5. ask the guest to shut down over the same channel and wait for QEMU to
#      exit cleanly.
#
# Every failure prints a machine-readable QEMU record and the serial log.
# Evidence is QEMU/mock; nothing here touches a physical device.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64
log="$out/serial.log"
control="$out/control.sock"
mock="$root/targets/qemu-aarch64/scripts/mock_control.py"
# Bounded wait for the whole check, in seconds (0.1 s polling ticks). CI may
# raise it because TCG on a hosted runner is slower than a local host.
timeout=${FERRIX_QEMU_CHECK_TIMEOUT:-90}
case "$timeout" in
  ""|*[!0-9]*|0) echo "invalid FERRIX_QEMU_CHECK_TIMEOUT: $timeout (positive integer seconds)" >&2; exit 2 ;;
esac
command -v python3 >/dev/null || { echo "missing python3; needed for the mock-control client" >&2; exit 2; }
mkdir -p "$out"
: > "$log"

qemu_pid=
fail() {
  reason=$1
  printf '{"evidence":"QEMU","event":"check-failed","reason":"%s"}\n' "$reason" >&2
  if [ -n "$qemu_pid" ] && kill -0 "$qemu_pid" 2>/dev/null; then
    kill "$qemu_pid" 2>/dev/null || true
    wait "$qemu_pid" 2>/dev/null || true
  fi
  echo "--- serial log ($log) ---" >&2
  cat "$log" >&2
  exit 1
}

deadline_ticks=$((timeout * 10))
tick=0
# wait_for PATTERN NAME: poll the serial log until PATTERN matches or the
# shared deadline passes. QEMU exiting early is also a failure.
wait_for() {
  while ! grep -q -- "$1" "$log" 2>/dev/null; do
    kill -0 "$qemu_pid" 2>/dev/null || fail "qemu-exited-before-$2"
    [ "$tick" -lt "$deadline_ticks" ] || fail "timeout-waiting-for-$2"
    tick=$((tick + 1))
    sleep 0.1
  done
}

FERRIX_QEMU_ACCEL=tcg FERRIX_QEMU_GRAPHICS=off FERRIX_QEMU_MODE=check FERRIX_QEMU_CONTROL_SOCKET="$control" \
  "$root/targets/qemu-aarch64/scripts/qemu-run.sh" >"$log" 2>&1 &
qemu_pid=$!

# Both records are required: ferrix-core's own (evidence mock) and init's
# QEMU-labelled one, which init may only emit after the core reported.
wait_for '"evidence":"mock","event":"boot-health","status":"ok"' core-boot-health
wait_for '"evidence":"QEMU","event":"boot-health","status":"ok"' guest-boot-health
wait_for '"event":"mock-control","status":"ready"' mock-control-ready
if grep -q '"charging":true' "$log"; then
  fail "charging-true-before-transition"
fi

# Host-side records go to stdout, never into the serial log QEMU is writing.
ack=$(python3 "$mock" --socket "$control" --timeout 30 set-power charging=true) || fail "set-power-command-rejected"
printf '{"evidence":"QEMU","event":"mock-command-sent","cmd":"set-power","ack":%s}\n' "$ack"
case "$ack" in
  *'"status":"ok"'*'"charging":true'*) ;;
  *) fail "set-power-ack-mismatch" ;;
esac
wait_for '"event":"power-changed","battery_percent":[0-9]*,"charging":true' power-changed

# The guest powers off right after accepting; the ack may not make it back,
# so the serial record and a clean QEMU exit are the assertions here.
if ack=$(python3 "$mock" --socket "$control" --timeout 30 shutdown); then
  printf '{"evidence":"QEMU","event":"mock-command-sent","cmd":"shutdown","ack":%s}\n' "$ack"
else
  printf '{"evidence":"QEMU","event":"mock-command-sent","cmd":"shutdown","ack":null}\n'
fi
wait_for '"event":"shutdown","status":"accepted"' shutdown-accepted
while kill -0 "$qemu_pid" 2>/dev/null; do
  [ "$tick" -lt "$deadline_ticks" ] || fail "timeout-waiting-for-poweroff"
  tick=$((tick + 1))
  sleep 0.1
done
status=0
wait "$qemu_pid" || status=$?
cat "$log"
[ "$status" -eq 0 ] || fail "qemu-exit-$status"
printf '{"evidence":"QEMU","event":"check-passed","transition":"unplugged-to-charging","channel":"virtio-serial ferrix.control","seconds":%s}\n' "$((tick / 10))"
