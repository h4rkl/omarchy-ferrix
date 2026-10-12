#!/bin/sh
# Run every offline Pixel 9a test under tools/pixel-9a/tests/.
#
#   test-offline.sh [NAME ...]
#
# Discovers tests/*.sh (run with sh) and tests/*.py (run with python3) in
# byte-sorted (LC_ALL=C) order, skipping the sourced/spawned helpers listed in
# $helpers. With NAME arguments (e.g. acm-probe.sh) only those tests run.
# Every test runs even after a failure; each one's output is kept in a log
# directory and the tail of a failing log is printed. A test that exits 0 but
# prints a line starting with "skip " (the tests' own convention for a stage
# whose host prerequisite is absent) is reported as PASS with skips.
#
# On Linux hosts, tests named in tests/quarantine-linux.txt (#127) still run,
# but a failure is reported as QUARANTINED and does not fail the run; a
# quarantined test that passes is flagged for removal from the list. The list
# is ignored on other hosts.
#
# Exit status: 0 when every non-quarantined test passed, 1 when any of them
# failed, 2 on usage errors. Never contacts a device: each test uses its own adb/fastboot stubs.
set -u

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
tests_dir="$root/tools/pixel-9a/tests"

# Not tests: sourced or spawned by the CDC-ACM workflow tests.
helpers="acm-fake-guest.sh acm-fake-guest.py"

is_helper() {
	for h in $helpers; do
		[ "$1" = "$h" ] && return 0
	done
	return 1
}

quarantine_file="$tests_dir/quarantine-linux.txt"
quarantine=""
if [ "$(uname -s)" = Linux ] && [ -f "$quarantine_file" ]; then
	quarantine=$(sed -e 's/#.*//' -e 's/[[:space:]]*$//' -e '/^$/d' "$quarantine_file")
fi

is_quarantined() {
	for q in $quarantine; do
		[ "$1" = "$q" ] && return 0
	done
	return 1
}

log_dir=${FERRIX_PIXEL9A_TEST_LOG_DIR:-}
if [ -z "$log_dir" ]; then
	log_dir=$(mktemp -d "${TMPDIR:-/tmp}/ferrix-pixel-9a-tests.XXXXXX") || exit 2
fi
mkdir -p "$log_dir" || exit 2

list=$(cd "$tests_dir" && LC_ALL=C ls -1 -- *.sh *.py 2>/dev/null | LC_ALL=C sort)
if [ "$#" -gt 0 ]; then
	for name in "$@"; do
		printf '%s\n' "$list" | grep -Fxq -- "$name" || {
			printf 'test-offline: no such test: %s\n' "$name" >&2
			exit 2
		}
	done
	list=$(printf '%s\n' "$@")
fi

for q in $quarantine; do
	[ -f "$tests_dir/$q" ] || printf 'test-offline: warning: quarantine-linux.txt names a missing test: %s\n' "$q" >&2
done

pass=0
fail=0
skipped=0
quarantined=0
failed=""
with_skips=""
quarantined_failed=""
quarantined_passed=""

for name in $list; do
	is_helper "$name" && continue
	log="$log_dir/$name.log"
	case $name in
	*.py) set -- env PYTHONDONTWRITEBYTECODE=1 python3 "$tests_dir/$name" ;;
	*) set -- sh "$tests_dir/$name" ;;
	esac
	start=$(date +%s)
	printf 'RUN  %s\n' "$name"
	"$@" >"$log" 2>&1
	status=$?
	secs=$(($(date +%s) - start))
	if [ "$status" -eq 0 ]; then
		pass=$((pass + 1))
		skips=$(grep -c '^skip ' "$log" 2>/dev/null) || skips=0
		if [ "$skips" -gt 0 ]; then
			skipped=$((skipped + 1))
			with_skips="$with_skips $name"
			printf 'PASS %s (%ss, %s skip line(s))\n' "$name" "$secs" "$skips"
			grep '^skip ' "$log" | sed 's/^/     /'
		else
			printf 'PASS %s (%ss)\n' "$name" "$secs"
		fi
		if is_quarantined "$name"; then
			quarantined_passed="$quarantined_passed $name"
			printf '     %s: quarantined but passed; remove from list\n' "$name"
		fi
	elif is_quarantined "$name"; then
		quarantined=$((quarantined + 1))
		quarantined_failed="$quarantined_failed $name"
		printf 'QUARANTINED (#127) %s (exit %s, %ss); last lines of %s:\n' "$name" "$status" "$secs" "$log"
		tail -n 20 "$log" | sed 's/^/     /'
	else
		fail=$((fail + 1))
		failed="$failed $name"
		printf 'FAIL %s (exit %s, %ss); last lines of %s:\n' "$name" "$status" "$secs" "$log"
		tail -n 20 "$log" | sed 's/^/     /'
	fi
done

total=$((pass + fail + quarantined))
printf '\nPixel 9a offline tests: %s run, %s passed (%s with skips), %s failed, %s quarantined failures (#127)\n' \
	"$total" "$pass" "$skipped" "$fail" "$quarantined"
[ -z "$with_skips" ] || printf 'with skips:%s\n' "$with_skips"
[ -z "$quarantined_failed" ] || printf 'QUARANTINED (#127), not counted as failures:%s\n' "$quarantined_failed"
[ -z "$quarantined_passed" ] || printf 'quarantined but passed; remove from list:%s\n' "$quarantined_passed"
[ -z "$failed" ] || printf 'failed:%s\n' "$failed"
printf 'logs: %s\n' "$log_dir"
[ "$fail" -eq 0 ]
