#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
script="$root/tools/pixel-9a/enter-fastboot.sh"
[ -x "$script" ] || exit 1
sh -n "$script"

if grep -vE '^[[:space:]]*#' "$script" | grep -nE '^[[:space:]]*(adb|fastboot) .* (flashing|erase|format|flash|sideload|set_active|reboot fastboot)' >/dev/null; then
	echo 'enter-fastboot workflow contains a forbidden device mutation' >&2
	exit 1
fi
grep -F -- 'adb -s "$adb_serial" reboot bootloader' "$script" >/dev/null

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-enter-fastboot.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/bin" "$work/state"
calls="$work/calls.log"
serial='SYN-FASTBOOT-001'

cat >"$work/bin/adb" <<'STUB'
#!/bin/sh
printf 'adb\t' >>"$STUB_CALLS"
printf '%s\t' "$@" >>"$STUB_CALLS"
printf '\n' >>"$STUB_CALLS"
case "$1" in
devices)
	echo 'List of devices attached'
	[ -f "$STUB_STATE/rebooted" ] || [ "${STUB_START_FASTBOOT:-}" = 1 ] || printf '%s device usb:1\n' "$STUB_SERIAL"
	;;
-s)
	[ "$2" = "$STUB_SERIAL" ] || exit 99
	case "$3" in
	shell)
		[ "$4" = getprop ] || exit 99
		case "$5" in
		ro.product.manufacturer) echo Google ;;
		ro.product.model) echo 'Pixel 9a' ;;
		ro.product.device) echo tegu ;;
		ro.boot.hardware.revision) echo MP1.0 ;;
		ro.boot.hardware.sku) echo GXQ96 ;;
		ro.boot.slot_suffix) echo _a ;;
		*) exit 99 ;;
		esac
		;;
	reboot)
		[ "$4" = bootloader ] || exit 99
		: >"$STUB_STATE/rebooted"
		;;
	*) exit 99 ;;
	esac
	;;
*) exit 99 ;;
esac
STUB

cat >"$work/bin/fastboot" <<'STUB'
#!/bin/sh
printf 'fastboot\t' >>"$STUB_CALLS"
printf '%s\t' "$@" >>"$STUB_CALLS"
printf '\n' >>"$STUB_CALLS"
case "$1" in
devices)
	{ [ -f "$STUB_STATE/rebooted" ] || [ "${STUB_START_FASTBOOT:-}" = 1 ]; } && printf '%s fastboot usb:1\n' "$STUB_SERIAL"
	;;
-s)
	[ "$2" = "$STUB_SERIAL" ] || exit 99
	[ "$3" = getvar ] || exit 99
	case "$4" in
	product) value=tegu ;;
	variant) value=${STUB_FASTBOOT_VARIANT:-GXQ96} ;;
	hw-revision) value=MP1.0 ;;
	current-slot) value=a ;;
	slot-count) value=2 ;;
	is-userspace) value=no ;;
	*) exit 99 ;;
	esac
	printf '(bootloader) %s: %s\n' "$4" "$value" >&2
	;;
*) exit 99 ;;
esac
STUB
chmod 0755 "$work/bin/adb" "$work/bin/fastboot"
export PATH="$work/bin:$PATH" STUB_CALLS="$calls" STUB_STATE="$work/state" STUB_SERIAL="$serial" FERRIX_PIXEL9A_FASTBOOT_WAIT_SECONDS=2

run_case() {
	case_name=$1
	shift
	rm -f "$work/state/rebooted"
	: >"$calls"
	status=0
	sh "$script" "$@" >"$work/$case_name.out" 2>"$work/$case_name.err" <"$work/$case_name.in" || status=$?
}

: >"$work/preflight.in"
run_case preflight --preflight
[ "$status" -eq 0 ]
grep -q '^preflight=passed$' "$work/preflight.out"
! grep -q 'reboot' "$calls"

printf '%s\n' 'CONFIRM PIXEL 9A ENTER FASTBOOT' >"$work/enter.in"
export STUB_FASTBOOT_VARIANT=N/A
run_case enter --confirm-enter
[ "$status" -eq 0 ]
grep -q '^result=entered-fastboot$' "$work/enter.out"
grep -q '^adb\t-s\tSYN-FASTBOOT-001\treboot\tbootloader\t$' "$calls"
grep -q '^fastboot\t-s\tSYN-FASTBOOT-001\tgetvar\tproduct\t$' "$calls"
grep -q '^variant=N/A$' "$work/enter.out"
grep -q '^sku_gate=adb-correlated-or-fastboot-unavailable$' "$work/enter.out"
unset STUB_FASTBOOT_VARIANT

printf '%s\n' 'wrong phrase' >"$work/reject.in"
run_case reject --confirm-enter
[ "$status" -eq 1 ]
grep -q 'no reboot performed' "$work/reject.err"
! grep -q 'reboot' "$calls"

: >"$work/already.in"
export STUB_START_FASTBOOT=1
run_case already --confirm-enter
[ "$status" -eq 0 ]
grep -q '^result=already-in-fastboot$' "$work/already.out"
! grep -q 'reboot' "$calls"
unset STUB_START_FASTBOOT

printf '%s\n' '{"event":"pixel-9a-enter-fastboot-test","status":"passed"}'
