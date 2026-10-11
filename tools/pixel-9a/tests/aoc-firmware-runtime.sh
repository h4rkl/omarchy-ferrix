#!/bin/sh
# Device-free test for the target-local AoC firmware loader. It sources the
# OpenRC service against a synthetic sysfs/firmware tree; no host /sys or
# /lib/firmware path is read or written. `sleep` is replaced by a function
# that plays the driver: after the control names aoc.bin it may flip the
# synthetic revision to online, and it counts polls so the bounds are checked.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
service="$root/targets/boards/google-pixel-9a-tegu-gxq96-mp1-0/pmos/device-google-tegu/ferrix-aoc-firmware.initd"
[ -f "$service" ] || { echo 'AoC firmware service is missing' >&2; exit 1; }
sh -n "$service"

fail() {
	echo "aoc-firmware-runtime: $*" >&2
	exit 1
}

# The loader names only the Tegu AoC device and writes only its control.
grep -q '/sys/devices/platform/17000000.aoc' "$service" || fail 'service does not name the Tegu AoC device'
code=$(grep -v '^[[:space:]]*#' "$service")
if printf '%s\n' "$code" | grep -Eq '\b(adb|fastboot|dd|mkfs|flash|insmod|modprobe|rmmod|amixer|alsactl|alsaucm|tee)\b|/persist|speaker\.cal|calib|cs40l26|tas25|ucm'; then
	fail 'service reaches beyond the AoC firmware control'
fi
writes=$(printf '%s\n' "$code" | sed -e 's/2>\/dev\/null//g' -e 's/>\/dev\/null//g' -e 's/2>&1//g' | grep -n '>' | grep -v '"\$aoc_device/firmware"' || true)
[ -z "$writes" ] || fail "service writes outside the AoC firmware control: $writes"
if printf '%s\n' "$code" | grep -Eq '^[[:space:]]*need '; then
	fail 'service must only order itself; a hard dependency would stop fail-open boot'
fi
printf '%s\n' "$code" | grep -q 'before ferrix-pipewire ferrix-audio ferrix-core' ||
	fail 'service is not ordered before the audio session and ferrix-core'

work=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-aoc-firmware-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM

hash_of() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | awk '{ print $1 }'
	else
		shasum -a 256 "$1" | awk '{ print $1 }'
	fi
}

ebegin() { :; }
eend() { return "${1:-0}"; }
einfo() { printf 'info: %s\n' "$*" >>"$work/log"; }
eerror() { printf 'error: %s\n' "$*" >>"$work/log"; }
after() { :; }
before() { :; }
export FERRIX_AOC_DEVICE="$work/sys/aoc"
export FERRIX_PIXEL9A_FIRMWARE_ROOT="$work/guest"
export FERRIX_PIXEL9A_FIRMWARE_MANIFEST="$work/guest/usr/share/ferrix/pixel-9a/firmware-manifest.tsv"
export FERRIX_AOC_DEVICE_WAIT=3
export FERRIX_AOC_ONLINE_WAIT=5
. "$service"

manifest=$FERRIX_PIXEL9A_FIRMWARE_MANIFEST
# Not `image`: the sourced service uses that name itself.
staged="$work/guest/lib/firmware/aoc.bin"
control="$work/sys/aoc/firmware"
revision="$work/sys/aoc/revision"

polls=0
driver_boots=0
sleep() {
	polls=$((polls + 1))
	if [ "$driver_boots" = 1 ] && [ "$(cat "$control" 2>/dev/null)" = aoc.bin ]; then
		printf 'synthetic online revision\n' >"$revision"
	fi
}

manifest_row() {
	printf 'firmware\t%s\tvendor.img\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" \
		"$(wc -c <"$staged" | tr -d '[:space:]')" "$(hash_of "$staged")" >>"$manifest"
}

reset() {
	rm -rf "$work/sys" "$work/guest" "$work/log"
	mkdir -p "$work/sys/aoc" "$work/guest/lib/firmware" "$(dirname -- "$manifest")"
	: >"$control"
	printf 'Offline\n' >"$revision"
	printf 'synthetic-aoc-image\n' >"$staged"
	{
		printf 'schema_version\t1\n'
		printf 'target_id\tgoogle-pixel-9a-tegu-gxq96-mp1-0\n'
	} >"$manifest"
	manifest_row audio-aoc /firmware/aoc.bin lib/firmware/aoc.bin
	: >"$work/log"
	device_wait=3
	online_wait=5
	polls=0
	driver_boots=0
}

