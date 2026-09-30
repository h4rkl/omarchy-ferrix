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
grep -F -- 'fastboot -s "$fastboot_serial" reboot bootloader' "$script" >/dev/null

work=$(mktemp -d "${TMPDIR:-/tmp}/pixel-9a-enter-fastboot.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/bin" "$work/state"
calls="$work/calls.log"
serial='SYN-FASTBOOT-001'
tab=$(printf '\t')

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
	[ "${STUB_EXTRA_FASTBOOT:-}" = 1 ] && printf 'SYN-OTHER fastboot usb:2\n'
	;;
-s)
	[ "$2" = "$STUB_SERIAL" ] || exit 99
	if [ "$3" = reboot ]; then
		[ "$4" = bootloader ] || exit 99
		: >"$STUB_STATE/rebooted"
		exit 0
	fi
	[ "$3" = getvar ] || exit 99
	case "$4" in
	product) value=tegu ;;
	variant)
		if [ "${STUB_START_FASTBOOTD:-}" = 1 ]; then
			if [ -f "$STUB_STATE/rebooted" ]; then value=N/A; else value='MSM USF'; fi
		else
			value=${STUB_FASTBOOT_VARIANT:-GXQ96}
		fi
		;;
	hw-revision) value=MP1.0 ;;
	current-slot) value=a ;;
	slot-count) value=2 ;;
	is-userspace)
		if [ "${STUB_START_FASTBOOTD:-}" = 1 ] && [ ! -f "$STUB_STATE/rebooted" ]; then value=yes; else value=no; fi
		;;
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
grep -Fqx "adb${tab}-s${tab}${serial}${tab}reboot${tab}bootloader${tab}" "$calls"
grep -Fqx "fastboot${tab}-s${tab}${serial}${tab}getvar${tab}product${tab}" "$calls"
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

export STUB_START_FASTBOOT=1 STUB_START_FASTBOOTD=1 FERRIX_OUT_DIR="$work/out"
mkdir -p "$FERRIX_OUT_DIR"
: >"$work/fastbootd-missing.in"
run_case fastbootd-missing --preflight
[ "$status" -eq 1 ]
grep -q 'same-serial Android identity' "$work/fastbootd-missing.err"
! grep -q 'reboot' "$calls"

prefix=$(printf '%s' "$serial" | shasum -a 256 | cut -c1-12)
cat >"$FERRIX_OUT_DIR/discovery-synthetic.txt" <<EOF
transport=adb
result=ok
serial_sha256_prefix=$prefix
ro.product.manufacturer=Google
ro.product.model=Pixel 9a
ro.product.device=tegu
ro.boot.hardware.revision=MP1.0
ro.boot.hardware.sku=GXQ96
EOF
: >"$work/fastbootd-preflight.in"
run_case fastbootd-preflight --preflight
[ "$status" -eq 0 ] || { cat "$work/fastbootd-preflight.err" >&2; exit 1; }
grep -q '^transport=fastbootd$' "$work/fastbootd-preflight.out"
grep -q '^sku_gate=adb-correlated-fastbootd-placeholder$' "$work/fastbootd-preflight.out"
! grep -q 'reboot' "$calls"

export STUB_EXTRA_FASTBOOT=1
: >"$work/fastbootd-ambiguous.in"
run_case fastbootd-ambiguous --preflight
[ "$status" -eq 1 ]
grep -q 'expected exactly one ADB or Fastboot device' "$work/fastbootd-ambiguous.err"
! grep -q 'reboot' "$calls"
unset STUB_EXTRA_FASTBOOT

printf '%s\n' 'wrong phrase' >"$work/fastbootd-reject.in"
run_case fastbootd-reject --confirm-enter
[ "$status" -eq 1 ]
! grep -q 'reboot' "$calls"

printf '%s\n' 'CONFIRM PIXEL 9A EXIT FASTBOOTD' >"$work/fastbootd-enter.in"
run_case fastbootd-enter --confirm-enter
[ "$status" -eq 0 ]
grep -q '^result=entered-fastboot$' "$work/fastbootd-enter.out"
grep -q '^transport=fastboot$' "$work/fastbootd-enter.out"
grep -Fqx "fastboot${tab}-s${tab}${serial}${tab}reboot${tab}bootloader${tab}" "$calls"
[ "$(grep -c 'reboot' "$calls")" -eq 1 ]
unset STUB_START_FASTBOOT STUB_START_FASTBOOTD FERRIX_OUT_DIR

printf '%s\n' '{"event":"pixel-9a-enter-fastboot-test","status":"passed"}'
