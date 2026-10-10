#!/bin/sh
# Device-free test for the target-local AoC firmware loader. It sources the
# OpenRC service against a synthetic sysfs/firmware tree; no host /sys or
# /lib/firmware path is read or written.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
service="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/ferrix-aoc-firmware.initd"
[ -f "$service" ] || { echo 'AoC firmware service is missing' >&2; exit 1; }
sh -n "$service"

fail() {
	echo "aoc-firmware-runtime: $*" >&2
	exit 1
}

# The loader must only touch the AoC platform device and the firmware tree.
grep -q '/sys/devices/platform/17000000.aoc' "$service" || fail 'service does not name the Tegu AoC device'
if grep -Eq '\b(adb|fastboot|dd|mkfs|flash|insmod|modprobe|rmmod)\b|/persist|amixer|alsactl' "$service"; then
	fail 'service reaches beyond the AoC firmware write'
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-aoc-firmware-test.XXXXXX")
helper=
trap '[ -z "$helper" ] || kill "$helper" 2>/dev/null || :; rm -rf "$work"' EXIT HUP INT TERM

ebegin() { :; }
eend() { return "${1:-0}"; }
einfo() { printf 'info: %s\n' "$*" >>"$work/log"; }
eerror() { printf 'error: %s\n' "$*" >>"$work/log"; }
need() { :; }
before() { :; }
export FERRIX_AOC_DEVICE="$work/sys/aoc"
export FERRIX_AOC_FIRMWARE_ROOT="$work/lib/firmware"
export FERRIX_AOC_DEVICE_WAIT=3
export FERRIX_AOC_ONLINE_WAIT=30
. "$service"

reset() {
	rm -rf "$work/sys" "$work/lib" "$work/log"
	mkdir -p "$work/sys/aoc" "$work/lib/firmware"
	: >"$work/sys/aoc/firmware"
	printf 'Offline\n' >"$work/sys/aoc/revision"
	: >"$work/log"
}

# 1. No staged image: fail, and never write the attribute.
reset
if start; then fail 'loader succeeded without a staged aoc.bin'; fi
[ ! -s "$work/sys/aoc/firmware" ] || fail 'loader wrote the attribute without a staged image'
grep -q 'is not staged' "$work/log" || fail 'missing-image failure is not reported'

# 2. A symlinked image is not a reviewed staged file.
reset
printf 'fixture-aoc\n' >"$work/aoc-elsewhere.bin"
ln -s "$work/aoc-elsewhere.bin" "$work/lib/firmware/aoc.bin"
if start; then fail 'loader accepted a symlinked aoc.bin'; fi

# 3. The driver never exposed the attribute (aoc_core not probed).
reset
printf 'fixture-aoc\n' >"$work/lib/firmware/aoc.bin"
rm "$work/sys/aoc/firmware"
if start; then fail 'loader succeeded without the AoC firmware attribute'; fi
grep -q 'did not appear' "$work/log" || fail 'missing-attribute failure is not reported'

# 4. Already online (for example after an OpenRC restart): no reload.
reset
printf 'fixture-aoc\n' >"$work/lib/firmware/aoc.bin"
printf 'FW Revision : 0x1\nHW Revision : 0x2\n' >"$work/sys/aoc/revision"
start || fail 'loader failed on an online AoC'
[ ! -s "$work/sys/aoc/firmware" ] || fail 'loader reloaded firmware into an online AoC'
grep -q 'already online' "$work/log" || fail 'online AoC is not reported'

# 5. Offline and the DSP never answers: the write happens, then it fails.
reset
printf 'fixture-aoc\n' >"$work/lib/firmware/aoc.bin"
online_wait=2
if start; then fail 'loader succeeded although AoC stayed offline'; fi
online_wait=30
[ "$(cat "$work/sys/aoc/firmware")" = aoc.bin ] || fail 'loader did not write the exact image name'
grep -q 'did not come online' "$work/log" || fail 'offline timeout is not reported'

# 6. Offline, then the synthetic driver comes online after the write.
reset
printf 'fixture-aoc\n' >"$work/lib/firmware/aoc.bin"
(
	for _ in $(seq 1 100); do
		if [ "$(cat "$work/sys/aoc/firmware" 2>/dev/null)" = aoc.bin ]; then
			printf 'FW Revision : 0x1\nHW Revision : 0x2\n' >"$work/sys/aoc/revision"
			exit 0
		fi
		sleep 0.05
	done
) &
helper=$!
start || fail 'loader failed although AoC came online'
wait "$helper" || :
helper=
[ "$(cat "$work/sys/aoc/firmware")" = aoc.bin ] || fail 'loader did not write the exact image name'
grep -q 'AoC online: FW Revision' "$work/log" || fail 'online result is not reported'

echo 'AoC firmware runtime test passed'
