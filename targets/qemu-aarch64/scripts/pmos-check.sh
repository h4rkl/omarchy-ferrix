#!/bin/sh
# Bounded, headless, TCG-only smoke check of the postmarketOS guest. Verifies
# the exported artifacts against manifest.json, boots with the serial console
# captured to a file, and requires the project readiness record
# ({"event":"pmos-boot-health","status":"ok"}) followed by a clean poweroff.
# Evidence label: QEMU. Never a physical-device claim.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
out=${FERRIX_OUT_DIR:-"$root/out"}/qemu-aarch64/pmos
# OPT-IN Android runtime image lives beside the default one (pmos-build.sh).
android=${FERRIX_PMOS_ANDROID:-0}
case "$android" in 0|1) ;; *) echo "invalid FERRIX_PMOS_ANDROID: $android (0|1)" >&2; exit 2 ;; esac
[ "$android" != 1 ] || out="$out-android"
timeout=${FERRIX_PMOS_CHECK_TIMEOUT:-600}
log="$out/check/serial.log"
mkdir -p "$out/check"

fail() {
  printf '{"evidence":"QEMU","event":"pmos-check-failed","reason":"%s","serial_log":"%s"}\n' "$1" "$log" >&2
  exit 1
}

[ -f "$out/manifest.json" ] || fail "missing-manifest"
python3 - "$out" <<'PY' || fail "artifact-hash-mismatch"
import hashlib, json, os, sys
out = sys.argv[1]
manifest = json.load(open(os.path.join(out, "manifest.json")))
ok = True
for name in ("vmlinuz", "initramfs", "qemu-aarch64.img"):
    entry = manifest["artifacts"].get(name)
    path = os.path.join(out, name)
    if not entry or not os.path.exists(path):
        print(f"missing artifact or manifest entry: {name}", file=sys.stderr)
        ok = False
        continue
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    if h.hexdigest() != entry["sha256"]:
        print(f"sha256 mismatch for {name}", file=sys.stderr)
        ok = False
sys.exit(0 if ok else 1)
PY

start=$(date +%s)
FERRIX_QEMU_ACCEL=tcg FERRIX_QEMU_GRAPHICS=off FERRIX_QEMU_MODE=check \
  "$root/targets/qemu-aarch64/scripts/pmos-run.sh" >"$log" 2>&1 &
qemu_pid=$!
ticks=$((timeout * 2))
while [ "$ticks" -gt 0 ]; do
  kill -0 "$qemu_pid" 2>/dev/null || break
  sleep 0.5
  ticks=$((ticks - 1))
done
status=0
if kill -0 "$qemu_pid" 2>/dev/null; then
  kill "$qemu_pid" 2>/dev/null || true
  status=124
fi
wait "$qemu_pid" || { [ "$status" -ne 0 ] || status=$?; }
elapsed=$(( $(date +%s) - start ))
cat "$log"
[ "$status" -ne 124 ] || fail "timeout-after-${timeout}s"
[ "$status" -eq 0 ] || fail "qemu-exit-$status"
grep -q '"event":"pmos-boot-health"' "$log" || fail "missing-pmos-boot-health"
grep '"event":"pmos-boot-health"' "$log" | grep -q '"status":"ok"' || fail "pmos-boot-health-not-ok"
grep -q '"event":"product-core-ready"' "$log" || fail "missing-product-core-ready"
grep -q '"event":"pmos-check-poweroff"' "$log" || fail "missing-pmos-check-poweroff"
if [ "$android" = 1 ]; then
  # The Waydroid image must at least report the Binder device nodes and its
  # init/container outcome; the records are reprinted so the check output
  # carries them. Nothing here claims the runtime renders anything.
  grep -q '"event":"android-binder-devices"' "$log" || fail "missing-android-binder-devices"
  grep -q '"event":"waydroid-init"' "$log" || fail "missing-waydroid-init"
  grep -E '"event":"(android-binder-devices|waydroid-init|waydroid-properties|waydroid-container|android-app-staged)"' "$log"
fi
printf '{"evidence":"QEMU","event":"pmos-check-passed","accel":"tcg","android":%s,"seconds":%s,"serial_log":"%s"}\n' "$android" "$elapsed" "$log"