refuses() {
	label=$1
	message=$2
	if start; then fail "loader succeeded with $label"; fi
	grep -Fq "$message" "$work/log" || fail "$label: missing '$message' in: $(cat "$work/log")"
}

not_written() {
	[ ! -s "$control" ] || fail "loader wrote the control with $1"
}

# 1. Offline, reviewed image, the synthetic driver boots after the write.
reset
driver_boots=1
start || fail "loader failed although AoC came online: $(cat "$work/log")"
[ "$(cat "$control")" = aoc.bin ] || fail 'loader did not write exactly aoc.bin'
[ "$(wc -c <"$control" | tr -d '[:space:]')" -eq 7 ] || fail 'loader wrote more than the image name'
grep -q 'info: AoC reports online' "$work/log" || fail 'online result is not reported'
! grep -q 'synthetic online revision' "$work/log" || fail 'loader logged the raw revision text'

# 2. Already online (for example an OpenRC restart): no reload, no image check.
reset
printf 'synthetic online revision\n' >"$revision"
rm -f "$staged"
start || fail 'loader failed on an online AoC'
not_written 'an online AoC'
grep -q 'already reports online' "$work/log" || fail 'online AoC is not reported'

# 3. The driver never exposed the control: bounded wait, then failure.
reset
rm -f "$control"
refuses 'no firmware control' 'has no firmware control'
[ "$polls" -eq 3 ] || fail "control wait polled $polls times, expected the bound of 3"
[ ! -e "$control" ] || fail 'loader created the firmware control'

# 4. An unreadable or empty revision is not treated as Offline.
reset
: >"$revision"
refuses 'an empty revision' 'revision is unreadable'
not_written 'an empty revision'

# 5. No staged image.
reset
rm -f "$staged"
refuses 'no staged image' 'is not a staged regular file'
not_written 'no staged image'

# 6. A symlinked image is not a reviewed staged file.
reset
mv "$staged" "$work/aoc-elsewhere.bin"
ln -s "$work/aoc-elsewhere.bin" "$staged"
refuses 'a symlinked image' 'is not a staged regular file'
not_written 'a symlinked image'

# 7. Bytes changed after staging.
reset
printf 'synthetic-aoc-imagX\n' >"$staged"
refuses 'a tampered image' 'hash differs from the manifest'
not_written 'a tampered image'

# 8. Size changed after staging.
reset
printf 'x' >>"$staged"
refuses 'a resized image' 'byte count differs from the manifest'
not_written 'a resized image'

# 9. Missing manifest.
reset
rm -f "$manifest"
refuses 'no manifest' 'missing firmware manifest'
not_written 'no manifest'

# 10. A row from another controller or source path is not the reviewed row.
reset
{ printf 'schema_version\t1\n'; } >"$manifest"
manifest_row gpu-mali /firmware/aoc.bin lib/firmware/aoc.bin
refuses 'a row from another controller' 'is not the reviewed'
not_written 'a row from another controller'
reset
{ printf 'schema_version\t1\n'; } >"$manifest"
manifest_row audio-aoc /firmware/other.bin lib/firmware/aoc.bin
refuses 'a row from another source path' 'is not the reviewed'
not_written 'a row from another source path'

# 11. Duplicate or absent rows.
reset
manifest_row audio-aoc /firmware/aoc.bin lib/firmware/aoc.bin
refuses 'a duplicated row' 'exactly one lib/firmware/aoc.bin row'
not_written 'a duplicated row'
reset
{ printf 'schema_version\t1\n'; } >"$manifest"
refuses 'no row' 'exactly one lib/firmware/aoc.bin row'
not_written 'no row'

# 12. Offline and the DSP never answers: the write happens, then a bounded
# failure.
reset
refuses 'an AoC that stays offline' 'did not report online within the bound'
[ "$(cat "$control")" = aoc.bin ] || fail 'loader did not write the image name before timing out'
[ "$polls" -eq 5 ] || fail "online wait polled $polls times, expected the bound of 5"

# 13. The control rejects the write.
reset
rm -f "$control"
mkdir "$control"
refuses 'a rejected write' 'failed'

# 14. Malformed or unbounded waits are refused before any sysfs access.
reset
device_wait=601
refuses 'an unbounded wait' 'wait bounds must be integers'
not_written 'an unbounded wait'
reset
online_wait='5;id'
refuses 'a malformed wait' 'wait bounds must be integers'
not_written 'a malformed wait'

echo 'AoC firmware runtime test passed'
