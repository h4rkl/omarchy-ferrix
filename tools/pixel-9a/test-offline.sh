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
# Exit status: 0 when every test passed, 1 when any test failed, 2 on usage
# errors. Never contacts a device: each test uses its own adb/fastboot stubs.
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

pass=0
fail=0
skipped=0
failed=""
with_skips=""

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
	else
		fail=$((fail + 1))
		failed="$failed $name"
		printf 'FAIL %s (exit %s, %ss); last lines of %s:\n' "$name" "$status" "$secs" "$log"
		tail -n 20 "$log" | sed 's/^/     /'
	fi
done

total=$((pass + fail))
printf '\nPixel 9a offline tests: %s run, %s passed (%s with skips), %s failed\n' \
	"$total" "$pass" "$skipped" "$fail"
[ -z "$with_skips" ] || printf 'with skips:%s\n' "$with_skips"
[ -z "$failed" ] || printf 'failed:%s\n' "$failed"
printf 'logs: %s\n' "$log_dir"
[ "$fail" -eq 0 ]
