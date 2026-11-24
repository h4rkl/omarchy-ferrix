#!/bin/sh
# usage: apk-bench.sh [LABEL] -- run the fixed APK benchmark on the attached
# Pixel 9a (Ferrix running, CDC-ACM bring-up shell) and save the records to
# out/pixel-9a/bench/<UTC stamp>[-LABEL].jsonl. Host side only pushes
# apk-bench-device.sh, runs it, and fetches the output; see that script for
# what is measured. Every later spec 10 change is judged against these records.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../../.." && pwd)
label=${1:-}
stamp=$(date -u +%Y%m%dT%H%M%SZ)
out_dir="$root/out/pixel-9a/bench"
mkdir -p "$out_dir"
out="$out_dir/$stamp${label:+-$label}.jsonl"
python3 "$here/shell-put.py" "$here/apk-bench-device.sh" /tmp/apk-bench-device.sh >/dev/null
python3 "$here/shell-run.py" 'sh /tmp/apk-bench-device.sh > /tmp/apk-bench.jsonl 2>/tmp/apk-bench.err; echo BENCH_EXIT=$?' 900 |
	grep -q 'BENCH_EXIT=0' || { echo 'STOP: device bench failed; see /tmp/apk-bench.err on the phone' >&2; exit 1; }
python3 "$here/shell-get.py" /tmp/apk-bench.jsonl "$out" >/dev/null
python3 - "$out" <<'EOF'
import json, sys
for line in open(sys.argv[1]):
    line = line.strip()
    if line:
        json.loads(line)  # every record must be valid JSON
print(sys.argv[1])
EOF
