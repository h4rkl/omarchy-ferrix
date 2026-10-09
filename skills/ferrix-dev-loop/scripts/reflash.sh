#!/bin/sh
# usage: reflash.sh IMAGE_DIR [WATCH_SECONDS]
# Reach Fastboot (from running Ferrix, Android, or Fastboot), flash slot b
# plus userdata through just pixel-9a-dev-flash, select b, reboot, and wait
# until OpenRC has settled. Everything goes through the checked-in gated
# workflows; this script only supplies their confirm tokens.
. "$(dirname -- "$0")/env.sh"
I=$1
watch=${2:-300}
cd "$FERRIX_ROOT" || exit 1

if ! fastboot devices | grep -q .; then
	if [ -e "$FERRIX_SHELL" ]; then
		phone 'sync; /usr/sbin/reboot-bootloader' 3 >/dev/null 2>&1
	else
		echo 'CONFIRM PIXEL 9A ENTER FASTBOOT' | just pixel-9a-enter-fastboot --confirm-enter 2>&1 |
			grep -q 'result=entered-fastboot' || { echo 'STOP: enter-fastboot failed'; exit 1; }
	fi
	n=0
	until fastboot devices | grep -q .; do
		n=$((n + 1))
		[ $n -gt 60 ] && { echo 'STOP: no Fastboot. Ask the human to hold Volume Down + Power.'; exit 1; }
		sleep 2
	done
fi

set -- boot_b="$I/boot.img" init_boot_b="$I/init_boot.img" vendor_boot_b="$I/vendor_boot.img" \
	vendor_kernel_boot_b="$I/vendor_kernel_boot.img" dtbo_b="$I/dtbo.img" vbmeta_b="$I/vbmeta.img" \
	userdata="$I/userdata.img"
token=$(just pixel-9a-dev-flash flash "$@" 2>&1 | sed -n 's/^confirm_token=//p')
[ -n "$token" ] || { just pixel-9a-dev-flash flash "$@" 2>&1 | tail -5; echo 'STOP: flash plan refused'; exit 1; }
just pixel-9a-dev-flash flash --confirm "$token" "$@" 2>&1 | grep -q '^result=flashed' || { echo 'STOP: flash failed'; exit 1; }

if [ "$(fastboot getvar current-slot 2>&1 | sed -n 's/^current-slot: *_*//p')" != b ]; then
	token=$(just pixel-9a-dev-flash set-active b 2>&1 | sed -n 's/^confirm_token=//p')
	just pixel-9a-dev-flash set-active --confirm "$token" b 2>&1 | grep -q '^result=slot-selected' ||
		{ echo 'STOP: set-active failed'; exit 1; }
fi
just pixel-9a-dev-flash reboot 2>&1 | grep -q '^result=rebooted' || { echo 'STOP: reboot failed'; exit 1; }

start=$(date +%s)
while :; do
	t=$(($(date +%s) - start))
	[ $t -ge "$watch" ] && { echo "OUTCOME: timeout after ${t}s"; exit 1; }
	fastboot devices | grep -q . && { echo 'OUTCOME: back in Fastboot (boot failed; slot b not marked successful)'; exit 1; }
	adb devices 2>/dev/null | awk 'NR>1&&NF' | grep -q . && { echo 'OUTCOME: fell back to stock Android'; exit 1; }
	if [ -e "$FERRIX_SHELL" ] && phone 'rc-status default | grep -c "ferrix-product.*\(started\|stopped\|crashed\)"' 4 2>/dev/null | grep -qx 1; then
		echo "OUTCOME: Ferrix up after ${t}s"
		phone 'uptime; rc-status default | grep -v " started"' 10
		exit 0
	fi
	sleep 4
done
